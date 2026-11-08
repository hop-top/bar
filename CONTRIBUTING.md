# Contributing

## Setup

Rust edition 2021, stable toolchain. `hop-top-kit` is pinned exactly.

```sh
make setup   # cargo fetch
make build   # release build
make check   # fmt-check + clippy -D warnings + tests + gallery-check
make gallery # redraw docs/gallery.md and docs/gallery/*.svg
```

`make check` needs `python3` for `gallery-check`, which fails when [docs/gallery.md](docs/gallery.md) no longer matches `bar` output; run `make gallery` after changing what a render looks like. `make gallery` also needs fontTools and Hack Nerd Font Mono ([docs/dependencies.md](docs/dependencies.md#gallery-tooling-not-built-into-bar)).

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

`next` cuts prereleases (`*-alpha.N`, `*-beta.N`, `*-rc.N`). `main` only takes stable releases and patches: a stable version is cut by merging `next` into `main` once the line is at an rc, and a `feat` or breaking change on `main` blocks its release. Nobody picks a version: no `Release-As` footers, no manifest edits. Details: [docs/technical.md](docs/technical.md#releasing).

## New dependencies

Vet before adding; see [docs/dependencies.md](docs/dependencies.md) for the record format. No release younger than 14 days unless it carries a security fix. Exact pins stay exact. Owned crates (`hop-top-kit`) are exempt.

## Plugins

Protocol in [docs/plugins.md](docs/plugins.md). Reference plugins live in `plugins/`.

## Conformance

Stories under `e2e/stories/` describe the CLI for the 12-factor conformance gate. After changing a command, update its story and run:

```sh
kit conformance verify-stories
kit conformance verify-no-leak --audit
```

Do not add scenario rubrics, assertions or judge keys to stories.

## Docs

User: [docs/config.md](docs/config.md), [docs/plugins.md](docs/plugins.md). Technical: [docs/technical.md](docs/technical.md). Links are repo-relative.
