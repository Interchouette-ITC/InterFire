#!/usr/bin/env bash
# Measure idle `interfired` RSS against the idle RSS budget (< 40 MiB).
# Non-root: starts with --no-ebpf --no-nfqueue so CI and developer machines can
# run the same gate without caps.
set -euo pipefail

PATH="/usr/sbin:/usr/bin:/sbin:/bin:${PATH}"
readonly script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly repo_root="$(cd -- "${script_dir}/.." && pwd)"
readonly budget_kib="${INTERFIRE_MEMCHECK_BUDGET_KIB:-40960}"

daemon_bin="${INTERFIRED_BIN:-${repo_root}/target/debug/interfired}"
if [[ ! -x "${daemon_bin}" ]]; then
  printf '%s\n' "FAIL  missing ${daemon_bin}; build with: cargo build -p interfire-daemon" >&2
  exit 1
fi

work_dir="$(mktemp -d)"
socket="${work_dir}/interfired.sock"
rules_path="${work_dir}/rules.toml"
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
  --audit="${work_dir}/audit.log" \
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
  printf '%s\n' "FAIL  daemon socket never appeared at ${socket}" >&2
  sed 's/^/      /' "${work_dir}/daemon.log" >&2 || true
  exit 1
fi

# Let allocator / threads settle briefly before sampling.
sleep 0.2

rss_kib="$(awk '/^VmRSS:/ { print $2; exit }' "/proc/${daemon_pid}/status")"
if [[ -z "${rss_kib}" ]]; then
  printf '%s\n' "FAIL  could not read VmRSS for pid ${daemon_pid}" >&2
  exit 1
fi

printf '%s\n' "idle interfired VmRSS=${rss_kib} KiB (budget ${budget_kib} KiB)"

if (( rss_kib > budget_kib )); then
  printf '%s\n' "FAIL  idle RSS ${rss_kib} KiB exceeds budget ${budget_kib} KiB (< 40 MiB)" >&2
  exit 1
fi

printf '%s\n' "PASS  idle daemon RSS within budget"
