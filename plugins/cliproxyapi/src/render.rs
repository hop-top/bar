//! Turning a collection into render output, and the plugin's declaration
//! (`--ext-info`). One item per account, each with its windows and the
//! time they reset, plus the pool's `usage` and the preferred `account`.

use crate::collect::{Snapshot, Status, STALE_AFTER};
use crate::model::{glob, Acct, Win};
use crate::pool::{self, Pool};
use bar_plugin::proto::{ExtInfo, Inputs, OptSpec, Output, RenderInput};
use serde_json::Value;

pub const NAME: &str = "cliproxyapi";
/// Cells a label may take.
const LABEL_MAX: usize = 12;
/// Percent left at or under which an account is drawn `bad`.
const BAD_LEFT: f64 = 10.0;

/// The `--ext-info` document.
pub fn ext_info() -> ExtInfo {
    let mut o = std::collections::BTreeMap::new();
    o.insert(
        "provider".into(),
        OptSpec::choice(&["auto", "claude", "codex"], "auto"),
    );
    let text = |default: &str| OptSpec {
        default: Some(default.into()),
        ..OptSpec::string()
    };
    o.insert("windows".into(), text("5h,7d,wk"));
    o.insert("show".into(), text("label,pct,reset"));
    o.insert("count".into(), OptSpec::choice(&["left", "used"], "left"));
    o.insert(
        "label".into(),
        OptSpec::choice(&["alias", "index", "none"], "alias"),
    );
    o.insert(
        "sort".into(),
        OptSpec::choice(&["name", "used", "reset"], "used"),
    );
    o.insert("accounts".into(), OptSpec::string());
    o.insert("compact".into(), OptSpec::bool());
    o.insert(
        "stale".into(),
        OptSpec::choice(&["mark", "dim", "hide"], "mark"),
    );
    o.insert("link".into(), OptSpec::string());
    o.insert(
        "pool".into(),
        OptSpec::choice(&["mean", "active", "max"], "mean"),
    );
    o.insert(
        "route-provider".into(),
        OptSpec::choice(&["claude", "codex", "none"], "claude"),
    );
    o.insert("base-url".into(), OptSpec::string());
    ExtInfo {
        name: NAME.into(),
        version: crate::VERSION.into(),
        description: "Per-account quota of a CLIProxyAPI proxy".into(),
        capabilities: vec!["discover".into(), "placeholder".into(), "check".into()],
        options: o,
        ttl: "60s".into(),
        icon: None,
        cache: None,
        inputs: Some(Inputs {
            env: vec!["ANTHROPIC_BASE_URL".into()],
            session: vec!["model_id".into(), "model".into()],
            accounts: true,
        }),
        timeout: Some("15s".into()),
        actions: crate::switch::actions(),
        aliases: vec!["cpa".into()],
        web: Some(serde_json::json!({
            "category": "usage",
            "samples": [
                { "output": { "label": "alice", "text": "5h 59% \u{21BB}2h10m 7d 88% \u{21BB}3d4h", "score": 59 } },
                { "when": { "count": "used" },
                  "output": { "label": "alice", "text": "5h 41% \u{21BB}2h10m 7d 12% \u{21BB}3d4h", "score": 59 } },
                { "when": { "compact": "true" },
                  "output": { "label": "alice", "text": "59%\u{B7}2h10m 88%\u{B7}3d4h", "score": 59 } },
                { "when": { "label": "index" },
                  "output": { "label": "#1", "text": "5h 59% \u{21BB}2h10m 7d 88% \u{21BB}3d4h", "score": 59 } },
                { "when": { "show": "pct" },
                  "output": { "text": "5h 59% 7d 88%", "score": 59 } },
                { "when": { "provider": "codex" },
                  "output": { "label": "bob", "text": "7d 6% \u{21BB}1d3h", "style": "bad", "score": 6 } }
            ]
        })),
    }
}

