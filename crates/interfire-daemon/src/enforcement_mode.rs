//! Persisted enforcement mode (`paused` | `active`).
#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tracing::{info, warn};

/// Default path under the daemon state directory.
pub const DEFAULT_MODE_PATH: &str = "/var/lib/interfire/enforcement.mode";

static STORE_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Operator-selected enforcement preference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnforcementMode {
    /// Owned nft table absent; new TCP is not queued.
    Paused,
    /// Owned nft table present; new TCP goes to NFQUEUE.
    Active,
}

impl EnforcementMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Paused => "paused",
            Self::Active => "active",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "paused" => Some(Self::Paused),
            "active" => Some(Self::Active),
            _ => None,
        }
    }
}

/// Load mode from disk. Missing or invalid file → [`EnforcementMode::Paused`].
#[must_use]
pub fn load(path: &Path) -> EnforcementMode {
    match fs::read_to_string(path) {
        Ok(raw) => EnforcementMode::parse(&raw).unwrap_or(EnforcementMode::Paused),
        Err(error) if error.kind() == io::ErrorKind::NotFound => EnforcementMode::Paused,
        Err(error) => {
            warn!(%error, path = %path.display(), "enforcement mode unreadable; default paused");
            EnforcementMode::Paused
        }
    }
}

/// Persist mode atomically under the parent directory.
///
/// # Errors
///
/// Returns I/O failures while creating the parent or writing the file.
pub fn store(path: &Path, mode: EnforcementMode) -> io::Result<()> {
    ensure_parent(path)?;
    // Unique sibling name so parallel tests do not race on the same `.tmp`.
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

/// Apply persisted mode to the owned nft table after the queue is ready.
pub fn apply_table(mode: EnforcementMode) {
    match mode {
        EnforcementMode::Paused => {
            if let Err(error) = crate::nft::remove() {
                warn!(%error, "paused boot: owned table remove skipped");
            } else {
                info!("enforcement mode paused; owned nft table absent");
            }
        }
        EnforcementMode::Active => match crate::nft::install() {
            Ok(()) => info!("enforcement mode active; owned nft table installed"),
            Err(error) => warn!(%error, "active boot: owned table install failed"),
        },
    }
}

/// Resolve mode path from env or the package default.
#[must_use]
pub fn path_from_env() -> PathBuf {
    resolve_mode_path(
        env::var("INTERFIRE_ENFORCEMENT_MODE")
            .ok()
            .map(PathBuf::from),
    )
}

#[must_use]
pub fn resolve_mode_path(override_path: Option<PathBuf>) -> PathBuf {
    override_path.unwrap_or_else(|| PathBuf::from(DEFAULT_MODE_PATH))
}

#[cfg(test)]
mod tests {
    use super::{EnforcementMode, ensure_parent, load, store};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_mode() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("interfire-mode-{}-{}-{n}", std::process::id(), n))
    }

    #[test]
    fn missing_file_is_paused() {
        let path = temp_mode();
        let _ = fs::remove_file(&path);
        assert_eq!(load(&path), EnforcementMode::Paused);
    }

    #[test]
    fn round_trip_active_and_paused() {
        let path = temp_mode();
        let _ = fs::remove_file(&path);
        store(&path, EnforcementMode::Active).expect("store active");
        assert_eq!(load(&path), EnforcementMode::Active);
        store(&path, EnforcementMode::Paused).expect("store paused");
        assert_eq!(load(&path), EnforcementMode::Paused);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn invalid_contents_default_paused() {
        let path = temp_mode();
        fs::write(&path, "nope\n").expect("write");
        assert_eq!(load(&path), EnforcementMode::Paused);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn unreadable_path_defaults_paused() {
        let path = temp_mode();
        let _ = fs::remove_file(&path);
        fs::create_dir_all(&path).expect("dir as mode path");
        assert_eq!(load(&path), EnforcementMode::Paused);
        let _ = fs::remove_dir_all(&path);
    }

    #[test]
    fn store_creates_missing_parent() {
        let path = temp_mode().join("nested").join("enforcement.mode");
        let _ = fs::remove_dir_all(path.parent().expect("parent").parent().expect("grand"));
        store(&path, EnforcementMode::Active).expect("store");
        assert_eq!(load(&path), EnforcementMode::Active);
        let _ = fs::remove_dir_all(path.parent().expect("parent").parent().expect("grand"));
    }

    #[test]
    fn ensure_parent_skips_empty_and_root() {
        ensure_parent(Path::new("enforcement.mode")).expect("relative");
        ensure_parent(Path::new("/")).expect("root");
    }

    #[test]
    fn apply_table_covers_active_and_paused_outcomes() {
        let _ok = crate::nft::ForceNftOk::arm();
        crate::enforcement_mode::apply_table(EnforcementMode::Paused);
        crate::enforcement_mode::apply_table(EnforcementMode::Active);
    }

    #[test]
    fn apply_table_logs_when_nft_rejects() {
        {
            let _reject = crate::nft::ForceNftRemoveReject::arm();
            crate::enforcement_mode::apply_table(EnforcementMode::Paused);
        }
        let _reject = crate::nft::ForceNftInstallReject::arm();
        crate::enforcement_mode::apply_table(EnforcementMode::Active);
    }

    #[test]
    fn path_from_env_default_and_override() {
        assert_eq!(
            crate::enforcement_mode::resolve_mode_path(None),
            PathBuf::from(super::DEFAULT_MODE_PATH)
        );
        let custom = temp_mode();
        assert_eq!(
            crate::enforcement_mode::resolve_mode_path(Some(custom.clone())),
            custom
        );
        // Smoke the env wrapper (no mutation): returns a path ending in mode file name.
        let from_env = crate::enforcement_mode::path_from_env();
        assert!(
            from_env.file_name().is_some_and(
                |name| name == "enforcement.mode" || name == custom.file_name().unwrap()
            )
        );
    }
}
