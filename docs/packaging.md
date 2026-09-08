# Packaging and reboot recovery

InterFire ships systemd units, an owned nftables script, and a Debian package
builder under `packaging/` / `scripts/build-deb.sh`. This document describes
install layout and how protection returns after reboot.

## Layout

| Path | Role |
| --- | --- |
| `packaging/systemd/interfired.service` | Daemon unit (`interfired`) |
| `packaging/systemd/interfire-nft.service` | Oneshot: load/remove `inet interfire` |
| `packaging/nft/interfire.nft` | Owned table script (queue **4242**) |
| `packaging/tmpfiles.d/interfire.conf` | `/run`, `/var/lib`, `/etc` dirs |
| `packaging/defaults/rules.toml` | Empty durable rules (`schema_version = 1`) |
| `packaging/debian/interfire.desktop` | Desktop entry for `interfire-ui` |
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
`postinst` enables and starts `interfire-nft` + `interfired`. Primary verify
images: Debian (stable) GNOME and Pop!\_OS, `x86_64`. Full install / upgrade /
reboot / uninstall checklist and migration notes:
[`install-matrix.md`](install-matrix.md).

| Host path | Content |
| --- | --- |
| `/usr/bin/interfired` | Daemon |
| `/usr/lib/systemd/system/interfired.service` | Daemon unit |
| `/usr/lib/systemd/system/interfire-nft.service` | nft oneshot |
| `/usr/share/interfire/interfire.nft` | Owned table |
| `/usr/lib/tmpfiles.d/interfire.conf` | Directory mode |
| `/etc/interfire/rules.toml` | Durable rules |
| `/run/interfire/interfired.sock` | IPC socket |
| `/var/lib/interfire/audit.log` | Capped audit log |

## Capabilities

`interfired.service` bounds ambient capabilities to BPF / perfmon / net admin /
net raw / sys admin. The process still runs as root so NFQUEUE and eBPF attach
can work on primary Debian and Pop!\_OS kernels. Missing caps show as
`observation=degraded` or `enforcement=degraded` on `status`.

## Reboot recovery

1. Enable both units: `systemctl enable --now interfire-nft.service interfired.service`.
2. After reboot, systemd starts `interfire-nft` before `interfired`, reloading
   only table `inet interfire` from `/usr/share/interfire/interfire.nft`.
3. The daemon binds NFQUEUE **4242** and recreates `/run/interfire/interfired.sock`.
4. Durable rules under `/etc/interfire/rules.toml` and audit under
   `/var/lib/interfire/` survive reboot.

If `interfire-nft` fails (nft missing or conflict), the daemon may still start
with `enforcement=nfqueue` or `degraded`, but packets are not queued until the
owned table is present. Fix by repairing the nft oneshot and restarting both
units. Do not edit unrelated firewall tables to recover.

Manual smoke without the package:

```bash
sudo install -d -m 0750 /run/interfire /var/lib/interfire /etc/interfire
sudo install -d -m 0755 /usr/share/interfire
sudo cp packaging/defaults/rules.toml /etc/interfire/rules.toml
sudo cp packaging/nft/interfire.nft /usr/share/interfire/interfire.nft
sudo cp packaging/systemd/*.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now interfire-nft.service interfired.service
interfirectl status
```
