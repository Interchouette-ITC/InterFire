//! `InterFire` desktop shell: left navigation, tray, alert, Rules, and Log.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use std::time::Duration;

use gpui_kit::component::input::InputState;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use interfire_proto::{NetworkStatus, ProcessRow, PromptRow, RuleRow};

use crate::alert::{AlertScope, AlertVerdict, ConnectionAlert};
use crate::alert_view::alert_overlay;
use crate::applications_view::{ProcessViewer, applications_body, try_open_viewer};
use crate::audit_host::{AuditEvent, AuditHost};
use crate::brand;
use crate::ipc_poll;
use crate::log_buf::LogBuffer;
use crate::log_view::log_body;
use crate::network_view::network_body;
use crate::proc_sample::{CpuTracker, ProcSample};
use crate::rss_probe::{RssProbeMode, prompt_load_fixture};
use crate::rules::{RuleVerdict, next_rule_id, validate_new_rule};
use crate::rules_view::{add_rule_overlay, rules_body};
use crate::section::Section;
use crate::theme::{self, ChromeMode, ChromePreference};
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
    viewer_message: Option<&'a str>,
    network: Option<&'a NetworkStatus>,
    network_message: Option<&'a str>,
    log: &'a LogBuffer,
    profiling: &'a ProfilingSnapshot,
    chrome_pref: ChromePreference,
    chrome_mode: ChromeMode,
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
    viewer_message: Option<String>,
    network: Option<NetworkStatus>,
    network_message: Option<String>,
    alert: Option<ConnectionAlert>,
    log: LogBuffer,
    audit: AuditHost,
    rss_probe: Option<RssProbeMode>,
    profiling: ProfilingSnapshot,
    ui_cpu: CpuTracker,
    daemon_cpu: CpuTracker,
    chrome_pref: ChromePreference,
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
            viewer_message: None,
            network: None,
            network_message: None,
            alert: None,
            log: LogBuffer::new(),
            audit,
            rss_probe: None,
            profiling: ProfilingSnapshot::default(),
            ui_cpu: CpuTracker::default(),
            daemon_cpu: CpuTracker::default(),
            chrome_pref: ChromePreference::System,
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
            self.network = snapshot.network;
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

    pub(crate) fn set_chrome_preference(
        &mut self,
        preference: ChromePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.chrome_pref = preference;
        let mode = preference.resolve(window.appearance());
        theme::apply_phoenix_theme(mode, Some(window), cx);
        cx.notify();
    }

    /// Current theme preference (Settings switcher).
    #[must_use]
    pub const fn chrome_preference(&self) -> ChromePreference {
        self.chrome_pref
    }

    pub(crate) fn select_rule(&mut self, id: u64, cx: &mut Context<Self>) {
        self.selected_rule = Some(id);
        cx.notify();
    }

    pub(crate) fn select_process(&mut self, pid: u32, start_ticks: u64, cx: &mut Context<Self>) {
        self.selected_process = Some((pid, start_ticks));
        self.viewer_message = None;
        cx.notify();
    }

    pub(crate) fn open_process_viewer(
        &mut self,
        viewer: ProcessViewer,
        pid: u32,
        cx: &mut Context<Self>,
    ) {
        self.viewer_message = Some(match try_open_viewer(viewer, pid) {
            Ok(()) => format!("opened {} for pid {pid}", viewer.label()),
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

    pub(crate) fn install_network_table(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        match ipc_poll::install_network(&self.socket) {
            Ok(()) => {
                self.network_message = Some("installed InterFire nftables table".into());
                self.refresh_from_daemon();
            }
            Err(message) => {
                self.network_message = Some(message);
            }
        }
        cx.notify();
    }

    pub(crate) fn remove_network_table(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        match ipc_poll::remove_network(&self.socket) {
            Ok(()) => {
                self.network_message = Some("removed InterFire nftables table".into());
                self.refresh_from_daemon();
            }
            Err(message) => {
                self.network_message = Some(message);
            }
        }
        cx.notify();
    }

    pub(crate) fn refresh_network_status(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.network_message = None;
        self.refresh_from_daemon();
        cx.notify();
    }
}

impl Render for App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let viewer_message = self.viewer_message.clone();
        let network = self.network.clone();
        let network_message = self.network_message.clone();
        let log = self.log.clone();
        let profiling = self.profiling.clone();
        let chrome_pref = self.chrome_pref;
        let chrome_mode = chrome_pref.resolve(window.appearance());

        let mut shell = div()
            .id("interfire-shell")
            .relative()
            .flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(nav_column(selected, chrome_mode, cx))
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
                    viewer_message: viewer_message.as_deref(),
                    network: network.as_ref(),
                    network_message: network_message.as_deref(),
                    log: &log,
                    profiling: &profiling,
                    chrome_pref,
                    chrome_mode,
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

fn nav_column(selected: Section, chrome_mode: ChromeMode, cx: &Context<App>) -> impl IntoElement {
    let mut column = div()
        .id("nav")
        .w(px(196.))
        .h_full()
        .flex()
        .flex_col()
        .gap_1()
        .px_3()
        .py_3()
        .bg(cx.theme().sidebar)
        .border_r_1()
        .border_color(cx.theme().sidebar_border)
        .child(nav_brand(chrome_mode, cx));

    for section in Section::ALL {
        let is_selected = section == selected;
        column = column.child(nav_button(section, is_selected, cx));
    }

    column.child(nav_footer(cx))
}

fn nav_brand(chrome_mode: ChromeMode, cx: &Context<App>) -> impl IntoElement {
    div()
        .id("nav-brand")
        .flex()
        .items_center()
        .gap_2()
        .mb_3()
        .px_1()
        .child(
            img(brand::nav_mark_source(chrome_mode))
                .id("nav-mark")
                .w(px(36.))
                .h(px(36.))
                .rounded_md()
                .object_fit(ObjectFit::Contain),
        )
        .child(
            div()
                .v_flex()
                .gap_0()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(cx.theme().sidebar_foreground)
                        .child("InterFire"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("FIREWALL"),
                ),
        )
}

fn nav_footer(cx: &Context<App>) -> impl IntoElement {
    div()
        .id("nav-footer")
        .mt_auto()
        .pt_3()
        .border_t_1()
        .border_color(cx.theme().sidebar_border)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child("SECURE · CONTROL")
}

fn nav_button(section: Section, selected: bool, cx: &Context<App>) -> impl IntoElement {
    let label = section.label();
    div()
        .id(ElementId::Name(label.into()))
        .px_3()
        .py_2()
        .rounded_md()
        .cursor_pointer()
        .text_sm()
        .when(selected, |this| {
            this.bg(cx.theme().sidebar_accent)
                .text_color(cx.theme().sidebar_accent_foreground)
                .font_semibold()
        })
        .when(!selected, |this| {
            this.text_color(cx.theme().sidebar_foreground)
                .hover(|style| {
                    style
                        .bg(cx.theme().sidebar_accent.opacity(0.18))
                        .text_color(cx.theme().foreground)
                })
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
        .bg(cx.theme().background)
        .p_4()
        .gap_3()
        .child(content_header(content, cx))
        .child(
            div()
                .id("content-panel")
                .flex_1()
                .v_flex()
                .gap_3()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().group_box)
                .child(section_body(content, cx)),
        )
        .child(status_bar(content, cx))
}

fn content_header(content: &ShellContent<'_>, cx: &Context<App>) -> impl IntoElement {
    div()
        .id("content-header")
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_lg()
                .font_semibold()
                .text_color(cx.theme().foreground)
                .child(content.section.label()),
        )
        .child(tray_chip(content.tray_state, cx))
}

fn tray_chip(state: TrayState, cx: &Context<App>) -> impl IntoElement {
    let (fill, label_color) = match state {
        TrayState::Protected => (cx.theme().success.opacity(0.2), cx.theme().success),
        TrayState::Prompting => (cx.theme().accent.opacity(0.25), cx.theme().accent),
        TrayState::Degraded => (cx.theme().warning.opacity(0.22), cx.theme().warning),
        TrayState::Unavailable => (cx.theme().danger.opacity(0.22), cx.theme().danger),
    };
    div()
        .id("tray-chip")
        .px_2()
        .py_1()
        .rounded_md()
        .bg(fill)
        .text_xs()
        .font_semibold()
        .text_color(label_color)
        .child(state.label().to_uppercase())
}

fn status_bar(content: &ShellContent<'_>, cx: &Context<App>) -> impl IntoElement {
    div()
        .id("status-bar")
        .mt_auto()
        .pt_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(format!(
            "{}  ·  tray {}  ·  {}",
            content.section.label(),
            content.tray_state.label(),
            content.socket
        ))
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
        Section::Status => status_body(content.socket, content.tray_state, content.link, muted, cx),
        Section::Applications => applications_body(
            content.processes,
            content.selected_process,
            content.viewer_message,
            cx,
        ),
        Section::Log => log_body(content.log, cx),
        Section::Network => {
            network_body(content.link, content.network, content.network_message, cx)
        }
        Section::Profiling => profiling_body(content.profiling, muted),
        Section::Settings => settings_body(
            content.socket,
            content.tray_state,
            content.chrome_pref,
            content.chrome_mode,
            muted,
            cx,
        ),
    }
}

fn settings_body(
    socket: &str,
    tray_state: TrayState,
    chrome_pref: ChromePreference,
    chrome_mode: ChromeMode,
    muted: Hsla,
    cx: &Context<App>,
) -> Div {
    div()
        .v_flex()
        .gap_3()
        .child(
            img(brand::logo_horizontal_source())
                .id("settings-logo")
                .w(px(220.))
                .h(px(82.))
                .object_fit(ObjectFit::Contain),
        )
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child("FIREWALL · SECURE · CONTROL"),
        )
        .child(settings_row("Socket", socket, muted, cx))
        .child(settings_row("Tray", tray_state.label(), muted, cx))
        .child(
            div()
                .v_flex()
                .gap_2()
                .px_3()
                .py_2()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .child(div().text_sm().font_semibold().child("Theme"))
                .child(div().text_xs().text_color(muted).child(format!(
                    "Appearance: {} (phoenix orange brand)",
                    chrome_mode.label()
                )))
                .child(theme_switcher(chrome_pref, cx)),
        )
        .child(div().text_color(muted).text_xs().child(
            "Diagnostics and reconnect live on Status. Packaging lands with the install slice.",
        ))
}

