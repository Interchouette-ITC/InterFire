//! Linux `StatusNotifierItem` host for `InterFire` tray states.
#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

use std::sync::mpsc::{self, Sender};
use std::thread;

use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use ksni::{MenuItem, ToolTip, Tray};

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
        self.state.icon_name().into()
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
            icon_name: self.state.icon_name().into(),
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
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}
