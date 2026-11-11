.PHONY: quality audit unused coverage dupes mutation plugin-check npm-check tray-size build test lint format check setup web web-check gallery gallery-check bench bench-check bench-docs bench-docs-check bench-badge bench-badge-check

check: lint test plugin-check npm-check web-check gallery-check bench-docs-check bench-badge-check

build:
	cargo build --release --workspace

test:
	cargo test --locked --workspace --features hop-bar/test-tools,bar-cliproxyapi/test-tools

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings

format:
	cargo fmt --all

setup:
	cargo fetch

# Line-coverage floor for `make coverage`, whole workspace. Raise it as
# tests land; never lower it to get a change through.
COVERAGE_MIN ?= 94

# Engineering gates beyond check: advisories and licenses, unused
# dependencies, a coverage floor and copy-paste. Tools are pinned in
# mise.toml (`mise install`). Dead code needs no extra tool: clippy
# -D warnings already fails on it. There is no `pkgcheck`: every crate is
# `publish = false`, so `cargo package` has nothing to check; the release
# archives and npm packages are checked by `npm-check` and the release
# workflow.
quality: audit unused coverage dupes

audit:
	cargo deny --workspace check

unused:
	cargo machete

# needs the llvm-tools component of a rustup toolchain
coverage:
	cargo llvm-cov --locked --workspace --features hop-bar/test-tools,bar-cliproxyapi/test-tools --fail-under-lines $(COVERAGE_MIN)

dupes:
	jscpd .

# checks that the tests catch changed behaviour, not only that they run
# the code. Slow: CI runs it weekly, report-only.
mutation:
	cargo mutants --workspace --features hop-bar/test-tools,bar-cliproxyapi/test-tools --timeout 120 --jobs 2

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

# release build of the tray app against its size budget (scripts/tray-size);
# macOS and Windows only, where the archives ship it
tray-size:
	cargo build --release --locked -p bar-tray
	scripts/tray-size target/release/bar-tray$(if $(filter Windows_NT,$(OS)),.exe,)

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
