//! Tray state machine matching the desktop UX contract.
#![forbid(unsafe_code)]

use interfire_proto::DaemonStatus;

/// Desktop tray / chrome state for `interfire-ui`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayState {
    /// Rules and prompt policy active; traffic open.
    Protected,
    /// Bounded prompt queue needs operator attention.
    Prompting,
    /// Observation or enforcement reported degraded.
    Degraded,
    /// Operator paused rules; owned queue table absent (traffic may still be blocked).
    Paused,
    /// Traffic kill-switch active (fail-closed drop except loopback).
    Blocked,
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
                if status.traffic == "blocked" {
                    return Self::Blocked;
                }
                if status.enforcement == "paused" {
                    return Self::Paused;
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
            Self::Paused => "paused",
            Self::Blocked => "blocked",
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
            Self::Paused => {
                "Rules paused: owned queue table removed; new TCP is not filtered. Use Rules Start to enforce."
            }
            Self::Blocked => {
                "Traffic blocked (machine or user kill-switch). Open the Traffic tab to adjust scope and direction."
            }
            Self::Unavailable => {
                "Cannot reach the daemon socket. Start interfired (Daemon Start), check the socket path, then reconnect."
            }
        }
    }

    /// Embedded brand PNG for tray / attention icons (not stock dialog names).
    #[must_use]
    pub const fn brand_png(self) -> &'static [u8] {
        match self {
            Self::Protected => crate::brand::icon_protected_png(),
            Self::Prompting => crate::brand::icon_prompting_png(),
            Self::Degraded | Self::Paused | Self::Blocked => crate::brand::icon_degraded_png(),
            Self::Unavailable => crate::brand::icon_unavailable_png(),
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

    fn status(enforcement: &str, observation: &str, traffic: &str) -> DaemonStatus {
        DaemonStatus {
            enforcement: enforcement.into(),
            observation: observation.into(),
            traffic: traffic.into(),
            traffic_machine: if traffic == "blocked" {
                "out".into()
            } else {
                "open".into()
            },
            traffic_user: "open".into(),
            traffic_effective: if traffic == "blocked" {
                "machine:out".into()
            } else {
                "open".into()
            },
            ipc_version: 1,
            pid: None,
            rss_kib: None,
            cpu_jiffies: None,
        }
    }

    #[test]
    fn states_from_daemon_link() {
        assert_eq!(
            TrayState::from_link(&DaemonLink::Down {
                reason: "connect".into()
            }),
            TrayState::Unavailable
        );
        assert_eq!(
            TrayState::from_link(&DaemonLink::Up {
                status: status("nfqueue", "attached", "open"),
                pending_prompts: 0,
            }),
            TrayState::Protected
        );
        assert_eq!(
            TrayState::from_link(&DaemonLink::Up {
                status: status("paused", "attached", "blocked"),
                pending_prompts: 0,
            }),
            TrayState::Blocked
        );
        assert_eq!(
            TrayState::from_link(&DaemonLink::Up {
                status: status("paused", "attached", "open"),
                pending_prompts: 0,
            }),
            TrayState::Paused
        );
    }

    #[test]
    fn prompting_outranks_blocked() {
        assert_eq!(
            TrayState::from_link(&DaemonLink::Up {
                status: status("paused", "degraded", "blocked"),
                pending_prompts: 1,
            }),
            TrayState::Prompting
        );
    }

    #[test]
    fn unavailable_guidance_mentions_reconnect() {
        let text = TrayState::Unavailable.guidance();
        assert!(text.contains("socket"));
        assert!(text.contains("reconnect") || text.contains("Start"));
    }
}
