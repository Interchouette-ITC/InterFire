//! Shared desktop chrome: buttons, selects, play/pause, color semantics.
#![allow(clippy::wildcard_imports)]
#![forbid(unsafe_code)]

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::app::App;
use crate::filter::{ResultLimit, VerdictFilter};

/// Play glyph for start / resume.
pub const PLAY: &str = "▶";
/// Pause glyph for stop / pause.
pub const PAUSE: &str = "❚❚";

type ClickFn = fn(&mut App, &mut Window, &mut Context<App>);

/// Accent fill for on / active controls.
#[must_use]
pub fn on_fill(cx: &Context<App>) -> Hsla {
    cx.theme().accent
}

/// Muted label for off / inactive.
#[must_use]
pub fn off_fg(cx: &Context<App>) -> Hsla {
    cx.theme().muted_foreground
}

/// Primary filled action (Add rule, confirm).
pub fn primary_btn(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    cx: &Context<App>,
    on_click: ClickFn,
) -> impl IntoElement {
    styled_btn(id, label, enabled, true, false, cx, on_click)
}

/// Secondary outline action.
pub fn secondary_btn(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    cx: &Context<App>,
    on_click: ClickFn,
) -> impl IntoElement {
    styled_btn(id, label, enabled, false, false, cx, on_click)
}

/// Ghost / quiet action (Clear, menu items).
pub fn ghost_btn(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    cx: &Context<App>,
    on_click: ClickFn,
) -> impl IntoElement {
    styled_btn(id, label, enabled, false, true, cx, on_click)
}

fn styled_btn(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    primary: bool,
    ghost: bool,
    cx: &Context<App>,
    on_click: ClickFn,
) -> impl IntoElement {
    let accent = cx.theme().accent;
    let accent_fg = cx.theme().accent_foreground;
    let border = cx.theme().border;
    let muted = cx.theme().muted_foreground;
    let fg = cx.theme().foreground;
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .text_sm()
        .font_semibold()
        .when(enabled, Styled::cursor_pointer)
        .when(!enabled, |this| this.opacity(0.45).cursor_not_allowed())
        .when(primary && enabled, |this| {
            this.bg(accent)
                .text_color(accent_fg)
                .border_1()
                .border_color(accent)
        })
        .when(!primary && !ghost, |this| {
            this.border_1()
                .border_color(border)
                .text_color(fg)
                .hover(|style| style.bg(accent.opacity(0.12)))
        })
        .when(ghost, |this| {
            this.text_color(muted)
                .hover(|style| style.bg(accent.opacity(0.1)).text_color(fg))
        })
        .when(enabled, |this| {
            this.on_click(cx.listener(move |app, _, window, cx| on_click(app, window, cx)))
        })
        .child(label)
}

/// Play or pause icon button with short status label.
pub fn play_pause_control(
    id: &'static str,
    label: String,
    play: bool,
    enabled: bool,
    active_on: bool,
    cx: &Context<App>,
    on_click: ClickFn,
) -> impl IntoElement {
    let glyph = if play { PLAY } else { PAUSE };
    let label_color = if active_on { on_fill(cx) } else { off_fg(cx) };
    let btn_bg = if play && enabled {
        on_fill(cx)
    } else if enabled {
        cx.theme().secondary.opacity(0.85)
    } else {
        cx.theme().muted.opacity(0.5)
    };
    let glyph_fg = if play && enabled {
        cx.theme().accent_foreground
    } else {
        cx.theme().foreground
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(label_color)
                .child(label),
        )
        .child(
            div()
                .id(ElementId::Name(format!("{id}-glyph").into()))
                .w(px(28.))
                .h(px(28.))
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .bg(btn_bg)
                .text_color(glyph_fg)
                .text_xs()
                .font_semibold()
                .when(enabled, |this| {
                    this.cursor_pointer()
                        .on_click(cx.listener(move |app, _, window, cx| {
                            on_click(app, window, cx);
                        }))
                })
                .when(!enabled, |this| this.opacity(0.4).cursor_not_allowed())
                .child(glyph),
        )
}

/// Non-interactive status pill (matches tray vocabulary).
pub fn state_pill(label: &'static str, kind: PillKind, cx: &Context<App>) -> impl IntoElement {
    let (fill, fg) = match kind {
        PillKind::On => (cx.theme().accent.opacity(0.22), cx.theme().accent),
        PillKind::Off => (cx.theme().muted.opacity(0.35), off_fg(cx)),
        PillKind::Warn => (cx.theme().warning.opacity(0.2), cx.theme().warning),
        PillKind::Danger => (cx.theme().danger.opacity(0.2), cx.theme().danger),
        PillKind::Info => (cx.theme().accent.opacity(0.18), cx.theme().foreground),
    };
    div()
        .id("state-pill")
        .px_2()
        .py_1()
        .rounded_md()
        .bg(fill)
        .text_xs()
        .font_semibold()
        .text_color(fg)
        .child(label.to_uppercase())
}

/// Visual kind for [`state_pill`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PillKind {
    On,
    #[allow(dead_code)]
    Off,
    Warn,
    Danger,
    Info,
}

