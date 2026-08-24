.PHONY: build test lint fmt fmt-check check release-patch release-minor release-major

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

release-patch:
	vership bump patch

release-minor:
	vership bump minor

release-major:
	vership bump major
