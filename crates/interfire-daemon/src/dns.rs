//! Bounded DNS observation cache: IP is authoritative; names expire.
#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use interfire_proto::MAX_DNS_ENTRIES;
use tracing::debug;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Entry {
    hostname: String,
    expires_at: Instant,
}

/// Maps observed IPv4 answers to hostnames until TTL expiry.
pub struct DnsCache {
    capacity: usize,
    default_ttl: Duration,
    by_ip: HashMap<u32, Entry>,
    order: VecDeque<u32>,
}

impl DnsCache {
    /// Create a bounded DNS cache.
    ///
    /// # Panics
    ///
    /// Panics when `capacity` is zero.
    #[must_use]
    pub fn new(capacity: usize, default_ttl: Duration) -> Self {
        assert!(capacity > 0, "DNS cache must be bounded");
        Self {
            capacity,
            default_ttl,
            by_ip: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(MAX_DNS_ENTRIES, Duration::from_secs(300))
    }

    /// Record an observed name → address binding.
    pub fn observe(&mut self, hostname: &str, ipv4: u32, ttl: Option<Duration>) {
        self.expire();
        let hostname = hostname.to_ascii_lowercase();
        let ttl = ttl.unwrap_or(self.default_ttl);
        let entry = Entry {
            hostname,
            expires_at: Instant::now() + ttl,
        };
        if let Some(existing) = self.by_ip.get_mut(&ipv4) {
            *existing = entry;
            return;
        }
        while self.by_ip.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.by_ip.remove(&oldest);
            } else {
                self.by_ip.clear();
                break;
            }
        }
        self.order.push_back(ipv4);
        self.by_ip.insert(ipv4, entry);
        debug!(ipv4 = %Ipv4Addr::from(ipv4.to_ne_bytes()), "DNS observation cached");
    }

    /// Fresh hostname for `ipv4`, or [`None`] when missing or expired.
    ///
    /// Stale names are dropped so they never override IP truth for verdicts.
    #[must_use]
    pub fn hostname_for(&mut self, ipv4: u32) -> Option<String> {
        self.expire();
        self.by_ip.get(&ipv4).map(|entry| entry.hostname.clone())
    }

    /// List still-fresh bindings as `hostname|ipv4|ttl_secs`.
    #[must_use]
    pub fn list_fresh(&mut self) -> Vec<(String, u32, u64)> {
        self.expire();
        let now = Instant::now();
        self.order
            .iter()
            .filter_map(|ipv4| {
                self.by_ip.get(ipv4).map(|entry| {
                    let remaining = entry.expires_at.saturating_duration_since(now).as_secs();
                    (entry.hostname.clone(), *ipv4, remaining)
                })
            })
            .collect()
    }

    fn expire(&mut self) {
        let now = Instant::now();
        let expired: Vec<u32> = self
            .by_ip
            .iter()
            .filter_map(|(ipv4, entry)| (now >= entry.expires_at).then_some(*ipv4))
            .collect();
        for ipv4 in expired {
            self.by_ip.remove(&ipv4);
        }
        self.order.retain(|ipv4| self.by_ip.contains_key(ipv4));
    }
}

#[cfg(test)]
impl DnsCache {
    fn insert_orphan_for_test(&mut self, ipv4: u32, hostname: &str) {
        self.by_ip.insert(
            ipv4,
            Entry {
                hostname: hostname.to_ascii_lowercase(),
                expires_at: Instant::now() + self.default_ttl,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_name_is_returned() {
        let mut cache = DnsCache::new(4, Duration::from_secs(60));
        let ip = u32::from_ne_bytes([203, 0, 113, 1]);
        cache.observe("Example.TEST", ip, None);
        assert_eq!(cache.hostname_for(ip).as_deref(), Some("example.test"));
    }

    #[test]
    fn stale_name_is_not_returned() {
        let mut cache = DnsCache::new(4, Duration::from_millis(1));
        let ip = u32::from_ne_bytes([203, 0, 113, 2]);
        cache.observe("gone.test", ip, Some(Duration::from_millis(1)));
        std::thread::sleep(Duration::from_millis(5));
        assert!(cache.hostname_for(ip).is_none());
    }

    #[test]
    fn capacity_evicts_oldest_ip() {
        let mut cache = DnsCache::new(1, Duration::from_secs(60));
        let first = u32::from_ne_bytes([1, 0, 0, 1]);
        let second = u32::from_ne_bytes([1, 0, 0, 2]);
        cache.observe("a.test", first, None);
        cache.observe("b.test", second, None);
        assert!(cache.hostname_for(first).is_none());
        assert_eq!(cache.hostname_for(second).as_deref(), Some("b.test"));
    }

    #[test]
    fn with_defaults_and_list_fresh() {
        let mut cache = DnsCache::with_defaults();
        let ip = u32::from_ne_bytes([203, 0, 113, 3]);
        cache.observe("fresh.test", ip, Some(Duration::from_secs(120)));
        let rows = cache.list_fresh();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "fresh.test");
        assert_eq!(rows[0].1, ip);
        assert!(rows[0].2 > 0);
    }

    #[test]
    fn observe_updates_existing_ip() {
        let mut cache = DnsCache::new(4, Duration::from_secs(60));
        let ip = u32::from_ne_bytes([203, 0, 113, 4]);
        cache.observe("first.test", ip, None);
        cache.observe("second.test", ip, None);
        assert_eq!(cache.hostname_for(ip).as_deref(), Some("second.test"));
    }

    #[test]
    fn observe_breaks_when_order_empty_but_cache_full() {
        let mut cache = DnsCache::new(1, Duration::from_secs(60));
        let first = u32::from_ne_bytes([1, 0, 0, 1]);
        cache.insert_orphan_for_test(first, "orphan.test");
        cache.observe("b.test", u32::from_ne_bytes([1, 0, 0, 2]), None);
        assert!(cache.hostname_for(first).is_none());
    }
}
