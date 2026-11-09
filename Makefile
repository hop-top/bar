.PHONY: build test lint format check setup gallery gallery-check bench bench-check bench-docs bench-docs-check bench-badge bench-badge-check

check: lint test gallery-check bench-docs-check bench-badge-check

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

# benchmark in a pinned container (Docker or Podman): results, generated docs
# and badge, staged for a chore(bench) commit; last step before any release
bench:
	scripts/bench

# fails when HEAD's benchmark inputs differ from the recorded run
bench-check:
	scripts/bench-inputs --check

# generated benchmark sections of bench/README.md and README.md from bench/results/
bench-docs:
	scripts/bench-render

# fails when those sections no longer match the recorded results
bench-docs-check:
	scripts/bench-render --check

# docs/badges/bench.svg from the results in bench/results/
bench-badge:
	scripts/bench-badge

# fails when the badge no longer matches the recorded results
bench-badge-check:
	scripts/bench-badge --check
