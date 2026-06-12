.PHONY: build test lint fmt fmt-check check

build:
	cargo build

test:
	cargo nextest run

lint:
	cargo clippy --all-targets -- -D warnings

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

check: fmt-check lint test
