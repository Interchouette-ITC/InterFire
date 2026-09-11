# InterFire local gates (match CI)

CLIPPY_FLAGS := -D warnings -D clippy::all -D clippy::pedantic -D clippy::nursery
CARGO ?= cargo +stable
DOC_OUT ?= target/doc

.PHONY: help fmt format lint test coverage coverage-summary coverage-html machete outdated fuzz fuzz-build geiger audit deny doc doc-open doc-clean ci memcheck memcheck-ui profile-ui integration ebpf ui ui-test run-daemon run-ui run-tui run-ctl deb

.DEFAULT_GOAL := help

# Dev smoke socket (override: `make run-ui SOCKET=/path/to.sock`).
SOCKET ?= /tmp/interfire.sock

## Paths filtered from llvm-cov / Codecov upload (bins, BPF, live OS glue).
COVERAGE_IGNORE := scripts/|fixtures/|crates/interfire-ebpf-programs/|crates/interfire-daemon/src/main\.rs|crates/interfirectl/src/main\.rs|crates/interfire-tui/src/main\.rs|ui/src/main\.rs|nfqueue_live\.rs|observe_live\.rs|nft_live\.rs|loader_attach\.rs|loader_live\.rs

## Default fuzz target: interfire_proto::Request::parse. Override: FUZZ_TARGET=… FUZZ_TIME=…
FUZZ_TARGET ?= parse-request
FUZZ_TIME ?= 10

help:
	@echo "InterFire targets"
	@echo "  make fmt / format   cargo fmt --check / cargo fmt"
	@echo "  make lint           fmt check + clippy (workspace)"
	@echo "  make test           cargo test --workspace"
	@echo "  make coverage       cargo llvm-cov → coverage/lcov.info"
	@echo "  make coverage-summary  cargo llvm-cov --summary-only"
	@echo "  make coverage-html  cargo llvm-cov HTML → coverage/html/"
	@echo "  make machete        unused workspace deps (cargo-machete)"
	@echo "  make outdated       outdated crates report (cargo-outdated)"
	@echo "  make fuzz           cargo +nightly fuzz (default: parse-request)"
	@echo "  make fuzz-build     build fuzz targets only"
	@echo "  make geiger         unsafe surface report (cargo-geiger)"
	@echo "  make doc            rustdoc → docs/api-rust/"
	@echo "  make doc-open       build docs and open docs/api-rust/index.html"
	@echo "  make audit          cargo audit"
	@echo "  make deny           cargo deny check"
	@echo "  make ci             lint + test + doc"
	@echo "  make ui             build interfire-ui (GPUI desktop client)"
	@echo "  make ui-test        test interfire-ui (needs X11/Wayland UI libs)"
	@echo "  make run-daemon     run interfired (smoke: --no-ebpf --no-nfqueue)"
	@echo "  make run-ui         run interfire-ui (software GL default; SOCKET=$(SOCKET))"
	@echo "                      native GPU: INTERFIRE_UI_NATIVE_GPU=1 make run-ui"
	@echo "  make run-tui        run interfire-tui (SOCKET=$(SOCKET))"
	@echo "  make run-ctl        run interfirectl ping (SOCKET=$(SOCKET))"
	@echo "  make ebpf           rebuild embedded TCP-connect eBPF object (nightly)"
	@echo "  make memcheck       idle interfired RSS vs < 40 MiB budget"
	@echo "  make memcheck-ui    release UI RSS gates (idle / prompt-load / combined)"
	@echo "  make profile-ui     hotpath-alloc report for interfire-ui (optional)"
	@echo "  make integration    root netns allow/deny gate (not part of make ci)"
	@echo "  make deb            build amd64 .deb (release binaries + packaging/)"

fmt:
	$(CARGO) fmt --check
	$(CARGO) fmt --check --manifest-path crates/interfire-ebpf-programs/Cargo.toml

format:
	$(CARGO) fmt
	$(CARGO) fmt --manifest-path crates/interfire-ebpf-programs/Cargo.toml

lint: fmt
	$(CARGO) clippy --workspace --all-targets --exclude interfire-ui -- $(CLIPPY_FLAGS)
	$(CARGO) clippy -p interfire-ui --all-targets -- $(CLIPPY_FLAGS)

test:
	$(CARGO) test --workspace --exclude interfire-ui

## Requires `cargo install cargo-llvm-cov` + llvm-tools-preview. Writes coverage/lcov.info.
## Exclude interfire-ui: GPUI needs system fontconfig/xkb; covered by `make ui-test` in CI.
coverage:
	mkdir -p coverage
	RUSTUP_TOOLCHAIN=stable $(CARGO) llvm-cov --workspace --locked --exclude interfire-ui --lcov \
		--ignore-filename-regex '$(COVERAGE_IGNORE)' \
		--output-path coverage/lcov.info

## Terminal summary only (fast local check).
coverage-summary:
	RUSTUP_TOOLCHAIN=stable $(CARGO) llvm-cov --workspace --locked --summary-only \
		--exclude interfire-ui \
		--ignore-filename-regex '$(COVERAGE_IGNORE)'

## HTML report → coverage/html/.
coverage-html:
	mkdir -p coverage
	RUSTUP_TOOLCHAIN=stable $(CARGO) llvm-cov --workspace --locked --html \
		--exclude interfire-ui \
		--ignore-filename-regex '$(COVERAGE_IGNORE)' \
		--output-dir coverage/html

