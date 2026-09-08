//! `InterFire` phoenix theme tokens for the GPUI shell.
#![forbid(unsafe_code)]

use gpui_kit::component::{Colorize, Theme, ThemeMode};
use gpui_kit::{App, Hsla, hsla};

/// Brand accent from phoenix artwork (`#F84800`).
pub const ACCENT: &str = "#F84800";
/// Hot accent / hover (`#F03105`).
pub const ACCENT_HOT: &str = "#F03105";
/// Deep brand red (`#D00000`).
pub const BRAND_DEEP: &str = "#D00000";
/// Near-black canvas (`#0E1114`).
pub const CANVAS: &str = "#0E1114";
/// Primary surface (`#161B20`).
pub const SURFACE: &str = "#161B20";
/// Elevated / hover surface (`#1E252C`).
pub const ELEVATED: &str = "#1E252C";
/// Border (`#2C3540`).
pub const BORDER: &str = "#2C3540";
/// Primary text (`#E8EDF2`).
pub const TEXT: &str = "#E8EDF2";
/// Muted text (`#8B97A5`).
pub const MUTED: &str = "#8B97A5";
/// On-accent / on-danger label (`#FFFFFF`).
pub const ON_ACCENT: &str = "#FFFFFF";
/// Danger (`#EF5350`).
pub const DANGER: &str = "#EF5350";
/// Strong deny (`#DC3545`).
pub const DANGER_STRONG: &str = "#DC3545";
/// Success / protected (`#3DDC97`).
pub const OK: &str = "#3DDC97";
/// Warning / degraded (`#F5A623`).
pub const WARN: &str = "#F5A623";

struct PhoenixColors {
    canvas: Hsla,
    surface: Hsla,
    elevated: Hsla,
    border: Hsla,
    text: Hsla,
    muted_fg: Hsla,
    accent: Hsla,
    accent_hot: Hsla,
    brand_deep: Hsla,
    on_accent: Hsla,
    danger: Hsla,
    danger_strong: Hsla,
    ok: Hsla,
    warn: Hsla,
    list_active: Hsla,
    accent_soft: Hsla,
    scrim: Hsla,
}

impl PhoenixColors {
    fn lock() -> Self {
        let accent = hex(ACCENT);
        Self {
            canvas: hex(CANVAS),
            surface: hex(SURFACE),
            elevated: hex(ELEVATED),
            border: hex(BORDER),
            text: hex(TEXT),
            muted_fg: hex(MUTED),
            accent,
            accent_hot: hex(ACCENT_HOT),
            brand_deep: hex(BRAND_DEEP),
            on_accent: hex(ON_ACCENT),
            danger: hex(DANGER),
            danger_strong: hex(DANGER_STRONG),
            ok: hex(OK),
            warn: hex(WARN),
            list_active: accent.opacity(0.28),
            accent_soft: accent.opacity(0.22),
            scrim: hsla(0.0, 0.0, 0.0, 0.72),
        }
    }
}

/// Parse a locked brand hex; panics only on programmer error.
#[must_use]
pub fn hex(value: &str) -> Hsla {
    Hsla::parse_hex(value).unwrap_or_else(|_| panic!("invalid brand hex: {value}"))
}

/// Apply dark phoenix chrome after `gpui_kit::init`.
pub fn apply_phoenix_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    paint_phoenix(Theme::global_mut(cx));
    Theme::sync_base(cx);
}

fn paint_phoenix(theme: &mut Theme) {
    let colors = PhoenixColors::lock();
    paint_base(theme, &colors);
    paint_brand_actions(theme, &colors);
    paint_sidebar_and_lists(theme, &colors);
    paint_chrome(theme, &colors);
}

fn paint_base(theme: &mut Theme, c: &PhoenixColors) {
    theme.background = c.canvas;
    theme.foreground = c.text;
    theme.accordion = c.canvas;
    theme.border = c.border;
    theme.input = c.border;
    theme.ring = c.accent;
    theme.caret = c.text;
    theme.selection = c.accent.opacity(0.45);
    theme.muted = c.elevated;
    theme.muted_foreground = c.muted_fg;
    theme.popover = c.elevated;
    theme.popover_foreground = c.text;
    theme.group_box = c.surface;
    theme.group_box_foreground = c.text;
    theme.skeleton = c.elevated;
    theme.overlay = c.scrim;
    theme.window_border = c.border;
}

