# Interfire UI contract

This contract locks the initial UI behavior. A later design change requires an
explicit update to this document.

## States

| State | Tray | Main window | Enforcement |
| --- | --- | --- | --- |
| Protected | enabled | normal status | rules and prompt policy active |
| Prompting | attention state | pending alert count | bounded prompt queue active |
| Degraded | warning state | reason and recovery action | documented fail-closed policy |
| Daemon unavailable | warning state | reconnect guidance | UI makes no policy claim |

## Alert rules

- Display full executable path, destination, port, protocol, and the remaining
  decision time.
- Provide Allow and Deny for one connection, session, and permanent scopes.
- Disable stale prompt controls after another client resolves or expires them.
- Keep advanced matching fields behind Details; never hide the executable path.

## Bounded rendering

- The renderer owns at most 2,000 log rows and 100 visible pending prompts.
- Log views are virtualized and subscriptions have explicit cancellation.
- No background animation is required for status.
- Reconnect replaces, rather than adds to, an existing subscription.

## Tabs

Applications lists observed identities and their effective rule. Rules is the
authoritative editable policy view. Log is a capped audit stream. Network only
shows Interfire-owned nftables state. Settings exposes daemon health, retention,
and diagnostic information.

