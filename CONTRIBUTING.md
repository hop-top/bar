# Contributing

## Setup

Rust edition 2021, stable toolchain. `hop-top-kit` is pinned exactly.

```sh
make setup   # cargo fetch
make build   # release build
make check   # fmt-check + clippy -D warnings + tests + npm-check + web-check + gallery-check + bench-badge-check
make gallery # redraw docs/gallery.md and docs/gallery/*.svg
make bench-badge # redraw docs/badges/bench.svg from bench/results/
make bench       # benchmark in a pinned container (Docker or Podman); stages results, docs, badge
make bench-check # fails when the recorded benchmark run is stale for HEAD
make quality     # audit + unused + coverage + dupes (tools: mise install)
make mutation    # cargo-mutants over the workspace; slow, report-only
make web         # web configurator: bar's renderer as wasm in web/pkg/ (docs/web.md)
```

`make check` needs `python3` for `gallery-check`, which fails when [docs/gallery.md](docs/gallery.md) no longer matches `bar` output; run `make gallery` after changing what a render looks like. `bench-docs-check` and `bench-badge-check` fail when the generated benchmark sections of [bench/README.md](bench/README.md) and [README.md](README.md), or [docs/badges/bench.svg](docs/badges/bench.svg), no longer match the results in `bench/results/`; `make bench` writes all of them. `make gallery` also needs fontTools and Hack Nerd Font Mono ([docs/dependencies.md](docs/dependencies.md#gallery-tooling-not-built-into-bar)).

`npm-check` stages the npm packages (`packaging/npm/`) from fake release archives with `scripts/npm-stage`, packs them with `npm pack` and runs the launcher from the unpacked tarballs; offline, skipped without `node` and `npm`.

`web-check` builds and lints the wasm crate `web/wasm` when the toolchain has the `wasm32-unknown-unknown` target, and skips otherwise.

`make check` must pass before a change goes up.

## Quality gates

`make quality` runs the gates beyond `make check`; CI runs it on every pull request and weekly (`.github/workflows/quality.yml`).

| Target | Tool | Fails on |
|--------|------|----------|
| `audit` | cargo-deny (`deny.toml`) | RustSec advisories, licenses outside the allow-list, wildcard versions, unknown registries or git sources |
| `unused` | cargo-machete | dependencies a crate does not use |
| `coverage` | cargo-llvm-cov | line coverage of the workspace below `COVERAGE_MIN` (Makefile) |
| `dupes` | jscpd (`.jscpd.json`) | more than 3% duplicated code |

`make mutation` (cargo-mutants) checks that the tests catch changed behaviour; a surviving mutant is a missing assertion. It takes hours, so CI runs it weekly and only reports.

The tools are pinned in `mise.toml`: install them with `mise install` (`jscpd` needs Node). `make coverage` needs the `llvm-tools-preview` component of a rustup toolchain (`rustup component add llvm-tools-preview`); a distro or Homebrew Rust lacks it. Raise `COVERAGE_MIN` as tests land, never lower it to get a change through.

Lints are shared by the whole workspace (`[workspace.lints]` in `Cargo.toml`): `unsafe_code` is denied, and only the modules that call libc or Win32 allow it, each with a reason; clippy's `cognitive_complexity` threshold is 15 (`clippy.toml`). Split a function over it rather than allowing it.

## Dev container

`.devcontainer/` holds a Debian container (linux/arm64 and linux/amd64) with everything `make check` and `make web` need: the Rust toolchain with clippy, rustfmt and the wasm32 target, `wasm-bindgen-cli`, `wasm-opt`, git, gh, jj and Python, plus Claude Code. Open the repository in an editor that supports dev containers, or use the CLI:

```sh
devcontainer up --workspace-folder .
devcontainer exec --workspace-folder . make check
```

- Claude Code is the latest release when the image is built and updates itself while the container runs. Rebuild without cache for a newer one, or pin a version with the `CLAUDE_CODE_VERSION` build arg in `devcontainer.json`; a pinned version does not auto-update.
- After creation the container installs bar from the working tree (`cargo install --path . --locked`). Run `bar install` inside to make it Claude Code's status line there, and the same `cargo install` again after a change.
- Nothing from the host is mounted besides the repository: no credentials and no `~/.claude`. Log in to Claude Code (`claude`) and, when needed, gh inside the container. Claude Code's config dir, bar's config, state and data dirs, the cargo registry and the build dir are named volumes (`bar-devcontainer-*`), so they survive a rebuild; remove the volumes to start fresh.
- `make bench` needs Docker or Podman and does not run inside the container.
- Pins and checksums: [docs/dependencies.md](docs/dependencies.md#dev-container).

## Changes

- Conventional Commits: `type(scope): subject`; types `feat fix refactor build ci chore docs style perf test`.
- Test first. A fix gets a test that fails without it.
- Keep files under about 500 lines; comment only non-obvious logic.
- Tests that touch state use temp `HOME` / `XDG_*` dirs, a fake `gh` on `PATH` and a non-git working directory. `tests/common` holds the helpers. Temp dirs go under `<temp>/bar-tests` through `tests/common/tmp.rs`: each is removed when its fixture drops, and any left behind (a killed run, a detached `bar refresh` writing late) is swept by a later test run once 10 minutes old. `BAR_TEST_KEEP=1 cargo test ...` keeps a failing test's dir and prints its path. Never read real config, tokens or the keychain.

## Branches

Pick the target branch by what the change is:

| Change | Target branch |
|--------|---------------|
| New feature (`feat:`) or breaking change (`feat!:`, `BREAKING CHANGE:`) | `next` |
| Fix for the current stable release (`fix:`, `perf:`) | `main`, then forward-ported to `next` |
| Fix for an older supported release | `release/<major>.<minor>` of the oldest line it affects, then forward-ported up |

Release PRs (opened by release-please on `next`, `main` and `release/<major>.<minor>`) merge only with a fresh benchmark run: as the last step before merging one, run `make bench` on the release PR's base branch and commit its staged results to that branch (`chore(bench): …`), not to the release PR branch, which release-please recreates from its base. The required `bench-fresh` check and the tag build fail on a stale run. Details: [docs/technical.md](docs/technical.md#releasing).

`next` cuts prereleases (`*-alpha.N`, `*-beta.N`, `*-rc.N`). `main` only takes stable releases and patches: a stable version is cut by merging `next` into `main` once the line is at an rc, and a `feat` or breaking change on `main` blocks its release. Nobody picks a version: no `Release-As` footers, no manifest edits. Details: [docs/technical.md](docs/technical.md#releasing).

## New dependencies

Vet before adding; see [docs/dependencies.md](docs/dependencies.md) for the record format. No release younger than 14 days unless it carries a security fix. Exact pins stay exact. Owned crates (`hop-top-kit`) are exempt.

## Plugins

Protocol in [docs/plugins.md](docs/plugins.md). Reference plugins live in `plugins/`.

## Click handler tests

Tests cover the generated AppleScript, plist edits, install command list and platform guard with fake runners, and links, pane parsing, every refusal, pane choice, the double check and the keys sent with a fake environment; nothing in the tests runs tmux, `osascript` or `ps`. After a change there, run the checks under [Click handler](docs/config.md#click-handler-macos) and [Hooks](docs/config.md#hooks) in a Claude Code session inside tmux.

The input-box parser reads screen output. `tests/fixtures/sessions/synthetic/` holds hand-made panes: `empty*.ansi` must parse as empty, every other file must refuse. Real captures go in a dir named after the Claude Code version (`claude --version`), and the test checks every dir there. Save one with `tmux capture-pane -p -e -t <pane> > tests/fixtures/sessions/<version>/<name>.ansi` (`empty.ansi`, `draft.ansi`, `menu.ansi`, ...) while running those checks.

## Conformance

Stories under `e2e/stories/` describe the CLI for the 12-factor conformance gate. After changing a command, update its story and run:

```sh
kit conformance verify-stories
kit conformance verify-no-leak --audit
```

Do not add scenario rubrics, assertions or judge keys to stories.

The graded factors (structured output, errors, contracts, preview, idempotency, state, exit codes) come from the scenario in `e2e/conformance/scenarios/bar/agent-config-contract/1.0.0/scenario.yaml`: steps run against bar, assertions on their exit codes, stdout and stderr. It pins its story, `e2e/stories/agent-config-contract.yaml`, by sha256; after editing the story, put `shasum -a 256` of the file in the scenario's `content_hash`. `.verifynoleak.allow` keeps the scenario out of the leak gate. To grade locally (needs a kit with `conformance svc` and `conformance harness`):

```sh
cargo build
scripts/conformance-grade
```

It starts a grading service on 127.0.0.1 with a token for that run, records every step against `target/debug/bar` in throwaway HOME, XDG and state dirs, prints the result as JSON and exits 0 on pass, 68 on fail. The 12fc workflow does the same with `--serve` on every PR into `next` and every push to it, hands the cassette to the Action, and fails on any failed assertion.

## Docs

User: [docs/config.md](docs/config.md), [docs/plugins.md](docs/plugins.md). Technical: [docs/technical.md](docs/technical.md). Links are repo-relative.
