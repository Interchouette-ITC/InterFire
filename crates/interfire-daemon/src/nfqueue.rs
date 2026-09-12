//! NFQUEUE bind and fail-closed verdict application.
#![forbid(unsafe_code)]

use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::thread;
#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
use nfq::Verdict;
#[cfg(not(test))]
use tracing::warn;
#[cfg(test)]
use tracing::{debug, info, warn};

#[cfg(test)]
use interfire_proto::NFQUEUE_NUM;

#[cfg(test)]
use crate::packet;
use crate::shared::Shared;

#[cfg(test)]
const LOOKUP_MAX_ATTEMPTS: u32 = 50;

#[cfg(test)]
static FORCE_NFQUEUE_FAIL: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static FORCE_NFQUEUE_SIMULATE_BOUND: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static NFQUEUE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(test)]
static NFQUEUE_ONE_SHOT_PAYLOAD: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);

#[cfg(test)]
struct FakeTransport {
    payloads: std::collections::VecDeque<Vec<u8>>,
    verdicts: Vec<Verdict>,
    recv_error: Option<std::io::Error>,
}

#[cfg(test)]
impl NfqueueTransport for FakeTransport {
    fn recv_payload(&mut self) -> std::io::Result<Vec<u8>> {
        if let Some(error) = self.recv_error.take() {
            return Err(error);
        }
        self.payloads
            .pop_front()
            .ok_or_else(|| std::io::Error::other("no payload"))
    }

    fn submit_verdict(&mut self, verdict: Verdict) -> std::io::Result<()> {
        self.verdicts.push(verdict);
        Ok(())
    }
}

/// Force [`run`] to fail before bind (unit tests only).
#[cfg(test)]
pub struct ForceNfqueueFail {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl ForceNfqueueFail {
    #[must_use]
    pub fn arm() -> Self {
        let guard = NFQUEUE_TEST_LOCK.lock().expect("nfqueue test lock");
        FORCE_NFQUEUE_FAIL.store(true, Ordering::Relaxed);
        Self { _guard: guard }
    }
}

#[cfg(test)]
impl Drop for ForceNfqueueFail {
    fn drop(&mut self) {
        FORCE_NFQUEUE_FAIL.store(false, Ordering::Relaxed);
    }
}

/// Simulate a bound queue and one payload verdict (unit tests only).
#[cfg(test)]
pub struct ForceNfqueueSimulateBound {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl ForceNfqueueSimulateBound {
    #[must_use]
    pub fn arm(payload: Vec<u8>) -> Self {
        let guard = NFQUEUE_TEST_LOCK.lock().expect("nfqueue test lock");
        *NFQUEUE_ONE_SHOT_PAYLOAD.lock().expect("one-shot payload") = Some(payload);
        FORCE_NFQUEUE_SIMULATE_BOUND.store(true, Ordering::Relaxed);
        Self { _guard: guard }
    }

