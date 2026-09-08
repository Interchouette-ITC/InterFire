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

fn parse_vm_rss_kib(status: &str) -> io::Result<u64> {
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

fn read_vm_rss_kib() -> io::Result<u64> {
    parse_vm_rss_kib(&fs::read_to_string("/proc/self/status")?)
}

fn parse_cpu_jiffies(stat: &str) -> io::Result<u64> {
    // Field 1 can contain spaces inside parentheses; split after the last ')'.
    let after_comm = stat
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

fn read_cpu_jiffies() -> io::Result<u64> {
    parse_cpu_jiffies(&fs::read_to_string("/proc/self/stat")?)
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

    #[test]
    fn parse_vm_rss_handles_valid_and_invalid_status() {
        assert_eq!(parse_vm_rss_kib("VmRSS:\t2048 kB\n").unwrap(), 2048);
        assert!(parse_vm_rss_kib("Name:\tbash\n").is_err());
        assert!(parse_vm_rss_kib("VmRSS:\tbad\n").is_err());
    }

    #[test]
    fn parse_cpu_jiffies_handles_valid_and_invalid_stat() {
        let mut fields = vec!["S".to_owned()];
        fields.extend((0..10).map(|_| "0".to_owned()));
        fields.push("100".to_owned());
        fields.push("200".to_owned());
        let stat = format!("42 (worker) {}", fields.join(" "));
        assert_eq!(parse_cpu_jiffies(&stat).unwrap(), 300);
        assert!(parse_cpu_jiffies("broken").is_err());
        assert!(parse_cpu_jiffies("1 (a) S").is_err());
    }
}
