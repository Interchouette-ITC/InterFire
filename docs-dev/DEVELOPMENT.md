# Development

## Layout

```text
crates/interfire-rules/          rule matching + TOML persistence
crates/interfire-proto/          IPC version + frame bounds
crates/interfire-daemon/         interfired binary
crates/interfirectl/             CLI
crates/interfire-ebpf/           eBPF loader stub
crates/interfire-ebpf-programs/  eBPF program stub
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
```

CI mirrors `make ci`, plus coverage upload to Codecov and a supply-chain job.

## Smoke (non-root)

```bash
cargo run -p interfire-daemon -- --socket=/tmp/interfire.sock
cargo run -p interfirectl -- --socket=/tmp/interfire.sock ping
```

## NFQUEUE spike (root)

```bash
sudo scripts/phase0-nfqueue-spike.sh
```

Creates state only inside a temporary network namespace, then tears it down.
See [`../docs/architecture.md`](../docs/architecture.md).
