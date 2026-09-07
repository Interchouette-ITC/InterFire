//! Unix IPC request handling with peer-credential checks on mutate.
#![forbid(unsafe_code)]

use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;

use interfire_proto::{IPC_VERSION, MAX_FRAME_BYTES, Request, Response, RuleScope};
use interfire_rules::{Direction, Protocol, Rule, Scope, Verdict};
use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
use nix::unistd::Uid;
use tracing::{debug, warn};

use crate::prompts::{AnswerError, PromptQueue};
use crate::shared::Shared;

/// Handle one client connection.
///
/// # Errors
///
/// Returns I/O failures while reading or writing the frame.
pub fn respond(mut stream: UnixStream, shared: &Arc<Shared>) -> io::Result<()> {
    let mut frame = String::new();
    let bytes = BufReader::new(stream.try_clone()?).read_line(&mut frame)?;
    let response = if bytes > MAX_FRAME_BYTES {
        Response::Error("frame_too_large")
    } else {
        match Request::parse(&frame) {
            Ok(Request::Ping) => Response::Pong,
            Ok(Request::Status) => Response::Status {
                enforcement: shared.enforcement(),
                observation: shared.observation(),
                ipc_version: IPC_VERSION,
            },
            Ok(Request::RuleList) => rule_list(shared),
            Ok(Request::RuleDelete { id }) => mutate(shared, &stream, Mutate::Delete { id }),
            Ok(Request::RuleAdd {
                id,
                executable,
                verdict,
                port,
            }) => mutate(
                shared,
                &stream,
                Mutate::Add {
                    id,
                    executable,
                    verdict,
                    port,
                },
            ),
            Ok(Request::PromptList) => prompt_list(shared),
            Ok(Request::PromptAnswer { id, verdict, scope }) => {
                prompt_answer(shared, &stream, id, &verdict, &scope)
            }
            Err(_) => Response::Error("malformed_request"),
        }
    };
    stream.write_all(response.encode().as_bytes())
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
