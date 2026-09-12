//! Persisted traffic mode (`open` | `blocked`).
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tracing::warn;

/// Default path under the daemon state directory.
pub const DEFAULT_TRAFFIC_PATH: &str = "/var/lib/interfire/traffic.mode";

static STORE_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Operator-selected traffic preference (kill-switch).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrafficMode {
    /// Normal path: Rules mode owns the table (queue or absent).
    Open,
    /// Fail-closed drop of new outbound TCP except loopback.
    Blocked,
}

impl TrafficMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Blocked => "blocked",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "open" => Some(Self::Open),
            "blocked" => Some(Self::Blocked),
            _ => None,
        }
    }
}

/// Load mode from disk. Missing or invalid file → [`TrafficMode::Open`].
#[must_use]
pub fn load(path: &Path) -> TrafficMode {
    match fs::read_to_string(path) {
        Ok(raw) => TrafficMode::parse(&raw).unwrap_or(TrafficMode::Open),
        Err(error) if error.kind() == io::ErrorKind::NotFound => TrafficMode::Open,
        Err(error) => {
            warn!(%error, path = %path.display(), "traffic mode unreadable; default open");
            TrafficMode::Open
        }
    }
}

/// Persist mode atomically under the parent directory.
///
/// # Errors
///
/// Returns I/O failures while creating the parent or writing the file.
pub fn store(path: &Path, mode: TrafficMode) -> io::Result<()> {
    ensure_parent(path)?;
    let seq = STORE_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("tmp.{seq}"));
    fs::write(&temporary, format!("{}\n", mode.as_str()))?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn ensure_parent(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => fs::create_dir_all(parent),
        _ => Ok(()),
    }
}

/// Resolve traffic path from env or the package default.
#[must_use]
pub fn path_from_env() -> PathBuf {
    resolve_traffic_path(env::var("INTERFIRE_TRAFFIC_MODE").ok().map(PathBuf::from))
}

#[must_use]
pub fn resolve_traffic_path(override_path: Option<PathBuf>) -> PathBuf {
    override_path.unwrap_or_else(|| PathBuf::from(DEFAULT_TRAFFIC_PATH))
}

#[cfg(test)]
mod tests {
    use super::{TrafficMode, load, resolve_traffic_path, store};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_traffic() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "interfire-traffic-{}-{}-{n}",
            std::process::id(),
            n
        ))
    }

    #[test]
    fn missing_file_is_open() {
        let path = temp_traffic();
        let _ = fs::remove_file(&path);
        assert_eq!(load(&path), TrafficMode::Open);
    }

    #[test]
    fn round_trip_blocked_and_open() {
        let path = temp_traffic();
        let _ = fs::remove_file(&path);
        store(&path, TrafficMode::Blocked).expect("store blocked");
        assert_eq!(load(&path), TrafficMode::Blocked);
        store(&path, TrafficMode::Open).expect("store open");
        assert_eq!(load(&path), TrafficMode::Open);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn invalid_contents_default_open() {
        let path = temp_traffic();
        fs::write(&path, "nope\n").expect("write");
        assert_eq!(load(&path), TrafficMode::Open);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn resolve_path_default_and_override() {
        assert_eq!(
            resolve_traffic_path(None),
            PathBuf::from(super::DEFAULT_TRAFFIC_PATH)
        );
        let custom = temp_traffic();
        assert_eq!(resolve_traffic_path(Some(custom.clone())), custom);
    }

    #[test]
    fn as_str_and_parse_cover_both_modes() {
        assert_eq!(TrafficMode::Open.as_str(), "open");
        assert_eq!(TrafficMode::Blocked.as_str(), "blocked");
        assert_eq!(TrafficMode::parse(" open "), Some(TrafficMode::Open));
        assert_eq!(TrafficMode::parse("blocked"), Some(TrafficMode::Blocked));
        assert_eq!(TrafficMode::parse("nope"), None);
    }

    #[test]
    fn unreadable_path_defaults_open() {
        let path = temp_traffic();
        fs::create_dir_all(&path).expect("dir");
        assert_eq!(load(&path), TrafficMode::Open);
        let _ = fs::remove_dir_all(&path);
    }

    #[test]
    fn store_creates_parent_and_path_from_env_default() {
        let path = temp_traffic().join("nested").join("traffic.mode");
        store(&path, TrafficMode::Blocked).expect("store nested");
        assert_eq!(load(&path), TrafficMode::Blocked);
        let _ = fs::remove_dir_all(path.parent().expect("parent").parent().expect("root"));
        assert_eq!(
            super::path_from_env(),
            PathBuf::from(super::DEFAULT_TRAFFIC_PATH)
        );
        assert!(super::ensure_parent(Path::new("traffic.mode")).is_ok());
    }
}
