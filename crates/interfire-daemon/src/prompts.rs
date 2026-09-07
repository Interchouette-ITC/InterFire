//! Bounded prompt lifecycle: IDs, expiry, dedupe, and idempotent answers.
#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use interfire_proto::{MAX_PENDING_PROMPTS, PromptState, RuleScope};
use interfire_rules::Verdict;
use tracing::warn;

/// Identity used to coalesce duplicate pending prompts.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PromptKey {
    pub executable: String,
    pub ipv4: u32,
    pub port: u16,
}

impl PromptKey {
    #[must_use]
    pub fn ipv4_display(&self) -> String {
        Ipv4Addr::from(self.ipv4.to_ne_bytes()).to_string()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prompt {
    pub id: u64,
    pub key: PromptKey,
    pub state: PromptState,
    pub created_at: Instant,
    pub expires_at: Instant,
    pub answer_verdict: Option<Verdict>,
    pub answer_scope: Option<RuleScope>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnqueueOutcome {
    Created(u64),
    Deduped(u64),
    Full,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Answered {
    pub id: u64,
    pub key: PromptKey,
    pub verdict: Verdict,
    pub scope: RuleScope,
    pub duplicate: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AnswerError {
    #[error("prompt not found")]
    NotFound,
    #[error("prompt expired")]
    Expired,
}

/// Bounded pending-prompt store with once-allow tokens.
pub struct PromptQueue {
    capacity: usize,
    ttl: Duration,
    next_id: u64,
    order: VecDeque<u64>,
    prompts: HashMap<u64, Prompt>,
    pending_by_key: HashMap<PromptKey, u64>,
    once_allows: HashMap<PromptKey, Instant>,
    once_denies: HashSet<PromptKey>,
}

impl PromptQueue {
    /// Create a bounded prompt queue.
    ///
    /// # Panics
    ///
    /// Panics when `capacity` is zero.
    #[must_use]
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        assert!(capacity > 0, "prompt queue must be bounded");
        Self {
            capacity,
            ttl,
            next_id: 1,
            order: VecDeque::new(),
            prompts: HashMap::new(),
            pending_by_key: HashMap::new(),
            once_allows: HashMap::new(),
            once_denies: HashSet::new(),
        }
    }

    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(MAX_PENDING_PROMPTS, Duration::from_secs(60))
    }

    /// Consume a one-shot allow token for `key`, if still fresh.
    pub fn take_once_allow(&mut self, key: &PromptKey) -> bool {
        self.expire();
        self.once_allows
            .remove(key)
            .is_some_and(|inserted| Instant::now() <= inserted + self.ttl)
    }

    /// Return whether a one-shot deny token still applies.
    pub fn consume_once_deny(&mut self, key: &PromptKey) -> bool {
        self.expire();
        self.once_denies.remove(key)
    }

    /// Enqueue a prompt or coalesce with an existing pending one.
    pub fn enqueue(&mut self, key: PromptKey) -> EnqueueOutcome {
        self.expire();
        if let Some(id) = self.pending_by_key.get(&key).copied() {
            return EnqueueOutcome::Deduped(id);
        }
        if self.pending_by_key.len() >= self.capacity {
            warn!(
                executable = %key.executable,
                port = key.port,
                "prompt queue full; denying"
            );
            return EnqueueOutcome::Full;
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let now = Instant::now();
        self.prompts.insert(
            id,
            Prompt {
                id,
                key: key.clone(),
                state: PromptState::Pending,
                created_at: now,
                expires_at: now + self.ttl,
                answer_verdict: None,
                answer_scope: None,
            },
        );
        self.order.push_back(id);
        self.pending_by_key.insert(key, id);
        EnqueueOutcome::Created(id)
    }

    /// List still-pending prompts (oldest first).
    #[must_use]
    pub fn list_pending(&mut self) -> Vec<Prompt> {
        self.expire();
        self.order
            .iter()
            .filter_map(|id| {
                self.prompts.get(id).and_then(|prompt| {
                    (prompt.state == PromptState::Pending).then(|| prompt.clone())
                })
            })
            .collect()
    }

    /// Answer a prompt. Duplicate answers return the prior decision.
    ///
    /// # Errors
    ///
    /// Returns [`AnswerError::NotFound`] or [`AnswerError::Expired`].
    pub fn answer(
        &mut self,
        id: u64,
        verdict: Verdict,
        scope: RuleScope,
    ) -> Result<Answered, AnswerError> {
        self.expire();
        let prompt = self.prompts.get_mut(&id).ok_or(AnswerError::NotFound)?;
        if prompt.state == PromptState::Expired {
            return Err(AnswerError::Expired);
        }
        if prompt.state == PromptState::Answered {
            return Ok(Answered {
                id,
                key: prompt.key.clone(),
                verdict: prompt.answer_verdict.unwrap_or(verdict),
                scope: prompt.answer_scope.unwrap_or(scope),
                duplicate: true,
            });
        }
        prompt.state = PromptState::Answered;
        prompt.answer_verdict = Some(verdict);
        prompt.answer_scope = Some(scope);
        let key = prompt.key.clone();
        self.pending_by_key.remove(&key);
        match (verdict, scope) {
            (Verdict::Allow, RuleScope::Once) => {
                self.once_allows.insert(key.clone(), Instant::now());
            }
            (Verdict::Deny, RuleScope::Once) => {
                self.once_denies.insert(key.clone());
            }
            _ => {}
        }
        Ok(Answered {
            id,
            key,
            verdict,
            scope,
            duplicate: false,
        })
    }

    fn expire(&mut self) {
        let now = Instant::now();
        let expired: Vec<(u64, PromptKey)> = self
            .prompts
            .values()
            .filter(|prompt| prompt.state == PromptState::Pending && now > prompt.expires_at)
            .map(|prompt| (prompt.id, prompt.key.clone()))
            .collect();
        for (id, key) in expired {
            if let Some(prompt) = self.prompts.get_mut(&id) {
                prompt.state = PromptState::Expired;
            }
            self.pending_by_key.remove(&key);
        }
        self.order.retain(|id| {
            self.prompts
                .get(id)
                .is_some_and(|prompt| prompt.state == PromptState::Pending)
        });
        self.once_allows
            .retain(|_, inserted| now <= *inserted + self.ttl);
        if self.once_denies.len() > self.capacity {
            self.once_denies.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(port: u16) -> PromptKey {
        PromptKey {
            executable: "/usr/bin/curl".into(),
            ipv4: u32::from_be_bytes([127, 0, 0, 1]),
            port,
        }
    }

    #[test]
    fn dedupes_pending_prompts() {
        let mut queue = PromptQueue::new(4, Duration::from_secs(60));
        assert_eq!(queue.enqueue(key(443)), EnqueueOutcome::Created(1));
        assert_eq!(queue.enqueue(key(443)), EnqueueOutcome::Deduped(1));
        assert_eq!(queue.list_pending().len(), 1);
    }

    #[test]
    fn full_queue_denies_new_keys() {
        let mut queue = PromptQueue::new(1, Duration::from_secs(60));
        assert!(matches!(queue.enqueue(key(1)), EnqueueOutcome::Created(_)));
        assert_eq!(queue.enqueue(key(2)), EnqueueOutcome::Full);
    }

    #[test]
    fn answer_is_idempotent() {
        let mut queue = PromptQueue::new(4, Duration::from_secs(60));
        let EnqueueOutcome::Created(id) = queue.enqueue(key(80)) else {
            panic!("expected created");
        };
        let first = queue
            .answer(id, Verdict::Allow, RuleScope::Once)
            .expect("answer");
        assert!(!first.duplicate);
        let second = queue
            .answer(id, Verdict::Deny, RuleScope::Permanent)
            .expect("duplicate");
        assert!(second.duplicate);
        assert_eq!(second.verdict, Verdict::Allow);
        assert_eq!(second.scope, RuleScope::Once);
    }

    #[test]
    fn once_allow_token_is_single_use() {
        let mut queue = PromptQueue::new(4, Duration::from_secs(60));
        let EnqueueOutcome::Created(id) = queue.enqueue(key(443)) else {
            panic!("expected created");
        };
        queue.answer(id, Verdict::Allow, RuleScope::Once).unwrap();
        assert!(queue.take_once_allow(&key(443)));
        assert!(!queue.take_once_allow(&key(443)));
    }

    #[test]
    fn expired_prompt_cannot_be_answered() {
        let mut queue = PromptQueue::new(4, Duration::from_millis(1));
        let EnqueueOutcome::Created(id) = queue.enqueue(key(9)) else {
            panic!("expected created");
        };
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(
            queue.answer(id, Verdict::Allow, RuleScope::Once),
            Err(AnswerError::Expired)
        );
    }
}
