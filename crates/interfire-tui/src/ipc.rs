//! Async Unix IPC client: status, audit stream, rules/prompts list/mutate.
#![forbid(unsafe_code)]

use std::io;
use std::time::Duration;

use interfire_proto::{
    AuditStreamRecord, DaemonStatus, MAX_FRAME_BYTES, ProcessRow, PromptRow, RuleRow,
    parse_error_message,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::time;

/// Fixed subscription id so reconnect replaces the prior stream.
pub const AUDIT_SUBSCRIBER_ID: &str = "interfire-tui";

const STATUS_INTERVAL: Duration = Duration::from_secs(1);
const LIST_INTERVAL: Duration = Duration::from_secs(2);
const RECONNECT_BACKOFF: Duration = Duration::from_millis(500);

/// Events pushed to the UI from background IPC tasks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IpcEvent {
    Status(DaemonStatus),
    Down(String),
    Audit(AuditStreamRecord),
    SubscriptionReady,
    Rules(Vec<RuleRow>),
    Prompts(Vec<PromptRow>),
    Processes(Vec<ProcessRow>),
    ActionOk(String),
    ActionError(String),
}

/// Commands from the UI to the IPC worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IpcCommand {
    RefreshRules,
    RefreshPrompts,
    DeleteRule {
        id: u64,
    },
    AddRule {
        id: u64,
        executable: String,
        verdict: String,
        port: u16,
    },
    AnswerPrompt {
        id: u64,
        verdict: String,
        scope: String,
    },
}

