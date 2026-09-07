//! Stable, dependency-free IPC framing for the first daemon/CLI slice.
#![forbid(unsafe_code)]

pub const IPC_VERSION: u16 = 1;
pub const MAX_FRAME_BYTES: usize = 8 * 1024;
pub const MAX_LOG_RECORDS_PER_SUBSCRIBER: usize = 2_000;
pub const MAX_PENDING_PROMPTS: usize = 100;

/// PID is never a durable identity: the daemon pairs it with start ticks read
/// from `/proc/<pid>/stat` before retaining a process record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub start_ticks: u64,
    pub executable: String,
    pub uid: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportProtocol {
    Tcp,
    Udp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Destination {
    pub address: String,
    pub hostname: Option<String>,
    pub port: u16,
    pub protocol: TransportProtocol,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionEvent {
    pub process: ProcessIdentity,
    pub destination: Destination,
    pub observed_at_millis: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuleScope {
    Once,
    Session,
    Permanent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromptState {
    Pending,
    Answered,
    Expired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DaemonState {
    FeasibilityGated,
    Protected,
    Degraded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedLogRecord {
    pub sequence: u64,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Request {
    Ping,
    Status,
    RuleList,
    RuleAdd {
        id: u64,
        executable: String,
        verdict: String,
        port: u16,
    },
    RuleDelete {
        id: u64,
    },
}

impl Request {
    /// Parse one newline-terminated `v1 …` control frame.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::UnsupportedVersion`] when the frame is not `v1`,
    /// or [`ProtocolError::Malformed`] when the command or fields are invalid.
    pub fn parse(frame: &str) -> Result<Self, ProtocolError> {
        let mut fields = frame.split_whitespace();
        let version = fields.next().ok_or(ProtocolError::Malformed)?;
        if version != "v1" {
            return Err(ProtocolError::UnsupportedVersion);
        }
        match fields.next() {
            Some("ping") if fields.next().is_none() => Ok(Self::Ping),
            Some("status") if fields.next().is_none() => Ok(Self::Status),
            Some("rule-list") if fields.next().is_none() => Ok(Self::RuleList),
            Some("rule-delete") => {
                let id = fields.next().and_then(|value| value.parse().ok());
                if fields.next().is_none() {
                    id.map(|id| Self::RuleDelete { id })
                        .ok_or(ProtocolError::Malformed)
                } else {
                    Err(ProtocolError::Malformed)
                }
            }
            Some("rule-add") => {
                let id = fields.next().and_then(|value| value.parse().ok());
                let executable = fields.next().map(str::to_owned);
                let verdict = fields.next().map(str::to_owned);
                let port = fields.next().and_then(|value| value.parse().ok());
                if fields.next().is_some() {
                    return Err(ProtocolError::Malformed);
                }
                match (id, executable, verdict, port) {
                    (Some(id), Some(executable), Some(verdict), Some(port)) => Ok(Self::RuleAdd {
                        id,
                        executable,
                        verdict,
                        port,
                    }),
                    _ => Err(ProtocolError::Malformed),
                }
            }
            _ => Err(ProtocolError::Malformed),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Response {
    Pong,
    Status {
        /// Verdict path state (`none`, `nfqueue`, or `degraded`).
        enforcement: &'static str,
        /// eBPF observation state (`attached` or `degraded`).
        observation: &'static str,
        ipc_version: u16,
    },
    Error(&'static str),
    Rules(String),
}

impl Response {
    /// Encode a response as one newline-terminated control frame.
    #[must_use]
    pub fn encode(&self) -> String {
        match self {
            Self::Pong => "v1 pong\n".into(),
            Self::Status {
                enforcement,
                observation,
                ipc_version,
            } => format!(
                "v1 status enforcement={enforcement} observation={observation} ipc_version={ipc_version}\n"
            ),
            Self::Error(message) => format!("v1 error {message}\n"),
            Self::Rules(value) => format!("v1 rules {value}\n"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    Malformed,
    UnsupportedVersion,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_requests() {
        assert_eq!(Request::parse("v1 ping\n"), Ok(Request::Ping));
        assert_eq!(Request::parse("v1 status\n"), Ok(Request::Status));
    }

    #[test]
    fn rejects_unknown_versions() {
        assert_eq!(
            Request::parse("v9 ping\n"),
            Err(ProtocolError::UnsupportedVersion)
        );
    }

    #[test]
    fn ping_fixture_is_stable() {
        let fixture = include_str!("../../../fixtures/ipc/v1-ping.request");
        assert_eq!(Request::parse(fixture), Ok(Request::Ping));
    }

    #[test]
    fn status_response_includes_observation() {
        let frame = Response::Status {
            enforcement: "none",
            observation: "degraded",
            ipc_version: IPC_VERSION,
        }
        .encode();
        assert_eq!(
            frame,
            "v1 status enforcement=none observation=degraded ipc_version=1\n"
        );
    }
}
