//! Shared daemon state for IPC, observation, and NFQUEUE threads.
#![forbid(unsafe_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use interfire_rules::{RuleSet, RulesStore};

use crate::pending::PendingTable;
use crate::process::ProcessCache;
use crate::prompts::PromptQueue;

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
    pub prompts: Mutex<PromptQueue>,
    enforcement: AtomicU8,
    observation: AtomicU8,
}

impl Shared {
    #[must_use]
    pub fn new(
        rules: RuleSet,
        store: RulesStore,
        observation: &'static str,
        pending_capacity: usize,
        pending_ttl: Duration,
        process_capacity: usize,
    ) -> Self {
        Self {
            rules: Mutex::new(rules),
            store,
            pending: Mutex::new(PendingTable::new(pending_capacity, pending_ttl)),
            process_cache: Mutex::new(ProcessCache::new(process_capacity)),
            prompts: Mutex::new(PromptQueue::with_defaults()),
            enforcement: AtomicU8::new(ENFORCEMENT_NONE),
            observation: AtomicU8::new(observation_code(observation)),
        }
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
