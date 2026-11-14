# bar

A status line for [Claude Code](https://code.claude.com/docs/en/statusline) that any script can extend: name it `bar-<name>`, put it on `PATH`, and it becomes a placeholder, cached so the line never waits.

<p align="center">
  <img src="docs/gallery/skin-powerline.svg" alt="bar with the powerline skin and nord palette: model, context, repo and branch, PRs with CI marks, issues, 5-hour and 7-day usage bars, account.">
  <br>
  <img src="docs/gallery/skin-capsule.svg" alt="Capsule skin, dracula palette: each segment in a rounded pill.">
  <br>
  <img src="docs/gallery/skin-plain.svg" alt="Plain skin, default palette: segments split by muted bars.">
  <br>
  <img src="docs/gallery/two-line.svg" alt="The two-line preset with the nord palette.">
</p>

---

<p align="center">
  <a href="https://github.com/hop-top/bar/actions/workflows/test-rs-caller.yml"><img alt="CI" src="https://github.com/hop-top/bar/actions/workflows/test-rs-caller.yml/badge.svg?branch=next"></a>
  <a href="bench/README.md#ben-results"><img alt="bench" src="docs/badges/bench.svg"></a>
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue.svg"></a>
  <a href=".github/release-please-config.json"><img alt="version" src="https://img.shields.io/static/v1?label=version&message=0.1.0-rc.0&color=blue"></a> <!-- x-release-please-version -->
  <a href="https://github.com/hop-top/spec-12fc"><img alt="12fc" src="https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/hop-top/bar/next/.12fc.json"></a>
</p>
<p align="center">
  <img alt="macOS" src="docs/badges/macos.svg"> <img alt="Linux" src="docs/badges/linux.svg"> <img alt="Windows" src="docs/badges/windows.svg">
</p>

## Why bar

- **Plugins are plain programs.** Any executable that prints one JSON object is a placeholder. bar runs it in the background and caches the result ([Plugins](docs/plugins.md)).
- **The layout is a config file.** tmux-style, options on every placeholder, linted by `bar check`, previewed by `bar preview`.
- **It knows your Claude usage.** 5-hour and 7-day bars coloured by pace, projections, sparklines; context, cost, burn rate, tokens, prompt cache, spend history and budgets.
- **It knows your proxy accounts.** Behind a CLIProxyAPI proxy, `bar-cliproxyapi` shows each account's 5-hour and 7-day quota with reset countdowns, feeds `#{usage}` and `#{account}`, and switches accounts from the command line or a click ([plugin](docs/plugins/cliproxyapi.md)).
- **It can switch a router.** `bar-routellm` shows whether a RouteLLM router behind your proxy is routing and turns it on or off from a click or `bar rtellm on|off` ([plugin](docs/plugins/routellm.md)).
- **It knows your repo.** Branch, dirty state, commit, tag, stash, remotes, upstream, fork and operation in progress from files; lines added and removed and commit age from a background `git`; Jujutsu changes, PRs and issues from `gh` (merge requests from `glab` on GitLab), failing CI marked.

### Benchmarks

<!-- bench:readme:start -->
With token counters:

| Tool | Warm | Cold | Cold CPU, incl. background | Memory |
|---|---|---|---|---|
| **bar** | **0.83 ms** | **2.14 ms** | **6.6 ms** | **4.1 MiB** |
| [claude-powerline](https://github.com/Owloops/claude-powerline) | 43.6 ms | 58.2 ms | 79.3 ms | 102 MiB |
| [ccstatusline](https://github.com/sirmalloc/ccstatusline) | 94.4 ms | 101.0 ms | 127.4 ms | 116 MiB |

No token counters:

| Tool | Warm | Cold | Cold CPU, incl. background | Memory |
|---|---|---|---|---|
| **bar** | **0.81 ms** | **1.00 ms** | **1.9 ms** | **4.0 MiB** |
| [ccline](https://github.com/Haleclipse/CCometixLine) | 0.94 ms | 6.44 ms | 2.3 ms | 5.1 MiB |
<!-- bench:readme:end -->

Medians, same session data and equivalent layouts ([method and raw results](bench/README.md#ben-results)).

Full feature comparison: [docs/comparison.md](docs/comparison.md).

## Install

Download an archive from [Releases](https://github.com/hop-top/bar/releases) and put `bar`, `bar-cliproxyapi` and `bar-routellm` on your `PATH` (on Linux, the `-musl` archives are static and run on any distribution; the `-gnu` ones need glibc 2.35 or newer). The macOS and Windows archives also hold `bar-tray`, the menu bar / tray app; keep it in the same directory as `bar` ([tray](docs/tray.md)). Or build from source with a [Rust toolchain](https://rustup.rs):

```sh
git clone https://github.com/hop-top/bar.git
cd bar
cargo install --path . --locked
cargo install --path plugins/cliproxyapi --locked   # optional: bar-cliproxyapi plugin
```

With Node or Bun, from npm (prebuilt binary, no install script):

```sh
npx @hop-top/bar preview --preset   # or: bunx @hop-top/bar preview --preset
npm install -g @hop-top/bar         # then `bar install`; the status line runs the binary, not Node
```

The binaries are not signed. On macOS, an archive downloaded with a browser is quarantined and Gatekeeper refuses to run what it unpacks; clear the flag once, in the directory you unpacked to:

```sh
xattr -d com.apple.quarantine bar bar-cliproxyapi bar-routellm bar-tray
```

Homebrew, npm and `curl` downloads are not quarantined. On Windows, SmartScreen may stop the first run of `bar.exe` or `bar-tray.exe`: **More info**, then **Run anyway**.

Guided setup inside Claude Code: `/plugin marketplace add hop-top/bar`, then `/plugin install bar@hop-top` and `/bar` ([docs](docs/claude-plugin.md)).

Optional: `gh` (GitHub) or `glab` (GitLab) for PRs and issues, `jq` for the reference plugins.

## Use

```sh
bar install --refresh 5       # set bar as the status line in ~/.claude/settings.json (shows the diff, asks first)
bar install --subagent        # also draw the subagent rows below the prompt (subagentStatusLine)
```

Or by hand, in `~/.claude/settings.json`:

```json
{ "statusLine": { "type": "command", "command": "bar render", "refreshInterval": 5 } }
```

If the line stays empty, use the absolute path to `bar` (on Windows with forward slashes).

```sh
bar preview --preset          # every preset, with sample data
bar init --preset powerline   # write one as your config
bar edit                      # edit the config with a live preview
bar check                     # lint the config
bar preview --subagent        # the subagent rows, with sample subagents
```

```text
set skin powerline
set palette nord
set left  "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=5]}"
set right "#{usage[type=5h]} │ #{usage[type=7d]}"
```

Skins, palettes, icons, multi-line layouts and [subagent rows](docs/config.md#subagent-rows): [configuration](docs/config.md), [gallery](docs/gallery.md).

Compose a config in the browser with a live preview: run `make web`, open `web/index.html` ([web configurator](docs/web.md)).

## Tray

`bar-tray` shows plugin actions (an account picker, a routing toggle) and session counts in the macOS menu bar, the Windows notification area or a Linux tray. Opt a plugin in with a `tray` line in your config (`tray cliproxyapi`), then let bar start the tray at login:

```sh
cargo install --path crates/bar-tray --locked   # beside bar; on Linux add --features linux-sni
bar tray install --dry-run                       # what it would write; then run it without --dry-run
```

Per-platform notes, the menu and the install details: [docs/tray.md](docs/tray.md).

## Write a plugin

```sh
#!/bin/sh
# bar-load: 1-minute load average as #{load}
if [ "$1" = "--ext-info" ]; then
  echo '{"name":"load","version":"0.1.0","capabilities":["discover","placeholder"],"ttl":"30s"}'
  exit 0
fi
load=$(uptime | sed 's/.*load averages*: *//; s/[, ].*//')
printf '{"label":"load","text":"%s"}\n' "$load"
```

Save it as `bar-load` on `PATH`, make it executable, add `#{load}` to a line. Protocol and reference plugins: [docs/plugins.md](docs/plugins.md). Shipped with bar: [`bar-cliproxyapi`](docs/plugins/cliproxyapi.md), the per-account quota of a CLIProxyAPI proxy, with `bar cpa list|use|use --next|pick` (alias of `bar cliproxyapi …`) to switch accounts. Plugins can also return several items, supply `#{usage}` and `#{account}`, and bind click actions.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

[Apache-2.0](LICENSE)
