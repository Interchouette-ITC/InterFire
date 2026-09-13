//! Network statistics shell chrome: toolbar, tabs, menu, about overlay.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::app::App;
use crate::brand;
use crate::rules_view;
use crate::section::Section;
use crate::stats_view::EMPTY_CELL;
use crate::theme::ChromeMode;
use crate::tray::{DaemonLink, TrayState};

struct OperatorLabels {
    daemon_up: bool,
    rules_paused: bool,
    daemon_label: &'static str,
    rules_label: String,
    traffic_label: String,
    traffic_accent: bool,
}

fn operator_labels(tray_state: TrayState, link: &DaemonLink) -> OperatorLabels {
    let daemon_up = !matches!(tray_state, TrayState::Unavailable);
    let rules_paused = match link {
        DaemonLink::Up { status, .. } => status.enforcement == "paused",
        DaemonLink::Down { .. } => true,
    };
    let daemon_label = if daemon_up {
        "Daemon: Running"
    } else {
        "Daemon: Stopped"
    };
    let rules_label = if !daemon_up {
        format!("Rules: {EMPTY_CELL}")
    } else if rules_paused {
        "Rules: Paused".to_owned()
    } else {
        "Rules: Active".to_owned()
    };
    let traffic_label = if daemon_up {
        match link {
            DaemonLink::Up { status, .. } => {
                if status.traffic_machine != "open" {
                    "Traffic: Machine".to_owned()
                } else if status.traffic_user != "open" {
                    "Traffic: User".to_owned()
                } else {
                    "Traffic: Open".to_owned()
                }
            }
            DaemonLink::Down { .. } => format!("Traffic: {EMPTY_CELL}"),
        }
    } else {
        format!("Traffic: {EMPTY_CELL}")
    };
    let traffic_accent = matches!(
        link,
        DaemonLink::Up { status, .. } if status.traffic == "blocked"
    );
    OperatorLabels {
        daemon_up,
        rules_paused,
        daemon_label,
        rules_label,
        traffic_label,
        traffic_accent,
    }
}

fn toolbar_left(chrome_mode: ChromeMode, menu_open: bool, cx: &Context<App>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            img(brand::nav_mark_source(chrome_mode))
                .id("toolbar-mark")
                .w(px(28.))
                .h(px(28.))
                .rounded_md()
                .object_fit(ObjectFit::Contain),
        )
        .child(div().text_sm().font_semibold().child("InterFire"))
        .child(rules_view::action_chip(
            "app-menu",
            if menu_open { "Menu ▾" } else { "Menu" },
            true,
            menu_open,
            cx,
            App::toggle_app_menu,
        ))
        .child(rules_view::action_chip(
            "prefs-btn",
            "Preferences",
            true,
            false,
            cx,
            App::open_preferences,
        ))
        .child(rules_view::action_chip(
            "add-rule-toolbar",
            "Add rule",
            true,
            true,
            cx,
            App::begin_add_rule,
        ))
}

fn toolbar_right(
    labels: &OperatorLabels,
    tray_state: TrayState,
    cx: &Context<App>,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(control_chip(
            "daemon-btn",
            labels.daemon_label,
            if labels.daemon_up { "Stop" } else { "Start" },
            true,
            !labels.daemon_up,
            cx,
            if labels.daemon_up {
                App::request_daemon_stop_click
            } else {
                App::request_daemon_start_click
            },
        ))
        .child(control_chip(
            "rules-btn",
            &labels.rules_label,
            if labels.rules_paused {
                "Start"
            } else {
                "Pause"
            },
            labels.daemon_up,
            labels.rules_paused,
            cx,
            if labels.rules_paused {
                App::request_resume_confirm_click
            } else {
                App::request_pause_confirm_click
            },
        ))
        .child(control_chip(
            "traffic-btn",
            &labels.traffic_label,
            "Open",
            labels.daemon_up,
            labels.traffic_accent,
            cx,
            App::open_traffic_tab,
        ))
        .child(tray_chip(tray_state, cx))
}

/// Top toolbar: brand, menu, Preferences, Add rule, operator chips.
pub fn toolbar_row(
    tray_state: TrayState,
    link: &DaemonLink,
    chrome_mode: ChromeMode,
    menu_open: bool,
    cx: &Context<App>,
) -> impl IntoElement {
    let labels = operator_labels(tray_state, link);
    div()
        .id("toolbar")
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .child(toolbar_left(chrome_mode, menu_open, cx))
        .child(toolbar_right(&labels, tray_state, cx))
}

