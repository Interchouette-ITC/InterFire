//! Versioned, bounded Unix-socket IPC framing for daemon and clients.
#![forbid(unsafe_code)]

pub const IPC_VERSION: u16 = 1;
pub const MAX_FRAME_BYTES: usize = 8 * 1024;
pub const MAX_LOG_RECORDS_PER_SUBSCRIBER: usize = 2_000;
pub const MAX_PENDING_PROMPTS: usize = 100;
pub const MAX_DNS_ENTRIES: usize = 2_048;
pub const MAX_AUDIT_FILE_BYTES: u64 = 1_048_576;

/// Default daemon listen path.
pub const DEFAULT_SOCKET_PATH: &str = "/run/interfire/interfired.sock";
/// Default durable rules path.
pub const DEFAULT_RULES_PATH: &str = "/etc/interfire/rules.toml";
/// Default on-disk audit log path.
pub const DEFAULT_AUDIT_PATH: &str = "/var/lib/interfire/audit.log";

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
    PromptList,
    PromptAnswer {
        id: u64,
        verdict: String,
        scope: String,
    },
    DnsList,
    DnsNote {
        hostname: String,
        ipv4: String,
        ttl_secs: Option<u64>,
    },
    AuditTail {
        limit: usize,
    },
    AuditSubscribe {
        id: String,
        since: u64,
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
            Some("prompt-list") if fields.next().is_none() => Ok(Self::PromptList),
            Some("prompt-answer") => {
                let id = fields.next().and_then(|value| value.parse().ok());
                let verdict = fields.next().map(str::to_owned);
                let scope = fields.next().map(str::to_owned);
                if fields.next().is_some() {
                    return Err(ProtocolError::Malformed);
                }
                match (id, verdict, scope) {
                    (Some(id), Some(verdict), Some(scope)) => {
                        Ok(Self::PromptAnswer { id, verdict, scope })
                    }
                    _ => Err(ProtocolError::Malformed),
                }
            }
            Some("dns-list") if fields.next().is_none() => Ok(Self::DnsList),
            Some("dns-note") => {
                let hostname = fields.next().map(str::to_owned);
                let ipv4 = fields.next().map(str::to_owned);
                let ttl_secs = match fields.next() {
                    Some(value) => Some(value.parse().map_err(|_| ProtocolError::Malformed)?),
                    None => None,
                };
                if fields.next().is_some() {
                    return Err(ProtocolError::Malformed);
                }
                match (hostname, ipv4) {
                    (Some(hostname), Some(ipv4)) => Ok(Self::DnsNote {
                        hostname,
                        ipv4,
                        ttl_secs,
                    }),
                    _ => Err(ProtocolError::Malformed),
                }
            }
            Some("audit-tail") => {
                let limit = match fields.next() {
                    Some(value) => value.parse().map_err(|_| ProtocolError::Malformed)?,
                    None => MAX_LOG_RECORDS_PER_SUBSCRIBER,
                };
                if fields.next().is_some() {
                    return Err(ProtocolError::Malformed);
                }
                Ok(Self::AuditTail { limit })
            }
            Some("audit-subscribe") => {
                let id = fields.next().map(str::to_owned);
                let since = match fields.next() {
                    Some(value) => value
                        .strip_prefix("since=")
                        .and_then(|value| value.parse().ok())
                        .ok_or(ProtocolError::Malformed)?,
                    None => 0,
                };
                if fields.next().is_some() {
                    return Err(ProtocolError::Malformed);
                }
                id.map(|id| Self::AuditSubscribe { id, since })
                    .ok_or(ProtocolError::Malformed)
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
    Prompts(String),
    Dns(String),
    Audit(String),
    Subscribed(String),
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
            Self::Prompts(value) => format!("v1 prompts {value}\n"),
            Self::Dns(value) => format!("v1 dns {value}\n"),
            Self::Audit(value) => format!("v1 audit-tail {value}\n"),
            Self::Subscribed(id) => format!("v1 subscribed {id}\n"),
        }
    }
}

/// Parsed daemon status fields (owned; for clients).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonStatus {
    pub enforcement: String,
    pub observation: String,
    pub ipc_version: u16,
}