fn theme_switcher(selected: ChromePreference, cx: &Context<App>) -> impl IntoElement {
    let mut row = div().id("theme-switcher").flex().gap_2();
    for preference in ChromePreference::ALL {
        let is_selected = preference == selected;
        let label = preference.label();
        row = row.child(
            div()
                .id(ElementId::Name(format!("theme-{label}").into()))
                .px_3()
                .py_1()
                .rounded_md()
                .border_1()
                .cursor_pointer()
                .when(is_selected, |this| {
                    this.bg(cx.theme().accent)
                        .text_color(cx.theme().accent_foreground)
                        .border_color(cx.theme().accent)
                        .font_semibold()
                })
                .when(!is_selected, |this| {
                    this.border_color(cx.theme().border)
                        .hover(|style| style.bg(cx.theme().accent.opacity(0.15)))
                })
                .on_click(cx.listener(move |app, _, window, cx| {
                    app.set_chrome_preference(preference, window, cx);
                }))
                .child(label),
        );
    }
    row
}

fn settings_row(label: &str, value: &str, muted: Hsla, cx: &Context<App>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().popover)
        .child(div().text_sm().font_semibold().child(label.to_owned()))
        .child(div().text_sm().text_color(muted).child(value.to_owned()))
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

fn status_body(
    socket: &str,
    tray_state: TrayState,
    link: &DaemonLink,
    muted: Hsla,
    cx: &Context<App>,
) -> Div {
    let body = div()
        .v_flex()
        .gap_3()
        .child(
            div()
                .font_semibold()
                .child(format!("Tray: {}", tray_state.label())),
        )
        .child(div().text_color(muted).child(tray_state.guidance()))
        .child(div().text_color(muted).child(format!("socket = {socket}")));

    match link {
        DaemonLink::Down { reason } => body.child(
            div()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(cx.theme().danger.opacity(0.18))
                .text_color(cx.theme().danger)
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
