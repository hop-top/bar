# Gallery

What the styling options look like. Every image is real `bar preview` output for the config shown under it (sample data, `bar render` with seeded history for sparklines), drawn with Hack Nerd Font Mono on a dark background. Regenerate with `make gallery`; `make check` fails when these files are stale. Options: [Configuration](config.md).

## Skins

`set skin` picks how segments are drawn. Powerline and capsule glyphs need a Nerd Font in the terminal. See [Skins](config.md#skins).

![Plain skin, default palette: segments split by muted bars.](gallery/skin-plain.svg)

`COLUMNS=100 bar preview --state early` with:

```text
set palette default
set icons nerdfont
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

![Powerline skin, nord palette: segments on coloured backgrounds joined by arrows.](gallery/skin-powerline.svg)

`COLUMNS=100 bar preview --state early` with:

```text
set skin powerline
set palette nord
set icons nerdfont
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

![Capsule skin, dracula palette: each segment in a rounded pill.](gallery/skin-capsule.svg)

`COLUMNS=100 bar preview --state early` with:

```text
set skin capsule
set palette dracula
set icons nerdfont
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

![Powerline skin with skin-glyphs ascii: > and < instead of Nerd Font arrows, default palette.](gallery/skin-ascii.svg)

`COLUMNS=100 bar preview --state early` with:

```text
set skin powerline
set skin-glyphs ascii
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

![Powerline skin with bg=52 on ctx and bg=22 on the 5-hour usage: per-segment backgrounds over the palette's, nord palette.](gallery/skin-bg.svg)

`COLUMNS=100 bar preview --state early` with:

```text
set skin powerline
set palette nord
set icons nerdfont
set left "#{model}#{effort} │ #{ctx[bg=52]} │ #{repo}:#{branch}"
set center "#{usage[type=5h,bg=22]} │ #{usage[type=7d]}"
set right "#{account}"
```

## Palettes

`set palette` swaps muted text, state colours, the usage ramp and the skin backgrounds; here on the powerline skin in the `burning` state. `terminal` uses the terminal's own 16 colours, drawn here with the xterm defaults. See [Palettes](config.md#palettes).

![Powerline skin with the default palette.](gallery/palette-default.svg)

`COLUMNS=100 bar preview --state burning` with:

```text
set skin powerline
set palette default
set icons nerdfont
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

![Powerline skin with the nord palette.](gallery/palette-nord.svg)

`COLUMNS=100 bar preview --state burning` with:

```text
set skin powerline
set palette nord
set icons nerdfont
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

![Powerline skin with the gruvbox palette.](gallery/palette-gruvbox.svg)

`COLUMNS=100 bar preview --state burning` with:

```text
set skin powerline
set palette gruvbox
set icons nerdfont
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

![Powerline skin with the dracula palette.](gallery/palette-dracula.svg)

`COLUMNS=100 bar preview --state burning` with:

```text
set skin powerline
set palette dracula
set icons nerdfont
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

![Powerline skin with the solarized palette.](gallery/palette-solarized.svg)

`COLUMNS=100 bar preview --state burning` with:

```text
set skin powerline
set palette solarized
set icons nerdfont
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

![Powerline skin with the mono palette.](gallery/palette-mono.svg)

`COLUMNS=100 bar preview --state burning` with:

```text
set skin powerline
set palette mono
set icons nerdfont
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

![Powerline skin with the terminal palette.](gallery/palette-terminal.svg)

`COLUMNS=100 bar preview --state burning` with:

```text
set skin powerline
set palette terminal
set icons nerdfont
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

## Bar glyphs

`glyphs=` on `usage` picks the cells of the bar; 10 cells wide here, `burning` state (25% and 30% left), so `eighths` shows its partial last cell.

![Usage bars drawn with the blocks glyph set, default palette.](gallery/glyphs-blocks.svg)

`COLUMNS=60 bar preview --state burning` with:

```text
set palette default
set left "#{usage[type=5h,glyphs=blocks,width=10]} │ #{usage[type=7d,glyphs=blocks,width=10]}"
```

![Usage bars drawn with the shades glyph set, nord palette.](gallery/glyphs-shades.svg)

`COLUMNS=60 bar preview --state burning` with:

```text
set palette nord
set left "#{usage[type=5h,glyphs=shades,width=10]} │ #{usage[type=7d,glyphs=shades,width=10]}"
```

![Usage bars drawn with the dots glyph set, gruvbox palette.](gallery/glyphs-dots.svg)

`COLUMNS=60 bar preview --state burning` with:

```text
set palette gruvbox
set left "#{usage[type=5h,glyphs=dots,width=10]} │ #{usage[type=7d,glyphs=dots,width=10]}"
```

![Usage bars drawn with the squares glyph set, dracula palette.](gallery/glyphs-squares.svg)

`COLUMNS=60 bar preview --state burning` with:

```text
set palette dracula
set left "#{usage[type=5h,glyphs=squares,width=10]} │ #{usage[type=7d,glyphs=squares,width=10]}"
```

![Usage bars drawn with the bars glyph set, solarized palette.](gallery/glyphs-bars.svg)

`COLUMNS=60 bar preview --state burning` with:

```text
set palette solarized
set left "#{usage[type=5h,glyphs=bars,width=10]} │ #{usage[type=7d,glyphs=bars,width=10]}"
```

![Usage bars drawn with the braille glyph set, terminal palette.](gallery/glyphs-braille.svg)

`COLUMNS=60 bar preview --state burning` with:

```text
set palette terminal
set left "#{usage[type=5h,glyphs=braille,width=10]} │ #{usage[type=7d,glyphs=braille,width=10]}"
```

![Usage bars drawn with the eighths glyph set, mono palette.](gallery/glyphs-eighths.svg)

`COLUMNS=60 bar preview --state burning` with:

```text
set palette mono
set left "#{usage[type=5h,glyphs=eighths,width=10]} │ #{usage[type=7d,glyphs=eighths,width=10]}"
```

## Solid and gradient bars

`color=solid` (default) paints every filled cell in the colour of the current value; `color=gradient` runs the filled cells along the ramp. Both on the default palette, whose ramp has eleven steps.

![10-cell usage bars with color=solid, default palette, burning state.](gallery/color-solid.svg)

`COLUMNS=60 bar preview --state burning` with:

```text
set palette default
set left "#{usage[type=5h,color=solid,width=10,show=bar,pct]} │ #{usage[type=7d,color=solid,width=10,show=bar,pct]}"
```

![10-cell usage bars with color=gradient, default palette, burning state.](gallery/color-gradient.svg)

`COLUMNS=60 bar preview --state burning` with:

```text
set palette default
set left "#{usage[type=5h,color=gradient,width=10,show=bar,pct]} │ #{usage[type=7d,color=gradient,width=10,show=bar,pct]}"
```

## Projections and count

`eta` projects the time until the budget runs out at the burn so far (`→ok` when it lasts), `proj` the used percent at reset; `count=used` fills the bar and percentage with what is used. `burning` state. See [usage](config.md#usage).

![Usage with show=time,bar,eta,proj, default palette, burning state.](gallery/usage-eta-proj.svg)

`COLUMNS=80 bar preview --state burning` with:

```text
set palette default
set left "#{usage[type=5h,show=time,bar,eta,proj]} │ #{usage[type=7d,show=time,bar,eta,proj]}"
```

![Usage with count=used and show=pct,bar: the bar fills with what is used, default palette, burning state.](gallery/usage-count-used.svg)

`COLUMNS=60 bar preview --state burning` with:

```text
set palette default
set left "#{usage[type=5h,count=used,show=pct,bar]} │ #{usage[type=7d,count=used,show=pct,bar]}"
```

## Spark and gauge

`spark` draws the used percent of the last samples, `gauge` one cell for the current value. `bar preview` keeps no history, so these come from `bar render` with a sample session (5-hour window 90% used, weekly 60%) and five seeded history samples per window (15, 35, 55, 75, 90 and 10, 20, 30, 45, 60 percent used).

![5-hour and weekly usage as time, sparkline, gauge cell and percentage, solarized palette.](gallery/spark-gauge.svg)

`COLUMNS=60 bar render` with:

```text
set palette solarized
set left "#{usage[type=5h,show=time,spark,gauge,pct]} │ #{usage[type=7d,show=time,spark,gauge,pct]}"
```

![Sparklines with color=gradient, each cell coloured by its own value, default palette.](gallery/spark-gradient.svg)

`COLUMNS=60 bar render` with:

```text
set palette default
set left "#{usage[type=5h,show=spark,bar,color=gradient,width=8]} │ #{usage[type=7d,show=spark,bar,color=gradient,width=8]}"
```

## Pace states

`bar preview --state` picks the sample usage; the colour follows the pace score, flipped in the final stretch.

![State early: plenty of budget left; bars green, gruvbox palette.](gallery/state-early.svg)

`COLUMNS=100 bar preview --state early` with:

```text
set palette gruvbox
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

![State burning: spending fast; bars orange to yellow, gruvbox palette.](gallery/state-burning.svg)

`COLUMNS=100 bar preview --state burning` with:

```text
set palette gruvbox
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

![State final: last stretch with budget left over; red means budget going unused, gruvbox palette.](gallery/state-final.svg)

`COLUMNS=100 bar preview --state final` with:

```text
set palette gruvbox
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

## Icons

`set icons nerdfont` adds a muted glyph before each segment, replacing the text label where there is one. `set icons emoji` draws emoji instead; their look depends on the viewer's emoji font, so it is shown as text below. See [Icons](config.md#icons).

![Plain skin with icons none, dracula palette.](gallery/icons-none.svg)

`COLUMNS=100 bar preview --state early` with:

```text
set palette dracula
set icons none
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} #{dirty} │ #{prs[limit=3]}"
set right "#{usage[type=5h]} │ #{usage[type=7d]}"
```

![Plain skin with icons nerdfont, dracula palette.](gallery/icons-nerdfont.svg)

`COLUMNS=100 bar preview --state early` with:

```text
set palette dracula
set icons nerdfont
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} #{dirty} │ #{prs[limit=3]}"
set right "#{usage[type=5h]} │ #{usage[type=7d]}"
```

`set icons emoji`, as text, colour stripped:

```text
🤖 Opus 4.7⚡ [high] │ 🧠 38% │ 📁 bar:🌿 main ✏️ +2 ~3 ?1 │ 🔀 #131❌ #129 #128 │ ⏳ 4h40m █████ │ 📅 6d ████░
```

`COLUMNS=100 bar preview` with:

```text
set icons emoji
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} #{dirty} │ #{prs[limit=3]}"
set right "#{usage[type=5h]} │ #{usage[type=7d]}"
```

## Lines

`set left@2` and friends add lines, up to 64. The `two-line` preset puts session and git on line 1 and usage below. `set wrap on` flows a line too long for `COLUMNS` onto more rows instead. See [Lines and wrapping](config.md#lines-and-wrapping).

![The two-line preset with the nord palette.](gallery/two-line.svg)

`COLUMNS=100 bar preview --state early` with:

```text
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} #{dirty} #{ahead} │ #{prs[limit=3,author=@me]} │ #{issues[limit=3,assignee=@me]}"
set right "#{account}"
set left@2 "#{usage[type=5h]} │ #{usage[type=7d]} │ #{spendhist}"
set right@2 "#{sessions}"
set palette nord
```

![One long line with set wrap on at COLUMNS=60: segments flow onto rows, left-aligned, capsule skin, gruvbox palette.](gallery/wrap.svg)

`COLUMNS=60 bar preview --state early` with:

```text
set skin capsule
set palette gruvbox
set wrap on
set left "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch}"
set center "#{usage[type=5h]} │ #{usage[type=7d]}"
set right "#{account}"
```

## PR checks

`checks=` on `prs` marks CI state after each number: #131 failing, #129 pending, #128 passing. The default marks failures only. See [PR checks](config.md#pr-checks).

![PR numbers with the default checks=fail: only #131 is marked, terminal palette.](gallery/checks-default.svg)

`COLUMNS=40 bar preview --state early` with:

```text
set palette terminal
set left "#{repo}:#{branch} │ #{prs}"
```

![PR numbers with checks=fail,pending,pass: a mark after each, terminal palette.](gallery/checks-all.svg)

`COLUMNS=40 bar preview --state early` with:

```text
set palette terminal
set left "#{repo}:#{branch} │ #{prs[checks=fail,pending,pass]}"
```

![The same marks as Nerd Font glyphs with icons nerdfont, gruvbox palette.](gallery/checks-nerdfont.svg)

`COLUMNS=40 bar preview --state early` with:

```text
set palette gruvbox
set icons nerdfont
set left "#{repo}:#{branch} │ #{prs[checks=fail,pending,pass]}"
```