fn paint_brand_actions(theme: &mut Theme, c: &PhoenixColors) {
    theme.accent = c.accent;
    theme.accent_foreground = c.on_accent;
    theme.primary = c.accent;
    theme.primary_hover = c.accent_hot;
    theme.primary_active = c.brand_deep;
    theme.primary_foreground = c.on_accent;
    theme.secondary = c.elevated;
    theme.secondary_hover = c.border;
    theme.secondary_active = c.border;
    theme.secondary_foreground = c.text;

    theme.button = c.elevated;
    theme.button_hover = c.border;
    theme.button_active = c.border;
    theme.button_foreground = c.text;
    theme.button_primary = c.accent;
    theme.button_primary_hover = c.accent_hot;
    theme.button_primary_active = c.brand_deep;
    theme.button_primary_foreground = c.on_accent;
    theme.button_secondary = c.elevated;
    theme.button_secondary_hover = c.border;
    theme.button_secondary_active = c.border;
    theme.button_secondary_foreground = c.text;
    theme.button_danger = c.danger_strong;
    theme.button_danger_hover = c.danger;
    theme.button_danger_active = c.brand_deep;
    theme.button_danger_foreground = c.on_accent;

    theme.danger = c.danger_strong;
    theme.danger_hover = c.danger;
    theme.danger_active = c.brand_deep;
    theme.danger_foreground = c.on_accent;
    theme.success = c.ok;
    theme.success_hover = c.ok;
    theme.success_active = c.ok;
    theme.success_foreground = c.canvas;
    theme.warning = c.warn;
    theme.warning_hover = c.warn;
    theme.warning_active = c.warn;
    theme.warning_foreground = c.canvas;
    theme.info = c.accent;
    theme.info_hover = c.accent_hot;
    theme.info_active = c.brand_deep;
    theme.info_foreground = c.on_accent;
}

fn paint_sidebar_and_lists(theme: &mut Theme, c: &PhoenixColors) {
    theme.sidebar = c.canvas;
    theme.sidebar_foreground = c.text;
    theme.sidebar_border = c.border;
    theme.sidebar_accent = c.accent;
    theme.sidebar_accent_foreground = c.on_accent;
    theme.sidebar_primary = c.accent;
    theme.sidebar_primary_foreground = c.on_accent;

    theme.colors.list = c.canvas;
    theme.colors.list_even = c.surface.opacity(0.55);
    theme.colors.list_head = c.surface.opacity(0.55);
    theme.colors.list_hover = c.elevated;
    theme.colors.list_active = c.list_active;
    theme.colors.list_active_border = c.accent;

    theme.table = c.canvas;
    theme.table_even = c.surface.opacity(0.55);
    theme.table_head = c.surface;
    theme.table_head_foreground = c.muted_fg;
    theme.table_hover = c.elevated;
    theme.table_active = c.list_active;
    theme.table_active_border = c.accent;
    theme.table_row_border = c.border.opacity(0.7);
}

fn paint_chrome(theme: &mut Theme, c: &PhoenixColors) {
    theme.tab = hsla(0.0, 0.0, 0.0, 0.0);
    theme.tab_foreground = c.muted_fg;
    theme.tab_active = c.surface;
    theme.tab_active_foreground = c.text;
    theme.tab_bar = c.surface;
    theme.tab_bar_segmented = c.surface;
    theme.title_bar = c.surface;
    theme.title_bar_border = c.border;
    theme.status_bar = c.surface;
    theme.status_bar_border = c.border;
    theme.tiles = c.surface;
    theme.progress_bar = c.accent;
    theme.slider_bar = c.accent;
    theme.slider_thumb = c.on_accent;
    theme.switch = c.border;
    theme.switch_thumb = c.on_accent;
    theme.scrollbar = hsla(0.0, 0.0, 0.0, 0.0);
    theme.scrollbar_thumb = c.muted_fg.opacity(0.55);
    theme.scrollbar_thumb_hover = c.muted_fg;
    theme.link = c.text;
    theme.link_hover = c.accent;
    theme.link_active = c.accent_hot;
    theme.drag_border = c.accent;
    theme.drop_target = c.accent_soft;
    theme.description_list_label = c.elevated;
    theme.description_list_label_foreground = c.text;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_hex_tokens_parse() {
        for value in [
            ACCENT,
            ACCENT_HOT,
            BRAND_DEEP,
            CANVAS,
            SURFACE,
            ELEVATED,
            BORDER,
            TEXT,
            MUTED,
            ON_ACCENT,
            DANGER,
            DANGER_STRONG,
            OK,
            WARN,
        ] {
            let _ = hex(value);
        }
    }
}
