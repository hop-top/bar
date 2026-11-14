//! The `pick-label` option: a template for each entry of the account
//! chooser (the `pick` action). `{name}` is a placeholder; `{{` is a
//! literal `{` and `}}` a literal `}`. Read left to right, at each `{` the
//! first of `{{`, `{name}` (1 to 32 of `a-z`, `0-9`, `_`) and a lone `{`
//! that fits wins, and at each `}` the first of `}}` and a lone `}`: so
//! `{{{used}}}` is `{42}`. A lone `{` or `}` stays as it is; an unknown
//! name stays as written (`{nope}`), so a typo shows in the list.
//!
//! The render puts a template other than [`DEFAULT`] in its output as
//! [`FIELD`]; the action gets that output from bar with the click.

use crate::chooser::{sanitize, MAX_ITEM};
use crate::model::Win;
use crate::rank::Ranked;

/// The option's name, and its default.
pub const OPTION: &str = "pick-label";
pub const DEFAULT: &str = "{label} - {used}%";
/// The output field that carries a template other than the default.
pub const FIELD: &str = "pick_label";
/// Longest template read, in characters.
pub const MAX_TEMPLATE: usize = 256;
const MAX_NAME: usize = 32;

/// `tpl` with every `{name}` that `value` knows replaced, `{{` made `{`
/// and `}}` made `}`.
pub fn expand(tpl: &str, value: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = tpl;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let (c, after) = (&rest[i..=i], &rest[i + 1..]);
        if let Some(r) = after.strip_prefix(c) {
            out.push_str(c);
            rest = r;
            continue;
        }
        rest = after;
        if c == "}" {
            out.push('}');
            continue;
        }
        let n = after
            .bytes()
            .take_while(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
            .count();
        if !(1..=MAX_NAME).contains(&n) || !after[n..].starts_with('}') {
            out.push('{');
            continue;
        }
        let name = &after[..n];
        match value(name) {
            Some(v) => out.push_str(&v),
            None => out.push_str(&format!("{{{name}}}")),
        }
        rest = &after[n + 1..];
    }
    out.push_str(rest);
    out
}

/// Entries the chooser can tell apart and hand back: when one is empty,
/// reads `false` (both mean Cancel to the chooser) or equals another,
/// every entry gets its position first, `<n>. `.
pub fn distinct(labels: Vec<String>) -> Vec<String> {
    let clash = labels.iter().enumerate().any(|(i, l)| {
        l.is_empty() || l == "false" || labels[..i].iter().any(|o| o == l)
    });
    if !clash {
        return labels;
    }
    labels
        .into_iter()
        .enumerate()
        .map(|(i, l)| sanitize(&format!("{}. {l}", i + 1), MAX_ITEM))
        .collect()
}

/// The template a render's options ask for, when it is not the default.
pub fn from_options(o: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    o.get(OPTION)
        .and_then(serde_json::Value::as_str)
        .map(|s| s.chars().take(MAX_TEMPLATE).collect::<String>())
        .filter(|s| s != DEFAULT)
}

/// The template an action's stdin carries (the render's output), else the
/// default.
pub fn from_action(v: &serde_json::Value) -> String {
    v["output"][FIELD]
        .as_str()
        .map_or_else(|| DEFAULT.into(), |s| s.chars().take(MAX_TEMPLATE).collect())
}

/// Percent used and left, and the reset time, of window `w` at `now`; a
/// window past its reset is unused.
fn win_parts(w: &Win, now: i64) -> (f64, f64, Option<i64>) {
    if w.resets_at > 0 && w.resets_at <= now {
        return (0.0, 100.0, None);
    }
    let used = w.used.clamp(0.0, 100.0);
    (used, 100.0 - used, (w.resets_at > now).then_some(w.resets_at))
}

/// How one account is shown: its position (from 1), whether the proxy
/// prefers it, whether every row shows the same single window.
struct Row<'a> {
    r: &'a Ranked<'a>,
    n: usize,
    current: bool,
    bare: bool,
}

