//! Capped on-disk audit log with replace-on-reconnect streaming.
#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;

use interfire_proto::{BoundedLogRecord, MAX_AUDIT_FILE_BYTES};
use tracing::{debug, warn};

/// Default on-disk audit cap (1 MiB).
pub const DEFAULT_AUDIT_MAX_BYTES: u64 = MAX_AUDIT_FILE_BYTES;

const SUBSCRIBER_CHANNEL_CAPACITY: usize = 256;

#[derive(Clone, Debug)]
enum Fanout {
    Record(BoundedLogRecord),
    Replaced,
}

/// Restart-safe audit store with bounded memory, disk, and subscribers.
pub struct AuditLog {
    path: PathBuf,
    max_bytes: u64,
    max_records: usize,
    next_seq: u64,
    records: VecDeque<BoundedLogRecord>,
    subscribers: HashMap<String, SyncSender<Fanout>>,
}

impl AuditLog {
    /// Open or create an audit log at `path`.
    ///
    /// # Errors
    ///
    /// Returns I/O failures while creating parents or reading an existing file.
    ///
    /// # Panics
    ///
    /// Panics when `max_records` is zero.
    pub fn open(path: impl Into<PathBuf>, max_bytes: u64, max_records: usize) -> io::Result<Self> {
        assert!(max_records > 0, "audit memory must be bounded");
        let path = path.into();
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let mut log = Self {
            path,
            max_bytes,
            max_records,
            next_seq: 1,
            records: VecDeque::new(),
            subscribers: HashMap::new(),
        };
        log.load_from_disk()?;
        Ok(log)
    }

    /// Append a message, persist under the byte cap, and fan out to subscribers.
    pub fn append(&mut self, message: impl Into<String>) {
        let record = BoundedLogRecord {
            sequence: self.next_seq,
            message: message.into(),
        };
        self.next_seq = self.next_seq.saturating_add(1);
        if self.records.len() == self.max_records {
            self.records.pop_front();
        }
        self.records.push_back(record.clone());
        if let Err(error) = self.persist_record(&record) {
            warn!(%error, "audit disk write failed");
        }
        self.fanout(&Fanout::Record(record));
    }

    /// Return up to `limit` recent records (oldest first among the window).
    #[must_use]
    pub fn tail(&self, limit: usize) -> Vec<BoundedLogRecord> {
        let limit = limit.min(self.records.len());
        self.records
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// Records with `sequence > since`.
    #[must_use]
    pub fn since(&self, since: u64) -> Vec<BoundedLogRecord> {
        self.records
            .iter()
            .filter(|record| record.sequence > since)
            .cloned()
            .collect()
    }

    /// Register `id`, replacing any prior subscription with the same id.
    fn subscribe(&mut self, id: &str) -> Receiver<Fanout> {
        let (sender, receiver) = mpsc::sync_channel(SUBSCRIBER_CHANNEL_CAPACITY);
        if let Some(previous) = self.subscribers.insert(id.to_owned(), sender) {
            let _ = previous.try_send(Fanout::Replaced);
            debug!(%id, "audit subscription replaced");
        } else {
            debug!(%id, "audit subscription registered");
        }
        receiver
    }

    /// Start streaming to `stream`, replacing any prior subscription with `id`.
    pub fn attach_subscriber(
        &mut self,
        id: String,
        since: u64,
        stream: std::os::unix::net::UnixStream,
        on_exit: impl FnOnce(&str) + Send + 'static,
    ) {
        let backlog = self.since(since);
        let receiver = self.subscribe(&id);
        spawn_subscriber(id, receiver, stream, backlog, on_exit);
    }

    /// Remove a subscription id after its stream ends.
    pub fn remove_subscriber(&mut self, id: &str) {
        self.subscribers.remove(id);
    }

    /// Number of active subscription ids.
    #[cfg(test)]
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }

    fn fanout(&mut self, event: &Fanout) {
        let mut dead = Vec::new();
        for (id, sender) in &self.subscribers {
            match sender.try_send(event.clone()) {
                Ok(()) => {}
                Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                    dead.push(id.clone());
                }
            }
        }
        for id in dead {
            self.subscribers.remove(&id);
        }
    }

    fn persist_record(&mut self, record: &BoundedLogRecord) -> io::Result<()> {
        let line = format_record(record);
        let line_len = u64::try_from(line.len()).unwrap_or(u64::MAX);
        let current = self.path.metadata().map_or(0, |meta| meta.len());
        if current.saturating_add(line_len) > self.max_bytes {
            self.trim_to_disk_budget();
            self.rewrite_disk_from_memory()?;
            return Ok(());
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&self.path)?;
        file.write_all(line.as_bytes())?;
        file.sync_all()
    }

    fn trim_to_disk_budget(&mut self) {
        while !self.records.is_empty() {
            let encoded: u64 = self
                .records
                .iter()
                .map(|record| u64::try_from(format_record(record).len()).unwrap_or(u64::MAX))
                .fold(0, u64::saturating_add);
            if encoded <= self.max_bytes {
                break;
            }
            self.records.pop_front();
        }
    }

    fn rewrite_disk_from_memory(&self) -> io::Result<()> {
        let temporary = self.path.with_extension("tmp");
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        for record in &self.records {
            file.write_all(format_record(record).as_bytes())?;
        }
        file.sync_all()?;
        fs::rename(&temporary, &self.path)?;
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    fn load_from_disk(&mut self) -> io::Result<()> {
        if !self.path.exists() {
            return Ok(());
        }
        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            if let Some(record) = parse_record(&line) {
                self.next_seq = self.next_seq.max(record.sequence.saturating_add(1));
                if self.records.len() == self.max_records {
                    self.records.pop_front();
                }
                self.records.push_back(record);
            }
        }
        Ok(())
    }
}

