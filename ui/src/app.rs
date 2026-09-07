//! `InterFire` desktop shell: left navigation, tray, alert, Rules, and Log.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use std::time::Duration;

use gpui_kit::component::input::InputState;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use interfire_proto::{PromptRow, RuleRow};

use crate::alert::{AlertScope, AlertVerdict, ConnectionAlert};
use crate::alert_view::alert_overlay;
use crate::audit_host::{AuditEvent, AuditHost};
use crate::ipc_poll;
use crate::log_buf::LogBuffer;
use crate::log_view::log_body;
use crate::rss_probe::{RssProbeMode, prompt_load_fixture};
use crate::rules::{RuleVerdict, next_rule_id, validate_new_rule};
use crate::rules_view::{add_rule_overlay, rules_body};
use crate::section::Section;
use crate::tray::{DaemonLink, TrayState};
#[cfg(target_os = "linux")]
use crate::tray_host::TrayHost;

const POLL_INTERVAL: Duration = Duration::from_secs(1);

struct ShellContent<'a> {
    section: Section,
    socket: &'a str,
    tray_state: TrayState,
    link: &'a DaemonLink,
    rules: &'a [RuleRow],
    selected_rule: Option<u64>,
    rules_message: Option<&'a str>,
    adding: bool,
    log: &'a LogBuffer,
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
    alert: Option<ConnectionAlert>,
    log: LogBuffer,
    audit: AuditHost,
    rss_probe: Option<RssProbeMode>,
    #[cfg(target_os = "linux")]
    tray: Option<TrayHost>,
}

impl App {
    #[must_use]
    pub fn new(socket: String) -> Self {
        let link = DaemonLink::Down {
            reason: "connecting".into(),
        };
        let tray_state = TrayState::from_link(&link);
        let audit = AuditHost::spawn(socket.clone());
        Self {
            section: Section::Rules,
            socket,
            link,
            tray_state,
            prompts: Vec::new(),
            rules: Vec::new(),
            selected_rule: None,
            rules_message: None,
            add_form: None,
            alert: None,
            log: LogBuffer::new(),
            audit,
            rss_probe: None,
            #[cfg(target_os = "linux")]
            tray: TrayHost::try_spawn(tray_state),
        }
    }

    /// Stage synthetic load for `make memcheck-ui` (`--rss-probe=…`).
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
        if self.rss_probe != Some(RssProbeMode::PromptLoad) {
            self.prompts = snapshot.prompts;
            self.rules = snapshot.rules;
            if let Some(id) = self.selected_rule
                && !self.rules.iter().any(|row| row.id == id)
            {
                self.selected_rule = None;
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
        let log = self.log.clone();

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
                    log: &log,
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
        Section::Applications => div()
            .text_color(muted)
            .child("Observed identities and effective rules are not listed here yet."),
        Section::Log => log_body(content.log, cx),
        Section::Network => div()
            .text_color(muted)
            .child("InterFire-owned nftables controls are not available yet."),
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
