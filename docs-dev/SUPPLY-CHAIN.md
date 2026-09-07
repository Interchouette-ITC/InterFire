# Supply chain

## Local

| Command | Tool |
| --- | --- |
| `make audit` | [`cargo-audit`](https://crates.io/crates/cargo-audit) |
| `make deny` | [`cargo-deny`](https://crates.io/crates/cargo-deny) via [`deny.toml`](../deny.toml) |

Install once:

```bash
cargo install --locked cargo-audit --version 0.22.0
cargo install --locked cargo-deny --version 0.20.2
```

## CI

The `supply-chain` job runs `make audit && make deny` on every PR and on pushes
to `dev`.

`deny.toml` allows additional licenses pulled by the GPUI stack (**CC0-1.0**,
**MPL-2.0**, **0BSD**, **bzip2-1.0.6**) and **Unlicense** for the Linux tray
crate `ksni`. Unmaintained advisories fail only for direct workspace crates
(`unmaintained = "workspace"`); yanked stays warn.

## Dependabot

[`.github/dependabot.yml`](../.github/dependabot.yml) opens weekly PRs for Cargo
and GitHub Actions (non-major updates, limited concurrency).
