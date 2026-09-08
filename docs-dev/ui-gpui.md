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
| Profil.|  (daemon + UI RSS/CPU)                   |
| Settings|                                         |
+--------+------------------------------------------+
| section | daemon | Ready|Loading|Saving            |
+---------------------------------------------------+
```

Tray: protected | prompting | degraded | daemon-unavailable (Linux SNI via
`ksni`; state also mirrored in Status chrome and the window status bar).
On Debian GNOME, a visible tray needs a shell that exposes StatusNotifierItem;
when SNI registration fails, Status still shows tray state and must not claim
an icon is present. Pop!_OS is a primary verify target alongside Debian GNOME.
Rules: live list / select / add / delete over Unix IPC (`rule-list`,
`rule-add`, `rule-delete`). Log: capped at 2,000 rows with a virtualized
viewport; long-lived `audit-subscribe` id `interfire-ui` (reconnect
replaces; tear-down on window/host drop). Applications lists observed process
identities from the daemon cache (`v1 process-list`: absolute path, PID + start
ticks, cmdline, uid, recent destinations, effective rule) with optional open-in
htop/atop/btop/top when those binaries are on `PATH`.
Network and Settings are still thin placeholders until those surfaces deepen.

## UI tests (`make ui-test`)

Harness coverage for prompt Allow/Deny frames (including stale/expired
disable) and audit reconnect that reuses the stable `interfire-ui`
subscriber id. Run with the Linux UI packages above.

## Stack pin

| Crate | Version | Role |
| --- | --- | --- |
| `gpui-kit` | **0.6.0** (crates.io) | One dependency wrapping GPUI + components + shell |
| Transitive | `gpui-pre` 0.3.x, `gpui-component` 0.6.x | Pulled by `gpui-kit` |
| `hotpath` | **0.25.x** (crates.io) | Optional profiler; see [Memory and profiling](#memory-and-profiling) |

Pinned 2026-09-07 (`gpui-kit`). `hotpath` added 2026-09-08 for RSS investigation.
Rebuild with `make ui`. Prefer `gpui-ce` only if Linux tray work is blocked by
upstream gaps.

### Linux system packages

Linking `interfire-ui` needs development packages on the primary targets
(Debian and Pop!_OS both use `apt`):

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
- Install targets for the future `.deb`: Debian (stable) GNOME and Pop!_OS (not packaged yet)

## Memory and profiling

### Why ~190 MiB idle is expected (GPUI floor)

`make memcheck-ui` samples `/proc/<pid>/VmRSS` on a **release** `interfire-ui`
with software GL defaults (`LIBGL_ALWAYS_SOFTWARE=1`, `WGPU_BACKEND=gl`).

| Process | Typical idle VmRSS | Gate | Notes |
| --- | --- | --- | --- |
| `interfired` | ~6–7 MiB | < 40 MiB (`make memcheck`) | Enforcement path; this is the firewall budget that matters |
| `interfire-ui` | **~190 MiB** | < 220 MiB idle / < 260 MiB prompt-load | GPUI + wgpu + fonts/atlas; framework floor before InterFire tables grow |
| Combined daemon + idle UI | ~200 MiB | < 260 MiB | Dominated by the UI process |

Recorded context (2026-09): release GPUI + wgpu on Linux idles around **190 MiB**
even with empty Rules / no big audit buffer. That is **not** InterFire policy
state and **not** the daemon. Do not market the desktop client as a tiny
process. Original sketch ceilings (80 / 120 / 150 MiB for UI / combined) were
retired after measurement; gates were raised to measured ceilings with headroom,
not lowered by pretending the floor is smaller.

Order-of-magnitude context (not InterFire-measured): a minimal **Electron** app
often idles in the **~150–300 MiB** band (main + renderer + GPU processes). Real
Electron products are often higher. GPUI here sits in the **Electron-low / mid**
band, not in a GTK/Qt tray-applet band (~10–50 MiB). Cutting under ~80 MiB idle
for a full GPUI main window is unlikely without a different UI strategy
(tray-only, thinner toolkit, or deferred window).

### Release gates (`make memcheck-ui`)

```bash
make memcheck-ui
```

Needs `DISPLAY` or `xvfb-run`. Prompt-load stages 100 pending prompts, an alert,
and a full 2,000-row audit buffer inside the UI process.

| Condition | Budget | Env override |
| --- | --- | --- |
| Idle UI | < 220 MiB | `INTERFIRE_UI_IDLE_BUDGET_KIB` (default 225280) |
| Prompt-load UI | < 260 MiB | `INTERFIRE_UI_PROMPT_BUDGET_KIB` (default 266240) |
| Combined daemon + idle UI | < 260 MiB | `INTERFIRE_UI_COMBINED_BUDGET_KIB` (default 266240) |

Settle time: `INTERFIRE_UI_MEMCHECK_SETTLE_SECS` (default 4). Fail the release if
any sample exceeds budget. **Do not raise these ceilings further** to greenwash.

Product contract summary: [`../docs/ux-interfire.md`](../docs/ux-interfire.md).

### hotpath-rs (optional allocation profiling)

Upstream: [hotpath.rs](https://hotpath.rs/), blog notes at
[hotpath.rs/blog](https://hotpath.rs/blog/). Crate `hotpath` **0.25.x** (MIT).

Wired only on `interfire-ui`. Default `make ui` / packaging builds stay cold:
macros and `CountingAllocator` are pass-through unless features are enabled.

| Cargo feature | Effect |
| --- | --- |
| `hotpath` | Enable timing / instrumentation (`hotpath/hotpath`) |
| `hotpath-alloc` | Track allocations via `hotpath::CountingAllocator` (`hotpath/hotpath-alloc`) |

```bash
make profile-ui
# equivalent:
cargo build -p interfire-daemon --release
cargo build -p interfire-ui --release --features hotpath,hotpath-alloc
bash scripts/profile-ui.sh
```

`scripts/profile-ui.sh`:

1. Starts a temp daemon (`--no-ebpf --no-nfqueue`).
2. Runs release `interfire-ui --rss-probe=idle` under DISPLAY or `xvfb-run`.
3. Sets `HOTPATH_SHUTDOWN_MS` (default **8000**) so
   `HotpathGuardBuilder::build_with_shutdown` prints the report and exits.

Instrumented InterFire paths (see `#[hotpath::measure]`):

- `App::new`
- `App::apply_rss_probe`
- `prompt_load_fixture`
- `poll_snapshot`

How to read a report:

1. If instrumented InterFire functions allocate little vs ~190 MiB VmRSS, the
   floor is GPUI/wgpu (expected). Document and keep honest gates; do not claim
   a low-RSS desktop shell.
2. If InterFire paths dominate growth (prompt-load / log), cut those paths and
   only then lower `INTERFIRE_UI_*_BUDGET_KIB` defaults.
3. Never enable `hotpath` / `hotpath-alloc` in default release or `.deb` builds.

Env knobs: `HOTPATH_SHUTDOWN_MS`, `HOTPATH_ALLOC_METRIC=count` (upstream), plus
the memcheck overrides above.

## Related docs

- Product contract: [`../docs/ux-interfire.md`](../docs/ux-interfire.md)
- Kerio structure study: [`../docs/ux-kerio.md`](../docs/ux-kerio.md)
- Make index: [`DEVELOPMENT.md`](DEVELOPMENT.md)
- Private manuals/screenshots: `.cursor/refs/kerio/` (not in application git)
