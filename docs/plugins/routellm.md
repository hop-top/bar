# bar-routellm

The plugin is a `bar-routellm` program next to `bar`; its commands run through bar as `bar routellm …`, or with the alias `bar rtellm …`.

`bar-routellm` shows, and switches, whether a [RouteLLM](https://github.com/lm-sys/RouteLLM) router behind a [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) proxy is routing, as the placeholder `#{routellm}`. It ships next to `bar` and is found like any [plugin](../plugins.md).

It is opt-in. Nothing runs, and nothing is read, until the placeholder is in your layout and the plugin's own configuration file exists.

```
set left "#{model} │ #{routellm} │ #{cliproxyapi} │ #{repo}:#{branch}"
```

Claude Code must reach the proxy through `ANTHROPIC_BASE_URL`; the segment is hidden for any other base URL, and for a proxy the plugin is not configured for.

## What it does

The proxy serves the model ids `route-auto`, `route-vision` and so on (the `route-*` aliases). Where they go depends on one switch, the `disabled` flag of the proxy's RouteLLM provider entry:

| Routing | `route-*` requests go to |
|---|---|
| **ON** | RouteLLM. Its classifier picks a tier and a leaf: a local model, OpenRouter, or back to the proxy's own frontier accounts. |
| **OFF** | The proxy's frontier accounts directly, as the model that `oauth-model-alias` maps the alias to (by default `claude-sonnet-5-5` for `route-auto` and `route-vision`). |

Claude Code does not change: it keeps asking for the same model id and the proxy lists the same ids in both states. The switch takes effect per request, within a fraction of a second, with no restart; a stream already open finishes where it started. `bar rtellm on` and `bar rtellm off`, or a click, write that one flag (and the provider's priority on ON, when it is lower than configured). Nothing else in the proxy's configuration is changed.

`route-private` stays fail-closed. It has no frontier row, so with routing OFF a session that asked for it gets the proxy's `model_not_found` error (HTTP 400) instead of having its data sent to a cloud model. The segment shows it as blocked.

The plugin has two jobs, kept apart: a render only reads, and only an explicit action (`bar rtellm on|off` or a bound click) writes.

## Setup

1. **Enable the proxy's management API** and keep its key in a separate owner-only file, exactly as for [`bar-cliproxyapi`](cliproxyapi.md#setup). If that plugin already reads the instance, there is nothing more to do for the key.

2. **Add the provider and the aliases to the proxy.** `bar rtellm setup` prints the block; it never writes it. Paste it into the proxy's `config.yaml`, merging each section into the file's existing one:

   ```
   bar rtellm setup [--layout v8|legacy]
   ```

   `--layout` picks the file layout; the default is `v8`, the layout of the current CLIProxyAPI template (top-level `config-version: 8`). Use `legacy` for a flat file without it. Each layout prints a complete block with the field names that layout expects. The two differ in the section names and in the provider's key list (`keys` against `api-key-entries`); a block of one layout makes the proxy refuse to load a file of the other.

   v8 layout:

   ```yaml
   oauth:
     model-alias:
       claude:
         - {name: claude-sonnet-5-5, alias: route-auto, fork: true}
         - {name: claude-sonnet-5-5, alias: route-vision, fork: true}
   api-keys:
     openai-compatibility:
       - name: routellm
         disabled: true
         base-url: http://127.0.0.1:6060/v1
         priority: 1000
         disable-cooling: true
         keys:
           - api-key: "unused"
         models:
           - {name: auto, alias: route-auto}
           - {name: vision, alias: route-vision}
           - {name: private, alias: route-private}
   ```

   Legacy layout:

   ```yaml
   oauth-model-alias:
     claude:
       - {name: claude-sonnet-5-5, alias: route-auto, fork: true}
       - {name: claude-sonnet-5-5, alias: route-vision, fork: true}
   openai-compatibility:
     - name: routellm
       disabled: true
       base-url: http://127.0.0.1:6060/v1
       priority: 1000
       disable-cooling: true
       api-key-entries:
         - api-key: "unused"
       models:
         - {name: auto, alias: route-auto}
         - {name: vision, alias: route-vision}
         - {name: private, alias: route-private}
   ```

   - `disabled: true` keeps the provider OFF: pasting the block never turns routing on. It stays OFF until `bar rtellm on`.
   - `api-key` is a placeholder; put the key RouteLLM expects there if it checks one.
   - `priority` must be higher than every frontier account's, so that ON wins when both providers serve a `route-*` id.
   - `disable-cooling: true` keeps the provider from being skipped for about a minute after an upstream error once it answers again.
   - The `claude` alias rows are the OFF side. `fork: true` is required: it keeps the original model id available, which RouteLLM's own frontier calls use.
   - There is no row for `route-private`, on purpose.
   - A v8-layout file is rewritten by the proxy on the plugin's first toggle (comments are dropped, defaults are added). Keep a backup of the file. A legacy file keeps its comments.

3. **Create `routellm.conf`** in bar's configuration directory, next to `cliproxyapi.conf` (`~/.config/bar/` on Linux, `~/Library/Application Support/bar/` on macOS; `$XDG_CONFIG_HOME/bar/` when set). It must be a regular file owned by you with no group or other permissions (`chmod 600`), at most 16 KiB.

   ```
   instance http://127.0.0.1:8318
     routellm http://127.0.0.1:6060
     key-from cliproxyapi
   ```

4. **Add `#{routellm}`** to a layout and run `bar check`, then `bar rtellm status --probe`, which reads the proxy once and reports a missing provider entry, aliases without an OFF row, or a RouteLLM that does not answer.

`bar check` (the plugin's `--check`) makes no request. It reports, from the configuration file and the last cache: a missing, unreadable or malformed `routellm.conf` (with the line); a key source that is unreadable; a state directory that is not writable; a key the proxy refused; management off (404); no matching provider entry; an entry that points at another RouteLLM; aliases without an OFF row; a `private` alias that has an OFF row (which would send it to the frontier); a proxy not read yet; and an `on-priority` of 0 or less.

The first time the plugin switches routing, the proxy re-saves its configuration file. A file in the legacy flat layout gets one line, `disabled: true`, which switching ON removes again; comments stay. A file in the v8 layout is re-serialised on the first write, which drops its comments. The proxy answers before it has applied the change; the plugin reads the entry again and reports what it finds. Keep this in mind for a configuration kept under version control: the switch shows up as a diff.

## Configuration file

One `instance` block per proxy; lines start with a keyword, `#` starts a comment, values may be quoted.

| Line | Meaning |
|---|---|
| `instance <url>` | The proxy, as Claude Code's `ANTHROPIC_BASE_URL` names it. Only scheme, host and port count. A session whose base URL matches no instance shows nothing; there is no fallback to another instance. |
| `routellm <url>` | Where this machine reaches RouteLLM. Required; it is never guessed from the proxy. |
| `provider <name>` | The `openai-compatibility` entry that carries RouteLLM. Default `routellm`. |
| `key-from cliproxyapi` | Use the key source of the same instance in `cliproxyapi.conf`. The default when the block has no other key line. |
| `key-file <path>`, `key-keychain <service>`, `key-command <program> [args...]` | The key sources of [`bar-cliproxyapi`](cliproxyapi.md#key-sources), with the same rules. At most one key line per instance. |
| `management <url>` | The management API base, ending in `/v0/management` (or `/v8/management`, which is rewritten to `/v0/management`). Default: `<instance origin>/v0/management`. |
| `local-tiers <list>` | Tiers you declare to stay on this machine. Default `private,default,local_text`. |
| `on-priority <n>` | Priority the provider gets when routing is switched ON and its own is lower. Default `1000`. |
| `off-map <alias>=<model>` | The model an alias maps to when routing is OFF; repeatable. Used by `bar rtellm setup` and to label the OFF state. Default `route-auto=claude-sonnet-5-5` and `route-vision=claude-sonnet-5-5`. |
| `remote yes` | Allow a management or RouteLLM host that is not on this machine. Requires `https` for both. |

A block with a mistake is ignored as a whole, and `bar check` says which line.

The file is separate from `bar.conf` on purpose, and separate from `cliproxyapi.conf`: this plugin changes the proxy's configuration, so the grant is explicit and can be removed on its own, and nothing that decides where the management key is sent can be set from a repository's `.bar.conf`.

`key-from cliproxyapi` reads the `instance` block with the same URL in `cliproxyapi.conf` (an owner-only file, held to the same rules) and takes its key line. One management key then has one source, and one refusal memory (see [Failures and ban safety](#failures-and-ban-safety)).

Management is contacted over plain `http` only on `localhost`, `127.0.0.1` and `::1`. Any other host needs `remote yes` and `https`, for management and for RouteLLM. The RouteLLM probes carry no credentials.

## What it shows

```
RL on │ auto ☁
RL off │ auto → sonnet-5-5
RL off │ private ✗
```

| Item | Text | When |
|---|---|---|
| state | `RL on` (`good`), `RL off` (`dim`), `RL on ✗` (`bad`: on, but RouteLLM does not answer), `RL key?` (`bad`), `RL ?` | always, once the instance matches |
| alias | the session's `route-*` alias, such as `route-auto` | `show` has `alias` and the session's model is an alias of the provider entry |
| tier | the tier behind the alias and a marker: `auto ☁`, `private ⌂`. With routing OFF, the model the alias maps to, without its `claude-` prefix: `auto → sonnet-5-5`. With routing OFF and no row for the alias, or with routing on and RouteLLM not answering: `private ✗` in `bad` | `show` has `tier` |
| capabilities | `vis tools 200k`, in `dim` | `show` has `caps` |

- `⌂` marks a tier you declared local with `local-tiers`, `☁` any other: it may leave this host. The marker is **declared, not derived**: it says what `local-tiers` says, and RouteLLM does not report where a tier runs.
- Capabilities come from the router's model list. A tier's capabilities are the union over every leaf it can reach, so `vis` reads "some leaf of this tier can".
- ` ~` after the state text marks data older than two minutes; `stale=dim` draws every item dim instead, and `stale=hide` hides the segment. An unknown state (`RL ?`, `RL key?`) is never marked stale.
- The state is what the proxy is configured to do. It says "routing is on", not "this request was routed".
- An alias, tier or capability item needs the session's model to be an alias of the provider entry. A session on another model shows the state alone. Tiers without a client alias (`hard`, `light`, `codex`, `media` in a common setup) are listed only by `bar rtellm tiers --all`.
- The segment is hidden when the proxy has no `routellm` entry, when its entry points at another RouteLLM than the configured one, and when management is off.

What the segment cannot show is a property of what the router exposes today: it has no view of which model served a request (RouteLLM reports the decision only in the body of a non-streamed reply, and Claude Code streams), no per-model health, and no OpenRouter limits or credits. The plugin does not read RouteLLM's log or trace files, and does not hold an OpenRouter key. `RL on` means the proxy routes to RouteLLM and RouteLLM's `/health` answers; it does not mean every model behind it is healthy.

### Options

| Option | Default | Meaning |
|---|---|---|
| `show` | `state,alias,tier` | Items, in this order: `state`, `alias`, `tier`, `caps`. |
| `label` | `alias` | `alias` draws the alias item, `none` leaves it out. |
| `compact` | off | Leaves the alias item out, whatever `show` and `label` say; the state and tier items stay: `RL on │ auto ☁`. |
| `stale` | `mark` | Old data: `mark` (`~`), `dim` or `hide`. |
| `link` | none | A link template for the alias, tier and capabilities items, with `{instance}`, `{alias}` and `{tier}` (percent-encoded). The state item takes no `link`: it keeps the toggle click. |
| `routed` | off | Hide the whole segment unless the session's model is a `route-*` alias. Off, the state shows for every session on the proxy. |
| `base-url` | none | Base URL to use when `ANTHROPIC_BASE_URL` is not in the environment bar passes. |

The list option `show` is comma separated: `#{routellm[show=state,tier,caps]}`; a list that names none of `state`, `alias`, `tier`, `caps` keeps the default, and a segment with nothing selected is hidden. The plugin's `ttl` is 15 s and its time limit 8 s; an action may run for 78 s. It emits no `usage` and no `account`; those stay with [`bar-cliproxyapi`](cliproxyapi.md).

## Switching

```
bar rtellm status [--instance URL] [--probe] [--json]
bar rtellm on  [--instance URL] [--dry-run] [--force]
bar rtellm off [--instance URL] [--dry-run]
bar rtellm tiers [--all] [--instance URL]
bar rtellm setup [--layout v8|legacy] [--instance URL]
```

- `status` prints the state, the provider's priority, each alias with its tier and OFF model, whether RouteLLM answers, and the last change. It answers from the plugin's cache; `--probe` makes the calls (once, ban-safe). `--json` prints the cache.
- `on` and `off` read the entry, change nothing when it already is in the target state (`already on`), then write the one flag and read it back, a few times if the proxy is slow to apply it. `on` first checks that RouteLLM answers and serves every aliased tier, and refuses otherwise; `--force` overrides the check. `off` is never gated: it works with RouteLLM down and never contacts it, so it is always the safe way out. `--dry-run` prints the change and sends nothing.
- `tiers` lists the aliased tiers with their capabilities and marker; `--all` adds the tiers without a client alias.
- `setup` prints the proxy configuration block for the chosen layout (default `v8`); it never writes it.
- The instance is `--instance`, else the session's `ANTHROPIC_BASE_URL`, else the only configured instance.
- Exit status: 0 done, 1 refused or failed (the reason on stderr), 2 bad usage.

The last change (time, from, to) is kept in the plugin's own cache, and `status` shows it. No command prints a key, an `api-key` entry, a header or an email.

### Clicking the segment

The plugin declares the actions `toggle`, `on` and `off`, with the tokens `instance` and `to`, where `to` is the state a click would switch to. Bind one in `bar.conf`:

```
action routellm routellm.toggle
```

The action times out after 78 s. A click re-reads the state before it writes and does nothing when `to` already holds, so a stale link cannot flip routing back. Links need the click handler installed with action support (macOS): run `bar click install`, again after upgrading bar. bar shows the outcome in `action-status` in its state directory and refreshes the segment at the next render. `on` and `off` bind separate links, for a layout with two clicks.

## With bar-cliproxyapi

The two segments share a layout and one proxy.

- **Accounts and usage are unchanged in both states.** The RouteLLM provider is not an account: it does not appear in the account listing that `#{cliproxyapi}` reads, and its priority is not an account priority. RouteLLM's frontier calls go through the proxy's Claude accounts, so their rate-limit signals keep updating while routing is ON. `bar cpa use` keeps working; keep account priorities below the provider's.
- **`route-*` sessions keep the accounts segment.** A session on `route-auto` has no provider name in its model id, so `bar-cliproxyapi` takes it from its `route-provider` option, default `claude`: every `route-*` alias leads to a Claude account or to a local model. Set `#{cliproxyapi[route-provider=codex]}` if your aliases lead to Codex.
- One management key, one refusal memory: see below.

```
set left "#{model} │ #{routellm} │ #{cliproxyapi} │ #{account}"
```

## Failures and ban safety

The proxy bans a client IP for 30 minutes after five failed management attempts, localhost included. Both plugins therefore share one record per management address: a refused key is remembered once, and neither plugin sends it again.

- **401 or 403** stops every call to that proxy, from both plugins. The segment shows `RL key?` until the key changes or 24 hours pass. The plugin compares a salted, truncated digest, never the key. Processes take turns under a lock in the state directory, so a refused key costs one failed attempt however many sessions render at once; a process that cannot take the lock, or cannot write the state directory, sends nothing.
- **An empty key is never sent**; an unreadable key source shows `RL key?` without any request.
- **404** on the management route means management is off, or the proxy is too old to have the route: the segment is hidden and the proxy is not asked again for an hour, and `bar check` says why. `bar rtellm on|off` and `bar rtellm status --probe` ask anyway, so a transient 404 never blocks the way out.
- **No `routellm` entry, or an entry pointing at another RouteLLM**: the segment is hidden, switching is refused, and `bar rtellm setup` shows what to add. The plugin never toggles an unrelated provider of the same name.
- **RouteLLM down while ON**: `RL on ✗`, with the session's tier marked `✗`. Whether a request falls through to the frontier accounts depends on the proxy; `route-private` fails. `bar rtellm off` is one command and never needs RouteLLM.
- **RouteLLM down on `bar rtellm on`**: refused by the check; `--force` overrides it.
- **Timeouts, 5xx**: back-off from one minute doubling to an hour, serving the last good state marked `~`. A RouteLLM that does not answer does not back off the management read.
- **The proxy replies before it applies a change**: the plugin reads the entry back a few times over about a second and a half. When it still does not show the change, or the request itself timed out, the command says the state is unknown and the cached state is dropped, so the next `bar rtellm status` or render reads the proxy again. The change is never sent twice.
- **A failed save of the proxy's configuration** is reported as an error; run the command again, or `bar rtellm off`.

Responses are size-capped (128 KiB for a management read and for the model list, 4 KiB for `/health`, 64 KiB for the cache), and a loopback proxy is contacted directly even when `HTTP_PROXY` is set.

## Privacy

- The plugin reads the proxy's RouteLLM provider entry (alias and tier names, priority, state), the proxy's Claude alias rows, and RouteLLM's `/health` and model list. It never sends a prompt or a completion request, never reads RouteLLM's log or its trace files, and never reads a client key or an OpenRouter key.
- The proxy returns the provider entry with its `api-key` entries and headers in plain text. The plugin parses only the fields it needs, discards the rest as it reads, and wipes the buffer; those fields reach no cache, output or log. This read is the only one that carries a secret, and the management key is kept like any other key: passed to `curl` on its standard input, never in a command line, an environment variable, bar's configuration, a cache file or an output.
- The only change it makes is the provider's `disabled` flag (and `priority` on ON), on an explicit action.
- Its cache (`plugins/routellm/<hash>.json` in bar's state directory, mode 0600 in a 0700 directory) holds the state, tier names and capabilities, the alias map, the last change and failure timers. No keys, no `api-key` entries or headers, no emails, no prompts, no file paths. The refusal digest lives in the record the two plugins share.
