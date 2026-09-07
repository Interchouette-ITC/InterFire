# Kerio interaction study

This is an interaction study, not a request to copy Kerio branding, artwork, icons,
or wording. InterFire will use its own assets and neutral Linux terminology.

Two Kerio-era products inform structure only:

1. **Personal firewall** tray + connection alert (short decision path).
2. **WinRoute Firewall 6.x Administration Console** (left section tree, right
   content pane, dense Traffic Rules tables, status bar, Apply/reconnect).

InterFire is an application firewall, not a WinRoute gateway. NAT, DHCP, VPN,
content filtering, licensing, and directory trees are out of scope. Proprietary
manuals and screenshots stay local for operators; they are not committed.

## Why the interaction model works

The useful part of the classic personal-firewall model is its short decision path:
the alert identifies the program, identifies the destination, and offers a safe,
understandable choice. The main window separates application permissions, packet
rules, and history rather than presenting a dashboard. WinRoute’s admin chrome
adds a durable left-nav + dense rules table pattern without requiring gateway
features.

## Interaction targets

### Tray state

The tray has states: protected, prompting, degraded, and daemon unavailable. It
must expose the current state in accessible text as well as colour. The menu
opens the main window, shows pending requests, and offers a temporary pause only
after a confirmation explaining the effect.

### Connection alert

The dialog presents these fields in order:

1. Application name and full executable path.
2. Authoritative IP address and port; hostname only when DNS metadata is fresh.
3. Protocol, direction, user, and command line in a collapsed details section.
4. Rule lifetime: this connection, until application exit, or permanently.

The primary actions are Allow and Deny. The default focus is Deny after the
countdown. A decision must say whether it creates a rule. Prompts never claim a
hostname is verified when it is only cached DNS data. Identity rules match
[`ux-interfire.md`](ux-interfire.md) (PID + start ticks; path never basename-only).

### Main window

The stable information architecture is Status, Applications, Rules, Log,
Network, and Settings, with **Rules** as the primary surface. Lists are dense
but readable, sortable, and capped. The Network view only exposes InterFire-owned
packet-filter state; it does not imply ownership of the host firewall.

## Recreate, do not copy

Use original icons, illustrations, text, and styles. Historical screenshots and
manuals may inform structure and information density, but proprietary bitmaps,
logos, trade dress, and trademarked product names are excluded from the
repository.