/// `<1m`, `41m`, `2h10m`, `3d4h`: a number of seconds.
pub fn countdown(secs: i64) -> String {
    let (big, small, bu, su) = match secs {
        s if s < 60 => return "<1m".into(),
        s if s < 3600 => return format!("{}m", s / 60),
        s if s < 86_400 => (secs / 3600, secs % 3600 / 60, 'h', 'm'),
        s => (s / 86_400, s % 86_400 / 3600, 'd', 'h'),
    };
    if small == 0 {
        format!("{big}{bu}")
    } else {
        format!("{big}{bu}{small}{su}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelMode {
    Alias,
    Index,
    Nothing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Name,
    Used,
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stale {
    Mark,
    Dim,
    Hide,
}

/// The placeholder's options.
#[derive(Debug, Clone)]
pub struct Opts {
    pub windows: Vec<String>,
    pub label_part: bool,
    pub pct: bool,
    pub reset: bool,
    pub plan: bool,
    pub used: bool,
    pub label: LabelMode,
    pub sort: Sort,
    pub accounts: Vec<String>,
    pub compact: bool,
    pub stale: Stale,
    pub link: Option<String>,
    pub pool: Pool,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            windows: vec!["5h".into(), "7d".into(), "wk".into()],
            label_part: true,
            pct: true,
            reset: true,
            plan: false,
            used: false,
            label: LabelMode::Alias,
            sort: Sort::Used,
            accounts: vec![],
            compact: false,
            stale: Stale::Mark,
            link: None,
            pool: Pool::Mean,
        }
    }
}

/// A list option: a comma list as a string, or a JSON array of strings.
fn list(v: Option<&Value>) -> Option<Vec<String>> {
    let parts: Vec<String> = match v? {
        Value::String(s) => s.split(',').map(|p| p.trim().to_string()).collect(),
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
        _ => return None,
    };
    Some(parts.into_iter().filter(|p| !p.is_empty()).collect())
}

fn flag(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => matches!(s.as_str(), "true" | "yes" | "on" | "1" | ""),
        _ => false,
    }
}

impl Opts {
    pub fn from_input(i: &RenderInput) -> Opts {
        let mut o = Opts::default();
        let get = |k: &str| i.options.get(k);
        if let Some(w) = list(get("windows")).filter(|w| !w.is_empty()) {
            o.windows = w;
        }
        if let Some(s) = list(get("show")).filter(|s| !s.is_empty()) {
            let has = |p: &str| s.iter().any(|x| x == p);
            (o.label_part, o.pct, o.reset, o.plan) =
                (has("label"), has("pct"), has("reset"), has("plan"));
        }
        o.used = i.opt_str("count") == Some("used");
        o.label = match i.opt_str("label") {
            Some("index") => LabelMode::Index,
            Some("none") => LabelMode::Nothing,
            _ => LabelMode::Alias,
        };
        o.sort = match i.opt_str("sort") {
            Some("name") => Sort::Name,
            Some("reset") => Sort::Reset,
            _ => Sort::Used,
        };
        o.accounts = list(get("accounts")).unwrap_or_default();
        o.compact = flag(get("compact"));
        o.stale = match i.opt_str("stale") {
            Some("dim") => Stale::Dim,
            Some("hide") => Stale::Hide,
            _ => Stale::Mark,
        };
        o.link = i
            .opt_str("link")
            .filter(|l| !l.is_empty())
            .map(String::from);
        o.pool = Pool::parse(i.opt_str("pool").unwrap_or(""));
        o
    }
}

fn cut(s: &str) -> String {
    s.chars().take(LABEL_MAX).collect()
}

/// The label of the account at 1-based position `idx`, as `mode` draws it.
fn label_of(a: &Acct, idx: usize, mode: LabelMode) -> Option<String> {
    match mode {
        LabelMode::Alias => Some(cut(&a.label)),
        LabelMode::Index => Some(format!("#{idx}")),
        LabelMode::Nothing => None,
    }
}

/// Does `a`, at position `idx`, match one entry of the `accounts` option:
/// a position (`2` or `#2`), a label (globs), or the index.
fn wanted(a: &Acct, idx: usize, entry: &str) -> bool {
    let n = entry.strip_prefix('#').unwrap_or(entry);
    n.parse::<usize>().ok() == Some(idx) || glob(entry, &a.label) || entry == a.key
}

fn percent(w: &Win, used: bool) -> String {
    let p = if used { w.used } else { 100.0 - w.used };
    format!("{:.0}%", p.clamp(0.0, 100.0).max(0.0))
}

/// URL-escaped, so a label or index cannot add to the link's structure.
fn escape(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            b => format!("%{b:02X}"),
        })
        .collect()
}

fn expand(tpl: &str, a: &Acct, label: &str, instance: &str) -> String {
    tpl.replace("{account}", &escape(&a.key))
        .replace("{label}", &escape(label))
        .replace("{provider}", &escape(&a.provider))
        .replace("{instance}", &escape(instance))
}

