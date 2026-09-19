//! Network statistics shell chrome: toolbar, tabs, menus, overlays.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::app::App;
use crate::brand;
use crate::chrome::{self, PillKind};
use crate::section::Section;
use crate::stats_view::EMPTY_CELL;
use crate::theme::{ChromeMode, ChromePreference};
use crate::tray::{DaemonLink, TrayState};

struct OperatorLabels {
    daemon_up: bool,
    rules_paused: bool,
    daemon_label: &'static str,
    rules_label: String,
    traffic_label: String,
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
    OperatorLabels {
        daemon_up,
        rules_paused,
        daemon_label,
        rules_label,
        traffic_label,
    }
}

fn toolbar_left(chrome_mode: ChromeMode, menu_open: bool, cx: &Context<App>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .relative()
        .child(
            img(brand::nav_mark_source(chrome_mode))
                .id("toolbar-mark")
                .w(px(28.))
                .h(px(28.))
                .rounded_md()
                .object_fit(ObjectFit::Contain),
        )
        .child(div().text_sm().font_semibold().child("InterFire"))
        .child(
            div()
                .relative()
                .child(chrome::secondary_btn(
                    "app-menu",
                    if menu_open { "Menu ▾" } else { "Menu" },
                    true,
                    cx,
                    App::toggle_app_menu,
                ))
                .when(menu_open, |this| this.child(app_menu_dropdown(cx))),
        )
        .child(chrome::primary_btn(
            "add-rule-toolbar",
            "Add rule",
            true,
            cx,
            App::begin_add_rule,
        ))
}

fn app_menu_dropdown(cx: &Context<App>) -> impl IntoElement {
    div()
        .id("app-menu-dropdown")
        .absolute()
        .top_full()
        .left_0()
        .mt_1()
        .w(px(200.))
        .p_1()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().popover)
        .shadow_lg()
        .v_flex()
        .gap_0()
        .child(chrome::menu_item(
            "menu-open",
            "Open",
            cx,
            App::focus_main_window,
        ))
        .child(chrome::menu_item(
            "menu-prefs",
            "Preferences",
            cx,
            App::open_preferences,
        ))
        .child(div().h(px(1.)).my_1().bg(cx.theme().border))
        .child(chrome::menu_item(
            "menu-traffic",
            "Traffic…",
            cx,
            App::open_traffic_tab,
        ))
        .child(chrome::menu_item(
            "menu-network",
            "Network…",
            cx,
            App::open_network,
        ))
        .child(chrome::menu_item(
            "menu-profiling",
            "Profiling…",
            cx,
            App::open_profiling,
        ))
        .child(div().h(px(1.)).my_1().bg(cx.theme().border))
        .child(chrome::menu_item(
            "menu-about",
            "About",
            cx,
            App::open_about,
        ))
        .child(chrome::menu_item("menu-quit", "Quit", cx, App::quit_app))
}

fn toolbar_right(
    labels: &OperatorLabels,
    tray_state: TrayState,
    cx: &Context<App>,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(chrome::play_pause_control(
            "daemon-btn",
            labels.daemon_label.to_owned(),
            !labels.daemon_up,
            true,
            labels.daemon_up,
            cx,
            if labels.daemon_up {
                App::request_daemon_stop_click
            } else {
                App::request_daemon_start_click
            },
        ))
        .child(chrome::play_pause_control(
            "rules-btn",
            labels.rules_label.clone(),
            labels.rules_paused,
            labels.daemon_up,
            labels.daemon_up && !labels.rules_paused,
            cx,
            if labels.rules_paused {
                App::request_resume_confirm_click
            } else {
                App::request_pause_confirm_click
            },
        ))
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(if labels.daemon_up {
                            cx.theme().foreground
                        } else {
                            chrome::off_fg(cx)
                        })
                        .child(labels.traffic_label.clone()),
                )
                .child(chrome::secondary_btn(
                    "traffic-btn",
                    "Open",
                    labels.daemon_up,
                    cx,
                    App::open_traffic_tab,
                )),
        )
        .child(tray_pill(tray_state, cx))
}

/// Top toolbar: brand, app menu, Add rule, operator play/pause, state pill.
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

