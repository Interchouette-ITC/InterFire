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
use crate::traffic_mode::{self, TrafficPreference};
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
    /// Machine traffic preference file (`traffic.machine`).
    traffic_path: PathBuf,
    paused: AtomicBool,
    machine_traffic: std::sync::Mutex<TrafficPreference>,
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
    pub traffic_path: PathBuf,
}

impl Shared {
    /// Build shared state.
    ///
    /// # Errors
    ///
    /// Returns I/O failures while opening the audit log.
    pub fn new(config: SharedConfig) -> std::io::Result<Self> {
        let mode = enforcement_mode::load(&config.mode_path);
        traffic_mode::migrate_legacy(&config.traffic_path);
        let machine = traffic_mode::load(&config.traffic_path);
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
            traffic_path: config.traffic_path,
            paused: AtomicBool::new(mode == EnforcementMode::Paused),
            machine_traffic: Mutex::new(machine),
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

    /// True when machine kill-switch is active (overrides user).
    #[must_use]
    pub fn is_traffic_blocked(&self) -> bool {
        self.machine_preference().is_blocked()
    }

    #[must_use]
    pub fn mode_path(&self) -> &Path {
        self.mode_path.as_path()
    }

    #[must_use]
    pub fn traffic_path(&self) -> &Path {
        self.traffic_path.as_path()
    }

    #[must_use]
    pub fn machine_preference(&self) -> TrafficPreference {
        *self
            .machine_traffic
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[must_use]
    pub fn user_preference(&self, uid: u32) -> TrafficPreference {
        traffic_mode::load(&traffic_mode::user_path(&self.traffic_path, uid))
    }

    #[must_use]
    fn bind_label(&self) -> &'static str {
        enforcement_label(self.bind_state.load(Ordering::Relaxed))
    }

    fn rules_mode(&self) -> EnforcementMode {
        if self.is_paused() {
            EnforcementMode::Paused
        } else {
            EnforcementMode::Active
        }
    }

    fn queue_bound(&self) -> bool {
        self.bind_label() == "nfqueue"
    }

    fn reapply_table(&self) -> std::io::Result<()> {
        let machine = self.machine_preference();
        let users = traffic_mode::load_user_blocks(&self.traffic_path);
        let rules = self.rules_mode();
        let queue_bound = self.queue_bound();
        let rules_active = rules == EnforcementMode::Active;
        let machine_blocks = machine.is_blocked();
        let users_block = users.iter().any(|(_, preference)| preference.is_blocked());

        if !machine_blocks && !users_block && !rules_active {
            return crate::nft::remove();
        }
        if !machine_blocks && !users_block && rules_active {
            return if queue_bound {
                crate::nft::install()
            } else {
                crate::nft::remove()
            };
        }
        crate::nft::install_composed(&crate::nft::TrafficCompose {
            machine,
            users: &users,
            rules_active: rules_active && !machine.wants_out(),
            queue_bound,
        })
    }

    /// Persist pause; keep Traffic kill-switch table when any block is active.
    ///
    /// # Errors
    ///
    /// Returns mode-file or nft failures.
    pub fn pause(&self) -> std::io::Result<()> {
        enforcement_mode::store(&self.mode_path, EnforcementMode::Paused)?;
        self.paused.store(true, Ordering::Relaxed);
        self.reapply_table()
    }

    /// Persist active and install the owned queue table (or keep Traffic table).
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
        self.reapply_table()
    }

    /// Persist a Traffic Block for machine or one UID, then reapply nft.
    ///
    /// # Errors
    ///
    /// Returns mode-file or nft failures.
    pub fn traffic_block(
        &self,
        scope: TrafficScope,
        preference: TrafficPreference,
        uid: u32,
    ) -> std::io::Result<()> {
        if matches!(preference, TrafficPreference::Open) {
            return Err(std::io::Error::other(
                "traffic block requires out, in, or all",
            ));
        }
        match scope {
            TrafficScope::Machine => {
                traffic_mode::store(&self.traffic_path, preference)?;
                *self
                    .machine_traffic
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = preference;
            }
            TrafficScope::User => {
                traffic_mode::store(
                    &traffic_mode::user_path(&self.traffic_path, uid),
                    preference,
                )?;
            }
        }
        self.reapply_table()
    }

    /// Persist Traffic Open for a scope and restore Rules-owned table as needed.
    ///
    /// # Errors
    ///
    /// Returns mode-file or nft failures.
    pub fn traffic_unblock(&self, scope: TrafficScope, uid: u32) -> std::io::Result<()> {
        match scope {
            TrafficScope::Machine => {
                traffic_mode::store(&self.traffic_path, TrafficPreference::Open)?;
                *self
                    .machine_traffic
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = TrafficPreference::Open;
            }
            TrafficScope::User => {
                traffic_mode::store(
                    &traffic_mode::user_path(&self.traffic_path, uid),
                    TrafficPreference::Open,
                )?;
            }
        }
        self.reapply_table()
    }

