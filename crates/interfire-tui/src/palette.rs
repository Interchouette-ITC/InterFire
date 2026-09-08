//! Shared phoenix palette roles for the control-plane TUI.
#![forbid(unsafe_code)]

use ratatui::style::{Color, Modifier, Style};

/// Near-black canvas.
pub const CANVAS: Color = Color::Rgb(0x0e, 0x11, 0x14);
/// Elevated surface.
pub const SURFACE: Color = Color::Rgb(0x16, 0x1b, 0x20);
/// Panel / hover.
pub const ELEVATED: Color = Color::Rgb(0x1e, 0x25, 0x2c);
/// Border tone.
pub const BORDER: Color = Color::Rgb(0x2c, 0x35, 0x40);
/// Primary text.
pub const TEXT: Color = Color::Rgb(0xe8, 0xed, 0xf2);
/// Muted text.
pub const MUTED: Color = Color::Rgb(0x8b, 0x97, 0xa5);
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
    Style::default().fg(MUTED)
}

/// Default body text.
#[must_use]
pub fn body() -> Style {
    Style::default().fg(TEXT)
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
    Style::default().fg(TEXT).bg(CANVAS)
}

/// Tab / status chrome strip.
#[must_use]
pub fn chrome() -> Style {
    Style::default().fg(TEXT).bg(SURFACE)
}

/// Overlay card surface.
#[must_use]
pub fn overlay_panel() -> Style {
    Style::default().fg(TEXT).bg(ELEVATED)
}

/// Accent-colored title for overlay frames.
#[must_use]
pub fn accent_label() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Border style for blocks.
#[must_use]
pub fn border() -> Style {
    Style::default().fg(BORDER)
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
        assert_eq!(SURFACE, Color::Rgb(0x16, 0x1b, 0x20));
        let _ = (ok(), err(), warn(), overlay_panel());
    }
}
