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

/// InterFire-owned nftables table name (`inet` family).
pub const NFT_TABLE: &str = "interfire";
/// InterFire-owned output chain name inside [`NFT_TABLE`].
pub const NFT_CHAIN: &str = "output";
/// NFQUEUE number the daemon binds and the owned table queues into.
pub const NFQUEUE_NUM: u16 = 4242;

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
    ProcessList,
    NetworkStatus,
    NetworkInstall,
    NetworkRemove,
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
            Some("rule-delete") => parse_rule_delete(&mut fields),
            Some("rule-add") => parse_rule_add(&mut fields),
            Some("prompt-list") if fields.next().is_none() => Ok(Self::PromptList),
            Some("prompt-answer") => parse_prompt_answer(&mut fields),
            Some("dns-list") if fields.next().is_none() => Ok(Self::DnsList),
            Some("dns-note") => parse_dns_note(&mut fields),
            Some("audit-tail") => parse_audit_tail(&mut fields),
            Some("audit-subscribe") => parse_audit_subscribe(&mut fields),
            Some("process-list") if fields.next().is_none() => Ok(Self::ProcessList),
            Some("network-status") if fields.next().is_none() => Ok(Self::NetworkStatus),
            Some("network-install") if fields.next().is_none() => Ok(Self::NetworkInstall),
            Some("network-remove") if fields.next().is_none() => Ok(Self::NetworkRemove),
            _ => Err(ProtocolError::Malformed),
        }
    }
}

fn parse_rule_delete<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
) -> Result<Request, ProtocolError> {
    let id = fields.next().and_then(|value| value.parse().ok());
    if fields.next().is_none() {
        id.map(|id| Request::RuleDelete { id })
            .ok_or(ProtocolError::Malformed)
    } else {
        Err(ProtocolError::Malformed)
    }
}

fn parse_rule_add<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
) -> Result<Request, ProtocolError> {
    let id = fields.next().and_then(|value| value.parse().ok());
    let executable = fields.next().map(str::to_owned);
    let verdict = fields.next().map(str::to_owned);
    let port = fields.next().and_then(|value| value.parse().ok());
    if fields.next().is_some() {
        return Err(ProtocolError::Malformed);
    }
    match (id, executable, verdict, port) {
        (Some(id), Some(executable), Some(verdict), Some(port)) => Ok(Request::RuleAdd {
            id,
            executable,
            verdict,
            port,
        }),
        _ => Err(ProtocolError::Malformed),
    }
}

fn parse_prompt_answer<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
) -> Result<Request, ProtocolError> {
    let id = fields.next().and_then(|value| value.parse().ok());
    let verdict = fields.next().map(str::to_owned);
    let scope = fields.next().map(str::to_owned);
    if fields.next().is_some() {
        return Err(ProtocolError::Malformed);
    }
    match (id, verdict, scope) {
        (Some(id), Some(verdict), Some(scope)) => Ok(Request::PromptAnswer { id, verdict, scope }),
        _ => Err(ProtocolError::Malformed),
    }
}

fn parse_dns_note<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
) -> Result<Request, ProtocolError> {
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
        (Some(hostname), Some(ipv4)) => Ok(Request::DnsNote {
            hostname,
            ipv4,
            ttl_secs,
        }),
        _ => Err(ProtocolError::Malformed),
    }
}

fn parse_audit_tail<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
) -> Result<Request, ProtocolError> {
    let limit = match fields.next() {
        Some(value) => value.parse().map_err(|_| ProtocolError::Malformed)?,
        None => MAX_LOG_RECORDS_PER_SUBSCRIBER,
    };
    if fields.next().is_some() {
        return Err(ProtocolError::Malformed);
    }
    Ok(Request::AuditTail { limit })
}

fn parse_audit_subscribe<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
) -> Result<Request, ProtocolError> {
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
    id.map(|id| Request::AuditSubscribe { id, since })
        .ok_or(ProtocolError::Malformed)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Response {
    Pong,
    Status(StatusBody),
    Error(&'static str),
    Rules(String),
    Prompts(String),
    Dns(String),
    Audit(String),
    Subscribed(String),
    Processes(String),
    Network(NetworkStatusBody),
}

/// Owned nftables table presence for the Network tab / `network-status`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkTableState {
    /// Table `inet interfire` is absent.
    Missing,
    /// Table present with the expected outbound TCP queue rule.
    Installed,
    /// Table present but missing or mismatched queue rule.
    Incomplete,
}

impl NetworkTableState {
    /// Wire token used in IPC (`missing` / `installed` / `incomplete`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Installed => "installed",
            Self::Incomplete => "incomplete",
        }
    }

    /// Parse a wire token.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::Malformed`] when the token is unknown.
    pub fn parse(value: &str) -> Result<Self, ProtocolError> {
        match value {
            "missing" => Ok(Self::Missing),
            "installed" => Ok(Self::Installed),
            "incomplete" => Ok(Self::Incomplete),
            _ => Err(ProtocolError::Malformed),
        }
    }
}

