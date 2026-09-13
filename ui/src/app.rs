//! `InterFire` desktop shell: network statistics chrome, tray, alert, rules.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use std::time::Duration;

use gpui_kit::component::input::InputState;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use interfire_proto::{NetworkStatus, ProcessRow, PromptRow, RuleRow, StatsRow, StatsSummary};

use crate::alert::{AlertScope, AlertVerdict, ConnectionAlert};
use crate::alert_view::alert_overlay;
use crate::applications_view::{ProcessViewer, applications_body, try_open_viewer};
use crate::audit_host::{AuditEvent, AuditHost};
use crate::brand;
use crate::confirm_queue::ConfirmKind;
use crate::filter::{ListFilter, ResultLimit, VerdictFilter};
use crate::ipc_poll;
use crate::log_buf::LogBuffer;
use crate::network_view::network_body;
use crate::proc_sample::{CpuTracker, ProcSample};
use crate::rss_probe::{RssProbeMode, prompt_load_fixture};
use crate::rules::{RuleVerdict, next_rule_id, validate_new_rule};
use crate::rules_view::{add_rule_overlay, rules_body};
use crate::section::Section;
use crate::service;
use crate::shell_chrome::{about_overlay, menu_row, tabs_row, toolbar_row};
use crate::stats_view::{
    applications_stats_note, daemon_body, events_body, stats_footer, stats_table_body,
};
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
    traffic_scope_machine: bool,
    traffic_direction: &'static str,
    filter: &'a ListFilter,
    filter_input: Option<&'a Entity<InputState>>,
    stats_summary: Option<&'a StatsSummary>,
    stats_hosts: &'a [StatsRow],
    stats_procs: &'a [StatsRow],
    stats_addrs: &'a [StatsRow],
    stats_ports: &'a [StatsRow],
    stats_users: &'a [StatsRow],
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
    /// Traffic panel: `true` = machine scope (needs pkexec).
    traffic_scope_machine: bool,
    /// Traffic panel direction: `out` | `in` | `all`.
    traffic_direction: &'static str,
    /// Pending operator confirmation (header or tray).
    fw_confirm: Option<ConfirmKind>,
    filter: ListFilter,
    filter_input: Option<Entity<InputState>>,
    menu_open: bool,
    about_open: bool,
    stats_summary: Option<StatsSummary>,
    stats_hosts: Vec<StatsRow>,
    stats_procs: Vec<StatsRow>,
    stats_addrs: Vec<StatsRow>,
    stats_ports: Vec<StatsRow>,
    stats_users: Vec<StatsRow>,
    #[cfg(target_os = "linux")]
    tray: Option<TrayHost>,
}

impl App {
    #[must_use]
    #[hotpath::measure]
    pub fn new(socket: String, #[cfg(target_os = "linux")] tray: Option<TrayHost>) -> Self {
        let link = DaemonLink::Down {
            reason: "connecting".into(),
        };
        let tray_state = TrayState::from_link(&link);
        let audit = AuditHost::spawn(socket.clone());
        let mut app = Self {
            section: Section::Events,
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
            traffic_scope_machine: false,
            traffic_direction: "out",
            fw_confirm: None,
            filter: ListFilter::default(),
            filter_input: None,
            menu_open: false,
            about_open: false,
            stats_summary: None,
            stats_hosts: Vec::new(),
            stats_procs: Vec::new(),
            stats_addrs: Vec::new(),
            stats_ports: Vec::new(),
            stats_users: Vec::new(),
            #[cfg(target_os = "linux")]
            tray,
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
        if let Some(kind) = crate::confirm_queue::take() {
            if kind == ConfirmKind::OpenTraffic {
                self.section = Section::Traffic;
            } else {
                self.fw_confirm = Some(kind);
            }
        }
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
            self.stats_summary = snapshot.stats_summary;
            self.stats_hosts = snapshot.stats_hosts;
            self.stats_procs = snapshot.stats_procs;
            self.stats_addrs = snapshot.stats_addrs;
            self.stats_ports = snapshot.stats_ports;
            self.stats_users = snapshot.stats_users;
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
        self.menu_open = false;
        cx.notify();
    }

    /// Create the shared filter input once the window exists.
    pub fn attach_filter_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.filter_input.is_some() {
            return;
        }
        self.filter_input = Some(cx.new(|cx| InputState::new(window, cx)));
    }

