.PHONY: build test lint format check setup gallery

check: lint test

build:
	cargo build --release

test:
	cargo test --locked

lint:
	cargo fmt --all -- --check
	cargo clippy --all-targets -- -D warnings

format:
	cargo fmt --all

setup:
	cargo fetch

# docs/gallery.md and docs/gallery/*.svg from real bar output
gallery:
	scripts/gallery
