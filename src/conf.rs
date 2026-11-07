//! Config parser: line-oriented, tmux-style directives.

use crate::options::Options;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfError {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrap {
    Auto,
    On,
    Off,
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
    pub separator: String,
    pub gap: usize,
    pub wrap: Wrap,
    /// Default ttl floor for placeholders, seconds.
    pub refresh: u64,
    pub colors: Colors,
    pub ramp: [u8; 11],
    pub defaults: BTreeMap<String, Options>,
    pub accounts: Vec<AccountRule>,
    pub rules: Vec<Rule>,
    pub plugins: BTreeMap<String, Options>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            left: String::new(),
            center: String::new(),
            right: String::new(),
            separator: "│".into(),
            gap: 3,
            wrap: Wrap::Auto,
            refresh: 5,
            colors: Colors {
                good: 114,
                warn: 179,
                bad: 203,
            },
            ramp: [196, 202, 208, 214, 220, 226, 190, 154, 118, 82, 46],
            defaults: BTreeMap::new(),
            accounts: Vec::new(),
            rules: Vec::new(),
            plugins: BTreeMap::new(),
        }
    }
}

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

fn apply_set(c: &mut Config, key: &str, val: &str) -> Result<(), String> {
    match key {
        "left" => c.left = val.into(),
        "center" => c.center = val.into(),
        "right" => c.right = val.into(),
        "separator" => c.separator = val.into(),
        "gap" => c.gap = val.parse().map_err(|_| format!("invalid gap `{val}`"))?,
        "wrap" => {
            c.wrap = match val {
                "auto" => Wrap::Auto,
                "on" => Wrap::On,
                "off" => Wrap::Off,
                _ => return Err(format!("invalid wrap `{val}` (auto|on|off)")),
            }
        }
        "refresh" => c.refresh = duration(val)?,
        "color.good" => c.colors.good = code(val)?,
        "color.warn" => c.colors.warn = code(val)?,
        "color.bad" => c.colors.bad = code(val)?,
        "ramp" => {
            let codes: Result<Vec<u8>, _> = val.split_whitespace().map(code).collect();
            c.ramp = codes?
                .try_into()
                .map_err(|_| "ramp needs exactly 11 codes".to_string())?;
        }
        _ => return Err(format!("unknown set key `{key}`")),
    }
    Ok(())
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

pub fn parse(src: &str) -> Result<Config, Vec<ConfError>> {
    let mut c = Config::default();
    let mut errs = Vec::new();
    for (i, raw) in src.lines().enumerate() {
        let r = tokenize(raw).and_then(|t| {
            if t.is_empty() {
                Ok(())
            } else {
                apply_line(&mut c, &t)
            }
        });
        if let Err(message) = r {
            errs.push(ConfError {
                line: i + 1,
                message,
            });
        }
    }
    if errs.is_empty() {
        Ok(c)
    } else {
        Err(errs)
    }
}

/// Candidate paths, first existing wins.
pub fn search_paths(
    env: &dyn Fn(&str) -> Option<String>,
    home: &Path,
    kit_dir: &Path,
) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(x) = env("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
        v.push(PathBuf::from(x).join("bar/bar.conf"));
    }
    v.push(home.join(".config/bar/bar.conf"));
    v.push(kit_dir.join("bar.conf"));
    v
}

/// First existing candidate, or `None`.
pub fn find_path(
    env: &dyn Fn(&str) -> Option<String>,
    home: &Path,
    kit_dir: &Path,
) -> Option<PathBuf> {
    search_paths(env, home, kit_dir)
        .into_iter()
        .find(|p| p.is_file())
}

/// Seam for tests: explicit env lookup, home, and kit config dir.
pub fn load_from(env: &dyn Fn(&str) -> Option<String>, home: &Path, kit_dir: &Path) -> LoadResult {
    let Some(path) = find_path(env, home, kit_dir) else {
        return LoadResult::Missing;
    };
    match std::fs::read_to_string(&path) {
        Err(e) => LoadResult::Invalid(vec![ConfError {
            line: 0,
            message: format!("{}: {e}", path.display()),
        }]),
        Ok(s) => match parse(&s) {
            Ok(c) => LoadResult::Ok(c),
            Err(e) => LoadResult::Invalid(e),
        },
    }
}

/// Process environment, `HOME` and the kit config dir for `bar`.
pub fn find_path_default() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    find_path(
        &|k| std::env::var(k).ok(),
        &home,
        &hop_top_kit::xdg::raw_config_dir("bar"),
    )
}

pub fn load() -> LoadResult {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    load_from(
        &|k| std::env::var(k).ok(),
        &home,
        &hop_top_kit::xdg::raw_config_dir("bar"),
    )
}
