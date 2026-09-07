# GPUI desktop client notes

Private developer notes for the `interfire-ui` GPUI app under `ui/`.

## Goal

Linux desktop client with Kerio-feeling chrome (left nav + dense Rules) and a
personal-firewall tray/alert path. Same Unix IPC as `interfirectl` /
`interfire-tui`. Rules is the primary surface.

## Layout wireframe

```text
+--------+------------------------------------------+
| Status |  content for selected section            |
| Apps   |                                          |
| Rules* |  (Rules: dense table + detail/actions)   |
| Log    |                                          |
| Network|                                          |
| Settings|                                         |
+--------+------------------------------------------+
| section | daemon | Ready|Loading|Saving            |
+---------------------------------------------------+
```

Tray: protected | prompting | degraded | daemon-unavailable (Linux SNI via
`ksni`; state also mirrored in Status chrome and the window status bar).
Rules: live list / select / add / delete over Unix IPC (`rule-list`,
`rule-add`, `rule-delete`). Log: capped at 2,000 rows with a virtualized
viewport; long-lived `audit-subscribe` id `interfire-ui` (reconnect
replaces; tear-down on window/host drop). Other left-nav sections stay
thin shells until their dedicated slices.

## UI tests (`make ui-test`)

Harness coverage for prompt Allow/Deny frames (including stale/expired
disable) and audit reconnect that reuses the stable `interfire-ui`
subscriber id. Run with the Linux UI packages above.

## Stack pin

| Crate | Version | Role |
| --- | --- | --- |
| `gpui-kit` | **0.6.0** (crates.io) | One dependency wrapping GPUI + components + shell |
| Transitive | `gpui-pre` 0.3.x, `gpui-component` 0.6.x | Pulled by `gpui-kit` |

Pinned 2026-09-07. Rebuild with `make ui`. Prefer `gpui-ce` only if Linux tray
work is blocked by upstream gaps.

### Linux system packages

Linking `interfire-ui` needs development packages (Debian/Ubuntu):

```bash
sudo apt-get install -y \
  libxkbcommon-dev libxkbcommon-x11-dev \
  libxcb1-dev libxcb-xfixes0-dev libxcb-shape0-dev \
  libfontconfig1-dev libfreetype6-dev
```

`make test` excludes `interfire-ui` so machines without those packages stay green.
Use `make ui-test` (and CI) when the packages are present.

## Binary and packaging

- Binary name: `interfire-ui`
- Tree: [`ui/`](../ui/) (workspace member)
- Debian packaging: later packaging work; keep a single installable binary

## RSS budgets

| Condition | Budget |
| --- | --- |
| Idle | < 80 MiB |
| Under prompt load | < 120 MiB |
| Combined steady with daemon | < 150 MiB |

## Related docs

- Product contract: [`../docs/ux-interfire.md`](../docs/ux-interfire.md)
- Kerio structure study: [`../docs/ux-kerio.md`](../docs/ux-kerio.md)
- Private manuals/screenshots: `.cursor/refs/kerio/` (not in application git)
