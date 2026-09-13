//! Aggregate stats tables and shared filter strip chrome.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use interfire_proto::{StatsRow, StatsSummary};

use crate::app::App;
use crate::filter::{ListFilter, ResultLimit, VerdictFilter};
use crate::section::Section;
use crate::tray::DaemonLink;

/// ASCII placeholder when a stats cell or chrome label has no value yet.
pub const EMPTY_CELL: &str = "-";

/// Filter strip for list tabs (text, verdict, limit, clear, shown/total).
pub fn filter_strip(
    filter: &ListFilter,
    filter_input: &Entity<gpui_kit::component::input::InputState>,
    shown: usize,
    total: usize,
    cx: &Context<App>,
) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let mut row = div()
        .id("filter-strip")
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(div().w(px(220.)).child(Input::new(filter_input)));
    for verdict in VerdictFilter::ALL {
        let selected = filter.verdict == verdict;
        row = row.child(crate::rules_view::action_chip(
            match verdict {
                VerdictFilter::All => "filter-verdict-all",
                VerdictFilter::Allow => "filter-verdict-allow",
                VerdictFilter::Deny => "filter-verdict-deny",
                VerdictFilter::Prompt => "filter-verdict-prompt",
            },
            verdict.label(),
            true,
            selected,
            cx,
            match verdict {
                VerdictFilter::All => App::set_filter_verdict_all,
                VerdictFilter::Allow => App::set_filter_verdict_allow,
                VerdictFilter::Deny => App::set_filter_verdict_deny,
                VerdictFilter::Prompt => App::set_filter_verdict_prompt,
            },
        ));
    }
    for preset in ResultLimit::PRESETS {
        let selected = matches!(filter.limit, ResultLimit::Preset(n) if n == preset);
        row = row.child(limit_chip(preset, selected, cx));
    }
    row = row
        .child(crate::rules_view::action_chip(
            "filter-limit-all",
            "All",
            true,
            matches!(filter.limit, ResultLimit::All),
            cx,
            App::set_filter_limit_all,
        ))
        .child(crate::rules_view::action_chip(
            "filter-limit-custom",
            "Custom",
            true,
            matches!(filter.limit, ResultLimit::Custom(_)),
            cx,
            App::set_filter_limit_custom,
        ))
        .child(crate::rules_view::action_chip(
            "filter-clear",
            "Clear",
            true,
            false,
            cx,
            App::clear_list_filter,
        ))
        .child(div().text_xs().text_color(muted).child(format!(
            "{shown} / {total} · limit {}",
            filter.limit.label()
        )));
    row
}

fn limit_chip(preset: usize, selected: bool, cx: &Context<App>) -> impl IntoElement {
    let id: &'static str = match preset {
        50 => "filter-limit-50",
        100 => "filter-limit-100",
        200 => "filter-limit-200",
        300 => "filter-limit-300",
        _ => "filter-limit-custom",
    };
    let label: &'static str = match preset {
        50 => "50",
        100 => "100",
        200 => "200",
        300 => "300",
        _ => "?",
    };
    let handler: fn(&mut App, &mut Window, &mut Context<App>) = if preset == 50 {
        App::set_filter_limit_50
    } else if preset == 200 {
        App::set_filter_limit_200
    } else if preset == 300 {
        App::set_filter_limit_300
    } else {
        App::set_filter_limit_100
    };
    crate::rules_view::action_chip(id, label, true, selected, cx, handler)
}

/// Events table (filtered audit stream).
pub fn events_body(
    lines: &[(u64, String)],
    selected: Option<usize>,
    subscribed: bool,
    filter: &ListFilter,
    filter_input: &Entity<gpui_kit::component::input::InputState>,
    cx: &Context<App>,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let (shown, total) = filter.apply_rows(lines.to_vec(), |(_seq, message)| {
        let verdict = extract_outcome(message).unwrap_or("");
        (message.clone(), verdict.to_owned())
    });
    let status = if subscribed {
        format!(
            "audit subscribe: ready  ·  showing {} of {}",
            shown.len(),
            total
        )
    } else {
        format!(
            "audit subscribe: connecting…  ·  showing {} of {}",
            shown.len(),
            total
        )
    };
    let body = div()
        .v_flex()
        .gap_2()
        .child(filter_strip(filter, filter_input, shown.len(), total, cx))
        .child(div().text_xs().text_color(muted).child(status))
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child("seq|message (Events; cap 2000)"),
        );
    if shown.is_empty() {
        return body.child(div().text_color(muted).child("no events match filter"));
    }
    let mut table = div().id("events-table").v_flex().gap_1().flex_1();
    for (offset, (seq, message)) in shown.iter().enumerate() {
        let is_selected = selected == Some(offset);
        table = table.child(event_row(offset, *seq, message, is_selected, cx));
    }
    body.child(table).when_some(
        selected.and_then(|idx| lines.get(idx)),
        |this, (_seq, message)| {
            this.child(
                div()
                    .mt_2()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .v_flex()
                    .gap_1()
                    .child(div().font_semibold().child("Selected"))
                    .child(div().text_sm().child(message.clone())),
            )
        },
    )
}

