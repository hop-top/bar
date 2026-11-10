//! Wires config, placeholders and layout into the status line text.

use crate::cache::Cache;
use crate::color::{Color, Depth};
use crate::conf::{Config, LoadResult, Wrap};
use crate::expand::{CommandRunner, Expander};
use crate::format::{parse, segments, Token};
use crate::gradient::{self, Extent, Ink};
use crate::icons;
use crate::layout::{flow, join, place_groups, plain_joiner, Group};
use crate::look::{self, Look};
use crate::options::Options;
use crate::palette;
use crate::placeholders::{lookup_in, Ctx};
use crate::plugin;
use crate::session::Session;
use crate::skin::{self, Paint, Seg, Side};
use crate::style::{self, dim, visible_width_with, ITEM_BREAK};

/// SGR parameters in force after `text`: each `ESC[..m` sequence adds its
/// parameters (a colour as one unit); `0`, `39`, `49` and `22` take away
/// what they reset.
fn sgr_after(mut active: Vec<String>, text: &str) -> Vec<String> {
    let mut rest = text;
    while let Some(i) = rest.find("\x1b[") {
        let body = &rest[i + 2..];
        let Some(end) = body.find(|c: char| !(c.is_ascii_digit() || c == ';')) else {
            break;
        };
        let (params, tail) = body.split_at(end);
        rest = &tail[tail.starts_with('m') as usize..];
        if !tail.starts_with('m') {
            continue;
        }
        let kind = |p: &str| match p.split(';').next().unwrap_or("").parse::<u32>() {
            Ok(30..=38 | 90..=97) => 'f',
            Ok(40..=48 | 100..=107) => 'b',
            Ok(1 | 2) => 'w',
            _ => 'o',
        };
        match params {
            "" | "0" => active.clear(),
            "39" => active.retain(|p| kind(p) != 'f'),
            "49" => active.retain(|p| kind(p) != 'b'),
            "22" => active.retain(|p| kind(p) != 'w'),
            p => {
                // a colour replaces the one before it
                let k = kind(p);
                if k == 'f' || k == 'b' {
                    active.retain(|q| kind(q) != k);
                }
                active.push(p.to_string());
            }
        }
    }
    active
}

/// A segment's text cut at its [`ITEM_BREAK`]s, each cut dropping the
/// space after it (colour codes may sit between the two); one piece when
/// there is no break. Colour and weight in force at a cut are closed at
/// the end of the piece and set again at the start of the next, so every
/// row stands alone.
fn item_pieces(text: &str) -> Vec<String> {
    let mut parts = text.split(ITEM_BREAK);
    let mut out: Vec<String> = parts.next().map(String::from).into_iter().collect();
    let mut active = sgr_after(Vec::new(), out.first().map_or("", String::as_str));
    for part in parts {
        let mut head = 0;
        // only SGR codes (a gradient's colour) can sit before the space
        let bytes = part.as_bytes();
        while bytes[head..].starts_with(b"\x1b[") {
            match part[head..].find('m') {
                Some(n) => head += n + 1,
                None => break,
            }
        }
        let rest = part[head..].strip_prefix(' ').unwrap_or(&part[head..]);
        let reopen: String = active.iter().map(|p| format!("\x1b[{p}m")).collect();
        if !active.is_empty() {
            if let Some(prev) = out.last_mut() {
                prev.push_str("\x1b[0m");
            }
        }
        let piece = format!("{reopen}{}{rest}", &part[..head]);
        active = sgr_after(active, &format!("{}{rest}", &part[..head]));
        out.push(piece);
    }
    out
}

use std::path::PathBuf;

/// Cold cache: every `%command%` expands to nothing.
pub struct ColdRunner;

impl CommandRunner for ColdRunner {
    fn run(&self, _cmd: &str) -> Option<String> {
        None
    }
}

pub const CONFIG_ERROR: &str = "bar: config error, run bar check";

/// `builtin_config` as config text.
pub const BUILTIN_CONF: &str = "set left \"#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=3;author=@me]} │ #{issues[limit=3;assignee=@me]}\"
set center \"#{usage[type=5h]} │ #{usage[type=7d]}\"
set right \"#{account}\"
";

/// Layout used when no config file exists.
pub fn builtin_config() -> Config {
    Config {
        left: "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=3;author=@me]} │ #{issues[limit=3;assignee=@me]}".into(),
        center: "#{usage[type=5h]} │ #{usage[type=7d]}".into(),
        right: "#{account}".into(),
        ..Config::default()
    }
}

