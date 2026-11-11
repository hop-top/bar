# Tray

A tray app shows bar's toggles and plugin actions as a native menu. This page covers the config that opts a plugin in; the app itself is documented with it.

## Config

```
tray myproxy                     # every menu action of the plugin
tray myproxy.use                 # one action only
set tray-views on                # also list the view toggles (default off)
set tray-poll 30s                # idle poll, 10s to 10m
```

- `tray <plugin>[.<action>]` names a plugin whose menu the tray may show, and optionally one of its declared actions. Names are letters, digits, `-` and `_`. Repeating a line changes nothing.
- A `tray` line is your decision, in your config only. A project `.bar.conf` cannot add one, in any mode; the same goes for `set tray-views` and `set tray-poll`.
- `action` lines do not imply `tray`: binding a click in the status line does not put the action in the tray menu.
- `set tray-views` takes `on` or `off`. `set tray-poll` takes a duration with a unit (`s`, `m`), from `10s` to `10m`.

`bar check` warns about a `tray` line whose plugin is not on `PATH`, does not declare the `menu` capability, or does not declare the named action, and reports invalid option values.

## State

`bar tray state` builds what the tray shows and writes it to `tray-menu.json` in the state directory (mode 0600, replaced atomically); `--json` prints the same object. It reads the toggle files, `action-status` and the session markers, then runs `bar-<plugin> --menu` for every plugin with a `tray` line, all at once, each within 3 s. A plugin that is not on `PATH`, does not declare the `menu` capability or gives no menu gets one disabled row saying so. Items whose action the plugin does not declare are dropped; an item whose action has no `tray` line is shown disabled, with the line that would enable it. The render cache is never touched.

```json
{"version": 1, "generation": "7c1f0a9e…", "at": 1762800000,
 "last_action": {"at": 1762799988, "ok": true, "note": "preferred alice"},
 "sessions": {"waiting": 1, "done": 2},
 "views": [{"key": "5h", "label": "5h usage", "on": true}],
 "plugins": [{"name": "myproxy", "groups": [
   {"id": "claude@8317", "label": "Claude · :8317", "kind": "radio", "items": [
     {"id": "3", "label": "alice", "checked": true, "action": "use",
      "tokens": {"account": "3", "provider": "claude"}}]}]}]}
```

- `generation` is new on every run: the tray hands it back when it runs an item, so a choice made on an older menu is refused.
- `views` is empty unless `set tray-views on`: the usage views (`5h`, `7d`) and the alternate text of every plugin with a `tray` line, each flipped with `bar toggle <key>`.
- `plugins` lists the plugins in `tray` line order, each with the groups and items of its [menu](plugins.md#menu); `confirm` and `limit` (seconds the action may run: 10, or its declared `timeout` up to 120) come from the action's declaration.
