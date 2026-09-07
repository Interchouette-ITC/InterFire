# Interfire UI contract

This contract locks the initial UI behavior. A later design change requires an
explicit update to this document.

## Shell

v0.1 clients share the same Unix IPC. What exists today vs what is planned:

| Surface | Status | Role |
| --- | --- | --- |
| `interfirectl` | Shipped | One-shot commands only (`ping`, `status`, rules / prompts / dns / audit) |
| `interfire-tui` | Shipped | Interactive control plane (Status \| Rules \| Prompts \| Log \| Help) |
| GPUI app under `ui/` | Not shipped (`ui/` is empty) | Desktop tray, alerts, Kerio-style tabs |

The interactive ops client today is a **ratatui** terminal UI
(`crates/interfire-tui`). The desktop tray, when implemented, is a **GPUI**
native Rust app under `ui/`. There is no web UI and no hybrid web+native shell
for v0.1. Hard RSS budgets for UI clients are release gates. Do not grow
`interfirectl` into a multi-screen interactive client; scripted and one-command
checks stay on the CLI, browse/answer flows stay in the TUI (and the tray once
it ships).

## Current TUI surfaces

Tabs: Status | Rules | Prompts | Log | Help. Prompt alerts show path,
destination, port, protocol, and Allow/Deny for once|session|permanent scopes.
Stale or expired prompts disable answer controls. Log is capped and
virtualized; reconnect replaces the audit subscription.

## Tray contract (not shipped)

When the GPUI tray lands, these states and tabs apply. They are not live UI
today.

| State | Tray | Main window | Enforcement |
| --- | --- | --- | --- |
| Protected | enabled | normal status | rules and prompt policy active |
| Prompting | attention state | pending alert count | bounded prompt queue active |
| Degraded | warning state | reason and recovery action | documented fail-closed policy |
| Daemon unavailable | warning state | reconnect guidance | UI makes no policy claim |

### Alert rules

- Display full executable path, destination, port, protocol, and the remaining
  decision time.
- Provide Allow and Deny for one connection, session, and permanent scopes.
- Disable stale prompt controls after another client resolves or expires them.
- Keep advanced matching fields behind Details; never hide the executable path.

### Bounded rendering

- The renderer owns at most 2,000 log rows and 100 visible pending prompts.
- Log views are virtualized and subscriptions have explicit cancellation.
- No background animation is required for status.
- Reconnect replaces, rather than adds to, an existing subscription.

### Tabs

Applications lists observed identities and their effective rule. Rules is the
authoritative editable policy view. Log is a capped audit stream. Network only
shows Interfire-owned nftables state. Settings exposes daemon health, retention,
and diagnostic information.
