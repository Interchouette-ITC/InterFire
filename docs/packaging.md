# Packaging and reboot recovery

InterFire ships systemd units, an owned nftables script, and a Debian package
builder under `packaging/` / `scripts/build-deb.sh`. This document describes
install layout and how protection returns after reboot.

## Layout

| Path | Role |
| --- | --- |
| `packaging/systemd/interfired.service` | Daemon unit (`interfired`) |
| `packaging/systemd/interfire-nft.service` | Manual recovery oneshot for `inet interfire` (not enabled by default) |
| `packaging/nft/interfire.nft` | Owned table script (queue **4242**) |
| `packaging/tmpfiles.d/interfire.conf` | `/run`, `/var/lib`, `/etc` dirs |
| `packaging/defaults/rules.toml` | Empty durable rules (`schema_version = 1`) |
| `packaging/debian/interfire.desktop` | Desktop entry for `interfire-ui` |
| `packaging/debian/interfire-autostart.desktop` | Session autostart for tray UI |
| `scripts/build-deb.sh` | Stage + `dpkg-deb` (`make deb`) |

## Debian package

```bash
make deb
# → target/debian/interfire_0.1.0_amd64.deb
sudo dpkg -i target/debian/interfire_0.1.0_amd64.deb
```

The package installs binaries (`interfired`, `interfirectl`, `interfire-tui`,
`interfire-ui`), systemd units, the owned nft script, tmpfiles, and default
rules. eBPF bytecode is embedded in `interfired` (no separate object file).
`postinst` enables and starts **`interfired` only**, and **disables**
`interfire-nft`. Default enforcement mode is **paused** (no owned table until
Start / `interfirectl resume`). Primary verify images: Debian (stable) GNOME and
Pop!\_OS, `x86_64`. Full install / upgrade / reboot / uninstall checklist and
migration notes: [`install-matrix.md`](install-matrix.md).

| Host path | Content |
| --- | --- |
| `/usr/bin/interfired` | Daemon |
| `/usr/lib/systemd/system/interfired.service` | Daemon unit |
| `/usr/lib/systemd/system/interfire-nft.service` | nft oneshot (manual recovery) |
| `/usr/share/interfire/interfire.nft` | Owned table |
| `/usr/lib/tmpfiles.d/interfire.conf` | Directory mode |
| `/etc/interfire/rules.toml` | Durable rules |
| `/usr/share/applications/interfire.desktop` | Applications menu entry |
| `/etc/xdg/autostart/interfire.desktop` | Session autostart for `interfire-ui` (tray) |
| `/run/interfire/interfired.sock` | IPC socket |
| `/var/lib/interfire/audit.log` | Capped audit log |
| `/var/lib/interfire/enforcement.mode` | `paused` or `active` (default missing = paused) |

## Capabilities

`interfired.service` bounds ambient capabilities to BPF / perfmon / net admin /
net raw / sys admin. The process still runs as root so NFQUEUE and eBPF attach
can work on primary Debian and Pop!\_OS kernels. Missing caps show as
`observation=degraded` or `enforcement=degraded` on `status`.

## Operator group (unprivileged clients)

Day-to-day clients (`interfirectl`, `interfire-tui`, `interfire-ui`) run as a
normal session user. They must be members of system group **`interfire`**.

| Path | Mode / ownership |
| --- | --- |
| `/run/interfire/` | `0750` `root:interfire` |
| `/run/interfire/interfired.sock` | `0660` `root:interfire` (set after bind) |

Policy mutation over IPC is allowed for root, the daemon UID, or peers in group
`interfire`. Durable rules and audit under `/etc/interfire` and
`/var/lib/interfire` stay root-owned; clients change policy only via IPC.

Package `postinst` creates group `interfire` and, when `dpkg` was invoked via
`sudo`, adds `SUDO_USER` to that group. **Re-login** (or start a new session)
so the supplementary group is active. Manual join:

```bash
sudo usermod -aG interfire "$USER"
# then log out and back in
```

Sudo is for package install, enabling units, and recovery only - not for Status,
Rules, or the desktop UI.

## Reboot recovery

1. Enable the daemon only: `systemctl enable --now interfired.service`.
   Leave `interfire-nft` **disabled** unless recovering manually.
2. After reboot, `interfired` binds NFQUEUE **4242** with **fail-open**, then
   reads `/var/lib/interfire/enforcement.mode`. If `active`, it installs the
   owned table; if `paused` or missing, the table stays absent (network works).
3. Stopping `interfired` runs `nft delete table inet interfire` so a dead queue
   cannot freeze the host.
4. Durable rules under `/etc/interfire/rules.toml` and audit under
   `/var/lib/interfire/` survive reboot.

Pause / Start: `interfirectl pause` / `resume`, or the UI header / tray control.
Do not enable `interfire-nft` at boot for day-to-day use. Do not Start while
another application-firewall queue is already active on the host.

Manual smoke without the package:

```bash
sudo install -d -m 0750 /run/interfire /var/lib/interfire /etc/interfire
sudo install -d -m 0755 /usr/share/interfire
sudo cp packaging/defaults/rules.toml /etc/interfire/rules.toml
sudo cp packaging/nft/interfire.nft /usr/share/interfire/interfire.nft
sudo cp packaging/systemd/*.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now interfired.service
interfirectl status
# expect enforcement=paused until: interfirectl resume
```
