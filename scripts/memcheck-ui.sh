#!/usr/bin/env bash
# Measure `interfire-ui` RSS against release gates (idle / prompt-load / combined).
# Uses release binaries. Needs a working display (DISPLAY or xvfb-run) and software
# GL (`LIBGL_ALWAYS_SOFTWARE=1`, `WGPU_BACKEND=gl`) for headless-friendly sampling.
set -euo pipefail

PATH="/usr/sbin:/usr/bin:/sbin:/bin:${PATH}"
readonly script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly repo_root="$(cd -- "${script_dir}/.." && pwd)"

# Release GPUI+wgpu baseline on Linux is ~190 MiB idle; budgets are measured
# ceilings with headroom (see docs-dev/ui-gpui.md).
readonly idle_budget_kib="${INTERFIRE_UI_IDLE_BUDGET_KIB:-225280}"         # 220 MiB
readonly prompt_budget_kib="${INTERFIRE_UI_PROMPT_BUDGET_KIB:-266240}"     # 260 MiB
readonly combined_budget_kib="${INTERFIRE_UI_COMBINED_BUDGET_KIB:-266240}" # 260 MiB

daemon_bin="${INTERFIRED_BIN:-${repo_root}/target/release/interfired}"
ui_bin="${INTERFIRE_UI_BIN:-${repo_root}/target/release/interfire-ui}"

if [[ ! -x "${daemon_bin}" ]]; then
  printf '%s\n' "FAIL  missing ${daemon_bin}; build with: cargo build -p interfire-daemon --release" >&2
  exit 1
fi
if [[ ! -x "${ui_bin}" ]]; then
  printf '%s\n' "FAIL  missing ${ui_bin}; build with: cargo build -p interfire-ui --release" >&2
  exit 1
fi

work_dir="$(mktemp -d)"
socket="${work_dir}/interfired.sock"
rules_path="${work_dir}/rules.toml"
audit_path="${work_dir}/audit.log"
daemon_pid=""
ui_pid=""

cleanup() {
  local result=$?
  [[ -n "${ui_pid}" ]] && kill "${ui_pid}" 2>/dev/null || true
  [[ -n "${ui_pid}" ]] && wait "${ui_pid}" 2>/dev/null || true
  [[ -n "${daemon_pid}" ]] && kill "${daemon_pid}" 2>/dev/null || true
  [[ -n "${daemon_pid}" ]] && wait "${daemon_pid}" 2>/dev/null || true
  rm -rf "${work_dir}"
  exit "${result}"
}
trap cleanup EXIT

read_rss_kib() {
  local pid="$1"
  awk '/^VmRSS:/ { print $2; exit }' "/proc/${pid}/status"
}

start_daemon() {
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
      return 0
    fi
    if ! kill -0 "${daemon_pid}" 2>/dev/null; then
      printf '%s\n' "FAIL  daemon exited before listening" >&2
      sed 's/^/      /' "${work_dir}/daemon.log" >&2 || true
      exit 1
    fi
    sleep 0.02
  done
  printf '%s\n' "FAIL  daemon socket never appeared at ${socket}" >&2
  sed 's/^/      /' "${work_dir}/daemon.log" >&2 || true
  exit 1
}

start_ui() {
  local mode="$1"
  local logfile="${work_dir}/ui-${mode}.log"
  export LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE:-1}"
  export WGPU_BACKEND="${WGPU_BACKEND:-gl}"
  if [[ -z "${DISPLAY:-}" ]] && command -v xvfb-run >/dev/null 2>&1; then
    xvfb-run -a -s '-screen 0 1280x720x24' \
      env LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE}" WGPU_BACKEND="${WGPU_BACKEND}" \
      "${ui_bin}" --socket="${socket}" --rss-probe="${mode}" \
      >"${logfile}" 2>&1 &
  else
    if [[ -z "${DISPLAY:-}" ]]; then
      printf '%s\n' "FAIL  DISPLAY unset and xvfb-run not found" >&2
      exit 1
    fi
    "${ui_bin}" --socket="${socket}" --rss-probe="${mode}" >"${logfile}" 2>&1 &
  fi
  ui_pid=$!
  sleep "${INTERFIRE_UI_MEMCHECK_SETTLE_SECS:-4}"
  if ! kill -0 "${ui_pid}" 2>/dev/null; then
    printf '%s\n' "FAIL  interfire-ui exited during ${mode} settle" >&2
    sed 's/^/      /' "${logfile}" >&2 || true
    exit 1
  fi
}

stop_ui() {
  if [[ -n "${ui_pid}" ]]; then
    kill "${ui_pid}" 2>/dev/null || true
    wait "${ui_pid}" 2>/dev/null || true
    ui_pid=""
  fi
}

check_budget() {
  local label="$1"
  local rss_kib="$2"
  local budget_kib="$3"
  printf '%s\n' "${label} VmRSS=${rss_kib} KiB (budget ${budget_kib} KiB)"
  if (( rss_kib > budget_kib )); then
    printf '%s\n' "FAIL  ${label} RSS ${rss_kib} KiB exceeds budget ${budget_kib} KiB" >&2
    exit 1
  fi
}

start_daemon
sleep 0.2
daemon_rss="$(read_rss_kib "${daemon_pid}")"
if [[ -z "${daemon_rss}" ]]; then
  printf '%s\n' "FAIL  could not read daemon VmRSS" >&2
  exit 1
fi

start_ui idle
idle_rss="$(read_rss_kib "${ui_pid}")"
if [[ -z "${idle_rss}" ]]; then
  printf '%s\n' "FAIL  could not read idle UI VmRSS" >&2
  exit 1
fi
check_budget "idle interfire-ui" "${idle_rss}" "${idle_budget_kib}"
combined=$((daemon_rss + idle_rss))
check_budget "combined daemon+ui" "${combined}" "${combined_budget_kib}"
stop_ui

start_ui prompt-load
prompt_rss="$(read_rss_kib "${ui_pid}")"
if [[ -z "${prompt_rss}" ]]; then
  printf '%s\n' "FAIL  could not read prompt-load UI VmRSS" >&2
  exit 1
fi
check_budget "prompt-load interfire-ui" "${prompt_rss}" "${prompt_budget_kib}"
stop_ui

printf '%s\n' "PASS  UI RSS gates within budget (idle / prompt-load / combined)"
