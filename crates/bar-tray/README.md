# bar-tray

The menu bar / tray app: bar's plugin actions (an account picker, a routing toggle) and session counts as a native menu in the macOS menu bar, the Windows notification area or a Linux StatusNotifierItem tray. It holds no logic of its own: the menu comes from `bar tray state --json` and every click runs `bar tray run <id>`. Setup, config, the menu and the command line: [docs/tray.md](../../docs/tray.md).

```sh
cargo install --path crates/bar-tray --locked   # beside bar; on Linux add --features linux-sni
bar tray install --dry-run                       # what bar would write to start it at login
```

Try a menu without bar or a desktop: `--print` prints it as text and exits.

```sh
cargo run -p bar-tray -- --snapshot crates/bar-tray/tests/fixtures/snapshot.json --print
```

## Layout

- [`src/main.rs`](src/main.rs): the command line.
- [`src/model/`](src/model/): platform-neutral: the snapshot contract (`snapshot.rs`), the menu it builds, the diff between two menus (`diff.rs`) and the click state machine (`state.rs`).
- [`src/backend/`](src/backend/): the event loop's decisions (`app.rs`), rows and icon, child processes (`spawn.rs`), snapshot sources (`source.rs`), and one backend per OS: `macos.rs`, `windows.rs`, `linux.rs` (with the pure `sni_map.rs` and `sni_loop.rs`).
- [`src/lock.rs`](src/lock.rs): one tray per state dir. [`src/record.rs`](src/record.rs): what `bar tray install` recorded (`--dirs`).
- [`tests/`](tests/): a second tray exiting on a held lock, child waits; `fixtures/snapshot.json` is a sample snapshot. The end-to-end test against `bar` is [`tests/tray_e2e.rs`](../../tests/tray_e2e.rs) in the root crate.

## Build and test

- `cargo test -p bar-tray`; on Linux also `cargo test -p bar-tray --features linux-sni`.
- The Linux backend is behind the `linux-sni` feature, off by default (see the comment in [`Cargo.toml`](Cargo.toml)); without it the Linux build has no backend and `bar-tray --backend` exits 1.
- `make tray-size` builds the release binary and fails when it is over its size budget ([scripts/tray-size](../../scripts/tray-size)).