/// Stands in for every placeholder in a preview: no plugins, no real data.
/// The flag says whether the text label is drawn (false when an icon is).
/// For a name that is not a core placeholder it gets the options with
/// `icon` still in them and draws label and icon itself.
pub type Sample<'a> = &'a dyn Fn(&str, &Options, &Ctx, bool) -> Option<String>;

/// Core names first; anything else is a `bar-<name>` plugin. Returns the
/// value, styled by `fg`, `bold` and `gradient`, and the `bg` option.
fn render_placeholder(
    name: &str,
    inline: &Options,
    ctx: &Ctx,
    ex: &Expander,
    sample: Option<Sample>,
) -> (String, Option<Color>) {
    let (look, opts) = Look::split(&expanded(name, inline, ctx, ex), ctx.config);
    (
        look.paint(
            render_value(name, &opts, ctx, sample),
            ctx.config.icon_width,
        ),
        look.bg,
    )
}

fn render_value(name: &str, opts: &Options, ctx: &Ctx, sample: Option<Sample>) -> String {
    let core = lookup_in(ctx, name);
    // a subagent row draws no plugin
    if core.is_none() && ctx.session.task.is_some() {
        return String::new();
    }
    // a plugin sample paints its own label and icon, from the full options
    if let (Some(sample), None) = (sample, core) {
        return sample(name, opts, ctx, true).unwrap_or_default();
    }
    let (pick, opts) = icons::split(opts);
    let glyph = icons::resolve(
        ctx.config,
        &pick,
        core.and_then(|_| icons::builtin(ctx.config.icons, name, &opts)),
    );
    let label = glyph.is_none();
    let with_icon = |value: Option<String>| {
        let value = value.unwrap_or_default();
        match &glyph {
            Some(g) if !value.is_empty() => icons::attach(g, value),
            _ => value,
        }
    };
    if let Some(sample) = sample {
        return with_icon(sample(name, &opts, ctx, label));
    }
    let found = match core {
        Some(_) => None,
        None => match plugin::find(ctx.plugin_dirs, name) {
            Some(f) => Some(f),
            None => return String::new(),
        },
    };
    match (core, found) {
        (Some(p), _) => {
            let v = with_icon(p.render_labeled(ctx, &opts, label));
            if name == "account" && !v.is_empty() {
                plugin::link_account(ctx, v)
            } else {
                v
            }
        }
        (None, Some(f)) => plugin::render_found_with(ctx, &f, &opts, &pick).unwrap_or_default(),
        (None, None) => String::new(),
    }
}

/// Inline options over the configured defaults, values expanded.
fn expanded(name: &str, inline: &Options, ctx: &Ctx, ex: &Expander) -> Options {
    let merged = match ctx.config.defaults.get(name) {
        Some(d) => inline.merged_over(d),
        None => inline.clone(),
    };
    let mut opts = Options::default();
    for (k, v) in merged.iter() {
        opts.set(k, &ex.expand(v));
    }
    opts
}

/// A rendered segment and the first `bg` among its placeholders.
struct Rendered {
    text: String,
    bg: Option<Color>,
}

/// Text tokens survive only beside non-empty placeholders, so `repo:` loses
/// its colon when the branch is empty. Between two non-empty placeholders
/// one text is kept, the one after the last empty placeholder, so
/// `#{branch} #{dirty} #{ahead}` with a clean tree reads `main ↑2`.
fn render_segment(
    seg: &[Token],
    ctx: &Ctx,
    ex: &Expander,
    sample: Option<Sample>,
) -> Option<Rendered> {
    let mut bg = None;
    let values: Vec<Option<String>> = seg
        .iter()
        .map(|t| match t {
            Token::Placeholder { name, options } => {
                let (v, b) = render_placeholder(name, options, ctx, ex, sample);
                bg = bg.or(b);
                Some(v)
            }
            Token::Text(_) => None,
        })
        .collect();
    let has_placeholder = values.iter().any(Option::is_some);
    if has_placeholder && values.iter().flatten().all(|v| v.is_empty()) {
        return None;
    }
    let keep = kept_text(&values);
    let mut out = String::new();
    for (i, t) in seg.iter().enumerate() {
        match (t, &values[i]) {
            (Token::Text(s), _) if keep[i] => out.push_str(&dim(s)),
            (_, Some(v)) => out.push_str(v),
            _ => {}
        }
    }
    Some(Rendered { text: out, bg })
}

