#!/bin/sh
# systemd ExecStop helper: preserve Traffic Block; otherwise drop owned table.
set -eu
MODE_FILE="${INTERFIRE_TRAFFIC_MODE:-/var/lib/interfire/traffic.mode}"
BLOCK_SCRIPT="${INTERFIRE_BLOCK_NFT:-/usr/share/interfire/interfire-block.nft}"

if [ -f "$MODE_FILE" ] && grep -qx 'blocked' "$MODE_FILE"; then
  if [ -f "$BLOCK_SCRIPT" ]; then
    /usr/sbin/nft delete table inet interfire >/dev/null 2>&1 || true
    /usr/sbin/nft -f "$BLOCK_SCRIPT"
  fi
else
  /usr/sbin/nft delete table inet interfire >/dev/null 2>&1 || true
fi
