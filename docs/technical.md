# Technical notes

## Flow

`bar render`: stdin session JSON, config, `COLUMNS` in; at most two lines out; always exit 0.

1. `session::parse` (lenient) and `conf::load`.
2. Per group (`left`, `center`, `right`): `format::parse` into tokens, split into segments at the separator, render each placeholder, drop empty segments.
3. `layout::compose` fits the groups to `COLUMNS` minus margin 4 (one line, else two).

Module map: `conf` (parser, search path), `format` (placeholder syntax), `render` (wiring), `placeholders/` (core placeholders), `plugin/` (`bar-<name>` host), `cache` (background refresh), `expr` (label logic to GitHub queries), `pace` (score), `style` (SGR, OSC 8, width), `click` (toggle state, URL handler install), `check`, `preview`.

## Cache

Anything slow is cache-backed: `gh` lists, `%command%` values, plugin `--ext-info` and plugin render output. A reader never waits. It returns the stored value, or nothing on a cold cache, and the lock winner spawns a detached `bar refresh <request>`.

- State dir: `BAR_STATE_DIR`, else kit `raw_state_dir("bar")`. Files mode 0600.
- Refresh job carries kinds and options only; secrets stay in the keychain, read by plugins.
- Failed refresh backs off 60 s (or the ttl if shorter). Locks expire after 30 s. Stored values cap at 64 KiB.

## Check and preview

`check` reuses the parsers the renderer uses (`conf`, `format`, `expr`, `gh::plan`, `plugin::validate`), so it flags what render would silently drop. Core option names and value kinds live in one table in `check.rs`; adding a core option means adding it there.

`preview` goes through the normal layout code with a stand-in for every placeholder (`render::render_sample`). The stand-in reads nothing outside the process: `model`, `effort` and `ctx` run on a synthetic session, `usage` on synthetic windows with a state dir that does not exist, the rest are fixed strings.

## Performance

Target: a cached `bar render` under 50 ms in a release build.

Cost per render of the default layout, in a git repository with a GitHub remote:

- `repo` and `branch` spawn up to 4 uncached `git` processes (remote URL, toplevel, current branch, remote URL again). `prs` and `issues` each ask for the remote URL once more. With a GitHub remote that is 5 processes per render. git results are not cached.
- Everything else is in-process.

Measured on macOS 15 with Apple git 2.39.5 (`/usr/bin/git`), release build, temp repo with an `origin` remote, fake `gh` on `PATH` returning `[]`, warm cache, 20 runs after warm-up:

| Run | CPU median | Wall median |
|-----|-----------|-------------|
| `bar --version` | 4.9 ms | not usable |
| `bar render`, default layout | 92 ms | not usable |
| one `git` call (`branch`, `remote get-url`, `rev-parse`) | 14 ms | not usable |

The machine was saturated (load average 50 to 70) during the run, so wall time was dominated by scheduling (`bar --version` took about 390 ms wall). CPU time of the process and its children is the usable figure. It is above the 50 ms target, and the git processes account for nearly all of it. A quiet-machine wall-clock run is still to do.

Options if the figure holds: memoise the remote URL within one render, then cache the git results per working directory with a short ttl through the existing cache. Neither is done.