/// Which text tokens (`None` in `values`) are drawn. A text beside an empty
/// placeholder is dropped, except that every stretch between two non-empty
/// placeholders keeps its last text, so the values stay apart.
fn kept_text(values: &[Option<String>]) -> Vec<bool> {
    let empty = |i: usize| values.get(i).is_some_and(|v| v.as_deref() == Some(""));
    let full = |i: usize| values[i].as_deref().is_some_and(|v| !v.is_empty());
    let mut keep: Vec<bool> = (0..values.len())
        .map(|i| values[i].is_none() && !(i > 0 && empty(i - 1)) && !empty(i + 1))
        .collect();
    let fulls: Vec<usize> = (0..values.len()).filter(|&i| full(i)).collect();
    for w in fulls.windows(2) {
        let between = w[0] + 1..w[1];
        if !between.clone().any(empty) {
            continue;
        }
        if let Some(last) = between.clone().rev().find(|&i| values[i].is_none()) {
            for i in between {
                keep[i] = i == last;
            }
        }
    }
    keep
}

fn render_group(src: &str, ctx: &Ctx, ex: &Expander, sample: Option<Sample>) -> Vec<Rendered> {
    let Ok(tokens) = parse(src) else {
        return Vec::new();
    };
    segments(&tokens, &ctx.config.separator)
        .iter()
        .filter_map(|seg| render_segment(seg, ctx, ex, sample))
        .collect()
}

pub fn render(
    session: &Session,
    config: &Config,
    columns: Option<usize>,
    runner: &dyn CommandRunner,
) -> String {
    render_with(session, config, columns, runner, None)
}

pub fn render_with(
    session: &Session,
    config: &Config,
    columns: Option<usize>,
    runner: &dyn CommandRunner,
    cache: Option<&Cache>,
) -> String {
    let dirs = plugin::plugin_dirs(std::env::var_os("PATH").as_deref());
    render_in(session, config, columns, runner, cache, &dirs)
}

/// As `render_with`, looking plugins up in `plugin_dirs` instead of `PATH`.
pub fn render_in(
    session: &Session,
    config: &Config,
    columns: Option<usize>,
    runner: &dyn CommandRunner,
    cache: Option<&Cache>,
    plugin_dirs: &[PathBuf],
) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let src = Sources {
        runner,
        cache,
        plugin_dirs,
        sample: None,
    };
    render_impl(session, config, columns, now, &src)
}

/// Renders `config` at `now` (Unix seconds) with every placeholder
/// answered by `sample`: no clock, cache, command or plugin.
pub fn render_sample(
    session: &Session,
    config: &Config,
    columns: Option<usize>,
    now: i64,
    sample: Sample,
) -> String {
    let src = Sources {
        runner: &ColdRunner,
        cache: None,
        plugin_dirs: &[],
        sample: Some(sample),
    };
    render_impl(session, config, columns, now, &src)
}

/// Where placeholder values come from.
struct Sources<'a> {
    runner: &'a dyn CommandRunner,
    cache: Option<&'a Cache>,
    plugin_dirs: &'a [PathBuf],
    sample: Option<Sample<'a>>,
}

fn render_impl(
    session: &Session,
    config: &Config,
    columns: Option<usize>,
    now: i64,
    from: &Sources,
) -> String {
    let ex = Expander::new(from.runner);
    let sample = from.sample;
    let ctx = Ctx {
        session,
        config,
        now,
        runner: from.runner,
        cache: from.cache,
        columns,
        plugin_dirs: from.plugin_dirs,
        memo: Default::default(),
    };
    let _muted = style::use_muted(config.muted);
    let _ansi = style::use_ansi(config.palette.ansi);
    let depth = config.color_depth.resolve(|k| std::env::var(k).ok());
    let _depth = style::use_depth(depth);
    // without colour a skin is only its glyphs: draw the plain layout
    let paint = match depth {
        Depth::None => None,
        _ => Paint::of(config.skin),
    };
    let mut rows: Vec<String> = Vec::new();
    for src in config.lines() {
        let budget = config.max_rows.saturating_sub(rows.len());
        if budget == 0 {
            break;
        }
        let groups = src.map(|g| render_group(g, &ctx, &ex, sample));
        if groups.iter().all(Vec::is_empty) {
            continue;
        }
        rows.extend(line_rows(config, paint, groups, columns, budget));
    }
    let out = rows.join("\n");
    let out = if out.contains(ITEM_BREAK) {
        out.replace(ITEM_BREAK, "")
    } else {
        out
    };
    if depth == Depth::None {
        style::strip_colors(&out)
    } else {
        out
    }
}