fn format_record(record: &BoundedLogRecord) -> String {
    let message = record.message.replace('\n', " ");
    format!("{}\t{message}\n", record.sequence)
}

fn parse_record(line: &str) -> Option<BoundedLogRecord> {
    let (sequence, message) = line.split_once('\t')?;
    Some(BoundedLogRecord {
        sequence: sequence.parse().ok()?,
        message: message.to_owned(),
    })
}

/// Encode a record as a streaming IPC frame.
#[must_use]
pub fn encode_audit_frame(record: &BoundedLogRecord) -> String {
    format!("v1 audit {}|{}\n", record.sequence, record.message)
}

/// Drive a subscriber until replace, disconnect, or channel close.
///
/// Returns `true` when the caller should remove `id` from the registry.
fn run_subscriber(
    receiver: &Receiver<Fanout>,
    mut stream: impl Write,
    backlog: Vec<BoundedLogRecord>,
) -> bool {
    for record in backlog {
        if stream
            .write_all(encode_audit_frame(&record).as_bytes())
            .is_err()
        {
            return true;
        }
    }
    while let Ok(event) = receiver.recv() {
        match event {
            Fanout::Replaced => {
                let _ = stream.write_all(b"v1 audit-replaced\n");
                return false;
            }
            Fanout::Record(record) => {
                if stream
                    .write_all(encode_audit_frame(&record).as_bytes())
                    .is_err()
                {
                    return true;
                }
            }
        }
    }
    true
}