/// The accounts of `provider` the proxy can use, with their position.
fn enabled<'a>(s: &'a Snapshot, provider: &str) -> Vec<(usize, &'a Acct)> {
    s.accounts
        .iter()
        .filter(|a| a.provider == provider && !a.disabled)
        .enumerate()
        .map(|(i, a)| (i + 1, a))
        .collect()
}

/// The windows an account shows, in the order asked for; `None` when the
/// account is not shown at all (hidden stale, or none of its windows asked
/// for).
fn shown_windows<'a>(a: &'a Acct, o: &Opts, now: i64) -> Option<Vec<&'a Win>> {
    let stale = a.at > 0 && now - a.at > STALE_AFTER;
    if stale && o.stale == Stale::Hide {
        return None;
    }
    let wins: Vec<&Win> = o
        .windows
        .iter()
        .filter_map(|id| a.windows.iter().find(|w| &w.id == id))
        .collect();
    (!wins.is_empty() || a.windows.is_empty()).then_some(wins)
}

/// Whether the window id can be left out of every shown account's text:
/// each shows exactly one window, and it is the same one for all. Any mix
/// keeps every prefix, so a short line never leaves the reader guessing
/// which window a number belongs to.
pub fn single_window<'a>(sets: impl IntoIterator<Item = &'a [&'a Win]>) -> bool {
    let mut id: Option<&str> = None;
    let mut any = false;
    for w in sets {
        any = true;
        match w {
            [one] if id.is_none_or(|i| i == one.id) => id = Some(&one.id),
            _ => return false,
        }
    }
    any
}

fn item(a: &Acct, idx: usize, o: &Opts, instance: &str, now: i64, bare: bool) -> Option<Output> {
    let stale = a.at > 0 && now - a.at > STALE_AFTER;
    let wins = shown_windows(a, o, now)?;
    let mut parts: Vec<String> = wins
        .iter()
        .map(|w| {
            let cd = (o.reset && w.resets_at > now).then(|| countdown(w.resets_at - now));
            let pct = o.pct.then(|| percent(w, o.used));
            match (o.compact, pct, cd) {
                (true, Some(p), Some(c)) => format!("{p}\u{B7}{c}"),
                (true, p, c) => p.or(c).unwrap_or_default(),
                (false, p, c) => {
                    let mut t = if bare { String::new() } else { w.id.clone() };
                    for part in [p, c.map(|c| format!("\u{21BB}{c}"))].into_iter().flatten() {
                        if !t.is_empty() {
                            t.push(' ');
                        }
                        t += &part;
                    }
                    t
                }
            }
        })
        .filter(|p| !p.is_empty())
        .collect();
    if a.windows.is_empty() {
        parts.push("?".into());
    }
    let mut text = parts.join(" ");
    if o.plan {
        if let Some(p) = &a.plan {
            text = format!("{p} {text}");
        }
    }
    if stale && o.stale == Stale::Mark {
        text = format!("~{text}");
    }
    let label = o.label_part.then(|| label_of(a, idx, o.label)).flatten();
    let mut out = Output::text(&text);
    // a user `account` line that matched colours the label
    if label.is_some() && o.label == LabelMode::Alias {
        out.label_color = a.color.clone();
    }
    out.label = label.clone();
    let left = wins.iter().map(|w| 100.0 - w.used).fold(f64::NAN, f64::min);
    if left.is_finite() {
        out.score = Some(left.clamp(0.0, 100.0));
    }
    out.style = if a.upstream_auth || left <= BAD_LEFT {
        Some("bad".into())
    } else if stale && o.stale == Stale::Dim {
        Some("dim".into())
    } else {
        None
    };
    if let Some(t) = &o.link {
        let shown = label_of(a, idx, o.label).unwrap_or_else(|| a.label.clone());
        out.link = Some(expand(t, a, &shown, instance));
    }
    out.tokens.insert("account".into(), a.key.clone());
    out.tokens.insert("provider".into(), a.provider.clone());
    Some(out)
}

