//! Map observation + rules into NFQUEUE verdicts (fail closed).
#![forbid(unsafe_code)]

use std::net::{IpAddr, Ipv4Addr};

use interfire_ebpf::TcpConnectEvent;
use interfire_rules::{Connection, Direction, Protocol, RuleSet, Verdict as RuleVerdict};
use nfq::Verdict;
use tracing::{debug, warn};

use crate::dns::DnsCache;
use crate::pending::DestKey;
use crate::process::{self, AttributionError, ProcessCache, ProcessIdentity};
use crate::prompts::{EnqueueOutcome, PromptKey, PromptQueue};
use crate::recent::{RecentConnects, RecentDest};

/// Outcome of attributing and deciding a connect event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decision {
    pub key: DestKey,
    pub packet_verdict: Verdict,
    pub attributed: bool,
    pub prompt_id: Option<u64>,
}

/// Attribute via `/proc`, match rules / once-tokens / prompts, and map to a packet verdict.
///
/// Unattributed events, prompts, and full prompt queues become [`Verdict::Drop`].
/// Fresh DNS names may annotate the connection; stale or missing names stay unset so
/// the destination IP remains authoritative.
#[must_use]
pub fn decide(
    event: TcpConnectEvent,
    rules: &RuleSet,
    cache: &mut ProcessCache,
    prompts: &mut PromptQueue,
    dns: &mut DnsCache,
    recent: &mut RecentConnects,
) -> Decision {
    let key = DestKey {
        ipv4: event.destination_ipv4,
        port: event.destination_port,
    };
    match attribute(event, cache) {
        Ok(identity) => decide_attributed(AttributedDecide {
            identity: &identity,
            event,
            rules,
            prompts,
            dns,
            recent,
            key,
        }),
        Err(error) => {
            warn!(pid = event.pid, %error, "unattributed connect; denying");
            Decision {
                key,
                packet_verdict: Verdict::Drop,
                attributed: false,
                prompt_id: None,
            }
        }
    }
}

struct AttributedDecide<'a> {
    identity: &'a ProcessIdentity,
    event: TcpConnectEvent,
    rules: &'a RuleSet,
    prompts: &'a mut PromptQueue,
    dns: &'a mut DnsCache,
    recent: &'a mut RecentConnects,
    key: DestKey,
}

fn decide_attributed(ctx: AttributedDecide<'_>) -> Decision {
    let AttributedDecide {
        identity,
        event,
        rules,
        prompts,
        dns,
        recent,
        key,
    } = ctx;
    let prompt_key = PromptKey {
        executable: identity.executable.display().to_string(),
        ipv4: event.destination_ipv4,
        port: event.destination_port,
    };
    if prompts.take_once_allow(&prompt_key) {
        debug!(port = key.port, "once-allow token consumed");
        record_recent(recent, identity, event, "allow");
        return Decision {
            key,
            packet_verdict: Verdict::Accept,
            attributed: true,
            prompt_id: None,
        };
    }
    if prompts.consume_once_deny(&prompt_key) {
        debug!(port = key.port, "once-deny token consumed");
        record_recent(recent, identity, event, "deny");
        return Decision {
            key,
            packet_verdict: Verdict::Drop,
            attributed: true,
            prompt_id: None,
        };
    }
    let connection = connection_from(identity, event, dns);
    let rule_verdict = rules.verdict_for(&connection);
    let (packet_verdict, prompt_id, label) = match rule_verdict {
        RuleVerdict::Allow => (Verdict::Accept, None, "allow"),
        RuleVerdict::Deny => (Verdict::Drop, None, "deny"),
        RuleVerdict::Prompt => match prompts.enqueue(prompt_key) {
            EnqueueOutcome::Created(id) | EnqueueOutcome::Deduped(id) => {
                (Verdict::Drop, Some(id), "prompt")
            }
            EnqueueOutcome::Full => (Verdict::Drop, None, "prompt"),
        },
    };
    record_recent(recent, identity, event, label);
    let executable = identity.executable.display().to_string();
    debug!(
        pid = identity.pid,
        executable = %executable,
        port = event.destination_port,
        hostname = ?connection.hostname,
        ?rule_verdict,
        ?packet_verdict,
        ?prompt_id,
        "attributed connect decision"
    );
    Decision {
        key,
        packet_verdict,
        attributed: true,
        prompt_id,
    }
}

