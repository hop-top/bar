//! Wires config, placeholders and layout into the status line text.

use crate::cache::Cache;
use crate::conf::{Config, LoadResult, Wrap, MAX_LINES};
use crate::expand::{CommandRunner, Expander};
use crate::format::{parse, segments, Token};
use crate::icons;
use crate::layout::{flow, join, place_groups, plain_joiner, Group};
use crate::options::Options;
use crate::palette;
use crate::placeholders::{lookup, Ctx};
use crate::plugin;
use crate::session::Session;
use crate::skin::{self, Paint, Seg, Side};
use crate::style::{self, dim, visible_width_with};
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
pub const BUILTIN_CONF: &str = "set left \"#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=3,author=@me]} │ #{issues[limit=3,assignee=@me]}\"
set center \"#{usage[type=5h]} │ #{usage[type=7d]}\"
set right \"#{account}\"
";

/// Layout used when no config file exists.
pub fn builtin_config() -> Config {
    Config {
        left: "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=3,author=@me]} │ #{issues[limit=3,assignee=@me]}".into(),
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
/// value and the `bg` option.
fn render_placeholder(
    name: &str,
    inline: &Options,
    ctx: &Ctx,
    ex: &Expander,
    sample: Option<Sample>,
) -> (String, Option<u8>) {
    let (bg, opts) = skin::split_bg(&expanded(name, inline, ctx, ex));
    (render_value(name, &opts, ctx, sample), bg)
}

fn render_value(name: &str, opts: &Options, ctx: &Ctx, sample: Option<Sample>) -> String {
    let core = lookup(name);
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
        (Some(p), _) => with_icon(p.render_labeled(ctx, &opts, label)),
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
    bg: Option<u8>,
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
    let dirs = plugin::path_dirs(std::env::var_os("PATH").as_deref());
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
    };
    let _muted = style::use_muted(config.muted);
    let _ansi = style::use_ansi(palette::by_name(config.palette).is_some_and(|p| p.ansi));
    let mut rows: Vec<String> = Vec::new();
    for src in config.lines() {
        let budget = MAX_LINES - rows.len();
        if budget == 0 {
            break;
        }
        let groups = src.map(|g| render_group(g, &ctx, &ex, sample));
        if groups.iter().all(Vec::is_empty) {
            continue;
        }
        rows.extend(line_rows(config, groups, columns, budget));
    }
    rows.join("\n")
}

/// One status line as at most `budget` rows: placed as usual, or, with
/// `set wrap on` and a line that does not fit one row, its segments
/// flowed left to right onto rows, left-aligned. An empty line has none.
fn line_rows(
    config: &Config,
    groups: [Vec<Rendered>; 3],
    columns: Option<usize>,
    budget: usize,
) -> Vec<String> {
    let paint = Paint::of(config.skin);
    let [l, c, r] = segs(config, groups);
    let group = |segs: &[Seg], side| match paint {
        Some(p) => skin::paint(p, config.skin_glyphs, segs, side),
        None => {
            let text = join(&segs.iter().map(|s| s.text.clone()).collect::<Vec<_>>());
            Group {
                width: visible_width_with(&text, config.icon_width),
                text,
            }
        }
    };
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
            flow(all.len(), cols, budget, |rg| {
                group(&all[rg], Side::Left).width
            })
            .into_iter()
            .map(|rg| group(&all[rg], Side::Left).text)
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

/// Segments with their widths and backgrounds. Backgrounds cycle through
/// the palette across all groups of the line, left to right; a segment's
/// `bg` option takes its place. Plain lines ignore them.
fn segs(config: &Config, groups: [Vec<Rendered>; 3]) -> [Vec<Seg>; 3] {
    let bgs = if config.bgs.is_empty() {
        palette::default().bgs
    } else {
        config.bgs
    };
    let mut n = 0;
    groups.map(|g| {
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
            .collect()
    })
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