/// App menu secondary entries (Traffic / Network / Profiling / About / Quit).
pub fn menu_row(cx: &Context<App>) -> impl IntoElement {
    div()
        .id("app-menu-row")
        .flex()
        .flex_wrap()
        .gap_2()
        .px_1()
        .child(rules_view::action_chip(
            "menu-traffic",
            "Traffic…",
            true,
            false,
            cx,
            App::open_traffic_tab,
        ))
        .child(rules_view::action_chip(
            "menu-network",
            "Network…",
            true,
            false,
            cx,
            App::open_network,
        ))
        .child(rules_view::action_chip(
            "menu-profiling",
            "Profiling…",
            true,
            false,
            cx,
            App::open_profiling,
        ))
        .child(rules_view::action_chip(
            "menu-about",
            "About",
            true,
            false,
            cx,
            App::open_about,
        ))
        .child(rules_view::action_chip(
            "menu-quit",
            "Quit",
            true,
            false,
            cx,
            App::quit_app,
        ))
}

/// Horizontal primary tabs.
pub fn tabs_row(selected: Section, cx: &Context<App>) -> impl IntoElement {
    let mut row = div().id("primary-tabs").flex().flex_wrap().gap_1();
    for section in Section::PRIMARY {
        let is_selected = section == selected;
        row = row.child(tab_button(section, is_selected, cx));
    }
    row
}

fn tab_button(section: Section, selected: bool, cx: &Context<App>) -> impl IntoElement {
    let label = section.label();
    div()
        .id(ElementId::Name(format!("tab-{label}").into()))
        .px_3()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .text_sm()
        .when(selected, |this| {
            this.bg(cx.theme().accent.opacity(0.22))
                .text_color(cx.theme().foreground)
                .font_semibold()
        })
        .when(!selected, |this| {
            this.text_color(cx.theme().muted_foreground).hover(|style| {
                style
                    .bg(cx.theme().accent.opacity(0.1))
                    .text_color(cx.theme().foreground)
            })
        })
        .on_click(cx.listener(move |app, _, _, cx| app.select(section, cx)))
        .child(label)
}

pub fn control_chip(
    id: &'static str,
    label: &str,
    action: &'static str,
    enabled: bool,
    warning: bool,
    cx: &Context<App>,
    on_click: fn(&mut App, &mut Window, &mut Context<App>),
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(if warning {
                    cx.theme().warning
                } else if enabled {
                    cx.theme().success
                } else {
                    cx.theme().muted_foreground
                })
                .child(label.to_owned()),
        )
        .child(rules_view::action_chip(
            id, action, enabled, warning, cx, on_click,
        ))
}

fn tray_chip(state: TrayState, cx: &Context<App>) -> impl IntoElement {
    let (fill, label_color) = match state {
        TrayState::Protected => (cx.theme().success.opacity(0.2), cx.theme().success),
        TrayState::Prompting => (cx.theme().accent.opacity(0.25), cx.theme().accent),
        TrayState::Degraded => (cx.theme().warning.opacity(0.22), cx.theme().warning),
        TrayState::Paused => (cx.theme().warning.opacity(0.18), cx.theme().warning),
        TrayState::Blocked => (cx.theme().danger.opacity(0.18), cx.theme().danger),
        TrayState::Unavailable => (cx.theme().danger.opacity(0.22), cx.theme().danger),
    };
    div()
        .id("tray-chip")
        .px_2()
        .py_1()
        .rounded_md()
        .bg(fill)
        .text_xs()
        .font_semibold()
        .text_color(label_color)
        .child(state.label().to_uppercase())
}

/// About overlay (version + git describe).
pub fn about_overlay(cx: &Context<App>) -> impl IntoElement {
    let version = env!("CARGO_PKG_VERSION");
    let git = option_env!("INTERFIRE_GIT_DESCRIBE").unwrap_or("unknown");
    div()
        .id("about-overlay")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(cx.theme().background.opacity(0.72))
        .child(
            div()
                .w(px(400.))
                .p_4()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .v_flex()
                .gap_3()
                .child(div().text_lg().font_semibold().child("About InterFire"))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("Version {version} ({git})")),
                )
                .child(div().flex().justify_end().child(rules_view::action_chip(
                    "about-close",
                    "Close",
                    true,
                    false,
                    cx,
                    App::close_about,
                ))),
        )
}
