//! Bounded in-memory connect aggregates for stats IPC tabs.
#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

/// Maximum distinct keys retained per aggregate dimension.
pub const MAX_STATS_KEYS: usize = 512;

/// Per-key hit counters (allow / deny / prompt).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StatsCounters {
    pub hits: u64,
    pub allow: u64,
    pub deny: u64,
    pub prompt: u64,
}

impl StatsCounters {
    fn bump(&mut self, verdict: &str) {
        self.hits = self.hits.saturating_add(1);
        match verdict {
            "allow" => self.allow = self.allow.saturating_add(1),
            "deny" => self.deny = self.deny.saturating_add(1),
            "prompt" => self.prompt = self.prompt.saturating_add(1),
            _ => {}
        }
    }
}

/// One attributed connect used to update aggregates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectStats {
    pub executable: String,
    pub host: String,
    pub ipv4: String,
    pub port: u16,
    pub uid: u32,
    pub verdict: &'static str,
}

/// Footer / Daemon summary counters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatsSummary {
    pub connections: u64,
    pub denied: u64,
    pub uptime_secs: u64,
    pub rules: u64,
}

/// Bounded host/exe/addr/port/uid aggregates since daemon start.
pub struct StatsStore {
    started: Instant,
    connections: u64,
    denied: u64,
    hosts: HashMap<String, StatsCounters>,
    host_order: VecDeque<String>,
    procs: HashMap<String, StatsCounters>,
    proc_order: VecDeque<String>,
    addrs: HashMap<String, StatsCounters>,
    addr_order: VecDeque<String>,
    ports: HashMap<u16, StatsCounters>,
    port_order: VecDeque<u16>,
    users: HashMap<u32, StatsCounters>,
    user_order: VecDeque<u32>,
}

impl StatsStore {
    /// Create an empty store anchored at now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            connections: 0,
            denied: 0,
            hosts: HashMap::new(),
            host_order: VecDeque::new(),
            procs: HashMap::new(),
            proc_order: VecDeque::new(),
            addrs: HashMap::new(),
            addr_order: VecDeque::new(),
            ports: HashMap::new(),
            port_order: VecDeque::new(),
            users: HashMap::new(),
            user_order: VecDeque::new(),
        }
    }

    /// Record an attributed verdicted connect into all dimensions.
    pub fn record(&mut self, hit: &ConnectStats) {
        self.connections = self.connections.saturating_add(1);
        if hit.verdict == "deny" {
            self.denied = self.denied.saturating_add(1);
        }
        bump_string(
            &mut self.hosts,
            &mut self.host_order,
            &hit.host,
            hit.verdict,
        );
        bump_string(
            &mut self.procs,
            &mut self.proc_order,
            &hit.executable,
            hit.verdict,
        );
        bump_string(
            &mut self.addrs,
            &mut self.addr_order,
            &hit.ipv4,
            hit.verdict,
        );
        bump_port(&mut self.ports, &mut self.port_order, hit.port, hit.verdict);
        bump_uid(&mut self.users, &mut self.user_order, hit.uid, hit.verdict);
    }

    /// Count an unattributed connect as seen + denied (no aggregate keys).
    pub const fn record_unattributed_deny(&mut self) {
        self.connections = self.connections.saturating_add(1);
        self.denied = self.denied.saturating_add(1);
    }

    /// Footer / summary snapshot.
    #[must_use]
    pub fn summary(&self, rules: u64) -> StatsSummary {
        StatsSummary {
            connections: self.connections,
            denied: self.denied,
            uptime_secs: self.started.elapsed().as_secs(),
            rules,
        }
    }

    /// Host aggregate rows (newest-first order of first-seen keys, capped).
    #[must_use]
    pub fn hosts(&self) -> Vec<(String, StatsCounters)> {
        snapshot_string(&self.hosts, &self.host_order)
    }

    /// Executable aggregate rows.
    #[must_use]
    pub fn procs(&self) -> Vec<(String, StatsCounters)> {
        snapshot_string(&self.procs, &self.proc_order)
    }

    /// Destination address aggregate rows.
    #[must_use]
    pub fn addrs(&self) -> Vec<(String, StatsCounters)> {
        snapshot_string(&self.addrs, &self.addr_order)
    }

    /// Destination port aggregate rows.
    #[must_use]
    pub fn ports(&self) -> Vec<(String, StatsCounters)> {
        self.port_order
            .iter()
            .rev()
            .filter_map(|port| {
                self.ports
                    .get(port)
                    .map(|counters| (port.to_string(), counters.clone()))
            })
            .collect()
    }

    /// UID aggregate rows.
    #[must_use]
    pub fn users(&self) -> Vec<(String, StatsCounters)> {
        self.user_order
            .iter()
            .rev()
            .filter_map(|uid| {
                self.users
                    .get(uid)
                    .map(|counters| (uid.to_string(), counters.clone()))
            })
            .collect()
    }
}

