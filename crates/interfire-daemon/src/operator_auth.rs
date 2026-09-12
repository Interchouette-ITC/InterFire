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

/// After bind: open the socket (and parent dir) to [`OPERATOR_GROUP`] when present.
///
/// When the group is missing, keep owner-only `0600` and warn so desktop clients
/// fail honestly until packaging creates the group.
pub fn prepare_operator_socket(socket: &Path) -> io::Result<()> {
    match Group::from_name(OPERATOR_GROUP) {
        Ok(Some(group)) => {
            let gid = group.gid.as_raw();
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
        Ok(None) => {
            fs::set_permissions(socket, fs::Permissions::from_mode(0o600))?;
            warn!(
                group = OPERATOR_GROUP,
                "operator group missing; socket remains owner-only"
            );
            Ok(())
        }
        Err(error) => {
            fs::set_permissions(socket, fs::Permissions::from_mode(0o600))?;
            warn!(
                %error,
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
    use super::{OPERATOR_GROUP, uid_may_mutate};
    use nix::unistd::Uid;

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
}
