#!/usr/bin/env bash
# Build an amd64 .deb from release binaries + packaging/ assets.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "${root}"

version="${INTERFIRE_DEB_VERSION:-0.1.0}"
arch="${INTERFIRE_DEB_ARCH:-amd64}"
out_dir="${INTERFIRE_DEB_OUT:-${root}/target/debian}"
stage="${out_dir}/interfire_${version}_${arch}"
deb="${out_dir}/interfire_${version}_${arch}.deb"

cargo="${CARGO:-cargo +stable}"

echo "==> release build"
unset CARGO_TARGET_DIR
case "${CARGO_TARGET_DIR:-}" in *cursor-sandbox-cache*) unset CARGO_TARGET_DIR ;; esac
${cargo} build --release -p interfire-daemon -p interfirectl -p interfire-tui -p interfire-ui

echo "==> stage ${stage}"
rm -rf "${stage}"
mkdir -p \
  "${stage}/DEBIAN" \
  "${stage}/usr/bin" \
  "${stage}/usr/lib/systemd/system" \
  "${stage}/usr/lib/tmpfiles.d" \
  "${stage}/usr/share/interfire" \
  "${stage}/usr/share/applications" \
  "${stage}/usr/share/doc/interfire" \
  "${stage}/etc/interfire"

install -m 0755 target/release/interfired "${stage}/usr/bin/interfired"
install -m 0755 target/release/interfirectl "${stage}/usr/bin/interfirectl"
install -m 0755 target/release/interfire-tui "${stage}/usr/bin/interfire-tui"
install -m 0755 target/release/interfire-ui "${stage}/usr/bin/interfire-ui"

install -m 0644 packaging/systemd/interfired.service \
  "${stage}/usr/lib/systemd/system/interfired.service"
install -m 0644 packaging/systemd/interfire-nft.service \
  "${stage}/usr/lib/systemd/system/interfire-nft.service"
install -m 0644 packaging/tmpfiles.d/interfire.conf \
  "${stage}/usr/lib/tmpfiles.d/interfire.conf"
install -m 0644 packaging/nft/interfire.nft \
  "${stage}/usr/share/interfire/interfire.nft"
install -m 0644 packaging/defaults/rules.toml \
  "${stage}/etc/interfire/rules.toml"
install -m 0644 packaging/debian/interfire.desktop \
  "${stage}/usr/share/applications/interfire.desktop"

{
  printf '%s\n' 'Format: 1.0'
  printf '%s\n' 'Name: InterFire'
  printf '%s\n' 'Upstream-Name: InterFire'
  printf '%s\n' 'Source: https://github.com/Interchouette-ITC/InterFire'
  printf '%s\n' 'Files: *'
  printf '%s\n' 'Copyright: Interchouette ITC'
  printf '%s\n' 'License: Apache-2.0'
  printf '%s\n' ' Apache License 2.0; see /usr/share/common-licenses/Apache-2.0'
} >"${stage}/usr/share/doc/interfire/copyright"

size_kib="$(du -sk "${stage}" | awk '{print $1}')"

cat >"${stage}/DEBIAN/control" <<EOF
Package: interfire
Version: ${version}
Section: net
Priority: optional
Architecture: ${arch}
Maintainer: Interchouette ITC <contact@interchouette.net>
Installed-Size: ${size_kib}
Depends: nftables, libnetfilter-queue1, libc6, libgcc-s1
Recommends: libfontconfig1, libxkbcommon0, libwayland-client0
Description: Linux application firewall (daemon, CLI, TUI, desktop UI)
 InterFire attributes outbound connections to processes, matches durable
 rules, and verdicts via NFQUEUE. Ships interfired, interfirectl,
 interfire-tui, interfire-ui, systemd units, and the InterFire-owned
 nftables queue script (inet interfire / queue 4242).
EOF

cat >"${stage}/DEBIAN/conffiles" <<EOF
/etc/interfire/rules.toml
EOF

cat >"${stage}/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v systemd-tmpfiles >/dev/null 2>&1; then
  systemd-tmpfiles --create /usr/lib/tmpfiles.d/interfire.conf || true
fi
if command -v systemctl >/dev/null 2>&1; then
  systemctl daemon-reload || true
  systemctl enable interfire-nft.service interfired.service >/dev/null 2>&1 || true
  systemctl start interfire-nft.service interfired.service >/dev/null 2>&1 || true
fi
exit 0
EOF

cat >"${stage}/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e
if command -v systemctl >/dev/null 2>&1; then
  systemctl stop interfired.service interfire-nft.service >/dev/null 2>&1 || true
  systemctl disable interfired.service interfire-nft.service >/dev/null 2>&1 || true
fi
exit 0
EOF

cat >"${stage}/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if command -v systemctl >/dev/null 2>&1; then
  systemctl daemon-reload || true
fi
if [ "$1" = "purge" ]; then
  rm -rf /var/lib/interfire /run/interfire
fi
exit 0
EOF

chmod 0755 "${stage}/DEBIAN/postinst" "${stage}/DEBIAN/prerm" "${stage}/DEBIAN/postrm"

echo "==> dpkg-deb ${deb}"
mkdir -p "${out_dir}"
fakeroot dpkg-deb --build "${stage}" "${deb}"
dpkg-deb --info "${deb}"
dpkg-deb --contents "${deb}" | head -40
echo "built ${deb}"