fn event_row(
    index: usize,
    seq: u64,
    message: &str,
    selected: bool,
    cx: &Context<App>,
) -> impl IntoElement {
    div()
        .id(ElementId::Name(format!("event-{index}").into()))
        .px_2()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .when(selected, |this| this.bg(cx.theme().accent.opacity(0.18)))
        .when(!selected, |this| {
            this.hover(|style| style.bg(cx.theme().accent.opacity(0.08)))
        })
        .on_click(cx.listener(move |app, _, _, cx| app.select_log_row(index, cx)))
        .child(
            div()
                .text_xs()
                .font_family("monospace")
                .child(format!("{seq}|{message}")),
        )
}

/// Single local daemon status row.
pub fn daemon_body(
    socket: &str,
    link: &DaemonLink,
    summary: Option<&StatsSummary>,
    cx: &Context<App>,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let (enforcement, traffic, version, git, uptime, connections, denied, rules) =
        match (link, summary) {
            (DaemonLink::Up { status, .. }, Some(summary)) => (
                status.enforcement.as_str(),
                status.traffic_effective.as_str(),
                summary.version.as_str(),
                summary.git.as_str(),
                format!("{}s", summary.uptime_secs),
                summary.connections.to_string(),
                summary.denied.to_string(),
                summary.rules.to_string(),
            ),
            (DaemonLink::Up { status, .. }, None) => (
                status.enforcement.as_str(),
                status.traffic_effective.as_str(),
                EMPTY_CELL,
                EMPTY_CELL,
                EMPTY_CELL.into(),
                EMPTY_CELL.into(),
                EMPTY_CELL.into(),
                EMPTY_CELL.into(),
            ),
            (DaemonLink::Down { reason }, _) => (
                "stopped",
                EMPTY_CELL,
                EMPTY_CELL,
                EMPTY_CELL,
                EMPTY_CELL.into(),
                EMPTY_CELL.into(),
                EMPTY_CELL.into(),
                reason.clone(),
            ),
        };
    div()
        .v_flex()
        .gap_2()
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child("socket  version  git  uptime  enforcement  traffic  connections  denied  rules"),
        )
        .child(
            div()
                .text_sm()
                .font_family("monospace")
                .child(format!(
                    "{socket}  {version}  {git}  {uptime}  {enforcement}  {traffic}  {connections}  {denied}  {rules}"
                )),
        )
}

/// Aggregate stats table for Hosts / Addresses / Ports / Users (and Applications hits).
pub fn stats_table_body(
    section: Section,
    rows: &[StatsRow],
    filter: &ListFilter,
    filter_input: &Entity<gpui_kit::component::input::InputState>,
    cx: &Context<App>,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let key_label = match section {
        Section::Hosts => "host",
        Section::Addresses => "address",
        Section::Ports => "port",
        Section::Users => "uid",
        Section::Applications => "executable",
        _ => "key",
    };
    let (shown, total) = filter.apply_rows(rows.to_vec(), |row| {
        let verdict = dominant_verdict(row);
        (row.key.clone(), verdict)
    });
    let body = div()
        .v_flex()
        .gap_2()
        .child(filter_strip(filter, filter_input, shown.len(), total, cx))
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child(format!("{key_label}  hits  allow  deny  prompt")),
        );
    if shown.is_empty() {
        return body.child(div().text_color(muted).child("no rows match filter"));
    }
    let mut table = div().id("stats-table").v_flex().gap_1();
    for row in &shown {
        table = table.child(
            div()
                .px_2()
                .py_1()
                .rounded_md()
                .text_xs()
                .font_family("monospace")
                .child(format!(
                    "{}  {}  {}  {}  {}",
                    row.key, row.hits, row.allow, row.deny, row.prompt
                )),
        );
    }
    body.child(table)
}

/// Applications: process list note plus aggregate executable hits.
pub fn applications_stats_note(rows: &[StatsRow], muted: Hsla) -> impl IntoElement {
    div().text_xs().text_color(muted).child(format!(
        "aggregate executables observed: {} (same stats store as Hosts/Ports)",
        rows.len()
    ))
}

fn dominant_verdict(row: &StatsRow) -> String {
    if row.deny > 0 && row.deny >= row.allow && row.deny >= row.prompt {
        "deny".into()
    } else if row.prompt > 0 && row.prompt >= row.allow {
        "prompt".into()
    } else if row.allow > 0 {
        "allow".into()
    } else {
        String::new()
    }
}

fn extract_outcome(message: &str) -> Option<&str> {
    message
        .split_whitespace()
        .find_map(|part| part.strip_prefix("outcome="))
}

/// Rich footer counters.
pub fn stats_footer(
    summary: Option<&StatsSummary>,
    rules_fallback: usize,
    cx: &Context<App>,
) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let (connections, denied, uptime, rules, version) = summary.map_or_else(
        || {
            (
                EMPTY_CELL.into(),
                EMPTY_CELL.into(),
                EMPTY_CELL.into(),
                rules_fallback.to_string(),
                format!(
                    "{}+{}",
                    env!("CARGO_PKG_VERSION"),
                    option_env!("INTERFIRE_GIT_DESCRIBE").unwrap_or("unknown")
                ),
            )
        },
        |summary| {
            (
                summary.connections.to_string(),
                summary.denied.to_string(),
                format!("{}s", summary.uptime_secs),
                summary.rules.to_string(),
                format!("{}+{}", summary.version, summary.git),
            )
        },
    );
    div()
        .id("stats-footer")
        .mt_auto()
        .pt_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .text_xs()
        .text_color(muted)
        .child(format!(
            "Connections {connections}  ·  Denied {denied}  ·  Uptime {uptime}  ·  Rules {rules}  ·  Version {version}"
        ))
}
