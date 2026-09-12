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

/// Result of looking up an operator group for socket permission setup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GroupLookup {
    Present { gid: u32 },
    Missing,
}

/// After bind: open the socket (and parent dir) to [`OPERATOR_GROUP`] when present.
///
/// When the group is missing, keep owner-only `0600` and warn so desktop clients
/// fail honestly until packaging creates the group.
pub fn prepare_operator_socket(socket: &Path) -> io::Result<()> {
    prepare_operator_socket_with(socket, resolve_group(OPERATOR_GROUP))
}

fn resolve_group(name: &str) -> GroupLookup {
    match Group::from_name(name) {
        Ok(Some(group)) => GroupLookup::Present {
            gid: group.gid.as_raw(),
        },
        Ok(None) | Err(_) => GroupLookup::Missing,
    }
}

fn prepare_operator_socket_with(socket: &Path, lookup: GroupLookup) -> io::Result<()> {
    match lookup {
        GroupLookup::Present { gid } => {
            if let Some(parent) = socket.parent().filter(|p| !p.as_os_str().is_empty()) {
                let _ = chown(parent, None, Some(gid));
                let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o750));
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
    }
}

/// True when `uid` may mutate policy (root, daemon UID, or operator group).
#[must_use]
pub fn uid_may_mutate(uid: Uid) -> bool {
    uid.is_root() || uid == Uid::current() || uid_in_named_group(uid, OPERATOR_GROUP)
}

fn uid_in_named_group(uid: Uid, group_name: &str) -> bool {
    let Ok(Some(group)) = Group::from_name(group_name) else {
        return false;
    };
    let target = group.gid;
    let Ok(Some(user)) = User::from_uid(uid) else {
        return false;
    };
    user_in_gid(&user, target)
}

fn user_in_gid(user: &User, target: nix::unistd::Gid) -> bool {
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
        resolve_group, uid_in_named_group, uid_may_mutate, user_in_gid,
    };
    use std::ffi::CString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use nix::unistd::{Gid, Uid, User, getgid};

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

    fn mode_of(path: &Path) -> u32 {
        fs::metadata(path).expect("meta").permissions().mode() & 0o777
    }

    fn cleanup(sock: &Path) {
        if let Some(parent) = sock.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn operator_group_name_is_interfire() {
        assert_eq!(OPERATOR_GROUP, "interfire");
    }

    #[test]
    fn current_and_root_may_mutate() {
        assert!(uid_may_mutate(Uid::current()));
        assert!(uid_may_mutate(Uid::from_raw(0)));
    }

    #[test]
    fn unknown_uid_without_passwd_cannot_mutate() {
        assert!(!uid_may_mutate(Uid::from_raw(65_534)));
    }

    #[test]
    fn resolve_root_and_missing_groups() {
        assert!(matches!(resolve_group("root"), GroupLookup::Present { .. }));
        assert_eq!(
            resolve_group("interfire-no-such-group-for-tests"),
            GroupLookup::Missing
        );
        // Nul in the name is treated as missing by nix / this resolver.
        assert_eq!(resolve_group("bad\0name"), GroupLookup::Missing);
    }

    #[test]
    fn missing_group_keeps_owner_only_mode() {
        let sock = temp_socket();
        prepare_operator_socket_with(&sock, GroupLookup::Missing).expect("prepare");
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
    fn present_group_without_parent_dir_still_modes_socket() {
        let cwd = std::env::temp_dir();
        let prev = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&cwd).expect("cd temp");
        let name = format!("interfire-sock-{}", std::process::id());
        let sock = PathBuf::from(&name);
        let _ = fs::remove_file(&sock);
        fs::File::create(&sock).expect("touch");
        let gid = getgid().as_raw();
        let result = prepare_operator_socket_with(&sock, GroupLookup::Present { gid });
        let mode = mode_of(&sock);
        let _ = fs::remove_file(&sock);
        std::env::set_current_dir(prev).expect("restore cwd");
        result.expect("prepare");
        assert_eq!(mode, 0o660);
    }

    #[test]
    fn prepare_operator_socket_runs_against_host_group_table() {
        let sock = temp_socket();
        prepare_operator_socket(&sock).expect("prepare");
        let mode = mode_of(&sock);
        assert!(mode == 0o600 || mode == 0o660, "mode {mode:#o}");
        cleanup(&sock);
    }

    #[test]
    fn root_uid_is_in_root_group() {
        assert!(uid_in_named_group(Uid::from_raw(0), "root"));
    }

    #[test]
    fn unknown_uid_is_not_in_root_group() {
        assert!(!uid_in_named_group(Uid::from_raw(65_533), "root"));
    }

    #[test]
    fn user_in_gid_hits_primary_and_supplementary() {
        let user = User::from_uid(Uid::current())
            .expect("user")
            .expect("passwd");
        assert!(user_in_gid(&user, user.gid));
        let other = Gid::from_raw(user.gid.as_raw().wrapping_add(9_001));
        // Exercises getgrouplist path when primary gid does not match.
        let _ = user_in_gid(&user, other);
    }

    #[test]
    fn cstring_rejects_interior_nul_like_user_in_gid_guard() {
        assert!(CString::new("bad\0name").is_err());
    }
}
