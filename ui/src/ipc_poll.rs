//! Sync one-shot Unix IPC polls for tray, Status, and connection alerts.
#![forbid(unsafe_code)]

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use interfire_proto::{DaemonStatus, MAX_FRAME_BYTES, PromptRow, parse_error_message};

use crate::tray::DaemonLink;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

/// One poll of daemon status plus pending prompts.
#[derive(Clone, Debug)]
pub struct PollSnapshot {
    pub link: DaemonLink,
    pub prompts: Vec<PromptRow>,
}

/// Poll daemon status and pending prompts.
#[must_use]
pub fn poll_snapshot(socket: &str) -> PollSnapshot {
    match fetch_status(socket) {
        Ok(status) => {
            let prompts = fetch_prompts(socket).unwrap_or_default();
            PollSnapshot {
                link: DaemonLink::Up {
                    status,
                    pending_prompts: prompts.len(),
                },
                prompts,
            }
        }
        Err(reason) => PollSnapshot {
            link: DaemonLink::Down { reason },
            prompts: Vec::new(),
        },
    }
}

/// Answer a pending prompt (`v1 prompt-answer …`). Success replies with `v1 pong`.
///
/// # Errors
///
/// Returns a daemon error message or transport failure text.
pub fn answer_prompt(socket: &str, id: u64, verdict: &str, scope: &str) -> Result<(), String> {
    let request = format!("v1 prompt-answer {id} {verdict} {scope}\n");
    let frame = one_shot(socket, &request).map_err(|e| e.to_string())?;
    if frame.starts_with("v1 pong") {
        return Ok(());
    }
    Err(parse_error_message(&frame).unwrap_or_else(|| frame.trim().to_owned()))
}

fn fetch_status(socket: &str) -> Result<DaemonStatus, String> {
    let frame = one_shot(socket, "v1 status\n").map_err(|e| e.to_string())?;
    DaemonStatus::parse(&frame).map_err(|_| "malformed_status".to_owned())
}

fn fetch_prompts(socket: &str) -> Result<Vec<PromptRow>, String> {
    let frame = one_shot(socket, "v1 prompt-list\n").map_err(|e| e.to_string())?;
    PromptRow::parse_frame(&frame).map_err(|_| "malformed_prompts".to_owned())
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
