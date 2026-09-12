# Install matrix and migration

Checklist for packaging verification on the primary v0.1 images. Run after
`make deb` produces `target/debian/interfire_0.1.0_amd64.deb`.

## Primary images

| Image | Arch | Notes |
| --- | --- | --- |
| Debian (stable) with GNOME | `x86_64` | Primary |
| Pop!\_OS (current supported) | `x86_64` | Primary |

CI Ubuntu runners are a compile proxy only; they do not expand support claims.

## Install

On each primary image:

1. `sudo dpkg -i target/debian/interfire_0.1.0_amd64.deb` (fix deps with
   `sudo apt-get install -f` if needed).
2. Confirm group `interfire` exists (`getent group interfire`). If you installed
   with `sudo`, your login user should already be a member; **log out and back
   in** so the group applies. Otherwise:
   `sudo usermod -aG interfire "$USER"` then re-login.
3. As that **non-root** user (no sudo): `id -nG` lists `interfire`.
4. `systemctl is-active interfire-nft.service interfired.service` → `active`.
5. `nft list table inet interfire` shows the owned queue rule (queue **4242**).
6. **Without sudo:** `interfirectl status` returns `enforcement=` / `observation=`
   (nfqueue or degraded when caps/BTF are missing; never silent allow).
7. **Without sudo:** `interfirectl ping` succeeds against
   `/run/interfire/interfired.sock`.
8. **Without sudo:** `interfire-tui` opens and shows Status (daemon up).
9. Under a graphical session, launch `interfire-ui` (menu or CLI) **without
   sudo**. Confirm the window opens. Tray: StatusNotifierItem visible, **or**
   Status chrome reports honest degrade when the shell has no SNI.
10. With tray present: close the main window; tray remains. Tray **Open
    InterFire** reopens the window; **Quit InterFire UI** exits the process.

## Upgrade

1. Install an older package build (or the same `.deb` again).
2. Install the newer `.deb` with `dpkg -i`.
3. Confirm `/etc/interfire/rules.toml` is preserved (conffile).
4. Confirm units restarted and `interfirectl status` still works.
5. Confirm `nft list table inet interfire` still present after upgrade.

## Reboot

1. `sudo reboot` with units enabled.
2. After login (non-root, group `interfire`): both units `active`; owned table
   restored; socket present; `interfirectl status` works **without sudo**.
3. Desktop smoke again (`interfire-ui` + tray or honest Status degrade).

## Uninstall

1. `sudo dpkg -r interfire` (remove, keep conffiles).
2. Confirm units stopped; `interfired` not running.
3. `nft list table inet interfire` should fail (table removed on nft unit stop).
4. `sudo dpkg --purge interfire` removes `/etc/interfire` conffiles and
   `/var/lib/interfire` (postrm purge).

## Migration from other host app firewalls

InterFire does **not** import foreign rule databases, daemons, or eBPF objects.

Before installing InterFire as the sole application firewall:

1. Disable or uninstall any other host app-firewall that owns outbound NFQUEUE
   or competing nft output hooks for the same traffic.
2. Confirm no leftover queue rules target a different userspace listener on
   queue **4242**.
3. Install InterFire, then verify with the Install checklist above.
4. Recreate allow/deny policy in InterFire rules (TUI, UI, or
   `interfirectl rules …`); do not copy proprietary rule files into
   `/etc/interfire/`.

Rollback: `sudo dpkg -r interfire` (or purge), restore the previous product if
needed, and reload that product's firewall state per its docs.
