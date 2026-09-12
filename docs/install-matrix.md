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
4. `systemctl is-active interfired.service` → `active`.
   `systemctl is-enabled interfire-nft.service` → `disabled` (or not enabled).
5. `nft list table inet interfire` should **fail** after a fresh install
   (default mode **paused**; no owned table until Start).
6. **Without sudo:** `interfirectl status` returns `enforcement=paused` (or
   `nfqueue` / `degraded` after Start; never silent allow while Active).
7. **Without sudo:** `interfirectl ping` succeeds against
   `/run/interfire/interfired.sock`.
8. **Without sudo:** `interfire-tui` opens and shows Status (daemon up).
9. Under a graphical session, launch `interfire-ui` (menu or CLI) **without
   sudo**. Confirm the window opens. Header shows **Firewall: Paused** with
   Start. Tray: StatusNotifierItem visible, **or** Status chrome reports honest
   degrade when the shell has no SNI.
10. With tray present: close the main window; tray remains. Tray **Open
    InterFire** reopens the window; **Quit InterFire UI** exits the process.
11. Start firewall (UI confirm or `interfirectl resume`); confirm
    `nft list table inet interfire` and `enforcement=nfqueue`.
12. Pause again; table gone; network still usable.

## Upgrade

1. Install an older package build (or the same `.deb` again).
2. Install the newer `.deb` with `dpkg -i`.
3. Confirm `/etc/interfire/rules.toml` is preserved (conffile).
4. Confirm units restarted and `interfirectl status` still works.
5. Confirm enforcement mode file and table match last Pause/Start choice
   (paused → no table; active → table present after daemon bind).

## Reboot

1. `sudo reboot` with `interfired` enabled (`interfire-nft` disabled).
2. After login (non-root, group `interfire`): `interfired` active; network
   usable; if mode was paused, no owned table; socket present;
   `interfirectl status` works **without sudo**.
3. Desktop smoke again (`interfire-ui` + Pause/Start header + tray).
4. Optional: while Active, `sudo kill -9` the daemon PID; host network must
   remain usable (NFQUEUE fail-open). Restart the unit afterward.

## Uninstall

1. `sudo dpkg -r interfire` (remove, keep conffiles).
2. Confirm units stopped; `interfired` not running.
3. `nft list table inet interfire` should fail (table removed on daemon stop).
4. `sudo dpkg --purge interfire` removes `/etc/interfire` conffiles and
   `/var/lib/interfire` (postrm purge).

## Safe enforcement dogfood (Phase S)

On each primary image after install:

1. Reboot while **Paused**: desktop network (browser, package tools) works.
2. Start from UI or `interfirectl resume`: owned table present; new TCP filtered.
3. Pause: table removed; connectivity restored.
4. Start again, then `sudo kill -9 $(pidof interfired)`: host stays usable.
5. `sudo systemctl start interfired` and confirm status again.

Only when this checklist is green is Phase R unblocked.
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
