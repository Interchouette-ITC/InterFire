# InterFire local gates (match CI)

CLIPPY_FLAGS := -D warnings -D clippy::all -D clippy::pedantic -D clippy::nursery
CARGO ?= cargo +stable
DOC_OUT ?= target/doc

.PHONY: help fmt format lint test coverage audit deny doc doc-open doc-clean ci memcheck ebpf

.DEFAULT_GOAL := help

help:
	@echo "InterFire targets"
	@echo "  make fmt / format   cargo fmt --check / cargo fmt"
	@echo "  make lint           fmt check + clippy (workspace)"
	@echo "  make test           cargo test --workspace"
	@echo "  make coverage       cargo llvm-cov → coverage/lcov.info"
	@echo "  make doc            rustdoc → docs/api-rust/"
	@echo "  make doc-open       build docs and open docs/api-rust/index.html"
	@echo "  make audit          cargo audit"
	@echo "  make deny           cargo deny check"
	@echo "  make ci             lint + test + doc"
	@echo "  make ebpf           rebuild embedded TCP-connect eBPF object (nightly)"
	@echo "  make memcheck       placeholder until enforcement runs"

fmt:
	$(CARGO) fmt --check

format:
	$(CARGO) fmt

lint: fmt
	$(CARGO) clippy --workspace --all-targets --exclude interfire-ebpf-programs -- $(CLIPPY_FLAGS)

test:
	$(CARGO) test --workspace

## Requires cargo-llvm-cov + llvm-tools-preview. Writes coverage/lcov.info.
coverage:
	mkdir -p coverage
	RUSTUP_TOOLCHAIN=stable $(CARGO) llvm-cov --workspace --lcov \
		--ignore-filename-regex 'scripts/|fixtures/|crates/interfire-ebpf-programs/|crates/interfire-daemon/src/main\.rs|crates/interfirectl/src/main\.rs' \
		--output-path coverage/lcov.info

## Requires `cargo install cargo-audit`.
audit:
	$(CARGO) audit

## Requires `cargo install cargo-deny`.
deny:
	$(CARGO) deny check

## rustdoc → `docs/api-rust/` (gitignored except README).
doc:
	RUSTDOCFLAGS='-D warnings' $(CARGO) doc --workspace --no-deps --exclude interfire-ebpf-programs
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
		'(`interfired`), `interfirectl`, and `interfire-ebpf` (loader). The BPF program' \
		'crate is built with `make ebpf`, not rustdoc.' \
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

## Rebuild `crates/interfire-ebpf/bpf/interfire-ebpf-programs` (needs nightly + bpf-linker).
ebpf:
	cargo +nightly build -Z build-std=core --target bpfel-unknown-none \
		-p interfire-ebpf-programs --release
	cp -f target/bpfel-unknown-none/release/interfire-ebpf-programs \
		crates/interfire-ebpf/bpf/interfire-ebpf-programs

memcheck:
	@echo "No long-running enforcement process to measure yet."
