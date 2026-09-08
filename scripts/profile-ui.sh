#!/usr/bin/env bash
# Profile `interfire-ui` allocations with hotpath-rs (optional features).
# Needs DISPLAY or xvfb-run. Default builds stay cold; this target enables
# `hotpath` + `hotpath-alloc` only for the profile binary.
# See https://hotpath.rs/ and docs-dev/ui-gpui.md.
set -euo pipefail

PATH="/usr/sbin:/usr/bin:/sbin:/bin:${PATH}"
readonly script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly repo_root="$(cd -- "${script_dir}/.." && pwd)"

readonly shutdown_ms="${HOTPATH_SHUTDOWN_MS:-8000}"

daemon_bin="${INTERFIRED_BIN:-${repo_root}/target/release/interfired}"
ui_bin="${INTERFIRE_UI_BIN:-${repo_root}/target/release/interfire-ui}"

if [[ ! -x "${daemon_bin}" ]]; then
  printf '%s\n' "FAIL  missing ${daemon_bin}; build release daemon first" >&2
  exit 1
fi
if [[ ! -x "${ui_bin}" ]]; then
  printf '%s\n' "FAIL  missing ${ui_bin}; build with hotpath features first" >&2
  exit 1
fi

work_dir="$(mktemp -d)"
socket="${work_dir}/interfired.sock"
rules_path="${work_dir}/rules.toml"
audit_path="${work_dir}/audit.log"
daemon_pid=""

cleanup() {
  local result=$?
  [[ -n "${daemon_pid}" ]] && kill "${daemon_pid}" 2>/dev/null || true
  [[ -n "${daemon_pid}" ]] && wait "${daemon_pid}" 2>/dev/null || true
  rm -rf "${work_dir}"
  exit "${result}"
}
trap cleanup EXIT

cat >"${rules_path}" <<'EOF'
schema_version = 1
EOF

RUST_LOG=error "${daemon_bin}" \
  --socket="${socket}" \
  --rules="${rules_path}" \
  --audit="${audit_path}" \
  --no-ebpf \
  --no-nfqueue \
  >"${work_dir}/daemon.log" 2>&1 &
daemon_pid=$!

for _ in $(seq 1 100); do
  if [[ -S "${socket}" ]]; then
    break
  fi
  if ! kill -0 "${daemon_pid}" 2>/dev/null; then
    printf '%s\n' "FAIL  daemon exited before listening" >&2
    sed 's/^/      /' "${work_dir}/daemon.log" >&2 || true
    exit 1
  fi
  sleep 0.02
done

if [[ ! -S "${socket}" ]]; then
  printf '%s\n' "FAIL  daemon socket never appeared" >&2
  exit 1
fi

export LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE:-1}"
export WGPU_BACKEND="${WGPU_BACKEND:-gl}"
export HOTPATH_SHUTDOWN_MS="${shutdown_ms}"

ui_cmd=("${ui_bin}" "--socket=${socket}" "--rss-probe=idle")

printf '%s\n' "profiling interfire-ui (hotpath-alloc, shutdown ${shutdown_ms} ms)…"
if [[ -n "${DISPLAY:-}" ]]; then
  "${ui_cmd[@]}"
elif command -v xvfb-run >/dev/null 2>&1; then
  xvfb-run -a -s "-screen 0 1280x720x24" "${ui_cmd[@]}"
else
  printf '%s\n' "FAIL  need DISPLAY or xvfb-run" >&2
  exit 1
fi

printf '%s\n' "PASS  profile-ui finished (see hotpath report above)"
