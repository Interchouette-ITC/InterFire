//! Linux `StatusNotifierItem` host for `InterFire` tray states.
#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, OnceLock};
use std::thread;

use image::GenericImageView;
use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, ToolTip, Tray};

use crate::tray::TrayState;

/// Commands from the GPUI app to the tray host thread.
#[derive(Clone, Copy, Debug)]
pub enum TrayCommand {
    /// Replace the published tray state.
    SetState(TrayState),
}

/// Best-effort Linux tray handle (None when SNI/D-Bus is unavailable).
#[derive(Clone)]
pub struct TrayHost {
    tx: Sender<TrayCommand>,
}

impl TrayHost {
    /// Spawn a background SNI tray. Returns `None` when registration fails.
    #[must_use]
    pub fn try_spawn(initial: TrayState) -> Option<Self> {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();

        thread::Builder::new()
            .name("interfire-tray".into())
            .spawn(move || {
                let tray = InterfireTray { state: initial };
                match tray.spawn() {
                    Ok(handle) => {
                        let _ = ready_tx.send(true);
                        while let Ok(command) = rx.recv() {
                            match command {
                                TrayCommand::SetState(state) => {
                                    handle.update(|item| {
                                        item.state = state;
                                    });
                                }
                            }
                        }
                    }
                    Err(_) => {
                        let _ = ready_tx.send(false);
                    }
                }
            })
            .ok()?;

        match ready_rx.recv() {
            Ok(true) => Some(Self { tx }),
            _ => None,
        }
    }

    /// Push a new tray state (ignored if the host thread ended).
    pub fn set_state(&self, state: TrayState) {
        let _ = self.tx.send(TrayCommand::SetState(state));
    }
}

struct InterfireTray {
    state: TrayState,
}

impl Tray for InterfireTray {
    fn id(&self) -> String {
        "interfire-ui".into()
    }

    fn title(&self) -> String {
        "InterFire".into()
    }

    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![brand_icon(self.state.brand_png()).as_ref().clone()]
    }

    fn attention_icon_name(&self) -> String {
        String::new()
    }

    fn attention_icon_pixmap(&self) -> Vec<Icon> {
        if matches!(self.state, TrayState::Prompting) {
            vec![brand_icon(self.state.brand_png()).as_ref().clone()]
        } else {
            Vec::new()
        }
    }

    fn status(&self) -> ksni::Status {
        match self.state {
            TrayState::Prompting => ksni::Status::NeedsAttention,
            TrayState::Protected | TrayState::Degraded | TrayState::Unavailable => {
                ksni::Status::Active
            }
        }
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "InterFire".into(),
            description: format!("{}: {}", self.state.label(), self.state.guidance()),
            icon_pixmap: self.icon_pixmap(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: format!("State: {}", self.state.label()),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit InterFire UI".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn brand_icon(png: &'static [u8]) -> Arc<Icon> {
    static PROTECTED: OnceLock<Arc<Icon>> = OnceLock::new();
    static PROMPTING: OnceLock<Arc<Icon>> = OnceLock::new();
    static DEGRADED: OnceLock<Arc<Icon>> = OnceLock::new();
    static UNAVAILABLE: OnceLock<Arc<Icon>> = OnceLock::new();

    let slot = if std::ptr::eq(png, crate::brand::icon_protected_png()) {
        &PROTECTED
    } else if std::ptr::eq(png, crate::brand::icon_prompting_png()) {
        &PROMPTING
    } else if std::ptr::eq(png, crate::brand::icon_degraded_png()) {
        &DEGRADED
    } else {
        &UNAVAILABLE
    };
    slot.get_or_init(|| Arc::new(png_to_argb_icon(png))).clone()
}

fn png_to_argb_icon(png: &[u8]) -> Icon {
    let img =
        image::load_from_memory_with_format(png, image::ImageFormat::Png).expect("brand tray png");
    let (width, height) = img.dimensions();
    let mut data = img.into_rgba8().into_vec();
    for pixel in data.chunks_exact_mut(4) {
        pixel.rotate_right(1);
    }
    Icon {
        width: i32::try_from(width).unwrap_or(64),
        height: i32::try_from(height).unwrap_or(64),
        data,
    }
}