/// Spawn a subscriber thread that exits on replace or write failure.
fn spawn_subscriber(
    id: String,
    receiver: Receiver<Fanout>,
    stream: std::os::unix::net::UnixStream,
    backlog: Vec<BoundedLogRecord>,
    on_exit: impl FnOnce(&str) + Send + 'static,
) {
    thread::spawn(move || {
        if run_subscriber(&receiver, stream, backlog) {
            on_exit(&id);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "interfire-audit-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    #[test]
    fn append_caps_memory_and_survives_reload() {
        let path = temp_path("reload.log");
        let _ = fs::remove_file(&path);
        {
            let mut log = AuditLog::open(&path, 4_096, 2).unwrap();
            log.append("one");
            log.append("two");
            log.append("three");
            assert_eq!(log.tail(10).len(), 2);
            assert_eq!(log.tail(10)[0].message, "two");
        }
        let reloaded = AuditLog::open(&path, 4_096, 2).unwrap();
        assert_eq!(reloaded.tail(10).len(), 2);
        assert_eq!(reloaded.tail(10)[1].message, "three");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn disk_rewrites_when_over_byte_cap() {
        let path = temp_path("cap.log");
        let _ = fs::remove_file(&path);
        let mut log = AuditLog::open(&path, 40, 50).unwrap();
        for index in 0..20 {
            log.append(format!("msg-{index}"));
        }
        let size = fs::metadata(&path).unwrap().len();
        assert!(size <= 40, "size {size} should be capped");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn reconnect_replaces_subscription() {
        let path = temp_path("sub.log");
        let _ = fs::remove_file(&path);
        let mut log = AuditLog::open(&path, 4_096, 100).unwrap();
        let first = log.subscribe("ui");
        assert_eq!(log.subscriber_count(), 1);
        let second = log.subscribe("ui");
        assert_eq!(log.subscriber_count(), 1);
        assert!(matches!(
            first.recv_timeout(Duration::from_millis(50)),
            Ok(Fanout::Replaced)
        ));
        log.append("hello");
        assert!(matches!(
            second.recv_timeout(Duration::from_millis(50)),
            Ok(Fanout::Record(ref record)) if record.message == "hello"
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn since_returns_records_after_sequence() {
        let path = temp_path("since.log");
        let _ = fs::remove_file(&path);
        let mut log = AuditLog::open(&path, 4_096, 100).unwrap();
        log.append("one");
        log.append("two");
        let rows = log.since(1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].message, "two");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn tail_respects_limit_and_order() {
        let path = temp_path("tail.log");
        let _ = fs::remove_file(&path);
        let mut log = AuditLog::open(&path, 4_096, 100).unwrap();
        log.append("a");
        log.append("b");
        log.append("c");
        let rows = log.tail(2);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].message, "b");
        assert_eq!(rows[1].message, "c");
        assert!(log.tail(0).is_empty());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn encode_audit_frame_and_parse_record() {
        let record = BoundedLogRecord {
            sequence: 3,
            message: "line\nbreak".into(),
        };
        assert_eq!(encode_audit_frame(&record), "v1 audit 3|line\nbreak\n");
        assert_eq!(
            parse_record("3\tplain"),
            Some(BoundedLogRecord {
                sequence: 3,
                message: "plain".into(),
            })
        );
        assert!(parse_record("bad").is_none());
        assert!(parse_record("x\t").is_none());
    }

    #[test]
    fn load_from_disk_skips_malformed_lines() {
        let path = temp_path("malformed.log");
        let _ = fs::remove_file(&path);
        fs::write(&path, "1\tgood\nbad-line\n2\talso-good\n").unwrap();
        let log = AuditLog::open(&path, 4_096, 100).unwrap();
        assert_eq!(log.tail(10).len(), 2);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn remove_subscriber_clears_registry() {
        let path = temp_path("remove.log");
        let _ = fs::remove_file(&path);
        let mut log = AuditLog::open(&path, 4_096, 100).unwrap();
        let _receiver = log.subscribe("ui");
        assert_eq!(log.subscriber_count(), 1);
        log.remove_subscriber("ui");
        assert_eq!(log.subscriber_count(), 0);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn fanout_removes_full_subscriber_channel() {
        let path = temp_path("full.log");
        let _ = fs::remove_file(&path);
        let mut log = AuditLog::open(&path, 4_096, 100).unwrap();
        let _receiver = log.subscribe("blocked");
        for index in 0..=SUBSCRIBER_CHANNEL_CAPACITY {
            log.append(format!("fill-{index}"));
        }
        assert_eq!(log.subscriber_count(), 0);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn fanout_drops_disconnected_subscribers() {
        let path = temp_path("fanout.log");
        let _ = fs::remove_file(&path);
        let mut log = AuditLog::open(&path, 4_096, 100).unwrap();
        let receiver = log.subscribe("gone");
        drop(receiver);
        log.append("after-drop");
        assert_eq!(log.subscriber_count(), 0);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn open_creates_parent_directories() {
        let base = temp_path("nested");
        let _ = fs::remove_dir_all(&base);
        let path = base.join("nested/audit.log");
        let mut log = AuditLog::open(&path, 4_096, 100).unwrap();
        assert!(path.parent().unwrap().exists());
        log.append("boot");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn append_survives_read_only_disk_target() {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_path("readonly.log");
        let _ = fs::remove_file(&path);
        let mut log = AuditLog::open(&path, 4_096, 100).unwrap();
        log.append("seed");
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o444);
        fs::set_permissions(&path, perms).unwrap();
        log.append("should warn not panic");
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o644);
        let _ = fs::set_permissions(&path, perms);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn attach_subscriber_invokes_on_exit_when_stream_write_fails() {
        use std::os::unix::net::UnixStream;
        use std::sync::{Arc, Mutex};

        let path = temp_path("attach.log");
        let _ = fs::remove_file(&path);
        let mut log = AuditLog::open(&path, 4_096, 100).unwrap();
        log.append("seed");
        let (client, server) = UnixStream::pair().unwrap();
        drop(client);
        let removed = Arc::new(Mutex::new(false));
        let flag = Arc::clone(&removed);
        log.attach_subscriber("ui".into(), 0, server, move |id| {
            assert_eq!(id, "ui");
            *flag.lock().expect("flag") = true;
        });
        std::thread::sleep(Duration::from_millis(50));
        assert!(*removed.lock().expect("flag"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn run_subscriber_returns_true_when_channel_closes() {
        use std::os::unix::net::UnixStream;

        let (_client, server) = UnixStream::pair().unwrap();
        let (sender, receiver) = mpsc::sync_channel(4);
        drop(sender);
        assert!(run_subscriber(&receiver, server, vec![]));
    }

    #[test]
    fn run_subscriber_returns_true_when_backlog_write_fails() {
        struct FailingWrite;

        impl std::io::Write for FailingWrite {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("write failed"))
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        assert!(FailingWrite.flush().is_ok());

        let (sender, receiver) = mpsc::sync_channel(4);
        drop(sender);
        let record = BoundedLogRecord {
            sequence: 1,
            message: "x".into(),
        };
        assert!(run_subscriber(&receiver, FailingWrite, vec![record],));
    }

    #[test]
    fn run_subscriber_handles_replaced_event() {
        use std::io::Read;
        use std::os::unix::net::UnixStream;

        let (client, server) = UnixStream::pair().unwrap();
        let (sender, receiver) = mpsc::sync_channel(4);
        sender.try_send(Fanout::Replaced).unwrap();
        drop(sender);
        assert!(!run_subscriber(&receiver, server, vec![]));
        let mut client = client;
        let mut buffer = [0_u8; 32];
        let read = client.read(&mut buffer).unwrap();
        assert_eq!(&buffer[..read], b"v1 audit-replaced\n");
    }
}
