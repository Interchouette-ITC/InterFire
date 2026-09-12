//! Privileged daemon lifecycle and machine traffic via `pkexec`.
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

/// Machine-wide traffic block via `pkexec interfirectl` (root peer).
///
/// # Errors
///
/// Returns a short failure message when `pkexec` or `interfirectl` fails.
pub fn traffic_block_machine(socket: &str, direction: &str) -> Result<(), String> {
    let direction_arg = format!("--direction={direction}");
    run_interfirectl(
        socket,
        &[
            "traffic",
            "block",
            "--scope=machine",
            direction_arg.as_str(),
        ],
    )
}

/// Clear machine-wide traffic block via `pkexec interfirectl`.
///
/// # Errors
///
/// Returns a short failure message when `pkexec` or `interfirectl` fails.
pub fn traffic_unblock_machine(socket: &str) -> Result<(), String> {
    run_interfirectl(socket, &["traffic", "unblock", "--scope=machine"])
}

fn run_systemctl(action: &str) -> Result<(), String> {
    let output = Command::new("pkexec")
        .args(["systemctl", action, "interfired.service"])
        .output()
        .map_err(|error| format!("pkexec failed: {error}"))?;
    finish_output(&output, &format!("systemctl {action} interfired"))
}

fn run_interfirectl(socket: &str, args: &[&str]) -> Result<(), String> {
    let socket_arg = format!("--socket={socket}");
    let mut command = Command::new("pkexec");
    command.arg("interfirectl").arg(&socket_arg).args(args);
    let output = command
        .output()
        .map_err(|error| format!("pkexec failed: {error}"))?;
    finish_output(&output, "interfirectl traffic")
}

fn finish_output(output: &std::process::Output, label: &str) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = stderr.trim();
    if !detail.is_empty() {
        return Err(detail.to_owned());
    }
    let out = stdout.trim();
    if !out.is_empty() {
        return Err(out.to_owned());
    }
    Err(format!("{label} failed"))
}
