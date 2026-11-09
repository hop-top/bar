.PHONY: build test lint format check setup gallery gallery-check bench-badge bench-badge-check

check: lint test gallery-check bench-badge-check

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

# fails when the gallery no longer matches bar's output
gallery-check:
	scripts/gallery --check

# docs/badges/bench.svg from the results in bench/results/
bench-badge:
	scripts/bench-badge

# fails when the badge no longer matches the recorded results
bench-badge-check:
	scripts/bench-badge --check
