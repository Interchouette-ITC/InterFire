//! Unix IPC request handling with peer-credential checks on mutate.
#![forbid(unsafe_code)]

use std::io::{self, BufRead, BufReader, Write};
use std::net::{IpAddr, Ipv4Addr};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use interfire_proto::{
    IPC_VERSION, MAX_FRAME_BYTES, ProcessRow, Request, Response, RuleScope, StatusBody,
};
use interfire_rules::{Connection, Direction, Protocol, Rule, Scope, Verdict};
use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
use nix::unistd::Uid;
use tracing::{debug, warn};

use crate::nft;
use crate::prompts::{AnswerError, PromptQueue};
use crate::shared::Shared;

/// Handle one client connection (one-shot or audit stream).
///
/// # Errors
///
/// Returns I/O failures while reading or writing the frame.
pub fn handle(mut stream: UnixStream, shared: &Arc<Shared>) -> io::Result<()> {
    let mut frame = String::new();
    let bytes = BufReader::new(stream.try_clone()?).read_line(&mut frame)?;
    if bytes > MAX_FRAME_BYTES {
        return stream.write_all(Response::Error("frame_too_large").encode().as_bytes());
    }
    match Request::parse(&frame) {
        Ok(Request::AuditSubscribe { id, since }) => audit_subscribe(stream, shared, id, since),
        Ok(request) => {
            let response = dispatch(request, shared, &stream);
            stream.write_all(response.encode().as_bytes())
        }
        Err(_) => stream.write_all(Response::Error("malformed_request").encode().as_bytes()),
    }
}

