# Development

## Layout

```text
crates/interfire-rules/          rule matching + TOML persistence
crates/interfire-proto/          IPC version + frame bounds
crates/interfire-daemon/         interfired binary
crates/interfirectl/             one-shot CLI
crates/interfire-tui/            ratatui control-plane TUI (`interfire-tui`)
crates/interfire-ebpf/           TCP event contract + aya loader
crates/interfire-ebpf/bpf/       Embedded eBPF object (regenerate with `make ebpf`)
crates/interfire-ebpf-programs/  aya TCP-connect program (bpfel target)
docs/                            product hub + community health
docs-dev/                        developer notes (this tree)
fixtures/                        IPC and rules fixtures
scripts/                         capability probe + NFQUEUE spike
ui/                              reserved tray client
packaging/debian/                reserved packaging
.github/workflows/               CI
```

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
make memcheck       # idle interfired VmRSS vs < 40 MiB (non-root)
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

## TUI terminal hygiene

`interfire-tui` enables raw mode and the alternate screen. On normal exit it restores both. A panic hook also restores the terminal before the default panic printer runs, so a crash should not leave the tty stuck. Prefer quitting with `q` during development; do not kill `-9` the process if you can avoid it.

The TUI polls `status` on a timer and keeps a long-lived `audit-subscribe` with id `interfire-tui` (reconnect replaces that subscription). When the daemon socket is missing, the status chrome shows **daemon unavailable** instead of an empty UI. Observation and enforcement fields are shown as reported by the daemon.

Tabs: Status | Rules | Prompts | Log | Help. Left/Right or `1`..`5` change tabs; `h`/`l` move list/detail focus; `j`/`k` move the list. `Esc` dismisses an overlay and never quits from root chrome (`q` quits). Help documents the keys.

```bash
cargo run -p interfire-tui -- --socket=/tmp/interfire.sock
```

## Idle RSS (`make memcheck`)

Builds `interfired`, starts it with `--no-ebpf --no-nfqueue`, samples `VmRSS`, and fails when the sample exceeds 40960 KiB (40 MiB). Override with `INTERFIRE_MEMCHECK_BUDGET_KIB`.

## NFQUEUE spike and enforcement integration (root)

```bash
sudo scripts/phase0-nfqueue-spike.sh
sudo make integration
```

`make integration` runs `scripts/enforcement-allow-deny.sh`: temporary network namespace, InterFire-owned nft queue 4242, daemon with live observation + NFQUEUE, controlled `python3` client allow then deny. Failures print daemon/client logs. Not part of `make ci` (requires root and eBPF attach).

See [`../docs/architecture.md`](../docs/architecture.md).
