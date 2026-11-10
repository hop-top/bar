# bar-cliproxyapi

`bar-cliproxyapi` shows the quota of the accounts behind a [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) proxy as the placeholder `#{cliproxyapi}`: one entry per account, each with its usage windows and the time they reset. It ships next to `bar` and is found like any [plugin](../plugins.md).

It is opt-in. Nothing runs, and nothing is read, until the placeholder is in your layout and the plugin's own configuration file exists.

```
set left "#{model} │ #{cliproxyapi} │ #{repo}:#{branch}"
```

Claude Code must reach the proxy through `ANTHROPIC_BASE_URL`; the segment is hidden for any other base URL.

## What it shows

Per account of the provider that matches the current model (`claude`, `opus`, `sonnet`, `haiku` for Claude; `gpt`, `codex`, `o3`-style names for Codex), one entry, most used first:

```
work 5h 59% ↻2h10m 7d 88% ↻3d4h │ home 7d 20% ↻5d
```

- `work` is the account's label. `5h 59%` is a window and the share **left**; `↻2h10m` is the time to its reset (`3d4h`, `2h10m`, `41m`, `<1m`). `count=used` draws the share used instead.
- A window the provider does not report is left out, never shown as 0%. Claude often reports only the weekly window; a Codex Pro plan has a weekly window only. An account never read yet shows `?`.
- The entry turns `bad` at 10% left or less in any shown window. Otherwise it follows the colour ramp of the lowest share left.
- `~` before the windows marks data older than a few minutes; data older than an hour is dropped.
- `key?` in the `bad` colour means the proxy refused the management key (see [Failures](#failures)).
- An account whose own token the provider refused shows in `bad`.
- The entries are items of one placeholder: with `set wrap on` a long line breaks between accounts, never inside one.
- Nothing is shown when the session does not use a configured instance, the model's provider is not one the instance reads, or no account has data.

What the plugin shows is what the proxy observes: it has no view of usage that did not go through that proxy (another machine, another client, the provider's own apps), so the figures can lag or differ from the provider's account page.

Claude windows come from the rate-limit headers the proxy records from real traffic through each account. They are held in the proxy's memory: an account that has served no request since the proxy started shows no windows yet, and a restart clears them. Codex windows are read from Codex's usage endpoint through the proxy, using each account's own token, only while a Codex model is active.

## Setup

1. **Enable the proxy's management API.** In the proxy's `config.yaml`, set `management.secret-key` to a password of your choice and restart. Without it the management routes answer 404, and the plugin stays hidden. Leave `management.allow-remote` off when the proxy runs on the same machine.

2. **Keep the plaintext key in a separate file.** The proxy hashes the `secret-key` with bcrypt on startup and writes the hash back into `config.yaml` (re-serialising the file), so the plaintext cannot stay there. Put it in its own file, readable only by you:

   ```sh
   umask 077
   printf '%s\n' 'the-secret-key' > ~/.config/cliproxyapi/management-key
   ```

3. **Create the plugin's configuration file**, `cliproxyapi.conf` in bar's configuration directory (`~/.config/bar/` on Linux, `~/Library/Application Support/bar/` on macOS; `$XDG_CONFIG_HOME/bar/` when set). It must be a regular file owned by you with no group or other permissions (`chmod 600`), at most 16 KiB; otherwise the plugin refuses it.

   ```
   instance http://127.0.0.1:8318
     key-file ~/.config/cliproxyapi/management-key
   ```

4. **Add `#{cliproxyapi}`** to a layout and run `bar check`: it reports an unreadable or too-open file, a missing key, and a proxy that has management turned off or has refused the key.

## Configuration file

One `instance` block per proxy; lines start with a keyword, `#` starts a comment, values may be quoted.

| Line | Meaning |
|---|---|
| `instance <url>` | The proxy, as Claude Code's `ANTHROPIC_BASE_URL` names it. Only scheme, host and port count (`HTTP://Localhost:8318/` equals `http://localhost:8318`); a path, query or user info is ignored or refused. A session whose base URL matches no instance shows nothing; there is no fallback to another instance. |
| `management <url>` | The management API base, ending in `/v8/management` (or `/v0/management`). Default: `<instance origin>/v8/management`. |
| `key-file <path>` | First line of an owner-only (0600) file, at most 1 KiB. `~/` is expanded. |
| `key-keychain <service>` | macOS: the generic password with that service name (`/usr/bin/security find-generic-password -s <service> -w`). |
| `key-command <program> [args...]` | The first line of the program's output. The program must be an absolute path; there is no shell, so no variables, pipes or globs. 5 s limit. |
| `providers <list>` | Providers to read, default `claude,codex`. |
| `alias "<glob>" <label>` | Label for accounts whose email matches the glob (`*` and `?`, case-insensitive); the first match wins. |
| `remote yes` | Allow a management host that is not on this machine. Requires an `https` management URL. |

Exactly one key source per instance. A block with a mistake is ignored as a whole, and `bar check` says which line.

Without an alias an account is labelled with the part of its email before the `@`; the option `label=index` draws `#1`, `#2` instead, for screen sharing.

The file is separate from `bar.conf` on purpose: a project's `.bar.conf` can replace plugin lines, and nothing that decides where the management key is sent, or which command runs, should be settable from a repository.

## Key sources

| Source | Notes |
|---|---|
| File | Refused unless it is a regular file owned by you with mode 0600. Symbolic links are followed and the target checked. |
| Keychain | macOS only. The plugin reads the item through `/usr/bin/security`; the key is read from a pipe, never passed as an argument. |
| Command | For a password manager: `key-command /opt/homebrew/bin/pass show cpa/management`. |

The key is held in memory that is overwritten when released, and passed to `curl` on its standard input. It never appears in a command line, an environment variable, bar's configuration, a cache file, the plugin's output or a log.

## Placeholder options

| Option | Default | Meaning |
|---|---|---|
| `provider` | `auto` | `auto` follows the model; `claude` or `codex` forces one. |
| `windows` | `5h,7d,wk` | Windows to show, in this order; a window an account lacks is omitted. `7d` is a window of exactly seven days, `wk` any other length of about a week. |
| `show` | `label,pct,reset` | Parts per account: `label`, `pct`, `reset`, `plan`. |
| `count` | `left` | `left` or `used`: which share `pct` draws. |
| `label` | `alias` | `alias` (the `alias` line, else the part of the email before `@`), `local` (always the part before `@`), `index` (`#1`, `#2`, for screen sharing) or `none`. Labels are cut at 12 characters. |
| `sort` | `used` | `used` (most used window first), `reset` (soonest reset first) or `name`. |
| `accounts` | all | Accounts to show: labels or local parts (`*` and `?` match), or positions in the proxy's list (`2` or `#2`). |
| `compact` | off | `59%·2h10m` per window, without the window name. |
| `stale` | `mark` | Old data: `mark` (`~`), `dim` or `hide`. |
| `link` | none | A link template for each account; `{account}`, `{label}`, `{provider}` and `{instance}` are URL-escaped. |
| `pool` | `mean` | How `usage` pools the accounts, below. |
| `base-url` | none | Base URL to use when `ANTHROPIC_BASE_URL` is not in the environment bar passes. |

`windows`, `show` and `accounts` are lists: separate the items with commas and the options with semicolons, `#{cliproxyapi[windows=5h,7d;count=used]}`.

```
set left "#{cliproxyapi[windows=5h,7d;compact;sort=reset]}"
default cliproxyapi stale=dim label=index
```

The plugin's `ttl` is 60 s and its time limit 15 s.

## Usage and account sources

Behind a proxy Claude Code reports no `rate_limits`, so `#{usage}` has nothing to show. The plugin supplies the pool's windows instead:

```
default cliproxyapi pool=mean
set usage-source cliproxyapi
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
```

`usage` covers the provider of the current model. With `set usage-source`, `#{usage}` shows it when Claude Code sends none, with the usual pace, projection and rules. Only the result for the placeholder's `default` options counts, so put `pool` on the `default` line.

| `pool` | `usage` is |
|---|---|
| `mean` | The mean share used over the accounts that have data, with the latest reset: the strictest pace. |
| `active` | The one account in the top priority tier, when there is exactly one; otherwise `mean`. |
| `max` | The most-used account, per window. |

A window is pooled only when every account with data reports it and has not yet reset, so a pool that holds a Codex Pro account has no 5h window. Accounts the proxy has disabled or cooled down, or whose token was refused, do not count (except for `active`, which follows the proxy's own routing).

The plugin also names the account the proxy prefers, for `#{account}`:

```
set account-source cliproxyapi
set right "#{cliproxyapi[windows=5h]} │ #{account}"
account "work" good
```

The `account` is emitted only when it is certain: the only account in the highest priority tier of the current provider that the proxy can route to. A round-robin pool of equals has no current account, and then `#{account}` shows what it showed before. A session bound to an account by the proxy's session affinity is not visible to the plugin. The account shown is the one the proxy **prefers**, not a guarantee about any request. With `label=index` the account's label is `#1`, `#2` too.

To open your own handler for an account instead, give each entry a link: `#{cliproxyapi[link=myproxy://use?account={account}]}`.

## Failures

The proxy bans a client IP for 30 minutes after five failed management attempts, localhost included, and a ban does not affect proxy traffic but blocks the management panel. The plugin therefore never retries a refusal:

- **401 or 403** from the management API stops every call. The segment shows `key?` until the key changes (the plugin compares a fingerprint, never the key) or 24 hours pass. That is at most one failed attempt per key.
- **An empty key is never sent**; an unreadable key source shows `key?` without any request.
- **404** means management is off (or an older proxy without `/v8`): the plugin tries the legacy `/v0/management` paths once, then waits an hour.
- **Timeouts, 5xx, bad responses**: back-off from one minute doubling to an hour, serving the last good data meanwhile.
- **An upstream 401 or 403 for one account** (its own token was refused) marks that account and pauses it for an hour; it does not touch the management key. An upstream 429 honours `Retry-After`.

Responses are size-capped (256 KiB for the account list, 128 KiB for an upstream call, 64 KiB for the cache), and a loopback proxy is contacted directly even when `HTTP_PROXY` is set. Plain `http` is used only for `localhost`, `127.0.0.1` and `::1`.

## Switching accounts

`bar-cliproxyapi` can also prefer one account. The proxy serves the highest *available* priority tier of a provider (a cooling or disabled account drops out), so "use X" sets X's priority one above every other available account of its provider. Nothing is disabled and no other account is changed; the proxy keeps the priority with the credential, across restarts.

```
bar-cliproxyapi list [--provider P] [--instance URL]
bar-cliproxyapi use <account> [--provider P] [--instance URL] [--dry-run]
bar-cliproxyapi use --reset [<account>] [--instance URL] [--dry-run]
```

- `list` prints one line per account: label, provider, priority, status (`active`, `unavailable`, `disabled`) and the proxy's account index. Never an email.
- `<account>` is an account index, a label (an alias, or the email's part before the `@`, any case) or a position (`3` or `#3`, as `label=index` draws). A name that fits several accounts (the same label under two providers) is refused and the candidates listed; narrow it with `--provider`. A disabled account is refused: bar never enables one.
- `use` prints the change (`work (claude): priority 0 -> 3`). An account already above every other available one, or with no available rival, is left as it is. `--dry-run` prints the change and sends nothing.
- `--reset` puts back the priorities this plugin changed (kept in the plugin's own state file, mode 0600, with account indexes and numbers only). An account whose priority was changed by someone else since is left alone. With an account, only that one is restored.
- The instance is `--instance`, else the session's `ANTHROPIC_BASE_URL`, else the only configured instance.
- Exit status: 0 done, 1 refused or failed (the reason on stderr), 2 bad usage.

The change applies to new selections. With the proxy's `session-affinity` on, a running session stays on the account it is bound to until that account fails over, and new sessions follow the priority. The proxy's `routing.strategy` still decides between accounts of the same tier.

### Clicking an account

The plugin declares the action `use` with the tokens `account` and `provider`. Bind it in `bar.conf` for either placement:

```
# the account segment: a click runs `use` for the account it shows
set account-source cliproxyapi
action account cliproxyapi.use

# the plugin's own segment: a click on an account's item runs it for that account
action cliproxyapi cliproxyapi.use
```

Links need the click handler installed with action support: run `bar click install` (again after upgrading bar). A click runs `bar-cliproxyapi --action use` with the tokens on its standard input; the plugin checks that the token names an account of its current listing, performs `use`, and exits non-zero with a reason when it refuses. bar shows the outcome in `action-status` in its state directory and refreshes the segment at the next render. A click has no session environment, so the plugin finds the instance whose listing holds the account; when two instances list the same index it refuses.

Management failures behave as for rendering: a 401 or 403, on the listing or on the change, stops every call until the key changes.

## Privacy

- The plugin reads accounts and quota. The only change it ever makes is an account's routing priority, when you ask (`use`, `--reset` or a bound click); it never disables or enables an account.
- Its output carries labels, the proxy's opaque account index and the provider name; bar's own cache of it (mode 0600) holds the same. `label=index` keeps even the local part of an email off the screen and out of that output.
- Its cache (`plugins/cliproxyapi/<hash>.json` in bar's state directory, mode 0600 in a 0700 directory) holds the label shown, the local part of the account's email (for `label=local`), the proxy's opaque account index and routing priority, plan name, usage windows and failure timers. It holds no emails (a label derived from one is the part before the `@`, or your alias), no file paths, no tokens, no account ids and no keys.
- Upstream addresses are compiled into the plugin. The proxy's `api-call` endpoint forwards an account's token to any URL it is given, so no configuration can choose where it goes.
- Querying a provider's usage endpoint with your own credentials, through your own proxy, is subject to that provider's terms; check them before enabling it.
