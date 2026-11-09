# Claude Code plugin

A Claude Code plugin that walks through setting up bar.

```text
/plugin marketplace add hop-top/bar
/plugin install bar@hop-top
/bar:setup
```

`/bar:setup [preset]` asks one question at a time:

1. Checks `bar` is on `PATH`; if not, shows the install options from the [README](../README.md#install).
2. Finds an existing `bar.conf`; keeps it or, on request, replaces it.
3. Shows `bar preview --preset` for every preset and writes the chosen one with `bar init --preset`.
4. Sets skin, palette and icons (`set skin`, `set palette`, `set icons`), then runs `bar check` and `bar preview`.
5. Wires `statusLine` through `bar install` when the installed bar has it; otherwise adds this entry to `~/.claude/settings.json`:

```json
{ "statusLine": { "type": "command", "command": "bar render", "refreshInterval": 5 } }
```

Nothing is overwritten without a yes: an existing config, an existing `statusLine`, or other `settings.json` keys.

The plugin lives in `claude-plugin/`; the marketplace manifest is `.claude-plugin/marketplace.json`. `make check` validates both with `claude plugin validate` (JSON syntax only when the `claude` CLI is absent).
