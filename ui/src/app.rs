//! `InterFire` desktop shell: left navigation, rules-first content, tray chrome.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use std::time::Duration;

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

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
            #[cfg(target_os = "linux")]
            tray: TrayHost::try_spawn(tray_state),
        }
    }

    /// Start periodic daemon polls that drive tray + Status chrome.
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
        self.link = ipc_poll::poll_link(&self.socket);
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
}

impl Render for App {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.section;
        let socket = self.socket.clone();
        let tray_state = self.tray_state;
        let link = self.link.clone();

        div()
            .id("interfire-shell")
            .flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(nav_column(selected, cx))
            .child(content_column(selected, &socket, tray_state, &link, cx))
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