fn record_recent(
    recent: &mut RecentConnects,
    identity: &ProcessIdentity,
    event: TcpConnectEvent,
    verdict: &str,
) {
    recent.record(
        identity.pid,
        identity.start_ticks,
        RecentDest {
            ipv4: Ipv4Addr::from(event.destination_octets()),
            port: event.destination_port,
            verdict: verdict.to_owned(),
        },
    );
}

fn attribute(
    event: TcpConnectEvent,
    cache: &mut ProcessCache,
) -> Result<ProcessIdentity, AttributionError> {
    if let Some(cached) = cache
        .get(event.pid, event.process_start_ticks)
        .filter(|_| event.process_start_ticks != 0)
        .cloned()
    {
        return Ok(cached);
    }
    let identity = process::resolve(event.pid, event.process_start_ticks)?;
    cache.insert(identity.clone());
    Ok(identity)
}

fn connection_from(
    identity: &ProcessIdentity,
    event: TcpConnectEvent,
    dns: &mut DnsCache,
) -> Connection {
    Connection {
        executable: identity.executable.display().to_string(),
        protocol: Protocol::Tcp,
        direction: Direction::Outbound,
        address: IpAddr::V4(Ipv4Addr::from(event.destination_octets())),
        hostname: dns.hostname_for(event.destination_ipv4),
        port: event.destination_port,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interfire_proto::RuleScope;
    use interfire_rules::{Rule, Scope, Verdict as RulesVerdict};
    use std::time::Duration;

    fn event(pid: u32, port: u16) -> TcpConnectEvent {
        TcpConnectEvent {
            pid,
            process_start_ticks: 0,
            destination_ipv4: u32::from_ne_bytes([127, 0, 0, 1]),
            destination_port: port,
            reserved: 0,
        }
    }

    #[test]
    fn unattributed_never_allows() {
        let rules = RuleSet::default();
        let mut cache = ProcessCache::new(8);
        let mut prompts = PromptQueue::new(8, Duration::from_secs(60));
        let mut dns = DnsCache::new(8, Duration::from_secs(60));
        let mut recent = RecentConnects::new(8, 8);
        let decision = decide(
            event(u32::MAX, 9),
            &rules,
            &mut cache,
            &mut prompts,
            &mut dns,
            &mut recent,
        );
        assert!(!decision.attributed);
        assert_eq!(decision.packet_verdict, Verdict::Drop);
    }

    #[test]
    fn allow_rule_accepts_attributed_self() {
        let self_pid = std::process::id();
        let identity = process::resolve(self_pid, 0).expect("self /proc");
        let mut rules = RuleSet::default();
        rules
            .insert(Rule {
                id: 1,
                executable: identity.executable.display().to_string(),
                protocol: Some(Protocol::Tcp),
                direction: Some(Direction::Outbound),
                address: None,
                hostname: None,
                port: Some(8443),
                verdict: RuleVerdict::Allow,
                scope: Scope::Permanent,
            })
            .unwrap();
        let mut cache = ProcessCache::new(8);
        let mut prompts = PromptQueue::new(8, Duration::from_secs(60));
        let mut dns = DnsCache::new(8, Duration::from_secs(60));
        let mut recent = RecentConnects::new(8, 8);
        let decision = decide(
            event(self_pid, 8443),
            &rules,
            &mut cache,
            &mut prompts,
            &mut dns,
            &mut recent,
        );
        assert!(decision.attributed);
        assert_eq!(decision.packet_verdict, Verdict::Accept);
    }

    #[test]
    fn default_prompt_enqueues_and_drops() {
        let self_pid = std::process::id();
        let rules = RuleSet::default();
        let mut cache = ProcessCache::new(8);
        let mut prompts = PromptQueue::new(8, Duration::from_secs(60));
        let mut dns = DnsCache::new(8, Duration::from_secs(60));
        let mut recent = RecentConnects::new(8, 8);
        let decision = decide(
            event(self_pid, 9_001),
            &rules,
            &mut cache,
            &mut prompts,
            &mut dns,
            &mut recent,
        );
        assert!(decision.attributed);
        assert_eq!(decision.packet_verdict, Verdict::Drop);
        assert!(decision.prompt_id.is_some());
        assert_eq!(prompts.list_pending().len(), 1);
    }

    #[test]
    fn full_prompt_queue_drops_without_id() {
        let self_pid = std::process::id();
        let rules = RuleSet::default();
        let mut cache = ProcessCache::new(8);
        let mut prompts = PromptQueue::new(1, Duration::from_secs(60));
        let mut dns = DnsCache::new(8, Duration::from_secs(60));
        let mut recent = RecentConnects::new(8, 8);
        let first = decide(
            event(self_pid, 9_002),
            &rules,
            &mut cache,
            &mut prompts,
            &mut dns,
            &mut recent,
        );
        assert!(first.prompt_id.is_some());
        let second = decide(
            event(self_pid, 9_003),
            &rules,
            &mut cache,
            &mut prompts,
            &mut dns,
            &mut recent,
        );
        assert_eq!(second.packet_verdict, Verdict::Drop);
        assert!(second.prompt_id.is_none());
    }

    #[test]
    fn stale_hostname_does_not_override_ip_allow() {
        let self_pid = std::process::id();
        let identity = process::resolve(self_pid, 0).expect("self /proc");
        let ip = u32::from_ne_bytes([127, 0, 0, 1]);
        let mut rules = RuleSet::default();
        rules
            .insert(Rule {
                id: 1,
                executable: identity.executable.display().to_string(),
                protocol: Some(Protocol::Tcp),
                direction: Some(Direction::Outbound),
                address: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                hostname: None,
                port: Some(9_100),
                verdict: RuleVerdict::Allow,
                scope: Scope::Permanent,
            })
            .unwrap();
        rules
            .insert(Rule {
                id: 2,
                executable: identity.executable.display().to_string(),
                protocol: Some(Protocol::Tcp),
                direction: Some(Direction::Outbound),
                address: None,
                hostname: Some("evil.test".into()),
                port: Some(9_100),
                verdict: RuleVerdict::Deny,
                scope: Scope::Permanent,
            })
            .unwrap();

        let mut cache = ProcessCache::new(8);
        let mut prompts = PromptQueue::new(8, Duration::from_secs(60));
        let mut dns = DnsCache::new(8, Duration::from_millis(1));
        let mut recent = RecentConnects::new(8, 8);
        dns.observe("evil.test", ip, Some(Duration::from_millis(1)));
        std::thread::sleep(Duration::from_millis(5));
        let decision = decide(
            event(self_pid, 9_100),
            &rules,
            &mut cache,
            &mut prompts,
            &mut dns,
            &mut recent,
        );
        assert_eq!(decision.packet_verdict, Verdict::Accept);
    }

    #[test]
    fn attributed_connect_emits_debug_record() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let self_pid = std::process::id();
        let rules = RuleSet::default();
        let mut cache = ProcessCache::new(8);
        let mut prompts = PromptQueue::new(8, Duration::from_secs(60));
        let mut dns = DnsCache::new(8, Duration::from_secs(60));
        let mut recent = RecentConnects::new(8, 8);
        let decision = decide(
            event(self_pid, 9_400),
            &rules,
            &mut cache,
            &mut prompts,
            &mut dns,
            &mut recent,
        );
        assert!(decision.attributed);
    }

    #[test]
    fn deny_rule_drops_attributed_connect() {
        let self_pid = std::process::id();
        let identity = process::resolve(self_pid, 0).expect("self /proc");
        let mut rules = RuleSet::default();
        rules
            .insert(Rule {
                id: 1,
                executable: identity.executable.display().to_string(),
                protocol: Some(Protocol::Tcp),
                direction: Some(Direction::Outbound),
                address: None,
                hostname: None,
                port: Some(9_200),
                verdict: RuleVerdict::Deny,
                scope: Scope::Permanent,
            })
            .unwrap();
        let mut cache = ProcessCache::new(8);
        let mut prompts = PromptQueue::new(8, Duration::from_secs(60));
        let mut dns = DnsCache::new(8, Duration::from_secs(60));
        let mut recent = RecentConnects::new(8, 8);
        let decision = decide(
            event(self_pid, 9_200),
            &rules,
            &mut cache,
            &mut prompts,
            &mut dns,
            &mut recent,
        );
        assert!(decision.attributed);
        assert_eq!(decision.packet_verdict, Verdict::Drop);
    }

    #[test]
    fn once_allow_accepts_without_re_prompt() {
        let self_pid = std::process::id();
        let rules = RuleSet::default();
        let mut cache = ProcessCache::new(8);
        let mut prompts = PromptQueue::new(8, Duration::from_secs(60));
        let mut dns = DnsCache::new(8, Duration::from_secs(60));
        let mut recent = RecentConnects::new(8, 8);
        let first = decide(
            event(self_pid, 9_300),
            &rules,
            &mut cache,
            &mut prompts,
            &mut dns,
            &mut recent,
        );
        let id = first.prompt_id.expect("prompt");
        prompts
            .answer(id, RulesVerdict::Allow, RuleScope::Once)
            .unwrap();
        let second = decide(
            event(self_pid, 9_300),
            &rules,
            &mut cache,
            &mut prompts,
            &mut dns,
            &mut recent,
        );
        assert_eq!(second.packet_verdict, Verdict::Accept);
        assert!(second.prompt_id.is_none());
    }

    #[test]
    fn once_deny_drops_without_re_prompt() {
        let self_pid = std::process::id();
        let rules = RuleSet::default();
        let mut cache = ProcessCache::new(8);
        let mut prompts = PromptQueue::new(8, Duration::from_secs(60));
        let mut dns = DnsCache::new(8, Duration::from_secs(60));
        let mut recent = RecentConnects::new(8, 8);
        let first = decide(
            event(self_pid, 9_301),
            &rules,
            &mut cache,
            &mut prompts,
            &mut dns,
            &mut recent,
        );
        let id = first.prompt_id.expect("prompt");
        prompts
            .answer(id, RulesVerdict::Deny, RuleScope::Once)
            .unwrap();
        let second = decide(
            event(self_pid, 9_301),
            &rules,
            &mut cache,
            &mut prompts,
            &mut dns,
            &mut recent,
        );
        assert_eq!(second.packet_verdict, Verdict::Drop);
        assert!(second.prompt_id.is_none());
    }

    #[test]
    fn cached_identity_avoids_re_resolve() {
        let self_pid = std::process::id();
        let identity = process::resolve(self_pid, 0).expect("self /proc");
        let mut cache = ProcessCache::new(8);
        cache.insert(identity.clone());
        let rules = RuleSet::default();
        let mut prompts = PromptQueue::new(8, Duration::from_secs(60));
        let mut dns = DnsCache::new(8, Duration::from_secs(60));
        let mut recent = RecentConnects::new(8, 8);
        let mut connect = event(self_pid, 9_400);
        connect.process_start_ticks = identity.start_ticks;
        let decision = decide(
            connect,
            &rules,
            &mut cache,
            &mut prompts,
            &mut dns,
            &mut recent,
        );
        assert!(decision.attributed);
    }
}
