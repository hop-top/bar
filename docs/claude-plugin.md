# Claude Code plugin

A Claude Code plugin that walks through setting up bar.

```text
/plugin marketplace add hop-top/bar
/plugin install bar@hop-top
/bar
```

`/bar [preset]` asks one question at a time:

1. Checks `bar` is on `PATH`; if not, shows the install options from the [README](../README.md#install).
2. Finds an existing `bar.conf`; keeps it or, on request, replaces it.
3. Shows `bar preview --preset` for every preset and writes the chosen one with `bar init --preset`.
4. Sets skin, palette and icons (`set skin`, `set palette`, `set icons`), then runs `bar check` and `bar preview`.
5. Runs `bar install --refresh 5` (user settings, or `--project` / `--local` on request) to show the `settings.json` diff, then the same with `--yes` after a yes. With a bar older than `bar install`, it shows this entry and adds it to `~/.claude/settings.json` on a yes:

```json
{ "statusLine": { "type": "command", "command": "bar render", "refreshInterval": 5 } }
```

Nothing is overwritten without a yes: an existing config, an existing `statusLine`, or other `settings.json` keys.

The plugin lives in `claude-plugin/skills/bar/SKILL.md` (a skill named like the plugin, so it runs as `/bar`; the namespaced `/bar:bar` also works); the marketplace manifest is `.claude-plugin/marketplace.json`. `make check` validates both with `claude plugin validate` (JSON syntax only when the `claude` CLI is absent).
