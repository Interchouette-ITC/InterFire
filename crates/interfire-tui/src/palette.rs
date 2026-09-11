//! Shared phoenix palette roles for the control-plane TUI (light and dark).
#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicU8, Ordering};

use ratatui::style::{Color, Modifier, Style};

const MODE_DARK: u8 = 0;
const MODE_LIGHT: u8 = 1;

static MODE: AtomicU8 = AtomicU8::new(MODE_DARK);

/// TUI chrome mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// Near-black canvas.
    Dark,
    /// Light canvas with the same orange brand.
    Light,
}

impl Mode {
    /// Parse `--theme=` values.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            _ => None,
        }
    }
}

/// Install the active TUI palette mode (call once at startup).
pub fn set_mode(mode: Mode) {
    MODE.store(
        match mode {
            Mode::Dark => MODE_DARK,
            Mode::Light => MODE_LIGHT,
        },
        Ordering::Relaxed,
    );
}

fn is_dark() -> bool {
    MODE.load(Ordering::Relaxed) != MODE_LIGHT
}

fn canvas() -> Color {
    if is_dark() {
        Color::Rgb(0x0e, 0x11, 0x14)
    } else {
        Color::Rgb(0xf5, 0xf6, 0xf8)
    }
}

fn surface() -> Color {
    if is_dark() {
        Color::Rgb(0x16, 0x1b, 0x20)
    } else {
        Color::Rgb(0xff, 0xff, 0xff)
    }
}

fn elevated() -> Color {
    if is_dark() {
        Color::Rgb(0x1e, 0x25, 0x2c)
    } else {
        Color::Rgb(0xec, 0xef, 0xf3)
    }
}

fn border_color() -> Color {
    if is_dark() {
        Color::Rgb(0x2c, 0x35, 0x40)
    } else {
        Color::Rgb(0xd0, 0xd7, 0xde)
    }
}

fn text() -> Color {
    if is_dark() {
        Color::Rgb(0xe8, 0xed, 0xf2)
    } else {
        Color::Rgb(0x1f, 0x23, 0x28)
    }
}

fn muted_fg() -> Color {
    if is_dark() {
        Color::Rgb(0x8b, 0x97, 0xa5)
    } else {
        Color::Rgb(0x65, 0x6d, 0x76)
    }
}

/// Brand accent `#F84800`.
pub const ACCENT: Color = Color::Rgb(0xf8, 0x48, 0x00);
/// On-accent.
pub const ON_ACCENT: Color = Color::Rgb(0xff, 0xff, 0xff);
/// Danger / deny.
pub const DANGER: Color = Color::Rgb(0xdc, 0x35, 0x45);
/// Success / protected.
pub const OK: Color = Color::Rgb(0x3d, 0xdc, 0x97);
/// Warning / degraded.
pub const WARN: Color = Color::Rgb(0xf5, 0xa6, 0x23);

/// Title / brand wordmark style.
#[must_use]
pub fn title() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Selected tab / list row.
#[must_use]
pub fn selected() -> Style {
    Style::default()
        .fg(ON_ACCENT)
        .bg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

/// Unselected chrome text.
#[must_use]
pub fn muted() -> Style {
    Style::default().fg(muted_fg())
}

/// Default body text.
#[must_use]
pub fn body() -> Style {
    Style::default().fg(text())
}

/// Allow / ok emphasis.
#[must_use]
pub fn allow() -> Style {
    Style::default()
        .fg(ON_ACCENT)
        .bg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

/// Deny / danger emphasis.
#[must_use]
pub fn deny() -> Style {
    Style::default()
        .fg(ON_ACCENT)
        .bg(DANGER)
        .add_modifier(Modifier::BOLD)
}

/// Focused field.
#[must_use]
pub fn focus() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Main pane / list panel.
#[must_use]
pub fn panel() -> Style {
    Style::default().fg(text()).bg(canvas())
}

/// Tab / status chrome strip.
#[must_use]
pub fn chrome() -> Style {
    Style::default().fg(text()).bg(surface())
}

/// Overlay card surface.
#[must_use]
pub fn overlay_panel() -> Style {
    Style::default().fg(text()).bg(elevated())
}

/// Accent-colored title for overlay frames.
#[must_use]
pub fn accent_label() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Border style for blocks.
#[must_use]
pub fn border() -> Style {
    Style::default().fg(border_color())
}

/// Status ok.
#[must_use]
pub fn ok() -> Style {
    Style::default().fg(OK).add_modifier(Modifier::BOLD)
}

/// Status warn.
#[must_use]
pub fn warn() -> Style {
    Style::default().fg(WARN).add_modifier(Modifier::BOLD)
}

/// Status error.
#[must_use]
pub fn err() -> Style {
    Style::default().fg(DANGER).add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_is_phoenix_orange() {
        assert_eq!(ACCENT, Color::Rgb(248, 72, 0));
        set_mode(Mode::Dark);
        let _ = (
            ok(),
            err(),
            warn(),
            overlay_panel(),
            chrome(),
            panel(),
            muted(),
            border(),
        );
        set_mode(Mode::Light);
        let _ = (body(), muted(), border());
        assert_eq!(Mode::parse("light"), Some(Mode::Light));
        assert_eq!(Mode::parse("dark"), Some(Mode::Dark));
        assert_eq!(Mode::parse("nope"), None);
    }
}
