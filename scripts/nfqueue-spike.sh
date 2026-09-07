#!/usr/bin/env bash
# Isolated NFQUEUE verdict test. Run only as root. It creates all firewall state
# inside a new network namespace and removes it before returning.
set -euo pipefail

PATH="/usr/sbin:/usr/bin:/sbin:/bin:${PATH}"
readonly queue_number=4242
readonly port=18080
readonly script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly listener_source="${script_dir}/nfqueue-listener.c"

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

work_dir="$(mktemp -d)"
listener_pid=""
server_pid=""
cleanup() {
  local result=$?
  [[ -n "${listener_pid}" ]] && kill "${listener_pid}" 2>/dev/null || true
  [[ -n "${server_pid}" ]] && kill "${server_pid}" 2>/dev/null || true
  nft delete table inet interfire_nfqueue_spike 2>/dev/null || true
  rm -rf "${work_dir}"
  exit "${result}"
}
trap cleanup EXIT

cc -std=c17 -Wall -Wextra -Werror "${listener_source}" \
  -lnetfilter_queue -o "${work_dir}/nfqueue-listener"

ip link set lo up
python3 -m http.server "${port}" --bind 127.0.0.1 \
  >"${work_dir}/http.log" 2>&1 &
server_pid=$!

# Confirm the local test server is accepting before any NFQUEUE rule exists.
# Without this, a slow Python start can be mistaken for an allow verdict failure.
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

run_case() {
  local action=$1
  local expected=$2
  local listener_log="${work_dir}/${action}.log"

  "${work_dir}/nfqueue-listener" "${action}" >"${listener_log}" 2>&1 &
  listener_pid=$!
  for _ in $(seq 1 50); do
    grep -qx ready "${listener_log}" && break
    sleep 0.02
  done
  grep -qx ready "${listener_log}"

  nft -f - <<RULES
table inet interfire_nfqueue_spike {
  chain output {
    type filter hook output priority filter; policy accept;
    ip daddr 127.0.0.1 tcp dport ${port} queue num ${queue_number}
  }
}
RULES

  set +e
  timeout 2 python3 -c \
    "import socket; socket.create_connection(('127.0.0.1', ${port}), timeout=1).close()" \
    2>"${work_dir}/${action}.client-error.log"
  local result=$?
  set -e

  nft delete table inet interfire_nfqueue_spike
  wait "${listener_pid}"
  listener_pid=""

  if [[ "${expected}" == pass && ${result} -eq 0 ]] ||
     [[ "${expected}" == fail && ${result} -ne 0 ]]; then
    printf 'PASS  %s verdict behaved as expected\n' "${action}"
  else
    printf 'FAIL  %s verdict returned client status %s\n' "${action}" "${result}" >&2
    return 1
  fi
}

printf '%s\n' 'InterFire NFQUEUE isolated test: temporary network namespace'
run_case allow pass
run_case deny fail
printf '%s\n' 'PASS  NFQUEUE delivered one packet and honored both userspace verdicts.'
