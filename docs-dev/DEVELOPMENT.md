# Development

## Layout

```text
crates/interfire-rules/          rule matching + TOML persistence
crates/interfire-proto/          IPC version + frame bounds
crates/interfire-daemon/         interfired binary
crates/interfirectl/             CLI
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

## Idle RSS (`make memcheck`)

Builds `interfired`, starts it with `--no-ebpf --no-nfqueue`, samples `VmRSS`, and fails when the sample exceeds 40960 KiB (40 MiB). Override with `INTERFIRE_MEMCHECK_BUDGET_KIB`.

## NFQUEUE spike and enforcement integration (root)

```bash
sudo scripts/phase0-nfqueue-spike.sh
sudo make integration
```

`make integration` runs `scripts/enforcement-allow-deny.sh`: temporary network namespace, InterFire-owned nft queue 4242, daemon with live observation + NFQUEUE, controlled `python3` client allow then deny. Failures print daemon/client logs. Not part of `make ci` (requires root and eBPF attach).

See [`../docs/architecture.md`](../docs/architecture.md).
