# InterFire UI contract

This contract locks the initial UI behavior. A later design change requires an
explicit update to this document.

## Shell

v0.1 clients share the same Unix IPC:

| Surface | Status | Role |
| --- | --- | --- |
| `interfirectl` | Shipped | One-shot commands only (`ping`, `status`, `pause`, `resume`, `traffic status|block|unblock`, rules / prompts / dns / audit / network / stats) |
| `interfire-tui` | Shipped | Interactive control plane (Events \| Rules \| Prompts \| Help + filter/footer subset) |
| GPUI app under `ui/` | Shipped (`interfire-ui`: tray, alert, network statistics shell, RSS gates) | - |

The interactive ops client is a **ratatui** terminal UI (`crates/interfire-tui`).
The desktop client is a **GPUI** native Rust app under `ui/` (`interfire-ui`).
There is no web UI and no hybrid web+native shell for v0.1. Hard RSS budgets
for UI clients are release gates. Do not grow `interfirectl` into a multi-screen
interactive client; scripted and one-command checks stay on the CLI,
browse/answer flows stay in the TUI and desktop UI.

Primary desktop targets: Debian (stable) with GNOME, and Pop!_OS. Tray uses
StatusNotifierItem; when the shell does not expose SNI, Status chrome still
reports state.

## Design references (structure only)

- Classic **personal firewall** tray + connection alert: short decision path
  (program, destination, safe choice).
- Network **statistics shell** chrome: top toolbar, horizontal primary tabs,
  dense tables, shared filter strip, rich footer. InterFire is not a gateway
  product: no NAT, DHCP, VPN, content filter, licensing tree, or multi-node
  mesh. Private manuals/screenshots stay out of the application tree; shipped
  text never embeds proprietary bitmaps.

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

Tabs: Events | Rules | Prompts | Help (plus Status summary on Events/footer).
Prompt alerts show path, destination, port, protocol, and Allow/Deny for
once|session|permanent scopes. Stale or expired prompts disable answer
controls. Events is capped and virtualized; reconnect replaces the audit
subscription. Filter strip (text + All/Allow/Deny/Prompt + limit + Clear) and
footer counters (Connections, Denied, Uptime, Rules, Version) match the
desktop contract at a thinner parity.

## Desktop contract

### Tray states

| State | Tray | Main window | Enforcement |
| --- | --- | --- | --- |
| Protected | enabled | normal status | rules and prompt policy active |
| Prompting | attention state | pending alert count | bounded prompt queue active |
| Degraded | warning state | reason and recovery action | queue or observation degraded |
| Paused | warning state | Firewall: Paused + Start | owned table absent; network unfiltered |
| Daemon unavailable | muted orange tray warning | reconnect guidance | UI makes no policy claim |

### Connection alert

- Display full executable path, destination, port, protocol, and remaining time.
- Provide Allow and Deny for once|session|permanent scopes.
- Disable stale prompt controls after another client resolves or expires them.
- Keep advanced matching fields behind Details; never hide the executable path.

### Main window (network statistics shell)

Top toolbar + horizontal primary tabs (left nav is retired as primary chrome):

```text
Toolbar: App menu | Preferences | Add rule | Rules Active/Paused | Daemon | Traffic
Tabs:    Events | Daemon | Rules | Hosts | Applications | Addresses | Ports | Users
         (+ filter strip on list tabs)
Footer:  Connections | Denied | Uptime | Rules | Version (+ git)
```

**App menu:** Open / Quit / Preferences / About (crate version + short git
describe) / Traffic… / Network… / Profiling….

**Preferences:** socket path, tray label, theme, and prompt defaults when the
daemon exposes them (English labels).

**Verdicts:** `allow` | `deny` | `prompt` only. Filter uses those plus **All**.
There is no reject verdict.

**Denied (footer):** count of deny verdicts observed since daemon start (not an
invented TCP abandon counter).

**Daemon tab:** one local daemon row (socket, version, uptime, enforcement,
traffic, rules). No remote node mesh.

**Events:** capped audit / connect stream (formerly Log).

**Rules:** authoritative editable policy view (dense table); Add rule from the
toolbar.

