# Tray

`bar-tray` puts bar's plugin actions in the macOS menu bar, the Windows notification area or a Linux StatusNotifierItem tray: an account picker, a routing toggle, buttons and status rows, as a native menu. It is a thin client: everything it shows comes from [`bar tray state`](#state) and everything it runs goes through [`bar tray run`](#running-actions), so the checks a status-line click passes apply to the menu too. Nothing shows until you opt a plugin in with a `tray` line. Waybar users can show the same state as a text module: see [Waybar](waybar.md).

## Quick start

1. Put `bar-tray` in the same directory as `bar`. From a checkout: `cargo install --path crates/bar-tray --locked` (on Linux add `--features linux-sni`, see [Linux](#linux)); `cargo install --path . --locked` puts `bar` in the same place.
2. Add a `tray` line to your config for every plugin whose menu you want, for example `tray cliproxyapi`. `bar check` tells you when a plugin cannot show one.
3. Run `bar tray install --dry-run` to see what it writes, then `bar tray install`. The tray starts now and at every login.

`bar tray status` says whether it is installed and running; `bar tray uninstall` removes it again.

## Config

```
tray myproxy                     # every menu action of the plugin
tray myproxy.use                 # one action only
set tray-views on                # also list the view toggles (default off)
set tray-poll 2m                 # idle poll (default 30s)
```

- `tray <plugin>[.<action>]` names a plugin whose menu the tray may show, and optionally one of its declared actions. Names are letters, digits, `-` and `_`. Repeating a line changes nothing.
- A `tray` line is your decision, in your config only. A project `.bar.conf` cannot add one, in any mode; the same goes for `set tray-views` and `set tray-poll`.
- `action` lines do not imply `tray`: binding a click in the status line does not put the action in the tray menu. `bar tray install` prints a suggested `tray` line for every `action` line no `tray` line covers yet, and leaves your config alone.
- With `tray <plugin>.<action>` only, the plugin's other items are still shown, disabled, each with the line that would enable it.
- `set tray-views` takes `on` or `off`.
- `set tray-poll` takes a duration with a unit (`s`, `m`), from `10s` to `10m` (default `30s`), and is checked like any other option. It is the time the tray waits after a refresh before the next idle one. The running tray picks a change up with its next refresh, with nothing to reinstall or restart; [`bar-tray --poll`](#bar-tray-command-line), when given, wins over it.

`bar check` warns about a `tray` line whose plugin is not on `PATH`, does not declare the `menu` capability, or does not declare the named action, and reports invalid option values.

## The menu

The menu has one section per plugin with a `tray` line, in config order, then the tray's own rows. A plugin's [menu](plugins.md#menu) maps to rows like this:

| Group kind | Shown as |
|---|---|
| `radio` | a submenu named after the group, the current choice checked |
| `check` | one check item per toggle |
| `buttons` | plain items that run once |
| `info` | disabled text rows |

An item's `detail` is shown on the same row after a dash. Labels are cut to 80 characters. After the plugin sections come, each only when there is something to show:

- `Last: ok: <note> · 12 s ago` (or `refused: …`): the outcome of the last action run from a menu or a status-line click, aged every second;
- `Sessions: 1 waiting · 2 done`: Claude Code sessions waiting for you and finished, from the hooks `bar tray install` registers;
- the view toggles (`5h usage`, `7d usage` and each plugin's alternate text), with `set tray-views on`; choosing one runs `bar toggle <key>`;
- `Quit`, always.

Choosing an item:

- runs `bar tray run <plugin>.<group>.<item>` with the snapshot's generation; the row reads `<label> …` and is disabled until it ends, and no other item runs meanwhile;
- for an action declared with `confirm`, the first choice only turns the row into `Confirm: <label>`; choosing it again within 10 s runs it;
- a refusal or failure marks the icon (see the table below) and puts the reason in the tooltip for 10 s; the `Last:` row shows the outcome from the next refresh.

The tray asks `bar tray state --json` for a fresh menu every time the menu opens, after every action, and while idle once `set tray-poll` (30 s by default) has passed since the last refresh. It watches no files and opens no socket. When `bar` cannot be run or gives no snapshot, the menu keeps the last one, and the icon stays marked with the error in the tooltip until a snapshot arrives. Until the first snapshot the menu holds only `Quit`.

## Platforms

| | macOS | Windows 10/11 | Linux (SNI hosts) |
|---|---|---|---|
| Where | menu bar, no Dock icon | notification area; a new icon starts in the overflow | the panel's StatusNotifierItem tray: KDE Plasma, waybar's `tray` module, GNOME with the extension, other SNI hosts |
| Icon | template image, tinted by the menu bar | dark or light glyph, swapped when the taskbar theme changes | `bar-tray-symbolic` from the icon theme, drawn in the panel's colours; built-in image when the theme lacks it |
| After a refusal | dot on the icon, reason in the tooltip | reason in the tooltip | `NeedsAttention` status (dot on the icon where the host shows it), reason in the tooltip |
| Toggles | check items | check items | check items |
| Picker | submenu, chosen item checked | submenu, chosen item checked | submenu with a radio group |
| Item `style` (`good`, `warn`, `bad`, `dim`) | not shown | not shown | passed to the host, which may tint the row |
| Left click | opens the menu | opens the menu | opens the menu |
| Refresh | on every click on the icon | on every click; a snapshot that lands while the menu is open shows from the next open | when the host announces the menu; some hosts redraw an open menu, some do not |
| Starts at login | LaunchAgent | `Run` value | autostart entry, or systemd user unit with `--systemd` |
| One instance | lock file | lock file and a per-session mutex | lock file |
| Build | default | default | cargo feature `linux-sni` |

### macOS

The tray is a bare executable, not an app bundle: it sets itself up as an accessory app, so it has no Dock icon and no app menu. It has no Developer ID signature and is not notarized. A file downloaded with a browser carries the quarantine attribute, and Gatekeeper refuses to open an unsigned binary that has it; a build from source does not.

### Windows

`bar-tray.exe` is a GUI program: it opens no console window, so it prints nothing, and a failure shows only as its exit code. Windows puts a new notification icon in the overflow area (the `^` next to the clock); `bar tray install` says how to keep it visible the first time: drag it onto the taskbar, or turn `bar-tray` on in Settings > Personalization > Taskbar > Other system tray icons. Windows keeps that choice across restarts, since the icon keeps one identity.

`bar.exe` and `bar-tray.exe` are not code-signed. A copy downloaded from the internet makes SmartScreen show "Windows protected your PC" the first time it runs: choose "More info", then "Run anyway", or clear the download mark beforehand (file Properties > Unblock, or `Unblock-File` in PowerShell). With Smart App Control turned on, Windows blocks unsigned programs and offers no override.

### Linux

The Linux backend speaks the StatusNotifierItem and dbusmenu protocols on the session bus directly, without GTK or libappindicator. It is behind the cargo feature `linux-sni`, which is off by default:

```sh
cargo install --path crates/bar-tray --locked --features linux-sni
bar-tray --backend   # prints `sni`; without the feature it says there is no backend and exits 1
```

A tray started before the panel (at login, say) waits for it, and comes back when the panel restarts. Menu labels are visible to every program on the session bus, which is why plugin menus carry no secrets.

**GNOME** shows tray icons only with the AppIndicator and KStatusNotifierItem support extension: install `gnome-shell-extension-appindicator` and enable it with `gnome-extensions enable appindicatorsupport@rgcjonas.gmail.com`. Ubuntu ships it enabled. `bar tray install` prints this reminder when `XDG_CURRENT_DESKTOP` names GNOME.

**Hyprland, sway and other compositors** show the icon in waybar's `tray` module. Under uwsm, `bar tray install --systemd` binds the tray to `graphical-session.target`. Sessions that run neither autostart entries nor that target (Hyprland started without uwsm, sway from a TTY) install with `--no-autostart` and start the tray from their own config, for example in Hyprland:

```
exec-once = bar tray start
```

Without a `linux-sni` build, or without a tray host at all, [`bar waybar`](waybar.md) shows the same state as a waybar text module and runs items through a launcher.

## Install

```
bar tray install [--dry-run] [--no-autostart] [--no-hooks] [--systemd]
bar tray uninstall [--keep-hooks]
bar tray start | stop | status [--json]
```

`bar tray install` sets the tray up for the environment it runs in:

- writes `tray-dirs.json` in the state directory (mode 0600): the absolute `bar` and `bar-tray` paths, the state and Claude Code dirs they resolve to, and the dir variables (`BAR_STATE_DIR`, the XDG bases, `CLAUDE_CONFIG_DIR`, `TMUX_TMPDIR`, `PATH`) as set at install;
- writes the `tray-installed` marker, which also publishes the renderer's state dir to processes started without your shell's environment;
- registers the Claude Code hooks that feed the session rows, as `bar click install --hooks-only` does (`--no-hooks` skips this);
- prints a `tray` line for every `action` line no `tray` line covers yet: `action` lines do not grant the tray, so these are only suggested, and your config is not changed;
- on macOS, writes the LaunchAgent `~/Library/LaunchAgents/top.hop.bar-tray.plist` and loads it with `launchctl bootstrap gui/<uid>`, which starts the tray now and at every login. The agent runs `bar-tray --bar <bar> --lock <state>/tray.lock` with the recorded variables, in the GUI session only, and restarts it after a crash but not after `Quit`;
- on Windows, sets the per-user value `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\bar-tray` to `"<dir>\bar-tray.exe" --dirs "<state>\tray-dirs.json"` (with `reg.exe` from the system directory) and starts the tray now. A `Run` value cannot set variables, so the tray reads `bar`, its lock and the recorded variables from `tray-dirs.json`; the state directory is `%LOCALAPPDATA%\bar` unless `BAR_STATE_DIR` or `XDG_STATE_HOME` moved it. The command line must fit the 260 characters Windows allows there. Nothing else in the registry is written: Windows puts a new icon in the taskbar overflow (the `^` next to the clock) and only you can pin it, so the first install says how (drag it onto the taskbar, or turn `bar-tray` on in Settings > Personalization > Taskbar > Other system tray icons);
- on Linux, writes the icon `bar-tray-symbolic.svg` to `$XDG_DATA_HOME/icons/hicolor/scalable/apps/` (default `~/.local/share`), so panels draw it in their own colours, then the autostart entry `$XDG_CONFIG_HOME/autostart/bar-tray.desktop` (default `~/.config`), which GNOME, Plasma, XFCE, Cinnamon and LXQt run at login, and starts the tray now. With `--systemd` it writes the user unit `$XDG_CONFIG_HOME/systemd/user/bar-tray.service` instead (`WantedBy=graphical-session.target`, restarted after a crash but not after `Quit`), runs `systemctl --user daemon-reload` and `enable`, and starts it; installing one kind removes the other. Both run `bar-tray --bar <bar> --lock <state>/tray.lock` with the recorded variables. On GNOME it prints a reminder that GNOME Shell shows tray icons only with the AppIndicator extension (`gnome-shell-extension-appindicator`; Ubuntu ships it enabled).

`--no-autostart` skips the autostart step; platforms other than macOS, Windows and Linux need it for now.

The Linux tray needs a `bar-tray` built with the cargo feature `linux-sni` (off by default). Install asks `bar-tray --backend` first and, without that backend, refuses to set up autostart, which would only fail at every login; `--no-autostart` still records the dirs and installs the icon. For sessions that run no autostart at all, see [Linux](#linux).

`--dry-run` prints every file it would write and every command it would run, and changes nothing. `bar-tray` must be in the same directory as `bar`; install stops with `no bar-tray at <path>` otherwise.

### Start, stop, status

One tray runs per state directory: a running tray holds a lock on `tray.lock` and wrote its pid there and in `tray.pid` (Windows keeps a locked file unreadable to other processes), and a second one exits at once. `bar tray start` starts the tray unless it runs, through `launchctl` when the LaunchAgent is installed, `systemctl --user start` when the unit is, else in the background with the command line install recorded. `bar tray stop` unloads the LaunchAgent (it loads again at the next login) or stops the unit, and stops a tray that still holds the lock: SIGTERM, or on Windows `taskkill` (the tray quits on the close request; `taskkill /F` when no window takes it). The `Run` value and the autostart entry stay, so the tray starts again at the next login. `bar tray status` says whether the tray is installed, starts at login and runs, and how old the menu snapshot is:

```
$ bar tray status
installed  yes
state dir  /Users/me/Library/Application Support/bar
autostart  /Users/me/Library/LaunchAgents/top.hop.bar-tray.plist
running    yes (pid 4242)
menu       written 12 s ago
```

`--json` prints the same as one object: `installed`, `state_dir`, `autostart` (a path or `null`), `running`, `pid` and `snapshot_age` (seconds or `null`).

### Uninstall

`bar tray uninstall` stops the tray and removes the LaunchAgent, the `Run` value, or the unit (disabled first), the autostart entry and the icon, then `tray-menu.json`, `tray-dirs.json`, the lock, `tray.pid` and the marker. It removes the hooks only when its install added them, `bar click install` is not installed (its `click-installed` marker is absent) and `--keep-hooks` is not given. `bar click install` and `bar tray install` are independent: either works without the other.

## bar-tray command line

`bar tray install` and `bar tray start` run the tray for you; these flags are for running it by hand or from a compositor config.

```
bar-tray [--bar <path>] [--lock <file>] [--dirs <file>] [--poll <seconds>]
bar-tray --snapshot <file> [--print]
bar-tray --print
bar-tray --backend
```

- `--bar <path>`: the `bar` to run. Without it, the `bar` in the same directory as `bar-tray`; `PATH` is never searched.
- `--lock <file>`: hold a lock on this file while running; a second tray given the same file exits at once (with status 0). Install passes `<state>/tray.lock`.
- `--dirs <file>`: read the `tray-dirs.json` install wrote and take its `bar`, its lock and its variables, unless `--bar` or `--lock` say otherwise. The Windows logon command passes it, since a `Run` value cannot set variables.
- `--poll <seconds>`: the idle poll, 10 to 600, whatever `set tray-poll` says. Without it the tray follows `set tray-poll` (30 s by default). Install does not pass it.
- `--snapshot <file>`: show a snapshot file instead of asking `bar`, for trying a menu out. Choosing an item then fails with `no bar to run` unless `--bar` is given too.
- `--print`: print the menu once as text (`[x]` checked, `>` submenu, `---` separator, flags and row ids) and exit, without showing anything.
- `--backend`: print the tray backend of this build (`macos`, `windows`, `sni`) and exit; exit 1 when there is none.

The tray passes its children only the variables that move a directory `bar` reads (`HOME`, `BAR_STATE_DIR`, the XDG bases, `CLAUDE_CONFIG_DIR`, `TMUX_TMPDIR`, `PATH`; on Windows `USERPROFILE`, `APPDATA`, `LOCALAPPDATA` and `SYSTEMROOT` in place of `HOME`), never through a shell, with no stdin. An action is stopped 5 s past its declared limit (125 s at most); `bar tray state` and `bar toggle` get 15 s.

## State

`bar tray state` builds what the tray shows and writes it to `tray-menu.json` in the state directory (mode 0600, replaced atomically); `--json` prints the same object. It reads the toggle files, `action-status` and the session markers, then runs `bar-<plugin> --menu` for every plugin with a `tray` line, all at once, each within 3 s. A plugin that is not on `PATH`, does not declare the `menu` capability or gives no menu gets one disabled row saying so. Items whose action the plugin does not declare are dropped; an item whose action has no `tray` line is shown disabled, with the line that would enable it. The render cache is never touched.

```json
{"version": 1, "generation": "7c1f0a9e…", "at": 1762800000, "poll": 30,
 "last_action": {"at": 1762799988, "ok": true, "note": "preferred alice"},
 "sessions": {"waiting": 1, "done": 2},
 "views": [{"key": "5h", "label": "5h usage", "on": true}],
 "plugins": [{"name": "myproxy", "groups": [
   {"id": "claude@8317", "label": "Claude · :8317", "kind": "radio", "items": [
     {"id": "3", "label": "alice", "checked": true, "action": "use",
      "tokens": {"account": "3", "provider": "claude"}}]}]}]}
```

- `generation` identifies the runnable items: each item's id, action, token values, `enabled` and `confirm`, in order. The tray hands it back when it runs an item, so a choice made on a menu whose items have since moved is refused. Labels, details, the checked row, styles, the view toggles and the last action do not change it: a run that merely refreshes them (an idle poll, `bar waybar`) keeps the generation a tray holds.
- `poll` is `set tray-poll` in seconds: how long the tray waits before its next idle refresh.
- `views` is empty unless `set tray-views on`: the usage views (`5h`, `7d`) and the alternate text of every plugin with a `tray` line, each flipped with `bar toggle <key>`.
- `plugins` lists the plugins in `tray` line order, each with the groups and items of its [menu](plugins.md#menu); `confirm` and `limit` (seconds the action may run: 10, or its declared `timeout` up to 120) come from the action's declaration.

## Running actions

`bar tray run <plugin>.<group>.<item> [--confirmed] [--generation <id>]` runs one item of the snapshot. The command line carries the item's id and flags only: the plugin, the action and every token value come from `tray-menu.json`, which only `bar tray state` writes. On success the action's first line is printed; otherwise one line on stderr says why, the exit code is 1 and nothing ran. The outcome goes to `action-status` either way, so the next snapshot shows it.

Each check refuses on its own, in this order:

| Check | Refusal |
|---|---|
| The id has three parts in their character sets, at most 96 characters | `bad item id` |
| `tray-menu.json` exists in the state directory | `no menu, reopen it` |
| It is a plain file, mode 0600, owned by you | `menu file is not private` (or `… owned by another user`) |
| It was written within the last 10 minutes and is a version 1 snapshot | `menu is stale, reopen it` |
| `--generation`, when given, is the snapshot's | `menu changed, reopen it` |
| The item is in the snapshot, enabled, with an action and well-formed tokens | `item is not in the menu`, `item is disabled`, … |
| A `tray` line in your config covers the action (`action` lines do not count) | ``no `tray` line for <plugin>.<action>`` |
| `bar-<plugin>` is found and its `--ext-info` declares the action with exactly the item's token names | `plugin not found`, `the plugin does not declare that action`, … |
| An action declared with `confirm` was given `--confirmed` | `needs confirmation` |

Then the action runs exactly as a [`bar://action` link](plugins.md#actions) would: `bar-<plugin> --action <action>` with `{"action", "tokens"}` on stdin, within its declared limit. Afterwards the snapshot is spent (its modification time is set to zero), so the same click cannot run twice: the tray fetches a new menu with `bar tray state` before anything else can run.

Ten minutes is the longest idle poll a tray accepts: a tray that is running refreshes the snapshot at least that often and on every menu open, so an older file was left by a tray that is gone, and its tokens (account indices, instance ids) may no longer mean what they did.

The link handler's nonce and cache-entry binding are not used here. The nonce proves a `bar://` URL was made by this install, because any web page can open one; the tray exposes no URL, no socket and no pipe, so there is nothing for another origin to reach. The cache entry ties a link to the render it was drawn from; for the tray that render is the snapshot itself, which is private, fresh and identified by its generation.
