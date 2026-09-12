//! Shared daemon state for IPC, observation, and NFQUEUE threads.
#![forbid(unsafe_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use interfire_rules::{RuleSet, RulesStore};

use crate::audit::{AuditLog, DEFAULT_AUDIT_MAX_BYTES};
use crate::dns::DnsCache;
use crate::enforcement_mode::{self, EnforcementMode};
use crate::pending::PendingTable;
use crate::process::ProcessCache;
use crate::prompts::PromptQueue;
use crate::recent::RecentConnects;
use interfire_proto::MAX_LOG_RECORDS_PER_SUBSCRIBER;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

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
    mode_path: PathBuf,
    paused: AtomicBool,
    /// Live NFQUEUE bind state (`none` / `nfqueue` / `degraded`), independent of pause.
    bind_state: AtomicU8,
    observation: AtomicU8,
}

/// Inputs for [`Shared::new`] (keeps the constructor under Clippy's argument limit).
pub struct SharedConfig {
    pub rules: RuleSet,
    pub store: RulesStore,
    pub observation: &'static str,
    pub pending_capacity: usize,
    pub pending_ttl: Duration,
    pub process_capacity: usize,
    pub audit_path: PathBuf,
    pub mode_path: PathBuf,
}

impl Shared {
    /// Build shared state.
    ///
    /// # Errors
    ///
    /// Returns I/O failures while opening the audit log.
    pub fn new(config: SharedConfig) -> std::io::Result<Self> {
        let mode = enforcement_mode::load(&config.mode_path);
        Ok(Self {
            rules: Mutex::new(config.rules),
            store: config.store,
            pending: Mutex::new(PendingTable::new(
                config.pending_capacity,
                config.pending_ttl,
            )),
            process_cache: Mutex::new(ProcessCache::new(config.process_capacity)),
            recent: Mutex::new(RecentConnects::new(config.process_capacity, 8)),
            prompts: Mutex::new(PromptQueue::with_defaults()),
            dns: Mutex::new(DnsCache::with_defaults()),
            audit: Mutex::new(AuditLog::open(
                config.audit_path,
                DEFAULT_AUDIT_MAX_BYTES,
                MAX_LOG_RECORDS_PER_SUBSCRIBER,
            )?),
            mode_path: config.mode_path,
            paused: AtomicBool::new(mode == EnforcementMode::Paused),
            bind_state: AtomicU8::new(ENFORCEMENT_NONE),
            observation: AtomicU8::new(observation_code(config.observation)),
        })
    }

    pub fn set_enforcement(&self, label: &'static str) {
        self.bind_state
            .store(enforcement_code(label), Ordering::Relaxed);
    }

    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn mode_path(&self) -> &Path {
        self.mode_path.as_path()
    }

    #[must_use]
    fn bind_label(&self) -> &'static str {
        enforcement_label(self.bind_state.load(Ordering::Relaxed))
    }

    /// Persist pause and drop the owned table.
    ///
    /// # Errors
    ///
    /// Returns mode-file or nft remove failures.
    pub fn pause(&self) -> std::io::Result<()> {
        enforcement_mode::store(&self.mode_path, EnforcementMode::Paused)?;
        self.paused.store(true, Ordering::Relaxed);
        crate::nft::remove()
    }

    /// Persist active and install the owned table.
    ///
    /// # Errors
    ///
    /// Returns when the queue is not bound, or mode-file / nft install fails.
    pub fn resume(&self) -> std::io::Result<()> {
        if self.bind_label() != "nfqueue" {
            return Err(std::io::Error::other(
                "NFQUEUE not bound; cannot start enforcement",
            ));
        }
        enforcement_mode::store(&self.mode_path, EnforcementMode::Active)?;
        self.paused.store(false, Ordering::Relaxed);
        crate::nft::install()
    }

    #[must_use]
    pub fn enforcement(&self) -> &'static str {
        if self.is_paused() {
            return "paused";
        }
        self.bind_label()
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
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "interfire-shared-{}-{n}-{name}",
            std::process::id()
        ))
    }

    #[test]
    fn new_initializes_state_and_audit_log() {
        let audit_path = temp_audit("new.log");
        let _ = fs::remove_file(&audit_path);
        let store = RulesStore::new(audit_path.with_extension("rules.toml"));
        let shared = Shared::new(SharedConfig {
            rules: RuleSet::default(),
            store,
            observation: "attached",
            pending_capacity: 8,
            pending_ttl: Duration::from_secs(5),
            process_capacity: 8,
            audit_path: audit_path.clone(),
            mode_path: audit_path.with_extension("mode"),
        })
        .expect("shared state");
        assert_eq!(shared.observation(), "attached");
        assert_eq!(shared.enforcement(), "paused");
        shared.audit.lock().unwrap().append("boot");
        assert!(audit_path.exists());
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn new_fails_when_audit_path_is_a_directory() {
        let audit_dir = temp_audit("dir");
        let _ = fs::remove_dir_all(&audit_dir);
        fs::create_dir_all(&audit_dir).expect("mkdir");
        let store = RulesStore::new(audit_dir.join("rules.toml"));
        let result = Shared::new(SharedConfig {
            rules: RuleSet::default(),
            store,
            observation: "attached",
            pending_capacity: 8,
            pending_ttl: Duration::from_secs(5),
            process_capacity: 8,
            audit_path: audit_dir.clone(),
            mode_path: audit_dir.join("mode"),
        });
        assert!(result.is_err());
    }

    #[test]
    fn enforcement_setters_and_getters() {
        let audit_path = temp_audit("enforce.log");
        let _ = fs::remove_file(&audit_path);
        let mode_path = audit_path.with_extension("mode");
        crate::enforcement_mode::store(&mode_path, EnforcementMode::Active).expect("mode");
        let store = RulesStore::new(audit_path.with_extension("rules.toml"));
        let shared = Shared::new(SharedConfig {
            rules: RuleSet::default(),
            store,
            observation: "degraded",
            pending_capacity: 8,
            pending_ttl: Duration::from_secs(5),
            process_capacity: 8,
            audit_path: audit_path.clone(),
            mode_path,
        })
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

    #[test]
    fn pause_resume_and_apply_table_paths() {
        let audit_path = temp_audit("pause.log");
        let mode_path = audit_path.with_extension("mode");
        let _ = fs::remove_file(&audit_path);
        let _ = fs::remove_file(&mode_path);
        crate::enforcement_mode::store(&mode_path, EnforcementMode::Active).expect("mode");
        let store = RulesStore::new(audit_path.with_extension("rules.toml"));
        let shared = Shared::new(SharedConfig {
            rules: RuleSet::default(),
            store,
            observation: "attached",
            pending_capacity: 8,
            pending_ttl: Duration::from_secs(5),
            process_capacity: 8,
            audit_path: audit_path.clone(),
            mode_path: mode_path.clone(),
        })
        .expect("shared");
        shared.set_enforcement("nfqueue");
        assert_eq!(shared.mode_path(), mode_path.as_path());
        let _ok = crate::nft::ForceNftOk::arm();
        crate::enforcement_mode::apply_table(EnforcementMode::Paused);
        shared.pause().expect("pause");
        assert_eq!(shared.enforcement(), "paused");
        shared.resume().expect("resume");
        assert_eq!(shared.enforcement(), "nfqueue");
        let _ = fs::remove_file(audit_path);
        let _ = fs::remove_file(mode_path);
    }
}
