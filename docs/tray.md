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
