//! Bounded recent outbound destinations per process identity.
#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::net::Ipv4Addr;

/// One recent connect target for an observed process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecentDest {
    pub ipv4: Ipv4Addr,
    pub port: u16,
    pub verdict: String,
}

/// Per-identity ring of recent destinations (firewall-observed only).
pub struct RecentConnects {
    capacity_keys: usize,
    per_key: usize,
    order: VecDeque<(u32, u64)>,
    entries: HashMap<(u32, u64), VecDeque<RecentDest>>,
}

impl RecentConnects {
    /// Create a bounded recent-connect table.
    ///
    /// # Panics
    ///
    /// Panics when either capacity is zero.
    #[must_use]
    pub fn new(capacity_keys: usize, per_key: usize) -> Self {
        assert!(capacity_keys > 0 && per_key > 0);
        Self {
            capacity_keys,
            per_key,
            order: VecDeque::new(),
            entries: HashMap::new(),
        }
    }

    /// Record a destination for `(pid, start_ticks)`, newest last.
    pub fn record(&mut self, pid: u32, start_ticks: u64, dest: RecentDest) {
        let key = (pid, start_ticks);
        if !self.entries.contains_key(&key) {
            while self.entries.len() >= self.capacity_keys {
                if let Some(oldest) = self.order.pop_front() {
                    self.entries.remove(&oldest);
                } else {
                    self.entries.clear();
                    break;
                }
            }
            self.order.push_back(key);
            self.entries.insert(key, VecDeque::new());
        }
        let queue = self.entries.get_mut(&key).expect("just inserted");
        if let Some(existing) = queue
            .iter_mut()
            .find(|row| row.ipv4 == dest.ipv4 && row.port == dest.port)
        {
            existing.verdict.clone_from(&dest.verdict);
            return;
        }
        while queue.len() >= self.per_key {
            queue.pop_front();
        }
        queue.push_back(dest);
    }

    /// Destinations for one identity (oldest first).
    #[must_use]
    pub fn for_process(&self, pid: u32, start_ticks: u64) -> Vec<RecentDest> {
        self.entries
            .get(&(pid, start_ticks))
            .map(|queue| queue.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
impl RecentConnects {
    fn insert_orphan_key_for_test(&mut self, pid: u32, start_ticks: u64) {
        self.entries.insert((pid, start_ticks), VecDeque::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_per_key_and_updates_verdict() {
        let mut recent = RecentConnects::new(2, 2);
        let dest = |port, verdict: &str| RecentDest {
            ipv4: Ipv4Addr::new(203, 0, 113, 10),
            port,
            verdict: verdict.into(),
        };
        recent.record(1, 9, dest(443, "prompt"));
        recent.record(1, 9, dest(80, "allow"));
        recent.record(1, 9, dest(22, "deny"));
        let rows = recent.for_process(1, 9);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].port, 80);
        assert_eq!(rows[1].port, 22);
        recent.record(1, 9, dest(80, "deny"));
        let rows = recent.for_process(1, 9);
        assert_eq!(rows[0].verdict, "deny");
    }

    #[test]
    fn evicts_oldest_process_key() {
        let mut recent = RecentConnects::new(2, 4);
        let dest = |port| RecentDest {
            ipv4: Ipv4Addr::new(203, 0, 113, 10),
            port,
            verdict: "prompt".into(),
        };
        recent.record(1, 1, dest(443));
        recent.record(2, 2, dest(80));
        recent.record(3, 3, dest(22));
        assert!(recent.for_process(1, 1).is_empty());
        assert_eq!(recent.for_process(3, 3).len(), 1);
    }

    #[test]
    fn unknown_process_returns_empty() {
        let recent = RecentConnects::new(2, 2);
        assert!(recent.for_process(99, 1).is_empty());
    }

    #[test]
    fn record_breaks_when_order_empty_but_keys_full() {
        let mut recent = RecentConnects::new(1, 4);
        recent.insert_orphan_key_for_test(1, 1);
        recent.record(
            2,
            2,
            RecentDest {
                ipv4: Ipv4Addr::new(203, 0, 113, 10),
                port: 443,
                verdict: "allow".into(),
            },
        );
        assert!(recent.for_process(1, 1).is_empty());
        assert_eq!(recent.for_process(2, 2).len(), 1);
    }
}
