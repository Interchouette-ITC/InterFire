//! Rules list, detail, and add-rule overlay.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use interfire_proto::RuleRow;

use crate::app::{AddRuleFormState, App};
use crate::rules::RuleVerdict;
use crate::theme;

pub fn rules_body(
    rules: &[RuleRow],
    selected_id: Option<u64>,
    message: Option<&str>,
    adding: bool,
    cx: &Context<App>,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut body = div()
        .v_flex()
        .gap_2()
        .child(
            div()
                .flex()
                .gap_2()
                .child(action_chip(
                    "add-rule",
                    "Add rule",
                    !adding,
                    true,
                    cx,
                    App::begin_add_rule,
                ))
                .child(action_chip(
                    "delete-rule",
                    "Delete selected",
                    selected_id.is_some() && !adding,
                    false,
                    cx,
                    App::delete_selected_rule,
                )),
        )
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child("id  path  verdict  port"),
        )
        .child(rules_table(rules, selected_id, cx));

    if let Some(id) = selected_id
        && let Some(rule) = rules.iter().find(|row| row.id == id)
    {
        body = body.child(rule_detail(rule, muted, cx));
    }

    if let Some(message) = message {
        body = body.child(div().text_color(muted).child(message.to_owned()));
    }

    body
}

pub fn add_rule_overlay(form: &AddRuleFormState, cx: &Context<App>) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    div()
        .id("add-rule-overlay")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(cx.theme().overlay)
        .child(
            div()
                .id("add-rule-card")
                .w(px(500.))
                .max_w_full()
                .p_5()
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().border)
                .bg(theme::hex(theme::ELEVATED))
                .shadow_lg()
                .v_flex()
                .gap_3()
                .child(div().text_lg().font_semibold().child("Add rule"))
                .child(field_label("id", muted))
                .child(Input::new(&form.id))
                .child(field_label("executable (absolute path)", muted))
                .child(Input::new(&form.executable))
                .child(field_label("port", muted))
                .child(Input::new(&form.port))
                .child(field_label("verdict", muted))
                .child(verdict_row(form.verdict, cx))
                .when_some(form.error.clone(), |this, error| {
                    this.child(div().text_color(cx.theme().danger).child(error))
                })
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(action_chip(
                            "submit-add-rule",
                            "Save",
                            true,
                            true,
                            cx,
                            App::submit_add_rule,
                        ))
                        .child(action_chip(
                            "cancel-add-rule",
                            "Cancel",
                            true,
                            false,
                            cx,
                            App::cancel_add_rule,
                        )),
                ),
        )
}

fn field_label(text: &str, muted: Hsla) -> Div {
    div().text_xs().text_color(muted).child(text.to_owned())
}

fn rules_table(rules: &[RuleRow], selected_id: Option<u64>, cx: &Context<App>) -> impl IntoElement {
    let mut table = div()
        .id("rules-table")
        .v_flex()
        .gap_1()
        .max_h(px(360.))
        .overflow_y_scroll();
    if rules.is_empty() {
        return table
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("No rules loaded. Add a rule or wait for the daemon."),
            )
            .into_any_element();
    }
    for rule in rules {
        let selected = Some(rule.id) == selected_id;
        let id = rule.id;
        table = table.child(
            div()
                .id(ElementId::Name(format!("rule-{id}").into()))
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
                .on_click(cx.listener(move |app, _, _, cx| app.select_rule(id, cx)))
                .child(rule.list_label()),
        );
    }
    table.into_any_element()
}

fn rule_detail(rule: &RuleRow, muted: Hsla, cx: &Context<App>) -> Div {
    div()
        .v_flex()
        .gap_1()
        .pt_2()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(theme::hex(theme::CANVAS))
        .child(div().font_semibold().child("Selected"))
        .child(div().text_color(muted).child(format!("id: {}", rule.id)))
        .child(
            div()
                .text_color(muted)
                .child(format!("path: {}", rule.executable)),
        )
        .child(
            div()
                .text_color(muted)
                .child(format!("verdict: {}", rule.verdict)),
        )
        .child(
            div()
                .text_color(muted)
                .child(format!("port: {}", rule.port)),
        )
}

fn verdict_row(selected: RuleVerdict, cx: &Context<App>) -> impl IntoElement {
    let mut row = div().id("add-rule-verdicts").flex().gap_2();
    for verdict in RuleVerdict::ALL {
        let is_selected = verdict == selected;
        let label = verdict.label();
        let is_allow = matches!(verdict, RuleVerdict::Allow);
        row = row.child(
            div()
                .id(ElementId::Name(format!("add-verdict-{label}").into()))
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .cursor_pointer()
                .when(is_selected && is_allow, |this| {
                    this.bg(cx.theme().accent)
                        .text_color(cx.theme().accent_foreground)
                        .border_color(cx.theme().accent)
                })
                .when(is_selected && !is_allow, |this| {
                    this.bg(cx.theme().danger)
                        .text_color(cx.theme().danger_foreground)
                        .border_color(cx.theme().danger)
                })
                .when(!is_selected, |this| this.border_color(cx.theme().border))
                .on_click(cx.listener(move |app, _, _, cx| app.set_add_verdict(verdict, cx)))
                .child(label),
        );
    }
    row
}

fn action_chip(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    primary: bool,
    cx: &Context<App>,
    on_click: impl Fn(&mut App, &mut Window, &mut Context<App>) + 'static,
) -> impl IntoElement {
    div()
        .id(ElementId::Name(id.into()))
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .when(primary && enabled, |this| {
            this.border_color(cx.theme().accent)
                .bg(cx.theme().accent)
                .text_color(cx.theme().accent_foreground)
        })
        .when(!primary || !enabled, |this| {
            this.border_color(cx.theme().border)
        })
        .when(enabled, |this| {
            this.cursor_pointer()
                .on_click(cx.listener(move |app, _, window, cx| on_click(app, window, cx)))
        })
        .when(!enabled, |this| {
            this.text_color(cx.theme().muted_foreground)
        })
        .child(label)
}
