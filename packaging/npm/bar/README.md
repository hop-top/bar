# @hop-top/bar

A status line for [Claude Code](https://code.claude.com/docs/en/statusline) that any script can extend. This package installs the prebuilt `bar`, `bar-cliproxyapi` and `bar-routellm` binaries for macOS, Linux (static) and Windows, x64 and arm64.

```sh
npx @hop-top/bar preview --preset   # try it: every preset, with sample data
npm install -g @hop-top/bar         # or: bun add -g @hop-top/bar
bar install --refresh 5             # set bar as the status line in ~/.claude/settings.json
```

npm picks the binary package for your platform (`@hop-top/bar-<os>-<arch>`, an optional dependency); nothing is downloaded at install time and no install script runs. `bar install` writes the path of that binary into Claude Code's settings, so the status line runs it directly, without Node. `bar-cliproxyapi` and `bar-routellm` sit next to it, where `bar` finds them as plugins.

Documentation, configuration and plugins: [github.com/hop-top/bar](https://github.com/hop-top/bar).