## Unused workspace dependencies. Requires `cargo install cargo-machete`.
machete:
	$(CARGO) machete

## Outdated crates report. Requires `cargo install cargo-outdated`.
outdated:
	$(CARGO) outdated --workspace

## Requires nightly + `cargo install cargo-fuzz`.
fuzz-build:
	cargo +nightly fuzz build

fuzz:
	cargo +nightly fuzz run $(FUZZ_TARGET) -- -max_total_time=$(FUZZ_TIME)

## Unsafe Rust surface. Requires `cargo install cargo-geiger`.
## cargo-geiger rejects virtual workspace roots; scan each package manifest.
## Advisory only: geiger exits non-zero when unsafe appears in the tree.
GEIGER_PACKAGES := crates/interfire-rules crates/interfire-proto crates/interfire-daemon crates/interfire-ebpf crates/interfirectl crates/interfire-tui ui

geiger:
	@root=$$(pwd); for dir in $(GEIGER_PACKAGES); do \
		echo "== $$dir =="; \
		$(CARGO) geiger --manifest-path "$$root/$$dir/Cargo.toml" || true; \
	done

## Requires `cargo install cargo-audit`.
audit:
	$(CARGO) audit

## Requires `cargo install cargo-deny`.
deny:
	$(CARGO) deny check

## rustdoc → `docs/api-rust/` (gitignored except README).
doc:
	RUSTDOCFLAGS='-D warnings' $(CARGO) doc --workspace --no-deps --exclude interfire-ui
	@test -d "$(DOC_OUT)" || (echo "missing $(DOC_OUT)"; exit 1)
	@rm -rf docs/api-rust
	@mkdir -p docs/api-rust
	@cp -a "$(DOC_OUT)/." docs/api-rust/
	@printf '%s\n' \
		'# Rust API documentation (rustdoc)' \
		'' \
		'Generate with `make doc`, then open [`index.html`](index.html).' \
		'' \
		'Workspace crates include `interfire-rules`, `interfire-proto`, `interfire-daemon`' \
		'(`interfired`), `interfirectl`, `interfire-tui`, and `interfire-ebpf` (loader). The BPF program' \
		'crate is a separate Cargo workspace under `crates/interfire-ebpf-programs/` (built with `make ebpf`,' \
		'not rustdoc).' \
		> docs/api-rust/README.md
	@touch docs/api-rust/.nojekyll

doc-open: doc
	xdg-open docs/api-rust/index.html >/dev/null 2>&1 || open docs/api-rust/index.html >/dev/null 2>&1 || true

doc-clean:
	rm -rf docs/api-rust
	mkdir -p docs/api-rust
	@printf '%s\n' \
		'# Rust API documentation (rustdoc)' \
		'' \
		'Generate with `make doc`, then open [`index.html`](index.html).' \
		> docs/api-rust/README.md

ci: lint test doc

## GPUI desktop client (needs libxkbcommon-x11-dev and related system libs).
ui:
	$(CARGO) build -p interfire-ui

ui-test:
	$(CARGO) test -p interfire-ui

## Smoke run aliases (foreground). Override socket: `make run-ui SOCKET=/path/to.sock`.
## `interfire-ui` defaults to CPU software GL; use `INTERFIRE_UI_NATIVE_GPU=1` for native GPU.
run-daemon:
	$(CARGO) run -p interfire-daemon -- --socket=$(SOCKET) --no-ebpf --no-nfqueue

run-ui:
	$(CARGO) run -p interfire-ui -- --socket=$(SOCKET)

run-tui:
	$(CARGO) run -p interfire-tui -- --socket=$(SOCKET)

run-ctl:
	$(CARGO) run -p interfirectl -- --socket=$(SOCKET) ping

## Rebuild `crates/interfire-ebpf/bpf/interfire-ebpf-programs` (needs nightly + bpf-linker).
## BPF sources live in a separate Cargo workspace under crates/interfire-ebpf-programs/.
ebpf:
	cargo +nightly build -Z build-std=core --target bpfel-unknown-none --release \
		--manifest-path crates/interfire-ebpf-programs/Cargo.toml
	cp -f crates/interfire-ebpf-programs/target/bpfel-unknown-none/release/interfire-ebpf-programs \
		crates/interfire-ebpf/bpf/interfire-ebpf-programs

memcheck:
	$(CARGO) build -p interfire-daemon
	bash scripts/memcheck-daemon.sh

## Release gate: GPUI UI RSS (needs DISPLAY or xvfb-run). Not part of `make ci`.
memcheck-ui:
	$(CARGO) build -p interfire-daemon -p interfire-ui --release
	bash scripts/memcheck-ui.sh

## Optional hotpath-rs alloc report for interfire-ui (not a CI gate).
profile-ui:
	$(CARGO) build -p interfire-daemon --release
	$(CARGO) build -p interfire-ui --release --features hotpath,hotpath-alloc
	bash scripts/profile-ui.sh

## Root-only: controlled allow/deny in a temporary network namespace.
integration:
	$(CARGO) build -p interfire-daemon -p interfirectl
	bash scripts/enforcement-allow-deny.sh

## Build amd64 .deb under target/debian/ (needs dpkg-deb, fakeroot).
deb:
	bash scripts/build-deb.sh