/// One item per account, sorted as asked.
pub fn items(s: &Snapshot, provider: &str, o: &Opts, instance: &str, now: i64) -> Vec<Output> {
    let mut rows: Vec<(usize, &Acct)> = enabled(s, provider)
        .into_iter()
        .filter(|(i, a)| o.accounts.is_empty() || o.accounts.iter().any(|e| wanted(a, *i, e)))
        .collect();
    let most_used = |a: &Acct| a.windows.iter().map(|w| w.used).fold(-1.0, f64::max);
    let soonest = |a: &Acct| {
        a.windows
            .iter()
            .filter(|w| w.resets_at > now)
            .map(|w| w.resets_at)
            .min()
            .unwrap_or(i64::MAX)
    };
    match o.sort {
        Sort::Name => rows.sort_by_key(|(_, a)| a.label.to_ascii_lowercase()),
        Sort::Used => rows.sort_by(|(_, a), (_, b)| most_used(b).total_cmp(&most_used(a))),
        Sort::Reset => rows.sort_by_key(|(_, a)| soonest(a)),
    }
    let bare = single_window(
        rows.iter()
            .filter_map(|(_, a)| shown_windows(a, o, now))
            .collect::<Vec<_>>()
            .iter()
            .map(Vec::as_slice),
    );
    rows.into_iter()
        .filter_map(|(i, a)| item(a, i, o, instance, now, bare))
        .collect()
}

/// The output for a session that does not use a proxy: nothing drawn, but
/// with exactly one configured instance (that reads Claude) the tokens its
/// `pick` and `next` actions take, so `#{account}` can link them. No
/// `account`: `#{account}` keeps showing the signed-in account.
pub fn plain(cfg: &crate::config::Config) -> Output {
    let mut out = Output::hidden();
    if let [inst] = cfg.instances.as_slice() {
        if inst.providers.iter().any(|p| p == "claude") {
            out.tokens.insert("provider".into(), "claude".into());
            out.tokens.insert(
                "instance".into(),
                crate::switch::instance_id(&inst.origin.to_string()),
            );
        }
    }
    out.finish()
}

