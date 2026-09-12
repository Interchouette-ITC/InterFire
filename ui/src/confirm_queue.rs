//! Cross-thread pending confirm from tray menu to the main window.
#![forbid(unsafe_code)]

use std::sync::Mutex;

/// Operator confirmations shared by header chips and the tray menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfirmKind {
    RulesPause,
    RulesResume,
    TrafficBlock,
    TrafficUnblock,
    DaemonStop,
    DaemonStart,
}

static PENDING: Mutex<Option<ConfirmKind>> = Mutex::new(None);

/// Queue a confirm overlay for the next UI poll.
pub fn push(kind: ConfirmKind) {
    if let Ok(mut guard) = PENDING.lock() {
        *guard = Some(kind);
    }
}

/// Take a pending confirm, if any.
#[must_use]
pub fn take() -> Option<ConfirmKind> {
    PENDING.lock().ok().and_then(|mut guard| guard.take())
}