impl DaemonStatus {
    /// Parse a `v1 status …` response frame.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] when the frame is not a well-formed status line.
    pub fn parse(frame: &str) -> Result<Self, ProtocolError> {
        let line = frame.trim_end_matches(['\r', '\n']);
        let mut fields = line.split_whitespace();
        match fields.next() {
            Some("v1") => {}
            Some(token) if token.starts_with('v') => return Err(ProtocolError::UnsupportedVersion),
            _ => return Err(ProtocolError::Malformed),
        }
        if fields.next() != Some("status") {
            return Err(ProtocolError::Malformed);
        }
        let mut enforcement = None;
        let mut observation = None;
        let mut ipc_version = None;
        for field in fields {
            if let Some(value) = field.strip_prefix("enforcement=") {
                enforcement = Some(value.to_owned());
            } else if let Some(value) = field.strip_prefix("observation=") {
                observation = Some(value.to_owned());
            } else if let Some(value) = field.strip_prefix("ipc_version=") {
                ipc_version = Some(value.parse().map_err(|_| ProtocolError::Malformed)?);
            } else {
                return Err(ProtocolError::Malformed);
            }
        }
        Ok(Self {
            enforcement: enforcement.ok_or(ProtocolError::Malformed)?,
            observation: observation.ok_or(ProtocolError::Malformed)?,
            ipc_version: ipc_version.ok_or(ProtocolError::Malformed)?,
        })
    }
}

/// One streamed audit record (`v1 audit SEQ|message`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditStreamRecord {
    pub sequence: u64,
    pub message: String,
}

impl AuditStreamRecord {
    /// Parse a streamed audit frame.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] when the frame is not `v1 audit SEQ|message`.
    pub fn parse(frame: &str) -> Result<Self, ProtocolError> {
        let line = frame.trim_end_matches(['\r', '\n']);
        let mut fields = line.splitn(3, ' ');
        match fields.next() {
            Some("v1") => {}
            Some(token) if token.starts_with('v') => return Err(ProtocolError::UnsupportedVersion),
            _ => return Err(ProtocolError::Malformed),
        }
        if fields.next() != Some("audit") {
            return Err(ProtocolError::Malformed);
        }
        let payload = fields.next().ok_or(ProtocolError::Malformed)?;
        let (sequence, message) = payload.split_once('|').ok_or(ProtocolError::Malformed)?;
        Ok(Self {
            sequence: sequence.parse().map_err(|_| ProtocolError::Malformed)?,
            message: message.to_owned(),
        })
    }
}

/// One rule row as returned by `v1 rules …`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleRow {
    pub id: u64,
    pub executable: String,
    pub verdict: String,
    pub port: u16,
}

impl RuleRow {
    /// Parse a `v1 rules …` response frame into rows.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] when the frame is not a well-formed rules list.
    pub fn parse_frame(frame: &str) -> Result<Vec<Self>, ProtocolError> {
        let line = frame.trim_end_matches(['\r', '\n']);
        let mut fields = line.splitn(3, ' ');
        match fields.next() {
            Some("v1") => {}
            Some(token) if token.starts_with('v') => return Err(ProtocolError::UnsupportedVersion),
            _ => return Err(ProtocolError::Malformed),
        }
        if fields.next() != Some("rules") {
            return Err(ProtocolError::Malformed);
        }
        let payload = fields.next().unwrap_or("");
        if payload.is_empty() {
            return Ok(Vec::new());
        }
        payload.split(',').map(Self::parse_row).collect()
    }

    fn parse_row(row: &str) -> Result<Self, ProtocolError> {
        let mut parts = row.splitn(4, '|');
        let id = parts
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or(ProtocolError::Malformed)?;
        let executable = parts.next().ok_or(ProtocolError::Malformed)?.to_owned();
        let verdict = parts.next().ok_or(ProtocolError::Malformed)?.to_owned();
        let port = parts
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or(ProtocolError::Malformed)?;
        Ok(Self {
            id,
            executable,
            verdict,
            port,
        })
    }

    /// Compact list label: `id exe verdict port`.
    #[must_use]
    pub fn list_label(&self) -> String {
        format!(
            "{}  {}  {}  :{}",
            self.id, self.executable, self.verdict, self.port
        )
    }
}

/// One pending prompt row as returned by `v1 prompts …`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptRow {
    pub id: u64,
    pub executable: String,
    pub destination: String,
    pub port: u16,
    pub protocol: String,
    pub remaining_secs: u64,
}

impl PromptRow {
    /// Parse a `v1 prompts …` response frame into rows.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] when the frame is not a well-formed prompts list.
    pub fn parse_frame(frame: &str) -> Result<Vec<Self>, ProtocolError> {
        let line = frame.trim_end_matches(['\r', '\n']);
        let mut fields = line.splitn(3, ' ');
        match fields.next() {
            Some("v1") => {}
            Some(token) if token.starts_with('v') => return Err(ProtocolError::UnsupportedVersion),
            _ => return Err(ProtocolError::Malformed),
        }
        if fields.next() != Some("prompts") {
            return Err(ProtocolError::Malformed);
        }
        let payload = fields.next().unwrap_or("");
        if payload.is_empty() {
            return Ok(Vec::new());
        }
        payload.split(',').map(Self::parse_row).collect()
    }