    /// Simulate bind without a one-shot payload (marks nfqueue only).
    #[must_use]
    pub fn arm_without_payload() -> Self {
        let guard = NFQUEUE_TEST_LOCK.lock().expect("nfqueue test lock");
        *NFQUEUE_ONE_SHOT_PAYLOAD.lock().expect("one-shot payload") = None;
        FORCE_NFQUEUE_SIMULATE_BOUND.store(true, Ordering::Relaxed);
        Self { _guard: guard }
    }
}

#[cfg(test)]
impl Drop for ForceNfqueueSimulateBound {
    fn drop(&mut self) {
        FORCE_NFQUEUE_SIMULATE_BOUND.store(false, Ordering::Relaxed);
        *NFQUEUE_ONE_SHOT_PAYLOAD.lock().expect("one-shot payload") = None;
    }
}

#[cfg(test)]
trait NfqueueTransport {
    fn recv_payload(&mut self) -> std::io::Result<Vec<u8>>;
    fn submit_verdict(&mut self, verdict: Verdict) -> std::io::Result<()>;
}

#[cfg(test)]
fn mark_nfqueue_bound(shared: &Shared) {
    shared.set_enforcement("nfqueue");
    info!(queue = NFQUEUE_NUM, "NFQUEUE bound");
}

#[cfg(test)]
fn service_one_message(
    transport: &mut impl NfqueueTransport,
    shared: &Shared,
) -> std::io::Result<()> {
    let payload = transport.recv_payload()?;
    let verdict = lookup_verdict(&payload, shared);
    transport.submit_verdict(verdict)
}

/// Bind queue [`interfire_proto::NFQUEUE_NUM`] and apply pending / default-deny verdicts.
///
/// # Errors
///
/// Returns I/O errors when the queue cannot be opened or bound.
pub fn run(shared: &Shared) -> std::io::Result<()> {
    #[cfg(test)]
    if FORCE_NFQUEUE_FAIL.load(Ordering::Relaxed) {
        return Err(std::io::Error::other("forced nfqueue bind failure"));
    }
    #[cfg(test)]
    if FORCE_NFQUEUE_SIMULATE_BOUND.load(Ordering::Relaxed) {
        mark_nfqueue_bound(shared);
        let payload = NFQUEUE_ONE_SHOT_PAYLOAD
            .lock()
            .expect("one-shot payload")
            .take();
        if let Some(payload) = payload {
            let mut transport = FakeTransport {
                payloads: std::collections::VecDeque::from([payload]),
                verdicts: Vec::new(),
                recv_error: None,
            };
            service_one_message(&mut transport, shared)?;
        }
        return Ok(());
    }
    #[cfg(not(test))]
    {
        crate::nfqueue_live::bind_and_serve(shared)
    }
    #[cfg(test)]
    Err(std::io::Error::other(
        "live NFQUEUE bind is unavailable under unit tests",
    ))
}

#[cfg(test)]
fn lookup_verdict(payload: &[u8], shared: &Shared) -> Verdict {
    lookup_verdict_with_attempts(payload, shared, LOOKUP_MAX_ATTEMPTS)
}

#[cfg(test)]
fn lookup_verdict_with_attempts(payload: &[u8], shared: &Shared, max_attempts: u32) -> Verdict {
    let Some(key) = packet::tcp_destination(payload) else {
        warn!("NFQUEUE packet not parseable as IPv4 TCP; dropping");
        return Verdict::Drop;
    };
    // Observation may still be draining the ring buffer after `tcp_v4_connect`;
    // the packet is held in NFQUEUE, so a short wait stays fail-closed.
    for attempt in 0..max_attempts {
        let Ok(mut pending) = shared.pending.lock() else {
            return Verdict::Drop;
        };
        if let Some(verdict) = pending.take(key) {
            debug!(
                port = key.port,
                attempt,
                ?verdict,
                "NFQUEUE matched pending decision"
            );
            return verdict;
        }
        drop(pending);
        thread::sleep(Duration::from_millis(1));
    }
    warn!(
        port = key.port,
        "NFQUEUE without pending decision; dropping"
    );
    Verdict::Drop
}

/// Attempt to bind; on failure mark enforcement degraded and return.
pub fn run_or_degrade(shared: &Arc<Shared>) {
    if let Err(error) = run(shared) {
        shared.set_enforcement("degraded");
        warn!(%error, "NFQUEUE unavailable; enforcement=degraded");
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;
    use std::sync::Arc;
    use std::time::Duration;

    use interfire_rules::{RuleSet, RulesStore};
    use nfq::Verdict;

    use crate::pending::DestKey;
    use crate::shared::{Shared, SharedConfig};

    use super::*;

    fn temp_audit(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("interfire-nfqueue-{}-{name}", std::process::id()))
    }

    fn test_shared() -> (Arc<Shared>, std::path::PathBuf) {
        let audit_path = temp_audit("audit.log");
        let rules_path = audit_path.with_extension("rules.toml");
        let mode_path = audit_path.with_extension("mode");
        let _ = fs::remove_file(&audit_path);
        let _ = fs::remove_file(&rules_path);
        let _ = fs::remove_file(&mode_path);
        crate::enforcement_mode::store(
            &mode_path,
            crate::enforcement_mode::EnforcementMode::Active,
        )
        .expect("mode");
        let shared = Arc::new(
            Shared::new(SharedConfig {
                rules: RuleSet::default(),
                store: RulesStore::new(&rules_path),
                observation: "attached",
                pending_capacity: 8,
                pending_ttl: Duration::from_secs(60),
                process_capacity: 8,
                audit_path: audit_path.clone(),
                mode_path,
            })
            .expect("shared state"),
        );
        (shared, audit_path)
    }

    fn tcp_packet(ipv4: [u8; 4], port: u16) -> Vec<u8> {
        let mut packet = vec![0_u8; 40];
        packet[0] = 0x45;
        packet[9] = 6;
        packet[16..20].copy_from_slice(&ipv4);
        packet[22..24].copy_from_slice(&port.to_be_bytes());
        packet
    }

    fn poison_pending(shared: &Arc<Shared>) {
        let shared = Arc::clone(shared);
        let handle = std::thread::spawn(move || {
            let _guard = shared.pending.lock().expect("pending lock");
            panic!("poison mutex");
        });
        let _ = handle.join();
    }

    #[test]
    fn lookup_verdict_returns_pending_accept() {
        let (shared, audit_path) = test_shared();
        let key = DestKey {
            ipv4: u32::from_be_bytes([203, 0, 113, 7]),
            port: 443,
        };
        shared
            .pending
            .lock()
            .expect("pending")
            .insert(key, Verdict::Accept);
        let payload = tcp_packet([203, 0, 113, 7], 443);
        assert_eq!(lookup_verdict(&payload, &shared), Verdict::Accept);
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn lookup_verdict_drops_unparseable_payload() {
        let (shared, audit_path) = test_shared();
        assert_eq!(lookup_verdict(&[1, 2, 3], &shared), Verdict::Drop);
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn lookup_verdict_drops_when_pending_missing() {
        let (shared, audit_path) = test_shared();
        let payload = tcp_packet([10, 0, 0, 1], 8080);
        assert_eq!(
            lookup_verdict_with_attempts(&payload, &shared, 1),
            Verdict::Drop
        );
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn lookup_verdict_waits_for_pending_on_retry() {
        let (shared, audit_path) = test_shared();
        let key = DestKey {
            ipv4: u32::from_be_bytes([198, 51, 100, 4]),
            port: 8443,
        };
        let payload = tcp_packet([198, 51, 100, 4], 8443);
        let delayed = Arc::clone(&shared);
        let handle = std::thread::spawn(move || {
            thread::sleep(Duration::from_millis(2));
            delayed
                .pending
                .lock()
                .expect("pending")
                .insert(key, Verdict::Accept);
        });
        assert_eq!(
            lookup_verdict_with_attempts(&payload, &shared, 5),
            Verdict::Accept
        );
        let _ = handle.join();
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn lookup_verdict_drops_on_poisoned_pending() {
        let (shared, audit_path) = test_shared();
        poison_pending(&shared);
        let payload = tcp_packet([127, 0, 0, 1], 80);
        assert_eq!(
            lookup_verdict_with_attempts(&payload, &shared, 1),
            Verdict::Drop
        );
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn service_one_message_applies_pending_verdict() {
        let (shared, audit_path) = test_shared();
        let payload = tcp_packet([203, 0, 113, 9], 9001);
        let key = DestKey {
            ipv4: u32::from_be_bytes([203, 0, 113, 9]),
            port: 9001,
        };
        shared
            .pending
            .lock()
            .expect("pending")
            .insert(key, Verdict::Accept);
        let mut transport = FakeTransport {
            payloads: VecDeque::from([payload]),
            verdicts: Vec::new(),
            recv_error: None,
        };
        service_one_message(&mut transport, &shared).expect("service one");
        assert_eq!(transport.verdicts, vec![Verdict::Accept]);
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn nfqueue_service_once_marks_bound_and_applies_verdict() {
        let (shared, audit_path) = test_shared();
        let payload = tcp_packet([203, 0, 113, 11], 9011);
        let key = DestKey {
            ipv4: u32::from_be_bytes([203, 0, 113, 11]),
            port: 9011,
        };
        shared
            .pending
            .lock()
            .expect("pending")
            .insert(key, Verdict::Accept);
        let mut transport = FakeTransport {
            payloads: VecDeque::from([payload]),
            verdicts: Vec::new(),
            recv_error: None,
        };
        mark_nfqueue_bound(&shared);
        service_one_message(&mut transport, &shared).expect("service one");
        assert_eq!(transport.verdicts, vec![Verdict::Accept]);
        assert_eq!(shared.enforcement(), "nfqueue");
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn simulated_bound_run_applies_lookup_verdict() {
        let (shared, audit_path) = test_shared();
        let payload = tcp_packet([203, 0, 113, 8], 9000);
        let key = DestKey {
            ipv4: u32::from_be_bytes([203, 0, 113, 8]),
            port: 9000,
        };
        shared
            .pending
            .lock()
            .expect("pending")
            .insert(key, Verdict::Drop);
        let _simulate = ForceNfqueueSimulateBound::arm(payload);
        run(&shared).expect("simulated bound run");
        assert_eq!(shared.enforcement(), "nfqueue");
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn run_or_degrade_keeps_nfqueue_when_simulated_run_succeeds() {
        let (shared, audit_path) = test_shared();
        let payload = tcp_packet([203, 0, 113, 12], 9012);
        let _simulate = ForceNfqueueSimulateBound::arm(payload);
        run_or_degrade(&shared);
        assert_eq!(shared.enforcement(), "nfqueue");
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn service_one_message_propagates_transport_recv_error() {
        let (shared, audit_path) = test_shared();
        let mut transport = FakeTransport {
            payloads: VecDeque::new(),
            verdicts: Vec::new(),
            recv_error: Some(std::io::Error::other("recv failed")),
        };
        let error = service_one_message(&mut transport, &shared).expect_err("recv");
        assert_eq!(error.kind(), std::io::ErrorKind::Other);
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn run_without_live_bind_is_unavailable() {
        let _guard = NFQUEUE_TEST_LOCK.lock().expect("nfqueue test lock");
        let (shared, audit_path) = test_shared();
        let error = run(&shared).expect_err("live bind unavailable");
        assert!(error.to_string().contains("unavailable"));
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn simulated_bound_without_payload_marks_nfqueue() {
        let (shared, audit_path) = test_shared();
        let _simulate = ForceNfqueueSimulateBound::arm_without_payload();
        run(&shared).expect("simulated idle bind");
        assert_eq!(shared.enforcement(), "nfqueue");
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn run_or_degrade_marks_enforcement_degraded_on_bind_failure() {
        let (shared, audit_path) = test_shared();
        let _fail = ForceNfqueueFail::arm();
        run_or_degrade(&shared);
        assert_eq!(shared.enforcement(), "degraded");
        let _ = fs::remove_file(audit_path);
    }
}
