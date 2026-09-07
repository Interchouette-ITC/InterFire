//! Sync one-shot Unix IPC polls for tray, Status, alerts, and Rules CRUD.
#![forbid(unsafe_code)]

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use interfire_proto::{DaemonStatus, MAX_FRAME_BYTES, PromptRow, RuleRow, parse_error_message};

use crate::tray::DaemonLink;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

/// One poll of daemon status, prompts, and rules.
#[derive(Clone, Debug)]
pub struct PollSnapshot {
    pub link: DaemonLink,
    pub prompts: Vec<PromptRow>,
    pub rules: Vec<RuleRow>,
}

/// Poll daemon status, pending prompts, and rules.
#[must_use]
#[hotpath::measure]
pub fn poll_snapshot(socket: &str) -> PollSnapshot {
    match fetch_status(socket) {
        Ok(status) => {
            let prompts = fetch_prompts(socket).unwrap_or_default();
            let rules = fetch_rules(socket).unwrap_or_default();
            PollSnapshot {
                link: DaemonLink::Up {
                    status,
                    pending_prompts: prompts.len(),
                },
                prompts,
                rules,
            }
        }
        Err(reason) => PollSnapshot {
            link: DaemonLink::Down { reason },
            prompts: Vec::new(),
            rules: Vec::new(),
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