/// Horizontal primary tabs (secondary sections leave none selected).
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
                .border_b_2()
                .border_color(cx.theme().accent)
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

fn tray_pill(state: TrayState, cx: &Context<App>) -> impl IntoElement {
    let kind = match state {
        TrayState::Protected => PillKind::On,
        TrayState::Prompting => PillKind::Info,
        TrayState::Degraded | TrayState::Paused => PillKind::Warn,
        TrayState::Blocked | TrayState::Unavailable => PillKind::Danger,
    };
    chrome::state_pill(state.label(), kind, cx)
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
                .child(div().flex().justify_end().child(chrome::secondary_btn(
                    "about-close",
                    "Close",
                    true,
                    cx,
                    App::close_about,
                ))),
        )
}

/// Preferences modal (theme; socket/tray read-only until daemon exposes edits).
pub fn preferences_overlay(
    socket: &str,
    tray_state: TrayState,
    chrome_pref: ChromePreference,
    chrome_mode: ChromeMode,
    cx: &Context<App>,
) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    div()
        .id("preferences-overlay")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(cx.theme().overlay)
        .child(
            div()
                .w(px(440.))
                .p_5()
                .rounded_xl()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .shadow_lg()
                .v_flex()
                .gap_3()
                .child(div().text_lg().font_semibold().child("Preferences"))
                .child(
                    img(brand::logo_horizontal_source())
                        .id("prefs-logo")
                        .w(px(200.))
                        .h(px(72.))
                        .object_fit(ObjectFit::Contain),
                )
                .child(prefs_row("Socket", socket, muted))
                .child(prefs_row("Tray", tray_state.label(), muted))
                .child(
                    div()
                        .v_flex()
                        .gap_2()
                        .child(div().text_sm().font_semibold().child("Theme"))
                        .child(div().text_xs().text_color(muted).child(format!(
                            "Appearance: {} (phoenix orange brand)",
                            chrome_mode.label()
                        )))
                        .child(theme_switcher(chrome_pref, cx)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(
                            "Prompt defaults appear here when the daemon exposes them. Use Daemon tab for live status.",
                        ),
                )
                .child(div().flex().justify_end().child(chrome::primary_btn(
                    "prefs-close",
                    "Done",
                    true,
                    cx,
                    App::close_preferences,
                ))),
        )
}

fn prefs_row(label: &'static str, value: &str, muted: Hsla) -> impl IntoElement {
    div()
        .flex()
        .justify_between()
        .gap_3()
        .px_2()
        .py_1()
        .rounded_md()
        .bg(muted.opacity(0.08))
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(muted)
                .child(label),
        )
        .child(
            div()
                .text_xs()
                .font_family("monospace")
                .child(value.to_owned()),
        )
}

fn theme_switcher(selected: ChromePreference, cx: &Context<App>) -> impl IntoElement {
    let mut row = div().id("theme-switcher").flex().gap_2();
    for preference in ChromePreference::ALL {
        let is_selected = preference == selected;
        let label = preference.label();
        row = row.child(
            div()
                .id(ElementId::Name(format!("theme-{label}").into()))
                .px_3()
                .py_1()
                .rounded_md()
                .border_1()
                .cursor_pointer()
                .when(is_selected, |this| {
                    this.bg(cx.theme().accent)
                        .text_color(cx.theme().accent_foreground)
                        .border_color(cx.theme().accent)
                        .font_semibold()
                })
                .when(!is_selected, |this| {
                    this.border_color(cx.theme().border)
                        .hover(|style| style.bg(cx.theme().accent.opacity(0.15)))
                })
                .on_click(cx.listener(move |app, _, window, cx| {
                    app.set_chrome_preference(preference, window, cx);
                }))
                .child(label),
        );
    }
    row
}

/// Secondary section banner when no primary tab is selected.
pub fn secondary_banner(title: &'static str, cx: &Context<App>) -> impl IntoElement {
    div()
        .id("secondary-banner")
        .px_2()
        .py_1()
        .rounded_md()
        .bg(cx.theme().muted.opacity(0.25))
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(format!("Secondary · {title}"))
}
