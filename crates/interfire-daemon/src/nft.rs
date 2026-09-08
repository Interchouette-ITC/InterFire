//! InterFire-owned nftables table query and install/remove.
//!
//! Operates only on `inet interfire`. Never lists or mutates other tables.
#![forbid(unsafe_code)]

use std::io;
#[cfg(not(test))]
use std::io::Write;
use std::process::{Command, Stdio};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

use interfire_proto::{
    NFQUEUE_NUM, NFT_CHAIN, NFT_TABLE, NetworkStatusBody, NetworkTableState, network_rule_token,
};
use tracing::{info, warn};

#[cfg(test)]
static FORCE_NFT_OK: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static FORCE_NFT_LIST_MISSING: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static FORCE_NFT_LIST_FAIL: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static FORCE_NFT_LIST_OUTPUT: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static FORCE_NFT_SPAWN_FAIL: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static FORCE_NFT_STDIN_UNAVAILABLE: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static FORCE_NFT_INSTALL_SUCCESS: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static FORCE_NFT_INSTALL_REJECT: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static FORCE_NFT_REMOVE_REJECT: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static NFT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
fn nft_test_lock() -> std::sync::MutexGuard<'static, ()> {
    NFT_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Force install/remove success without calling `nft` (unit tests only).
#[cfg(test)]
pub struct ForceNftOk {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl ForceNftOk {
    #[must_use]
    pub fn arm() -> Self {
        let guard = nft_test_lock();
        FORCE_NFT_OK.store(true, Ordering::Relaxed);
        Self { _guard: guard }
    }
}

#[cfg(test)]
impl Drop for ForceNftOk {
    fn drop(&mut self) {
        FORCE_NFT_OK.store(false, Ordering::Relaxed);
    }
}

/// Force `nft list` missing-table handling in [`status`] (unit tests only).
#[cfg(test)]
pub struct ForceNftListMissing {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl ForceNftListMissing {
    #[must_use]
    pub fn arm() -> Self {
        let guard = nft_test_lock();
        FORCE_NFT_LIST_MISSING.store(true, Ordering::Relaxed);
        Self { _guard: guard }
    }
}

#[cfg(test)]
impl Drop for ForceNftListMissing {
    fn drop(&mut self) {
        FORCE_NFT_LIST_MISSING.store(false, Ordering::Relaxed);
    }
}

/// Force `nft list` failure in [`status`] (unit tests only).
#[cfg(test)]
pub struct ForceNftListFail {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl ForceNftListFail {
    #[must_use]
    pub fn arm() -> Self {
        let guard = nft_test_lock();
        FORCE_NFT_LIST_FAIL.store(true, Ordering::Relaxed);
        Self { _guard: guard }
    }
}

#[cfg(test)]
impl Drop for ForceNftListFail {
    fn drop(&mut self) {
        FORCE_NFT_LIST_FAIL.store(false, Ordering::Relaxed);
    }
}

/// Force successful `nft list` output for [`status`] (unit tests only).
#[cfg(test)]
pub struct ForceNftListOutput {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl ForceNftListOutput {
    #[must_use]
    pub fn arm() -> Self {
        let guard = nft_test_lock();
        FORCE_NFT_LIST_OUTPUT.store(true, Ordering::Relaxed);
        Self { _guard: guard }
    }
}

#[cfg(test)]
impl Drop for ForceNftListOutput {
    fn drop(&mut self) {
        FORCE_NFT_LIST_OUTPUT.store(false, Ordering::Relaxed);
    }
}

/// Force `nft` spawn failure for mutating commands (unit tests only).
#[cfg(test)]
pub struct ForceNftSpawnFail {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl ForceNftSpawnFail {
    #[must_use]
    pub fn arm() -> Self {
        let guard = nft_test_lock();
        FORCE_NFT_SPAWN_FAIL.store(true, Ordering::Relaxed);
        Self { _guard: guard }
    }
}

#[cfg(test)]
impl Drop for ForceNftSpawnFail {
    fn drop(&mut self) {
        FORCE_NFT_SPAWN_FAIL.store(false, Ordering::Relaxed);
    }
}

/// Force install stdin-unavailable path (unit tests only).
#[cfg(test)]
pub struct ForceNftStdinUnavailable {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl ForceNftStdinUnavailable {
    #[must_use]
    pub fn arm() -> Self {
        let guard = nft_test_lock();
        FORCE_NFT_STDIN_UNAVAILABLE.store(true, Ordering::Relaxed);
        Self { _guard: guard }
    }
}

#[cfg(test)]
impl Drop for ForceNftStdinUnavailable {
    fn drop(&mut self) {
        FORCE_NFT_STDIN_UNAVAILABLE.store(false, Ordering::Relaxed);
    }
}

/// Force install success without calling `nft` (unit tests only).
#[cfg(test)]
pub struct ForceNftInstallSuccess {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl ForceNftInstallSuccess {
    #[must_use]
    pub fn arm() -> Self {
        let guard = nft_test_lock();
        FORCE_NFT_INSTALL_SUCCESS.store(true, Ordering::Relaxed);
        Self { _guard: guard }
    }
}

#[cfg(test)]
impl Drop for ForceNftInstallSuccess {
    fn drop(&mut self) {
        FORCE_NFT_INSTALL_SUCCESS.store(false, Ordering::Relaxed);
    }
}

/// Force install rejection after spawn (unit tests only).
#[cfg(test)]
pub struct ForceNftInstallReject {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl ForceNftInstallReject {
    #[must_use]
    pub fn arm() -> Self {
        let guard = nft_test_lock();
        FORCE_NFT_INSTALL_REJECT.store(true, Ordering::Relaxed);
        Self { _guard: guard }
    }
}

#[cfg(test)]
impl Drop for ForceNftInstallReject {
    fn drop(&mut self) {
        FORCE_NFT_INSTALL_REJECT.store(false, Ordering::Relaxed);
    }
}

/// Force remove rejection when the table is present (unit tests only).
#[cfg(test)]
pub struct ForceNftRemoveReject {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl ForceNftRemoveReject {
    #[must_use]
    pub fn arm() -> Self {
        let guard = nft_test_lock();
        FORCE_NFT_REMOVE_REJECT.store(true, Ordering::Relaxed);
        Self { _guard: guard }
    }
}

#[cfg(test)]
impl Drop for ForceNftRemoveReject {
    fn drop(&mut self) {
        FORCE_NFT_REMOVE_REJECT.store(false, Ordering::Relaxed);
    }
}

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
    #[cfg(test)]
    if FORCE_NFT_OK.load(Ordering::Relaxed) || FORCE_NFT_INSTALL_SUCCESS.load(Ordering::Relaxed) {
        info!(
            table = NFT_TABLE,
            queue = NFQUEUE_NUM,
            "InterFire nftables table installed"
        );
        return Ok(());
    }
    #[cfg(test)]
    if FORCE_NFT_SPAWN_FAIL.load(Ordering::Relaxed) {
        return Err(io::Error::other("nft spawn failed: forced"));
    }
    #[cfg(test)]
    if FORCE_NFT_INSTALL_REJECT.load(Ordering::Relaxed) {
        return Err(io::Error::other("nft install failed: forced reject"));
    }
    #[cfg(test)]
    if FORCE_NFT_STDIN_UNAVAILABLE.load(Ordering::Relaxed) {
        let mut child = Command::new("true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| io::Error::other(format!("nft spawn failed: {error}")))?;
        let Some(_stdin) = child.stdin.as_mut() else {
            return Err(io::Error::other("nft stdin unavailable"));
        };
        return Err(io::Error::other(
            "nft stdin unavailable: unexpected piped stdin under force flag",
        ));
    }
    #[cfg(test)]
    {
        Err(io::Error::other(
            "live nft install is unavailable under unit tests",
        ))
    }
    #[cfg(not(test))]
    {
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
        finish_install(&output)
    }
}

fn finish_install(output: &std::process::Output) -> io::Result<()> {
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
    #[cfg(test)]
    if FORCE_NFT_OK.load(Ordering::Relaxed) {
        info!(table = NFT_TABLE, "InterFire nftables table removed");
        return Ok(());
    }
    #[cfg(test)]
    if FORCE_NFT_SPAWN_FAIL.load(Ordering::Relaxed) {
        return Err(io::Error::other("nft spawn failed: forced"));
    }
    #[cfg(test)]
    let output = if FORCE_NFT_REMOVE_REJECT.load(Ordering::Relaxed) {
        Command::new("false")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| io::Error::other(format!("nft spawn failed: {error}")))?
    } else {
        return Err(io::Error::other(
            "live nft remove is unavailable under unit tests",
        ));
    };
    #[cfg(not(test))]
    let output = Command::new("nft")
        .args(["delete", "table", "inet", NFT_TABLE])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| io::Error::other(format!("nft spawn failed: {error}")))?;
    finish_remove(&output)
}

fn finish_remove(output: &std::process::Output) -> io::Result<()> {
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

#[derive(Debug)]
enum ListError {
    Missing,
    Failed(String),
}

fn list_owned_table() -> Result<String, ListError> {
    #[cfg(test)]
    if FORCE_NFT_LIST_MISSING.load(Ordering::Relaxed) {
        return Err(ListError::Missing);
    }
    #[cfg(test)]
    if FORCE_NFT_LIST_FAIL.load(Ordering::Relaxed) {
        return Err(ListError::Failed("forced list failure".into()));
    }
    #[cfg(test)]
    if FORCE_NFT_LIST_OUTPUT.load(Ordering::Relaxed) {
        return Ok(
            "table inet interfire { chain output { meta l4proto tcp ct state new queue num 4242 } }"
                .into(),
        );
    }
    #[cfg(test)]
    if FORCE_NFT_SPAWN_FAIL.load(Ordering::Relaxed) {
        return Err(ListError::Failed("nft spawn failed: forced".into()));
    }
    #[cfg(not(test))]
    {
        let output = Command::new("nft")
            .args(["list", "table", "inet", NFT_TABLE])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| ListError::Failed(format!("nft spawn failed: {error}")))?;
        interpret_list_output(&output)
    }
    #[cfg(test)]
    Err(ListError::Missing)
}

fn interpret_list_output(output: &std::process::Output) -> Result<String, ListError> {
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

    #[test]
    fn owned_script_matches_packaged_nft_file() {
        let packaged = include_str!("../../../packaging/nft/interfire.nft");
        assert_eq!(
            normalize_nft_tokens(&owned_table_script()),
            normalize_nft_tokens(packaged),
            "daemon owned_table_script() must stay in sync with packaging/nft/interfire.nft"
        );
    }

    #[test]
    fn status_reports_missing_when_table_is_absent() {
        let _missing = ForceNftListMissing::arm();
        let status = status();
        assert_eq!(status.state, NetworkTableState::Missing);
        assert_eq!(status.rule, "none");
    }

    #[test]
    fn interpret_list_output_handles_success_missing_and_failure() {
        let success = Command::new("sh")
            .args(["-c", "printf 'table inet interfire {}'"])
            .output()
            .expect("sh");
        let stdout = interpret_list_output(&success).expect("list success");
        assert!(stdout.contains("table inet interfire"));

        let missing = Command::new("sh")
            .args(["-c", "echo 'Error: No such file or directory' >&2; exit 1"])
            .output()
            .expect("sh");
        assert!(matches!(
            interpret_list_output(&missing),
            Err(ListError::Missing)
        ));

        let failed = Command::new("sh")
            .args(["-c", "echo permission denied >&2; exit 1"])
            .output()
            .expect("sh");
        assert!(matches!(
            interpret_list_output(&failed),
            Err(ListError::Failed(_))
        ));
    }

    #[test]
    fn finish_install_and_remove_cover_success_and_missing() {
        let install_ok = Command::new("true").output().expect("true");
        finish_install(&install_ok).expect("install ok");

        let install_fail = Command::new("false").output().expect("false");
        assert!(finish_install(&install_fail).is_err());

        let remove_ok = Command::new("true").output().expect("true");
        finish_remove(&remove_ok).expect("remove ok");

        let remove_missing = Command::new("sh")
            .args(["-c", "echo 'does not exist' >&2; exit 1"])
            .output()
            .expect("sh");
        finish_remove(&remove_missing).expect("remove missing ok");
    }

    #[test]
    fn install_success_path_is_ok() {
        let _success = ForceNftInstallSuccess::arm();
        install().expect("forced install success");
    }

    #[test]
    fn status_reports_missing_when_list_fails() {
        let _fail = ForceNftListFail::arm();
        let status = status();
        assert_eq!(status.state, NetworkTableState::Missing);
        assert_eq!(status.rule, "none");
    }

    #[test]
    fn status_reports_installed_from_list_output() {
        let _output = ForceNftListOutput::arm();
        let status = status();
        assert_eq!(status.state, NetworkTableState::Installed);
        assert_eq!(status.rule, "tcp_new_queue_4242");
    }

    #[test]
    fn install_spawn_failure_is_io_error() {
        let _fail = ForceNftSpawnFail::arm();
        let error = install().expect_err("spawn fail");
        assert!(error.to_string().contains("spawn failed"));
    }

    #[test]
    fn install_stdin_unavailable_is_io_error() {
        let _stdin = ForceNftStdinUnavailable::arm();
        let error = install().expect_err("stdin unavailable");
        assert!(error.to_string().contains("stdin unavailable"));
    }

    #[test]
    fn install_reject_is_io_error() {
        let _reject = ForceNftInstallReject::arm();
        let error = install().expect_err("install reject");
        assert!(error.to_string().contains("install failed"));
    }

    #[test]
    fn remove_spawn_failure_is_io_error() {
        let _fail = ForceNftSpawnFail::arm();
        let error = remove().expect_err("spawn fail");
        assert!(error.to_string().contains("spawn failed"));
    }

    #[test]
    fn remove_reject_is_io_error() {
        let _reject = ForceNftRemoveReject::arm();
        let error = remove().expect_err("remove reject");
        assert!(error.to_string().contains("remove failed"));
    }

    #[test]
    fn force_ok_short_circuits_install_and_remove() {
        let _ok = ForceNftOk::arm();
        install().expect("forced install ok");
        remove().expect("forced remove ok");
    }

    #[test]
    fn stderr_indicates_missing_matches_common_messages() {
        assert!(stderr_indicates_missing("Error: No such file or directory"));
        assert!(stderr_indicates_missing("table does not exist"));
        assert!(stderr_indicates_missing("object not found"));
        assert!(!stderr_indicates_missing("permission denied"));
    }

    fn normalize_nft_tokens(source: &str) -> String {
        source
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .flat_map(|line| line.split_whitespace())
            .collect::<Vec<_>>()
            .join(" ")
    }
}