impl Default for StatsStore {
    fn default() -> Self {
        Self::new()
    }
}

fn bump_string(
    map: &mut HashMap<String, StatsCounters>,
    order: &mut VecDeque<String>,
    key: &str,
    verdict: &str,
) {
    if !map.contains_key(key) {
        while map.len() >= MAX_STATS_KEYS {
            if let Some(oldest) = order.pop_front() {
                map.remove(&oldest);
            } else {
                map.clear();
                break;
            }
        }
        order.push_back(key.to_owned());
        map.insert(key.to_owned(), StatsCounters::default());
    }
    if let Some(counters) = map.get_mut(key) {
        counters.bump(verdict);
    }
}

fn bump_port(
    map: &mut HashMap<u16, StatsCounters>,
    order: &mut VecDeque<u16>,
    key: u16,
    verdict: &str,
) {
    if !map.contains_key(&key) {
        while map.len() >= MAX_STATS_KEYS {
            if let Some(oldest) = order.pop_front() {
                map.remove(&oldest);
            } else {
                map.clear();
                break;
            }
        }
        order.push_back(key);
        map.insert(key, StatsCounters::default());
    }
    if let Some(counters) = map.get_mut(&key) {
        counters.bump(verdict);
    }
}

fn bump_uid(
    map: &mut HashMap<u32, StatsCounters>,
    order: &mut VecDeque<u32>,
    key: u32,
    verdict: &str,
) {
    if !map.contains_key(&key) {
        while map.len() >= MAX_STATS_KEYS {
            if let Some(oldest) = order.pop_front() {
                map.remove(&oldest);
            } else {
                map.clear();
                break;
            }
        }
        order.push_back(key);
        map.insert(key, StatsCounters::default());
    }
    if let Some(counters) = map.get_mut(&key) {
        counters.bump(verdict);
    }
}

fn snapshot_string(
    map: &HashMap<String, StatsCounters>,
    order: &VecDeque<String>,
) -> Vec<(String, StatsCounters)> {
    order
        .iter()
        .rev()
        .filter_map(|key| map.get(key).map(|counters| (key.clone(), counters.clone())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_bumps_dimensions_and_denied() {
        let mut store = StatsStore::new();
        store.record(&ConnectStats {
            executable: "/usr/bin/curl".into(),
            host: "example.com".into(),
            ipv4: "93.184.216.34".into(),
            port: 443,
            uid: 1000,
            verdict: "deny",
        });
        store.record(&ConnectStats {
            executable: "/usr/bin/curl".into(),
            host: "example.com".into(),
            ipv4: "93.184.216.34".into(),
            port: 443,
            uid: 1000,
            verdict: "allow",
        });
        let summary = store.summary(3);
        assert_eq!(summary.connections, 2);
        assert_eq!(summary.denied, 1);
        assert_eq!(summary.rules, 3);
        assert_eq!(store.hosts()[0].1.hits, 2);
        assert_eq!(store.procs()[0].1.allow, 1);
        assert_eq!(store.ports()[0].0, "443");
        assert_eq!(store.users()[0].0, "1000");
    }

    #[test]
    fn unattributed_counts_without_keys() {
        let mut store = StatsStore::new();
        store.record_unattributed_deny();
        assert_eq!(store.summary(0).connections, 1);
        assert_eq!(store.summary(0).denied, 1);
        assert!(store.hosts().is_empty());
    }
}
