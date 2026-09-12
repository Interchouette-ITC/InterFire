//! Privileged daemon lifecycle via `pkexec` + `systemctl`.
#![forbid(unsafe_code)]

use std::process::Command;

/// Stop the `interfired` systemd unit (polkit prompt).
///
/// # Errors
///
/// Returns a short failure message when `pkexec` or `systemctl` fails.
pub fn stop_daemon() -> Result<(), String> {
    run_systemctl("stop")
}

/// Start the `interfired` systemd unit (polkit prompt).
///
/// # Errors
///
/// Returns a short failure message when `pkexec` or `systemctl` fails.
pub fn start_daemon() -> Result<(), String> {
    run_systemctl("start")
}

fn run_systemctl(action: &str) -> Result<(), String> {
    let output = Command::new("pkexec")
        .args(["systemctl", action, "interfired.service"])
        .output()
        .map_err(|error| format!("pkexec failed: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    if detail.is_empty() {
        Err(format!("systemctl {action} interfired failed"))
    } else {
        Err(detail.to_owned())
    }
}
