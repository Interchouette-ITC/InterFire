# Architecture and feasibility record

## Current status: live path wired; owned nft installable

InterFire uses eBPF for TCP-connect observation, then resolves the executable,
command line, uid, cgroup, and start-time through `/proc` in userspace. eBPF
observation alone is not the verdict path.

The primary verdict path is NFQUEUE: the daemon binds queue **4242** with
**fail-open** (daemon loss must not freeze the host), stores recent connect
decisions keyed by destination IPv4 and port, and accepts or drops queued
packets while enforcement is **active**. Default mode after install is
**paused** (owned table absent). Prompt verdicts stay **deny** until answered
over IPC (`interfire-tui` or `interfirectl`). Unattributed events and packets
without a pending decision are **deny** while Active.

The InterFire-owned nftables table is **`inet interfire`**. Pause/Start
(`interfirectl pause|resume`, UI header) and Network install/remove install or
remove only that table: an `output` chain that queues new outbound TCP
(`ct state new`) to queue 4242. Unrelated tables are never listed or edited. Open gaps: production latency/coexistence measurements. A root
network-namespace gate (`make integration` /
`scripts/enforcement-allow-deny.sh`) proves controlled allow and deny. Idle
daemon RSS is checked with `make memcheck` against the < 40 MiB budget.

## Event and identity flow

1. The TCP-connect eBPF program writes a bounded `TcpConnectEvent` to a ring
   buffer (PID/TGID, destination IPv4, destination port). Kernel start ticks
   are currently zero; userspace reads `/proc/<pid>/stat` immediately.
2. The daemon enriches via `/proc` (exe, cmdline, uid, cgroup, start time),
   updates a bounded process cache, and rejects reuse when start ticks are
   present and mismatched.
3. The rules engine returns allow, deny, or prompt. Prompt maps to deny until
   a client answers via IPC (TUI or one-shot CLI). Unattributed events never
   allow.
4. The decision is stored briefly; the NFQUEUE thread applies it to matching
   IPv4 TCP packets, or drops when no match / unparseable payload.

Daemon flags: `--no-ebpf` skips observation attach; `--no-nfqueue` skips the
queue bind (status `enforcement=none`). Without caps, observation or
enforcement report `degraded`.

## Failure behavior

No UI connection must block the daemon indefinitely. A full pending table,
backend error, incompatible kernel, or missing eBPF/NFQUEUE capability
produces a visible degraded state. Missing attribution or missing pending
decision fails closed (drop).

## Observation and attribution model

InterFire separates kernel observations (eBPF TCP-connect events) from
`/proc`-based application attribution and handles process lifecycle separately.
The userspace stack is Rust end-to-end: no foreign daemon or prebuilt eBPF
objects from other projects are linked or shipped.

## Supported baseline

Primary v0.1 targets (install and desktop dogfood):

| Distro / desktop                     | Role    |
| ------------------------------------ | ------- |
| Debian (stable) with GNOME           | Primary |
| Pop!\_OS (current supported release) | Primary |

Architecture: `x86_64`. Runtime assumptions: systemd (see
[`packaging.md`](packaging.md)), a kernel with BTF and
usable eBPF features, and NFQUEUE support. CI may use Ubuntu runners as a
compile proxy only; that does not expand the support claim.

Desktop UI (`interfire-ui`) draws with GPUI. On older or weak GPUs that cannot
meet that requirement, the app **defaults to CPU software rendering** so the
window still opens (slower, more CPU). Set `INTERFIRE_UI_NATIVE_GPU=1` only on
machines known to support modern GPU drawing for GPUI. The terminal client
(`interfire-tui`) does not need a GPU.

Tray icons use StatusNotifierItem. On GNOME, a visible tray needs a shell that
exposes SNI; when registration fails, Status chrome still reports tray state
and the UI must not claim a tray is present. Broader distros and desktop
environments are out of scope until documented later.

The exact oldest kernel is deliberately uncommitted until the capability probe
and controlled verdict test pass there.

## Isolated NFQUEUE verdict command

After installing `libnetfilter-queue-dev`, run
`sudo scripts/nfqueue-spike.sh`. The test creates a temporary network
namespace, a loopback-only HTTP server, and an nftables table solely inside that
namespace. It verifies an NFQUEUE listener can accept one connection and drop
one connection, then removes the namespace and all its firewall state. Queue
number **4242** matches the daemon bind.