fn dispatch(request: Request, shared: &Shared, stream: &UnixStream) -> Response {
    match request {
        Request::Ping => Response::Pong,
        Request::Status => {
            let metrics = crate::proc_metrics::sample_self().unwrap_or_else(|_| {
                crate::proc_metrics::SelfMetrics {
                    pid: std::process::id(),
                    rss_kib: 0,
                    cpu_jiffies: 0,
                }
            });
            Response::Status(StatusBody {
                enforcement: shared.enforcement(),
                observation: shared.observation(),
                ipc_version: IPC_VERSION,
                pid: metrics.pid,
                rss_kib: metrics.rss_kib,
                cpu_jiffies: metrics.cpu_jiffies,
            })
        }
        Request::RuleList => rule_list(shared),
        Request::RuleDelete { id } => mutate(shared, stream, Mutate::Delete { id }),
        Request::RuleAdd {
            id,
            executable,
            verdict,
            port,
        } => mutate(
            shared,
            stream,
            Mutate::Add {
                id,
                executable,
                verdict,
                port,
            },
        ),
        Request::PromptList => prompt_list(shared),
        Request::PromptAnswer { id, verdict, scope } => {
            prompt_answer(shared, stream, id, &verdict, &scope)
        }
        Request::DnsList => dns_list(shared),
        Request::DnsNote {
            hostname,
            ipv4,
            ttl_secs,
        } => dns_note(shared, stream, &hostname, &ipv4, ttl_secs),
        Request::AuditTail { limit } => audit_tail(shared, limit),
        Request::AuditSubscribe { .. } => Response::Error("malformed_request"),
        Request::ProcessList => process_list(shared),
        Request::NetworkStatus => Response::Network(nft::status()),
        Request::NetworkInstall => network_mutate(stream, NetworkMutate::Install),
        Request::NetworkRemove => network_mutate(stream, NetworkMutate::Remove),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NetworkMutate {
    Install,
    Remove,
}

fn network_mutate(stream: &UnixStream, action: NetworkMutate) -> Response {
    if !peer_may_mutate(stream) {
        warn!("network mutate rejected: unauthorized peer");
        return Response::Error("unauthorized");
    }
    let result = match action {
        NetworkMutate::Install => nft::install(),
        NetworkMutate::Remove => nft::remove(),
    };
    match result {
        Ok(()) => Response::Pong,
        Err(error) => {
            warn!(%error, "network mutate failed");
            match action {
                NetworkMutate::Install => Response::Error("nft_install_failed"),
                NetworkMutate::Remove => Response::Error("nft_remove_failed"),
            }
        }
    }
}

fn process_list(shared: &Shared) -> Response {
    let Ok(cache) = shared.process_cache.lock() else {
        return Response::Error("lock_poisoned");
    };
    let Ok(recent) = shared.recent.lock() else {
        return Response::Error("lock_poisoned");
    };
    let Ok(rules) = shared.rules.lock() else {
        return Response::Error("lock_poisoned");
    };
    let rows: Vec<String> = cache
        .list()
        .into_iter()
        .map(|identity| {
            let ports = recent
                .for_process(identity.pid, identity.start_ticks)
                .into_iter()
                .map(|dest| format!("{}:{}/{}", dest.ipv4, dest.port, dest.verdict))
                .collect::<Vec<_>>()
                .join("+");
            let connection = Connection {
                executable: identity.executable.display().to_string(),
                protocol: Protocol::Tcp,
                direction: Direction::Outbound,
                address: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                hostname: None,
                port: 0,
            };
            let verdict = format!("{:?}", rules.verdict_for(&connection)).to_ascii_lowercase();
            ProcessRow {
                pid: identity.pid,
                start_ticks: identity.start_ticks,
                uid: identity.uid,
                executable: identity.executable.display().to_string(),
                cmdline: identity.command_line.join(" "),
                verdict,
                ports,
            }
            .encode_row()
        })
        .collect();
    Response::Processes(rows.join(";"))
}

fn audit_tail(shared: &Shared, limit: usize) -> Response {
    let Ok(audit) = shared.audit.lock() else {
        return Response::Error("lock_poisoned");
    };
    Response::Audit(
        audit
            .tail(limit)
            .into_iter()
            .map(|record| format!("{}|{}", record.sequence, record.message))
            .collect::<Vec<_>>()
            .join(","),
    )
}

fn audit_subscribe(
    mut stream: UnixStream,
    shared: &Arc<Shared>,
    id: String,
    since: u64,
) -> io::Result<()> {
    if id.is_empty() || id.contains('|') || id.contains(' ') {
        return stream.write_all(Response::Error("invalid_subscriber").encode().as_bytes());
    }
    {
        let Ok(mut audit) = shared.audit.lock() else {
            return stream.write_all(Response::Error("lock_poisoned").encode().as_bytes());
        };
        stream.write_all(Response::Subscribed(id.clone()).encode().as_bytes())?;
        let shared_exit = Arc::clone(shared);
        audit.attach_subscriber(id, since, stream, move |subscriber| {
            if let Ok(mut audit) = shared_exit.audit.lock() {
                audit.remove_subscriber(subscriber);
            }
        });
    }
    Ok(())
}

enum Mutate {
    Delete {
        id: u64,
    },
    Add {
        id: u64,
        executable: String,
        verdict: String,
        port: u16,
    },
}

fn rule_list(shared: &Shared) -> Response {
    let Ok(rules) = shared.rules.lock() else {
        return Response::Error("lock_poisoned");
    };
    Response::Rules(
        rules
            .rules()
            .iter()
            .map(|rule| {
                format!(
                    "{}|{}|{:?}|{}",
                    rule.id,
                    rule.executable,
                    rule.verdict,
                    rule.port.unwrap_or(0)
                )
            })
            .collect::<Vec<_>>()
            .join(","),
    )
}

fn prompt_list(shared: &Shared) -> Response {
    let Ok(mut prompts) = shared.prompts.lock() else {
        return Response::Error("lock_poisoned");
    };
    Response::Prompts(encode_prompts(&mut prompts))
}

fn encode_prompts(prompts: &mut PromptQueue) -> String {
    let now = std::time::Instant::now();
    prompts
        .list_pending()
        .into_iter()
        .map(|prompt| {
            let remaining = prompt.expires_at.saturating_duration_since(now).as_secs();
            format!(
                "{}|{}|{}|{}|tcp|{}",
                prompt.id,
                prompt.key.executable,
                prompt.key.ipv4_display(),
                prompt.key.port,
                remaining
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn dns_list(shared: &Shared) -> Response {
    let Ok(mut dns) = shared.dns.lock() else {
        return Response::Error("lock_poisoned");
    };
    Response::Dns(
        dns.list_fresh()
            .into_iter()
            .map(|(hostname, ipv4, ttl)| {
                format!("{hostname}|{}|{ttl}", Ipv4Addr::from(ipv4.to_ne_bytes()))
            })
            .collect::<Vec<_>>()
            .join(","),
    )
}

fn dns_note(
    shared: &Shared,
    stream: &UnixStream,
    hostname: &str,
    ipv4: &str,
    ttl_secs: Option<u64>,
) -> Response {
    if !peer_may_mutate(stream) {
        warn!("rejected DNS note without peer credentials");
        return Response::Error("unauthorized");
    }
    if hostname.is_empty() || hostname.contains('|') {
        return Response::Error("invalid_hostname");
    }
    let Ok(addr) = Ipv4Addr::from_str(ipv4) else {
        return Response::Error("invalid_ipv4");
    };
    let ipv4 = u32::from_ne_bytes(addr.octets());
    let ttl = ttl_secs.map(Duration::from_secs);
    let Ok(mut dns) = shared.dns.lock() else {
        return Response::Error("lock_poisoned");
    };
    dns.observe(hostname, ipv4, ttl);
    debug!(%hostname, %addr, "DNS observation noted");
    Response::Pong
}

fn prompt_answer(
    shared: &Shared,
    stream: &UnixStream,
    id: u64,
    verdict: &str,
    scope: &str,
) -> Response {
    if !peer_may_mutate(stream) {
        warn!("rejected prompt answer without peer credentials");
        return Response::Error("unauthorized");
    }
    let verdict = match verdict {
        "allow" => Verdict::Allow,
        "deny" => Verdict::Deny,
        _ => return Response::Error("invalid_verdict"),
    };
    let scope = match scope {
        "once" => RuleScope::Once,
        "session" => RuleScope::Session,
        "permanent" => RuleScope::Permanent,
        _ => return Response::Error("invalid_scope"),
    };
    let Ok(mut prompts) = shared.prompts.lock() else {
        return Response::Error("lock_poisoned");
    };
    let answered = match prompts.answer(id, verdict, scope) {
        Ok(answered) => answered,
        Err(AnswerError::NotFound) => return Response::Error("prompt_not_found"),
        Err(AnswerError::Expired) => return Response::Error("prompt_expired"),
    };
    drop(prompts);
    if !answered.duplicate
        && matches!(answered.scope, RuleScope::Session | RuleScope::Permanent)
        && let Err(response) = persist_answered_rule(shared, &answered)
    {
        return response;
    }
    debug!(
        id = answered.id,
        duplicate = answered.duplicate,
        ?answered.verdict,
        ?answered.scope,
        "prompt answered"
    );
    Response::Pong
}

fn persist_answered_rule(
    shared: &Shared,
    answered: &crate::prompts::Answered,
) -> Result<(), Response> {
    let Ok(mut rules) = shared.rules.lock() else {
        return Err(Response::Error("lock_poisoned"));
    };
    let next_id = rules
        .rules()
        .iter()
        .map(|rule| rule.id)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let rule = Rule {
        id: next_id,
        executable: answered.key.executable.clone(),
        protocol: Some(Protocol::Tcp),
        direction: Some(Direction::Outbound),
        address: None,
        hostname: None,
        port: Some(answered.key.port),
        verdict: answered.verdict,
        scope: match answered.scope {
            RuleScope::Once => Scope::Once,
            RuleScope::Session => Scope::Session,
            RuleScope::Permanent => Scope::Permanent,
        },
    };
    if rules.insert(rule).is_err() {
        return Err(Response::Error("invalid_rule"));
    }
    if answered.scope == RuleScope::Permanent && shared.store.save(&rules).is_err() {
        return Err(Response::Error("persistence_failed"));
    }
    Ok(())
}

fn mutate(shared: &Shared, stream: &UnixStream, action: Mutate) -> Response {
    if !peer_may_mutate(stream) {
        warn!("rejected policy mutate without peer credentials");
        return Response::Error("unauthorized");
    }
    let Ok(mut rules) = shared.rules.lock() else {
        return Response::Error("lock_poisoned");
    };
    match action {
        Mutate::Delete { id } => {
            if !rules.remove(id) {
                return Response::Error("rule_not_found");
            }
            if shared.store.save(&rules).is_err() {
                return Response::Error("persistence_failed");
            }
            debug!(id, "rule deleted");
            Response::Pong
        }
        Mutate::Add {
            id,
            executable,
            verdict,
            port,
        } => {
            let verdict = match verdict.as_str() {
                "allow" => Verdict::Allow,
                "deny" => Verdict::Deny,
                "prompt" => Verdict::Prompt,
                _ => return Response::Error("invalid_verdict"),
            };
            let rule = Rule {
                id,
                executable,
                protocol: Some(Protocol::Tcp),
                direction: Some(Direction::Outbound),
                address: None,
                hostname: None,
                port: Some(port),
                verdict,
                scope: Scope::Permanent,
            };
            match rules.insert(rule) {
                Ok(()) if shared.store.save(&rules).is_ok() => {
                    debug!(id, "rule added");
                    Response::Pong
                }
                Ok(()) => Response::Error("persistence_failed"),
                Err(_) => Response::Error("invalid_rule"),
            }
        }
    }
}

/// Allow mutate when the peer UID matches the daemon UID or is root.
#[must_use]
pub fn peer_may_mutate(stream: &UnixStream) -> bool {
    let Ok(cred) = getsockopt(&stream.as_fd(), PeerCredentials) else {
        return false;
    };
    let peer = Uid::from_raw(cred.uid());
    peer.is_root() || peer == Uid::current()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{BufRead, BufReader, Write};
    use std::net::Ipv4Addr;
    use std::os::unix::net::UnixStream as StdUnixStream;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use interfire_proto::{
        DaemonStatus, IPC_VERSION, MAX_FRAME_BYTES, Request, Response, RuleScope,
        parse_error_message,
    };
    use interfire_rules::{Direction, Protocol, Rule, RuleSet, RulesStore, Scope, Verdict};

    use crate::ipc_test_support::{stream_without_peer_creds, suite_lock};
    use crate::process::ProcessIdentity;
    use crate::prompts::{EnqueueOutcome, PromptKey};
    use crate::recent::RecentDest;
    use crate::shared::Shared;

    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_paths(tag: &str) -> (PathBuf, PathBuf) {
        let serial = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!(
            "interfire-ipc-{}-{tag}-{serial}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        (
            base.with_extension("audit.log"),
            base.with_extension("rules.toml"),
        )
    }

    fn test_shared(observation: &'static str) -> (Arc<Shared>, PathBuf, PathBuf) {
        let (audit_path, rules_path) = temp_paths("shared");
        let _ = fs::remove_file(&audit_path);
        let _ = fs::remove_file(&rules_path);
        let shared = Arc::new(
            Shared::new(
                RuleSet::default(),
                RulesStore::new(&rules_path),
                observation,
                8,
                Duration::from_secs(60),
                8,
                audit_path.clone(),
            )
            .expect("shared state"),
        );
        (shared, audit_path, rules_path)
    }

    fn cleanup_paths(audit_path: &PathBuf, rules_path: &PathBuf) {
        let _ = fs::remove_file(audit_path);
        let _ = fs::remove_file(rules_path);
    }

    fn exchange(request: &str, shared: &Arc<Shared>) -> String {
        let (mut client, server) = StdUnixStream::pair().expect("socket pair");
        client.write_all(request.as_bytes()).expect("write request");
        handle(server, shared).expect("handle");
        let mut response = String::new();
        BufReader::new(&mut client)
            .read_line(&mut response)
            .expect("read response");
        response
    }

    fn error_code(response: &str) -> String {
        parse_error_message(response).expect("error frame")
    }

    enum PoisonTarget {
        Rules,
        Prompts,
        Dns,
        Audit,
        ProcessCache,
        Recent,
    }

    fn poison(shared: &Arc<Shared>, target: PoisonTarget) {
        let shared = Arc::clone(shared);
        let handle = std::thread::spawn(move || match target {
            PoisonTarget::Rules => {
                let _guard = shared.rules.lock().expect("rules lock");
                panic!("poison mutex");
            }
            PoisonTarget::Prompts => {
                let _guard = shared.prompts.lock().expect("prompts lock");
                panic!("poison mutex");
            }
            PoisonTarget::Dns => {
                let _guard = shared.dns.lock().expect("dns lock");
                panic!("poison mutex");
            }
            PoisonTarget::Audit => {
                let _guard = shared.audit.lock().expect("audit lock");
                panic!("poison mutex");
            }
            PoisonTarget::ProcessCache => {
                let _guard = shared.process_cache.lock().expect("process cache lock");
                panic!("poison mutex");
            }
            PoisonTarget::Recent => {
                let _guard = shared.recent.lock().expect("recent lock");
                panic!("poison mutex");
            }
        });
        let _ = handle.join();
    }

    fn enqueue_prompt(shared: &Arc<Shared>, executable: &str, port: u16) -> u64 {
        let key = PromptKey {
            executable: executable.into(),
            ipv4: u32::from_ne_bytes([203, 0, 113, 10]),
            port,
        };
        let outcome = shared.prompts.lock().expect("prompts").enqueue(key);
        match outcome {
            EnqueueOutcome::Created(id) => id,
            other => panic!("expected created prompt, got {other:?}"),
        }
    }

    #[test]
    fn peer_cred_allows_same_uid_pair() {
        let _suite = suite_lock();
        let (a, _b) = StdUnixStream::pair().unwrap();
        assert!(peer_may_mutate(&a));
    }

    #[test]
    fn peer_cred_rejects_non_socket_stream() {
        let _suite = suite_lock();
        let stream = stream_without_peer_creds();
        assert!(!peer_may_mutate(&stream));
    }

    #[test]
    fn handle_ping_and_status() {
        let _suite = suite_lock();
        let (shared, audit_path, rules_path) = test_shared("attached");
        shared.set_enforcement("nfqueue");
        let pong = exchange("v1 ping\n", &shared);
        assert_eq!(pong.trim(), "v1 pong");
        let status = exchange("v1 status\n", &shared);
        let parsed = DaemonStatus::parse(&status).expect("status frame");
        assert_eq!(parsed.enforcement, "nfqueue");
        assert_eq!(parsed.observation, "attached");
        assert_eq!(parsed.ipc_version, IPC_VERSION);
        assert!(parsed.pid.is_some());
        cleanup_paths(&audit_path, &rules_path);
    }

    #[test]
    fn handle_malformed_and_oversized_frames() {
        let _suite = suite_lock();
        let (shared, audit_path, rules_path) = test_shared("degraded");
        let malformed = exchange("not-a-frame\n", &shared);
        assert_eq!(error_code(&malformed), "malformed_request");
        let mut oversized = String::from("v1 ping ");
        oversized.push_str(&"x".repeat(MAX_FRAME_BYTES));
        oversized.push('\n');
        let (mut client, server) = StdUnixStream::pair().expect("pair");
        client.write_all(oversized.as_bytes()).expect("write");
        handle(server, &shared).expect("handle");
        let mut response = String::new();
        BufReader::new(&mut client)
            .read_line(&mut response)
            .expect("read");
        assert_eq!(error_code(&response), "frame_too_large");
        cleanup_paths(&audit_path, &rules_path);
    }

    #[test]
    fn dispatch_audit_subscribe_is_malformed() {
        let _suite = suite_lock();
        let (shared, audit_path, rules_path) = test_shared("attached");
        let (client, _server) = StdUnixStream::pair().expect("pair");
        let response = dispatch(
            Request::AuditSubscribe {
                id: "ui".into(),
                since: 0,
            },
            &shared,
            &client,
        );
        assert_eq!(response, Response::Error("malformed_request"));
        cleanup_paths(&audit_path, &rules_path);
    }

    #[test]
    fn rule_list_and_mutations() {
        let _suite = suite_lock();
        let (shared, audit_path, rules_path) = test_shared("attached");
        let empty = exchange("v1 rule-list\n", &shared);
        assert_eq!(empty.trim(), "v1 rules");
        assert_eq!(
            error_code(&exchange("v1 rule-add 1 curl allow 443\n", &shared)),
            "invalid_rule"
        );
        assert_eq!(
            error_code(&exchange("v1 rule-add 1 /usr/bin/curl bad 443\n", &shared)),
            "invalid_verdict"
        );
        assert!(
            exchange("v1 rule-add 1 /usr/bin/curl allow 443\n", &shared)
                .trim()
                .ends_with("pong")
        );
        assert!(
            exchange("v1 rule-add 2 /usr/bin/curl prompt 80\n", &shared)
                .trim()
                .ends_with("pong")
        );
        let listed = exchange("v1 rule-list\n", &shared);
        assert!(listed.contains("1|/usr/bin/curl|Allow|443"));
        assert!(listed.contains("2|/usr/bin/curl|Prompt|80"));
        assert_eq!(
            error_code(&exchange("v1 rule-add 1 /usr/bin/curl deny 80\n", &shared)),
            "invalid_rule"
        );
        assert_eq!(
            error_code(&exchange("v1 rule-delete 99\n", &shared)),
            "rule_not_found"
        );
        assert!(
            exchange("v1 rule-delete 1\n", &shared)
                .trim()
                .ends_with("pong")
        );
        cleanup_paths(&audit_path, &rules_path);
    }

    #[test]
    fn rule_mutate_unauthorized_without_peer_creds() {
        let _suite = suite_lock();
        let (shared, audit_path, rules_path) = test_shared("attached");
        let stream = stream_without_peer_creds();
        assert_eq!(
            dispatch(Request::RuleDelete { id: 1 }, &shared, &stream),
            Response::Error("unauthorized")
        );
        assert_eq!(
            dispatch(
                Request::RuleAdd {
                    id: 2,
                    executable: "/bin/curl".into(),
                    verdict: "allow".into(),
                    port: 443,
                },
                &shared,
                &stream,
            ),
            Response::Error("unauthorized")
        );
        cleanup_paths(&audit_path, &rules_path);
    }

    #[test]
    fn rule_persistence_failure_surfaces_error() {
        let _suite = suite_lock();
        let (audit_path, _) = temp_paths("persist");
        let _ = fs::remove_file(&audit_path);
        let blocker = audit_path.with_extension("blocker");
        fs::write(&blocker, "x").expect("write blocker");
        let store = RulesStore::new(blocker.join("rules.toml"));
        let shared = Arc::new(
            Shared::new(
                RuleSet::default(),
                store,
                "attached",
                8,
                Duration::from_secs(60),
                8,
                audit_path.clone(),
            )
            .expect("shared"),
        );
        assert_eq!(
            error_code(&exchange(
                "v1 rule-add 1 /usr/bin/curl allow 443\n",
                &shared
            )),
            "persistence_failed"
        );
        shared
            .rules
            .lock()
            .expect("rules")
            .insert(Rule {
                id: 3,
                executable: "/usr/bin/curl".into(),
                protocol: Some(Protocol::Tcp),
                direction: Some(Direction::Outbound),
                address: None,
                hostname: None,
                port: Some(443),
                verdict: Verdict::Allow,
                scope: Scope::Permanent,
            })
            .expect("seed rule");
        assert_eq!(
            error_code(&exchange("v1 rule-delete 3\n", &shared)),
            "persistence_failed"
        );
        let _ = fs::remove_file(audit_path);
        let _ = fs::remove_file(blocker);
    }

    #[test]
    fn prompt_list_and_answer_paths() {
        let _suite = suite_lock();
        let (shared, audit_path, rules_path) = test_shared("attached");
        assert_eq!(exchange("v1 prompt-list\n", &shared).trim(), "v1 prompts");
        let id = enqueue_prompt(&shared, "/usr/bin/curl", 443);
        let listed = exchange("v1 prompt-list\n", &shared);
        assert!(listed.contains("/usr/bin/curl"));
        assert!(listed.contains("203.0.113.10"));
        assert!(
            exchange(&format!("v1 prompt-answer {id} allow once\n"), &shared)
                .trim()
                .ends_with("pong")
        );
        let session_id = enqueue_prompt(&shared, "/usr/bin/wget", 80);
        assert!(
            exchange(
                &format!("v1 prompt-answer {session_id} deny session\n"),
                &shared
            )
            .trim()
            .ends_with("pong")
        );
        let permanent_id = enqueue_prompt(&shared, "/usr/bin/ssh", 22);
        assert!(
            exchange(
                &format!("v1 prompt-answer {permanent_id} allow permanent\n"),
                &shared
            )
            .trim()
            .ends_with("pong")
        );
        assert!(
            exchange(
                &format!("v1 prompt-answer {permanent_id} deny permanent\n"),
                &shared
            )
            .trim()
            .ends_with("pong")
        );
        assert_eq!(
            error_code(&exchange("v1 prompt-answer 999 allow once\n", &shared)),
            "prompt_not_found"
        );
        assert_eq!(
            error_code(&exchange(
                &format!("v1 prompt-answer {id} bad once\n"),
                &shared
            )),
            "invalid_verdict"
        );
        assert_eq!(
            error_code(&exchange(
                &format!("v1 prompt-answer {id} allow bad\n"),
                &shared
            )),
            "invalid_scope"
        );
        cleanup_paths(&audit_path, &rules_path);
    }

    #[test]
    fn prompt_answer_expired_and_invalid_rule() {
        let _suite = suite_lock();
        let (audit_path, rules_path) = temp_paths("short");
        let _ = fs::remove_file(&audit_path);
        let _ = fs::remove_file(&rules_path);
        let short = Arc::new(
            Shared::new(
                RuleSet::default(),
                RulesStore::new(&rules_path),
                "attached",
                8,
                Duration::from_millis(1),
                8,
                audit_path.clone(),
            )
            .expect("short shared"),
        );
        short.set_prompt_queue(crate::prompts::PromptQueue::new(
            4,
            Duration::from_millis(1),
        ));
        let id = enqueue_prompt(&short, "/usr/bin/curl", 9);
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(
            error_code(&exchange(
                &format!("v1 prompt-answer {id} allow once\n"),
                &short
            )),
            "prompt_expired"
        );
        let bad_id = enqueue_prompt(&short, "curl", 8080);
        assert_eq!(
            error_code(&exchange(
                &format!("v1 prompt-answer {bad_id} allow session\n"),
                &short
            )),
            "invalid_rule"
        );
        cleanup_paths(&audit_path, &rules_path);
    }

    #[test]
    fn prompt_answer_unauthorized() {
        let _suite = suite_lock();
        let (shared, audit_path, rules_path) = test_shared("attached");
        let stream = stream_without_peer_creds();
        assert_eq!(
            dispatch(
                Request::PromptAnswer {
                    id: 1,
                    verdict: "allow".into(),
                    scope: "once".into(),
                },
                &shared,
                &stream,
            ),
            Response::Error("unauthorized")
        );
        cleanup_paths(&audit_path, &rules_path);
    }

    #[test]
    fn dns_list_and_note_validation() {
        let _suite = suite_lock();
        let (shared, audit_path, rules_path) = test_shared("attached");
        let (client, _server) = StdUnixStream::pair().expect("pair");
        assert_eq!(exchange("v1 dns-list\n", &shared).trim(), "v1 dns");
        assert!(
            exchange("v1 dns-note example.com 203.0.113.1 120\n", &shared)
                .trim()
                .ends_with("pong")
        );
        let listed = exchange("v1 dns-list\n", &shared);
        assert!(listed.contains("example.com|203.0.113.1|"));
        assert_eq!(
            dispatch(
                Request::DnsNote {
                    hostname: String::new(),
                    ipv4: "203.0.113.1".into(),
                    ttl_secs: None,
                },
                &shared,
                &client,
            ),
            Response::Error("invalid_hostname")
        );
        assert_eq!(
            error_code(&exchange("v1 dns-note bad|name 203.0.113.1\n", &shared)),
            "invalid_hostname"
        );
        assert_eq!(
            error_code(&exchange("v1 dns-note example.com not-an-ip\n", &shared)),
            "invalid_ipv4"
        );
        let stream = stream_without_peer_creds();
        assert_eq!(
            dispatch(
                Request::DnsNote {
                    hostname: "example.com".into(),
                    ipv4: "203.0.113.2".into(),
                    ttl_secs: None,
                },
                &shared,
                &stream,
            ),
            Response::Error("unauthorized")
        );
        cleanup_paths(&audit_path, &rules_path);
    }

    #[test]
    fn audit_tail_and_subscribe() {
        let _suite = suite_lock();
        let (shared, audit_path, rules_path) = test_shared("attached");
        shared.audit.lock().expect("audit").append("one");
        shared.audit.lock().expect("audit").append("two");
        let tail = exchange("v1 audit-tail 1\n", &shared);
        assert!(tail.contains("|two"));
        let (mut client, server) = StdUnixStream::pair().expect("pair");
        audit_subscribe(server, &shared, String::new(), 0).expect("subscribe");
        let mut invalid = String::new();
        BufReader::new(&mut client)
            .read_line(&mut invalid)
            .expect("read");
        assert_eq!(error_code(&invalid), "invalid_subscriber");
        assert_eq!(
            error_code(&exchange("v1 audit-subscribe bad|id\n", &shared)),
            "invalid_subscriber"
        );
        let (mut space_client, space_server) = StdUnixStream::pair().expect("pair");
        audit_subscribe(space_server, &shared, "bad id".into(), 0).expect("subscribe");
        let mut space_invalid = String::new();
        BufReader::new(&mut space_client)
            .read_line(&mut space_invalid)
            .expect("read");
        assert_eq!(error_code(&space_invalid), "invalid_subscriber");
        shared.audit.lock().expect("audit").append("before");
        let (mut client, server) = StdUnixStream::pair().expect("pair");
        client
            .write_all(b"v1 audit-subscribe ui since=2\n")
            .expect("write");
        handle(server, &shared).expect("handle");
        let mut line = String::new();
        BufReader::new(&mut client)
            .read_line(&mut line)
            .expect("subscribed");
        assert_eq!(line.trim(), "v1 subscribed ui");
        client
            .set_read_timeout(Some(Duration::from_millis(500)))
            .expect("timeout");
        std::thread::sleep(Duration::from_millis(50));
        line.clear();
        BufReader::new(&mut client)
            .read_line(&mut line)
            .expect("backlog");
        assert!(line.contains("before"), "backlog frame: {line:?}");
        shared.audit.lock().expect("audit").append("live");
        line.clear();
        BufReader::new(&mut client)
            .read_line(&mut line)
            .expect("live");
        assert!(line.contains("live"));
        drop(client);
        shared.audit.lock().expect("audit").append("flush");
        for _ in 0..50 {
            if shared.audit.lock().expect("audit").subscriber_count() == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(shared.audit.lock().expect("audit").subscriber_count(), 0);
        cleanup_paths(&audit_path, &rules_path);
    }

    #[test]
    fn process_list_returns_rows() {
        let _suite = suite_lock();
        let (shared, audit_path, rules_path) = test_shared("attached");
        let empty = exchange("v1 process-list\n", &shared);
        assert_eq!(empty.trim(), "v1 processes");
        {
            let mut cache = shared.process_cache.lock().expect("cache");
            cache.insert(ProcessIdentity {
                pid: 4242,
                start_ticks: 99,
                executable: "/usr/bin/curl".into(),
                command_line: vec!["curl".into(), "-s".into()],
                uid: 1000,
                cgroup: String::new(),
            });
        }
        {
            let mut recent = shared.recent.lock().expect("recent");
            recent.record(
                4242,
                99,
                RecentDest {
                    ipv4: Ipv4Addr::new(203, 0, 113, 5),
                    port: 443,
                    verdict: "allow".into(),
                },
            );
        }
        {
            let mut rules = shared.rules.lock().expect("rules");
            rules
                .insert(Rule {
                    id: 7,
                    executable: "/usr/bin/curl".into(),
                    protocol: Some(Protocol::Tcp),
                    direction: Some(Direction::Outbound),
                    address: None,
                    hostname: None,
                    port: Some(443),
                    verdict: Verdict::Allow,
                    scope: Scope::Permanent,
                })
                .expect("rule");
        }
        let frame = exchange("v1 process-list\n", &shared);
        assert!(frame.contains("4242|99|1000|prompt|203.0.113.5:443/allow|/usr/bin/curl|curl -s"));
        cleanup_paths(&audit_path, &rules_path);
    }

    #[test]
    fn network_status_and_mutations() {
        let _suite = suite_lock();
        let (shared, audit_path, rules_path) = test_shared("attached");
        let status = exchange("v1 network-status\n", &shared);
        assert!(status.starts_with("v1 network "));
        let install = exchange("v1 network-install\n", &shared);
        assert!(install.trim().ends_with("pong") || error_code(&install) == "nft_install_failed");
        let remove = exchange("v1 network-remove\n", &shared);
        assert!(remove.trim().ends_with("pong") || error_code(&remove) == "nft_remove_failed");
        let stream = stream_without_peer_creds();
        assert_eq!(
            dispatch(Request::NetworkInstall, &shared, &stream),
            Response::Error("unauthorized")
        );
        assert_eq!(
            dispatch(Request::NetworkRemove, &shared, &stream),
            Response::Error("unauthorized")
        );
        cleanup_paths(&audit_path, &rules_path);
    }

    #[test]
    fn status_uses_zero_metrics_when_sample_fails() {
        let _suite = suite_lock();
        let (shared, audit_path, rules_path) = test_shared("attached");
        let _fail = crate::proc_metrics::ForceSampleFailure::arm();
        let frame = exchange("v1 status\n", &shared);
        assert!(frame.contains("rss_kib=0"));
        assert!(frame.contains("cpu_jiffies=0"));
        cleanup_paths(&audit_path, &rules_path);
    }

    #[test]
    fn network_mutate_success_path() {
        let _suite = suite_lock();
        let (shared, audit_path, rules_path) = test_shared("nft-ok");
        let _ok = crate::nft::ForceNftOk::arm();
        let (a, _b) = StdUnixStream::pair().unwrap();
        assert_eq!(
            dispatch(Request::NetworkInstall, &shared, &a),
            Response::Pong
        );
        assert_eq!(
            dispatch(Request::NetworkRemove, &shared, &a),
            Response::Pong
        );
        cleanup_paths(&audit_path, &rules_path);
    }

    #[test]
    fn lock_poisoned_responses() {
        let _suite = suite_lock();
        let mut paths = Vec::new();
        let (shared, audit_path, rules_path) = test_shared("attached");
        paths.push((audit_path, rules_path));
        poison(&shared, PoisonTarget::ProcessCache);
        assert_eq!(
            error_code(&exchange("v1 process-list\n", &shared)),
            "lock_poisoned"
        );
        let (shared, audit_path, rules_path) = test_shared("recent");
        paths.push((audit_path, rules_path));
        poison(&shared, PoisonTarget::Recent);
        assert_eq!(
            error_code(&exchange("v1 process-list\n", &shared)),
            "lock_poisoned"
        );
        let (shared, audit_path, rules_path) = test_shared("rules-lock");
        paths.push((audit_path, rules_path));
        poison(&shared, PoisonTarget::Rules);
        assert_eq!(
            error_code(&exchange("v1 process-list\n", &shared)),
            "lock_poisoned"
        );
        assert_eq!(
            error_code(&exchange("v1 rule-list\n", &shared)),
            "lock_poisoned"
        );
        assert_eq!(
            error_code(&exchange("v1 rule-delete 1\n", &shared)),
            "lock_poisoned"
        );
        assert_eq!(
            error_code(&exchange("v1 rule-add 1 /bin/x allow 1\n", &shared)),
            "lock_poisoned"
        );
        let (shared, audit_path, rules_path) = test_shared("prompts-lock");
        paths.push((audit_path, rules_path));
        poison(&shared, PoisonTarget::Prompts);
        assert_eq!(
            error_code(&exchange("v1 prompt-list\n", &shared)),
            "lock_poisoned"
        );
        assert_eq!(
            error_code(&exchange("v1 prompt-answer 1 allow once\n", &shared)),
            "lock_poisoned"
        );
        let (shared, audit_path, rules_path) = test_shared("prompt-rules-lock");
        paths.push((audit_path, rules_path));
        let session_id = enqueue_prompt(&shared, "/usr/bin/wget", 80);
        poison(&shared, PoisonTarget::Rules);
        assert_eq!(
            error_code(&exchange(
                &format!("v1 prompt-answer {session_id} allow session\n"),
                &shared
            )),
            "lock_poisoned"
        );
        let (shared, audit_path, rules_path) = test_shared("dns-lock");
        paths.push((audit_path, rules_path));
        poison(&shared, PoisonTarget::Dns);
        assert_eq!(
            error_code(&exchange("v1 dns-list\n", &shared)),
            "lock_poisoned"
        );
        assert_eq!(
            error_code(&exchange("v1 dns-note x.test 1.0.0.1\n", &shared)),
            "lock_poisoned"
        );
        let (shared, audit_path, rules_path) = test_shared("audit-lock");
        paths.push((audit_path, rules_path));
        poison(&shared, PoisonTarget::Audit);
        assert_eq!(
            error_code(&exchange("v1 audit-tail 1\n", &shared)),
            "lock_poisoned"
        );
        let (mut client, server) = StdUnixStream::pair().expect("pair");
        client
            .write_all(b"v1 audit-subscribe ui since=0\n")
            .expect("write");
        handle(server, &shared).expect("handle");
        let mut response = String::new();
        BufReader::new(&mut client)
            .read_line(&mut response)
            .expect("read");
        assert_eq!(error_code(&response), "lock_poisoned");
        for (audit_path, rules_path) in paths {
            cleanup_paths(&audit_path, &rules_path);
        }
    }

    #[test]
    fn persist_answered_rule_accepts_once_scope() {
        let _suite = suite_lock();
        let (shared, audit_path, rules_path) = test_shared("persist-once");
        let answered = crate::prompts::Answered {
            id: 1,
            key: PromptKey {
                executable: "/usr/bin/curl".into(),
                ipv4: u32::from_ne_bytes([203, 0, 113, 11]),
                port: 443,
            },
            verdict: Verdict::Allow,
            scope: RuleScope::Once,
            duplicate: false,
        };
        assert!(persist_answered_rule(&shared, &answered).is_ok());
        cleanup_paths(&audit_path, &rules_path);
    }

    #[test]
    fn prompt_answer_permanent_persistence_failure() {
        let _suite = suite_lock();
        let (audit_path, _) = temp_paths("prompt-persist");
        let _ = fs::remove_file(&audit_path);
        let blocker = audit_path.with_extension("blocker");
        fs::write(&blocker, "x").expect("blocker");
        let shared = Arc::new(
            Shared::new(
                RuleSet::default(),
                RulesStore::new(blocker.join("rules.toml")),
                "attached",
                8,
                Duration::from_secs(60),
                8,
                audit_path.clone(),
            )
            .expect("shared"),
        );
        let id = enqueue_prompt(&shared, "/usr/bin/curl", 443);
        assert_eq!(
            error_code(&exchange(
                &format!("v1 prompt-answer {id} allow permanent\n"),
                &shared
            )),
            "persistence_failed"
        );
        let _ = fs::remove_file(audit_path);
        let _ = fs::remove_file(blocker);
    }
}
