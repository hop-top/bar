# tests

Integration tests of the root crate (`hop-bar`): one file per area, each its own test binary (`render_*.rs`, `git*.rs`, `click*.rs`, `plugin*.rs`, `tray_*.rs`, `usage*.rs`, ...). Most run the real `bar` binary or call the library; none touches the network, the real config, tokens or the keychain. The plugin crates and `bar-tray` keep their own tests in their `tests/` dirs.

## Running

```sh
make test                                          # the whole workspace, as CI runs it
cargo test --features test-tools --test plugins_ci # one file of this crate
cargo test --features test-tools --test gitst parse  # tests of one file whose name contains `parse`
BAR_TEST_KEEP=1 cargo test --features test-tools --test render  # keep a failing test's temp dir
```

The `test-tools` feature lets `bar` take `curl` and `security` from `PATH` instead of fixed system paths, so the usage-API tests can fake them; `make test` turns it on and a release build refuses it. Files that use shell fixtures are Unix only (`#![cfg(unix)]`); [`windows.rs`](windows.rs) covers the Windows side.

## Layout

- [`common/`](common/): shared helpers, pulled in with `mod common;` or by `#[path]`.
  - [`mod.rs`](common/mod.rs): `isolation(root)`, the hermetic environment for a `bar` run: `HOME`, `BAR_STATE_DIR`, the XDG dirs and `CLAUDE_CONFIG_DIR` inside a temp dir, the global git config off, a fake-bin dir with failing `gh`, `curl` and `security` first on `PATH`, and a non-git working directory. Also `wait_until` and `BG_WAIT`.
  - [`tmp.rs`](common/tmp.rs): temp dirs under `<temp>/bar-tests`, removed on drop; ones left behind are swept once 10 minutes old.
  - [`exe.rs`](common/exe.rs): writes fake executables from a child process, so a parallel test's spawn cannot hit "text file busy".
  - [`deadline.rs`](common/deadline.rs): runs a child with a deadline, so a plugin that hangs fails its test instead of the suite. The plugin crates use it too.
  - `*_fx.rs`: fixtures per area: fake `gh` and repos (`gh_fx`, `git_fx`), plugins (`plugin_fx`, `plugins_fx`, `tlc_pick_fx`), renders (`render_fx`), the tray (`tray_fx`), the click handler's fake environment (`fake_env`), OSC 8 links (`osc_links`).
- [`fixtures/`](fixtures/): `plugins/` (small fixture plugins: echo, slow, account, ...), `pr-*.json` (`gh` replies), `session-full.json` (a statusline input), `sessions/` (captured and hand-made input-box panes, see [CONTRIBUTING.md](../CONTRIBUTING.md#click-handler-tests)), `skins/` (expected renders), `usage-api/`, `spec.conf` (a sample config `conf.rs` parses).

## Conventions

- Test first; a fix gets a test that fails without it.
- Every `bar` run goes through `isolation` or an equivalent fixture: temp `HOME`, `BAR_STATE_DIR` and XDG dirs, a fake `gh` (or `tlc`, `curl`, `security`) on `PATH`. Never read real config or credentials.
- Write fake executables with `common::exe`, not `std::fs::write`.
- No fixed sleeps for detached work (a background `bar refresh`): poll with `common::wait_until(BG_WAIT, ...)`; it returns as soon as the condition holds.
- Some tests measure time limits (a plugin killed at its 3 s timeout in `plugin_options.rs`, the runner's limits in `run_child.rs`, a refresh killed in `cache.rs`). Their margins are wide, but a heavily loaded host can still slow them; rerun the file alone before suspecting a regression.

More in [CONTRIBUTING.md](../CONTRIBUTING.md#changes).