/// The value of placeholder `name` for `row` at `now`.
fn account_value(name: &str, row: &Row, now: i64) -> Option<String> {
    let (r, n, current) = (row.r, row.n, row.current);
    let e = r.entry;
    let win = |ids: &[&str]| {
        ids.iter()
            .find_map(|id| e.windows.iter().find(|w| w.id == *id))
            .map(|w| win_parts(w, now))
    };
    let (five, week) = (win(&["5h"]), win(&["7d", "wk"]));
    let pct = |p: Option<f64>| p.map_or_else(|| "?".into(), |p| format!("{p:.0}"));
    let reset = |w: Option<(f64, f64, Option<i64>)>| {
        w.and_then(|w| w.2)
            .map_or_else(String::new, |t| crate::render::countdown(t - now))
    };
    Some(match name {
        "n" => n.to_string(),
        "label" => sanitize(&e.label, 40),
        "provider" => e.provider.clone(),
        "used" => pct(five.map(|w| w.0)),
        "left" => pct(five.map(|w| w.1)),
        "reset" => reset(five),
        "used7d" => pct(week.map(|w| w.0)),
        "left7d" => pct(week.map(|w| w.1)),
        "reset7d" => reset(week),
        "windows" => crate::switch::window_text(r, now, row.bare),
        "status" => if e.unavailable { "unavailable" } else { "active" }.into(),
        "current" => if current { "current" } else { "" }.into(),
        _ => return None,
    })
}

/// The chooser's entries for `ranked`, in order: `tpl` per account,
/// sanitised, made [`distinct`]. `preferred` is the proxy's preferred
/// account's `auth_index`.
pub fn entries(tpl: &str, ranked: &[Ranked], preferred: Option<&str>, now: i64) -> Vec<String> {
    let bare = crate::switch::bare_ids(ranked);
    let labels = ranked
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let row = Row {
                r,
                n: i + 1,
                current: preferred == Some(r.entry.index.as_str()),
                bare,
            };
            sanitize(&expand(tpl, |k| account_value(k, &row, now)), MAX_ITEM)
        })
        .collect();
    distinct(labels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(k: &str) -> Option<String> {
        match k {
            "label" => Some("work".into()),
            "used" => Some("42".into()),
            "x" => Some(String::new()),
            _ => None,
        }
    }

    #[test]
    fn the_default_renders_the_label_and_the_percent() {
        assert_eq!(expand(DEFAULT, vars), "work - 42%");
    }

    #[test]
    fn doubled_braces_are_literal_and_lone_ones_stay() {
        assert_eq!(expand("{{used}}", vars), "{used}");
        assert_eq!(expand("{{{used}}}", vars), "{42}");
        assert_eq!(expand("{used}}", vars), "42}");
        assert_eq!(expand("a { b } c", vars), "a { b } c");
        assert_eq!(expand("{", vars), "{");
        assert_eq!(expand("}", vars), "}");
        assert_eq!(expand("{label", vars), "{label");
        assert_eq!(expand("50% {x}|", vars), "50% |");
        assert_eq!(expand("%used%", vars), "%used%");
    }

    #[test]
    fn an_unknown_or_malformed_name_stays_as_written() {
        assert_eq!(expand("{nope} {label}", vars), "{nope} work");
        assert_eq!(expand("{Label}", vars), "{Label}");
        assert_eq!(expand("{la bel}", vars), "{la bel}");
        let long = format!("{{{}}}", "a".repeat(33));
        assert_eq!(expand(&long, vars), long);
        assert_eq!(expand("{ü}", vars), "{ü}");
    }

    #[test]
    fn duplicates_empty_and_false_number_every_entry() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(distinct(v(&["a", "b"])), v(&["a", "b"]));
        assert_eq!(distinct(v(&["a", "b", "a"])), v(&["1. a", "2. b", "3. a"]));
        assert_eq!(distinct(v(&["a", ""])), v(&["1. a", "2."]));
        assert_eq!(distinct(v(&["false", "b"])), v(&["1. false", "2. b"]));
        let long = "x".repeat(MAX_ITEM);
        let d = distinct(vec![long.clone(), long]);
        assert_ne!(d[0], d[1]);
        assert!(d.iter().all(|l| l.chars().count() <= MAX_ITEM));
    }

    #[test]
    fn options_carry_only_a_template_that_is_not_the_default() {
        let o = |v: serde_json::Value| v.as_object().unwrap().clone();
        assert_eq!(from_options(&o(serde_json::json!({}))), None);
        assert_eq!(
            from_options(&o(serde_json::json!({"pick-label": DEFAULT}))),
            None
        );
        assert_eq!(
            from_options(&o(serde_json::json!({"pick-label": "{n} {label}"}))).as_deref(),
            Some("{n} {label}")
        );
        let long = "y".repeat(MAX_TEMPLATE + 9);
        assert_eq!(
            from_options(&o(serde_json::json!({ "pick-label": long })))
                .unwrap()
                .len(),
            MAX_TEMPLATE
        );
        assert_eq!(from_action(&serde_json::json!({})), DEFAULT);
        assert_eq!(
            from_action(&serde_json::json!({"output": {"pick_label": "{n}"}})),
            "{n}"
        );
    }
}
