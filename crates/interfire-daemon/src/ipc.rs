//! Unix IPC request handling with peer-credential checks on mutate.
#![forbid(unsafe_code)]

use std::io::{self, BufRead, BufReader, Write};
use std::net::Ipv4Addr;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use interfire_proto::{IPC_VERSION, MAX_FRAME_BYTES, Request, Response, RuleScope};
use interfire_rules::{Direction, Protocol, Rule, Scope, Verdict};
use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
use nix::unistd::Uid;
use tracing::{debug, warn};

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
        Request::Status => Response::Status {
            enforcement: shared.enforcement(),
            observation: shared.observation(),
            ipc_version: IPC_VERSION,
        },
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
    }
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
    prompts
        .list_pending()
        .into_iter()
        .map(|prompt| {
            format!(
                "{}|{}|{}|{}",
                prompt.id,
                prompt.key.executable,
                prompt.key.ipv4_display(),
                prompt.key.port
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
    use super::*;
    use std::os::unix::net::UnixStream as StdUnixStream;

    #[test]
    fn peer_cred_allows_same_uid_pair() {
        let (a, _b) = StdUnixStream::pair().unwrap();
        assert!(peer_may_mutate(&a));
    }
}
