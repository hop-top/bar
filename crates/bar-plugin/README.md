# bar-plugin

The Rust library behind bar's compiled plugins: the [plugin protocol](../../docs/plugins.md)'s JSON types and the helpers a plugin that talks to a service needs. `bar` itself uses its protocol types and some helpers; [`bar-cliproxyapi`](../../plugins/cliproxyapi/) and [`bar-routellm`](../../plugins/routellm/) are built on it. A plugin does not have to be written in Rust or use this crate: any executable that prints the JSON works.

| Module | What it holds |
|---|---|
| [`proto`](src/proto.rs) | `ExtInfo` (`--ext-info`), `RenderInput` (render stdin), `Output` (render stdout), `CheckReport` (`--check`), actions, accounts, usage windows, and the text cleaning bar applies |
| [`menu`](src/menu.rs) | The `--menu` call for the [tray](../../docs/tray.md): its input and the menu a plugin prints |
| [`dirs`](src/dirs.rs) | bar's config and state directories, resolved as bar resolves them |
| [`fsio`](src/fsio.rs) | Capped reads, owner-only atomic writes, reads that refuse files other users could have touched |
| [`http`](src/http.rs) | Requests through the system `curl`, secrets on its stdin, never in argv or the environment |
| [`secret`](src/secret.rs) | Credential bytes, zeroed on drop |
| [`keysource`](src/keysource.rs) | Where a key comes from: an owner-only file, the macOS Keychain or a command |
| [`keyguard`](src/keyguard.rs), [`mgmt`](src/mgmt.rs), [`url`](src/url.rs) | Ban-safe calls to one proxy's management API, shared by every plugin of that proxy |
| [`sha256`](src/sha256.rs) | SHA-256 for cache file names and key fingerprints |
| [`wait`](src/wait.rs) | Waiting for a child process with a deadline |

## Example

A plugin `bar-hello` that shows `hello <model>` as `#{hello}`, or `hello <name>` with `#{hello[name=you]}`. Its `Cargo.toml` needs `bar-plugin` and `serde_json`.

```rust,no_run
use bar_plugin::proto::{ExtInfo, OptSpec, Output, RenderInput};
use std::io::Read;

fn ext_info() -> ExtInfo {
    let mut info = ExtInfo {
        name: "hello".into(),
        version: "0.1.0".into(),
        description: "Greets the session's model".into(),
        capabilities: vec!["discover".into(), "placeholder".into()],
        ttl: "60s".into(),
        ..Default::default()
    };
    info.options.insert("name".into(), OptSpec::string());
    info
}

fn render(input: &RenderInput) -> Output {
    let who = input.opt_str("name").unwrap_or(&input.session.model);
    if who.is_empty() {
        return Output::hidden();
    }
    Output::text(&format!("hello {who}"))
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--ext-info") {
        println!("{}", serde_json::to_string(&ext_info()).unwrap());
        return;
    }
    let mut raw = String::new();
    let _ = std::io::stdin().take(1 << 20).read_to_string(&mut raw);
    let input: RenderInput = serde_json::from_str(&raw).unwrap_or_default();
    println!("{}", render(&input).finish().to_json());
}
```

`Output::finish` cleans the text and drops what bar would refuse (bad tokens, surplus items) before printing. Build it, put `bar-hello` on `PATH` or beside `bar`, and check it with `bar plugin info hello`. The protocol, options, caching and actions: [docs/plugins.md](../../docs/plugins.md).

## Using it

The crate is not on crates.io (`publish = false`) and its version stays `0.0.0`; outside this repository depend on it by git and pin a revision:

```toml
[dependencies]
bar-plugin = { git = "https://github.com/hop-top/bar", rev = "<commit>" }
serde_json = "1"
```

The example above is compiled as a doc test, so `cargo test -p bar-plugin` fails when it no longer builds against the crate.
