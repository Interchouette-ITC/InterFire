//! `InterFire` desktop shell: left navigation and rules-first content pane.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use interfire_proto::DEFAULT_SOCKET_PATH;

use crate::section::Section;

/// Root application view for the main window.
pub struct App {
    section: Section,
    socket: String,
}

impl App {
    #[must_use]
    pub fn new() -> Self {
        Self {
            section: Section::Rules,
            socket: DEFAULT_SOCKET_PATH.to_owned(),
        }
    }

    fn select(&mut self, section: Section, cx: &mut Context<Self>) {
        self.section = section;
        cx.notify();
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for App {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.section;
        let socket = self.socket.clone();

        div()
            .id("interfire-shell")
            .flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(nav_column(selected, cx))
            .child(content_column(selected, &socket, cx))
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

fn content_column(section: Section, socket: &str, cx: &Context<App>) -> impl IntoElement {
    div()
        .id("content")
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .p_4()
        .gap_3()
        .child(div().text_lg().font_semibold().child(section.label()))
        .child(section_body(section, socket, cx))
        .child(
            div()
                .mt_auto()
                .pt_2()
                .border_t_1()
                .border_color(cx.theme().border)
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{}  |  {socket}  |  Ready", section.label())),
        )
}

fn section_body(section: Section, socket: &str, cx: &Context<App>) -> impl IntoElement {
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
        Section::Status => div()
            .v_flex()
            .gap_2()
            .child("Daemon status will poll over Unix IPC.")
            .child(
                div()
                    .text_color(muted)
                    .child(format!("Default socket: {socket}")),
            ),
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
            .child(div().text_color(muted).child(format!("socket = {socket}"))),
    }
}
