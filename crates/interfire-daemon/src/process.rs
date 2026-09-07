//! Race-aware `/proc` enrichment for kernel events.
#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub start_ticks: u64,
    pub executable: PathBuf,
    pub command_line: Vec<String>,
    pub uid: u32,
    pub cgroup: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AttributionError {
    #[error("procfs I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed procfs stat record")]
    MalformedStat,
    #[error("PID reused: expected {expected}, got {actual}")]
    PidReused { expected: u64, actual: u64 },
}

/// Resolve immediately after receiving an event. A start-tick mismatch means
/// the PID was reused and must never inherit the previous process's policy.
///
/// When `expected_start_ticks` is `0` (kernel program always emits `0`),
/// userspace accepts the live `/proc` start time without a reuse check.
///
/// # Errors
///
/// Returns I/O failures, malformed `/proc` records, or
/// [`AttributionError::PidReused`] when start ticks do not match.
pub fn resolve(pid: u32, expected_start_ticks: u64) -> Result<ProcessIdentity, AttributionError> {
    let root = PathBuf::from("/proc").join(pid.to_string());
    let start_ticks = parse_start_ticks(&fs::read_to_string(root.join("stat"))?)?;
    if expected_start_ticks != 0 && start_ticks != expected_start_ticks {
        return Err(AttributionError::PidReused {
            expected: expected_start_ticks,
            actual: start_ticks,
        });
    }
    let executable = fs::read_link(root.join("exe"))?;
    let command_line = fs::read(root.join("cmdline"))?
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect();
    let uid = fs::read_to_string(root.join("status"))?
        .lines()
        .find_map(|line| line.strip_prefix("Uid:\t"))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .ok_or(AttributionError::MalformedStat)?;
    let cgroup = fs::read_to_string(root.join("cgroup"))?;
    Ok(ProcessIdentity {
        pid,
        start_ticks,
        executable,
        command_line,
        uid,
        cgroup,
    })
}

/// Parse `starttime` (field 22) from a `/proc/<pid>/stat` line.
///
/// # Errors
///
/// Returns [`AttributionError::MalformedStat`] when the record cannot be parsed.
pub fn parse_start_ticks(stat: &str) -> Result<u64, AttributionError> {
    // The command name can contain spaces and parentheses; fields after the
    // final ')' start with field 3. starttime is field 22, index 19 here.
    stat.rsplit_once(')')
        .and_then(|(_, fields)| fields.split_whitespace().nth(19))
        .and_then(|value| value.parse().ok())
        .ok_or(AttributionError::MalformedStat)
}

pub struct ProcessCache {
    capacity: usize,
    entries: HashMap<(u32, u64), ProcessIdentity>,
    order: VecDeque<(u32, u64)>,
}

impl ProcessCache {
    /// Create a bounded process identity cache.
    ///
    /// # Panics
    ///
    /// Panics when `capacity` is zero.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "process cache must be bounded");
        Self {
            capacity,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn insert(&mut self, identity: ProcessIdentity) {
        let key = (identity.pid, identity.start_ticks);
        if self.entries.contains_key(&key) {
            return;
        }
        if self.entries.len() == self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        self.order.push_back(key);
        self.entries.insert(key, identity);
    }

    #[must_use]
    pub fn get(&self, pid: u32, start_ticks: u64) -> Option<&ProcessIdentity> {
        self.entries.get(&(pid, start_ticks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stat_with_spaces_in_command() {
        let mut fields = vec!["S".to_owned()];
        fields.extend((0..18).map(|_| "0".to_owned()));
        fields.push("12345".to_owned());
        assert_eq!(
            parse_start_ticks(&format!("7 (my worker) {}", fields.join(" "))).unwrap(),
            12345
        );
    }

    #[test]
    fn cache_evicts_oldest_identity() {
        let identity = |pid| ProcessIdentity {
            pid,
            start_ticks: 1,
            executable: "/bin/test".into(),
            command_line: vec![],
            uid: 0,
            cgroup: String::new(),
        };
        let mut cache = ProcessCache::new(1);
        cache.insert(identity(1));
        cache.insert(identity(2));
        assert!(cache.get(1, 1).is_none());
        assert!(cache.get(2, 1).is_some());
    }
}