/// The render output: one item per account, the pool's `usage` and, when
/// the proxy's preference is certain, `account`. Nothing is drawn when the
/// proxy is off or no account has data, but `usage` and `account` still
/// go out whatever the display options filter.
pub fn output(s: &Snapshot, provider: &str, o: &Opts, instance: &str, now: i64) -> Output {
    match s.status {
        Status::AuthStopped(_) | Status::NoKey => return Output::styled("key?", "bad"),
        Status::Off => return Output::hidden(),
        Status::Ok | Status::Backoff => {}
    }
    let rows = enabled(s, provider);
    let accts: Vec<&Acct> = rows.iter().map(|(_, a)| *a).collect();
    let mut out = if rows.iter().any(|(_, a)| !a.windows.is_empty()) {
        Output {
            items: items(s, provider, o, instance, now),
            ..Output::default()
        }
    } else {
        Output::hidden()
    };
    out.usage = pool::usage(&accts, o.pool, now);
    out.account = pool::account(&accts, |a| {
        let idx = rows
            .iter()
            .find(|(_, r)| r.key == a.key)
            .map_or(0, |(i, _)| *i);
        match o.label {
            LabelMode::Index => format!("#{idx}"),
            _ => cut(&a.label),
        }
    });
    // the colour of a user `account` line that matched: bar draws the
    // label as is
    if let (Some(acc), LabelMode::Alias) = (out.account.as_mut(), o.label) {
        acc.color = accts
            .iter()
            .find(|a| a.key == acc.id)
            .and_then(|a| a.color.clone());
    }
    // what the `next` actions take, so a link can be drawn for `#{account}`
    // even when the proxy has no single preferred account
    out.tokens.insert("provider".into(), provider.into());
    out.tokens
        .insert("instance".into(), crate::switch::instance_id(instance));
    if out.items.is_empty() {
        out.text = Some(String::new());
    }
    out.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1000;

    fn win(id: &str, used: f64, reset: i64) -> Win {
        Win {
            id: id.into(),
            used,
            resets_at: reset,
            period_s: 0,
        }
    }

    fn acct(key: &str, label: &str, wins: Vec<Win>, at: i64) -> Acct {
        Acct {
            key: key.into(),
            provider: "claude".into(),
            label: label.into(),
            color: None,
            priority: 0,
            plan: Some("max".into()),
            windows: wins,
            at,
            disabled: false,
            unavailable: false,
            upstream_auth: false,
            retry_at: 0,
            fails: 0,
        }
    }

    fn snap(accounts: Vec<Acct>) -> Snapshot {
        Snapshot {
            status: Status::Ok,
            accounts,
        }
    }

    fn two() -> Snapshot {
        snap(vec![
            acct("k1", "alice", vec![win("7d", 12.0, NOW + 90_000)], NOW),
            acct(
                "k2",
                "bob",
                vec![win("5h", 95.0, NOW + 7800), win("7d", 40.0, NOW + 400_000)],
                NOW,
            ),
        ])
    }

    fn run(s: &Snapshot, o: &Opts) -> Output {
        output(s, "claude", o, "http://127.0.0.1:1", NOW)
    }

    fn texts(o: &Output) -> Vec<(String, String)> {
        o.items
            .iter()
            .map(|i| {
                (
                    i.label.clone().unwrap_or_default(),
                    i.text.clone().unwrap_or_default(),
                )
            })
            .collect()
    }

    #[test]
    fn countdowns() {
        assert_eq!(countdown(30), "<1m");
        assert_eq!(countdown(150), "2m");
        assert_eq!(countdown(7200), "2h");
        assert_eq!(countdown(7800), "2h10m");
        assert_eq!(countdown(200_000), "2d7h");
        assert_eq!(countdown(3 * 86_400), "3d");
    }

    #[test]
    fn one_item_per_account_most_used_first_showing_what_is_left() {
        let o = run(&two(), &Opts::default());
        assert_eq!(
            texts(&o),
            vec![
                (
                    "bob".to_string(),
                    "5h 5% \u{21BB}2h10m 7d 60% \u{21BB}4d15h".to_string()
                ),
                ("alice".to_string(), "7d 88% \u{21BB}1d1h".to_string()),
            ]
        );
        assert_eq!(o.text, None, "items replace the text");
        let bob = &o.items[0];
        assert_eq!(bob.style.as_deref(), Some("bad"), "5h has 5% left");
        assert_eq!(bob.score, Some(5.0));
        assert_eq!(bob.tokens["account"], "k2");
        assert_eq!(bob.tokens["provider"], "claude");
        assert_eq!(o.items[1].style, None);
        assert_eq!(o.items[1].score, Some(88.0));
    }

    #[test]
    fn count_used_and_compact_and_show() {
        let o = Opts {
            used: true,
            compact: true,
            ..Opts::default()
        };
        assert_eq!(
            texts(&run(&two(), &o))[0].1,
            "95%\u{B7}2h10m 40%\u{B7}4d15h"
        );
        let o = Opts {
            label_part: false,
            reset: false,
            plan: true,
            ..Opts::default()
        };
        let r = run(&two(), &o);
        assert_eq!(texts(&r)[1], ("".to_string(), "max 7d 88%".to_string()));
    }

    #[test]
    fn windows_order_and_missing_are_honoured() {
        let o = Opts {
            windows: vec!["7d".into(), "5h".into()],
            ..Opts::default()
        };
        assert_eq!(
            texts(&run(&two(), &o))[0].1,
            "7d 60% \u{21BB}4d15h 5h 5% \u{21BB}2h10m"
        );
        // only 5h: alice has none of it, so she is left out, not shown as 0
        let o = Opts {
            windows: vec!["5h".into()],
            ..Opts::default()
        };
        let r = run(&two(), &o);
        assert_eq!(r.items.len(), 1);
        assert_eq!(texts(&r)[0].0, "bob");
    }

    #[test]
    fn label_modes() {
        let local = |m| Opts {
            label: m,
            sort: Sort::Name,
            ..Opts::default()
        };
        let l = |m| {
            texts(&run(&two(), &local(m)))
                .into_iter()
                .map(|t| t.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(l(LabelMode::Alias), ["alice", "bob"]);
        assert_eq!(l(LabelMode::Index), ["#1", "#2"]);
        assert_eq!(l(LabelMode::Nothing), ["", ""]);
        let long = snap(vec![acct(
            "k",
            "averyveryverylongname",
            vec![win("7d", 1.0, NOW + 5)],
            NOW,
        )]);
        assert_eq!(
            texts(&run(&long, &Opts::default()))[0].0.chars().count(),
            LABEL_MAX
        );
    }

    #[test]
    fn sorting() {
        let by = |s| {
            texts(&run(
                &two(),
                &Opts {
                    sort: s,
                    ..Opts::default()
                },
            ))
            .into_iter()
            .map(|t| t.0)
            .collect::<Vec<_>>()
        };
        assert_eq!(by(Sort::Name), ["alice", "bob"]);
        assert_eq!(by(Sort::Used), ["bob", "alice"]);
        assert_eq!(by(Sort::Reset), ["bob", "alice"], "bob's 5h resets first");
    }

    #[test]
    fn the_accounts_option_picks_by_label_or_position() {
        let pick = |e: &str| {
            texts(&run(
                &two(),
                &Opts {
                    accounts: vec![e.into()],
                    ..Opts::default()
                },
            ))
            .into_iter()
            .map(|t| t.0)
            .collect::<Vec<_>>()
        };
        assert_eq!(pick("alice"), ["alice"]);
        assert_eq!(pick("b*"), ["bob"]);
        assert_eq!(pick("2"), ["bob"]);
        assert_eq!(pick("#1"), ["alice"]);
        assert!(pick("nobody").is_empty());
        // a filter that shows nothing still reports the pool
        let r = run(
            &two(),
            &Opts {
                accounts: vec!["nobody".into()],
                ..Opts::default()
            },
        );
        assert!(r.usage.is_some() && r.items.is_empty());
    }

    #[test]
    fn stale_data_is_marked_dimmed_or_hidden() {
        let s = snap(vec![acct(
            "k",
            "old",
            vec![win("7d", 10.0, NOW + 90_000)],
            NOW - STALE_AFTER - 1,
        )]);
        let o = run(&s, &Opts::default());
        assert!(o.items[0].text.as_deref().unwrap().starts_with("~90%"));
        assert_eq!(o.items[0].style, None);
        let o = run(
            &s,
            &Opts {
                stale: Stale::Dim,
                ..Opts::default()
            },
        );
        assert!(o.items[0].text.as_deref().unwrap().starts_with("90%"));
        assert_eq!(o.items[0].style.as_deref(), Some("dim"));
        assert!(run(
            &s,
            &Opts {
                stale: Stale::Hide,
                ..Opts::default()
            }
        )
        .items
        .is_empty());
    }

    #[test]
    fn an_account_never_read_shows_a_question_mark_and_no_data_hides() {
        let s = snap(vec![
            acct("a", "idle", vec![], 0),
            acct("b", "busy", vec![win("7d", 1.0, NOW + 5000)], NOW),
        ]);
        let o = run(
            &s,
            &Opts {
                sort: Sort::Name,
                ..Opts::default()
            },
        );
        assert_eq!(texts(&o)[1], ("idle".to_string(), "?".to_string()));
        assert_eq!(o.items[1].score, None);
        // nothing known for anyone: nothing drawn
        let s = snap(vec![acct("a", "idle", vec![], 0)]);
        let o = run(&s, &Opts::default());
        assert!(o.items.is_empty());
        assert_eq!(o.text.as_deref(), Some(""));
        // another provider's accounts do not count
        assert!(output(&two(), "codex", &Opts::default(), "", NOW)
            .items
            .is_empty());
    }

    #[test]
    fn failures_render_as_the_contract_says() {
        let mut s = snap(vec![]);
        s.status = Status::AuthStopped(401);
        let o = run(&s, &Opts::default());
        assert_eq!(
            (o.text.as_deref(), o.style.as_deref()),
            (Some("key?"), Some("bad"))
        );
        let mut s = two();
        s.status = Status::Off;
        assert_eq!(run(&s, &Opts::default()).text.as_deref(), Some(""));
        let mut s = two();
        s.accounts[0].upstream_auth = true;
        let o = run(&s, &Opts::default());
        assert!(o
            .items
            .iter()
            .any(|i| i.label.as_deref() == Some("alice") && i.style.as_deref() == Some("bad")));
    }

    #[test]
    fn disabled_accounts_are_left_out() {
        let mut s = two();
        s.accounts[0].disabled = true;
        let o = run(&s, &Opts::default());
        assert_eq!(o.items.len(), 1);
        assert_eq!(o.items[0].tokens["account"], "k2");
    }

    #[test]
    fn link_template_is_escaped() {
        let s = snap(vec![acct(
            "k 1",
            "a&b",
            vec![win("7d", 1.0, NOW + 5000)],
            NOW,
        )]);
        let o = run(
            &s,
            &Opts {
                link: Some("x://use?a={account}&l={label}&p={provider}&i={instance}".into()),
                ..Opts::default()
            },
        );
        assert_eq!(
            o.items[0].link.as_deref(),
            Some("x://use?a=k%201&l=a%26b&p=claude&i=http%3A%2F%2F127.0.0.1%3A1")
        );
    }

    #[test]
    fn usage_and_account_ride_on_the_output() {
        let o = run(&two(), &Opts::default());
        let u = o.usage.unwrap();
        assert_eq!(u.seven_day.unwrap().used_percentage, 26.0);
        assert!(u.five_hour.is_none(), "alice reports no 5h");
        // two accounts, same priority: not certain which one is preferred
        assert!(o.account.is_none());
        let mut s = two();
        s.accounts[1].priority = 4;
        let o = run(
            &s,
            &Opts {
                label: LabelMode::Index,
                ..Opts::default()
            },
        );
        let a = o.account.unwrap();
        assert_eq!((a.id.as_str(), a.label.as_str()), ("k2", "#2"));
        let o = run(&s, &Opts::default());
        assert_eq!(o.account.unwrap().label, "bob");
    }

    #[test]
    fn options_parse_from_the_render_input() {
        let i: RenderInput = serde_json::from_str(
            r#"{"options":{"windows":"7d,5h","show":"label,pct","count":"used","label":"index",
                "sort":"name","accounts":"a,2","compact":true,"stale":"hide","link":"x://{account}","pool":"max"}}"#,
        )
        .unwrap();
        let o = Opts::from_input(&i);
        assert_eq!(o.windows, ["7d", "5h"]);
        assert!(o.label_part && o.pct && !o.reset && !o.plan);
        assert!(o.used && o.compact);
        assert_eq!(
            (o.label, o.sort, o.stale, o.pool),
            (LabelMode::Index, Sort::Name, Stale::Hide, Pool::Max)
        );
        assert_eq!(o.accounts, ["a", "2"]);
        assert_eq!(o.link.as_deref(), Some("x://{account}"));
        let i: RenderInput = serde_json::from_str(r#"{"options":{"compact":"true"}}"#).unwrap();
        assert!(Opts::from_input(&i).compact);
        let d = Opts::from_input(&RenderInput::default());
        assert_eq!(d.windows, ["5h", "7d", "wk"]);
        assert!(!d.used && d.reset && d.pct && d.label_part);
    }

    fn plain_texts(s: &Snapshot, o: &Opts) -> Vec<String> {
        run(s, o)
            .items
            .iter()
            .map(|i| i.text.clone().unwrap_or_default())
            .collect()
    }

    #[test]
    fn the_window_prefix_goes_only_when_every_account_shows_the_same_single_window() {
        // all 7d only: no prefix
        let s = snap(vec![
            acct(
                "k1",
                "innoprofits",
                vec![win("7d", 34.0, NOW + 5 * 86_400)],
                NOW,
            ),
            acct(
                "k2",
                "personal",
                vec![win("7d", 0.0, NOW + 5 * 86_400 + 16 * 3600)],
                NOW,
            ),
        ]);
        assert_eq!(
            plain_texts(&s, &Opts::default()),
            vec!["66% \u{21BB}5d", "100% \u{21BB}5d16h"]
        );
        // one account with two windows: every prefix stays
        let s = snap(vec![
            acct(
                "k1",
                "work",
                vec![
                    win("5h", 60.0, NOW + 7200),
                    win("7d", 34.0, NOW + 5 * 86_400),
                ],
                NOW,
            ),
            acct(
                "k2",
                "innoprofits",
                vec![win("7d", 34.0, NOW + 5 * 86_400)],
                NOW,
            ),
        ]);
        assert_eq!(
            plain_texts(&s, &Opts::default()),
            vec!["5h 40% \u{21BB}2h 7d 66% \u{21BB}5d", "7d 66% \u{21BB}5d"]
        );
        // single windows of different kinds: prefixes stay
        let s = snap(vec![
            acct("k1", "a", vec![win("5h", 60.0, NOW + 7200)], NOW),
            acct("k2", "b", vec![win("7d", 34.0, NOW + 5 * 86_400)], NOW),
        ]);
        assert_eq!(
            plain_texts(&s, &Opts::default()),
            vec!["5h 40% \u{21BB}2h", "7d 66% \u{21BB}5d"]
        );
        // one account: its only window needs no prefix, two keep theirs
        let one = snap(vec![acct(
            "k1",
            "a",
            vec![win("5h", 60.0, NOW + 7200)],
            NOW,
        )]);
        assert_eq!(plain_texts(&one, &Opts::default()), vec!["40% \u{21BB}2h"]);
        let two = snap(vec![acct(
            "k1",
            "a",
            vec![
                win("5h", 60.0, NOW + 7200),
                win("7d", 34.0, NOW + 5 * 86_400),
            ],
            NOW,
        )]);
        assert!(plain_texts(&two, &Opts::default())[0].starts_with("5h 40%"));
    }

    #[test]
    fn only_the_accounts_shown_count_for_the_prefix() {
        let s = snap(vec![
            acct(
                "k1",
                "work",
                vec![
                    win("5h", 60.0, NOW + 7200),
                    win("7d", 34.0, NOW + 5 * 86_400),
                ],
                NOW,
            ),
            acct("k2", "other", vec![win("7d", 34.0, NOW + 5 * 86_400)], NOW),
        ]);
        // the windows option leaves one window each: bare
        let o = Opts {
            windows: vec!["7d".into()],
            ..Opts::default()
        };
        assert_eq!(
            plain_texts(&s, &o),
            vec!["66% \u{21BB}5d", "66% \u{21BB}5d"]
        );
        // the accounts option leaves one account with two windows: kept
        let o = Opts {
            accounts: vec!["work".into()],
            ..Opts::default()
        };
        assert!(plain_texts(&s, &o)[0].starts_with("5h 40%"));
        // a hidden stale account does not count
        let s = snap(vec![
            acct(
                "k1",
                "work",
                vec![
                    win("5h", 60.0, NOW + 7200),
                    win("7d", 34.0, NOW + 5 * 86_400),
                ],
                NOW - STALE_AFTER - 1,
            ),
            acct("k2", "other", vec![win("7d", 34.0, NOW + 5 * 86_400)], NOW),
        ]);
        let o = Opts {
            stale: Stale::Hide,
            ..Opts::default()
        };
        assert_eq!(plain_texts(&s, &o), vec!["66% \u{21BB}5d"]);
        // an account without data (`?`) is not one single window
        let s = snap(vec![
            acct("k1", "a", vec![win("7d", 34.0, NOW + 5 * 86_400)], NOW),
            acct("k2", "b", vec![], NOW),
        ]);
        assert_eq!(
            plain_texts(&s, &Opts::default()),
            vec!["7d 66% \u{21BB}5d", "?"]
        );
    }

    #[test]
    fn a_matched_account_line_colours_the_label_and_the_account() {
        let mut a = acct("k1", "work", vec![win("7d", 10.0, NOW + 90_000)], NOW);
        a.color = Some("33".into());
        let b = acct("k2", "plain", vec![win("7d", 50.0, NOW + 90_000)], NOW);
        let s = snap(vec![a, b]);
        let o = run(&s, &Opts::default());
        let by = |l: &str| {
            o.items
                .iter()
                .find(|i| i.label.as_deref() == Some(l))
                .unwrap()
        };
        assert_eq!(by("work").label_color.as_deref(), Some("33"));
        assert_eq!(by("plain").label_color, None);
        assert!(o.account.is_none(), "a tie: no preferred account");
        // the preferred account carries its colour
        let mut top = acct("k1", "work", vec![win("7d", 10.0, NOW + 90_000)], NOW);
        top.color = Some("33".into());
        top.priority = 5;
        let s = snap(vec![top, acct("k2", "plain", vec![], NOW)]);
        let o = run(&s, &Opts::default());
        let acc = o.account.unwrap();
        assert_eq!(
            (acc.label.as_str(), acc.color.as_deref()),
            ("work", Some("33"))
        );
        // an index label is not the account's name: no colour
        let mut top = acct("k1", "work", vec![win("7d", 10.0, NOW + 90_000)], NOW);
        top.color = Some("33".into());
        let s = snap(vec![top]);
        let o = run(
            &s,
            &Opts {
                label: LabelMode::Index,
                ..Opts::default()
            },
        );
        assert_eq!(o.items[0].label_color, None);
        assert_eq!(o.account.unwrap().color, None);
    }

    #[test]
    fn a_session_without_a_proxy_offers_the_tokens_of_a_lone_instance() {
        use crate::config;
        let conf = |text: &str| config::parse(text, None);
        let one = conf("instance http://127.0.0.1:8317\n key-file /k\n");
        let o = plain(&one);
        assert_eq!(o.text.as_deref(), Some(""));
        assert!(o.account.is_none() && o.items.is_empty());
        assert_eq!(o.tokens["provider"], "claude");
        assert_eq!(
            o.tokens["instance"],
            crate::switch::instance_id(&one.instances[0].origin.to_string())
        );
        let two = conf(
            "instance http://127.0.0.1:8317\n key-file /k\ninstance http://127.0.0.1:8318\n key-file /k\n",
        );
        assert!(plain(&two).tokens.is_empty(), "which one would it be?");
        assert!(plain(&conf("")).tokens.is_empty());
        let codex = conf("instance http://127.0.0.1:8317\n key-file /k\n providers codex\n");
        assert!(plain(&codex).tokens.is_empty(), "no Claude to pick for");
    }
}