**Hosts / Applications / Addresses / Ports / Users:** bounded in-memory
aggregates from the daemon (hit counts keyed by host, executable, dest IP,
dest port, UID), updated on each verdicted connect, via read-only stats IPC.

**Applications** also keeps process identity detail (full path, PID + start
ticks, cmdline, uid, recent dest:port). Optional "Open in …" launches host
tools on `PATH` (`htop`, `atop`, `btop`, `top`) when available.

**Traffic / Network / Profiling:** secondary surfaces from the app menu (not
eight more primary tabs). Network shows InterFire-owned nftables state.
Profiling shows live daemon and `interfire-ui` RSS/CPU.

Desktop prompts stay alert-first; the TUI retains a Prompts tab.

### Filter strip (list tabs)

- Text filter (path, host, IP, port, user).
- Verdict filter: **All | Allow | Deny | Prompt** (not Reject).
- Result limit: **50 | 100 | 200 | 300 | All | Custom** (All/Custom hard-capped,
  e.g. 2000, matching audit subscriber limits).
- **Clear** resets filter and limit to defaults.
- Show `shown / total` result counts.

### Operator controls (header and tray)

Three orthogonal controls in the header (summary chips only):

| Control | Summary states | Action |
| --- | --- | --- |
| Daemon | Running / Stopped | Stop/Start via `pkexec systemctl` (polkit) |
| Rules | Active / Paused | `v1 pause` / `v1 resume` (group `interfire`) |
| Traffic | Open / User… / Machine… | Opens the Traffic panel (detail below) |

**Traffic panel** (GPUI **Traffic…** from app menu; tray **Traffic…**): choose scope (**This user** or **Entire machine**) and direction (**Outbound** / **Inbound** / **All**), then Block or Unblock. Machine actions confirm and run `pkexec interfirectl …`. Stored machine and user preferences persist independently; machine block outranks user without clearing the user preference. Effective filter when both open follows Rules (queue or absent). User inbound only affects sockets owned by that UID. Loopback is never queued or dropped; established/related are accepted; new TCP only.

**CLI:**

```text
interfirectl traffic status
interfirectl traffic block  --scope=user|machine --direction=out|in|all
interfirectl traffic unblock --scope=user|machine
```

Default CLI scope is `user`. Machine scope requires root (`pkexec interfirectl traffic block --scope=machine …`).

**TUI:** Status shows `traffic_machine` / `traffic_user` / `traffic_effective`. Key `t` opens a Traffic overlay (scope, direction, block/unblock). Shortcuts `b` / `B` / `u` block or unblock **user** scope. Machine changes need the CLI under `pkexec` (TUI does not elevate). Daemon Stop/Start stays on GPUI / `systemctl`.

Pause rules is not the same as Block traffic. Stopping the daemon does not clear
a Traffic Block. Loopback is never queued and never dropped.

### Bounded rendering

- The renderer owns at most 2,000 log rows and 100 visible pending prompts.
- Log views are virtualized and subscriptions have explicit cancellation.
- No background animation is required for status.
- Reconnect replaces, rather than adds to, an existing subscription.

### RSS budgets (release gates)

Release profile, sampled with `make memcheck-ui` (DISPLAY or `xvfb-run`,
software GL). GPUI + wgpu baseline on Linux is about **190 MiB** idle; the
original sketch ceilings (80 / 120 / 150 MiB) are retired. That floor is the
desktop toolkit, not `interfired` (daemon idle stays under 40 MiB). The desktop
**Profiling** section shows live daemon + UI RSS/CPU. Full benchmark notes and
optional `make profile-ui` ([hotpath-rs](https://hotpath.rs/)):
[`../docs-dev/ui-gpui.md`](../docs-dev/ui-gpui.md).

| Client | Idle | Under prompt load |
| --- | --- | --- |
| GPUI `interfire-ui` | < 220 MiB | < 260 MiB |
| Combined steady (daemon + one UI) | < 260 MiB | - |

Override ceilings with `INTERFIRE_UI_IDLE_BUDGET_KIB`,
`INTERFIRE_UI_PROMPT_BUDGET_KIB`, and `INTERFIRE_UI_COMBINED_BUDGET_KIB`.
A tagged release must pass `make memcheck-ui`.
