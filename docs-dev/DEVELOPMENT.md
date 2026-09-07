# Development

## Layout

Tree overview: [`../docs/README.md`](../docs/README.md) (Layout). Extra developer paths:

```text
crates/interfire-ebpf/bpf/       Embedded eBPF object (regenerate with `make ebpf`)
.github/workflows/               CI
```

## Supported desktops (v0.1)

Dogfood and packaging gates target **Debian (stable) with GNOME** and
**Pop!_OS** (current supported), `x86_64`. See
[`../docs/architecture.md`](../docs/architecture.md) (Supported baseline).
Build and link notes for `interfire-ui` are in [`ui-gpui.md`](ui-gpui.md).

## Gates

```bash
make fmt
make lint           # excludes interfire-ebpf-programs (BPF target only)
make test
make doc            # writes docs/api-rust/ (gitignored except README)
make coverage       # needs cargo-llvm-cov + llvm-tools-preview
make audit          # needs cargo-audit
make deny           # needs cargo-deny; config deny.toml
make ci             # lint + test + doc
make ebpf           # nightly + bpf-linker; refreshes embedded object
make ui             # build interfire-ui (needs GPUI system libs; see ui-gpui.md)
make ui-test        # test interfire-ui (same system libs)
make memcheck       # idle interfired VmRSS vs < 40 MiB (non-root)
make memcheck-ui    # release UI RSS gates (needs DISPLAY or xvfb-run)
make integration    # root netns allow/deny (not in make ci)
```

CI mirrors `make ci`, plus coverage upload to Codecov and a supply-chain job. Live eBPF attach needs root (or `CAP_BPF` / `CAP_PERFMON`) and is not required for `make ci`.

## Smoke (non-root)

```bash
cargo run -p interfire-daemon -- --socket=/tmp/interfire.sock --no-ebpf --no-nfqueue
cargo run -p interfirectl -- --socket=/tmp/interfire.sock ping
cargo run -p interfirectl -- --socket=/tmp/interfire.sock status
cargo run -p interfirectl -- --socket=/tmp/interfire.sock prompts list
```

Without `--no-ebpf`, the daemon tries to attach the embedded TCP-connect program and reports `observation=attached` or `observation=degraded`. Without `--no-nfqueue`, it tries to bind NFQUEUE 4242 and reports `enforcement=nfqueue` or `enforcement=degraded`.

Default rule miss is **prompt**: the daemon enqueues a bounded pending prompt (cap `MAX_PENDING_PROMPTS`), drops the packet, and exposes `prompt-list` / `prompt-answer` over IPC for the TUI (and one-shot `interfirectl prompts …`). Full queue or expiry stays deny. Answer scopes: `once` | `session` | `permanent`.

DNS names are optional metadata: `dns-note` / `dns-list` (and `interfirectl dns …`) feed a bounded TTL cache (`MAX_DNS_ENTRIES`). Fresh names may annotate connections for hostname rules; **stale or missing names stay unset** so the destination IP remains authoritative.

Audit: capped on-disk log (`--audit=PATH`, default under `/var/lib/interfire/audit.log`, `MAX_AUDIT_FILE_BYTES`) plus in-memory ring (`MAX_LOG_RECORDS_PER_SUBSCRIBER`). `audit-tail` is one-shot; `audit-subscribe ID [since=N]` streams frames and **replaces** any prior subscription with the same `ID` on reconnect.

## CLI vs TUI

| Client | Role |
| --- | --- |
| `interfirectl` | **One-shot** only: `ping`, `status`, single `rules` / `prompts` / `dns` / `audit` commands. No REPL, no multi-screen browse loop. |
| `interfire-tui` | Interactive control plane: tabs Status \| Rules \| Prompts \| Log \| Help, overlays for add-rule and answer-prompt. |
| `interfire-ui` (`ui/`) | GPUI desktop shell: tray, alert, Rules, Log, RSS gates (same IPC). |

Use the CLI from scripts and smoke checks. Use the TUI when you need to browse lists, answer prompts, or watch the log. The UX contract (`docs/ux-interfire.md`) locks this split.

## TUI terminal hygiene

`interfire-tui` enables raw mode and the alternate screen. On normal exit it restores both. A panic hook also restores the terminal before the default panic printer runs, so a crash should not leave the tty stuck. Prefer quitting with `q` during development; do not kill `-9` the process if you can avoid it.

The TUI polls `status` on a timer and keeps a long-lived `audit-subscribe` with id `interfire-tui` (reconnect replaces that subscription). The Log tab keeps at most `MAX_LOG_RECORDS_PER_SUBSCRIBER` (2,000) rows and renders a virtualized viewport around the selection. When the daemon socket is missing, the status chrome shows **daemon unavailable** instead of an empty UI. Observation and enforcement fields are shown as reported by the daemon.

Tabs: Status | Rules | Prompts | Log | Help. Left/Right or `1`..`5` change tabs; `h`/`l` move list/detail focus; `j`/`k` move the list. On Rules: `a` opens add overlay, `d` deletes the selected rule, `r` refreshes via IPC. On Prompts: `a`/`Enter` opens the answer overlay (Allow/Deny + once|session|permanent); expired/stale prompts disable answer. `Esc` dismisses an overlay and never quits from root chrome (`q` quits). Help documents the keys.

```bash
cargo run -p interfire-tui -- --socket=/tmp/interfire.sock
```

## Idle RSS (`make memcheck`)

Builds `interfired`, starts it with `--no-ebpf --no-nfqueue` and a temp
`--audit=` path, samples `VmRSS`, and fails when the sample exceeds 40960 KiB
(40 MiB). Override with `INTERFIRE_MEMCHECK_BUDGET_KIB`.

## UI RSS release gates (`make memcheck-ui`)

Builds release `interfired` + `interfire-ui`, samples idle / prompt-load /
combined `VmRSS`, and fails when any sample exceeds the ceilings in
[`ui-gpui.md`](ui-gpui.md). Requires `DISPLAY` or `xvfb-run`. Not part of
`make ci`; required before a tagged release.

## NFQUEUE isolated test and enforcement integration (root)

```bash
sudo scripts/nfqueue-spike.sh
sudo make integration
```

`make integration` runs `scripts/enforcement-allow-deny.sh`: temporary network namespace, InterFire-owned nft queue 4242, daemon with live observation + NFQUEUE, controlled `python3` client allow then deny. Failures print daemon/client logs. Not part of `make ci` (requires root and eBPF attach).

See [`../docs/architecture.md`](../docs/architecture.md).
