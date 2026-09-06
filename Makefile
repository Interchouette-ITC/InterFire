.PHONY: test lint fmt memcheck

fmt:
	cargo fmt --check

lint:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

memcheck:
	@echo "No long-running enforcement process to measure yet."
