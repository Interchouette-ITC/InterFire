#!/usr/bin/env bash
# Controlled allow/deny integration for the daemon verdict path.
# Run only as root. Creates firewall state inside a new network namespace and
# removes it before returning. Requires a built interfired/interfirectl and a
# kernel that can attach the embedded TCP-connect program.
set -euo pipefail

PATH="/usr/sbin:/usr/bin:/sbin:/bin:${PATH}"
readonly queue_number=4242
readonly port=18080
readonly script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly repo_root="$(cd -- "${script_dir}/.." && pwd)"

if [[ ${1:-} != --inside ]]; then
  if [[ ${EUID} -ne 0 ]]; then
    printf '%s\n' "Run with: sudo $0"
    exit 64
  fi
  exec unshare --fork --net --mount-proc bash "$0" --inside
fi

if [[ ${EUID} -ne 0 ]]; then
  printf '%s\n' "The isolated test requires root."
  exit 64
fi

daemon_bin="${INTERFIRED_BIN:-${repo_root}/target/debug/interfired}"
ctl_bin="${INTERFIRECTL_BIN:-${repo_root}/target/debug/interfirectl}"
if [[ ! -x "${daemon_bin}" || ! -x "${ctl_bin}" ]]; then
  printf '%s\n' "FAIL  missing binaries; build with: cargo build -p interfire-daemon -p interfirectl" >&2
  printf '%s\n' "      looked for ${daemon_bin} and ${ctl_bin}" >&2
  exit 1
fi

client_exe="$(readlink -f "$(command -v python3)")"
if [[ -z "${client_exe}" || ! -x "${client_exe}" ]]; then
  printf '%s\n' "FAIL  python3 is required as the controlled client" >&2
  exit 1
fi

work_dir="$(mktemp -d)"
socket="${work_dir}/interfired.sock"
rules_path="${work_dir}/rules.toml"
daemon_pid=""
server_pid=""

cleanup() {
  local result=$?
  [[ -n "${daemon_pid}" ]] && kill "${daemon_pid}" 2>/dev/null || true
  [[ -n "${server_pid}" ]] && kill "${server_pid}" 2>/dev/null || true
  nft delete table inet interfire_integration 2>/dev/null || true
  rm -rf "${work_dir}"
  exit "${result}"
}
trap cleanup EXIT

write_rules() {
  local verdict=$1
  cat >"${rules_path}" <<EOF
schema_version = 1

[[rules]]
id = 1
executable = "${client_exe}"
protocol = "tcp"
direction = "outbound"
port = ${port}
verdict = "${verdict}"
scope = "permanent"
EOF
}

wait_for_status() {
  local want_enforcement=$1
  local want_observation=$2
  local status=""
  for _ in $(seq 1 100); do
    if status="$("${ctl_bin}" --socket="${socket}" status 2>/dev/null)"; then
      if [[ "${status}" == *"enforcement=${want_enforcement}"* &&
            "${status}" == *"observation=${want_observation}"* ]]; then
        return 0
      fi
    fi
    sleep 0.05
  done
  printf '%s\n' "FAIL  daemon status never reached enforcement=${want_enforcement} observation=${want_observation}" >&2
  printf '%s\n' "      last status: ${status:-<none>}" >&2
  printf '%s\n' "      daemon log:" >&2
  sed 's/^/      /' "${work_dir}/daemon.log" >&2 || true
  return 1
}

start_daemon() {
  write_rules "$1"
  rm -f "${socket}"
  RUST_LOG=info "${daemon_bin}" \
    --socket="${socket}" \
    --rules="${rules_path}" \
    >"${work_dir}/daemon.log" 2>&1 &
  daemon_pid=$!
  wait_for_status nfqueue attached
}

stop_daemon() {
  if [[ -n "${daemon_pid}" ]]; then
    kill "${daemon_pid}" 2>/dev/null || true
    wait "${daemon_pid}" 2>/dev/null || true
    daemon_pid=""
  fi
}

install_queue() {
  nft -f - <<RULES
table inet interfire_integration {
  chain output {
    type filter hook output priority filter; policy accept;
    ip daddr 127.0.0.1 tcp dport ${port} queue num ${queue_number}
  }
}
RULES
}

remove_queue() {
  nft delete table inet interfire_integration 2>/dev/null || true
}

try_connect() {
  set +e
  timeout 3 python3 -c \
    "import socket; socket.create_connection(('127.0.0.1', ${port}), timeout=2).close()" \
    2>"${work_dir}/client-error.log"
  local result=$?
  set -e
  printf '%s\n' "${result}"
}

run_case() {
  local verdict=$1
  local expected=$2

  start_daemon "${verdict}"
  install_queue

  local result
  result="$(try_connect)"
  remove_queue
  stop_daemon

  if [[ "${expected}" == pass && ${result} -eq 0 ]] ||
     [[ "${expected}" == fail && ${result} -ne 0 ]]; then
    printf 'PASS  %s rule behaved as expected (client status %s)\n' "${verdict}" "${result}"
  else
    printf 'FAIL  %s rule returned client status %s (expected %s)\n' \
      "${verdict}" "${result}" "${expected}" >&2
    printf '%s\n' "      client error:" >&2
    sed 's/^/      /' "${work_dir}/client-error.log" >&2 || true
    printf '%s\n' "      daemon log:" >&2
    sed 's/^/      /' "${work_dir}/daemon.log" >&2 || true
    return 1
  fi
}

printf '%s\n' 'InterFire enforcement integration: isolated network namespace'
printf '%s\n' "client executable: ${client_exe}"

ip link set lo up
python3 -m http.server "${port}" --bind 127.0.0.1 \
  >"${work_dir}/http.log" 2>&1 &
server_pid=$!

for _ in $(seq 1 50); do
  if python3 -c \
    "import socket; socket.create_connection(('127.0.0.1', ${port}), timeout=.1).close()" \
    2>/dev/null; then
    break
  fi
  sleep 0.02
done
python3 -c \
  "import socket; socket.create_connection(('127.0.0.1', ${port}), timeout=.1).close()" \
  2>/dev/null

run_case allow pass
run_case deny fail
printf '%s\n' 'PASS  controlled process allow and deny via daemon NFQUEUE path.'
