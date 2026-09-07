//! Bounded map of recent connect decisions keyed by destination tuple.
#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use nfq::Verdict;

/// Destination tuple used to join observation events with NFQUEUE packets.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DestKey {
    pub ipv4: u32,
    pub port: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingEntry {
    verdict: Verdict,
    inserted_at: Instant,
}

/// Bounded, TTL-expiring store of packet verdicts.
pub struct PendingTable {
    capacity: usize,
    ttl: Duration,
    entries: HashMap<DestKey, PendingEntry>,
    order: VecDeque<DestKey>,
}

impl PendingTable {
    /// Create a bounded pending-verdict table.
    ///
    /// # Panics
    ///
    /// Panics when `capacity` is zero.
    #[must_use]
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        assert!(capacity > 0, "pending table must be bounded");
        Self {
            capacity,
            ttl,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn insert(&mut self, key: DestKey, verdict: Verdict) {
        self.expire();
        let fresh = PendingEntry {
            verdict,
            inserted_at: Instant::now(),
        };
        if let Some(existing) = self.entries.get_mut(&key) {
            *existing = fresh;
            return;
        }
        while self.entries.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
        self.order.push_back(key);
        self.entries.insert(key, fresh);
    }

    /// Take a still-fresh verdict for `key`, or [`None`] when missing/expired.
    pub fn take(&mut self, key: DestKey) -> Option<Verdict> {
        self.expire();
        self.entries.remove(&key).map(|entry| entry.verdict)
    }

    fn expire(&mut self) {
        let now = Instant::now();
        self.order.retain(|key| {
            let Some(entry) = self.entries.get(key) else {
                return false;
            };
            if now.duration_since(entry.inserted_at) > self.ttl {
                self.entries.remove(key);
                false
            } else {
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_key_is_none() {
        let mut table = PendingTable::new(4, Duration::from_secs(5));
        assert!(table.take(DestKey { ipv4: 1, port: 80 }).is_none());
    }

    #[test]
    fn insert_then_take_returns_verdict() {
        let mut table = PendingTable::new(4, Duration::from_secs(5));
        let key = DestKey {
            ipv4: 0x7f00_0001,
            port: 443,
        };
        table.insert(key, Verdict::Accept);
        assert_eq!(table.take(key), Some(Verdict::Accept));
        assert!(table.take(key).is_none());
    }

    #[test]
    fn capacity_evicts_oldest() {
        let mut table = PendingTable::new(1, Duration::from_secs(5));
        table.insert(DestKey { ipv4: 1, port: 1 }, Verdict::Accept);
        table.insert(DestKey { ipv4: 2, port: 2 }, Verdict::Drop);
        assert!(table.take(DestKey { ipv4: 1, port: 1 }).is_none());
        assert_eq!(
            table.take(DestKey { ipv4: 2, port: 2 }),
            Some(Verdict::Drop)
        );
    }
}
