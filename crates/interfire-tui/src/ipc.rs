//! Async Unix IPC client: status poll + replace-on-reconnect audit stream.
#![forbid(unsafe_code)]

use std::io;
use std::time::Duration;

use interfire_proto::{AuditStreamRecord, DaemonStatus, MAX_FRAME_BYTES};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::time;

/// Fixed subscription id so reconnect replaces the prior stream.
pub const AUDIT_SUBSCRIBER_ID: &str = "interfire-tui";

const STATUS_INTERVAL: Duration = Duration::from_secs(1);
const RECONNECT_BACKOFF: Duration = Duration::from_millis(500);

/// Events pushed to the UI from background IPC tasks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IpcEvent {
    Status(DaemonStatus),
    Down(String),
    Audit(AuditStreamRecord),
    SubscriptionReady,
}

/// Spawn non-blocking status poll and audit subscribe loops.
pub fn spawn(socket: String, tx: mpsc::UnboundedSender<IpcEvent>) {
    let status_socket = socket.clone();
    let status_tx = tx.clone();
    tokio::spawn(async move {
        status_loop(status_socket, status_tx).await;
    });
    tokio::spawn(async move {
        audit_loop(socket, tx).await;
    });
}

async fn status_loop(socket: String, tx: mpsc::UnboundedSender<IpcEvent>) {
    let mut interval = time::interval(STATUS_INTERVAL);
    loop {
        interval.tick().await;
        match fetch_status(&socket).await {
            Ok(status) => {
                if tx.send(IpcEvent::Status(status)).is_err() {
                    return;
                }
            }
            Err(error) => {
                if tx.send(IpcEvent::Down(error.to_string())).is_err() {
                    return;
                }
            }
        }
    }
}

async fn audit_loop(socket: String, tx: mpsc::UnboundedSender<IpcEvent>) {
    let mut since = 0_u64;
    loop {
        if let Ok(next_since) = subscribe_session(&socket, since, &tx).await {
            since = next_since;
        }
        time::sleep(RECONNECT_BACKOFF).await;
    }
}

async fn fetch_status(socket: &str) -> io::Result<DaemonStatus> {
    let stream = UnixStream::connect(socket).await?;
    let (reader, mut writer) = stream.into_split();
    writer.write_all(b"v1 status\n").await?;
    let mut line = String::new();
    BufReader::new(reader).read_line(&mut line).await?;
    if line.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame_too_large",
        ));
    }
    DaemonStatus::parse(&line)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "malformed_status"))
}

async fn subscribe_session(
    socket: &str,
    since: u64,
    tx: &mpsc::UnboundedSender<IpcEvent>,
) -> io::Result<u64> {
    let stream = UnixStream::connect(socket).await?;
    let (reader, mut writer) = stream.into_split();
    let request = format!("v1 audit-subscribe {AUDIT_SUBSCRIBER_ID} since={since}\n");
    writer.write_all(request.as_bytes()).await?;
    let mut lines = BufReader::new(reader).lines();
    let mut last_since = since;
    while let Some(line) = lines.next_line().await? {
        if line.len() > MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "frame_too_large",
            ));
        }
        if line.starts_with("v1 subscribed ") {
            if tx.send(IpcEvent::SubscriptionReady).is_err() {
                return Ok(last_since);
            }
            continue;
        }
        if line == "v1 audit-replaced" {
            continue;
        }
        match AuditStreamRecord::parse(&line) {
            Ok(record) => {
                last_since = record.sequence;
                if tx.send(IpcEvent::Audit(record)).is_err() {
                    return Ok(last_since);
                }
            }
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "malformed_audit",
                ));
            }
        }
    }
    Ok(last_since)
}

#[cfg(test)]
mod tests {
    use super::AUDIT_SUBSCRIBER_ID;

    #[test]
    fn subscriber_id_is_stable() {
        assert_eq!(AUDIT_SUBSCRIBER_ID, "interfire-tui");
    }
}