    pub(crate) fn toggle_app_menu(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.menu_open = !self.menu_open;
        cx.notify();
    }

    pub(crate) fn open_preferences(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.section = Section::Preferences;
        self.menu_open = false;
        cx.notify();
    }

    pub(crate) fn open_about(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.about_open = true;
        self.menu_open = false;
        cx.notify();
    }

    pub(crate) fn close_about(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.about_open = false;
        cx.notify();
    }

    pub(crate) fn quit_app(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.menu_open = false;
        cx.quit();
    }

    pub(crate) fn open_network(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.section = Section::Network;
        self.menu_open = false;
        cx.notify();
    }

    pub(crate) fn open_profiling(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.section = Section::Profiling;
        self.menu_open = false;
        cx.notify();
    }

    pub(crate) fn set_filter_verdict_all(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.filter.verdict = VerdictFilter::All;
        cx.notify();
    }

    pub(crate) fn set_filter_verdict_allow(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.filter.verdict = VerdictFilter::Allow;
        cx.notify();
    }

    pub(crate) fn set_filter_verdict_deny(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.filter.verdict = VerdictFilter::Deny;
        cx.notify();
    }

    pub(crate) fn set_filter_verdict_prompt(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.filter.verdict = VerdictFilter::Prompt;
        cx.notify();
    }

    pub(crate) fn set_filter_limit_50(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.filter.limit = ResultLimit::Preset(50);
        cx.notify();
    }

    pub(crate) fn set_filter_limit_100(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.filter.limit = ResultLimit::Preset(100);
        cx.notify();
    }

    pub(crate) fn set_filter_limit_200(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.filter.limit = ResultLimit::Preset(200);
        cx.notify();
    }

    pub(crate) fn set_filter_limit_300(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.filter.limit = ResultLimit::Preset(300);
        cx.notify();
    }

    pub(crate) fn set_filter_limit_all(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.filter.limit = ResultLimit::All;
        cx.notify();
    }

    pub(crate) fn set_filter_limit_custom(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.filter.limit = ResultLimit::Custom(2_000);
        cx.notify();
    }

