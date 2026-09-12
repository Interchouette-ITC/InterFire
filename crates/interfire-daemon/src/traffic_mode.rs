//! Persisted traffic preferences: machine + per-uid (`open` | `out` | `in` | `all`).
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tracing::warn;

/// Default machine traffic path under the daemon state directory.
pub const DEFAULT_MACHINE_PATH: &str = "/var/lib/interfire/traffic.machine";
/// Legacy Phase C path (`blocked` → machine `out`).
pub const LEGACY_TRAFFIC_PATH: &str = "/var/lib/interfire/traffic.mode";

static STORE_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Kill-switch preference for one scope (machine or one UID).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrafficPreference {
    /// No kill-switch for this scope; Rules mode may own the table.
    Open,
    /// Drop new outbound TCP (except loopback).
    Out,
    /// Drop new inbound TCP (except loopback).
    In,
    /// Drop new inbound and outbound TCP (except loopback).
    All,
}

impl TrafficPreference {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Out => "out",
            Self::In => "in",
            Self::All => "all",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "open" => Some(Self::Open),
            "out" | "blocked" => Some(Self::Out),
            "in" => Some(Self::In),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    #[must_use]
    pub const fn is_blocked(self) -> bool {
        !matches!(self, Self::Open)
    }

    #[must_use]
    pub const fn wants_out(self) -> bool {
        matches!(self, Self::Out | Self::All)
    }

    #[must_use]
    pub const fn wants_in(self) -> bool {
        matches!(self, Self::In | Self::All)
    }
}

/// Effective table owner after machine-priority resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectiveTraffic {
    Open,
    Machine(TrafficPreference),
    User {
        uid: u32,
        preference: TrafficPreference,
    },
}

impl EffectiveTraffic {
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Open => "open".to_owned(),
            Self::Machine(pref) => format!("machine:{}", pref.as_str()),
            Self::User { uid, preference } => {
                format!("user:{uid}:{}", preference.as_str())
            }
        }
    }
}

/// Resolve effective filter: machine wins; user applies only when machine is open.
#[must_use]
pub const fn effective(
    machine: TrafficPreference,
    user: TrafficPreference,
    uid: u32,
) -> EffectiveTraffic {
    if machine.is_blocked() {
        return EffectiveTraffic::Machine(machine);
    }
    if user.is_blocked() {
        return EffectiveTraffic::User {
            uid,
            preference: user,
        };
    }
    EffectiveTraffic::Open
}

/// Load preference from disk. Missing or invalid → [`TrafficPreference::Open`].
#[must_use]
pub fn load(path: &Path) -> TrafficPreference {
    match fs::read_to_string(path) {
        Ok(raw) => TrafficPreference::parse(&raw).unwrap_or(TrafficPreference::Open),
        Err(error) if error.kind() == io::ErrorKind::NotFound => TrafficPreference::Open,
        Err(error) => {
            warn!(%error, path = %path.display(), "traffic preference unreadable; default open");
            TrafficPreference::Open
        }
    }
}

/// Persist preference atomically under the parent directory.
///
/// # Errors
///
/// Returns I/O failures while creating the parent or writing the file.
pub fn store(path: &Path, preference: TrafficPreference) -> io::Result<()> {
    ensure_parent(path)?;
    let seq = STORE_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("tmp.{seq}"));
    fs::write(&temporary, format!("{}\n", preference.as_str()))?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn ensure_parent(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => fs::create_dir_all(parent),
        _ => Ok(()),
    }
}

/// State directory containing `traffic.machine` and `traffic.user.*`.
#[must_use]
pub fn state_dir(machine_path: &Path) -> PathBuf {
    machine_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Path for one user's preference file.
#[must_use]
pub fn user_path(machine_path: &Path, uid: u32) -> PathBuf {
    state_dir(machine_path).join(format!("traffic.user.{uid}"))
}

/// Legacy Phase C path beside the machine file.
#[must_use]
pub fn legacy_path(machine_path: &Path) -> PathBuf {
    state_dir(machine_path).join(
        Path::new(LEGACY_TRAFFIC_PATH)
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("traffic.mode")),
    )
}

/// Migrate `traffic.mode=blocked` → `traffic.machine=out` once.
pub fn migrate_legacy(machine_path: &Path) {
    if machine_path.exists() {
        return;
    }
    let legacy = legacy_path(machine_path);
    if load(&legacy) == TrafficPreference::Out {
        if let Err(error) = store(machine_path, TrafficPreference::Out) {
            warn!(%error, "legacy traffic.mode migrate failed");
        }
    }
}

