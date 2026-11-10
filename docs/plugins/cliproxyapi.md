# bar-cliproxyapi

The plugin is a `bar-cliproxyapi` program next to `bar`; its commands run through bar as `bar cliproxyapi …`, or with the alias `bar cpa …`.

`bar-cliproxyapi` shows the quota of the accounts behind a [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) proxy as the placeholder `#{cliproxyapi}`: one entry per account, each with its usage windows and the time they reset. It ships next to `bar` and is found like any [plugin](../plugins.md).

It is opt-in. Nothing runs, and nothing is read, until the placeholder is in your layout and the plugin's own configuration file exists.

```
set left "#{model} │ #{cliproxyapi} │ #{repo}:#{branch}"
```

Claude Code must reach the proxy through `ANTHROPIC_BASE_URL`; the segment is hidden for any other base URL. In a session with no `ANTHROPIC_BASE_URL` at all it is hidden too, but with exactly one instance configured the plugin still offers that instance's tokens, so `#{account}` can link the [`pick`](#clicking-an-account) action there.

## What it shows

Per account of the provider that matches the current model (`claude`, `opus`, `sonnet`, `haiku` for Claude; `gpt`, `codex`, `o3`-style names for Codex), one entry, most used first:

```
work 5h 59% ↻2h10m 7d 88% ↻3d4h │ home 7d 20% ↻5d
```

- `work` is the account's [label](#labels-and-colours). `5h 59%` is a window and the share **left**; `↻2h10m` is the time to its reset (`3d4h`, `2h10m`, `41m`, `<1m`). `count=used` draws the share used instead.
- The window name is left out only when every account shown has exactly one window and it is the same one for all (`innoprofits 66% ↻5d │ personal 100% ↻5d16h`, all weekly). Any mix keeps every name, so no number is left without its window: `work 5h 40% ↻2h 7d 66% ↻5d │ innoprofits 7d 66% ↻5d`. The `pick` text follows the same rule.
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
| `key-file <path>` | First line of an owner-only (0600) file, at most 1 KiB. `~/` is expanded (against `HOME`, `USERPROFILE` on Windows). On Windows the owner-only check is not made; keep the file in your profile directory, whose ACL keeps other users out. |
| `key-keychain <service>` | macOS: the generic password with that service name (`/usr/bin/security find-generic-password -s <service> -w`). |
| `key-command <program> [args...]` | The first line of the program's output. The program must be an absolute path; there is no shell, so no variables, pipes or globs. 5 s limit. |
| `providers <list>` | Providers to read, default `claude,codex`. |
| `alias "<glob>" <label>` | Label for accounts whose email matches the glob (`*` and `?`, case-insensitive, matched against the first 256 characters); the first match wins. |
| `remote yes` | Allow a management host that is not on this machine. Requires an `https` management URL. |

Exactly one key source per instance. A block with a mistake is ignored as a whole, and `bar check` says which line.

Without an alias an account is labelled with the part of its email before the `@`; the option `label=index` draws `#1`, `#2` instead, for screen sharing.

### Labels and colours