/// One status line as at most `budget` rows: placed as usual, or, with
/// `set wrap on` and a line that does not fit one row, its segments
/// flowed left to right onto rows, left-aligned. An empty line has none.
/// `paint` is the skin, `None` for the plain layout.
fn line_rows(
    config: &Config,
    paint: Option<Paint>,
    groups: [Vec<Rendered>; 3],
    columns: Option<usize>,
    budget: usize,
) -> Vec<String> {
    let [l, c, r] = segs(config, groups);
    let group = |segs: &[Seg], side| draw(config, paint, segs, side);
    let (gl, gc, gr) = (
        group(&l, Side::Left),
        group(&c, Side::Center),
        group(&r, Side::Right),
    );
    let joiner = match paint {
        Some(_) => " ".to_string(),
        None => plain_joiner(),
    };
    let placed = place_groups(&gl, &gc, &gr, columns, config.gap, &joiner);
    match columns {
        Some(cols) if config.wrap == Wrap::On && !placed.fits => {
            let all: Vec<Seg> = [l, c, r].into_iter().flatten().collect();
            // a segment with plugin items may also break between them
            let units: Vec<(usize, String)> = all
                .iter()
                .enumerate()
                .flat_map(|(i, s)| item_pieces(&s.text).into_iter().map(move |p| (i, p)))
                .collect();
            let rebuilt = |rg: std::ops::Range<usize>| -> Vec<Seg> {
                let mut segs: Vec<Seg> = Vec::new();
                let mut last = usize::MAX;
                for (i, piece) in units[rg].iter().map(|(i, p)| (*i, p)) {
                    match segs.last_mut().filter(|_| last == i) {
                        Some(s) => s.text = format!("{} {piece}", s.text),
                        None => segs.push(Seg {
                            text: piece.clone(),
                            width: 0,
                            bg: all[i].bg,
                        }),
                    }
                    last = i;
                }
                for s in &mut segs {
                    s.width = visible_width_with(&s.text, config.icon_width);
                }
                segs
            };
            flow(units.len(), cols, budget, |rg| {
                group(&rebuilt(rg), Side::Left).width
            })
            .into_iter()
            .map(|rg| group(&rebuilt(rg), Side::Left).text)
            .collect()
        }
        _ if placed.text.is_empty() => Vec::new(),
        _ => placed
            .text
            .split('\n')
            .take(budget)
            .map(str::to_string)
            .collect(),
    }
}

/// A group's segments painted by the skin, or joined by ` │ `.
fn draw(config: &Config, paint: Option<Paint>, segs: &[Seg], side: Side) -> Group {
    match paint {
        Some(p) => skin::paint(p, config.skin_glyphs, segs, side),
        None => {
            let text = join(&segs.iter().map(|s| s.text.clone()).collect::<Vec<_>>());
            Group {
                width: visible_width_with(&text, config.icon_width),
                text,
            }
        }
    }
}

/// Segments with their widths and backgrounds. Backgrounds cycle through
/// the palette across all groups of the line, left to right; a segment's
/// `bg` option takes its place. Plain lines ignore them. `set gradient`
/// recolours the segment text as one run across the line.
fn segs(config: &Config, groups: [Vec<Rendered>; 3]) -> [Vec<Seg>; 3] {
    let bgs: &[Color] = if config.bgs.is_empty() {
        &palette::default().bgs
    } else {
        &config.bgs
    };
    let mut n = 0;
    let mut out = groups.map(|g| {
        g.into_iter()
            .map(|s| {
                let bg = s.bg.unwrap_or(bgs[n % bgs.len()]);
                n += 1;
                Seg {
                    width: visible_width_with(&s.text, config.icon_width),
                    text: s.text,
                    bg,
                }
            })
            .collect::<Vec<Seg>>()
    });
    if let Some(g) = look::line_gradient(config) {
        let iw = config.icon_width;
        let extent = Extent::of(out.iter().flatten().map(|s| s.text.as_str()), iw);
        let mut start = 0;
        for s in out.iter_mut().flatten() {
            let span = gradient::span(&s.text, iw);
            let ink = Ink::Ramp {
                gradient: &g,
                start,
                extent,
                icon_width: iw,
            };
            s.text = gradient::recolor(&s.text, Some(&ink), false);
            start += span;
        }
    }
    out
}

