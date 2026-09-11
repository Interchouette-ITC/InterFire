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

| Target                  | Action                                                                                                |
| ----------------------- | ----------------------------------------------------------------------------------------------------- |
| `make fmt`              | `cargo fmt --check`                                                                                   |
| `make lint`             | fmt check + clippy (pedantic / nursery deny; excludes BPF program crate)                              |
| `make test`             | Workspace tests                                                                                       |
| `make coverage`         | `cargo llvm-cov` → `coverage/lcov.info` (CI / Codecov)                                                |
| `make coverage-summary` | `cargo llvm-cov --summary-only` (local)                                                               |
| `make coverage-html`    | `cargo llvm-cov` HTML → `coverage/html/` (local)                                                      |
| `make machete`          | Unused workspace deps (`cargo machete`; local)                                                        |
| `make outdated`         | Outdated crates report (`cargo outdated`; local)                                                      |
| `make fuzz`             | `cargo +nightly fuzz` (default target `parse-request`: `Request::parse`; local)                       |
| `make geiger`           | Unsafe surface report (`cargo geiger`; local)                                                         |
| `make audit`            | `cargo audit`                                                                                         |
| `make deny`             | `cargo deny check`                                                                                    |
| `make doc`              | rustdoc → `docs/api-rust/` (excludes `interfire-ui` and the BPF program crate)                        |
| `make ci`               | `lint` + `test` + `doc`                                                                               |
| `make ui`               | Build `interfire-ui` (needs GPUI system libs; see [`../docs-dev/ui-gpui.md`](../docs-dev/ui-gpui.md)) |
| `make ui-test`          | Test `interfire-ui` (same system libs; also run in CI)                                                |
| `make run-daemon`       | Smoke `interfired` (`--no-ebpf --no-nfqueue`)                                                         |
| `make run-ui`           | Run `interfire-ui` (CPU software GL by default; `INTERFIRE_UI_NATIVE_GPU=1` for native)               |
| `make run-tui`          | Run `interfire-tui`                                                                                   |
| `make run-ctl`          | `interfirectl ping` smoke                                                                             |
| `make ebpf`             | Rebuild embedded eBPF object (nightly + bpf-linker)                                                   |
| `make memcheck`         | Idle `interfired` RSS vs < 40 MiB (non-root)                                                          |
| `make memcheck-ui`      | Release UI RSS gates (idle / prompt-load / combined)                                                  |
| `make profile-ui`       | Optional [hotpath](https://hotpath.rs/) alloc report for `interfire-ui`                               |
| `make integration`      | Root netns allow/deny gate (not in `make ci`)                                                         |
| `make deb`              | Build amd64 `.deb` (release binaries + packaging/; needs `dpkg-deb`, `fakeroot`)                      |

Local hygiene install notes: `cargo-llvm-cov`, `cargo-machete`, `cargo-outdated`, `cargo-geiger`, and (for fuzz) nightly plus `cargo-fuzz`.


Extra operator scripts and smoke recipes: [`../docs-dev/DEVELOPMENT.md`](../docs-dev/DEVELOPMENT.md).

## Questions

Open a GitHub issue on [Interchouette-ITC/InterFire](https://github.com/Interchouette-ITC/InterFire).
