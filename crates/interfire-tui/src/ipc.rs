//! Async Unix IPC client: status, audit stream, rules list/mutate.
#![forbid(unsafe_code)]

use std::io;
use std::time::Duration;

use interfire_proto::{
    AuditStreamRecord, DaemonStatus, MAX_FRAME_BYTES, RuleRow, parse_error_message,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::time;

/// Fixed subscription id so reconnect replaces the prior stream.
pub const AUDIT_SUBSCRIBER_ID: &str = "interfire-tui";

const STATUS_INTERVAL: Duration = Duration::from_secs(1);
const RULES_INTERVAL: Duration = Duration::from_secs(2);
const RECONNECT_BACKOFF: Duration = Duration::from_millis(500);

/// Events pushed to the UI from background IPC tasks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IpcEvent {
    Status(DaemonStatus),
    Down(String),
    Audit(AuditStreamRecord),
    SubscriptionReady,
    Rules(Vec<RuleRow>),
    ActionOk(String),
    ActionError(String),
}

/// Commands from the UI to the IPC worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IpcCommand {
    RefreshRules,
    DeleteRule {
        id: u64,
    },
    AddRule {
        id: u64,
        executable: String,
        verdict: String,
        port: u16,
    },
}

/// Spawn non-blocking status, audit, rules poll, and command loops.
pub fn spawn(
    socket: String,
    tx: mpsc::UnboundedSender<IpcEvent>,
    mut commands: mpsc::UnboundedReceiver<IpcCommand>,
) {
    let status_socket = socket.clone();
    let status_tx = tx.clone();
    tokio::spawn(async move {
        status_loop(status_socket, status_tx).await;
    });
    let audit_socket = socket.clone();
    let audit_tx = tx.clone();
    tokio::spawn(async move {
        audit_loop(audit_socket, audit_tx).await;
    });
    let rules_socket = socket.clone();
    let rules_tx = tx.clone();
    tokio::spawn(async move {
        rules_poll_loop(rules_socket, rules_tx).await;
    });
    tokio::spawn(async move {
        while let Some(command) = commands.recv().await {
            handle_command(&socket, &tx, command).await;
        }
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

async fn rules_poll_loop(socket: String, tx: mpsc::UnboundedSender<IpcEvent>) {
    let mut interval = time::interval(RULES_INTERVAL);
    loop {
        interval.tick().await;
        if let Ok(rules) = fetch_rules(&socket).await {
            if tx.send(IpcEvent::Rules(rules)).is_err() {
                return;
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

async fn handle_command(socket: &str, tx: &mpsc::UnboundedSender<IpcEvent>, command: IpcCommand) {
    match command {
        IpcCommand::RefreshRules => match fetch_rules(socket).await {
            Ok(rules) => {
                let _ = tx.send(IpcEvent::Rules(rules));
            }
            Err(error) => {
                let _ = tx.send(IpcEvent::ActionError(error.to_string()));
            }
        },
        IpcCommand::DeleteRule { id } => {
            match one_shot(socket, &format!("v1 rule-delete {id}\n")).await {
                Ok(frame) if frame.starts_with("v1 pong") => {
                    let _ = tx.send(IpcEvent::ActionOk(format!("deleted rule {id}")));
                    if let Ok(rules) = fetch_rules(socket).await {
                        let _ = tx.send(IpcEvent::Rules(rules));
                    }
                }
                Ok(frame) => {
                    let message =
                        parse_error_message(&frame).unwrap_or_else(|| frame.trim().to_owned());
                    let _ = tx.send(IpcEvent::ActionError(message));
                }
                Err(error) => {
                    let _ = tx.send(IpcEvent::ActionError(error.to_string()));
                }
            }
        }
        IpcCommand::AddRule {
            id,
            executable,
            verdict,
            port,
        } => {
            let request = format!("v1 rule-add {id} {executable} {verdict} {port}\n");
            match one_shot(socket, &request).await {
                Ok(frame) if frame.starts_with("v1 pong") => {
                    let _ = tx.send(IpcEvent::ActionOk(format!("added rule {id}")));
                    if let Ok(rules) = fetch_rules(socket).await {
                        let _ = tx.send(IpcEvent::Rules(rules));
                    }
                }
                Ok(frame) => {
                    let message =
                        parse_error_message(&frame).unwrap_or_else(|| frame.trim().to_owned());
                    let _ = tx.send(IpcEvent::ActionError(message));
                }
                Err(error) => {
                    let _ = tx.send(IpcEvent::ActionError(error.to_string()));
                }
            }
        }
    }
}

async fn fetch_status(socket: &str) -> io::Result<DaemonStatus> {
    let frame = one_shot(socket, "v1 status\n").await?;
    DaemonStatus::parse(&frame)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "malformed_status"))
}

async fn fetch_rules(socket: &str) -> io::Result<Vec<RuleRow>> {
    let frame = one_shot(socket, "v1 rule-list\n").await?;
    RuleRow::parse_frame(&frame)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "malformed_rules"))
}

async fn one_shot(socket: &str, request: &str) -> io::Result<String> {
    let stream = UnixStream::connect(socket).await?;
    let (reader, mut writer) = stream.into_split();
    writer.write_all(request.as_bytes()).await?;
    let mut line = String::new();
    BufReader::new(reader).read_line(&mut line).await?;
    if line.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame_too_large",
        ));
    }
    Ok(line)
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
