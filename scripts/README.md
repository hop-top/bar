# scripts

Build, check and release helpers. Most run through a `make` target; each script's header comment has its usage.

| Script | What it does | Run by |
|---|---|---|
| [`bench`](bench) | The benchmark in a pinned container (Docker or Podman); stages results, generated docs and badge for a `chore(bench)` commit | `make bench` |
| [`bench-inputs`](bench-inputs) | Content hash of the inputs that decide benchmark results; `--check` fails when the recorded run is stale | `make bench-check`, the `bench-fresh` and release workflows |
| [`bench-render`](bench-render) | The generated benchmark sections of [bench/README.md](../bench/README.md) and the root README | `make bench-docs`, `make bench-docs-check` |
| [`bench-badge`](bench-badge) | [docs/badges/bench.svg](../docs/badges/bench.svg) from `bench/results/` | `make bench-badge`, `make bench-badge-check` |
| [`gallery`](gallery) | [docs/gallery.md](../docs/gallery.md) and `docs/gallery/*.svg` from real `bar` output | `make gallery`, `make gallery-check` |
| [`web`](web) | The [web configurator](../docs/web.md)'s wasm renderer and plugin manifest in `web/pkg/`; `--check` builds and lints only | `make web`, `make web-check` |
| [`web-font`](web-font) | Rebuilds the subset fonts in `web/fonts/` | by hand ([docs/web.md](../docs/web.md#font)) |
| [`npm-stage`](npm-stage) | Stages the npm packages of a release from its archives | the release workflow, `npm-check` |
| [`npm-check`](npm-check) | Stages the npm packages from fake archives, packs them and runs the launcher, offline | `make npm-check` |
| [`tray-size`](tray-size) | Fails when a release `bar-tray` is over its size budget | `make tray-size`, CI and the release workflow |
| [`conformance-grade`](conformance-grade) | Grades bar against its 12-factor conformance scenario with a local grading service | the `12fc` workflow, by hand ([CONTRIBUTING.md](../CONTRIBUTING.md#conformance)) |

`make check` runs the `--check` modes of `gallery`, `web`, `bench-render` and `bench-badge`, and `npm-check`. Versions of the outside tools they use (benchmark image, gallery, web, npm, conformance): [docs/dependencies.md](../docs/dependencies.md).
