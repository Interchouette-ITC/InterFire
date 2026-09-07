//! Tray state machine matching the desktop UX contract.
#![forbid(unsafe_code)]

use interfire_proto::DaemonStatus;

/// Desktop tray / chrome state for `interfire-ui`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayState {
    /// Rules and prompt policy active.
    Protected,
    /// Bounded prompt queue needs operator attention.
    Prompting,
    /// Observation or enforcement reported degraded.
    Degraded,
    /// Daemon socket unreachable; UI makes no policy claim.
    Unavailable,
}

impl TrayState {
    /// Derive tray state from a daemon link snapshot.
    #[must_use]
    pub fn from_link(link: &DaemonLink) -> Self {
        match link {
            DaemonLink::Down { .. } => Self::Unavailable,
            DaemonLink::Up {
                status,
                pending_prompts,
            } => {
                if *pending_prompts > 0 {
                    return Self::Prompting;
                }
                if is_degraded(status) {
                    return Self::Degraded;
                }
                Self::Protected
            }
        }
    }

    /// Short label for tray tooltip, menu, and status chrome.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Protected => "protected",
            Self::Prompting => "prompting",
            Self::Degraded => "degraded",
            Self::Unavailable => "daemon unavailable",
        }
    }

    /// Operator guidance shown in Status when this state is active.
    #[must_use]
    pub const fn guidance(self) -> &'static str {
        match self {
            Self::Protected => "Daemon reachable; rules and prompt policy are active.",
            Self::Prompting => {
                "Pending connection prompts need Allow/Deny (once|session|permanent)."
            }
            Self::Degraded => {
                "Observation or enforcement is degraded. Check capabilities and InterFire nft queue."
            }
            Self::Unavailable => {
                "Cannot reach the daemon socket. Start interfired, check the socket path, then reconnect."
            }
        }
    }

    /// Freedesktop icon name for `StatusNotifierItem` (theme icons).
    #[must_use]
    pub const fn icon_name(self) -> &'static str {
        match self {
            Self::Protected => "security-high",
            Self::Prompting => "dialog-question",
            Self::Degraded => "dialog-warning",
            Self::Unavailable => "dialog-error",
        }
    }
}

/// Latest daemon reachability and status used to drive the tray.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DaemonLink {
    /// Socket connect or protocol poll failed.
    Down {
        /// Last error text (for Status UI).
        reason: String,
    },
    /// Daemon answered status (and optional prompt count).
    Up {
        status: DaemonStatus,
        pending_prompts: usize,
    },
}

fn is_degraded(status: &DaemonStatus) -> bool {
    status.observation == "degraded" || status.enforcement == "degraded"
}

#[cfg(test)]
mod tests {
    use super::{DaemonLink, TrayState};
    use interfire_proto::DaemonStatus;

    fn status(enforcement: &str, observation: &str) -> DaemonStatus {
        DaemonStatus {
            enforcement: enforcement.into(),
            observation: observation.into(),
            ipc_version: 1,
        }
    }

    #[test]
    fn four_states_from_daemon_link() {
        assert_eq!(
            TrayState::from_link(&DaemonLink::Down {
                reason: "connect".into()
            }),
            TrayState::Unavailable
        );
        assert_eq!(
            TrayState::from_link(&DaemonLink::Up {
                status: status("rules", "attached"),
                pending_prompts: 0,
            }),
            TrayState::Protected
        );
        assert_eq!(
            TrayState::from_link(&DaemonLink::Up {
                status: status("rules", "attached"),
                pending_prompts: 2,
            }),
            TrayState::Prompting
        );
        assert_eq!(
            TrayState::from_link(&DaemonLink::Up {
                status: status("none", "degraded"),
                pending_prompts: 0,
            }),
            TrayState::Degraded
        );
    }

    #[test]
    fn prompting_outranks_degraded_when_queue_nonempty() {
        assert_eq!(
            TrayState::from_link(&DaemonLink::Up {
                status: status("none", "degraded"),
                pending_prompts: 1,
            }),
            TrayState::Prompting
        );
    }

    #[test]
    fn unavailable_guidance_mentions_reconnect() {
        let text = TrayState::Unavailable.guidance();
        assert!(text.contains("socket"));
        assert!(text.contains("reconnect") || text.contains("Start interfired"));
    }
}
