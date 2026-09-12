#!/bin/sh
# systemd ExecStop helper: preserve Traffic kill-switch; otherwise drop owned table.
set -eu
STATE_DIR="${INTERFIRE_STATE_DIR:-/var/lib/interfire}"
MACHINE_FILE="${INTERFIRE_TRAFFIC_MACHINE:-$STATE_DIR/traffic.machine}"
LEGACY_FILE="${INTERFIRE_TRAFFIC_MODE:-$STATE_DIR/traffic.mode}"
BLOCK_OUT="${INTERFIRE_BLOCK_NFT:-/usr/share/interfire/interfire-block.nft}"
BLOCK_IN="${INTERFIRE_BLOCK_IN_NFT:-/usr/share/interfire/interfire-block-in.nft}"
BLOCK_ALL="${INTERFIRE_BLOCK_ALL_NFT:-/usr/share/interfire/interfire-block-all.nft}"

machine="open"
if [ -f "$MACHINE_FILE" ]; then
  machine=$(tr -d '[:space:]' <"$MACHINE_FILE" || true)
elif [ -f "$LEGACY_FILE" ] && grep -qx 'blocked' "$LEGACY_FILE"; then
  machine="out"
fi

has_user=0
for f in "$STATE_DIR"/traffic.user.*; do
  [ -e "$f" ] || continue
  pref=$(tr -d '[:space:]' <"$f" || true)
  case "$pref" in
    out|in|all|blocked) has_user=1 ;;
  esac
done

reinstall() {
  script=$1
  if [ -f "$script" ]; then
    /usr/sbin/nft delete table inet interfire >/dev/null 2>&1 || true
    /usr/sbin/nft -f "$script"
  fi
}

case "$machine" in
  all)
    reinstall "$BLOCK_ALL"
    ;;
  in)
    reinstall "$BLOCK_IN"
    ;;
  out|blocked)
    reinstall "$BLOCK_OUT"
    ;;
  *)
    if [ "$has_user" -eq 1 ]; then
      # Preserve fail-closed host drop until daemon reloads precise uid rules.
      reinstall "$BLOCK_ALL"
    else
      /usr/sbin/nft delete table inet interfire >/dev/null 2>&1 || true
    fi
    ;;
esac
