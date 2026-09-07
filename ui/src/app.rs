//! `InterFire` desktop shell: left navigation, tray chrome, connection alert.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use std::time::Duration;

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use interfire_proto::PromptRow;

use crate::alert::{AlertScope, AlertVerdict, ConnectionAlert};
use crate::ipc_poll;
use crate::section::Section;
use crate::tray::{DaemonLink, TrayState};
#[cfg(target_os = "linux")]
use crate::tray_host::TrayHost;

const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Root application view for the main window.
pub struct App {
    section: Section,
    socket: String,
    link: DaemonLink,
    tray_state: TrayState,
    prompts: Vec<PromptRow>,
    alert: Option<ConnectionAlert>,
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
        Self {
            section: Section::Rules,
            socket,
            link,
            tray_state,
            prompts: Vec::new(),
            alert: None,
            #[cfg(target_os = "linux")]
            tray: TrayHost::try_spawn(tray_state),
        }
    }

    /// Start periodic daemon polls that drive tray, Status, and alerts.
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
        let snapshot = ipc_poll::poll_snapshot(&self.socket);
        self.link = snapshot.link;
        self.prompts = snapshot.prompts;
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

    fn select(&mut self, section: Section, cx: &mut Context<Self>) {
        self.section = section;
        cx.notify();
    }

    fn set_scope(&mut self, scope: AlertScope, cx: &mut Context<Self>) {
        if let Some(alert) = &mut self.alert {
            alert.scope = scope;
            cx.notify();
        }
    }

    fn toggle_details(&mut self, cx: &mut Context<Self>) {
        if let Some(alert) = &mut self.alert {
            alert.details_open = !alert.details_open;
            cx.notify();
        }
    }

    fn submit_verdict(&mut self, verdict: AlertVerdict, cx: &mut Context<Self>) {
        let Some(alert) = &self.alert else {
            return;
        };
        if !alert.can_submit() {
            return;
        }
        let id = alert.prompt.id;
        let scope = alert.scope.as_str();
        match ipc_poll::answer_prompt(&self.socket, id, verdict.as_str(), scope) {
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
}

impl Render for App {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.section;
        let socket = self.socket.clone();
        let tray_state = self.tray_state;
        let link = self.link.clone();
        let alert = self.alert.clone();

        let shell = div()
            .id("interfire-shell")
            .relative()
            .flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(nav_column(selected, cx))
            .child(content_column(selected, &socket, tray_state, &link, cx));

        if let Some(alert) = alert {
            shell.child(alert_overlay(&alert, cx))
        } else {
            shell
        }
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

fn content_column(
    section: Section,
    socket: &str,
    tray_state: TrayState,
    link: &DaemonLink,
    cx: &Context<App>,
) -> impl IntoElement {
    div()
        .id("content")
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .p_4()
        .gap_3()
        .child(div().text_lg().font_semibold().child(section.label()))
        .child(section_body(section, socket, tray_state, link, cx))
        .child(
            div()
                .mt_auto()
                .pt_2()
                .border_t_1()
                .border_color(cx.theme().border)
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!(
                    "{}  |  tray: {}  |  {socket}",
                    section.label(),
                    tray_state.label()
                )),
        )
}

fn section_body(
    section: Section,
    socket: &str,
    tray_state: TrayState,
    link: &DaemonLink,
    cx: &Context<App>,
) -> Div {
    let muted = cx.theme().muted_foreground;
    match section {
        Section::Rules => div()
            .v_flex()
            .gap_2()
            .child("Rules is the primary policy surface.")
            .child(
                div()
                    .text_color(muted)
                    .child("Scaffold: dense rule table and CRUD land in a later slice."),
            ),
        Section::Status => status_body(socket, tray_state, link, muted),
        Section::Applications => div()
            .text_color(muted)
            .child("Observed identities and effective rules (thin shell)."),
        Section::Log => div()
            .text_color(muted)
            .child("Capped audit stream (virtualized in a later slice)."),
        Section::Network => div()
            .text_color(muted)
            .child("InterFire-owned nftables view only (thin until packaging work)."),
        Section::Settings => div()
            .v_flex()
            .gap_2()
            .child("Socket path, diagnostics, reconnect.")
            .child(div().text_color(muted).child(format!("socket = {socket}")))
            .child(
                div()
                    .text_color(muted)
                    .child(format!("tray = {}", tray_state.label())),
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

fn alert_overlay(alert: &ConnectionAlert, cx: &Context<App>) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let can_submit = alert.can_submit();
    let prompt = &alert.prompt;

    div()
        .id("connection-alert")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(cx.theme().background.opacity(0.72))
        .child(
            div()
                .id("connection-alert-card")
                .w(px(520.))
                .max_w_full()
                .p_4()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .v_flex()
                .gap_3()
                .child(div().text_lg().font_semibold().child("Connection request"))
                .child(div().font_semibold().child(prompt.executable.clone()))
                .child(div().child(format!(
                    "{}:{} ({})",
                    prompt.destination, prompt.port, prompt.protocol
                )))
                .child(div().child(format!("remaining: {}s", prompt.remaining_secs)))
                .when(alert.stale, |this| {
                    this.child(
                        div()
                            .text_color(muted)
                            .child("Expired or resolved elsewhere. Answer controls disabled."),
                    )
                })
                .child(div().text_color(muted).child(alert.scope.rule_note()))
                .child(scope_row(alert.scope, can_submit, cx))
                .child(verdict_row(can_submit, cx))
                .child(details_toggle(alert.details_open, cx))
                .when(alert.details_open, |this| {
                    this.child(
                        div()
                            .v_flex()
                            .gap_1()
                            .text_color(muted)
                            .child(format!("prompt id: {}", prompt.id))
                            .child(format!("protocol: {}", prompt.protocol))
                            .child(
                                "PID + start ticks, cmdline, uid, and cgroup appear when the daemon exposes them.",
                            ),
                    )
                })
                .when_some(alert.status_message.clone(), |this, message| {
                    this.child(div().text_color(muted).child(format!("error: {message}")))
                }),
        )
}

fn scope_row(selected: AlertScope, enabled: bool, cx: &Context<App>) -> impl IntoElement {
    let mut row = div().id("alert-scopes").flex().gap_2();
    for scope in AlertScope::ALL {
        let is_selected = scope == selected;
        row = row.child(scope_chip(scope, is_selected, enabled, cx));
    }
    row
}

fn scope_chip(
    scope: AlertScope,
    selected: bool,
    enabled: bool,
    cx: &Context<App>,
) -> impl IntoElement {
    let label = scope.label();
    div()
        .id(ElementId::Name(format!("scope-{label}").into()))
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .when(selected, |this| {
            this.bg(cx.theme().accent)
                .text_color(cx.theme().accent_foreground)
        })
        .when(enabled, |this| {
            this.cursor_pointer()
                .on_click(cx.listener(move |app, _, _, cx| app.set_scope(scope, cx)))
        })
        .when(!enabled, |this| {
            this.text_color(cx.theme().muted_foreground)
        })
        .child(label)
}

fn verdict_row(enabled: bool, cx: &Context<App>) -> impl IntoElement {
    div()
        .id("alert-verdicts")
        .flex()
        .gap_2()
        .child(verdict_chip(AlertVerdict::Allow, enabled, false, cx))
        .child(verdict_chip(AlertVerdict::Deny, enabled, true, cx))
}

fn verdict_chip(
    verdict: AlertVerdict,
    enabled: bool,
    emphasize: bool,
    cx: &Context<App>,
) -> impl IntoElement {
    let label = verdict.label();
    div()
        .id(ElementId::Name(format!("verdict-{label}").into()))
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .when(emphasize, |this| {
            this.bg(cx.theme().accent)
                .text_color(cx.theme().accent_foreground)
        })
        .when(enabled, |this| {
            this.cursor_pointer()
                .on_click(cx.listener(move |app, _, _, cx| app.submit_verdict(verdict, cx)))
        })
        .when(!enabled, |this| {
            this.text_color(cx.theme().muted_foreground)
        })
        .child(label)
}

fn details_toggle(open: bool, cx: &Context<App>) -> impl IntoElement {
    let label = if open { "Hide details" } else { "Show details" };
    div()
        .id("alert-details-toggle")
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .cursor_pointer()
        .on_click(cx.listener(|app, _, _, cx| app.toggle_details(cx)))
        .child(label)
}
