# Kerio interaction study

This is an interaction study, not a request to copy Kerio branding, artwork, icons,
or wording. Interfire will use its own assets and neutral Linux terminology.

## Why the interaction model works

The useful part of the classic personal-firewall model is its short decision path:
the alert identifies the program, identifies the destination, and offers a safe,
understandable choice. The main window separates application permissions, packet
rules, and history rather than presenting a dashboard.

## Interaction targets

### Tray state

The tray has three states: protected, degraded, and waiting for a decision. It
must expose the current state in accessible text as well as colour. The menu
opens the main window, shows pending requests, and offers a temporary pause only
after a confirmation explaining the effect.

### Connection alert

The dialog presents these fields in order:

1. Application name and full executable path.
2. Remote hostname when known, followed by authoritative IP address and port.
3. Protocol, direction, user, and command line in a collapsed details section.
4. Rule lifetime: this connection, until application exit, or permanently.

The primary actions are Allow and Deny. The default focus is Deny after the
countdown. A decision must say whether it creates a rule. Prompts never claim a
hostname is verified when it is only cached DNS data.

### Main window

The stable information architecture is Applications, Rules, Log, Network, and
Settings. Lists are dense but readable, sortable, and capped. The Network view
only exposes Interfire-owned packet-filter state; it does not imply ownership of
the host firewall.

## Recreate, do not copy

Use original icons, illustrations, text, and CSS. Historical screenshots and
manuals may inform structure and information density, but proprietary bitmaps,
logos, trade dress, and trademarked names are excluded from the repository.