    pub(crate) fn clear_list_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter.clear();
        if let Some(input) = &self.filter_input {
            input.update(cx, |state, cx| {
                state.set_value("", window, cx);
            });
        }
        cx.notify();
    }

    fn filter_text_from_input(&self, cx: &Context<Self>) -> String {
        self.filter_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default()
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

    pub(crate) fn request_pause_confirm(&mut self, cx: &mut Context<Self>) {
        self.fw_confirm = Some(ConfirmKind::RulesPause);
        cx.notify();
    }

    pub(crate) fn request_resume_confirm(&mut self, cx: &mut Context<Self>) {
        self.fw_confirm = Some(ConfirmKind::RulesResume);
        cx.notify();
    }

    pub(crate) fn request_pause_confirm_click(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_pause_confirm(cx);
    }

    pub(crate) fn request_resume_confirm_click(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_resume_confirm(cx);
    }

    pub(crate) fn open_traffic_tab(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.section = Section::Traffic;
        cx.notify();
    }

    pub(crate) fn set_traffic_scope_user(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.traffic_scope_machine = false;
        cx.notify();
    }

    pub(crate) fn set_traffic_scope_machine(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.traffic_scope_machine = true;
        cx.notify();
    }

    pub(crate) fn set_traffic_direction_out(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.traffic_direction = "out";
        cx.notify();
    }

    pub(crate) fn set_traffic_direction_in(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.traffic_direction = "in";
        cx.notify();
    }

    pub(crate) fn set_traffic_direction_all(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.traffic_direction = "all";
        cx.notify();
    }

    pub(crate) fn request_panel_traffic_block(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fw_confirm = Some(ConfirmKind::TrafficBlock);
        cx.notify();
    }

    pub(crate) fn request_panel_traffic_unblock(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fw_confirm = Some(ConfirmKind::TrafficUnblock);
        cx.notify();
    }

    pub(crate) fn request_daemon_stop_click(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fw_confirm = Some(ConfirmKind::DaemonStop);
        cx.notify();
    }

    pub(crate) fn request_daemon_start_click(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fw_confirm = Some(ConfirmKind::DaemonStart);
        cx.notify();
    }

    pub(crate) fn cancel_fw_confirm(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.fw_confirm = None;
        cx.notify();
    }

    pub(crate) fn confirm_fw_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = self.fw_confirm.take();
        match kind {
            Some(ConfirmKind::RulesPause) => self.pause_firewall(window, cx),
            Some(ConfirmKind::RulesResume) => self.resume_firewall(window, cx),
            Some(ConfirmKind::TrafficBlock) => self.block_traffic(window, cx),
            Some(ConfirmKind::TrafficUnblock) => self.unblock_traffic(window, cx),
            Some(ConfirmKind::DaemonStop) => self.stop_daemon(window, cx),
            Some(ConfirmKind::DaemonStart) => self.start_daemon(window, cx),
            Some(ConfirmKind::OpenTraffic) | None => cx.notify(),
        }
    }

    pub(crate) fn pause_firewall(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        match ipc_poll::pause_firewall(&self.socket) {
            Ok(()) => {
                self.network_message = Some("rules paused".into());
                self.refresh_from_daemon();
            }
            Err(message) => {
                self.network_message = Some(message);
            }
        }
        cx.notify();
    }

    pub(crate) fn resume_firewall(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        match ipc_poll::resume_firewall(&self.socket) {
            Ok(()) => {
                self.network_message =
                    Some("rules started (stop other queue firewalls first if present)".into());
                self.refresh_from_daemon();
            }
            Err(message) => {
                self.network_message = Some(message);
            }
        }
        cx.notify();
    }

    pub(crate) fn block_traffic(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let direction = self.traffic_direction;
        let result = if self.traffic_scope_machine {
            service::traffic_block_machine(&self.socket, direction)
        } else {
            ipc_poll::traffic_block(&self.socket, "user", direction)
        };
        match result {
            Ok(()) => {
                self.network_message = Some(format!(
                    "traffic blocked ({})",
                    if self.traffic_scope_machine {
                        "machine"
                    } else {
                        "user"
                    }
                ));
                self.refresh_from_daemon();
            }
            Err(message) => {
                self.network_message = Some(message);
            }
        }
        cx.notify();
    }

    pub(crate) fn unblock_traffic(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let result = if self.traffic_scope_machine {
            service::traffic_unblock_machine(&self.socket)
        } else {
            ipc_poll::traffic_unblock(&self.socket, "user")
        };
        match result {
            Ok(()) => {
                self.network_message = Some("traffic unblocked".into());
                self.refresh_from_daemon();
            }
            Err(message) => {
                self.network_message = Some(message);
            }
        }
        cx.notify();
    }

    pub(crate) fn stop_daemon(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        match crate::service::stop_daemon() {
            Ok(()) => {
                self.network_message =
                    Some("daemon stop requested (Traffic Block may remain in kernel)".into());
                self.refresh_from_daemon();
            }
            Err(message) => {
                self.network_message = Some(message);
            }
        }
        cx.notify();
    }

    pub(crate) fn start_daemon(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        match crate::service::start_daemon() {
            Ok(()) => {
                self.network_message = Some("daemon start requested".into());
                self.refresh_from_daemon();
            }
            Err(message) => {
                self.network_message = Some(message);
            }
        }
        cx.notify();
    }
}

