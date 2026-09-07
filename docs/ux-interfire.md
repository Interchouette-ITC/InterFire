# InterFire UI contract

This contract locks the initial UI behavior. A later design change requires an
explicit update to this document.

## Shell

v0.1 clients share the same Unix IPC. What exists today vs what is planned:

| Surface | Status | Role |
| --- | --- | --- |
| `interfirectl` | Shipped | One-shot commands only (`ping`, `status`, rules / prompts / dns / audit) |
| `interfire-tui` | Shipped | Interactive control plane (Status \| Rules \| Prompts \| Log \| Help) |
| GPUI app under `ui/` | Scaffold + tray + connection alert (`interfire-ui`) | Left-nav shell; live Rules CRUD later |

The interactive ops client today is a **ratatui** terminal UI
(`crates/interfire-tui`). The desktop client, when implemented, is a **GPUI**
native Rust app under `ui/` (`interfire-ui`). There is no web UI and no hybrid
web+native shell for v0.1. Hard RSS budgets for UI clients are release gates.
Do not grow `interfirectl` into a multi-screen interactive client; scripted and
one-command checks stay on the CLI, browse/answer flows stay in the TUI (and
the desktop client once it ships).

## Design references (structure only)

- Classic **personal firewall** tray + connection alert: short decision path
  (program, destination, safe choice). See [`ux-kerio.md`](ux-kerio.md).
- **WinRoute 6.x Administration Console** chrome: left section tree, right
  content, dense Traffic Rules tables, status bar, Apply/reconnect. InterFire
  is not a gateway product: no NAT, DHCP, VPN, content filter, or licensing
  tree. Private manuals/screenshots stay out of git; shipped text never embeds
  proprietary bitmaps.

## Prompt identity (InterFire bar)

Every prompt and rule decision must make identity unmistakable:

- Full absolute executable path is always visible (never basename-only).
- Durable process identity is **PID + start ticks** from `/proc` (PID alone is
  never enough).
- Command line, uid, and cgroup appear under Details without hiding the path.
- Destination shows authoritative IPv4 and port first; hostname is optional DNS
  metadata and must never be presented as verified when the cache is stale or
  missing.
- Scopes are explicit: once | session | permanent. The UI states whether the
  answer creates or updates a rule.
- Default focus after countdown expiry is Deny. Unanswered prompts stay deny.

## Current TUI surfaces

Tabs: Status | Rules | Prompts | Log | Help. Prompt alerts show path,
destination, port, protocol, and Allow/Deny for once|session|permanent scopes.
Stale or expired prompts disable answer controls. Log is capped and
virtualized; reconnect replaces the audit subscription.

## Desktop contract

### Tray states

| State | Tray | Main window | Enforcement |
| --- | --- | --- | --- |
| Protected | enabled | normal status | rules and prompt policy active |
| Prompting | attention state | pending alert count | bounded prompt queue active |
| Degraded | warning state | reason and recovery action | documented fail-closed policy |
| Daemon unavailable | warning state | reconnect guidance | UI makes no policy claim |

### Connection alert

- Display full executable path, destination, port, protocol, and remaining time.
- Provide Allow and Deny for once|session|permanent scopes.
- Disable stale prompt controls after another client resolves or expires them.
- Keep advanced matching fields behind Details; never hide the executable path.

### Main window (rules-first)

Left navigation (WinRoute-style chrome, InterFire sections only):

```text
Status
Applications
Rules          ← primary surface
Log
Network
Settings
```

Rules is the authoritative editable policy view (dense sortable table).
Applications lists observed identities and their effective rule (thin OK at
first). Log is a capped audit stream. Network only shows InterFire-owned
nftables state. Settings exposes daemon health, socket path, retention, and
diagnostics. Desktop prompts are alert-first; the TUI retains a Prompts tab.

### Bounded rendering

- The renderer owns at most 2,000 log rows and 100 visible pending prompts.
- Log views are virtualized and subscriptions have explicit cancellation.
- No background animation is required for status.
- Reconnect replaces, rather than adds to, an existing subscription.

### RSS budgets (release gates)

| Client | Idle | Under prompt load |
| --- | --- | --- |
| GPUI `interfire-ui` | < 80 MiB | < 120 MiB |
| Combined steady (daemon + one UI) | < 150 MiB | - |
