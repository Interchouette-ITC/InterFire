# Contributing to InterFire

## Code changes

- Prefer one concern per PR.
- Run `make lint`, `make test`, and `make doc` before push (or `make ci`).
- Conventional commits: `feat: …`, `fix: …`, `docs: …`, `ci: …`, etc.
- PR body follows [`pull_request_template.md`](pull_request_template.md) (**Summary** + **Test plan** only).
- Follow the [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) and [`SECURITY.md`](SECURITY.md).
- Keep enforcement claims honest: the NFQUEUE path is wired; do not imply
  production-ready protection without operator nft, caps, and measured behavior
  on supported kernels. See [`architecture.md`](architecture.md).

## Local gates

| Target | Action |
| --- | --- |
| `make fmt` | `cargo fmt --check` |
| `make lint` | fmt check + clippy (pedantic / nursery deny; excludes BPF program crate) |
| `make test` | Workspace tests |
| `make coverage` | `cargo llvm-cov` → `coverage/lcov.info` |
| `make audit` | `cargo audit` |
| `make deny` | `cargo deny check` |
| `make doc` | rustdoc → `docs/api-rust/` |
| `make ci` | `lint` + `test` + `doc` |
| `make ebpf` | Rebuild embedded eBPF object (nightly + bpf-linker) |
| `make memcheck` | Idle `interfired` RSS vs < 40 MiB (non-root) |
| `make memcheck-ui` | Release UI RSS gates (idle / prompt-load / combined) |
| `make integration` | Root netns allow/deny gate (not in `make ci`) |

Extra operator scripts and smoke recipes: [`../docs-dev/DEVELOPMENT.md`](../docs-dev/DEVELOPMENT.md).

## Questions

Open a GitHub issue on [Interchouette-ITC/InterFire](https://github.com/Interchouette-ITC/InterFire).