impl Render for App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.section;
        let chrome_mode = self.chrome_pref.resolve(window.appearance());
        let menu_open = self.menu_open;
        let about_open = self.about_open;
        let snapshot = RenderSnapshot::capture(self, window, cx);
        let content = snapshot.content(self.traffic_scope_machine, self.traffic_direction);
        let mut shell = div()
            .id("interfire-shell")
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .p_3()
            .gap_2()
            .child(toolbar_row(
                snapshot.tray_state,
                &snapshot.link,
                chrome_mode,
                menu_open,
                cx,
            ));
        if menu_open {
            shell = shell.child(menu_row(cx));
        }
        shell = shell
            .child(tabs_row(selected, cx))
            .child(content_panel(&content, cx))
            .child(stats_footer(
                snapshot.stats_summary.as_ref(),
                snapshot.rules.len(),
                cx,
            ));
        if let Some(form) = &self.add_form {
            shell = shell.child(add_rule_overlay(form, cx));
        }
        if let Some(alert) = snapshot.alert {
            shell = shell.child(alert_overlay(&alert, cx));
        }
        if let Some(kind) = self.fw_confirm {
            shell = shell.child(fw_confirm_overlay(kind, cx));
        }
        if about_open {
            shell = shell.child(about_overlay(cx));
        }
        shell
    }
}

struct RenderSnapshot {
    socket: String,
    tray_state: TrayState,
    link: DaemonLink,
    alert: Option<ConnectionAlert>,
    rules: Vec<RuleRow>,
    selected_rule: Option<u64>,
    rules_message: Option<String>,
    adding: bool,
    processes: Vec<ProcessRow>,
    selected_process: Option<(u32, u64)>,
    viewer_message: Option<String>,
    network: Option<NetworkStatus>,
    network_message: Option<String>,
    log: LogBuffer,
    profiling: ProfilingSnapshot,
    chrome_pref: ChromePreference,
    chrome_mode: ChromeMode,
    filter: ListFilter,
    filter_input: Option<Entity<InputState>>,
    stats_summary: Option<StatsSummary>,
    stats_hosts: Vec<StatsRow>,
    stats_procs: Vec<StatsRow>,
    stats_addrs: Vec<StatsRow>,
    stats_ports: Vec<StatsRow>,
    stats_users: Vec<StatsRow>,
    section: Section,
}

impl RenderSnapshot {
    fn capture(app: &App, window: &Window, cx: &Context<App>) -> Self {
        let mut filter = app.filter.clone();
        filter.text = app.filter_text_from_input(cx);
        let chrome_mode = app.chrome_pref.resolve(window.appearance());
        Self {
            socket: app.socket.clone(),
            tray_state: app.tray_state,
            link: app.link.clone(),
            alert: app.alert.clone(),
            rules: app.rules.clone(),
            selected_rule: app.selected_rule,
            rules_message: app.rules_message.clone(),
            adding: app.add_form.is_some(),
            processes: app.processes.clone(),
            selected_process: app.selected_process,
            viewer_message: app.viewer_message.clone(),
            network: app.network.clone(),
            network_message: app.network_message.clone(),
            log: app.log.clone(),
            profiling: app.profiling.clone(),
            chrome_pref: app.chrome_pref,
            chrome_mode,
            filter,
            filter_input: app.filter_input.clone(),
            stats_summary: app.stats_summary.clone(),
            stats_hosts: app.stats_hosts.clone(),
            stats_procs: app.stats_procs.clone(),
            stats_addrs: app.stats_addrs.clone(),
            stats_ports: app.stats_ports.clone(),
            stats_users: app.stats_users.clone(),
            section: app.section,
        }
    }

