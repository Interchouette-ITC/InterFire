//! Sync one-shot Unix IPC polls for tray, Status, alerts, and Rules CRUD.
#![forbid(unsafe_code)]

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use interfire_proto::{
    DaemonStatus, MAX_FRAME_BYTES, NetworkStatus, ProcessRow, PromptRow, RuleRow, StatsRow,
    StatsSummary, parse_error_message,
};

use crate::tray::DaemonLink;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

/// One poll of daemon status, prompts, rules, and stats.
#[derive(Clone, Debug)]
pub struct PollSnapshot {
    pub link: DaemonLink,
    pub prompts: Vec<PromptRow>,
    pub rules: Vec<RuleRow>,
    pub processes: Vec<ProcessRow>,
    pub network: Option<NetworkStatus>,
    pub stats_summary: Option<StatsSummary>,
    pub stats_hosts: Vec<StatsRow>,
    pub stats_procs: Vec<StatsRow>,
    pub stats_addrs: Vec<StatsRow>,
    pub stats_ports: Vec<StatsRow>,
    pub stats_users: Vec<StatsRow>,
}

/// Poll daemon status, pending prompts, rules, and stats aggregates.
#[must_use]
#[hotpath::measure]
pub fn poll_snapshot(socket: &str) -> PollSnapshot {
    match fetch_status(socket) {
        Ok(status) => {
            let prompts = fetch_prompts(socket).unwrap_or_default();
            let rules = fetch_rules(socket).unwrap_or_default();
            let processes = fetch_processes(socket).unwrap_or_default();
            let network = fetch_network(socket).ok();
            let stats_summary = fetch_stats_summary(socket).ok();
            let stats_hosts = fetch_stats_rows(socket, "stats-hosts").unwrap_or_default();
            let stats_procs = fetch_stats_rows(socket, "stats-procs").unwrap_or_default();
            let stats_addrs = fetch_stats_rows(socket, "stats-addrs").unwrap_or_default();
            let stats_ports = fetch_stats_rows(socket, "stats-ports").unwrap_or_default();
            let stats_users = fetch_stats_rows(socket, "stats-users").unwrap_or_default();
            PollSnapshot {
                link: DaemonLink::Up {
                    status,
                    pending_prompts: prompts.len(),
                },
                prompts,
                rules,
                processes,
                network,
                stats_summary,
                stats_hosts,
                stats_procs,
                stats_addrs,
                stats_ports,
                stats_users,
            }
        }
        Err(reason) => PollSnapshot {
            link: DaemonLink::Down { reason },
            prompts: Vec::new(),
            rules: Vec::new(),
            processes: Vec::new(),
            network: None,
            stats_summary: None,
            stats_hosts: Vec::new(),
            stats_procs: Vec::new(),
            stats_addrs: Vec::new(),
            stats_ports: Vec::new(),
            stats_users: Vec::new(),
        },
    }
}

/// Send one control frame and require a `v1 pong` reply.
///
/// # Errors
///
/// Returns a daemon error message or transport failure text.
pub fn send_expect_pong(socket: &str, request: &str) -> Result<(), String> {
    let frame = one_shot(socket, request).map_err(|e| e.to_string())?;
    if frame.starts_with("v1 pong") {
        return Ok(());
    }
    Err(parse_error_message(&frame).unwrap_or_else(|| frame.trim().to_owned()))
}

/// Add a durable rule (`v1 rule-add …`).
///
/// # Errors
///
/// Returns a daemon error message or transport failure text.
pub fn add_rule(
    socket: &str,
    id: u64,
    executable: &str,
    verdict: &str,
    port: u16,
) -> Result<(), String> {
    let request = format!("v1 rule-add {id} {executable} {verdict} {port}\n");
    send_expect_pong(socket, &request)
}

/// Delete a durable rule (`v1 rule-delete …`).
///
/// # Errors
///
/// Returns a daemon error message or transport failure text.
pub fn delete_rule(socket: &str, id: u64) -> Result<(), String> {
    let request = format!("v1 rule-delete {id}\n");
    send_expect_pong(socket, &request)
}

/// Install the InterFire-owned nftables queue table (`v1 network-install`).
///
/// # Errors
///
/// Returns a daemon error message or transport failure text.
pub fn install_network(socket: &str) -> Result<(), String> {
    send_expect_pong(socket, "v1 network-install\n")
}

