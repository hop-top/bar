# Contributing

## Setup

Rust edition 2021, stable toolchain. `hop-top-kit` is pinned exactly.

```sh
make setup   # cargo fetch
make build   # release build
make check   # fmt-check + clippy -D warnings + tests + gallery-check + bench-badge-check
make gallery # redraw docs/gallery.md and docs/gallery/*.svg
make bench-badge # redraw docs/badges/bench.svg from bench/results/
make bench       # benchmark in a pinned container (Docker or Podman); stages results, docs, badge
make bench-check # fails when the recorded benchmark run is stale for HEAD
```

`make check` needs `python3` for `gallery-check`, which fails when [docs/gallery.md](docs/gallery.md) no longer matches `bar` output; run `make gallery` after changing what a render looks like. `bench-docs-check` and `bench-badge-check` fail when the generated benchmark sections of [bench/README.md](bench/README.md) and [README.md](README.md), or [docs/badges/bench.svg](docs/badges/bench.svg), no longer match the results in `bench/results/`; `make bench` writes all of them. `make gallery` also needs fontTools and Hack Nerd Font Mono ([docs/dependencies.md](docs/dependencies.md#gallery-tooling-not-built-into-bar)).

`make check` must pass before a change goes up.

## Changes

- Conventional Commits: `type(scope): subject`; types `feat fix refactor build ci chore docs style perf test`.
- Test first. A fix gets a test that fails without it.
- Keep files under about 500 lines; comment only non-obvious logic.
- Tests that touch state use temp `HOME` / `XDG_*` dirs, a fake `gh` on `PATH` and a non-git working directory. `tests/common` holds the helpers. Never read real config, tokens or the keychain.

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

## Docs

User: [docs/config.md](docs/config.md), [docs/plugins.md](docs/plugins.md). Technical: [docs/technical.md](docs/technical.md). Links are repo-relative.
