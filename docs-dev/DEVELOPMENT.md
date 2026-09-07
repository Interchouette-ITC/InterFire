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
```

CI mirrors `make ci`, plus coverage upload to Codecov and a supply-chain job. Live eBPF attach needs root (or `CAP_BPF` / `CAP_PERFMON`) and is not required for `make ci`.

## Smoke (non-root)

```bash
cargo run -p interfire-daemon -- --socket=/tmp/interfire.sock --no-ebpf --no-nfqueue
cargo run -p interfirectl -- --socket=/tmp/interfire.sock ping
cargo run -p interfirectl -- --socket=/tmp/interfire.sock status
```

Without `--no-ebpf`, the daemon tries to attach the embedded TCP-connect program and reports `observation=attached` or `observation=degraded`. Without `--no-nfqueue`, it tries to bind NFQUEUE 4242 and reports `enforcement=nfqueue` or `enforcement=degraded`.

## NFQUEUE spike (root)

```bash
sudo scripts/phase0-nfqueue-spike.sh
```

Creates state only inside a temporary network namespace, then tears it down.
See [`../docs/architecture.md`](../docs/architecture.md).
