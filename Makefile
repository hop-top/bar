.PHONY: plugin-check npm-check build test lint format check setup web web-check gallery gallery-check bench bench-check bench-docs bench-docs-check bench-badge bench-badge-check

check: lint test plugin-check npm-check web-check gallery-check bench-docs-check bench-badge-check

build:
	cargo build --release

test:
	cargo test --locked --features test-tools

lint:
	cargo fmt --all -- --check
	cargo clippy --all-targets -- -D warnings

format:
	cargo fmt --all

setup:
	cargo fetch

# validates the Claude Code plugin and marketplace manifests; skipped without the claude CLI
plugin-check:
	@if command -v claude >/dev/null 2>&1; then \
		claude plugin validate . && claude plugin validate claude-plugin; \
	else \
		python3 -c "import json;[json.load(open(f)) for f in ('.claude-plugin/marketplace.json','claude-plugin/.claude-plugin/plugin.json')]" && echo "claude CLI not found: JSON syntax checked only"; \
	fi

# npm packages (packaging/npm/): staged from fake archives, packed and run; skipped without node
npm-check:
	scripts/npm-check

# web configurator: bar's renderer as wasm in web/pkg/ (docs/web.md)
web:
	scripts/web

# builds and lints the wasm crate; skips when the toolchain has no wasm32 target
web-check:
	scripts/web --check

# docs/gallery.md and docs/gallery/*.svg from real bar output
gallery:
	scripts/gallery

# fails when the gallery no longer matches bar's output
gallery-check:
	scripts/gallery --check

# benchmark in a pinned container (Docker or Podman): results, generated docs
# and badge, staged for a chore(bench) commit; last step before any release.
# BENCH_RUNNER=native: same run in a prepared Linux environment (bench/README.md)
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
