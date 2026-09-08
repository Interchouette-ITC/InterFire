# InterFire

<p align="center">
  <img src="docs/brand/logo-banner-readme.png" alt="InterFire: FIREWALL · SECURE · CONTROL" width="560" />
</p>

<p align="center">
  <img src="docs/brand/seal-gh-dark-128.png#gh-dark-mode-only" alt="InterFire seal" width="72" height="72" />
  <img src="docs/brand/seal-gh-light-128.png#gh-light-mode-only" alt="InterFire seal" width="72" height="72" />
</p>

Linux-first Rust application firewall: eBPF observation, `/proc` attribution, and
NFQUEUE allow/deny (operator nft + caps required). Interactive control plane is
`interfire-tui`; desktop client is `interfire-ui` (GPUI tray, alert, Rules,
Applications, Log, Profiling). Debian packaging is not shipped.

<p align="center">
  <a href="https://github.com/Interchouette-ITC/InterFire/actions/workflows/ci.yml"><img src="https://github.com/Interchouette-ITC/InterFire/actions/workflows/ci.yml/badge.svg?branch=dev" alt="CI" /></a>
  <a href="https://codecov.io/gh/Interchouette-ITC/InterFire"><img src="https://codecov.io/gh/Interchouette-ITC/InterFire/branch/dev/graph/badge.svg" alt="codecov" /></a>
</p>

**Overview, run instructions, and docs index:** [`docs/README.md`](docs/README.md)

**Brand assets:** [`docs/brand/`](docs/brand/)

**Contributing:** [`docs/CONTRIBUTING.md`](docs/CONTRIBUTING.md)

**Developer notes:** [`docs-dev/README.md`](docs-dev/README.md)

**API rustdoc** (after `make doc`): [`docs/api-rust/`](docs/api-rust/)
