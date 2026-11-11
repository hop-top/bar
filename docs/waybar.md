# Waybar

`bar waybar` prints one line of JSON for a waybar [custom module](https://github.com/Alexays/Waybar/wiki/Module:-Custom): the tray's state as a short text, a tooltip and a CSS class. It needs no tray host, so it suits Hyprland, Sway and niri setups that prefer a text module to a tray menu. Plugins and actions are configured as for the [tray](tray.md#config); nothing here runs without a `tray` line.

```
$ bar waybar
{"text":"alice · 1 waiting","tooltip":"<b>cliproxyapi</b>\nClaude · :8317\n  ● alice (5h 59% · 7d 71%)\n  ○ bob","class":"good","alt":"good"}
```

- `text`: the checked item of the first two picker groups, then the number of sessions waiting; `bar` when there is nothing to show.
- `tooltip`: every plugin, group and item, a marker for the checked state, the details, and the last action's outcome.
- `class` and `alt`: the worst item `style` (`bad`, `warn`, `good`), `bad` also when the last action failed, `idle` when no item has a style. A plugin that is missing or answers nothing shows as `bad`.
- `text` and `tooltip` are Pango markup: `&`, `<` and `>` from a plugin are escaped.

It runs the same builder as [`bar tray state`](tray.md#state), so it costs one run of the granted plugins' `--menu`, in parallel, each within 3 s, and it writes the same `tray-menu.json`. A poll every 30 s is fine; a slow plugin delays the line once and no longer than 3 s. A config error prints a `bad` line telling you to run `bar check`.

## Config

```jsonc
"custom/bar": {
  "exec": "bar waybar",
  "return-type": "json",
  "interval": 30,
  "signal": 8,
  "tooltip": true,
  "on-click": "~/.config/waybar/bar-pick.sh"
}
```

Add `"custom/bar"` to `modules-right`. Style it by class:

```css
#custom-bar.good { color: #a6e3a1; }
#custom-bar.warn { color: #f9e2af; }
#custom-bar.bad  { color: #f38ba8; }
#custom-bar.idle { opacity: 0.6; }
```

Waybar starts `bar` with a minimal `PATH`; use the absolute path in `exec` if `bar` is not found. Waybar caps the line at 256 KiB, far above what the snapshot's limits allow.

## Refresh after an action

`signal: 8` makes waybar re-run the module on `SIGRTMIN+8`. Send it after anything that changes the state:

```sh
pkill -RTMIN+8 waybar
```

## Picker

A click cannot open a menu in a text module; open a launcher instead. [`bar tray run`](tray.md) takes the dispatch id of a snapshot item, `<plugin>.<group>.<item>`, runs its action and takes `--confirmed` for items that ask for it and `--generation` to refuse a choice made on an older snapshot. This script lists the enabled, clickable items of `tray-menu.json` in fuzzel, runs the chosen one, then refreshes the module (swap `fuzzel --dmenu` for `wofi --dmenu` or `rofi -dmenu`):

```sh
#!/bin/sh
# ~/.config/waybar/bar-pick.sh
snap="${BAR_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/bar}/tray-menu.json"
gen=$(jq -r .generation "$snap")
pick=$(jq -r '.plugins[] | .name as $p | .groups[] | .id as $g | .items[]
    | select(.action != null and (.enabled // true))
    | "\($p).\($g).\(.id)\t\(if .checked then "● " else "  " end)\($p) \(.label)"' "$snap" |
  fuzzel --dmenu --with-nth=2 --accept-nth=1 -d "$(printf '\t')") || exit 0
[ -n "$pick" ] || exit 0
bar tray run "$pick" --generation "$gen" || bar tray run "$pick" --generation "$gen" --confirmed
pkill -RTMIN+8 waybar
```

The second `bar tray run` in the example confirms blindly; for items with `"confirm": true` prompt first (a second `fuzzel --dmenu` with "yes" and "no") and pass `--confirmed` only on yes. Each `bar waybar` poll writes a new snapshot with a new generation, so a picker that stays open across a poll is refused; run the script again.
