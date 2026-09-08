//! InterFire-owned nftables table query and install/remove.
//!
//! Operates only on `inet interfire`. Never lists or mutates other tables.
#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::process::{Command, Stdio};

use interfire_proto::{
    NFQUEUE_NUM, NFT_CHAIN, NFT_TABLE, NetworkStatusBody, NetworkTableState, network_rule_token,
};
use tracing::{info, warn};

/// Query the InterFire-owned table via `nft list table`.
#[must_use]
pub fn status() -> NetworkStatusBody {
    match list_owned_table() {
        Ok(stdout) => classify_list_output(&stdout),
        Err(ListError::Missing) => body(NetworkTableState::Missing, "none"),
        Err(ListError::Failed(message)) => {
            warn!(%message, "nft list failed; treating owned table as missing");
            body(NetworkTableState::Missing, "none")
        }
    }
}

/// Install or replace the InterFire-owned outbound TCP queue rule.
///
/// # Errors
///
/// Returns I/O errors when `nft` is missing or rejects the fixed table script.
pub fn install() -> io::Result<()> {
    let _ = remove();
    let script = owned_table_script();
    let mut child = Command::new("nft")
        .arg("-f")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| io::Error::other(format!("nft spawn failed: {error}")))?;
    {
        let Some(stdin) = child.stdin.as_mut() else {
            return Err(io::Error::other("nft stdin unavailable"));
        };
        stdin.write_all(script.as_bytes())?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| io::Error::other(format!("nft wait failed: {error}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "nft install failed: {}",
            stderr.trim()
        )));
    }
    info!(
        table = NFT_TABLE,
        queue = NFQUEUE_NUM,
        "InterFire nftables table installed"
    );
    Ok(())
}

/// Delete the InterFire-owned table. Idempotent when the table is already absent.
///
/// # Errors
///
/// Returns I/O errors when `nft` is missing or delete fails for a reason other
/// than the table already being gone.
pub fn remove() -> io::Result<()> {
    let output = Command::new("nft")
        .args(["delete", "table", "inet", NFT_TABLE])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| io::Error::other(format!("nft spawn failed: {error}")))?;
    if output.status.success() {
        info!(table = NFT_TABLE, "InterFire nftables table removed");
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr_indicates_missing(&stderr) {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "nft remove failed: {}",
        stderr.trim()
    )))
}

fn owned_table_script() -> String {
    format!(
        "table inet {NFT_TABLE} {{\n\
  chain {NFT_CHAIN} {{\n\
    type filter hook output priority filter; policy accept;\n\
    meta l4proto tcp ct state new queue num {NFQUEUE_NUM}\n\
  }}\n\
}}\n"
    )
}

const fn body(state: NetworkTableState, rule: &'static str) -> NetworkStatusBody {
    NetworkStatusBody {
        table: NFT_TABLE,
        queue: NFQUEUE_NUM,
        state,
        rule,
    }
}

enum ListError {
    Missing,
    Failed(String),
}

fn list_owned_table() -> Result<String, ListError> {
    let output = Command::new("nft")
        .args(["list", "table", "inet", NFT_TABLE])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| ListError::Failed(format!("nft spawn failed: {error}")))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr_indicates_missing(&stderr) {
        return Err(ListError::Missing);
    }
    Err(ListError::Failed(stderr.trim().to_owned()))
}

fn stderr_indicates_missing(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    lower.contains("no such file")
        || lower.contains("does not exist")
        || lower.contains("not found")
}

/// Classify `nft list table inet interfire` stdout without touching other tables.
#[must_use]
pub fn classify_list_output(stdout: &str) -> NetworkStatusBody {
    let flattened: String = stdout
        .chars()
        .map(|ch| if ch.is_whitespace() { ' ' } else { ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let has_table = flattened.contains(&format!("table inet {NFT_TABLE}"));
    if !has_table {
        return body(NetworkTableState::Missing, "none");
    }
    let queue_needle = format!("queue num {NFQUEUE_NUM}");
    let has_queue = flattened.contains(&queue_needle);
    let has_tcp_new = flattened.contains("meta l4proto tcp")
        && (flattened.contains("ct state new") || flattened.contains("ct state"));
    if has_queue && has_tcp_new {
        body(
            NetworkTableState::Installed,
            network_rule_token(NFQUEUE_NUM),
        )
    } else {
        body(NetworkTableState::Incomplete, "none")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_missing_when_empty() {
        let status = classify_list_output("");
        assert_eq!(status.state, NetworkTableState::Missing);
        assert_eq!(status.rule, "none");
    }

    #[test]
    fn classifies_installed_owned_rule() {
        let stdout = r"
table inet interfire {
	chain output {
		type filter hook output priority filter; policy accept;
		meta l4proto tcp ct state new queue num 4242
	}
}
";
        let status = classify_list_output(stdout);
        assert_eq!(status.state, NetworkTableState::Installed);
        assert_eq!(status.table, NFT_TABLE);
        assert_eq!(status.queue, NFQUEUE_NUM);
        assert_eq!(status.rule, "tcp_new_queue_4242");
    }

    #[test]
    fn classifies_incomplete_without_queue() {
        let stdout = r"
table inet interfire {
	chain output {
		type filter hook output priority filter; policy accept;
	}
}
";
        let status = classify_list_output(stdout);
        assert_eq!(status.state, NetworkTableState::Incomplete);
        assert_eq!(status.rule, "none");
    }

    #[test]
    fn owned_script_targets_only_interfire_table() {
        let script = owned_table_script();
        assert!(script.contains("table inet interfire"));
        assert!(script.contains("queue num 4242"));
        assert!(script.contains("meta l4proto tcp ct state new"));
        assert!(!script.contains("firewalld"));
        assert!(!script.contains("ufw"));
    }
}
