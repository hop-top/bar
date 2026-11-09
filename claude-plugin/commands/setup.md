---
description: Set up the bar status line (install check, preset, skin, palette, icons, statusLine)
argument-hint: "[preset]"
allowed-tools: Bash(bar:*), Bash(command -v:*), Bash(test:*), Bash(mkdir:*), Read, Edit, Write, AskUserQuestion
---

Guide the user through setting up `bar`, a status line for Claude Code. Go step by step, one question at a time (use AskUserQuestion when available), and keep replies short. Never overwrite an existing config or `settings.json` without asking.

Optional preset from the user: $ARGUMENTS

## 1. Check bar is installed

Run `command -v bar && bar --version`.

If missing, show these options and stop until the user has installed it, then re-check:

- Download an archive from https://github.com/hop-top/bar/releases and put `bar` on `PATH`.
- From source with a Rust toolchain (https://rustup.rs): `git clone https://github.com/hop-top/bar.git && cd bar && cargo install --path . --locked`.

## 2. Existing config

Config search order: `$XDG_CONFIG_HOME/bar/bar.conf`, `~/.config/bar/bar.conf`, then the platform config dir for `bar` (macOS: `~/Library/Application Support/bar/bar.conf`). If one exists, tell the user which, and ask: keep it and only change skin, palette or icons in it, or replace it. Replacing means asking before deleting or moving the file (`bar init` refuses when a config exists).

## 3. Preset

Presets: `minimal`, `full`, `powerline`, `capsule`, `compact`, `two-line`. Run `bar preview --preset` (bare: every preset in turn) and show the user the output (it is ANSI; show the result as-is, or describe each preset in one line). Ask which to use, unless given in the arguments.

With no existing config (or after the user agreed to replace it): `bar init --preset <name>`.

## 4. Skin, palette, icons

Each is a `set` line in `bar.conf`; the preset already sets some. Ask in turn, showing the current value, and offer "keep":

- Skin: `plain`, `powerline`, `capsule`. Powerline and capsule need a Nerd Font or Powerline-patched font; otherwise add `set skin-glyphs ascii`.
- Palette: `default`, `nord`, `gruvbox`, `dracula`, `solarized`, `mono`, `terminal` (`terminal` follows the terminal's own theme).
- Icons: `none`, `nerdfont`, `emoji` (`nerdfont` needs a Nerd Font).

Edit the existing `set skin|palette|icons` line, or append one if the preset has none. Never write two of the same. Then run `bar check` (exit 2 means a config error: show it and fix) and `bar preview` to show the result; loop back to any step the user wants to change.

## 5. Wire the status line

Check `bar install --help`.

- If it succeeds, run `bar install` (read its help first for options and for whether it asks before touching `settings.json`).
- If it fails, edit `~/.claude/settings.json` yourself, after showing the user the change and getting a yes: merge, keeping every other key, this entry:

```json
"statusLine": { "type": "command", "command": "bar render", "refreshInterval": 5 }
```

If a different `statusLine` already exists, show it and ask before replacing it. If you cannot edit the file, print the entry for the user to add.

## 6. Finish

Tell the user the status line appears on the next Claude Code interaction (restart if not), and that `docs/config.md` in https://github.com/hop-top/bar documents every option.
