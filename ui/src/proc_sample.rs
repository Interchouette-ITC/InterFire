//! `/proc/self` samples and CPU % helpers for the Profiling section.
#![forbid(unsafe_code)]

use std::fs;
use std::io;
use std::time::{Duration, Instant};

/// One process sample used to compute RSS and CPU %.
#[derive(Clone, Copy, Debug)]
pub struct ProcSample {
    pub pid: u32,
    pub rss_kib: u64,
    pub cpu_jiffies: u64,
}

impl ProcSample {
    /// Sample this process from `/proc/self`.
    ///
    /// # Errors
    ///
    /// Returns I/O or parse failures when `/proc/self` is unreadable.
    pub fn sample_self() -> io::Result<Self> {
        Ok(Self {
            pid: std::process::id(),
            rss_kib: read_vm_rss_kib()?,
            cpu_jiffies: read_cpu_jiffies()?,
        })
    }
}

/// Pair of samples used to derive CPU percent over a wall interval.
#[derive(Clone, Copy, Debug, Default)]
pub struct CpuTracker {
    prev: Option<(u64, Instant)>,
}

impl CpuTracker {
    /// Update with the latest jiffy count and return CPU % since the previous sample.
    pub fn push(&mut self, cpu_jiffies: u64, at: Instant) -> Option<f64> {
        let percent = self.prev.and_then(|(prev_jiffies, prev_at)| {
            cpu_percent(prev_jiffies, prev_at, cpu_jiffies, at)
        });
        self.prev = Some((cpu_jiffies, at));
        percent
    }
}

/// Convert two jiffy samples into process CPU percent (may exceed 100 on multi-core).
#[must_use]
pub fn cpu_percent(
    prev_jiffies: u64,
    prev_at: Instant,
    next_jiffies: u64,
    next_at: Instant,
) -> Option<f64> {
    let elapsed = next_at.saturating_duration_since(prev_at);
    if elapsed < Duration::from_millis(50) {
        return None;
    }
    let delta = next_jiffies.saturating_sub(prev_jiffies);
    let ticks = f64::from(CLOCK_TICKS_PER_SEC);
    let secs = elapsed.as_secs_f64();
    if ticks <= 0.0 || secs <= 0.0 {
        return None;
    }
    let delta_capped = u32::try_from(delta.min(u64::from(u32::MAX))).unwrap_or(u32::MAX);
    Some(f64::from(delta_capped) / (ticks * secs) * 100.0)
}

/// Linux `USER_HZ` is almost always 100; good enough for UI CPU %.
const CLOCK_TICKS_PER_SEC: u32 = 100;

fn read_vm_rss_kib() -> io::Result<u64> {
    let status = fs::read_to_string("/proc/self/status")?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kib = rest
                .split_whitespace()
                .next()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "VmRSS missing value"))?;
            return kib.parse().map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("VmRSS: {error}"))
            });
        }
    }
    Err(io::Error::new(io::ErrorKind::InvalidData, "VmRSS missing"))
}

fn read_cpu_jiffies() -> io::Result<u64> {
    let line = fs::read_to_string("/proc/self/stat")?;
    let after_comm = line
        .rsplit_once(')')
        .map(|(_, rest)| rest.trim_start())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "stat missing comm"))?;
    let mut fields = after_comm.split_whitespace();
    let utime = fields
        .nth(11)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "stat missing utime"))?;
    let stime = fields
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "stat missing stime"))?;
    let utime: u64 = utime
        .parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("utime: {error}")))?;
    let stime: u64 = stime
        .parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("stime: {error}")))?;
    Ok(utime.saturating_add(stime))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_self_has_rss() {
        let sample = ProcSample::sample_self().expect("/proc/self");
        assert_eq!(sample.pid, std::process::id());
        assert!(sample.rss_kib > 0);
    }

    #[test]
    fn cpu_tracker_needs_two_samples() {
        let mut tracker = CpuTracker::default();
        let t0 = Instant::now();
        assert!(tracker.push(100, t0).is_none());
        let t1 = t0 + Duration::from_secs(1);
        let percent = tracker.push(200, t1).expect("second sample");
        assert!(percent > 0.0);
    }
}