    #[must_use]
    pub fn enforcement(&self) -> &'static str {
        if self.is_paused() {
            return "paused";
        }
        self.bind_label()
    }

    /// Effective traffic label for status (`open` or `machine:…` / `user:…`).
    #[must_use]
    pub fn traffic_effective(&self, uid: u32) -> String {
        traffic_mode::effective(self.machine_preference(), self.user_preference(uid), uid).label()
    }

    /// Compact tray/status token: `open` or `blocked`.
    #[must_use]
    pub fn traffic(&self) -> &'static str {
        if self.is_traffic_blocked() || traffic_mode::any_block_active(&self.traffic_path) {
            "blocked"
        } else {
            "open"
        }
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

/// Traffic mutate scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrafficScope {
    Machine,
    User,
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

    fn shared_for(audit_path: &Path, mode: EnforcementMode) -> Shared {
        let mode_path = audit_path.with_extension("mode");
        let traffic_path = audit_path.with_extension("traffic");
        let _ = fs::remove_file(audit_path);
        let _ = fs::remove_file(&mode_path);
        let _ = fs::remove_file(&traffic_path);
        enforcement_mode::store(&mode_path, mode).expect("mode");
        let store = RulesStore::new(audit_path.with_extension("rules.toml"));
        Shared::new(SharedConfig {
            rules: RuleSet::default(),
            store,
            observation: "attached",
            pending_capacity: 8,
            pending_ttl: Duration::from_secs(5),
            process_capacity: 8,
            audit_path: audit_path.to_path_buf(),
            mode_path,
            traffic_path,
        })
        .expect("shared")
    }

    #[test]
    fn new_initializes_state_and_audit_log() {
        let audit_path = temp_audit("new.log");
        let shared = shared_for(&audit_path, EnforcementMode::Paused);
        assert_eq!(shared.observation(), "attached");
        assert_eq!(shared.enforcement(), "paused");
        assert_eq!(shared.traffic(), "open");
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
            traffic_path: audit_dir.join("traffic"),
        });
        assert!(result.is_err());
    }

    #[test]
    fn enforcement_setters_and_getters() {
        let audit_path = temp_audit("enforce.log");
        let shared = shared_for(&audit_path, EnforcementMode::Active);
        assert_eq!(shared.observation(), "attached");
        shared.set_enforcement("nfqueue");
        assert_eq!(shared.enforcement(), "nfqueue");
        shared.set_enforcement("degraded");
        assert_eq!(shared.enforcement(), "degraded");
        shared.set_enforcement("unknown");
        assert_eq!(shared.enforcement(), "none");
        let _ = fs::remove_file(audit_path);
    }

    #[test]
    fn pause_resume_and_traffic_block_paths() {
        let audit_path = temp_audit("pause.log");
        let mode_path = audit_path.with_extension("mode");
        let traffic_path = audit_path.with_extension("traffic");
        let shared = shared_for(&audit_path, EnforcementMode::Active);
        shared.set_enforcement("nfqueue");
        assert_eq!(shared.mode_path(), mode_path.as_path());
        assert_eq!(shared.traffic_path(), traffic_path.as_path());
        let _ok = crate::nft::ForceNftOk::arm();
        shared.pause().expect("pause");
        assert_eq!(shared.enforcement(), "paused");
        shared.resume().expect("resume");
        assert_eq!(shared.enforcement(), "nfqueue");
        shared
            .traffic_block(TrafficScope::Machine, TrafficPreference::Out, 0)
            .expect("block");
        assert_eq!(shared.traffic(), "blocked");
        assert!(shared.is_traffic_blocked());
        shared.pause().expect("pause while blocked");
        assert_eq!(shared.enforcement(), "paused");
        assert_eq!(shared.traffic(), "blocked");
        shared.resume().expect("resume while blocked");
        assert_eq!(shared.enforcement(), "nfqueue");
        assert_eq!(shared.traffic(), "blocked");
        shared
            .traffic_block(TrafficScope::User, TrafficPreference::In, 1000)
            .expect("user block while machine on");
        assert_eq!(shared.user_preference(1000), TrafficPreference::In);
        shared
            .traffic_unblock(TrafficScope::Machine, 0)
            .expect("unblock machine");
        assert_eq!(shared.traffic_effective(1000), "user:1000:in");
        shared.pause().expect("pause open machine");
        shared
            .traffic_unblock(TrafficScope::User, 1000)
            .expect("unblock user");
        assert_eq!(shared.traffic(), "open");
        shared.set_enforcement("degraded");
        assert!(shared.resume().is_err());
        shared.set_enforcement("nfqueue");
        shared.resume().expect("resume active");
        shared
            .traffic_block(TrafficScope::Machine, TrafficPreference::All, 0)
            .expect("block all");
        shared.set_enforcement("none");
        shared
            .traffic_unblock(TrafficScope::Machine, 0)
            .expect("unblock without bind");
        assert!(
            shared
                .traffic_block(TrafficScope::User, TrafficPreference::Open, 1000)
                .is_err()
        );
        let _ = fs::remove_file(&audit_path);
        let _ = fs::remove_file(mode_path);
        let _ = fs::remove_file(traffic_mode::user_path(&traffic_path, 1000));
        let _ = fs::remove_file(&traffic_path);
    }
}
