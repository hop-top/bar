//! Wires config, placeholders and layout into the status line text.

use crate::cache::Cache;
use crate::conf::{Config, LoadResult};
use crate::expand::{CommandRunner, Expander};
use crate::format::{parse, segments, Token};
use crate::layout::compose;
use crate::options::Options;
use crate::placeholders::{lookup, Ctx};
use crate::plugin;
use crate::session::Session;
use crate::style::dim;
use std::path::PathBuf;

/// Cold cache: every `%command%` expands to nothing.
pub struct ColdRunner;

impl CommandRunner for ColdRunner {
    fn run(&self, _cmd: &str) -> Option<String> {
        None
    }
}

pub const CONFIG_ERROR: &str = "bar: config error, run bar check";

/// Layout used when no config file exists.
pub fn builtin_config() -> Config {
    Config {
        left: "#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=3,author=@me]} │ #{issues[limit=3,assignee=@me]}".into(),
        center: "#{usage[type=5h]} │ #{usage[type=7d]}".into(),
        right: "#{account}".into(),
        ..Config::default()
    }
}

/// Core names first; anything else is a `bar-<name>` plugin.
fn render_placeholder(name: &str, inline: &Options, ctx: &Ctx, ex: &Expander) -> String {
    let core = lookup(name);
    let found = match core {
        Some(_) => None,
        None => match plugin::find(ctx.plugin_dirs, name) {
            Some(f) => Some(f),
            None => return String::new(),
        },
    };
    let merged = match ctx.config.defaults.get(name) {
        Some(d) => inline.merged_over(d),
        None => inline.clone(),
    };
    let mut opts = Options::default();
    for (k, v) in merged.iter() {
        opts.set(k, &ex.expand(v));
    }
    match (core, found) {
        (Some(p), _) => p.render(ctx, &opts),
        (None, Some(f)) => plugin::render_found(ctx, &f, &opts),
        (None, None) => None,
    }
    .unwrap_or_default()
}

/// Text tokens survive only beside non-empty placeholders, so `repo:` loses
/// its colon when the branch is empty.
fn render_segment(seg: &[Token], ctx: &Ctx, ex: &Expander) -> Option<String> {
    let values: Vec<Option<String>> = seg
        .iter()
        .map(|t| match t {
            Token::Placeholder { name, options } => {
                Some(render_placeholder(name, options, ctx, ex))
            }
            Token::Text(_) => None,
        })
        .collect();
    let has_placeholder = values.iter().any(Option::is_some);
    if has_placeholder && values.iter().flatten().all(|v| v.is_empty()) {
        return None;
    }
    let empty = |i: Option<usize>| {
        i.and_then(|i| values.get(i))
            .is_some_and(|v| v.as_deref() == Some(""))
    };
    let mut out = String::new();
    for (i, t) in seg.iter().enumerate() {
        match (t, &values[i]) {
            (Token::Text(s), _) => {
                if !empty(i.checked_sub(1)) && !empty(Some(i + 1)) {
                    out.push_str(&dim(s));
                }
            }
            (_, Some(v)) => out.push_str(v),
            _ => {}
        }
    }
    Some(out)
}

fn render_group(src: &str, ctx: &Ctx, ex: &Expander) -> Vec<String> {
    let Ok(tokens) = parse(src) else {
        return Vec::new();
    };
    segments(&tokens, &ctx.config.separator)
        .iter()
        .filter_map(|seg| render_segment(seg, ctx, ex))
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
    let ex = Expander::new(runner);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let ctx = Ctx {
        session,
        config,
        now,
        runner,
        cache,
        columns,
        plugin_dirs,
    };
    compose(
        &render_group(&config.left, &ctx, &ex),
        &render_group(&config.center, &ctx, &ex),
        &render_group(&config.right, &ctx, &ex),
        columns,
        config.gap,
    )
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
