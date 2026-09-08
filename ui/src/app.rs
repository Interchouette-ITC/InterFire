//! `InterFire` desktop shell: left navigation, tray, alert, Rules, and Log.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use std::time::Duration;

use gpui_kit::component::input::InputState;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use interfire_proto::{ProcessRow, PromptRow, RuleRow};

use crate::alert::{AlertScope, AlertVerdict, ConnectionAlert};
use crate::alert_view::alert_overlay;
use crate::applications_view::{applications_body, try_open_htop};
use crate::audit_host::{AuditEvent, AuditHost};
use crate::ipc_poll;
use crate::log_buf::LogBuffer;
use crate::log_view::log_body;
use crate::proc_sample::{CpuTracker, ProcSample};
use crate::rss_probe::{RssProbeMode, prompt_load_fixture};
use crate::rules::{RuleVerdict, next_rule_id, validate_new_rule};
use crate::rules_view::{add_rule_overlay, rules_body};
use crate::section::Section;
use crate::tray::{DaemonLink, TrayState};
#[cfg(target_os = "linux")]
use crate::tray_host::TrayHost;

const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Latest RAM/CPU samples for the Profiling section.
#[derive(Clone, Debug, Default)]
pub struct ProfilingSnapshot {
    pub ui_pid: u32,
    pub ui_rss_kib: u64,
    pub ui_cpu_pct: Option<f64>,
    pub daemon_pid: Option<u32>,
    pub daemon_rss_kib: Option<u64>,
    pub daemon_cpu_pct: Option<f64>,
}

struct ShellContent<'a> {
    section: Section,
    socket: &'a str,
    tray_state: TrayState,
    link: &'a DaemonLink,
    rules: &'a [RuleRow],
    selected_rule: Option<u64>,
    rules_message: Option<&'a str>,
    adding: bool,
    processes: &'a [ProcessRow],
    selected_process: Option<(u32, u64)>,
    htop_message: Option<&'a str>,
    log: &'a LogBuffer,
    profiling: &'a ProfilingSnapshot,
}

/// Active add-rule form backed by GPUI input states.
pub struct AddRuleFormState {
    pub id: Entity<InputState>,
    pub executable: Entity<InputState>,
    pub port: Entity<InputState>,
    pub verdict: RuleVerdict,
    pub error: Option<String>,
}

/// Root application view for the main window.
pub struct App {
    section: Section,
    socket: String,
    link: DaemonLink,
    tray_state: TrayState,
    prompts: Vec<PromptRow>,
    rules: Vec<RuleRow>,
    selected_rule: Option<u64>,
    rules_message: Option<String>,
    add_form: Option<AddRuleFormState>,
    processes: Vec<ProcessRow>,
    selected_process: Option<(u32, u64)>,
    htop_message: Option<String>,
    alert: Option<ConnectionAlert>,
    log: LogBuffer,
    audit: AuditHost,
    rss_probe: Option<RssProbeMode>,
    profiling: ProfilingSnapshot,
    ui_cpu: CpuTracker,
    daemon_cpu: CpuTracker,
    #[cfg(target_os = "linux")]
    tray: Option<TrayHost>,
}

impl App {
    #[must_use]
    #[hotpath::measure]
    pub fn new(socket: String) -> Self {
        let link = DaemonLink::Down {
            reason: "connecting".into(),
        };
        let tray_state = TrayState::from_link(&link);
        let audit = AuditHost::spawn(socket.clone());
        let mut app = Self {
            section: Section::Rules,
            socket,
            link,
            tray_state,
            prompts: Vec::new(),
            rules: Vec::new(),
            selected_rule: None,
            rules_message: None,
            add_form: None,
            processes: Vec::new(),
            selected_process: None,
            htop_message: None,
            alert: None,
            log: LogBuffer::new(),
            audit,
            rss_probe: None,
            profiling: ProfilingSnapshot::default(),
            ui_cpu: CpuTracker::default(),
            daemon_cpu: CpuTracker::default(),
            #[cfg(target_os = "linux")]
            tray: TrayHost::try_spawn(tray_state),
        };
        app.refresh_profiling(None);
        app
    }

    /// Stage synthetic load for `make memcheck-ui` (`--rss-probe=…`).
    #[hotpath::measure]
    pub fn apply_rss_probe(&mut self, mode: RssProbeMode) {
        self.rss_probe = Some(mode);
        match mode {
            RssProbeMode::Idle => {}
            RssProbeMode::PromptLoad => {
                let fixture = prompt_load_fixture();
                self.prompts = fixture.prompts;
                self.alert = Some(fixture.alert);
                self.log = fixture.log;
                self.section = fixture.section;
                self.tray_state = fixture.tray_state;
                #[cfg(target_os = "linux")]
                if let Some(tray) = &self.tray {
                    tray.set_state(self.tray_state);
                }
            }
        }
    }

