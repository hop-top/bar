# plugins

The plugins that come with bar. Each is a program named `bar-<name>` that becomes the placeholder `#{name}`; the protocol is in [docs/plugins.md](../docs/plugins.md).

## Shell plugins

POSIX `sh` scripts, Unix only (Windows discovery skips extension-less files). Details and options: [Reference plugins](../docs/plugins.md#reference-plugins).

| Script | Placeholder | Shows | Needs |
|---|---|---|---|
| [`bar-tlc`](bar-tlc) | `#{tlc}` | Closed and total tasks of the session's track, else the project, from `tlc` | `jq`, `tlc` (track chooser: macOS `osascript`) |
| [`bar-claude-credits`](bar-claude-credits) | `#{claude-credits}` | Prepaid Claude Console balance and days to expiry | `jq`, `curl`, macOS `security` |
| [`bar-ci`](bar-ci) | `#{ci}` | State of the latest workflow run of the current branch | `jq`, `git`, `gh` |
| [`bar-mem`](bar-mem) | `#{mem}` | Used memory | `awk`; macOS `memory_pressure` or `vm_stat`, Linux `/proc/meminfo` |
| [`bar-health`](bar-health) | `#{health}` | `✓` or `✗` for a HEAD request to a URL | `jq`, `curl` |

They are not installed by the release archives, Homebrew or npm. To enable one, copy or symlink it onto `PATH` and make sure it is executable:

```sh
ln -s "$PWD/plugins/bar-mem" ~/.local/bin/bar-mem
bar plugin list   # every plugin bar finds, with its path
```

Each declares a `web` object (preview samples, option hints) for the [web configurator](../docs/web.md); `make web` collects them into its plugin manifest.

## Compiled plugins

Rust crates in this workspace, built on [`bar-plugin`](../crates/bar-plugin/). They ship beside `bar` in the release archives, Homebrew and npm, where bar finds them without `PATH`.

- [`cliproxyapi/`](cliproxyapi/): `bar-cliproxyapi`, per-account quota of a CLIProxyAPI proxy and account switching ([docs](../docs/plugins/cliproxyapi.md)).
- [`routellm/`](routellm/): `bar-routellm`, state and on/off toggle of a RouteLLM router behind that proxy ([docs](../docs/plugins/routellm.md)).

From source: `cargo install --path plugins/cliproxyapi --locked` (and `plugins/routellm`).

## Tests

- Shell plugins: driven from the root crate's tests with fakes first on `PATH` (Unix only): `tests/plugins.rs`, `tests/plugins_ci.rs`, `tests/plugins_credits.rs`, `tests/plugins_tlc.rs` and `tests/plugin_tlc_*.rs`. One file: `cargo test --test plugins_ci`.
- Compiled plugins: `cargo test -p bar-cliproxyapi --features test-tools` and `cargo test -p bar-routellm`, against in-process fake servers (each crate's `tests/fake.rs`).
- `make test` runs all of them. See [tests/](../tests/) for the conventions.
