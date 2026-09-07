//! Map observation + rules into NFQUEUE verdicts (fail closed).
#![forbid(unsafe_code)]

use std::net::{IpAddr, Ipv4Addr};

use interfire_ebpf::TcpConnectEvent;
use interfire_rules::{Connection, Direction, Protocol, RuleSet, Verdict as RuleVerdict};
use nfq::Verdict;
use tracing::{debug, warn};

use crate::pending::DestKey;
use crate::process::{self, AttributionError, ProcessCache, ProcessIdentity};

/// Outcome of attributing and deciding a connect event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decision {
    pub key: DestKey,
    pub packet_verdict: Verdict,
    pub attributed: bool,
}

/// Attribute via `/proc`, match rules, and map to a packet verdict.
///
/// Unattributed events and [`RuleVerdict::Prompt`] become [`Verdict::Drop`].
#[must_use]
pub fn decide(event: TcpConnectEvent, rules: &RuleSet, cache: &mut ProcessCache) -> Decision {
    let key = DestKey {
        ipv4: event.destination_ipv4,
        port: event.destination_port,
    };
    match attribute(event, cache) {
        Ok(identity) => {
            let connection = connection_from(&identity, event);
            let rule_verdict = rules.verdict_for(&connection);
            let packet_verdict = packet_verdict(rule_verdict);
            debug!(
                pid = identity.pid,
                executable = %identity.executable.display(),
                port = event.destination_port,
                ?rule_verdict,
                ?packet_verdict,
                "attributed connect decision"
            );
            Decision {
                key,
                packet_verdict,
                attributed: true,
            }
        }
        Err(error) => {
            warn!(pid = event.pid, %error, "unattributed connect; denying");
            Decision {
                key,
                packet_verdict: Verdict::Drop,
                attributed: false,
            }
        }
    }
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

fn connection_from(identity: &ProcessIdentity, event: TcpConnectEvent) -> Connection {
    Connection {
        executable: identity.executable.display().to_string(),
        protocol: Protocol::Tcp,
        direction: Direction::Outbound,
        address: IpAddr::V4(Ipv4Addr::from(event.destination_octets())),
        hostname: None,
        port: event.destination_port,
    }
}

const fn packet_verdict(rule: RuleVerdict) -> Verdict {
    match rule {
        RuleVerdict::Allow => Verdict::Accept,
        RuleVerdict::Deny | RuleVerdict::Prompt => Verdict::Drop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interfire_rules::{Rule, Scope};

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
    fn prompt_and_deny_drop_packets() {
        assert_eq!(packet_verdict(RuleVerdict::Prompt), Verdict::Drop);
        assert_eq!(packet_verdict(RuleVerdict::Deny), Verdict::Drop);
        assert_eq!(packet_verdict(RuleVerdict::Allow), Verdict::Accept);
    }

    #[test]
    fn unattributed_never_allows() {
        let rules = RuleSet::default();
        let mut cache = ProcessCache::new(8);
        let decision = decide(event(u32::MAX, 9), &rules, &mut cache);
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
        let decision = decide(event(self_pid, 8443), &rules, &mut cache);
        assert!(decision.attributed);
        assert_eq!(decision.packet_verdict, Verdict::Accept);
    }
}
