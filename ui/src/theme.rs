//! `InterFire` phoenix theme tokens for the GPUI shell (light and dark).
#![forbid(unsafe_code)]

use gpui_kit::component::{Colorize, Theme, ThemeMode};
use gpui_kit::{App, Hsla, Window, WindowAppearance, hsla};

/// Brand accent from phoenix artwork (`#F84800`).
pub const ACCENT: &str = "#F84800";
/// Hot accent / hover (`#F03105`).
pub const ACCENT_HOT: &str = "#F03105";
/// Deep brand red (`#D00000`).
pub const BRAND_DEEP: &str = "#D00000";
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

/// Dark canvas (`#0E1114`).
pub const DARK_CANVAS: &str = "#0E1114";
/// Dark surface (`#161B20`).
pub const DARK_SURFACE: &str = "#161B20";
/// Dark elevated (`#1E252C`).
pub const DARK_ELEVATED: &str = "#1E252C";
/// Dark border (`#2C3540`).
pub const DARK_BORDER: &str = "#2C3540";
/// Dark text (`#E8EDF2`).
pub const DARK_TEXT: &str = "#E8EDF2";
/// Dark muted (`#8B97A5`).
pub const DARK_MUTED: &str = "#8B97A5";

/// Light canvas (`#F5F6F8`).
pub const LIGHT_CANVAS: &str = "#F5F6F8";
/// Light surface (`#FFFFFF`).
pub const LIGHT_SURFACE: &str = "#FFFFFF";
/// Light elevated (`#ECEFF3`).
pub const LIGHT_ELEVATED: &str = "#ECEFF3";
/// Light border (`#D0D7DE`).
pub const LIGHT_BORDER: &str = "#D0D7DE";
/// Light text (`#1F2328`).
pub const LIGHT_TEXT: &str = "#1F2328";
/// Light muted (`#656D76`).
pub const LIGHT_MUTED: &str = "#656D76";

/// Resolved phoenix appearance (what is painted right now).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChromeMode {
    /// Near-black ops chrome.
    Dark,
    /// Light ops chrome with the same orange brand.
    Light,
}

impl ChromeMode {
    /// Map a window / app appearance to phoenix chrome.
    #[must_use]
    pub const fn from_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::Light,
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::Dark,
        }
    }

    /// Short Settings label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }
}

/// User preference for theme (Settings switcher).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ChromePreference {
    /// Follow the desktop appearance.
    #[default]
    System,
    /// Force light phoenix chrome.
    Light,
    /// Force dark phoenix chrome.
    Dark,
}

impl ChromePreference {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// Short Settings chip label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    /// Resolve to a concrete light/dark mode.
    #[must_use]
    pub const fn resolve(self, appearance: WindowAppearance) -> ChromeMode {
        match self {
            Self::System => ChromeMode::from_appearance(appearance),
            Self::Light => ChromeMode::Light,
            Self::Dark => ChromeMode::Dark,
        }
    }
}

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
    success_fg: Hsla,
}

impl PhoenixColors {
    fn for_mode(mode: ChromeMode) -> Self {
        let accent = hex(ACCENT);
        match mode {
            ChromeMode::Dark => Self {
                canvas: hex(DARK_CANVAS),
                surface: hex(DARK_SURFACE),
                elevated: hex(DARK_ELEVATED),
                border: hex(DARK_BORDER),
                text: hex(DARK_TEXT),
                muted_fg: hex(DARK_MUTED),
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
                success_fg: hex(DARK_CANVAS),
            },
            ChromeMode::Light => Self {
                canvas: hex(LIGHT_CANVAS),
                surface: hex(LIGHT_SURFACE),
                elevated: hex(LIGHT_ELEVATED),
                border: hex(LIGHT_BORDER),
                text: hex(LIGHT_TEXT),
                muted_fg: hex(LIGHT_MUTED),
                accent,
                accent_hot: hex(ACCENT_HOT),
                brand_deep: hex(BRAND_DEEP),
                on_accent: hex(ON_ACCENT),
                danger: hex(DANGER),
                danger_strong: hex(DANGER_STRONG),
                ok: hex(OK),
                warn: hex(WARN),
                list_active: accent.opacity(0.18),
                accent_soft: accent.opacity(0.14),
                scrim: hsla(0.0, 0.0, 0.0, 0.45),
                success_fg: hex(LIGHT_TEXT),
            },
        }
    }
}

/// Parse a locked brand hex; panics only on programmer error.
#[must_use]
pub fn hex(value: &str) -> Hsla {
    Hsla::parse_hex(value).unwrap_or_else(|_| panic!("invalid brand hex: {value}"))
}

/// Apply phoenix chrome for `mode` after `gpui_kit::init`.
pub fn apply_phoenix_theme(mode: ChromeMode, window: Option<&mut Window>, cx: &mut App) {
    let kit_mode = match mode {
        ChromeMode::Dark => ThemeMode::Dark,
        ChromeMode::Light => ThemeMode::Light,
    };
    Theme::change(kit_mode, None, cx);
    paint_phoenix(Theme::global_mut(cx), mode);
    Theme::sync_base(cx);
    if let Some(window) = window {
        window.refresh();
    }
}

fn paint_phoenix(theme: &mut Theme, mode: ChromeMode) {
    let colors = PhoenixColors::for_mode(mode);
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
    theme.success_foreground = c.success_fg;
    theme.warning = c.warn;
    theme.warning_hover = c.warn;
    theme.warning_active = c.warn;
    theme.warning_foreground = c.success_fg;
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
            DARK_CANVAS,
            DARK_SURFACE,
            DARK_ELEVATED,
            DARK_BORDER,
            DARK_TEXT,
            DARK_MUTED,
            LIGHT_CANVAS,
            LIGHT_SURFACE,
            LIGHT_ELEVATED,
            LIGHT_BORDER,
            LIGHT_TEXT,
            LIGHT_MUTED,
            ON_ACCENT,
            DANGER,
            DANGER_STRONG,
            OK,
            WARN,
        ] {
            let _ = hex(value);
        }
    }

    #[test]
    fn preference_resolves_system_and_forced() {
        assert_eq!(
            ChromePreference::Light.resolve(WindowAppearance::Dark),
            ChromeMode::Light
        );
        assert_eq!(
            ChromePreference::Dark.resolve(WindowAppearance::Light),
            ChromeMode::Dark
        );
        assert_eq!(
            ChromePreference::System.resolve(WindowAppearance::Light),
            ChromeMode::Light
        );
        assert_eq!(
            ChromePreference::System.resolve(WindowAppearance::Dark),
            ChromeMode::Dark
        );
    }
}
