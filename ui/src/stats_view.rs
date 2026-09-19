//! Aggregate stats tables and shared bottom filter bar chrome.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use interfire_proto::{StatsRow, StatsSummary};

use crate::app::App;
use crate::chrome;
use crate::filter::{ListFilter, ResultLimit};
use crate::section::Section;
use crate::tray::DaemonLink;

/// ASCII placeholder when a stats cell or chrome label has no value yet.
pub const EMPTY_CELL: &str = "-";

/// Inputs for the shared bottom filter bar.
pub struct FilterBarView<'a> {
    pub filter: &'a ListFilter,
    pub filter_input: &'a Entity<gpui_kit::component::input::InputState>,
    pub shown: usize,
    pub total: usize,
    pub verdict_open: bool,
    pub limit_open: bool,
    pub custom_limit_input: Option<&'a Entity<gpui_kit::component::input::InputState>>,
}

/// Bottom filter bar for list tabs (search, selects, clear, counts).
pub fn filter_bar(view: &FilterBarView<'_>, cx: &Context<App>) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let mut row = div()
        .id("filter-bar")
        .flex()
        .flex_wrap()
        .items_end()
        .gap_2()
        .pt_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(div().w(px(220.)).child(Input::new(view.filter_input)))
        .child(chrome::verdict_select(
            view.filter.verdict,
            view.verdict_open,
            cx,
        ))
        .child(chrome::limit_select(view.filter.limit, view.limit_open, cx));
    if matches!(view.filter.limit, ResultLimit::Custom(_))
        && let Some(input) = view.custom_limit_input
    {
        row = row
            .child(div().w(px(88.)).child(Input::new(input)))
            .child(chrome::secondary_btn(
                "filter-apply-custom",
                "Apply",
                true,
                cx,
                App::apply_custom_limit,
            ));
    }
    row.child(chrome::ghost_btn(
        "filter-clear",
        "Clear",
        true,
        cx,
        App::clear_list_filter,
    ))
    .child(div().text_xs().text_color(muted).child(format!(
        "{} / {} · limit {}",
        view.shown,
        view.total,
        view.filter.limit.label()
    )))
}

/// Shown/total for Events after filter.
#[must_use]
pub fn events_filter_counts(lines: &[(u64, String)], filter: &ListFilter) -> (usize, usize) {
    let (shown, total) = filter.apply_rows(lines.to_vec(), |(_seq, message)| {
        let verdict = extract_outcome(message).unwrap_or("");
        (message.clone(), verdict.to_owned())
    });
    (shown.len(), total)
}

/// Shown/total for stats rows after filter.
#[must_use]
pub fn stats_filter_counts(rows: &[StatsRow], filter: &ListFilter) -> (usize, usize) {
    let (shown, total) = filter.apply_rows(rows.to_vec(), |row| {
        let verdict = dominant_verdict(row);
        (row.key.clone(), verdict)
    });
    (shown.len(), total)
}

/// Shown/total for rules after filter.
#[must_use]
pub fn rules_filter_counts(
    rules: &[interfire_proto::RuleRow],
    filter: &ListFilter,
) -> (usize, usize) {
    let (shown, total) = filter.apply_rows(rules.to_vec(), |rule| {
        (
            format!("{} {} {}", rule.executable, rule.port, rule.verdict),
            rule.verdict.clone(),
        )
    });
    (shown.len(), total)
}

/// Events table (filtered audit stream; filter bar is shell-owned).
pub fn events_body(
    lines: &[(u64, String)],
    selected: Option<usize>,
    subscribed: bool,
    filter: &ListFilter,
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
                .child(
                    "socket  version  git  uptime  enforcement  traffic  connections  denied  rules",
                ),
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

/// Aggregate stats table for Hosts / Addresses / Ports / Users.
pub fn stats_table_body(
    section: Section,
    rows: &[StatsRow],
    filter: &ListFilter,
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
        .child(div().text_xs().text_color(muted).child(format!(
            "{key_label}  hits  allow  deny  prompt  ·  {0} of {1}",
            shown.len(),
            total
        )));
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
