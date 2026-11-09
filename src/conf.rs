//! Config parser: line-oriented, tmux-style directives.

use crate::compact::CompactAt;
use crate::options::Options;
use crate::palette;
use crate::prices::Price;
use std::collections::BTreeMap;

mod load;
pub mod project;
mod set;
pub use load::{find_path, find_path_default, load, load_from, load_layered, search_paths};
pub use project::ProjectMode;
use set::{apply_palette, apply_price, apply_set};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfError {
    pub line: usize,
    pub message: String,
}

/// What a line too wide for `COLUMNS` does: `Off` keeps the two-row
/// layout, `On` flows its segments onto more rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrap {
    Off,
    On,
}

/// Most status lines a config can set, and most rows a render prints.
pub const MAX_LINES: usize = 4;

/// The three groups of one status line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    pub left: String,
    pub center: String,
    pub right: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icons {
    None,
    Nerdfont,
    Emoji,
}

/// How segments are drawn: joined by ` │ `, or on coloured backgrounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skin {
    Plain,
    Powerline,
    Capsule,
}

/// Separator glyphs of a skin: Nerd Font / Powerline, or plain ASCII.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkinGlyphs {
    Nerd,
    Ascii,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Amount {
    Percent(u32),
    Cents(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Early {
    Thresholds { warn: Amount, bad: Amount },
    Pace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub target: String,
    pub early: Early,
    pub final_secs: u64,
    pub final_bad: Amount,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRule {
    pub glob: String,
    pub label: String,
    pub color: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colors {
    pub good: u8,
    pub warn: u8,
    pub bad: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub left: String,
    pub center: String,
    pub right: String,
    /// Lines 2 and up (`set left@2` ...); `left`/`center`/`right` are line 1.
    pub more_lines: [Line; MAX_LINES - 1],
    /// `set subagent`: layout of a `bar subagent` row; `None` uses
    /// [`crate::subagent::DEFAULT_LAYOUT`].
    pub subagent: Option<String>,
    pub separator: String,
    pub gap: usize,
    pub wrap: Wrap,
    /// Default ttl floor for placeholders, seconds.
    pub refresh: u64,
    pub icons: Icons,
    /// Cells a private-use (Nerd Font) glyph occupies: 1 or 2.
    pub icon_width: usize,
    pub colors: Colors,
    pub ramp: [u8; 11],
    /// Muted text colour, from the palette.
    pub muted: u8,
    pub skin: Skin,
    pub skin_glyphs: SkinGlyphs,
    pub palette: &'static str,
    /// Segment backgrounds skins cycle through, from the palette.
    pub bgs: &'static [u8],
    pub defaults: BTreeMap<String, Options>,
    pub accounts: Vec<AccountRule>,
    pub rules: Vec<Rule>,
    pub plugins: BTreeMap<String, Options>,
    /// Claude Code's data dir (`set claude-dir`); `None` follows the
    /// environment.
    pub claude_dir: Option<String>,
    /// `set price` overrides, in file order.
    pub prices: Vec<Price>,
    /// `set compact-at`: where auto-compaction runs, overriding what
    /// bar reads from Claude Code's environment and settings.
    pub compact_at: Option<CompactAt>,
    /// `set update-check on`: `#{update}` checks GitHub daily, in the background.
    pub update_check: bool,
    /// What a project `.bar.conf` may do (`set project-config`).
    pub project_config: ProjectMode,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            left: String::new(),
            center: String::new(),
            right: String::new(),
            more_lines: Default::default(),
            subagent: None,
            separator: "│".into(),
            gap: 3,
            wrap: Wrap::Off,
            refresh: 5,
            icons: Icons::None,
            icon_width: 1,
            colors: Colors {
                good: 114,
                warn: 179,
                bad: 203,
            },
            ramp: [196, 202, 208, 214, 220, 226, 190, 154, 118, 82, 46],
            muted: palette::default().muted,
            skin: Skin::Plain,
            skin_glyphs: SkinGlyphs::Nerd,
            palette: palette::default().name,
            bgs: palette::default().bgs,
            defaults: BTreeMap::new(),
            accounts: Vec::new(),
            rules: Vec::new(),
            plugins: BTreeMap::new(),
            claude_dir: None,
            prices: Vec::new(),
            compact_at: None,
            update_check: false,
            project_config: ProjectMode::default(),
        }
    }
}

impl Config {
    /// Left, centre and right sources of every line, line 1 first.
    pub fn lines(&self) -> [[&str; 3]; MAX_LINES] {
        let mut out = [[""; 3]; MAX_LINES];
        out[0] = [&self.left, &self.center, &self.right];
        for (o, l) in out[1..].iter_mut().zip(&self.more_lines) {
            *o = [&l.left, &l.center, &l.right];
        }
        out
    }
}

/// `left`, `left@N`, `center@N`, `right@N`: the group and its 1-based
/// line. `None` for any other key; `Err` for a bad or out-of-range `N`.
pub fn layout_key(key: &str) -> Option<Result<(&str, usize), String>> {
    let (group, n) = key.split_once('@').unwrap_or((key, "1"));
    if !matches!(group, "left" | "center" | "right") {
        return None;
    }
    Some(match n.parse::<usize>() {
        Ok(n) if (1..=MAX_LINES).contains(&n) => Ok((group, n)),
        _ => Err(format!("invalid line `{n}` in `{key}` (1-{MAX_LINES})")),
    })
}

/// Loaded once per run, so the size gap between variants is harmless.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum LoadResult {
    Missing,
    Ok(Config),
    Invalid(Vec<ConfError>),
}

/// Whitespace-split; double quotes group; `#` outside quotes starts a comment.
pub(crate) fn tokenize(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut in_tok, mut in_q) = (false, false);
    for ch in line.chars() {
        match ch {
            '"' => {
                in_q = !in_q;
                in_tok = true;
            }
            '#' if !in_q => break,
            c if c.is_whitespace() && !in_q => {
                if in_tok {
                    out.push(std::mem::take(&mut cur));
                    in_tok = false;
                }
            }
            c => {
                cur.push(c);
                in_tok = true;
            }
        }
    }
    if in_q {
        return Err("unterminated quote".into());
    }
    if in_tok {
        out.push(cur);
    }
    Ok(out)
}