    fn content<'a>(
        &'a self,
        traffic_scope_machine: bool,
        traffic_direction: &'static str,
    ) -> ShellContent<'a> {
        ShellContent {
            section: self.section,
            socket: &self.socket,
            tray_state: self.tray_state,
            link: &self.link,
            rules: &self.rules,
            selected_rule: self.selected_rule,
            rules_message: self.rules_message.as_deref(),
            adding: self.adding,
            processes: &self.processes,
            selected_process: self.selected_process,
            viewer_message: self.viewer_message.as_deref(),
            network: self.network.as_ref(),
            network_message: self.network_message.as_deref(),
            log: &self.log,
            profiling: &self.profiling,
            chrome_pref: self.chrome_pref,
            chrome_mode: self.chrome_mode,
            traffic_scope_machine,
            traffic_direction,
            filter: &self.filter,
            filter_input: self.filter_input.as_ref(),
            stats_summary: self.stats_summary.as_ref(),
            stats_hosts: &self.stats_hosts,
            stats_procs: &self.stats_procs,
            stats_addrs: &self.stats_addrs,
            stats_ports: &self.stats_ports,
            stats_users: &self.stats_users,
        }
    }
}

fn content_panel(content: &ShellContent<'_>, cx: &Context<App>) -> impl IntoElement {
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
        .child(section_body(content, cx))
}

fn fw_confirm_overlay(kind: ConfirmKind, cx: &Context<App>) -> impl IntoElement {
    let (title, body, ok) = match kind {
        ConfirmKind::RulesPause => (
            "Pause rules?",
            "Temporary pause removes the InterFire queue table. New outbound TCP is no longer filtered until you Start rules again.",
            "Pause",
        ),
        ConfirmKind::RulesResume => (
            "Start rules?",
            "Start installs the InterFire queue table. Do not Start while another application-firewall queue is already active.",
            "Start",
        ),
        ConfirmKind::TrafficBlock => (
            "Block traffic?",
            "Blocks new TCP for the selected scope and direction except localhost. Existing connections may continue. Machine scope asks for polkit.",
            "Block",
        ),
        ConfirmKind::TrafficUnblock => (
            "Unblock traffic?",
            "Clears the selected scope kill-switch. Machine overrides user without clearing stored user state.",
            "Unblock",
        ),
        ConfirmKind::DaemonStop => (
            "Stop daemon?",
            "Stops interfired (polkit). IPC and prompts end. A Traffic Block may remain in the kernel until Unblock.",
            "Stop",
        ),
        ConfirmKind::DaemonStart => (
            "Start daemon?",
            "Starts interfired via polkit so the UI can reconnect to the socket.",
            "Start",
        ),
        ConfirmKind::OpenTraffic => ("", "", ""),
    };
    div()
        .id("fw-confirm-overlay")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(cx.theme().background.opacity(0.72))
        .child(
            div()
                .w(px(440.))
                .p_4()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .v_flex()
                .gap_3()
                .child(div().text_lg().font_semibold().child(title))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(body),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .justify_end()
                        .child(crate::rules_view::action_chip(
                            "fw-confirm-cancel",
                            "Cancel",
                            true,
                            false,
                            cx,
                            App::cancel_fw_confirm,
                        ))
                        .child(crate::rules_view::action_chip(
                            "fw-confirm-ok",
                            ok,
                            true,
                            true,
                            cx,
                            App::confirm_fw_action,
                        )),
                ),
        )
}

fn section_body(content: &ShellContent<'_>, cx: &Context<App>) -> Div {
    let muted = cx.theme().muted_foreground;
    let filter_input = content.filter_input;
    match content.section {
        Section::Events => filter_input.map_or_else(
            || div().child(div().text_color(muted).child("filter input unavailable")),
            |input| {
                events_body(
                    &content.log.lines(),
                    content.log.selected_index(),
                    content.log.subscribed(),
                    content.filter,
                    input,
                    cx,
                )
            },
        ),
        Section::Daemon => daemon_body(content.socket, content.link, content.stats_summary, cx),
        Section::Rules => rules_body(
            content.rules,
            content.selected_rule,
            content.rules_message,
            content.adding,
            cx,
        ),
        Section::Hosts => stats_section(Section::Hosts, content, cx),
        Section::Applications => {
            let mut body = applications_body(
                content.processes,
                content.selected_process,
                content.viewer_message,
                cx,
            );
            body = body.child(applications_stats_note(content.stats_procs, muted));
            if let Some(input) = filter_input {
                body = body.child(stats_table_body(
                    Section::Applications,
                    content.stats_procs,
                    content.filter,
                    input,
                    cx,
                ));
            }
            body
        }
        Section::Addresses => stats_section(Section::Addresses, content, cx),
        Section::Ports => stats_section(Section::Ports, content, cx),
        Section::Users => stats_section(Section::Users, content, cx),
        Section::Network => {
            network_body(content.link, content.network, content.network_message, cx)
        }
        Section::Traffic => traffic_body(content, cx),
        Section::Profiling => profiling_body(content.profiling, muted),
        Section::Preferences => settings_body(
            content.socket,
            content.tray_state,
            content.chrome_pref,
            content.chrome_mode,
            muted,
            cx,
        ),
    }
}