/// `network-status` response body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NetworkStatusBody {
    pub table: &'static str,
    pub queue: u16,
    pub state: NetworkTableState,
    /// Compact rule token (`none` or `tcp_new_queue_<n>`).
    pub rule: &'static str,
}

/// Daemon `status` response body (encode side).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatusBody {
    /// Verdict path state (`none`, `nfqueue`, or `degraded`).
    pub enforcement: &'static str,
    /// eBPF observation state (`attached` or `degraded`).
    pub observation: &'static str,
    pub ipc_version: u16,
    /// Daemon process id.
    pub pid: u32,
    /// Daemon `VmRSS` in KiB from `/proc/self/status`.
    pub rss_kib: u64,
    /// Daemon `utime + stime` jiffies from `/proc/self/stat` (for CPU % over polls).
    pub cpu_jiffies: u64,
}

impl Response {
    /// Encode a response as one newline-terminated control frame.
    #[must_use]
    pub fn encode(&self) -> String {
        match self {
            Self::Pong => "v1 pong\n".into(),
            Self::Status(body) => format!(
                "v1 status enforcement={} observation={} ipc_version={} pid={} rss_kib={} cpu_jiffies={}\n",
                body.enforcement,
                body.observation,
                body.ipc_version,
                body.pid,
                body.rss_kib,
                body.cpu_jiffies,
            ),
            Self::Error(message) => format!("v1 error {message}\n"),
            Self::Rules(value) => format!("v1 rules {value}\n"),
            Self::Prompts(value) => format!("v1 prompts {value}\n"),
            Self::Dns(value) => format!("v1 dns {value}\n"),
            Self::Audit(value) => format!("v1 audit-tail {value}\n"),
            Self::Subscribed(id) => format!("v1 subscribed {id}\n"),
            Self::Processes(value) => format!("v1 processes {value}\n"),
            Self::Network(body) => format!(
                "v1 network table={} queue={} state={} rule={}\n",
                body.table,
                body.queue,
                body.state.as_str(),
                body.rule,
            ),
        }
    }
}

/// Parsed `network-status` fields (owned; for clients).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkStatus {
    pub table: String,
    pub queue: u16,
    pub state: NetworkTableState,
    pub rule: String,
}

impl NetworkStatus {
    /// Parse a `v1 network …` response frame.
    ///
    /// Unknown `key=value` fields are ignored for forward compatibility.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] when the frame is not a well-formed network line.
    pub fn parse(frame: &str) -> Result<Self, ProtocolError> {
        let line = frame.trim_end_matches(['\r', '\n']);
        let mut fields = line.split_whitespace();
        match fields.next() {
            Some("v1") => {}
            Some(token) if token.starts_with('v') => return Err(ProtocolError::UnsupportedVersion),
            _ => return Err(ProtocolError::Malformed),
        }
        if fields.next() != Some("network") {
            return Err(ProtocolError::Malformed);
        }
        let mut table = None;
        let mut queue = None;
        let mut state = None;
        let mut rule = None;
        for field in fields {
            if let Some(value) = field.strip_prefix("table=") {
                table = Some(value.to_owned());
            } else if let Some(value) = field.strip_prefix("queue=") {
                queue = Some(value.parse().map_err(|_| ProtocolError::Malformed)?);
            } else if let Some(value) = field.strip_prefix("state=") {
                state = Some(NetworkTableState::parse(value)?);
            } else if let Some(value) = field.strip_prefix("rule=") {
                rule = Some(value.to_owned());
            } else if field.contains('=') {
                // Forward-compatible: ignore unknown keys.
            } else {
                return Err(ProtocolError::Malformed);
            }
        }
        Ok(Self {
            table: table.ok_or(ProtocolError::Malformed)?,
            queue: queue.ok_or(ProtocolError::Malformed)?,
            state: state.ok_or(ProtocolError::Malformed)?,
            rule: rule.ok_or(ProtocolError::Malformed)?,
        })
    }

