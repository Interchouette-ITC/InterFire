//! Desktop operator group authorization for the Unix IPC socket.
#![forbid(unsafe_code)]

use std::ffi::CString;
use std::fs;
use std::io;
use std::os::unix::fs::{PermissionsExt, chown};
use std::path::Path;

use nix::unistd::{Group, Uid, User, getgrouplist};
use tracing::{info, warn};

/// System group that may connect and mutate policy over IPC.
pub const OPERATOR_GROUP: &str = "interfire";

/// Result of looking up [`OPERATOR_GROUP`] for socket permission setup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GroupLookup {
    Present { gid: u32 },
    Missing,
    Failed,
}

/// After bind: open the socket (and parent dir) to [`OPERATOR_GROUP`] when present.
///
/// When the group is missing, keep owner-only `0600` and warn so desktop clients
/// fail honestly until packaging creates the group.
pub fn prepare_operator_socket(socket: &Path) -> io::Result<()> {
    let lookup = match Group::from_name(OPERATOR_GROUP) {
        Ok(Some(group)) => GroupLookup::Present {
            gid: group.gid.as_raw(),
        },
        Ok(None) => GroupLookup::Missing,
        Err(_) => GroupLookup::Failed,
    };
    prepare_operator_socket_with(socket, lookup)
}

fn prepare_operator_socket_with(socket: &Path, lookup: GroupLookup) -> io::Result<()> {
    match lookup {
        GroupLookup::Present { gid } => {
            if let Some(parent) = socket.parent() {
                if let Err(error) = chown(parent, None, Some(gid)) {
                    warn!(%error, path = %parent.display(), "failed to chown IPC directory");
                }
                if let Err(error) = fs::set_permissions(parent, fs::Permissions::from_mode(0o750)) {
                    warn!(%error, path = %parent.display(), "failed to chmod IPC directory");
                }
            }
            chown(socket, None, Some(gid))?;
            fs::set_permissions(socket, fs::Permissions::from_mode(0o660))?;
            info!(group = OPERATOR_GROUP, "IPC socket open to operator group");
            Ok(())
        }
        GroupLookup::Missing => {
            fs::set_permissions(socket, fs::Permissions::from_mode(0o600))?;
            warn!(
                group = OPERATOR_GROUP,
                "operator group missing; socket remains owner-only"
            );
            Ok(())
        }
        GroupLookup::Failed => {
            fs::set_permissions(socket, fs::Permissions::from_mode(0o600))?;
            warn!(
                group = OPERATOR_GROUP,
                "operator group lookup failed; socket remains owner-only"
            );
            Ok(())
        }
    }
}

/// True when `uid` may mutate policy (root, daemon UID, or operator group).
#[must_use]
pub fn uid_may_mutate(uid: Uid) -> bool {
    uid.is_root() || uid == Uid::current() || uid_in_operator_group(uid)
}

fn uid_in_operator_group(uid: Uid) -> bool {
    let Ok(Some(group)) = Group::from_name(OPERATOR_GROUP) else {
        return false;
    };
    let target = group.gid;
    let Ok(Some(user)) = User::from_uid(uid) else {
        return false;
    };
    if user.gid == target {
        return true;
    }
    let Ok(c_name) = CString::new(user.name.as_str()) else {
        return false;
    };
    getgrouplist(c_name.as_c_str(), user.gid).is_ok_and(|gids| gids.contains(&target))
}

#[cfg(test)]
mod tests {
    use super::{
        GroupLookup, OPERATOR_GROUP, prepare_operator_socket, prepare_operator_socket_with,
        uid_in_operator_group, uid_may_mutate,
    };
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use nix::unistd::{Uid, getgid};

    fn temp_socket() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("interfire-opauth-{n}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("tempdir");
        let sock = dir.join("interfired.sock");
        fs::File::create(&sock).expect("touch socket stand-in");
        sock
    }

    fn mode_of(path: &std::path::Path) -> u32 {
        fs::metadata(path).expect("meta").permissions().mode() & 0o777
    }

    fn cleanup(sock: &std::path::Path) {
        if let Some(parent) = sock.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn operator_group_name_is_interfire() {
        assert_eq!(OPERATOR_GROUP, "interfire");
    }

    #[test]
    fn current_uid_may_mutate() {
        assert!(uid_may_mutate(Uid::current()));
    }

    #[test]
    fn root_uid_may_mutate() {
        assert!(uid_may_mutate(Uid::from_raw(0)));
    }

    #[test]
    fn unknown_uid_without_passwd_cannot_mutate() {
        let stranger = Uid::from_raw(65_534);
        if stranger != Uid::current() && !stranger.is_root() {
            assert!(!uid_may_mutate(stranger));
        }
    }

    #[test]
    fn missing_group_keeps_owner_only_mode() {
        let sock = temp_socket();
        prepare_operator_socket_with(&sock, GroupLookup::Missing).expect("prepare");
        assert_eq!(mode_of(&sock), 0o600);
        cleanup(&sock);
    }

    #[test]
    fn failed_group_lookup_keeps_owner_only_mode() {
        let sock = temp_socket();
        prepare_operator_socket_with(&sock, GroupLookup::Failed).expect("prepare");
        assert_eq!(mode_of(&sock), 0o600);
        cleanup(&sock);
    }

    #[test]
    fn present_group_opens_socket_to_group() {
        let sock = temp_socket();
        let gid = getgid().as_raw();
        prepare_operator_socket_with(&sock, GroupLookup::Present { gid }).expect("prepare");
        assert_eq!(mode_of(&sock), 0o660);
        assert_eq!(mode_of(sock.parent().expect("parent")), 0o750);
        cleanup(&sock);
    }

    #[test]
    fn prepare_operator_socket_runs_against_host_group_table() {
        let sock = temp_socket();
        prepare_operator_socket(&sock).expect("prepare");
        let mode = mode_of(&sock);
        assert!(
            mode == 0o600 || mode == 0o660,
            "unexpected socket mode {mode:#o}"
        );
        cleanup(&sock);
    }

    #[test]
    fn uid_in_operator_group_rejects_unknown_uid() {
        assert!(!uid_in_operator_group(Uid::from_raw(65_533)));
    }
}
