# bar

![12fc](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/hop-top/bar/main/.12fcc.json)

Configurable status line for Claude Code: tmux-style format, placeholders, plugins.

```text
Opus 4.7[high] │ ctx 38% │ bar:main │ #131 #129 #128 │ !57 !42    4h40m █████ │ 6d ████░    work
```

Output of `bar preview`, colour stripped. In a terminal the usage bars run red to green by
pace, and repo, branch, PRs and issues are links. With budget left over in the last stretch
(`--state final`) the bars turn red:

```text
Opus 4.7[high] │ ctx 38% │ bar:main │ #131 #129 #128 │ !57 !42   25m ███░░ │ 20h0m ████░    work
```

## Install

```sh
cargo install --path .   # command: bar
```

## Use

Point Claude Code at it in `settings.json`:

```json
{ "statusLine": { "type": "command", "command": "bar render", "refreshInterval": 5 } }
```

Without a config file the built-in layout above is used. Write `bar.conf` to change it
([docs/config.md](docs/config.md)), then:

```sh
bar check     # lint the config; prints the file it read; exit 2 on error
bar preview   # layout with sample data; --state early|burning|final
```

## Docs

| Tier | For | Read |
|------|-----|------|
| User | configure, extend | [Configuration](docs/config.md), [Plugins](docs/plugins.md) |
| Contributor | build, test, send changes | [CONTRIBUTING.md](CONTRIBUTING.md) |
| Technical | design, caching, performance | [Technical notes](docs/technical.md) |

Vetting record for third-party crates: [docs/dependencies.md](docs/dependencies.md).
