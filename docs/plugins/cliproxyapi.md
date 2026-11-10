# bar-cliproxyapi

`bar-cliproxyapi` shows the quota of the accounts behind a [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) proxy as the placeholder `#{cliproxyapi}`: one entry per account, each with its usage windows and the time they reset. It ships next to `bar` and is found like any [plugin](../plugins.md).

It is opt-in. Nothing runs, and nothing is read, until the placeholder is in your layout and the plugin's own configuration file exists.

```
set left "#{model} │ #{cliproxyapi} │ #{repo}:#{branch}"
```

Claude Code must reach the proxy through `ANTHROPIC_BASE_URL`; the segment is hidden for any other base URL.

## What it shows

Per account of the provider that matches the current model (`claude`, `opus`, `sonnet`, `haiku` for Claude; `gpt`, `codex`, `o3`-style names for Codex):

```
work 5h 41% ↻2h 7d 12% ↻3d │ home 7d 80% ↻5d
```

- `5h 41%` is a window and the share used; `↻2h` is the time to its reset.
- A window the provider does not report is left out, never shown as 0%. Claude often reports only the weekly window; a Codex Pro plan has a weekly window only.
- `~` after a name marks data older than a few minutes; data older than an hour is dropped.
- The text turns `warn` from 70% and `bad` from 90% used.
- `key?` in the `bad` colour means the proxy refused the management key (see [Failures](#failures)).

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
| `label` | `name` | `index` draws `#1`, `#2`. |
| `base-url` | none | Base URL to use when `ANTHROPIC_BASE_URL` is not in the environment bar passes. |

The plugin's `ttl` is 60 s and its time limit 15 s.

## Failures

The proxy bans a client IP for 30 minutes after five failed management attempts, localhost included, and a ban does not affect proxy traffic but blocks the management panel. The plugin therefore never retries a refusal:

- **401 or 403** from the management API stops every call. The segment shows `key?` until the key changes (the plugin compares a fingerprint, never the key) or 24 hours pass. That is at most one failed attempt per key.
- **An empty key is never sent**; an unreadable key source shows `key?` without any request.
- **404** means management is off (or an older proxy without `/v8`): the plugin tries the legacy `/v0/management` paths once, then waits an hour.
- **Timeouts, 5xx, bad responses**: back-off from one minute doubling to an hour, serving the last good data meanwhile.
- **An upstream 401 or 403 for one account** (its own token was refused) marks that account and pauses it for an hour; it does not touch the management key. An upstream 429 honours `Retry-After`.

Responses are size-capped (256 KiB for the account list, 128 KiB for an upstream call, 64 KiB for the cache), and a loopback proxy is contacted directly even when `HTTP_PROXY` is set. Plain `http` is used only for `localhost`, `127.0.0.1` and `::1`.

## Privacy

- The plugin reads accounts and quota only. It never changes the proxy.
- Its cache (`plugins/cliproxyapi/<hash>.json` in bar's state directory, mode 0600 in a 0700 directory) holds the label shown, the proxy's opaque account index, plan name, usage windows and failure timers. It holds no emails (a label derived from one is the part before the `@`, or your alias), no file paths, no tokens, no account ids and no keys.
- Upstream addresses are compiled into the plugin. The proxy's `api-call` endpoint forwards an account's token to any URL it is given, so no configuration can choose where it goes.
- Querying a provider's usage endpoint with your own credentials, through your own proxy, is subject to that provider's terms; check them before enabling it.