/// Compact select trigger + optional open menu for verdict.
pub fn verdict_select(current: VerdictFilter, open: bool, cx: &Context<App>) -> impl IntoElement {
    let mut col = div().id("verdict-select").relative().v_flex();
    col = col.child(select_trigger(
        "verdict-trigger",
        &format!("Verdict: {}", current.label()),
        open,
        cx,
        App::toggle_verdict_select,
    ));
    if open {
        let mut menu = div()
            .id("verdict-menu")
            .absolute()
            .bottom_full()
            .mb_1()
            .w(px(140.))
            .p_1()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .shadow_md()
            .v_flex()
            .gap_1();
        for verdict in VerdictFilter::ALL {
            menu = menu.child(select_option(
                match verdict {
                    VerdictFilter::All => "verdict-opt-all",
                    VerdictFilter::Allow => "verdict-opt-allow",
                    VerdictFilter::Deny => "verdict-opt-deny",
                    VerdictFilter::Prompt => "verdict-opt-prompt",
                },
                verdict.label(),
                current == verdict,
                cx,
                match verdict {
                    VerdictFilter::All => App::set_filter_verdict_all,
                    VerdictFilter::Allow => App::set_filter_verdict_allow,
                    VerdictFilter::Deny => App::set_filter_verdict_deny,
                    VerdictFilter::Prompt => App::set_filter_verdict_prompt,
                },
            ));
        }
        col = col.child(menu);
    }
    col
}

/// Compact select for result limit.
pub fn limit_select(current: ResultLimit, open: bool, cx: &Context<App>) -> impl IntoElement {
    let mut col = div().id("limit-select").relative().v_flex();
    col = col.child(select_trigger(
        "limit-trigger",
        &format!("Limit: {}", current.label()),
        open,
        cx,
        App::toggle_limit_select,
    ));
    if open {
        // Keep Select labels aligned with ResultLimit::PRESETS.
        debug_assert_eq!(ResultLimit::PRESETS, [50, 100, 200, 300]);
        let menu = div()
            .id("limit-menu")
            .absolute()
            .bottom_full()
            .mb_1()
            .w(px(160.))
            .p_1()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .shadow_md()
            .v_flex()
            .gap_1()
            .child(select_option(
                "limit-opt-50",
                "50",
                matches!(current, ResultLimit::Preset(50)),
                cx,
                App::set_filter_limit_50,
            ))
            .child(select_option(
                "limit-opt-100",
                "100",
                matches!(current, ResultLimit::Preset(100)),
                cx,
                App::set_filter_limit_100,
            ))
            .child(select_option(
                "limit-opt-200",
                "200",
                matches!(current, ResultLimit::Preset(200)),
                cx,
                App::set_filter_limit_200,
            ))
            .child(select_option(
                "limit-opt-300",
                "300",
                matches!(current, ResultLimit::Preset(300)),
                cx,
                App::set_filter_limit_300,
            ))
            .child(select_option(
                "limit-opt-all",
                "All",
                matches!(current, ResultLimit::All),
                cx,
                App::set_filter_limit_all,
            ))
            .child(select_option(
                "limit-opt-custom",
                "Custom…",
                matches!(current, ResultLimit::Custom(_)),
                cx,
                App::set_filter_limit_custom,
            ));
        col = col.child(menu);
    }
    col
}

fn select_trigger(
    id: &'static str,
    label: &str,
    open: bool,
    cx: &Context<App>,
    on_click: ClickFn,
) -> impl IntoElement {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(if open {
            cx.theme().accent
        } else {
            cx.theme().border
        })
        .bg(if open {
            cx.theme().accent.opacity(0.12)
        } else {
            cx.theme().popover
        })
        .text_xs()
        .cursor_pointer()
        .on_click(cx.listener(move |app, _, window, cx| on_click(app, window, cx)))
        .child(format!("{label} ▾"))
}

fn select_option(
    id: &'static str,
    label: &'static str,
    selected: bool,
    cx: &Context<App>,
    on_click: ClickFn,
) -> impl IntoElement {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_sm()
        .text_xs()
        .cursor_pointer()
        .when(selected, |this| {
            this.bg(cx.theme().accent.opacity(0.2))
                .font_semibold()
                .text_color(cx.theme().accent)
        })
        .when(!selected, |this| {
            this.hover(|style| style.bg(cx.theme().accent.opacity(0.1)))
        })
        .on_click(cx.listener(move |app, _, window, cx| on_click(app, window, cx)))
        .child(label)
}

/// App menu dropdown panel entry.
pub fn menu_item(
    id: &'static str,
    label: &'static str,
    cx: &Context<App>,
    on_click: ClickFn,
) -> impl IntoElement {
    div()
        .id(id)
        .w_full()
        .px_3()
        .py_2()
        .rounded_sm()
        .text_sm()
        .cursor_pointer()
        .hover(|style| style.bg(cx.theme().accent.opacity(0.12)))
        .on_click(cx.listener(move |app, _, window, cx| on_click(app, window, cx)))
        .child(label)
}
