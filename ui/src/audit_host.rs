//! Long-lived `audit-subscribe` host with explicit tear-down on drop.
#![forbid(unsafe_code)]

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use interfire_proto::{AuditStreamRecord, MAX_FRAME_BYTES};

/// Fixed subscription id so reconnect replaces the prior stream.
pub const AUDIT_SUBSCRIBER_ID: &str = "interfire-ui";

const RECONNECT_BACKOFF: Duration = Duration::from_millis(500);
const IO_TIMEOUT: Duration = Duration::from_millis(500);

/// Events from the audit host thread to the GPUI app.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuditEvent {
    Ready,
    Record(AuditStreamRecord),
    Down(String),
}

/// Owns the audit subscribe thread; dropping cancels the subscription.
pub struct AuditHost {
    stop: Arc<AtomicBool>,
    rx: Receiver<AuditEvent>,
    join: Option<JoinHandle<()>>,
}

impl AuditHost {
    /// Spawn a background audit subscriber for `socket`.
    #[must_use]
    pub fn spawn(socket: String) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let stop_thread = Arc::clone(&stop);
        let join = thread::Builder::new()
            .name("interfire-audit".into())
            .spawn(move || {
                audit_loop(&socket, &stop_thread, &tx);
            })
            .ok();
        Self { stop, rx, join }
    }

    /// Drain pending events without blocking.
    pub fn drain(&self) -> Vec<AuditEvent> {
        let mut out = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            out.push(event);
        }
        out
    }

    /// Signal stop and join the host thread (also runs on [`Drop`]).
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AuditHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn audit_loop(socket: &str, stop: &Arc<AtomicBool>, tx: &mpsc::Sender<AuditEvent>) {
    let mut since = 0_u64;
    while !stop.load(Ordering::SeqCst) {
        match subscribe_session(socket, since, stop, tx) {
            Ok(next) => since = next,
            Err(reason) => {
                let _ = tx.send(AuditEvent::Down(reason));
            }
        }
        if stop.load(Ordering::SeqCst) {
            break;
        }
        thread::sleep(RECONNECT_BACKOFF);
    }
}

fn subscribe_session(
    socket: &str,
    since: u64,
    stop: &AtomicBool,
    tx: &mpsc::Sender<AuditEvent>,
) -> Result<u64, String> {
    let stream = connect(socket).map_err(|e| e.to_string())?;
    let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
    let request = format!("v1 audit-subscribe {AUDIT_SUBSCRIBER_ID} since={since}\n");
    writer
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;

    let mut reader = BufReader::new(stream);
    let mut last_since = since;
    let mut line = String::new();

    while !stop.load(Ordering::SeqCst) {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Ok(last_since),
            Ok(_) => {
                if line.len() > MAX_FRAME_BYTES {
                    return Err("frame_too_large".into());
                }
                let trimmed = line.trim_end_matches(['\r', '\n']);
                if trimmed.starts_with("v1 subscribed ") {
                    if tx.send(AuditEvent::Ready).is_err() {
                        return Ok(last_since);
                    }
                    continue;
                }
                if trimmed == "v1 audit-replaced" {
                    continue;
                }
                match AuditStreamRecord::parse(trimmed) {
                    Ok(record) => {
                        last_since = record.sequence;
                        if tx.send(AuditEvent::Record(record)).is_err() {
                            return Ok(last_since);
                        }
                    }
                    Err(_) => return Err("malformed_audit".into()),
                }
            }
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(last_since)
}

fn connect(socket: &str) -> io::Result<UnixStream> {
    let stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn subscriber_id_is_stable() {
        assert_eq!(AUDIT_SUBSCRIBER_ID, "interfire-ui");
    }

    #[test]
    fn shutdown_joins_without_daemon() {
        let mut host = AuditHost::spawn("/tmp/interfire-audit-host-missing.sock".into());
        thread::sleep(Duration::from_millis(50));
        host.shutdown();
        assert!(host.join.is_none());
    }

    #[test]
    fn reconnect_reuses_stable_subscriber_id() {
        let path = temp_sock("reconnect");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind audit sock");
        listener.set_nonblocking(false).expect("blocking accept");

        let (seen_tx, seen_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept");
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .expect("read timeout");
                let mut line = String::new();
                BufReader::new(stream.try_clone().expect("clone"))
                    .read_line(&mut line)
                    .expect("read subscribe");
                assert!(
                    line.contains(&format!("audit-subscribe {AUDIT_SUBSCRIBER_ID}")),
                    "unexpected request: {line}"
                );
                let _ = seen_tx.send(line);
                let _ = stream.write_all(b"v1 subscribed interfire-ui\n");
                // Drop stream so the client reconnects with the same id.
            }
        });

        let mut host = AuditHost::spawn(path.to_string_lossy().into_owned());
        let first = seen_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("first subscribe");
        wait_for_ready(&host, Duration::from_secs(2));
        let second = seen_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("second subscribe");
        wait_for_ready(&host, Duration::from_secs(2));
        assert!(first.contains(AUDIT_SUBSCRIBER_ID));
        assert!(second.contains(AUDIT_SUBSCRIBER_ID));
        assert_eq!(
            first.split_whitespace().nth(2),
            second.split_whitespace().nth(2)
        );

        host.shutdown();
        server.join().expect("server");
        let _ = std::fs::remove_file(path);
    }

    fn wait_for_ready(host: &AuditHost, budget: Duration) {
        let deadline = SystemTime::now() + budget;
        loop {
            if host
                .drain()
                .into_iter()
                .any(|event| matches!(event, AuditEvent::Ready))
            {
                return;
            }
            assert!(
                SystemTime::now() <= deadline,
                "timed out waiting for AuditEvent::Ready"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn temp_sock(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "interfire-ui-audit-{}-{}-{name}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }
}
