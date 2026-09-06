# InterFire

<p align="center">
  <strong>Linux-first Rust application firewall.</strong>
</p>

<p align="center">
  <a href="https://github.com/Interchouette-ITC/InterFire/actions/workflows/ci.yml"><img src="https://github.com/Interchouette-ITC/InterFire/actions/workflows/ci.yml/badge.svg?branch=dev" alt="CI" /></a>
  <a href="https://codecov.io/gh/Interchouette-ITC/InterFire"><img src="https://codecov.io/gh/Interchouette-ITC/InterFire/branch/dev/graph/badge.svg" alt="codecov" /></a>
</p>

Linux-first **application firewall** in Rust: attribute outbound connections to
processes, match durable rules, and (when the verdict path is live) accept or
drop before the connect completes.

Canonical repo: [Interchouette-ITC/InterFire](https://github.com/Interchouette-ITC/InterFire).

**Status today:** workspace foundation and control plane only. The daemon and
CLI speak a versioned Unix-socket protocol, persist TOML rules, and resolve
process identity via `/proc`. They do **not** enforce network policy yet.
Pre-connect enforcement stays gated on the documented isolated NFQUEUE
feasibility path.

## What you get today

| Piece | Role |
| --- | --- |
| `interfire-rules` | Deterministic application-rule matching + TOML store |
| `interfire-proto` | Versioned, bounded Unix-socket framing |
| `interfired` | Daemon skeleton (rules load, process cache, IPC) |
| `interfirectl` | CLI: `ping`, `status`, `rules list` / `add` / `delete` |
| `interfire-ebpf*` | Placeholders for observation programs (not attached yet) |
| Docs | Architecture, threat model, UX contract and studies |

Still building toward: eBPF observation, NFQUEUE verdicts, tray UI, and Debian
packaging.

## Quick start

```bash
git clone https://github.com/Interchouette-ITC/InterFire.git
cd InterFire
make ci
```

Non-root smoke (daemon + CLI on a temp socket):

```bash
cargo run -p interfire-daemon -- --socket=/tmp/interfire.sock
cargo run -p interfirectl -- --socket=/tmp/interfire.sock ping
cargo run -p interfirectl -- --socket=/tmp/interfire.sock status
```

Isolated NFQUEUE accept/drop spike (root, temporary network namespace only):

```bash
sudo scripts/phase0-nfqueue-spike.sh
```

See [`architecture.md`](architecture.md) for the verdict path and how the spike
is scoped.

## Docs

| Doc | Topic |
| --- | --- |
| [`architecture.md`](architecture.md) | Event flow, NFQUEUE verdict path, baseline |
| [`threat-model.md`](threat-model.md) | Assets, trust boundaries, controls |
| [`ux-interfire.md`](ux-interfire.md) | Locked UI contract |
| [`ux-opensnitch.md`](ux-opensnitch.md) | OpenSnitch interaction study |
| [`ux-kerio.md`](ux-kerio.md) | Kerio-era interaction study |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Lint bar, Make targets, PR habits |
| [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) | Community standards |
| [`SECURITY.md`](SECURITY.md) | Vulnerability reporting |
| [`../docs-dev/README.md`](../docs-dev/README.md) | Developer docs index |
| [`api-rust/`](api-rust/) | rustdoc after `make doc` |

## Layout

```text
crates/interfire-rules/          rule matching + TOML persistence
crates/interfire-proto/          IPC version + frame bounds
crates/interfire-daemon/         interfired
crates/interfirectl/             CLI
crates/interfire-ebpf/           eBPF loader stub
crates/interfire-ebpf-programs/  eBPF program stub
docs/                            product docs (this hub)
docs-dev/                        developer notes
fixtures/                        rule fixtures
scripts/                         capability probe + NFQUEUE spike
ui/                              reserved for the tray client
packaging/debian/                reserved for packaging
```

## Contributing

1. Read [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`../docs-dev/DEVELOPMENT.md`](../docs-dev/DEVELOPMENT.md).
2. Prefer Make targets (`make ci`) over ad-hoc cargo lines.
3. One concern per PR. Commits and docs in **English**.
4. Do not claim enforcement until the verdict path is wired and measured.

## License

**Apache-2.0** (Apache License, Version 2.0). See [`../LICENSE`](../LICENSE).
