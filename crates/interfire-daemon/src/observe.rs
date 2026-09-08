//! Ring-buffer consumption and policy decisions.
#![forbid(unsafe_code)]

use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

#[cfg(test)]
use interfire_ebpf::{EVENT_MAP, LoadError};
use interfire_ebpf::{Observer, TcpConnectEvent};
use tracing::{debug, warn};

use crate::policy;
use crate::shared::Shared;

#[cfg(test)]
static FORCE_RING_UNAVAILABLE: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static FORCE_RING_BUF_ERROR: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static FORCE_SYNTHETIC_RING: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static SYNTHETIC_RING_ITEMS: std::sync::Mutex<Option<Vec<Vec<u8>>>> = std::sync::Mutex::new(None);
#[cfg(test)]
static OBSERVE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Feed synthetic ring items through [`run`] (unit tests only).
#[cfg(test)]
pub struct ForceSyntheticRing {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl ForceSyntheticRing {
    #[must_use]
    pub fn arm(items: Vec<Vec<u8>>) -> Self {
        let guard = OBSERVE_TEST_LOCK.lock().expect("observe test lock");
        *SYNTHETIC_RING_ITEMS.lock().expect("synthetic ring items") = Some(items);
        FORCE_SYNTHETIC_RING.store(true, Ordering::Relaxed);
        Self { _guard: guard }
    }
}

#[cfg(test)]
impl Drop for ForceSyntheticRing {
    fn drop(&mut self) {
        FORCE_SYNTHETIC_RING.store(false, Ordering::Relaxed);
        *SYNTHETIC_RING_ITEMS.lock().expect("synthetic ring items") = None;
    }
}

/// Force [`run`] to exit when the ring buffer is unavailable (unit tests only).
#[cfg(test)]
pub struct ForceRingUnavailable {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl ForceRingUnavailable {
    #[must_use]
    pub fn arm() -> Self {
        let guard = OBSERVE_TEST_LOCK.lock().expect("observe test lock");
        FORCE_RING_UNAVAILABLE.store(true, Ordering::Relaxed);
        Self { _guard: guard }
    }
}

#[cfg(test)]
impl Drop for ForceRingUnavailable {
    fn drop(&mut self) {
        FORCE_RING_UNAVAILABLE.store(false, Ordering::Relaxed);
    }
}

/// Force [`run`] to treat ring-buffer open as failed (unit tests only).
#[cfg(test)]
pub struct ForceRingBufError {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl ForceRingBufError {
    #[must_use]
    pub fn arm() -> Self {
        let guard = OBSERVE_TEST_LOCK.lock().expect("observe test lock");
        FORCE_RING_BUF_ERROR.store(true, Ordering::Relaxed);
        Self { _guard: guard }
    }
}

#[cfg(test)]
impl Drop for ForceRingBufError {
    fn drop(&mut self) {
        FORCE_RING_BUF_ERROR.store(false, Ordering::Relaxed);
    }
}

/// Poll the observer ring buffer until the process exits.
pub fn run(mut observer: Observer, shared: &Arc<Shared>) {
    if forced_ring_exit() {
        return;
    }
    #[cfg(test)]
    if FORCE_SYNTHETIC_RING.load(Ordering::Relaxed) {
        run_synthetic_ring(shared);
        return;
    }
    let mut ring = match observer.ring_buf() {
        Ok(ring) => ring,
        Err(error) => {
            warn!(%error, "ring buffer unavailable; observation thread exiting");
            return;
        }
    };
    observation_idle_loop(
        shared,
        || -> Option<Vec<u8>> {
            ring.next().map(|item| {
                let bytes: &[u8] = &item;
                bytes.to_vec()
            })
        },
        None,
    );
}

#[cfg(not(test))]
const fn forced_ring_exit() -> bool {
    false
}

#[cfg(test)]
fn forced_ring_exit() -> bool {
    if FORCE_RING_UNAVAILABLE.load(Ordering::Relaxed) {
        warn!(
            error = "forced ring buffer unavailable",
            "ring buffer unavailable; observation thread exiting"
        );
        return true;
    }
    if FORCE_RING_BUF_ERROR.load(Ordering::Relaxed) {
        let error = LoadError::MissingSymbol(EVENT_MAP);
        warn!(%error, "ring buffer unavailable; observation thread exiting");
        return true;
    }
    false
}

#[cfg(test)]
fn run_synthetic_ring(shared: &Shared) {
    let mut items = SYNTHETIC_RING_ITEMS
        .lock()
        .expect("synthetic ring items")
        .take()
        .unwrap_or_default()
        .into_iter();
    observation_idle_loop(shared, || items.next(), Some(0));
}

fn observation_idle_loop(
    shared: &Shared,
    mut poll: impl FnMut() -> Option<Vec<u8>>,
    max_idle_rounds: Option<usize>,
) {
    let mut idle_rounds = 0;
    loop {
        if poll_observation_batch(shared, &mut poll) {
            idle_rounds = 0;
        } else if max_idle_rounds.is_some_and(|limit| idle_rounds >= limit) {
            return;
        } else {
            idle_rounds += 1;
            thread::sleep(Duration::from_millis(1));
        }
    }
}

fn poll_observation_batch(shared: &Shared, poll: &mut impl FnMut() -> Option<Vec<u8>>) -> bool {
    let mut progressed = false;
    for bytes in std::iter::from_fn(poll) {
        progressed = true;
        process_ring_item(&bytes, shared);
    }
    progressed
}

fn process_ring_item(bytes: &[u8], shared: &Shared) {
    let Some(event) = TcpConnectEvent::try_from_bytes(bytes) else {
        warn!(len = bytes.len(), "dropping malformed ringbuf record");
        return;
    };
    handle_event(event, shared);
}

fn handle_event(event: TcpConnectEvent, shared: &Shared) {
    let Ok(rules) = shared.rules.lock() else {
        return;
    };
    let Ok(mut cache) = shared.process_cache.lock() else {
        return;
    };
    let Ok(mut prompts) = shared.prompts.lock() else {
        return;
    };
    let Ok(mut dns) = shared.dns.lock() else {
        return;
    };
    let Ok(mut recent) = shared.recent.lock() else {
        return;
    };
    let decision = policy::decide(
        event,
        &rules,
        &mut cache,
        &mut prompts,
        &mut dns,
        &mut recent,
    );
    drop(recent);
    drop(dns);
    drop(prompts);
    drop(cache);
    drop(rules);
    if let Ok(mut pending) = shared.pending.lock() {
        pending.insert(decision.key, decision.packet_verdict);
        debug!(
            attributed = decision.attributed,
            port = decision.key.port,
            prompt_id = ?decision.prompt_id,
            "pending verdict stored"
        );
    }
    if let Ok(mut audit) = shared.audit.lock() {
        let outcome = if decision.packet_verdict == nfq::Verdict::Accept {
            "allow"
        } else {
            "deny"
        };
        audit.append(format!(
            "connect port={} attributed={} outcome={outcome}",
            decision.key.port, decision.attributed
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::time::Duration;

    use interfire_ebpf::TcpConnectEvent;
    use interfire_rules::{Direction, Protocol};
    use interfire_rules::{Rule, RuleSet, RulesStore, Scope, Verdict as RuleVerdict};
    use nfq::Verdict;

    use crate::pending::DestKey;
    use crate::process;
    use crate::shared::Shared;

    use super::*;

    fn temp_audit(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("interfire-observe-{}-{name}", std::process::id()))
    }

    fn test_shared() -> (Arc<Shared>, std::path::PathBuf) {
        let audit_path = temp_audit("audit.log");
        let rules_path = audit_path.with_extension("rules.toml");
        let _ = fs::remove_file(&audit_path);
        let _ = fs::remove_file(&rules_path);
        let shared = Arc::new(
            Shared::new(
                RuleSet::default(),
                RulesStore::new(&rules_path),
                "attached",
                8,
                Duration::from_secs(60),
                8,
                audit_path.clone(),
            )
            .expect("shared state"),
        );
        (shared, audit_path)
    }

    fn event_bytes(pid: u32, port: u16) -> Vec<u8> {
        let event = TcpConnectEvent {
            pid,
            process_start_ticks: 0,
            destination_ipv4: u32::from_ne_bytes([127, 0, 0, 1]),
            destination_port: port,
            reserved: 0,
        };
        let mut bytes = vec![0_u8; 24];
        bytes[0..4].copy_from_slice(&event.pid.to_ne_bytes());
        bytes[8..16].copy_from_slice(&event.process_start_ticks.to_ne_bytes());
        bytes[16..20].copy_from_slice(&event.destination_ipv4.to_ne_bytes());
        bytes[20..22].copy_from_slice(&event.destination_port.to_ne_bytes());
        bytes
    }

    enum PoisonTarget {
        Rules,
        ProcessCache,
        Prompts,
        Dns,
        Recent,
        Pending,
        Audit,
    }

    fn poison(shared: &Arc<Shared>, target: PoisonTarget) {
        let shared = Arc::clone(shared);
        let handle = std::thread::spawn(move || match target {
            PoisonTarget::Rules => {
                let _guard = shared.rules.lock().expect("rules lock");
                panic!("poison mutex");
            }
            PoisonTarget::ProcessCache => {
                let _guard = shared.process_cache.lock().expect("process cache lock");
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
            PoisonTarget::Recent => {
                let _guard = shared.recent.lock().expect("recent lock");
                panic!("poison mutex");
            }
            PoisonTarget::Pending => {
                let _guard = shared.pending.lock().expect("pending lock");
                panic!("poison mutex");
            }
            PoisonTarget::Audit => {
                let _guard = shared.audit.lock().expect("audit lock");
                panic!("poison mutex");
            }
        });
        let _ = handle.join();
    }

    #[test]
    fn handle_event_stores_pending_and_allow_audit() {
        let (shared, audit_path) = test_shared();
        let self_pid = std::process::id();
        let identity = process::resolve(self_pid, 0).expect("self /proc");
        {
            let mut rules = shared.rules.lock().expect("rules");
            rules
                .insert(Rule {
                    id: 1,
                    executable: identity.executable.display().to_string(),
                    protocol: Some(Protocol::Tcp),
                    direction: Some(Direction::Outbound),
                    address: None,
                    hostname: None,
                    port: Some(9443),
                    verdict: RuleVerdict::Allow,
                    scope: Scope::Permanent,
                })
                .expect("rule");
        }
        let event = TcpConnectEvent {
            pid: self_pid,
            process_start_ticks: 0,
            destination_ipv4: u32::from_ne_bytes([127, 0, 0, 1]),
            destination_port: 9443,
            reserved: 0,
        };
        handle_event(event, &shared);
        let key = DestKey {
            ipv4: u32::from_ne_bytes([127, 0, 0, 1]),
            port: 9443,
        };
        assert_eq!(
            shared.pending.lock().expect("pending").take(key),
            Some(Verdict::Accept)
        );
        let audit = shared.audit.lock().expect("audit");
        assert!(
            audit
                .tail(4)
                .iter()
                .any(|record| record.message.contains("outcome=allow"))
        );
        drop(audit);
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn handle_event_records_deny_for_unattributed() {
        let (shared, audit_path) = test_shared();
        let event = TcpConnectEvent {
            pid: u32::MAX,
            process_start_ticks: 0,
            destination_ipv4: u32::from_ne_bytes([127, 0, 0, 1]),
            destination_port: 8080,
            reserved: 0,
        };
        handle_event(event, &shared);
        let key = DestKey {
            ipv4: u32::from_ne_bytes([127, 0, 0, 1]),
            port: 8080,
        };
        assert_eq!(
            shared.pending.lock().expect("pending").take(key),
            Some(Verdict::Drop)
        );
        let audit = shared.audit.lock().expect("audit");
        assert!(
            audit
                .tail(4)
                .iter()
                .any(|record| record.message.contains("outcome=deny"))
        );
        drop(audit);
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn handle_event_survives_poisoned_locks() {
        let event = TcpConnectEvent {
            pid: u32::MAX,
            process_start_ticks: 0,
            destination_ipv4: u32::from_ne_bytes([127, 0, 0, 1]),
            destination_port: 1,
            reserved: 0,
        };
        for target in [
            PoisonTarget::Rules,
            PoisonTarget::ProcessCache,
            PoisonTarget::Prompts,
            PoisonTarget::Dns,
            PoisonTarget::Recent,
        ] {
            let (shared, audit_path) = test_shared();
            poison(&shared, target);
            handle_event(event, &shared);
            let _ = fs::remove_file(audit_path);
        }
        let (shared, audit_path) = test_shared();
        handle_event(event, &shared);
        poison(&shared, PoisonTarget::Pending);
        handle_event(event, &shared);
        poison(&shared, PoisonTarget::Audit);
        handle_event(event, &shared);
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn poll_observation_batch_drops_malformed_and_handles_valid() {
        let (shared, audit_path) = test_shared();
        let valid = event_bytes(u32::MAX, 4444);
        let mut items = vec![vec![0_u8; 8], valid].into_iter();
        assert!(poll_observation_batch(&shared, &mut || items.next()));
        let key = DestKey {
            ipv4: u32::from_ne_bytes([127, 0, 0, 1]),
            port: 4444,
        };
        assert_eq!(
            shared.pending.lock().expect("pending").take(key),
            Some(Verdict::Drop)
        );
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn poll_observation_batch_empty_returns_false() {
        let (shared, audit_path) = test_shared();
        let mut items = std::iter::empty::<Vec<u8>>();
        assert!(!poll_observation_batch(&shared, &mut || items.next()));
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn observation_idle_loop_sleeps_when_ring_is_empty() {
        let (shared, audit_path) = test_shared();
        let mut items = std::iter::empty::<Vec<u8>>();
        observation_idle_loop(&shared, || items.next(), Some(1));
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn run_processes_synthetic_ring_without_ebpf() {
        let (shared, audit_path) = test_shared();
        let valid = event_bytes(u32::MAX, 5555);
        let _synthetic = ForceSyntheticRing::arm(vec![valid]);
        if let Ok(observer) = interfire_ebpf::Observer::load_embedded_and_attach() {
            run(observer, &shared);
        } else {
            run_synthetic_ring(&shared);
        }
        let key = DestKey {
            ipv4: u32::from_ne_bytes([127, 0, 0, 1]),
            port: 5555,
        };
        assert_eq!(
            shared.pending.lock().expect("pending").take(key),
            Some(Verdict::Drop)
        );
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn ring_startup_force_flags_exit_without_observer() {
        {
            let _unavailable = ForceRingUnavailable::arm();
            assert!(forced_ring_exit());
        }
        {
            let _buf_error = ForceRingBufError::arm();
            assert!(forced_ring_exit());
        }
    }
}