/// Spawn non-blocking status, audit, list poll, and command loops.
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
    let lists_socket = socket.clone();
    let lists_tx = tx.clone();
    tokio::spawn(async move {
        lists_poll_loop(lists_socket, lists_tx).await;
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

async fn lists_poll_loop(socket: String, tx: mpsc::UnboundedSender<IpcEvent>) {
    let mut interval = time::interval(LIST_INTERVAL);
    loop {
        interval.tick().await;
        if let Ok(rules) = fetch_rules(&socket).await
            && tx.send(IpcEvent::Rules(rules)).is_err()
        {
            return;
        }
        if let Ok(prompts) = fetch_prompts(&socket).await
            && tx.send(IpcEvent::Prompts(prompts)).is_err()
        {
            return;
        }
        if let Ok(processes) = fetch_processes(&socket).await
            && tx.send(IpcEvent::Processes(processes)).is_err()
        {
            return;
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
        IpcCommand::RefreshPrompts => match fetch_prompts(socket).await {
            Ok(prompts) => {
                let _ = tx.send(IpcEvent::Prompts(prompts));
            }
            Err(error) => {
                let _ = tx.send(IpcEvent::ActionError(error.to_string()));
            }
        },
        IpcCommand::DeleteRule { id } => {
            mutate_then_refresh_rules(
                socket,
                tx,
                &format!("v1 rule-delete {id}\n"),
                format!("deleted rule {id}"),
            )
            .await;
        }
        IpcCommand::AddRule {
            id,
            executable,
            verdict,
            port,
        } => {
            let request = format!("v1 rule-add {id} {executable} {verdict} {port}\n");
            mutate_then_refresh_rules(socket, tx, &request, format!("added rule {id}")).await;
        }
        IpcCommand::AnswerPrompt { id, verdict, scope } => {
            let request = format!("v1 prompt-answer {id} {verdict} {scope}\n");
            match one_shot(socket, &request).await {
                Ok(frame) if frame.starts_with("v1 pong") => {
                    let _ = tx.send(IpcEvent::ActionOk(format!(
                        "answered prompt {id} {verdict}/{scope}"
                    )));
                    if let Ok(prompts) = fetch_prompts(socket).await {
                        let _ = tx.send(IpcEvent::Prompts(prompts));
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

async fn mutate_then_refresh_rules(
    socket: &str,
    tx: &mpsc::UnboundedSender<IpcEvent>,
    request: &str,
    ok: String,
) {
    match one_shot(socket, request).await {
        Ok(frame) if frame.starts_with("v1 pong") => {
            let _ = tx.send(IpcEvent::ActionOk(ok));
            if let Ok(rules) = fetch_rules(socket).await {
                let _ = tx.send(IpcEvent::Rules(rules));
            }
        }
        Ok(frame) => {
            let message = parse_error_message(&frame).unwrap_or_else(|| frame.trim().to_owned());
            let _ = tx.send(IpcEvent::ActionError(message));
        }
        Err(error) => {
            let _ = tx.send(IpcEvent::ActionError(error.to_string()));
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

async fn fetch_prompts(socket: &str) -> io::Result<Vec<PromptRow>> {
    let frame = one_shot(socket, "v1 prompt-list\n").await?;
    PromptRow::parse_frame(&frame)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "malformed_prompts"))
}

async fn fetch_processes(socket: &str) -> io::Result<Vec<ProcessRow>> {
    let frame = one_shot(socket, "v1 process-list\n").await?;
    ProcessRow::parse_frame(&frame)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "malformed_processes"))
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
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use interfire_proto::{AuditStreamRecord, MAX_FRAME_BYTES, ProcessRow, Response, StatusBody};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::{UnixListener, UnixStream};
    use tokio::sync::mpsc;
    use tokio::time;

    use super::{
        AUDIT_SUBSCRIBER_ID, IpcCommand, IpcEvent, fetch_processes, fetch_prompts, fetch_rules,
        fetch_status, handle_command, one_shot, spawn, subscribe_session,
    };

    #[test]
    fn subscriber_id_is_stable() {
        assert_eq!(AUDIT_SUBSCRIBER_ID, "interfire-tui");
    }

    #[tokio::test]
    async fn fetch_status_rules_prompts_and_processes() {
        let daemon = FakeDaemon::start();
        let status = fetch_status(&daemon.path).await.expect("status");
        assert_eq!(status.enforcement, "nfqueue");
        assert_eq!(status.observation, "attached");

        let rules = fetch_rules(&daemon.path).await.expect("rules");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, 9);

        let prompts = fetch_prompts(&daemon.path).await.expect("prompts");
        assert_eq!(prompts[0].id, 4);

        let processes = fetch_processes(&daemon.path).await.expect("processes");
        assert_eq!(processes[0].pid, 100);
    }

    #[tokio::test]
    async fn fetch_status_rejects_malformed_frame() {
        let path = temp_socket("bad-status");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let (_, mut writer) = stream.into_split();
            writer
                .write_all(b"v1 status enforcement=only\n")
                .await
                .expect("write");
        });
        let error = fetch_status(&path.to_string_lossy())
            .await
            .expect_err("malformed");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        server.await.expect("server");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn one_shot_reports_missing_socket_and_oversized_frame() {
        let missing = one_shot("/tmp/interfire-tui-missing.sock", "v1 status\n").await;
        assert!(missing.is_err());

        let path = temp_socket("large-frame");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let (_, mut writer) = stream.into_split();
            let huge = format!("{}\n", "x".repeat(MAX_FRAME_BYTES + 1));
            writer.write_all(huge.as_bytes()).await.expect("write");
        });
        let error = one_shot(&path.to_string_lossy(), "v1 status\n")
            .await
            .expect_err("too large");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        server.await.expect("server");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn subscribe_session_streams_audit_and_subscription_ready() {
        let daemon = FakeDaemon::start();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let since = subscribe_session(&daemon.path, 0, &tx)
            .await
            .expect("subscribe");
        assert_eq!(since, 1);
        assert_eq!(rx.recv().await, Some(IpcEvent::SubscriptionReady));
        assert_eq!(
            rx.recv().await,
            Some(IpcEvent::Audit(AuditStreamRecord {
                sequence: 1,
                message: "boot".into(),
            }))
        );
        daemon.shutdown().await;
    }

    #[tokio::test]
    async fn subscribe_session_rejects_malformed_audit_and_oversized_lines() {
        let path_malformed = temp_socket("malformed");
        let _ = std::fs::remove_file(&path_malformed);
        let listener = UnixListener::bind(&path_malformed).expect("bind");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let (_, mut writer) = stream.into_split();
            writer
                .write_all(b"v1 subscribed interfire-tui\n")
                .await
                .expect("subscribed");
            writer.write_all(b"v1 audit bad\n").await.expect("audit");
        });
        let (tx, _rx) = mpsc::unbounded_channel();
        let error = subscribe_session(&path_malformed.to_string_lossy(), 0, &tx)
            .await
            .expect_err("malformed");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        server.await.expect("server");
        let _ = std::fs::remove_file(path_malformed);

        let path_large = temp_socket("oversized");
        let _ = std::fs::remove_file(&path_large);
        let listener = UnixListener::bind(&path_large).expect("bind");
        let huge = format!("{}\n", "y".repeat(MAX_FRAME_BYTES + 1));
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let (_, mut writer) = stream.into_split();
            writer
                .write_all(b"v1 subscribed interfire-tui\n")
                .await
                .expect("subscribed");
            writer.write_all(huge.as_bytes()).await.expect("audit");
        });
        let (tx, _rx) = mpsc::unbounded_channel();
        let error = subscribe_session(&path_large.to_string_lossy(), 0, &tx)
            .await
            .expect_err("oversized");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        server.await.expect("server");
        let _ = std::fs::remove_file(path_large);
    }

    #[tokio::test]
    async fn subscribe_session_stops_when_receiver_dropped() {
        let path = temp_socket("drop-rx");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let (_, mut writer) = stream.into_split();
            writer
                .write_all(b"v1 subscribed interfire-tui\n")
                .await
                .expect("subscribed");
            writer.write_all(b"v1 audit 2|line\n").await.expect("audit");
            time::sleep(Duration::from_millis(200)).await;
        });
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        let since = subscribe_session(&path.to_string_lossy(), 0, &tx)
            .await
            .expect("closed rx");
        assert_eq!(since, 0);
        server.await.expect("server");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn handle_command_refresh_mutations_and_errors() {
        let daemon = FakeDaemon::start();
        let (tx, mut rx) = mpsc::unbounded_channel();

        handle_command(&daemon.path, &tx, IpcCommand::RefreshRules).await;
        assert!(matches!(rx.recv().await, Some(IpcEvent::Rules(_))));

        handle_command(&daemon.path, &tx, IpcCommand::RefreshPrompts).await;
        assert!(matches!(rx.recv().await, Some(IpcEvent::Prompts(_))));

        handle_command(
            &daemon.path,
            &tx,
            IpcCommand::AddRule {
                id: 3,
                executable: "/bin/curl".into(),
                verdict: "allow".into(),
                port: 443,
            },
        )
        .await;
        assert!(matches!(rx.recv().await, Some(IpcEvent::ActionOk(_))));
        assert!(matches!(rx.recv().await, Some(IpcEvent::Rules(_))));

        handle_command(&daemon.path, &tx, IpcCommand::DeleteRule { id: 9 }).await;
        assert!(matches!(rx.recv().await, Some(IpcEvent::ActionOk(_))));
        assert!(matches!(rx.recv().await, Some(IpcEvent::Rules(_))));

        handle_command(
            &daemon.path,
            &tx,
            IpcCommand::AnswerPrompt {
                id: 4,
                verdict: "allow".into(),
                scope: "once".into(),
            },
        )
        .await;
        assert!(matches!(rx.recv().await, Some(IpcEvent::ActionOk(_))));
        assert!(matches!(rx.recv().await, Some(IpcEvent::Prompts(_))));

        let bad_path = temp_socket("cmd-error");
        let _ = std::fs::remove_file(&bad_path);
        handle_command(&bad_path.to_string_lossy(), &tx, IpcCommand::RefreshRules).await;
        assert!(matches!(rx.recv().await, Some(IpcEvent::ActionError(_))));

        let error_daemon = FakeDaemon::start_with(FakeDaemonConfig {
            rule_delete_error: true,
            ..FakeDaemonConfig::default()
        });
        handle_command(&error_daemon.path, &tx, IpcCommand::DeleteRule { id: 1 }).await;
        assert!(matches!(rx.recv().await, Some(IpcEvent::ActionError(_))));
    }

    #[tokio::test]
    async fn spawn_polls_status_lists_audit_and_commands() {
        let daemon = FakeDaemon::start();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        spawn(daemon.path.clone(), tx, cmd_rx);

        let status = time::timeout(Duration::from_secs(3), async {
            loop {
                match rx.recv().await {
                    Some(IpcEvent::Status(status)) => break IpcEvent::Status(status),
                    Some(IpcEvent::SubscriptionReady | IpcEvent::Audit(_)) => {}
                    Some(other) => panic!("unexpected event before status: {other:?}"),
                    None => panic!("channel closed before status"),
                }
            }
        })
        .await
        .expect("status timeout");
        assert!(matches!(status, IpcEvent::Status(_)));

        cmd_tx.send(IpcCommand::RefreshRules).expect("send refresh");
        let rules = time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("rules timeout")
            .expect("rules event");
        assert!(matches!(rules, IpcEvent::Rules(_)));

        let lists = time::timeout(Duration::from_secs(4), async {
            let mut saw = [false; 3];
            while saw.iter().any(|hit| !*hit) {
                match rx.recv().await {
                    Some(IpcEvent::Rules(_)) => saw[0] = true,
                    Some(IpcEvent::Prompts(_)) => saw[1] = true,
                    Some(IpcEvent::Processes(_)) => saw[2] = true,
                    Some(IpcEvent::SubscriptionReady | IpcEvent::Audit(_)) => {}
                    other => panic!("unexpected poll event: {other:?}"),
                }
            }
        })
        .await;
        lists.expect("lists poll");

        drop(cmd_tx);
        daemon.shutdown().await;
    }

    #[tokio::test]
    async fn audit_loop_reconnects_after_server_drop() {
        let subscribe_count = Arc::new(AtomicUsize::new(0));
        let daemon = FakeDaemon::start_with(FakeDaemonConfig {
            subscribe_count: subscribe_count.clone(),
            ..FakeDaemonConfig::default()
        });
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        spawn(daemon.path.clone(), tx, cmd_rx);

        let first = time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("first event timeout")
            .expect("first event");
        assert!(matches!(
            first,
            IpcEvent::Status(_) | IpcEvent::SubscriptionReady
        ));

        time::timeout(Duration::from_secs(6), async {
            while subscribe_count.load(Ordering::SeqCst) < 2 {
                if matches!(rx.recv().await, Some(IpcEvent::SubscriptionReady)) {
                    continue;
                }
                time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("second subscribe");

        drop(cmd_tx);
        daemon.shutdown().await;
    }

    struct FakeDaemon {
        path: String,
        shutdown: tokio::sync::oneshot::Sender<()>,
        task: tokio::task::JoinHandle<()>,
    }

    impl FakeDaemon {
        fn start() -> Self {
            Self::start_with(FakeDaemonConfig::default())
        }

        fn start_with(config: FakeDaemonConfig) -> Self {
            let socket_path = temp_socket("daemon");
            let _ = std::fs::remove_file(&socket_path);
            let path = socket_path.to_string_lossy().into_owned();
            let listener = UnixListener::bind(&path).expect("bind listener");
            let (shutdown, stop) = tokio::sync::oneshot::channel();
            let mut stop = stop;
            let task = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        accept = listener.accept() => {
                            let Ok((stream, _)) = accept else { break };
                            let cfg = config.clone();
                            tokio::spawn(handle_fake_client(stream, cfg));
                        }
                        _ = &mut stop => break,
                    }
                }
            });
            Self {
                path,
                shutdown,
                task,
            }
        }

        async fn shutdown(self) {
            let _ = self.shutdown.send(());
            let _ = self.task.await;
        }
    }

    #[derive(Clone)]
    struct FakeDaemonConfig {
        rule_delete_error: bool,
        subscribe_count: Arc<AtomicUsize>,
    }

    impl Default for FakeDaemonConfig {
        fn default() -> Self {
            Self {
                rule_delete_error: false,
                subscribe_count: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    async fn handle_fake_client(stream: UnixStream, config: FakeDaemonConfig) {
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        let Ok(Some(request)) = lines.next_line().await else {
            return;
        };
        if request.starts_with("v1 audit-subscribe") {
            config.subscribe_count.fetch_add(1, Ordering::SeqCst);
            let _ = writer.write_all(b"v1 subscribed interfire-tui\n").await;
            let _ = writer.write_all(b"v1 audit 1|boot\n").await;
            time::sleep(Duration::from_millis(50)).await;
            return;
        }
        let response = match request.as_str() {
            line if line.starts_with("v1 status") => Response::Status(StatusBody {
                enforcement: "nfqueue",
                observation: "attached",
                ipc_version: 1,
                pid: 42,
                rss_kib: 6400,
                cpu_jiffies: 99,
            })
            .encode(),
            line if line.starts_with("v1 rule-list") => {
                Response::Rules("9|/bin/curl|allow|443".into()).encode()
            }
            line if line.starts_with("v1 prompt-list") => {
                Response::Prompts("4|/bin/curl|203.0.113.1|443|tcp|30".into()).encode()
            }
            line if line.starts_with("v1 process-list") => {
                let row = ProcessRow {
                    pid: 100,
                    start_ticks: 50,
                    uid: 1000,
                    executable: "/bin/curl".into(),
                    cmdline: "curl".into(),
                    verdict: "allow".into(),
                    ports: "1.2.3.4:443/allow".into(),
                };
                Response::Processes(row.encode_row()).encode()
            }
            line if line.starts_with("v1 rule-delete") => {
                if config.rule_delete_error {
                    Response::Error("missing rule").encode()
                } else {
                    Response::Pong.encode()
                }
            }
            line if line.starts_with("v1 rule-add") || line.starts_with("v1 prompt-answer") => {
                Response::Pong.encode()
            }
            _ => Response::Error("unknown").encode(),
        };
        let _ = writer.write_all(response.as_bytes()).await;
    }

    fn temp_socket(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        PathBuf::from(format!("/tmp/interfire-tui-{label}-{nanos}.sock"))
    }
}
