//! Log section: virtualized capped audit list.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::app::App;
use crate::log_buf::{DEFAULT_VIEWPORT_ROWS, LogBuffer, VisibleLog};

pub fn log_body(buf: &LogBuffer, cx: &Context<App>) -> Div {
    let muted = cx.theme().muted_foreground;
    let window = buf.visible_window(DEFAULT_VIEWPORT_ROWS);
    let status = if buf.subscribed() {
        format!(
            "audit subscribe: ready (id=interfire-ui)  ·  showing {} of {} (cap {})",
            window.items.len(),
            window.total,
            crate::log_buf::MAX_AUDIT_LINES
        )
    } else {
        format!(
            "audit subscribe: connecting…  ·  showing {} of {} (cap {})",
            window.items.len(),
            window.total,
            crate::log_buf::MAX_AUDIT_LINES
        )
    };

    let mut body = div()
        .v_flex()
        .gap_2()
        .child(div().text_xs().text_color(muted).child(status))
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child("seq|message (viewport around selection; cap 2000)"),
        )
        .child(log_table(&window, cx));

    if let Some(line) = buf.selected_line() {
        body = body.child(
            div()
                .mt_2()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .v_flex()
                .gap_1()
                .child(div().font_semibold().child("Selected"))
                .child(div().text_sm().child(line.to_owned())),
        );
    } else {
        body = body.child(div().text_color(muted).child("no audit frame selected"));
    }

    body
}

fn log_table(window: &VisibleLog, cx: &Context<App>) -> impl IntoElement {
    let mut table = div().id("log-table").v_flex().gap_1().flex_1();
    if window.items.is_empty() {
        return table
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("no audit records yet"),
            )
            .into_any_element();
    }
    for (offset, line) in window.items.iter().enumerate() {
        let absolute = window.start + offset;
        let selected = offset == window.relative_selected;
        table = table.child(log_row(absolute, line, selected, cx));
    }
    table.into_any_element()
}

fn log_row(index: usize, line: &str, selected: bool, cx: &Context<App>) -> impl IntoElement {
    div()
        .id(ElementId::Name(format!("log-row-{index}").into()))
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
        .on_click(cx.listener(move |app, _, _, cx| app.select_log_row(index, cx)))
        .child(div().text_sm().child(line.to_owned()))
}