/// Remove the InterFire-owned nftables table (`v1 network-remove`).
///
/// # Errors
///
/// Returns a daemon error message or transport failure text.
pub fn remove_network(socket: &str) -> Result<(), String> {
    send_expect_pong(socket, "v1 network-remove\n")
}

/// Pause enforcement (`v1 pause`): remove owned table; network unfiltered.
///
/// # Errors
///
/// Returns a daemon error message or transport failure text.
pub fn pause_firewall(socket: &str) -> Result<(), String> {
    send_expect_pong(socket, "v1 pause\n")
}

/// Resume enforcement (`v1 resume`): install owned table when the queue is bound.
///
/// # Errors
///
/// Returns a daemon error message or transport failure text.
pub fn resume_firewall(socket: &str) -> Result<(), String> {
    send_expect_pong(socket, "v1 resume\n")
}

/// Block traffic for a scope (`v1 traffic-block scope=… direction=…`).
///
/// # Errors
///
/// Returns a daemon error message or transport failure text.
pub fn traffic_block(socket: &str, scope: &str, direction: &str) -> Result<(), String> {
    send_expect_pong(
        socket,
        &format!("v1 traffic-block scope={scope} direction={direction}\n"),
    )
}

/// Unblock traffic for a scope (`v1 traffic-unblock scope=…`).
///
/// # Errors
///
/// Returns a daemon error message or transport failure text.
pub fn traffic_unblock(socket: &str, scope: &str) -> Result<(), String> {
    send_expect_pong(socket, &format!("v1 traffic-unblock scope={scope}\n"))
}

/// Fetch owned nftables status (`v1 network-status`).
///
/// # Errors
///
/// Returns a transport or parse failure text.
pub fn fetch_network(socket: &str) -> Result<NetworkStatus, String> {
    let frame = one_shot(socket, "v1 network-status\n").map_err(|e| e.to_string())?;
    NetworkStatus::parse(&frame).map_err(|_| "malformed_network".to_owned())
}

fn fetch_status(socket: &str) -> Result<DaemonStatus, String> {
    let frame = one_shot(socket, "v1 status\n").map_err(|e| e.to_string())?;
    DaemonStatus::parse(&frame).map_err(|_| "malformed_status".to_owned())
}

fn fetch_prompts(socket: &str) -> Result<Vec<PromptRow>, String> {
    let frame = one_shot(socket, "v1 prompt-list\n").map_err(|e| e.to_string())?;
    PromptRow::parse_frame(&frame).map_err(|_| "malformed_prompts".to_owned())
}

fn fetch_rules(socket: &str) -> Result<Vec<RuleRow>, String> {
    let frame = one_shot(socket, "v1 rule-list\n").map_err(|e| e.to_string())?;
    RuleRow::parse_frame(&frame).map_err(|_| "malformed_rules".to_owned())
}

fn fetch_processes(socket: &str) -> Result<Vec<ProcessRow>, String> {
    let frame = one_shot(socket, "v1 process-list\n").map_err(|e| e.to_string())?;
    ProcessRow::parse_frame(&frame).map_err(|_| "malformed_processes".to_owned())
}

fn fetch_stats_summary(socket: &str) -> Result<StatsSummary, String> {
    let frame = one_shot(socket, "v1 stats-summary\n").map_err(|e| e.to_string())?;
    StatsSummary::parse(&frame).map_err(|_| "malformed_stats_summary".to_owned())
}

fn fetch_stats_rows(socket: &str, label: &str) -> Result<Vec<StatsRow>, String> {
    let frame = one_shot(socket, &format!("v1 {label}\n")).map_err(|e| e.to_string())?;
    StatsRow::parse_frame(&frame, label).map_err(|_| "malformed_stats".to_owned())
}

fn one_shot(socket: &str, request: &str) -> io::Result<String> {
    let stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(CONNECT_TIMEOUT))?;
    stream.set_write_timeout(Some(CONNECT_TIMEOUT))?;
    let mut writer = stream.try_clone()?;
    writer.write_all(request.as_bytes())?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    if line.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame_too_large",
        ));
    }
    Ok(line)
}
