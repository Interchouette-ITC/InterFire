//! Shared daemon state for IPC, observation, and NFQUEUE threads.
#![forbid(unsafe_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use interfire_rules::{RuleSet, RulesStore};

use crate::audit::{AuditLog, DEFAULT_AUDIT_MAX_BYTES};
use crate::dns::DnsCache;
use crate::pending::PendingTable;
use crate::process::ProcessCache;
use crate::prompts::PromptQueue;
use crate::recent::RecentConnects;
use interfire_proto::MAX_LOG_RECORDS_PER_SUBSCRIBER;
use std::path::PathBuf;

const ENFORCEMENT_NONE: u8 = 0;
const ENFORCEMENT_NFQUEUE: u8 = 1;
const ENFORCEMENT_DEGRADED: u8 = 2;

const OBSERVATION_ATTACHED: u8 = 0;
const OBSERVATION_DEGRADED: u8 = 1;

/// Cross-thread daemon state.
pub struct Shared {
    pub rules: Mutex<RuleSet>,
    pub store: RulesStore,
    pub pending: Mutex<PendingTable>,
    pub process_cache: Mutex<ProcessCache>,
    pub recent: Mutex<RecentConnects>,
    pub prompts: Mutex<PromptQueue>,
    pub dns: Mutex<DnsCache>,
    pub audit: Mutex<AuditLog>,
    enforcement: AtomicU8,
    observation: AtomicU8,
}

impl Shared {
    /// Build shared state.
    ///
    /// # Errors
    ///
    /// Returns I/O failures while opening the audit log.
    pub fn new(
        rules: RuleSet,
        store: RulesStore,
        observation: &'static str,
        pending_capacity: usize,
        pending_ttl: Duration,
        process_capacity: usize,
        audit_path: PathBuf,
    ) -> std::io::Result<Self> {
        Ok(Self {
            rules: Mutex::new(rules),
            store,
            pending: Mutex::new(PendingTable::new(pending_capacity, pending_ttl)),
            process_cache: Mutex::new(ProcessCache::new(process_capacity)),
            recent: Mutex::new(RecentConnects::new(process_capacity, 8)),
            prompts: Mutex::new(PromptQueue::with_defaults()),
            dns: Mutex::new(DnsCache::with_defaults()),
            audit: Mutex::new(AuditLog::open(
                audit_path,
                DEFAULT_AUDIT_MAX_BYTES,
                MAX_LOG_RECORDS_PER_SUBSCRIBER,
            )?),
            enforcement: AtomicU8::new(ENFORCEMENT_NONE),
            observation: AtomicU8::new(observation_code(observation)),
        })
    }

    pub fn set_enforcement(&self, label: &'static str) {
        self.enforcement
            .store(enforcement_code(label), Ordering::Relaxed);
    }

    #[must_use]
    pub fn enforcement(&self) -> &'static str {
        enforcement_label(self.enforcement.load(Ordering::Relaxed))
    }

    #[must_use]
    pub fn observation(&self) -> &'static str {
        observation_label(self.observation.load(Ordering::Relaxed))
    }

    /// Replace the prompt queue (unit tests only).
    #[cfg(test)]
    pub fn set_prompt_queue(&self, queue: PromptQueue) {
        *self.prompts.lock().expect("prompts lock") = queue;
    }
}

fn enforcement_code(label: &str) -> u8 {
    match label {
        "nfqueue" => ENFORCEMENT_NFQUEUE,
        "degraded" => ENFORCEMENT_DEGRADED,
        _ => ENFORCEMENT_NONE,
    }
}

const fn enforcement_label(code: u8) -> &'static str {
    match code {
        ENFORCEMENT_NFQUEUE => "nfqueue",
        ENFORCEMENT_DEGRADED => "degraded",
        _ => "none",
    }
}

fn observation_code(label: &str) -> u8 {
    if label == "attached" {
        OBSERVATION_ATTACHED
    } else {
        OBSERVATION_DEGRADED
    }
}

const fn observation_label(code: u8) -> &'static str {
    if code == OBSERVATION_ATTACHED {
        "attached"
    } else {
        "degraded"
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use interfire_rules::RulesStore;

    use super::*;

    fn temp_audit(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("interfire-shared-{}-{name}", std::process::id()))
    }

    #[test]
    fn new_initializes_state_and_audit_log() {
        let audit_path = temp_audit("new.log");
        let _ = fs::remove_file(&audit_path);
        let store = RulesStore::new(audit_path.with_extension("rules.toml"));
        let shared = Shared::new(
            RuleSet::default(),
            store,
            "attached",
            8,
            Duration::from_secs(5),
            8,
            audit_path.clone(),
        )
        .expect("shared state");
        assert_eq!(shared.observation(), "attached");
        assert_eq!(shared.enforcement(), "none");
        shared.audit.lock().unwrap().append("boot");
        assert!(audit_path.exists());
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn enforcement_setters_and_getters() {
        let audit_path = temp_audit("enforce.log");
        let _ = fs::remove_file(&audit_path);
        let store = RulesStore::new(audit_path.with_extension("rules.toml"));
        let shared = Shared::new(
            RuleSet::default(),
            store,
            "degraded",
            8,
            Duration::from_secs(5),
            8,
            audit_path.clone(),
        )
        .expect("shared state");
        assert_eq!(shared.observation(), "degraded");
        shared.set_enforcement("nfqueue");
        assert_eq!(shared.enforcement(), "nfqueue");
        shared.set_enforcement("degraded");
        assert_eq!(shared.enforcement(), "degraded");
        shared.set_enforcement("unknown");
        assert_eq!(shared.enforcement(), "none");
        let _ = fs::remove_file(audit_path);
    }
}
