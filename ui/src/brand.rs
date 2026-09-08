//! Embedded `InterFire` brand PNGs from `docs/brand/`.
#![forbid(unsafe_code)]

use std::sync::{Arc, OnceLock};

use gpui_kit::{Image, ImageFormat, ImageSource};

use crate::theme::ChromeMode;

const MARK_HEAD_64: &[u8] = include_bytes!("../../docs/brand/mark-phoenix-head-64.png");
const ICON_APP_64: &[u8] = include_bytes!("../../docs/brand/icon-app-phoenix-64.png");
const ICON_GRADIENT_64: &[u8] = include_bytes!("../../docs/brand/icon-app-phoenix-gradient-64.png");
const ICON_LIGHT_64: &[u8] = include_bytes!("../../docs/brand/icon-app-phoenix-light-64.png");
const ICON_MONO_64: &[u8] = include_bytes!("../../docs/brand/mark-phoenix-mono-64.png");
const LOGO_HORIZONTAL: &[u8] = include_bytes!("../../docs/brand/logo-horizontal-readme.png");

/// Nav mark for the active chrome mode.
#[must_use]
pub fn nav_mark_source(mode: ChromeMode) -> ImageSource {
    match mode {
        ChromeMode::Dark => {
            static IMAGE: OnceLock<Arc<Image>> = OnceLock::new();
            ImageSource::Image(IMAGE.get_or_init(|| png(MARK_HEAD_64)).clone())
        }
        ChromeMode::Light => {
            static IMAGE: OnceLock<Arc<Image>> = OnceLock::new();
            ImageSource::Image(IMAGE.get_or_init(|| png(ICON_LIGHT_64)).clone())
        }
    }
}

/// Compact horizontal lockup for Settings / About.
#[must_use]
pub fn logo_horizontal_source() -> ImageSource {
    static IMAGE: OnceLock<Arc<Image>> = OnceLock::new();
    ImageSource::Image(IMAGE.get_or_init(|| png(LOGO_HORIZONTAL)).clone())
}

/// Window / tray protected icon bytes.
#[must_use]
pub const fn icon_protected_png() -> &'static [u8] {
    ICON_APP_64
}

/// Tray prompting (attention) icon bytes.
#[must_use]
pub const fn icon_prompting_png() -> &'static [u8] {
    ICON_GRADIENT_64
}

/// Tray degraded icon bytes.
#[must_use]
pub const fn icon_degraded_png() -> &'static [u8] {
    ICON_LIGHT_64
}

/// Tray unavailable icon bytes.
#[must_use]
pub const fn icon_unavailable_png() -> &'static [u8] {
    ICON_MONO_64
}

fn png(bytes: &'static [u8]) -> Arc<Image> {
    Arc::new(Image::from_bytes(ImageFormat::Png, bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_pngs_are_nonempty() {
        assert!(MARK_HEAD_64.len() > 100);
        assert!(ICON_APP_64.len() > 100);
        assert!(ICON_GRADIENT_64.len() > 100);
        assert!(ICON_LIGHT_64.len() > 100);
        assert!(ICON_MONO_64.len() > 100);
        assert!(LOGO_HORIZONTAL.len() > 100);
        assert_eq!(icon_protected_png(), ICON_APP_64);
        let _ = nav_mark_source(ChromeMode::Dark);
        let _ = nav_mark_source(ChromeMode::Light);
    }
}