    /// Operator-facing summary line for Status / Network chrome.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "table {}  queue {}  {}  rule {}",
            self.table,
            self.queue,
            self.state.as_str(),
            self.rule
        )
    }
}

/// Compact rule token when the owned TCP queue rule is present.
#[must_use]
pub const fn network_rule_token(queue: u16) -> &'static str {
    match queue {
        4242 => "tcp_new_queue_4242",
        _ => "tcp_new_queue",
    }
}

/// Parsed daemon status fields (owned; for clients).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonStatus {
    pub enforcement: String,
    pub observation: String,
    pub ipc_version: u16,
    /// Present when the daemon includes process metrics on `status`.
    pub pid: Option<u32>,
    /// Daemon `VmRSS` in KiB when present.
    pub rss_kib: Option<u64>,
    /// Daemon CPU jiffies (`utime + stime`) when present.
    pub cpu_jiffies: Option<u64>,
}

impl DaemonStatus {
    /// Parse a `v1 status …` response frame.
    ///
    /// Unknown `key=value` fields are ignored so older clients keep working when
    /// the daemon adds metrics keys.
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
        let mut pid = None;
        let mut rss_kib = None;
        let mut cpu_jiffies = None;
        for field in fields {
            if let Some(value) = field.strip_prefix("enforcement=") {
                enforcement = Some(value.to_owned());
            } else if let Some(value) = field.strip_prefix("observation=") {
                observation = Some(value.to_owned());
            } else if let Some(value) = field.strip_prefix("ipc_version=") {
                ipc_version = Some(value.parse().map_err(|_| ProtocolError::Malformed)?);
            } else if let Some(value) = field.strip_prefix("pid=") {
                pid = Some(value.parse().map_err(|_| ProtocolError::Malformed)?);
            } else if let Some(value) = field.strip_prefix("rss_kib=") {
                rss_kib = Some(value.parse().map_err(|_| ProtocolError::Malformed)?);
            } else if let Some(value) = field.strip_prefix("cpu_jiffies=") {
                cpu_jiffies = Some(value.parse().map_err(|_| ProtocolError::Malformed)?);
            } else if field.contains('=') {
                // Forward-compatible: ignore unknown keys.
            } else {
                return Err(ProtocolError::Malformed);
            }
        }
        Ok(Self {
            enforcement: enforcement.ok_or(ProtocolError::Malformed)?,
            observation: observation.ok_or(ProtocolError::Malformed)?,
            ipc_version: ipc_version.ok_or(ProtocolError::Malformed)?,
            pid,
            rss_kib,
            cpu_jiffies,
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

/// One observed process row as returned by `v1 processes …`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessRow {
    pub pid: u32,
    pub start_ticks: u64,
    pub uid: u32,
    pub executable: String,
    pub cmdline: String,
    /// Effective rule verdict for this executable (`allow` / `deny` / `prompt`).
    pub verdict: String,
    /// Compact recent destinations: `ip:port/verdict+…`.
    pub ports: String,
}

impl ProcessRow {
    /// Parse a `v1 processes …` response frame into rows.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] when the frame is not a well-formed process list.
    pub fn parse_frame(frame: &str) -> Result<Vec<Self>, ProtocolError> {
        let line = frame.trim_end_matches(['\r', '\n']);
        let mut fields = line.splitn(3, ' ');
        match fields.next() {
            Some("v1") => {}
            Some(token) if token.starts_with('v') => return Err(ProtocolError::UnsupportedVersion),
            _ => return Err(ProtocolError::Malformed),
        }
        if fields.next() != Some("processes") {
            return Err(ProtocolError::Malformed);
        }
        let payload = fields.next().unwrap_or("");
        if payload.is_empty() {
            return Ok(Vec::new());
        }
        payload.split(';').map(Self::parse_row).collect()
    }

    fn parse_row(row: &str) -> Result<Self, ProtocolError> {
        let mut parts = row.splitn(7, '|');
        let pid = parts
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or(ProtocolError::Malformed)?;
        let start_ticks = parts
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or(ProtocolError::Malformed)?;
        let uid = parts
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or(ProtocolError::Malformed)?;
        let verdict = parts.next().ok_or(ProtocolError::Malformed)?.to_owned();
        let recent_ports = parts.next().ok_or(ProtocolError::Malformed)?.to_owned();
        let executable = unescape_field(parts.next().ok_or(ProtocolError::Malformed)?);
        let cmdline = unescape_field(parts.next().ok_or(ProtocolError::Malformed)?);
        Ok(Self {
            pid,
            start_ticks,
            uid,
            executable,
            cmdline,
            verdict,
            ports: recent_ports,
        })
    }

    /// Encode one row for a `v1 processes` payload.
    #[must_use]
    pub fn encode_row(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            self.pid,
            self.start_ticks,
            self.uid,
            self.verdict,
            self.ports,
            escape_field(&self.executable),
            escape_field(&self.cmdline),
        )
    }

    /// Compact list label for UI / TUI.
    #[must_use]
    pub fn list_label(&self) -> String {
        format!(
            "{}  {}  {}  {}",
            self.pid, self.executable, self.verdict, self.ports
        )
    }
}

fn escape_field(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('|', "%7C")
        .replace(';', "%3B")
}

fn unescape_field(value: &str) -> String {
    value
        .replace("%3B", ";")
        .replace("%7C", "|")
        .replace("%25", "%")
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
    fn status_response_includes_observation_and_metrics() {
        let frame = Response::Status(StatusBody {
            enforcement: "none",
            observation: "degraded",
            ipc_version: IPC_VERSION,
            pid: 42,
            rss_kib: 6400,
            cpu_jiffies: 1234,
        })
        .encode();
        assert_eq!(
            frame,
            "v1 status enforcement=none observation=degraded ipc_version=1 pid=42 rss_kib=6400 cpu_jiffies=1234\n"
        );
        assert_eq!(
            DaemonStatus::parse(&frame),
            Ok(DaemonStatus {
                enforcement: "none".into(),
                observation: "degraded".into(),
                ipc_version: 1,
                pid: Some(42),
                rss_kib: Some(6400),
                cpu_jiffies: Some(1234),
            })
        );
    }

    #[test]
    fn status_parse_ignores_unknown_keys_and_allows_legacy() {
        assert_eq!(
            DaemonStatus::parse(
                "v1 status enforcement=nfqueue observation=attached ipc_version=1\n"
            ),
            Ok(DaemonStatus {
                enforcement: "nfqueue".into(),
                observation: "attached".into(),
                ipc_version: 1,
                pid: None,
                rss_kib: None,
                cpu_jiffies: None,
            })
        );
        assert_eq!(
            DaemonStatus::parse(
                "v1 status enforcement=nfqueue observation=attached ipc_version=1 pid=9 future=1\n"
            ),
            Ok(DaemonStatus {
                enforcement: "nfqueue".into(),
                observation: "attached".into(),
                ipc_version: 1,
                pid: Some(9),
                rss_kib: None,
                cpu_jiffies: None,
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
    fn parses_process_list_frame() {
        assert_eq!(
            Request::parse("v1 process-list\n"),
            Ok(Request::ProcessList)
        );
        let row = ProcessRow {
            pid: 42,
            start_ticks: 99,
            uid: 1000,
            executable: "/usr/bin/curl".into(),
            cmdline: "curl https://example".into(),
            verdict: "prompt".into(),
            ports: "203.0.113.1:443/prompt".into(),
        };
        let frame = Response::Processes(row.encode_row()).encode();
        assert_eq!(ProcessRow::parse_frame(&frame), Ok(vec![row]));
        assert_eq!(ProcessRow::parse_frame("v1 processes\n"), Ok(vec![]));
    }

    #[test]
    fn parses_network_status_frames() {
        assert_eq!(
            Request::parse("v1 network-status\n"),
            Ok(Request::NetworkStatus)
        );
        assert_eq!(
            Request::parse("v1 network-install\n"),
            Ok(Request::NetworkInstall)
        );
        assert_eq!(
            Request::parse("v1 network-remove\n"),
            Ok(Request::NetworkRemove)
        );
        let frame = Response::Network(NetworkStatusBody {
            table: NFT_TABLE,
            queue: NFQUEUE_NUM,
            state: NetworkTableState::Installed,
            rule: network_rule_token(NFQUEUE_NUM),
        })
        .encode();
        assert_eq!(
            frame,
            "v1 network table=interfire queue=4242 state=installed rule=tcp_new_queue_4242\n"
        );
        assert_eq!(
            NetworkStatus::parse(&frame),
            Ok(NetworkStatus {
                table: "interfire".into(),
                queue: 4242,
                state: NetworkTableState::Installed,
                rule: "tcp_new_queue_4242".into(),
            })
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
        assert_eq!(
            Request::parse("v1 audit-tail\n"),
            Ok(Request::AuditTail {
                limit: MAX_LOG_RECORDS_PER_SUBSCRIBER
            })
        );
    }

    #[test]
    fn parses_rule_add_and_delete_requests() {
        assert_eq!(
            Request::parse("v1 rule-add 1 /bin/curl allow 443\n"),
            Ok(Request::RuleAdd {
                id: 1,
                executable: "/bin/curl".into(),
                verdict: "allow".into(),
                port: 443,
            })
        );
        assert_eq!(
            Request::parse("v1 rule-delete 9\n"),
            Ok(Request::RuleDelete { id: 9 })
        );
        assert_eq!(
            Request::parse("v1 rule-add bad\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            Request::parse("v1 rule-delete not-a-number\n"),
            Err(ProtocolError::Malformed)
        );
    }

    #[test]
    fn encodes_additional_response_variants() {
        assert_eq!(Response::Pong.encode(), "v1 pong\n");
        assert_eq!(Response::Error("bad").encode(), "v1 error bad\n");
        assert_eq!(Response::Dns("rows".into()).encode(), "v1 dns rows\n");
        assert_eq!(
            Response::Audit("tail".into()).encode(),
            "v1 audit-tail tail\n"
        );
        assert_eq!(
            Response::Subscribed("ui".into()).encode(),
            "v1 subscribed ui\n"
        );
    }

    #[test]
    fn network_table_state_and_rule_token() {
        assert_eq!(NetworkTableState::Missing.as_str(), "missing");
        assert_eq!(
            NetworkTableState::parse("installed").unwrap(),
            NetworkTableState::Installed
        );
        assert_eq!(
            NetworkTableState::parse("broken"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(network_rule_token(4242), "tcp_new_queue_4242");
        assert_eq!(network_rule_token(99), "tcp_new_queue");
    }

    #[test]
    fn network_status_parse_errors_and_summary() {
        assert_eq!(
            NetworkStatus::parse("v1 network table=interfire queue=4242 state=installed rule=x\n"),
            Ok(NetworkStatus {
                table: "interfire".into(),
                queue: 4242,
                state: NetworkTableState::Installed,
                rule: "x".into(),
            })
        );
        assert_eq!(
            NetworkStatus::parse("v2 network table=x queue=1 state=missing rule=none\n"),
            Err(ProtocolError::UnsupportedVersion)
        );
        assert_eq!(
            NetworkStatus::parse("v1 network table=x\n"),
            Err(ProtocolError::Malformed)
        );
        let status = NetworkStatus {
            table: "interfire".into(),
            queue: 4242,
            state: NetworkTableState::Incomplete,
            rule: "none".into(),
        };
        assert!(status.summary().contains("incomplete"));
    }

    #[test]
    fn audit_stream_record_rejects_malformed_frames() {
        assert_eq!(
            AuditStreamRecord::parse("v1 audit 9|allow curl\n"),
            Ok(AuditStreamRecord {
                sequence: 9,
                message: "allow curl".into(),
            })
        );
        assert_eq!(
            AuditStreamRecord::parse("v1 audit missing-pipe\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            AuditStreamRecord::parse("v2 audit 1|x\n"),
            Err(ProtocolError::UnsupportedVersion)
        );
    }

    #[test]
    fn rule_and_prompt_row_labels_and_malformed_rows() {
        let rule = RuleRow {
            id: 1,
            executable: "/bin/curl".into(),
            verdict: "Allow".into(),
            port: 443,
        };
        assert!(rule.list_label().contains("/bin/curl"));
        assert_eq!(
            RuleRow::parse_frame("v1 rules 1|/bin/curl|Allow\n"),
            Err(ProtocolError::Malformed)
        );

        let prompt = PromptRow {
            id: 1,
            executable: "/bin/curl".into(),
            destination: "127.0.0.1".into(),
            port: 443,
            protocol: "tcp".into(),
            remaining_secs: 5,
        };
        assert!(prompt.list_label().contains("127.0.0.1"));
        assert_eq!(
            PromptRow::parse_frame("v1 prompts 1|/bin/curl|127.0.0.1|443\n"),
            Ok(vec![PromptRow {
                id: 1,
                executable: "/bin/curl".into(),
                destination: "127.0.0.1".into(),
                port: 443,
                protocol: "tcp".into(),
                remaining_secs: 0,
            }])
        );
    }

    #[test]
    fn process_row_escape_round_trip_and_labels() {
        let row = ProcessRow {
            pid: 1,
            start_ticks: 2,
            uid: 1000,
            executable: "/usr/bin/a|b".into(),
            cmdline: "a;b".into(),
            verdict: "allow".into(),
            ports: "1.2.3.4:443/allow".into(),
        };
        let encoded = row.encode_row();
        assert!(encoded.contains("%7C"));
        let frame = Response::Processes(encoded).encode();
        let parsed = ProcessRow::parse_frame(&frame).unwrap();
        assert_eq!(parsed, vec![row.clone()]);
        assert!(row.list_label().contains("allow"));
        assert_eq!(
            ProcessRow::parse_frame("v1 processes 1|2\n"),
            Err(ProtocolError::Malformed)
        );
    }

    #[test]
    fn dns_note_without_ttl_and_malformed_prompt_answer() {
        assert_eq!(
            Request::parse("v1 dns-note example.test 203.0.113.1\n"),
            Ok(Request::DnsNote {
                hostname: "example.test".into(),
                ipv4: "203.0.113.1".into(),
                ttl_secs: None,
            })
        );
        assert_eq!(
            Request::parse("v1 dns-note bad 1.2.3.4 not-a-ttl\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            Request::parse("v1 prompt-answer 1 allow\n"),
            Err(ProtocolError::Malformed)
        );
    }

    #[test]
    fn daemon_status_rejects_malformed_frames() {
        assert_eq!(
            DaemonStatus::parse("v1 status enforcement=none\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            DaemonStatus::parse("v2 status enforcement=none observation=attached ipc_version=1\n"),
            Err(ProtocolError::UnsupportedVersion)
        );
    }

    #[test]
    fn request_parse_rejects_malformed_commands() {
        assert_eq!(Request::parse("v1\n"), Err(ProtocolError::Malformed));
        assert_eq!(
            Request::parse("v1 unknown\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            Request::parse("v1 rule-delete 1 extra\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            Request::parse("v1 rule-add 1 /bin/curl allow 443 extra\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            Request::parse("v1 prompt-answer 1 allow once extra\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            Request::parse("v1 dns-note host 1.2.3.4 30 extra\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            Request::parse("v1 audit-tail 10 extra\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            Request::parse("v1 audit-subscribe ui since=bad\n"),
            Err(ProtocolError::Malformed)
        );
    }

    #[test]
    fn network_status_rejects_bad_tokens() {
        assert_eq!(
            NetworkStatus::parse("v1 network table=x queue=bad state=missing rule=none\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            NetworkStatus::parse("v1 network table=x queue=1 state=broken rule=none\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            NetworkStatus::parse("v1 network table=x queue=1 state=missing bad\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            NetworkStatus::parse("v1 not-network table=x queue=1 state=missing rule=none\n"),
            Err(ProtocolError::Malformed)
        );
    }

    #[test]
    fn frame_parsers_reject_bad_versions_and_payloads() {
        assert_eq!(
            RuleRow::parse_frame("v2 rules 1|/bin/curl|Allow|443\n"),
            Err(ProtocolError::UnsupportedVersion)
        );
        assert_eq!(
            RuleRow::parse_frame("v1 not-rules\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            PromptRow::parse_frame("v2 prompts 1|/bin/c|127.0.0.1|443|tcp|1\n"),
            Err(ProtocolError::UnsupportedVersion)
        );
        assert_eq!(
            PromptRow::parse_frame("v1 not-prompts\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            PromptRow::parse_frame("v1 prompts 1|/bin/c|127.0.0.1|bad|tcp|1\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            ProcessRow::parse_frame("v2 processes 1|2|3|allow||/bin/c|cmd\n"),
            Err(ProtocolError::UnsupportedVersion)
        );
        assert_eq!(
            ProcessRow::parse_frame("v1 not-processes\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            AuditStreamRecord::parse("v1 not-audit 1|x\n"),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            DaemonStatus::parse(
                "v1 not-status enforcement=none observation=attached ipc_version=1\n"
            ),
            Err(ProtocolError::Malformed)
        );
        assert_eq!(
            DaemonStatus::parse(
                "v1 status enforcement=none observation=attached ipc_version=bad\n"
            ),
            Err(ProtocolError::Malformed)
        );
    }

    #[test]
    fn network_status_ignores_unknown_keys() {
        let status = NetworkStatus::parse(
            "v1 network table=interfire queue=4242 state=installed rule=x future=1\n",
        )
        .expect("network");
        assert_eq!(status.table, "interfire");
    }
}
