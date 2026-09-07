#!/usr/bin/env bash
# Read-only host capability probe. It does not attach BPF programs or alter
# nftables, packet filters, modules, services, or network traffic.
set -euo pipefail

# Desktop user shells commonly omit sbin directories even though these system
# utilities are installed there. Preserve a caller's PATH while making the
# probe suitable for both an interactive shell and a systemd diagnostic run.
PATH="/usr/sbin:/usr/bin:/sbin:/bin:${PATH}"

status=0
check() {
  local label=$1
  shift
  if "$@"; then
    printf 'PASS  %s\n' "$label"
  else
    printf 'FAIL  %s\n' "$label"
    status=1
  fi
}

printf 'InterFire capability probe\n'
printf 'kernel: %s\n' "$(uname -r)"
check 'BTF vmlinux is readable' test -r /sys/kernel/btf/vmlinux
check 'bpffs path exists' test -d /sys/fs/bpf
check 'NFQUEUE module is loaded' sh -c "grep -q '^nfnetlink_queue ' /proc/modules"
check 'OpenSnitch reference daemon is installed (optional)' command -v opensnitchd

if command -v bpftool >/dev/null 2>&1; then
  printf 'INFO  bpftool is available\n'
else
  printf 'INFO  bpftool is absent; package it before the eBPF load test\n'
fi

if command -v nft >/dev/null 2>&1; then
  printf 'INFO  nft is available\n'
else
  printf 'INFO  nft is absent; package it before the NFQUEUE verdict test\n'
fi

printf '%s\n' 'NEXT  Run the controlled root-only verdict test in an isolated network namespace.'
printf '%s\n' 'NEXT  Record allow, deny, first-connect latency, daemon loss, and coexistence results in docs/architecture.md.'
exit "$status"