fn stats_section(section: Section, content: &ShellContent<'_>, cx: &Context<App>) -> Div {
    let muted = cx.theme().muted_foreground;
    let Some(input) = content.filter_input else {
        return div().child(div().text_color(muted).child("filter input unavailable"));
    };
    let rows = match section {
        Section::Hosts => content.stats_hosts,
        Section::Addresses => content.stats_addrs,
        Section::Ports => content.stats_ports,
        Section::Users => content.stats_users,
        Section::Applications => content.stats_procs,
        _ => &[],
    };
    stats_table_body(section, rows, content.filter, input, cx)
}

fn traffic_body(content: &ShellContent<'_>, cx: &Context<App>) -> Div {
    let muted = cx.theme().muted_foreground;
    let (machine, user, effective) = match content.link {
        DaemonLink::Up { status, .. } => (
            status.traffic_machine.as_str(),
            status.traffic_user.as_str(),
            status.traffic_effective.as_str(),
        ),
        DaemonLink::Down { .. } => ("—", "—", "—"),
    };
    let scope_machine = content.traffic_scope_machine;
    let direction = content.traffic_direction;
    div()
        .v_flex()
        .gap_3()
        .child(
            div()
                .text_sm()
                .text_color(muted)
                .child(format!(
                    "machine={machine}  user={user}  effective={effective}"
                )),
        )
        .child(
            div()
                .text_sm()
                .text_color(muted)
                .child(
                    "Machine overrides user without clearing it. Loopback is never blocked. User inbound is uid-scoped.",
                ),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .child(traffic_toggle(
                    "scope-user",
                    "This user",
                    !scope_machine,
                    cx,
                    App::set_traffic_scope_user,
                ))
                .child(traffic_toggle(
                    "scope-machine",
                    "Entire machine",
                    scope_machine,
                    cx,
                    App::set_traffic_scope_machine,
                )),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .child(traffic_toggle(
                    "dir-out",
                    "Outbound",
                    direction == "out",
                    cx,
                    App::set_traffic_direction_out,
                ))
                .child(traffic_toggle(
                    "dir-in",
                    "Inbound",
                    direction == "in",
                    cx,
                    App::set_traffic_direction_in,
                ))
                .child(traffic_toggle(
                    "dir-all",
                    "All",
                    direction == "all",
                    cx,
                    App::set_traffic_direction_all,
                )),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .child(crate::rules_view::action_chip(
                    "traffic-block",
                    "Block",
                    true,
                    true,
                    cx,
                    App::request_panel_traffic_block,
                ))
                .child(crate::rules_view::action_chip(
                    "traffic-unblock",
                    "Unblock",
                    true,
                    false,
                    cx,
                    App::request_panel_traffic_unblock,
                )),
        )
        .when_some(content.network_message, |this, message| {
            this.child(div().text_sm().text_color(muted).child(message.to_owned()))
        })
}

fn traffic_toggle(
    id: &'static str,
    label: &'static str,
    selected: bool,
    cx: &Context<App>,
    handler: fn(&mut App, &mut Window, &mut Context<App>),
) -> impl IntoElement {
    crate::rules_view::action_chip(id, label, true, selected, cx, handler)
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