/// Load every non-open `traffic.user.<uid>` under the state directory.
#[must_use]
pub fn load_user_blocks(machine_path: &Path) -> Vec<(u32, TrafficPreference)> {
    let dir = state_dir(machine_path);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(uid_str) = name.strip_prefix("traffic.user.") else {
            continue;
        };
        let Ok(uid) = uid_str.parse::<u32>() else {
            continue;
        };
        let preference = load(&entry.path());
        if preference.is_blocked() {
            out.push((uid, preference));
        }
    }
    out.sort_by_key(|(uid, _)| *uid);
    out
}

/// True when machine or any user kill-switch is active (`ExecStop` preserve).
#[must_use]
pub fn any_block_active(machine_path: &Path) -> bool {
    load(machine_path).is_blocked() || !load_user_blocks(machine_path).is_empty()
}

/// Resolve machine path from env or the package default.
#[must_use]
pub fn path_from_env() -> PathBuf {
    resolve_machine_path(
        env::var("INTERFIRE_TRAFFIC_MACHINE")
            .ok()
            .map(PathBuf::from),
    )
}

#[must_use]
pub fn resolve_machine_path(override_path: Option<PathBuf>) -> PathBuf {
    override_path.unwrap_or_else(|| PathBuf::from(DEFAULT_MACHINE_PATH))
}

#[cfg(test)]
mod tests {
    use super::{
        TrafficPreference, any_block_active, effective, load, load_user_blocks, migrate_legacy,
        resolve_machine_path, store, user_path,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "interfire-traffic-dir-{}-{}-{n}",
            std::process::id(),
            n
        ));
        let _ = fs::create_dir_all(&path);
        path
    }

    #[test]
    fn missing_file_is_open() {
        let path = temp_dir().join("traffic.machine");
        let _ = fs::remove_file(&path);
        assert_eq!(load(&path), TrafficPreference::Open);
        let _ = fs::remove_dir_all(path.parent().expect("parent"));
    }

    #[test]
    fn round_trip_preferences() {
        let path = temp_dir().join("traffic.machine");
        store(&path, TrafficPreference::All).expect("store");
        assert_eq!(load(&path), TrafficPreference::All);
        store(&path, TrafficPreference::Open).expect("store open");
        assert_eq!(load(&path), TrafficPreference::Open);
        let _ = fs::remove_dir_all(path.parent().expect("parent"));
    }

    #[test]
    fn blocked_alias_is_out() {
        assert_eq!(
            TrafficPreference::parse("blocked"),
            Some(TrafficPreference::Out)
        );
    }

    #[test]
    fn effective_machine_priority() {
        assert_eq!(
            effective(TrafficPreference::Open, TrafficPreference::Open, 1000),
            super::EffectiveTraffic::Open
        );
        assert!(matches!(
            effective(TrafficPreference::Open, TrafficPreference::Out, 1000),
            super::EffectiveTraffic::User {
                uid: 1000,
                preference: TrafficPreference::Out
            }
        ));
        assert!(matches!(
            effective(TrafficPreference::All, TrafficPreference::Out, 1000),
            super::EffectiveTraffic::Machine(TrafficPreference::All)
        ));
    }

    #[test]
    fn migrate_legacy_blocked_to_machine_out() {
        let dir = temp_dir();
        let machine = dir.join("traffic.machine");
        let legacy = dir.join("traffic.mode");
        fs::write(&legacy, "blocked\n").expect("legacy");
        migrate_legacy(&machine);
        assert_eq!(load(&machine), TrafficPreference::Out);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_user_blocks_and_any_active() {
        let dir = temp_dir();
        let machine = dir.join("traffic.machine");
        store(&machine, TrafficPreference::Open).expect("machine");
        store(&user_path(&machine, 42), TrafficPreference::In).expect("user");
        assert_eq!(
            load_user_blocks(&machine),
            vec![(42, TrafficPreference::In)]
        );
        assert!(any_block_active(&machine));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_path_default_and_override() {
        assert_eq!(
            resolve_machine_path(None),
            PathBuf::from(super::DEFAULT_MACHINE_PATH)
        );
        let custom = temp_dir().join("traffic.machine");
        assert_eq!(resolve_machine_path(Some(custom.clone())), custom);
        let _ = fs::remove_dir_all(custom.parent().expect("parent"));
    }

    #[test]
    fn as_str_wants_and_path_from_env() {
        assert_eq!(TrafficPreference::Out.as_str(), "out");
        assert!(TrafficPreference::All.wants_out());
        assert!(TrafficPreference::All.wants_in());
        assert!(!TrafficPreference::Out.wants_in());
        assert_eq!(
            super::path_from_env(),
            PathBuf::from(super::DEFAULT_MACHINE_PATH)
        );
        assert!(super::ensure_parent(Path::new("traffic.machine")).is_ok());
    }
}
