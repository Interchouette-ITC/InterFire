//! Connection-alert overlay widgets.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::alert::{AlertScope, AlertVerdict, ConnectionAlert};
use crate::app::App;
use crate::theme;

pub fn alert_overlay(alert: &ConnectionAlert, cx: &Context<App>) -> impl IntoElement {
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
        .bg(cx.theme().overlay)
        .child(
            div()
                .id("connection-alert-card")
                .w(px(540.))
                .max_w_full()
                .p_5()
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().accent.opacity(0.55))
                .bg(theme::hex(theme::ELEVATED))
                .shadow_lg()
                .v_flex()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_lg()
                                .font_semibold()
                                .text_color(cx.theme().foreground)
                                .child("Connection request"),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .bg(cx.theme().accent.opacity(0.2))
                                .text_xs()
                                .font_semibold()
                                .text_color(cx.theme().accent)
                                .child(format!("{}s", prompt.remaining_secs)),
                        ),
                )
                .child(
                    div()
                        .font_semibold()
                        .text_color(cx.theme().foreground)
                        .child(prompt.executable.clone()),
                )
                .child(div().child(format!(
                    "{}:{} ({})",
                    prompt.destination, prompt.port, prompt.protocol
                )))
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
                            .p_3()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(theme::hex(theme::SURFACE))
                            .text_color(muted)
                            .child(format!("prompt id: {}", prompt.id))
                            .child(format!("protocol: {}", prompt.protocol))
                            .child(
                                "PID + start ticks, cmdline, uid, and cgroup appear when the daemon exposes them.",
                            ),
                    )
                })
                .when_some(alert.status_message.clone(), |this, message| {
                    this.child(
                        div()
                            .text_color(cx.theme().danger)
                            .child(format!("error: {message}")),
                    )
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
                .border_color(cx.theme().accent)
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
        .child(verdict_chip(AlertVerdict::Allow, enabled, cx))
        .child(verdict_chip(AlertVerdict::Deny, enabled, cx))
}

fn verdict_chip(verdict: AlertVerdict, enabled: bool, cx: &Context<App>) -> impl IntoElement {
    let label = verdict.label();
    let is_allow = matches!(verdict, AlertVerdict::Allow);
    div()
        .id(ElementId::Name(format!("verdict-{label}").into()))
        .flex_1()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .font_semibold()
        .when(is_allow, |this| {
            this.border_color(cx.theme().accent)
                .bg(cx.theme().accent)
                .text_color(cx.theme().accent_foreground)
        })
        .when(!is_allow, |this| {
            this.border_color(cx.theme().danger)
                .bg(cx.theme().danger)
                .text_color(cx.theme().danger_foreground)
        })
        .when(enabled, |this| {
            this.cursor_pointer()
                .on_click(cx.listener(move |app, _, _, cx| app.submit_verdict(verdict, cx)))
        })
        .when(!enabled, |this| {
            this.opacity(0.45).text_color(cx.theme().muted_foreground)
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
        .hover(|style| style.text_color(cx.theme().accent))
        .on_click(cx.listener(|app, _, _, cx| app.toggle_details(cx)))
        .child(label)
}
