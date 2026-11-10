# bar

A status line for [Claude Code](https://code.claude.com/docs/en/statusline) that any script can extend: name it `bar-<name>`, put it on `PATH`, and it becomes a placeholder, cached so the line never waits.

![bar with the powerline skin and nord palette: model, context, repo and branch, PRs with CI marks, issues, 5-hour and 7-day usage bars, account.](docs/gallery/skin-powerline.svg)

<p align="center">
  <a href="https://github.com/hop-top/bar/actions/workflows/test-rs-caller.yml"><img alt="CI" src="https://github.com/hop-top/bar/actions/workflows/test-rs-caller.yml/badge.svg?branch=next"></a>
  <a href="bench/README.md#ben-results"><img alt="bench" src="docs/badges/bench.svg"></a>
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
| **bar** | **0.82 ms** | **1.90 ms** | **5.7 ms** | **3.8 MiB** |
| [claude-powerline](https://github.com/Owloops/claude-powerline) | 41.9 ms | 56.9 ms | 72.3 ms | 102 MiB |
| [ccstatusline](https://github.com/sirmalloc/ccstatusline) | 91.7 ms | 97.0 ms | 122.2 ms | 116 MiB |

No token counters:

| Tool | Warm | Cold | Cold CPU, incl. background | Memory |
|---|---|---|---|---|
| **bar** | **0.81 ms** | **1.01 ms** | **1.8 ms** | **3.8 MiB** |
| [ccline](https://github.com/Haleclipse/CCometixLine) | 0.93 ms | 6.61 ms | 2.3 ms | 5.0 MiB |
<!-- bench:readme:end -->

Medians, same session data and equivalent layouts ([method and raw results](bench/README.md#ben-results)).

Full feature comparison: [docs/comparison.md](docs/comparison.md).

## Install

Download an archive from [Releases](https://github.com/hop-top/bar/releases) and put `bar`, `bar-cliproxyapi` and `bar-routellm` on your `PATH` (on Linux, the `-musl` archives are static and run on any distribution; the `-gnu` ones need glibc 2.35 or newer), or build from source with a [Rust toolchain](https://rustup.rs):

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