pub fn duration(s: &str) -> Result<u64, String> {
    let bad = || format!("invalid duration `{s}`");
    let unit = s.chars().last().ok_or_else(bad)?;
    let mult = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86400,
        _ => return Err(bad()),
    };
    let n: u64 = s[..s.len() - 1].parse().map_err(|_| bad())?;
    n.checked_mul(mult).ok_or_else(bad)
}

fn amount(s: &str) -> Result<Amount, String> {
    let bad = || format!("invalid amount `{s}`");
    if let Some(p) = s.strip_suffix('%') {
        return Ok(Amount::Percent(p.parse().map_err(|_| bad())?));
    }
    let d = s.strip_prefix('$').ok_or_else(bad)?;
    let (whole, frac) = d.split_once('.').unwrap_or((d, ""));
    if frac.len() > 2 {
        return Err(bad());
    }
    let whole: u64 = whole.parse().map_err(|_| bad())?;
    let frac_c: u64 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<2}").parse().map_err(|_| bad())?
    };
    whole
        .checked_mul(100)
        .and_then(|w| w.checked_add(frac_c))
        .map(Amount::Cents)
        .ok_or_else(bad)
}

fn code(s: &str) -> Result<u8, String> {
    s.parse()
        .map_err(|_| format!("invalid colour `{s}` (0-255)"))
}

fn kv<'a>(tok: &'a str, key: &str) -> Option<&'a str> {
    tok.strip_prefix(key)?.strip_prefix('=')
}

pub(crate) fn parse_options(toks: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    for t in toks {
        let (k, v) = t
            .split_once('=')
            .filter(|(k, _)| !k.is_empty())
            .ok_or_else(|| format!("expected key=value, got `{t}`"))?;
        o.set(k, v);
    }
    Ok(o)
}

fn parse_rule(t: &[String]) -> Result<Rule, String> {
    let usage = "usage: rule <target> early (warn=X bad=Y | pace) final=<dur> bad=<amount>";
    if t.len() < 2 || t[1] != "early" {
        return Err(usage.into());
    }
    let (early, rest) = if t.get(2).map(String::as_str) == Some("pace") {
        (Early::Pace, &t[3..])
    } else {
        let warn = t.get(2).and_then(|s| kv(s, "warn")).ok_or(usage)?;
        let bad = t.get(3).and_then(|s| kv(s, "bad")).ok_or(usage)?;
        let early = Early::Thresholds {
            warn: amount(warn)?,
            bad: amount(bad)?,
        };
        (early, &t[4..])
    };
    let fin = rest.first().and_then(|s| kv(s, "final")).ok_or(usage)?;
    let bad = rest.get(1).and_then(|s| kv(s, "bad")).ok_or(usage)?;
    if rest.len() != 2 {
        return Err(usage.into());
    }
    Ok(Rule {
        target: t[0].clone(),
        early,
        final_secs: duration(fin)?,
        final_bad: amount(bad)?,
    })
}