    /// Start periodic daemon polls that drive tray, Status, alerts, rules, and Log.
    pub fn start_watchers(cx: &Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                if this
                    .update(cx, |app, cx| {
                        app.refresh_from_daemon();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn refresh_from_daemon(&mut self) {
        self.drain_audit_events();
        let snapshot = ipc_poll::poll_snapshot(&self.socket);
        self.link = snapshot.link.clone();
        let daemon_metrics = match &snapshot.link {
            DaemonLink::Up { status, .. } => {
                match (status.pid, status.rss_kib, status.cpu_jiffies) {
                    (Some(pid), Some(rss_kib), Some(cpu_jiffies)) => {
                        Some((pid, rss_kib, cpu_jiffies))
                    }
                    _ => None,
                }
            }
            DaemonLink::Down { .. } => None,
        };
        self.refresh_profiling(daemon_metrics);
        if self.rss_probe != Some(RssProbeMode::PromptLoad) {
            self.prompts = snapshot.prompts;
            self.rules = snapshot.rules;
            self.processes = snapshot.processes;
            if let Some(id) = self.selected_rule
                && !self.rules.iter().any(|row| row.id == id)
            {
                self.selected_rule = None;
            }
            if let Some(key) = self.selected_process
                && !self
                    .processes
                    .iter()
                    .any(|row| row.pid == key.0 && row.start_ticks == key.1)
            {
                self.selected_process = None;
            }
            self.alert = ConnectionAlert::advance(self.alert.take(), &self.prompts);
            let next = TrayState::from_link(&self.link);
            if next != self.tray_state {
                self.tray_state = next;
                #[cfg(target_os = "linux")]
                if let Some(tray) = &self.tray {
                    tray.set_state(next);
                }
            }
        }
    }

    fn refresh_profiling(&mut self, daemon: Option<(u32, u64, u64)>) {
        let now = std::time::Instant::now();
        if let Ok(sample) = ProcSample::sample_self() {
            let ui_cpu_pct = self.ui_cpu.push(sample.cpu_jiffies, now);
            self.profiling.ui_pid = sample.pid;
            self.profiling.ui_rss_kib = sample.rss_kib;
            self.profiling.ui_cpu_pct = ui_cpu_pct;
        }
        if let Some((pid, rss_kib, cpu_jiffies)) = daemon {
            let daemon_cpu_pct = self.daemon_cpu.push(cpu_jiffies, now);
            self.profiling.daemon_pid = Some(pid);
            self.profiling.daemon_rss_kib = Some(rss_kib);
            self.profiling.daemon_cpu_pct = daemon_cpu_pct;
        } else {
            self.profiling.daemon_pid = None;
            self.profiling.daemon_rss_kib = None;
            self.profiling.daemon_cpu_pct = None;
            self.daemon_cpu = CpuTracker::default();
        }
    }

    fn drain_audit_events(&mut self) {
        for event in self.audit.drain() {
            match event {
                AuditEvent::Ready => self.log.set_subscribed(true),
                AuditEvent::Record(record) => {
                    self.log.push_line(record.sequence, &record.message);
                }
                AuditEvent::Down(_) => self.log.set_subscribed(false),
            }
        }
    }

    pub(crate) fn select(&mut self, section: Section, cx: &mut Context<Self>) {
        self.section = section;
        cx.notify();
    }

    pub(crate) fn select_rule(&mut self, id: u64, cx: &mut Context<Self>) {
        self.selected_rule = Some(id);
        cx.notify();
    }

    pub(crate) fn select_process(&mut self, pid: u32, start_ticks: u64, cx: &mut Context<Self>) {
        self.selected_process = Some((pid, start_ticks));
        self.htop_message = None;
        cx.notify();
    }

    pub(crate) fn open_htop(&mut self, pid: u32, cx: &mut Context<Self>) {
        self.htop_message = Some(match try_open_htop(pid) {
            Ok(()) => format!("opened htop for pid {pid}"),
            Err(message) => message,
        });
        cx.notify();
    }

    pub(crate) fn select_log_row(&mut self, index: usize, cx: &mut Context<Self>) {
        self.log.select(index);
        cx.notify();
    }

    pub(crate) fn set_scope(&mut self, scope: AlertScope, cx: &mut Context<Self>) {
        if let Some(alert) = &mut self.alert {
            alert.scope = scope;
            cx.notify();
        }
    }

    pub(crate) fn toggle_details(&mut self, cx: &mut Context<Self>) {
        if let Some(alert) = &mut self.alert {
            alert.details_open = !alert.details_open;
            cx.notify();
        }
    }

    pub(crate) fn submit_verdict(&mut self, verdict: AlertVerdict, cx: &mut Context<Self>) {
        let Some(frame) = self
            .alert
            .as_ref()
            .and_then(|alert| alert.answer_frame(verdict))
        else {
            return;
        };
        match ipc_poll::send_expect_pong(&self.socket, &frame) {
            Ok(()) => {
                self.alert = None;
                self.refresh_from_daemon();
            }
            Err(message) => {
                if let Some(alert) = &mut self.alert {
                    alert.status_message = Some(message);
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn begin_add_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.add_form.is_some() {
            return;
        }
        let next_id = next_rule_id(&self.rules);
        let id = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(next_id.to_string(), window, cx);
            state
        });
        let executable = cx.new(|cx| InputState::new(window, cx));
        let port = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value("443", window, cx);
            state
        });
        self.add_form = Some(AddRuleFormState {
            id,
            executable,
            port,
            verdict: RuleVerdict::Deny,
            error: None,
        });
        self.rules_message = None;
        cx.notify();
    }

    pub(crate) fn cancel_add_rule(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.add_form = None;
        cx.notify();
    }

    pub(crate) fn set_add_verdict(&mut self, verdict: RuleVerdict, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.add_form {
            form.verdict = verdict;
            cx.notify();
        }
    }

    pub(crate) fn submit_add_rule(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.add_form else {
            return;
        };
        let id = form.id.read(cx).value().to_string();
        let executable = form.executable.read(cx).value().to_string();
        let port = form.port.read(cx).value().to_string();
        let verdict = form.verdict;
        let parsed = match validate_new_rule(&id, &executable, verdict, &port) {
            Ok(rule) => rule,
            Err(message) => {
                if let Some(form) = &mut self.add_form {
                    form.error = Some(message);
                }
                cx.notify();
                return;
            }
        };
        match ipc_poll::add_rule(
            &self.socket,
            parsed.id,
            &parsed.executable,
            &parsed.verdict,
            parsed.port,
        ) {
            Ok(()) => {
                self.add_form = None;
                self.selected_rule = Some(parsed.id);
                self.rules_message = Some(format!("added rule {}", parsed.id));
                self.refresh_from_daemon();
            }
            Err(message) => {
                if let Some(form) = &mut self.add_form {
                    form.error = Some(message);
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn delete_selected_rule(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_rule else {
            return;
        };
        match ipc_poll::delete_rule(&self.socket, id) {
            Ok(()) => {
                self.selected_rule = None;
                self.rules_message = Some(format!("deleted rule {id}"));
                self.refresh_from_daemon();
            }
            Err(message) => {
                self.rules_message = Some(message);
            }
        }
        cx.notify();
    }
}

impl Render for App {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.section;
        let socket = self.socket.clone();
        let tray_state = self.tray_state;
        let link = self.link.clone();
        let alert = self.alert.clone();
        let rules = self.rules.clone();
        let selected_rule = self.selected_rule;
        let rules_message = self.rules_message.clone();
        let adding = self.add_form.is_some();
        let processes = self.processes.clone();
        let selected_process = self.selected_process;
        let htop_message = self.htop_message.clone();
        let log = self.log.clone();
        let profiling = self.profiling.clone();

        let mut shell = div()
            .id("interfire-shell")
            .relative()
            .flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(nav_column(selected, cx))
            .child(content_column(
                &ShellContent {
                    section: selected,
                    socket: &socket,
                    tray_state,
                    link: &link,
                    rules: &rules,
                    selected_rule,
                    rules_message: rules_message.as_deref(),
                    adding,
                    processes: &processes,
                    selected_process,
                    htop_message: htop_message.as_deref(),
                    log: &log,
                    profiling: &profiling,
                },
                cx,
            ));

        if let Some(form) = &self.add_form {
            shell = shell.child(add_rule_overlay(form, cx));
        }
        if let Some(alert) = alert {
            shell = shell.child(alert_overlay(&alert, cx));
        }
        shell
    }
}

fn nav_column(selected: Section, cx: &Context<App>) -> impl IntoElement {
    let mut column = div()
        .id("nav")
        .w(px(180.))
        .h_full()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .border_r_1()
        .border_color(cx.theme().border)
        .child(div().text_sm().font_semibold().mb_2().child("InterFire"));

    for section in Section::ALL {
        let is_selected = section == selected;
        column = column.child(nav_button(section, is_selected, cx));
    }

    column
}

fn nav_button(section: Section, selected: bool, cx: &Context<App>) -> impl IntoElement {
    let label = section.label();
    div()
        .id(ElementId::Name(label.into()))
        .px_2()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .when(selected, |this| {
            this.bg(cx.theme().accent)
                .text_color(cx.theme().accent_foreground)
        })
        .when(!selected, |this| {
            this.hover(|style| style.bg(cx.theme().accent.opacity(0.15)))
        })
        .on_click(cx.listener(move |app, _, _, cx| app.select(section, cx)))
        .child(label)
}

fn content_column(content: &ShellContent<'_>, cx: &Context<App>) -> impl IntoElement {
    div()
        .id("content")
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .p_4()
        .gap_3()
        .child(
            div()
                .text_lg()
                .font_semibold()
                .child(content.section.label()),
        )
        .child(section_body(content, cx))
        .child(
            div()
                .mt_auto()
                .pt_2()
                .border_t_1()
                .border_color(cx.theme().border)
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!(
                    "{}  |  tray: {}  |  {}",
                    content.section.label(),
                    content.tray_state.label(),
                    content.socket
                )),
        )
}

fn section_body(content: &ShellContent<'_>, cx: &Context<App>) -> Div {
    let muted = cx.theme().muted_foreground;
    match content.section {
        Section::Rules => rules_body(
            content.rules,
            content.selected_rule,
            content.rules_message,
            content.adding,
            cx,
        ),
        Section::Status => status_body(content.socket, content.tray_state, content.link, muted),
        Section::Applications => applications_body(
            content.processes,
            content.selected_process,
            content.htop_message,
            cx,
        ),
        Section::Log => log_body(content.log, cx),
        Section::Network => div()
            .text_color(muted)
            .child("InterFire-owned nftables controls are not available yet."),
        Section::Profiling => profiling_body(content.profiling, muted),
        Section::Settings => div()
            .v_flex()
            .gap_2()
            .child("Socket path, diagnostics, reconnect.")
            .child(
                div()
                    .text_color(muted)
                    .child(format!("socket = {}", content.socket)),
            )
            .child(
                div()
                    .text_color(muted)
                    .child(format!("tray = {}", content.tray_state.label())),
            ),
    }
}

fn profiling_body(snap: &ProfilingSnapshot, muted: Hsla) -> Div {
    let ui_cpu = snap
        .ui_cpu_pct
        .map_or_else(|| "…".to_owned(), |pct| format!("{pct:.1}%"));
    let daemon_rss = snap
        .daemon_rss_kib
        .map_or_else(|| "unavailable".to_owned(), format_mib);
    let daemon_cpu = snap.daemon_cpu_pct.map_or_else(
        || {
            if snap.daemon_pid.is_some() {
                "…".to_owned()
            } else {
                "unavailable".to_owned()
            }
        },
        |pct| format!("{pct:.1}%"),
    );
    let daemon_pid = snap
        .daemon_pid
        .map_or_else(|| "-".to_owned(), |pid| pid.to_string());

    div()
        .v_flex()
        .gap_3()
        .child(div().font_semibold().child("Profiling"))
        .child(
            div()
                .text_color(muted)
                .child("Live VmRSS and CPU from /proc (UI) and daemon status IPC. GPUI idle is often ~190 MiB; the daemon stays far smaller."),
        )
        .child(div().font_semibold().child("interfired (firewall)"))
        .child(div().child(format!("pid = {daemon_pid}")))
        .child(div().child(format!("RSS = {daemon_rss}")))
        .child(div().child(format!("CPU = {daemon_cpu}")))
        .child(div().font_semibold().mt_2().child("interfire-ui (GPUI)"))
        .child(div().child(format!("pid = {}", snap.ui_pid)))
        .child(div().child(format!("RSS = {}", format_mib(snap.ui_rss_kib))))
        .child(div().child(format!("CPU = {ui_cpu}")))
        .child(
            div()
                .text_color(muted)
                .mt_2()
                .child("CPU % uses consecutive 1s polls (USER_HZ=100). Release gates: make memcheck / make memcheck-ui."),
        )
}

fn format_mib(kib: u64) -> String {
    format!("{} MiB ({} KiB)", kib / 1024, kib)
}

fn status_body(socket: &str, tray_state: TrayState, link: &DaemonLink, muted: Hsla) -> Div {
    let body = div()
        .v_flex()
        .gap_2()
        .child(format!("Tray: {}", tray_state.label()))
        .child(div().text_color(muted).child(tray_state.guidance()))
        .child(div().text_color(muted).child(format!("socket = {socket}")));

    match link {
        DaemonLink::Down { reason } => body.child(
            div()
                .text_color(muted)
                .child(format!("last error: {reason}")),
        ),
        DaemonLink::Up {
            status,
            pending_prompts,
        } => body
            .child(div().text_color(muted).child(format!(
                "enforcement={}  observation={}  ipc={}",
                status.enforcement, status.observation, status.ipc_version
            )))
            .child(
                div()
                    .text_color(muted)
                    .child(format!("pending prompts: {pending_prompts}")),
            ),
    }
}
