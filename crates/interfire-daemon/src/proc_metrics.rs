//! Lightweight `/proc/self` RSS and CPU jiffy samples for status metrics.
#![forbid(unsafe_code)]

use std::fs;
use std::io;

/// Snapshot of this process for IPC / UI profiling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelfMetrics {
    pub pid: u32,
    pub rss_kib: u64,
    /// `utime + stime` from `/proc/self/stat` (clock ticks).
    pub cpu_jiffies: u64,
}

/// Read pid, `VmRSS`, and CPU jiffies for the current process.
///
/// # Errors
///
/// Returns I/O or parse failures when `/proc/self` is unreadable.
pub fn sample_self() -> io::Result<SelfMetrics> {
    let pid = std::process::id();
    let rss_kib = read_vm_rss_kib()?;
    let cpu_jiffies = read_cpu_jiffies()?;
    Ok(SelfMetrics {
        pid,
        rss_kib,
        cpu_jiffies,
    })
}

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
    // Field 1 can contain spaces inside parentheses; split after the last ')'.
    let after_comm = line
        .rsplit_once(')')
        .map(|(_, rest)| rest.trim_start())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "stat missing comm"))?;
    let mut fields = after_comm.split_whitespace();
    // After comm: state(2) … utime(14) stime(15) relative to full stat = indices 11,12 of remainder.
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
    fn sample_self_reads_positive_rss() {
        let metrics = sample_self().expect("/proc/self");
        assert_eq!(metrics.pid, std::process::id());
        assert!(metrics.rss_kib > 0);
    }
}