/// One `bar subagent` row: `layout` as a single left group, painted like
/// the status line, its trailing segments dropped while it is wider than
/// `columns`; a first segment still too wide is clipped with `…`. No
/// plugins; one row.
pub fn render_row(
    session: &Session,
    config: &Config,
    layout: &str,
    columns: Option<usize>,
    now: i64,
    runner: &dyn CommandRunner,
    cache: Option<&Cache>,
) -> String {
    let src = Sources {
        runner,
        cache,
        plugin_dirs: &[],
        sample: None,
    };
    row_impl(session, config, layout, columns, now, &src)
}

/// As [`render_row`] with every placeholder answered by `sample`.
pub fn render_row_sample(
    session: &Session,
    config: &Config,
    layout: &str,
    columns: Option<usize>,
    now: i64,
    sample: Sample,
) -> String {
    let src = Sources {
        runner: &ColdRunner,
        cache: None,
        plugin_dirs: &[],
        sample: Some(sample),
    };
    row_impl(session, config, layout, columns, now, &src)
}

fn row_impl(
    session: &Session,
    config: &Config,
    layout: &str,
    columns: Option<usize>,
    now: i64,
    from: &Sources,
) -> String {
    let ex = Expander::new(from.runner);
    let ctx = Ctx {
        session,
        config,
        now,
        runner: from.runner,
        cache: from.cache,
        columns,
        plugin_dirs: from.plugin_dirs,
        memo: Default::default(),
    };
    let _muted = style::use_muted(config.muted);
    let _ansi = style::use_ansi(config.palette.ansi);
    let rendered = render_group(layout, &ctx, &ex, from.sample);
    let groups = [rendered, Vec::new(), Vec::new()];
    let [mut l, _, _] = segs(config, groups);
    let paint = Paint::of(config.skin);
    let mut row = draw(config, paint, &l, Side::Left);
    while columns.is_some_and(|c| row.width > c) && l.len() > 1 {
        l.pop();
        row = draw(config, paint, &l, Side::Left);
    }
    let text = match columns {
        Some(c) if row.width > c => style::clip(&row.text, c, config.icon_width),
        _ => row.text,
    };
    text.replace(['\n', '\r'], " ")
}

/// What `bar render` prints: the line(s), or the short config error.
pub fn run(
    session: &Session,
    load: LoadResult,
    columns: Option<usize>,
    runner: &dyn CommandRunner,
    cache: Option<&Cache>,
) -> String {
    match load {
        LoadResult::Invalid(_) => CONFIG_ERROR.to_string(),
        LoadResult::Missing => render_with(session, &builtin_config(), columns, runner, cache),
        LoadResult::Ok(c) => render_with(session, &c, columns, runner, cache),
    }
}

#[cfg(test)]
mod pieces_tests {
    use super::*;

    const B: char = ITEM_BREAK;

    #[test]
    fn colour_and_bold_are_closed_and_reopened_at_each_cut() {
        let t = format!("\x1b[1m\x1b[38;5;1maaaa{B} bbbb{B} cccc\x1b[39m\x1b[22m");
        let p = item_pieces(&t);
        assert_eq!(
            p,
            [
                "\x1b[1m\x1b[38;5;1maaaa\x1b[0m",
                "\x1b[1m\x1b[38;5;1mbbbb\x1b[0m",
                "\x1b[1m\x1b[38;5;1mcccc\x1b[39m\x1b[22m",
            ]
        );
    }

    #[test]
    fn plain_and_gradient_text_is_unchanged() {
        assert_eq!(item_pieces(&format!("a{B} b")), ["a", "b"]);
        // a gradient recolours each character: the last colour carries
        let t = format!("\x1b[38;5;1ma\x1b[38;5;2mb{B} \x1b[38;5;3mc\x1b[39m");
        let p = item_pieces(&t);
        assert_eq!(p[0], "\x1b[38;5;1ma\x1b[38;5;2mb\x1b[0m");
        assert!(p[1].starts_with("\x1b[38;5;2m\x1b[38;5;3mc"), "{:?}", p[1]);
    }
}