fn apply_line(c: &mut Config, t: &[String]) -> Result<(), String> {
    let need = |n: usize, usage: &str| {
        if t.len() < n {
            Err(format!("usage: {usage}"))
        } else {
            Ok(())
        }
    };
    match t[0].as_str() {
        "set" => {
            need(3, "set <key> <value>")?;
            if t[1] == "price" {
                return apply_price(c, &t[2..]);
            }
            if t.len() > 3 {
                return Err("set takes one value; quote it".into());
            }
            apply_set(c, &t[1], &t[2])
        }
        "default" => {
            need(2, "default <placeholder> k=v ...")?;
            let o = parse_options(&t[2..])?;
            let slot = c.defaults.entry(t[1].clone()).or_default();
            *slot = o.merged_over(slot);
            Ok(())
        }
        "account" => {
            need(4, "account \"<glob>\" <label> <color>")?;
            if t.len() > 4 {
                return Err("usage: account \"<glob>\" <label> <color>".into());
            }
            c.accounts.push(AccountRule {
                glob: t[1].clone(),
                label: t[2].clone(),
                color: code(&t[3])?,
            });
            Ok(())
        }
        "rule" => {
            c.rules.push(parse_rule(&t[1..])?);
            Ok(())
        }
        "plugin" => {
            need(2, "plugin <name> k=v ...")?;
            c.plugins.insert(t[1].clone(), parse_options(&t[2..])?);
            Ok(())
        }
        other => Err(format!("unknown directive `{other}`")),
    }
}

/// Which file a config line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// The user's `bar.conf` (or the built-in layout when there is none).
    User,
    /// A project `.bar.conf`, applied after the user config.
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerError {
    pub layer: Layer,
    pub error: ConfError,
}

/// Applies every line of `text`; `guard` may refuse a line before it is
/// applied. Errors carry 1-based line numbers.
fn apply_text(
    c: &mut Config,
    text: &str,
    explicit: &mut Vec<String>,
    guard: &dyn Fn(&[String]) -> Result<(), String>,
) -> Vec<ConfError> {
    let mut errs = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let r = tokenize(raw).and_then(|t| {
            if t.is_empty() {
                return Ok(());
            }
            guard(&t)?;
            apply_line(c, &t)?;
            if t[0] == "set" {
                explicit.push(t[1].clone());
            }
            Ok(())
        });
        if let Err(message) = r {
            errs.push(ConfError {
                line: i + 1,
                message,
            });
        }
    }
    errs
}

pub fn parse(src: &str) -> Result<Config, Vec<ConfError>> {
    parse_layers(Some(src), |_| None).map_err(|e| e.into_iter().map(|e| e.error).collect())
}

/// [`parse_layers`] with what it left out of the project config.
#[derive(Debug)]
pub struct Layered {
    /// The config, or the user config's errors.
    pub config: Result<Config, Vec<LayerError>>,
    /// Project config lines left out, each with why: invalid, or refused
    /// by `project-config`. Line 0: the file could not be read.
    pub dropped: Vec<ConfError>,
}

/// The user config (`None`: the built-in layout) with the project config
/// over it; see [`layers`]. Dropped project lines are not errors.
pub fn parse_layers(
    user: Option<&str>,
    project: impl FnOnce(ProjectMode) -> Option<Result<String, String>>,
) -> Result<Config, Vec<LayerError>> {
    layers(user, project).config
}

/// The user config (`None`: the built-in layout) with the project config
/// over it. `project` is asked for the project file's text, or why it
/// could not be read, once the user config's `project-config` is known;
/// it is not called under `off`. Project lines apply after the user's: a
/// `set` wins, a `default` merges over, a `plugin` line replaces, and a
/// `rule` or `account` replaces the user's one with the same target or glob.
/// A project line that is invalid or that the mode refuses is dropped
/// and the rest applies, so a repository cannot break the status line;
/// an unreadable project file is dropped whole.
pub fn layers(
    user: Option<&str>,
    project: impl FnOnce(ProjectMode) -> Option<Result<String, String>>,
) -> Layered {
    let mut c = match user {
        Some(_) => Config::default(),
        None => crate::render::builtin_config(),
    };
    let mut explicit = Vec::new();
    let tag = |layer| move |error| LayerError { layer, error };
    let mut dropped = Vec::new();
    let errs: Vec<LayerError> = match user {
        Some(text) => apply_text(&mut c, text, &mut explicit, &|_| Ok(()))
            .into_iter()
            .map(tag(Layer::User))
            .collect(),
        None => Vec::new(),
    };
    let mode = c.project_config;
    let text = match mode {
        ProjectMode::Off => None,
        _ => project(mode),
    };
    match text {
        None => {}
        Some(Err(message)) => dropped.push(ConfError { line: 0, message }),
        Some(Ok(text)) => {
            let used = project::user_plugins(&c);
            let (rules, accounts) = (c.rules.len(), c.accounts.len());
            let guard = |t: &[String]| project::guard(mode, t, &used);
            dropped = apply_text(&mut c, &text, &mut explicit, &guard);
            project::replace_overridden(&mut c, rules, accounts);
            // the project file decides `project-config` only through the user's
            c.project_config = mode;
        }
    }
    apply_palette(&mut c, &explicit);
    let config = if errs.is_empty() { Ok(c) } else { Err(errs) };
    Layered { config, dropped }
}