The plugin asks bar for your `account "<glob>" <label> <color>` lines of `bar.conf` (it declares [`inputs.accounts`](../plugins.md#account-rules)) and matches each glob against the account's email, where the email is visible, case-insensitively. An account is named by:

1. the first matching `alias` line of `cliproxyapi.conf`, else
2. the first matching `account` line of `bar.conf`, else
3. the part of the email before the `@`.

The colour is that of the first matching `account` line, also when an `alias` supplied the label (an alias has no colour). The label is drawn in that colour in each entry, and the account `#{account}` shows (see below) is drawn as is, in it. The same names are accepted and shown by `bar cliproxyapi list`, `use` and `pick` and by the `pick` chooser, which get the lines from bar (`bar cpa …` and clicks alike; a plugin started directly, without `bar`, sees none). Labels are cut at 24 characters (12 in the segment).

```
account "*@corp.example" work 33
account "me@*" personal "#7aa2f7"
```

Only the label and colour are kept in the plugin's cache, never the glob or the email. A changed line shows at the next collection, at most a minute or two later.

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
| `label` | `alias` | `alias` (the `alias` line, else the part of the email before `@`), `index` (`#1`, `#2`, for screen sharing) or `none`. Labels are cut at 12 characters. |
| `sort` | `used` | `used` (most used window first), `reset` (soonest reset first) or `name`. |
| `accounts` | all | Accounts to show: labels or local parts (`*` and `?` match), or positions (`2` or `#2`, counted as `label=index` counts: among the enabled accounts of the provider). |
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

- **401 or 403** from the management API stops every call. The segment shows `key?` until the key changes or 24 hours pass; a refused key is tried again at most once a day. The plugin compares a salted, truncated digest, never the key. Processes of one proxy take turns under a lock in the state directory, so a refused key costs one failed attempt however many sessions render at once; a run that cannot take the lock, or cannot write the state directory, sends nothing. After a 401 or 403 on an upstream call the remaining calls of the run are not sent, and the first upstream call of a run goes alone.
- **An empty key is never sent**; an unreadable key source shows `key?` without any request.
- **404** means management is off (or an older proxy without `/v8`): the plugin tries the legacy `/v0/management` paths once, then waits an hour.
- **Timeouts, 5xx, bad responses**: back-off from one minute doubling to an hour, serving the last good data meanwhile. The same holds per account for its upstream reads. A run starts no new upstream call after 10 s and saves what it has learned as it goes.
- **An upstream 401 or 403 for one account** (its own token was refused) marks that account and pauses it for an hour; it does not touch the management key. An upstream 429 honours `Retry-After`.

Responses are size-capped (256 KiB for the account list, 128 KiB for an upstream call, 64 KiB for the cache), and a loopback proxy is contacted directly even when `HTTP_PROXY` is set. Plain `http` is used only for `localhost`, `127.0.0.1` and `::1`.

## Switching accounts

`bar cliproxyapi` (bar runs the `bar-cliproxyapi` plugin for any `bar <name> …`) can also prefer one account. The proxy serves the highest *available* priority tier of a provider (a cooling or disabled account drops out for the moment), so "use X" sets X's priority one above every other enabled account of its provider, including one only cooling down (it returns after the cooldown). Nothing is disabled and no other account is changed; the proxy keeps the priority with the credential, across restarts.

```
bar cliproxyapi list [--provider P] [--instance URL]   # or: bar cpa list …
bar cliproxyapi use <account> [--provider P] [--instance URL] [--dry-run]
bar cliproxyapi use --next [--strategy most-left|soonest-reset] [--provider P] [--instance URL] [--dry-run]
bar cliproxyapi use --reset [<account>] [--instance URL] [--dry-run]
bar cliproxyapi pick [--strategy most-left|soonest-reset] [--provider P] [--instance URL]
```

- `list` prints one line per account: label (see [Labels and colours](#labels-and-colours)), provider, priority, status (`active`, `unavailable`, `disabled`) and the proxy's account index. Never an email.
- `<account>` is an account index, a label (an alias, an `account` line of `bar.conf`, or the email's part before the `@`, any case; the label shown is the name, so an account renamed by an alias or line answers to that name only) or a position (`3` or `#3`, as `label=index` draws: among the provider's enabled accounts; with several providers in the instance a position needs `--provider`). A name that fits several accounts (the same label under two providers) is refused and the candidates listed; narrow it with `--provider`. A disabled account is refused: bar never enables one.
- `use` prints the change (`work (claude): priority 0 -> 3`). An account already above every other enabled one, or with no enabled rival, is left as it is. `--dry-run` prints the change and sends nothing.
- `--reset` puts back the priorities this plugin changed (kept in the plugin's own state file, mode 0600, with account indexes and numbers only). An account whose priority was changed by someone else since is left alone; a hand edit made between two switches becomes the priority a reset restores. With an account, only that one is restored.
- `--next` switches to the account a strategy recommends (see [Choosing the next account](#choosing-the-next-account)); with several providers in the instance it needs `--provider`. `pick` lists the accounts in the recommended order and, on a terminal, asks which to use. The [`pick` action](#clicking-an-account) does the same in a native macOS chooser.
- The instance is `--instance`, else the session's `ANTHROPIC_BASE_URL`, else the only configured instance.
- Exit status: 0 done, 1 refused or failed (the reason on stderr), 2 bad usage.

The change applies to new selections. With the proxy's `session-affinity` on, a running session stays on the account it is bound to until that account fails over, and new sessions follow the priority. The proxy's `routing.strategy` still decides between accounts of the same tier.

### Choosing the next account

A strategy ranks the enabled accounts of one provider. An account's *left* is the smallest share of quota left over its windows (100 minus the largest percent used; a window whose reset has passed counts as unused). An account with no window data is *unread*. The window that limits an account gives its *reset*, when its quota returns.

| Strategy | Order |
|---|---|
| `most-left` (default) | Left, highest first; a tie goes to the sooner reset. |
| `soonest-reset` | The accounts within 10 points of the best left come first, soonest reset first, so quota about to expire is used before it is lost; the rest follow by left. |

Both put available accounts with quota left first, then exhausted ones, then unread ones, then those the proxy has cooling down; disabled accounts are left out. `use --next` switches to the first account of the first group that is not the proxy's current preferred one, and refuses when there is none (an account with no data is never picked blindly). Quota comes from the proxy's listing and the plugin's cache, so a Codex account's figure is as fresh as the last background read.

`pick` prints one numbered row per account in that order, with each window's percent left and time to reset, and `*` on the account the proxy prefers (the window names are left out when every row has exactly one window and it is the same, as in the segment):

```
 1    work   claude  5h 80% left ↻1h20m  7d 55% left ↻3d2h
 2    home   claude  5h 60% left ↻4h10m  7d 90% left ↻5d
 3  * lab    claude  5h 10% left ↻20m
 4    spare  claude  no data
```

On a terminal it then asks `use which? [1-4, q to quit]` and switches as `use` does; a blank line or `q` quits. Without a terminal (a pipe, a script) it only prints the list.

### Clicking an account

The plugin declares the action `use` with the tokens `account` and `provider`, and the actions `next` (`most-left`), `next-soon` (`soonest-reset`) and `pick` with the tokens `provider` and `instance`, which its output always carries at the top level. Bind one in `bar.conf` for either placement:

```
# the account segment: a click runs `use` for the account it shows
set account-source cliproxyapi
action account cliproxyapi.use

# the plugin's own segment: a click on an account's item runs it for that account
action cliproxyapi cliproxyapi.use

# a click on #{account} switches to the next account, whichever shows
action account cliproxyapi.next
action account cliproxyapi.next-soon   # or: among the most left, the soonest reset
```

`pick` (macOS only) opens a native chooser instead of switching at once: the provider's accounts in the `most-left` order, each with its windows' percent left and time to reset, the proxy's preferred account marked `[current]`, the top recommendation selected. OK switches to the account chosen exactly as `use` does (the previous priority is recorded for `use --reset`); Cancel changes nothing and the status reads `ok: cancelled`. The list is passed to `/usr/bin/osascript` as arguments of a fixed script, so no account name can alter the script. Waiting for you, the action may run up to 120 s instead of the usual 10. On other systems the plugin does not declare `pick`, so `bar check` reports a binding of it, and running it refuses.

```
action account cliproxyapi.pick
```

With exactly one instance configured, `pick` is linked in sessions that do not use the proxy too: `#{account}` keeps showing the signed-in account, and a click offers the instance's Claude accounts. With several instances there is no link in such a session, since it is not clear which proxy is meant.

`next` and `next-soon` are linked even when `#{account}` shows bar's own signed-in account (a round-robin pool has no single preferred account); `use` needs the account the plugin named.

Links need the click handler installed with action support: run `bar click install` (again after upgrading bar). A click runs `bar-cliproxyapi --action <action>` with the tokens on its standard input; the plugin checks that the token names an account of its current listing, performs `use`, and exits non-zero with a reason when it refuses. bar shows the outcome in `action-status` in its state directory and refreshes the segment at the next render. A click has no session environment, so the plugin finds the instance whose listing holds the account; when two instances list the same index it refuses.

Management failures behave as for rendering: a 401 or 403, on the listing or on the change, stops every call until the key changes.

## Privacy

- The plugin reads accounts and quota. The only change it ever makes is an account's routing priority, when you ask (`use`, `--reset` or a bound click); it never disables or enables an account.
- Its output carries labels, the proxy's opaque account index and the provider name; bar's own cache of it (mode 0600) holds the same. `label=index` keeps the local part of an email off the screen and out of that output; the cache below still holds the label it replaced.
- Its cache (`plugins/cliproxyapi/<hash>.json` in bar's state directory, mode 0600 in a 0700 directory) holds the label shown (your alias, else your `account` line's label, else the part of the email before the `@`) and its colour, the proxy's opaque account index and routing priority, plan name, usage windows and failure timers. It holds no emails (a label derived from one is the part before the `@`, your alias or your `account` line's label; the lines' globs, which may hold an address, are matched and dropped), no file paths, no tokens, no account ids and no keys, only a salted, truncated digest of a refused key beside its random salt. The proxy is not called at all when this directory cannot be written, since a refusal could not be remembered; `bar check` says so.
- Upstream addresses are compiled into the plugin. The proxy's `api-call` endpoint forwards an account's token to any URL it is given, so no configuration can choose where it goes.
- Querying a provider's usage endpoint with your own credentials, through your own proxy, is subject to that provider's terms; check them before enabling it.