    fn parse_row(row: &str) -> Result<Self, ProtocolError> {
        let parts: Vec<&str> = row.split('|').collect();
        if parts.len() < 4 {
            return Err(ProtocolError::Malformed);
        }
        let id = parts[0].parse().map_err(|_| ProtocolError::Malformed)?;
        let executable = parts[1].to_owned();
        let destination = parts[2].to_owned();
        let port = parts[3].parse().map_err(|_| ProtocolError::Malformed)?;
        let protocol = parts.get(4).copied().unwrap_or("tcp").to_owned();
        let remaining_secs = parts
            .get(5)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        Ok(Self {
            id,
            executable,
            destination,
            port,
            protocol,
            remaining_secs,
        })
    }

    /// Compact list label.
    #[must_use]
    pub fn list_label(&self) -> String {
        format!(
            "{}  {}  {}:{} {}  {}s",
            self.id,
            self.executable,
            self.destination,
            self.port,
            self.protocol,
            self.remaining_secs
        )
    }

    /// Whether the TUI should allow answering this prompt.
    #[must_use]
    pub const fn can_answer(&self) -> bool {
        self.remaining_secs > 0
    }
}

/// Parse `v1 error …` into the message body.
#[must_use]
pub fn parse_error_message(frame: &str) -> Option<String> {
    let line = frame.trim_end_matches(['\r', '\n']);
    line.strip_prefix("v1 error ").map(str::to_owned)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProtocolError {
    #[error("malformed IPC frame")]
    Malformed,
    #[error("unsupported IPC version")]
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
        assert_eq!(
            DaemonStatus::parse(&frame),
            Ok(DaemonStatus {
                enforcement: "none".into(),
                observation: "degraded".into(),
                ipc_version: 1,
            })
        );
    }

    #[test]
    fn parses_audit_stream_record() {
        assert_eq!(
            AuditStreamRecord::parse("v1 audit 9|allow curl\n"),
            Ok(AuditStreamRecord {
                sequence: 9,
                message: "allow curl".into(),
            })
        );
    }

    #[test]
    fn parses_rules_list_frame() {
        assert_eq!(
            RuleRow::parse_frame("v1 rules 1|/bin/curl|Allow|443,2|/usr/bin/ssh|Deny|22\n"),
            Ok(vec![
                RuleRow {
                    id: 1,
                    executable: "/bin/curl".into(),
                    verdict: "Allow".into(),
                    port: 443,
                },
                RuleRow {
                    id: 2,
                    executable: "/usr/bin/ssh".into(),
                    verdict: "Deny".into(),
                    port: 22,
                },
            ])
        );
        assert_eq!(RuleRow::parse_frame("v1 rules\n"), Ok(vec![]));
        assert_eq!(
            parse_error_message("v1 error invalid_rule\n").as_deref(),
            Some("invalid_rule")
        );
    }

    #[test]
    fn parses_prompt_frames() {
        assert_eq!(Request::parse("v1 prompt-list\n"), Ok(Request::PromptList));
        assert_eq!(
            Request::parse("v1 prompt-answer 7 allow once\n"),
            Ok(Request::PromptAnswer {
                id: 7,
                verdict: "allow".into(),
                scope: "once".into(),
            })
        );
    }

    #[test]
    fn encodes_prompts_response() {
        assert_eq!(
            Response::Prompts("1|/bin/curl|127.0.0.1|443|tcp|12".into()).encode(),
            "v1 prompts 1|/bin/curl|127.0.0.1|443|tcp|12\n"
        );
        assert_eq!(
            PromptRow::parse_frame("v1 prompts 1|/bin/curl|127.0.0.1|443|tcp|12\n"),
            Ok(vec![PromptRow {
                id: 1,
                executable: "/bin/curl".into(),
                destination: "127.0.0.1".into(),
                port: 443,
                protocol: "tcp".into(),
                remaining_secs: 12,
            }])
        );
        assert!(
            !PromptRow {
                id: 1,
                executable: "/bin/curl".into(),
                destination: "127.0.0.1".into(),
                port: 443,
                protocol: "tcp".into(),
                remaining_secs: 0,
            }
            .can_answer()
        );
    }

    #[test]
    fn parses_dns_frames() {
        assert_eq!(Request::parse("v1 dns-list\n"), Ok(Request::DnsList));
        assert_eq!(
            Request::parse("v1 dns-note example.test 203.0.113.1 30\n"),
            Ok(Request::DnsNote {
                hostname: "example.test".into(),
                ipv4: "203.0.113.1".into(),
                ttl_secs: Some(30),
            })
        );
    }

    #[test]
    fn parses_audit_frames() {
        assert_eq!(
            Request::parse("v1 audit-tail 10\n"),
            Ok(Request::AuditTail { limit: 10 })
        );
        assert_eq!(
            Request::parse("v1 audit-subscribe ui since=3\n"),
            Ok(Request::AuditSubscribe {
                id: "ui".into(),
                since: 3,
            })
        );
    }
}
