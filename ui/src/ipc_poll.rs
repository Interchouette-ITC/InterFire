//! Sync one-shot Unix IPC polls for tray / Status chrome.
#![forbid(unsafe_code)]

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use interfire_proto::{DaemonStatus, MAX_FRAME_BYTES, PromptRow};

use crate::tray::DaemonLink;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

/// Poll daemon status and pending prompt count.
#[must_use]
pub fn poll_link(socket: &str) -> DaemonLink {
    match fetch_status(socket) {
        Ok(status) => {
            let pending_prompts = fetch_prompt_count(socket).unwrap_or(0);
            DaemonLink::Up {
                status,
                pending_prompts,
            }
        }
        Err(reason) => DaemonLink::Down { reason },
    }
}

fn fetch_status(socket: &str) -> Result<DaemonStatus, String> {
    let frame = one_shot(socket, "v1 status\n").map_err(|e| e.to_string())?;
    DaemonStatus::parse(&frame).map_err(|_| "malformed_status".to_owned())
}

fn fetch_prompt_count(socket: &str) -> Result<usize, String> {
    let frame = one_shot(socket, "v1 prompt-list\n").map_err(|e| e.to_string())?;
    PromptRow::parse_frame(&frame)
        .map(|rows| rows.len())
        .map_err(|_| "malformed_prompts".to_owned())
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
