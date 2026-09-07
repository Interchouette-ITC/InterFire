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

Tray: protected | prompting | degraded | daemon-unavailable.
Alert: modal over tray/main with countdown and once|session|permanent.

## Stack pin

Not pinned yet. Target: Zed `gpui` plus a Linux-friendly component kit
(`gpui-component` evaluation). Record exact crate versions here when the
scaffold PR lands. Prefer `gpui-ce` only if tray/platform gaps block shipping.

## Binary and packaging

- Binary name: `interfire-ui`
- Tree: `ui/` (Cargo package; workspace membership decided at scaffold)
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
