mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::budget::{standing, Kind};
use hop_bar::cache::Cache;
use hop_bar::conf::{self, Config};
use hop_bar::options::Options;
use hop_bar::placeholders::budget::{self, Budget};
use hop_bar::placeholders::cost::Cost;
use hop_bar::placeholders::spendhist::SpendHist;
use hop_bar::placeholders::{Ctx, Placeholder};
use hop_bar::render::ColdRunner;
use hop_bar::session::{Session, Window};
use hop_bar::spendhist::{cache_key, Spend, Tz};

// 2026-11-07T12:00:00Z
const NOW: i64 = 1_794_052_800;
const H: i64 = NOW / 3600;

fn opts(kv: &[(&str, &str)]) -> Options {
    let mut o = Options::default();
    for (k, v) in kv {
        o.set(k, v);
    }
    o
}

fn plain(s: &str) -> String {
    hop_bar::style::strip_codes(s)
}

fn cfg(src: &str) -> Config {
    conf::parse(src).unwrap()
}

fn spend(usd: f64) -> Spend {
    Spend {
        usd,
        unknown: false,
    }
}

#[test]
fn set_keys_parse_dollars_off_and_warn() {
    let c = cfg("set budget-session $10\nset budget-today 20\nset budget-month 12.50\nset budget-block $15\nset budget-warn 70%");
    let b = c.budgets;
    assert_eq!(b.get(Kind::Session), Some(1000));
    assert_eq!(b.get(Kind::Today), Some(2000));
    assert_eq!(b.get(Kind::Month), Some(1250));
    assert_eq!(b.get(Kind::Block), Some(1500));
    assert_eq!(b.warn, 70);
    let c = cfg("set budget-today $5\nset budget-today off");
    assert_eq!(c.budgets.get(Kind::Today), None);
    assert_eq!(c.budgets.warn, 80);
}

#[test]
fn set_keys_reject_bad_values() {
    for line in [
        "set budget-today 0",
        "set budget-today $0.00",
        "set budget-today -5",
        "set budget-today $1.234",
        "set budget-today ten",
        "set budget-week $5",
        "set budget-warn 80",
        "set budget-warn 0%",
        "set budget-warn 101%",
    ] {
        let e = conf::parse(line).unwrap_err();
        assert_eq!(e.len(), 1, "{line}");
    }
    let e = conf::parse("set budget-today ten").unwrap_err();
    assert!(e[0].message.contains("budget-today"), "{}", e[0].message);
}

#[test]
fn colour_ramps_then_warn_then_bad_then_over() {
    let c = Config {
        budgets: cfg("set budget-today $10").budgets,
        ..Config::default()
    };
    let (warn, bad) = (c.colors.warn, c.colors.bad);
    // nothing spent: the good end of the ramp
    assert_eq!(standing(0.0, 1000, &c).color, c.ramp[10]);
    // halfway to warn: three quarters up the ramp
    assert_eq!(standing(4.0, 1000, &c).color, c.ramp[8]);
    assert_eq!(standing(7.99, 1000, &c).color, c.ramp[5]);
    assert_eq!(standing(8.0, 1000, &c).color, warn);
    assert_eq!(standing(9.99, 1000, &c).color, warn);
    let at = standing(10.0, 1000, &c);
    assert_eq!((at.color, at.over), (bad, false));
    // whole cents decide
    assert!(!standing(10.004, 1000, &c).over);
    let over = standing(10.01, 1000, &c);
    assert_eq!((over.color, over.over), (bad, true));
    // a lower warn moves the switch
    let low = Config {
        budgets: cfg("set budget-warn 50%").budgets,
        ..Config::default()
    };
    assert_eq!(standing(5.0, 1000, &low).color, warn);
    assert_eq!(standing(4.9, 1000, &low).color, low.ramp[6]);
}

#[test]
fn components() {
    let c = Config::default();
    let f =
        |show: &str, usd: f64| plain(&budget::fmt(&c, &opts(&[("show", show)]), spend(usd), 2000));
    assert_eq!(f("ratio,bar", 5.0), "$5.00/$20 █░░░░");
    assert_eq!(f("spent,limit,pct", 5.0), "$5.00 $20 25%");
    assert_eq!(f("left", 5.0), "$15.00 left");
    assert_eq!(f("left", 23.5), "$3.50 over");
    assert_eq!(f("pct,gauge", 23.5), "118% █");
    assert_eq!(f("bar", 23.5), "█████");
    // unknown items are skipped, repeats dropped; nothing known: the default
    assert_eq!(f("pct,nope,pct", 5.0), "25%");
    assert_eq!(f("nope", 5.0), "$5.00/$20 █░░░░");
    let left = plain(&budget::fmt(
        &c,
        &opts(&[("show", "pct,bar"), ("count", "left"), ("width", "4")]),
        spend(5.0),
        2000,
    ));
    assert_eq!(left, "75% ███░");
    let cents = plain(&budget::fmt(
        &c,
        &opts(&[("show", "ratio")]),
        Spend {
            usd: 1.0,
            unknown: true,
        },
        1250,
    ));
    assert_eq!(cents, "$1.00~/$12.50");
}

#[test]
fn over_is_bold_bad() {
    let c = Config::default();
    let out = budget::fmt(&c, &opts(&[("show", "spent")]), spend(25.0), 2000);
    assert_eq!(
        out,
        format!("\x1b[1m\x1b[38;5;{}m$25.00\x1b[39m\x1b[22m", c.colors.bad)
    );
    let at = budget::fmt(&c, &opts(&[("show", "spent")]), spend(20.0), 2000);
    assert_eq!(at, format!("\x1b[38;5;{}m$20.00\x1b[39m", c.colors.bad));
}

fn ctx_go(
    p: &dyn Placeholder,
    s: &Session,
    cfg: &Config,
    cache: Option<&Cache>,
    o: &Options,
) -> Option<String> {
    let runner = ColdRunner;
    let ctx = Ctx {
        session: s,
        config: cfg,
        now: NOW,
        runner: &runner,
        cache,
        columns: None,
        plugin_dirs: &[],
        memo: Default::default(),
    };
    p.render(&ctx, o)
}

#[test]
fn session_budget_reads_the_session_cost() {
    let s = Session {
        cost_usd: Some(2.5),
        ..Session::default()
    };
    let c = cfg("set budget-session $10");
    let o = opts(&[("show", "ratio")]);
    assert_eq!(
        ctx_go(&Budget, &s, &c, None, &o).map(|o| plain(&o)),
        Some("$2.50/$10".into())
    );
    // type defaults to session
    assert!(ctx_go(&Budget, &s, &c, None, &opts(&[])).is_some());
    // no budget, or no cost: nothing
    assert_eq!(ctx_go(&Budget, &s, &Config::default(), None, &o), None);
    assert_eq!(ctx_go(&Budget, &Session::default(), &c, None, &o), None);
    assert_eq!(
        ctx_go(&Budget, &s, &c, None, &opts(&[("type", "week")])),
        None
    );
}

#[test]
fn cost_takes_the_session_budget_colour() {
    let s = Session {
        cost_usd: Some(12.0),
        ..Session::default()
    };
    let none = ctx_go(&Cost, &s, &Config::default(), None, &opts(&[])).unwrap();
    assert_eq!(none, "$12.00");
    let c = cfg("set budget-session $10");
    let over = ctx_go(&Cost, &s, &c, None, &opts(&[])).unwrap();
    assert_eq!(
        over,
        format!("\x1b[1m\x1b[38;5;{}m$12.00\x1b[39m\x1b[22m", c.colors.bad)
    );
}

/// Opus 4.5 at $5 per million input tokens.
const AGG: &str = r#"{"v":1,"days":{"2026-11-06":{"claude-opus-4-5":[2000000,0,0,0]},"2026-11-07":{"claude-opus-4-5":[1000000,0,0,0]}},"hours":{"HM8":{"claude-opus-4-5":[400000,0,0,0]},"HM3":{"claude-opus-4-5":[200000,0,0,0]},"HM1":{"claude-opus-4-5":[100000,0,0,0]}}}"#;

fn warm(t: &Tmp, src: &str) -> (Cache, Config) {
    let claude = t.path().join("cl");
    let cache = Cache::with_exe(&t.path().join("st"), "/usr/bin/false".into());
    let agg = AGG
        .replace("HM8", &(H - 8).to_string())
        .replace("HM3", &(H - 3).to_string())
        .replace("HM1", &(H - 1).to_string());
    cache.store(&cache_key(&claude, Tz::Utc), &agg).unwrap();
    let mut c = cfg(src);
    c.claude_dir = Some(claude.to_string_lossy().into_owned());
    (cache, c)
}

#[test]
fn today_month_and_block_read_the_aggregate() {
    let t = Tmp::new();
    let (cache, c) = warm(
        &t,
        "set budget-today $10\nset budget-month $100\nset budget-block $4",
    );
    let s = Session::default();
    let go = |ty: &str, s: &Session| {
        let o = opts(&[("type", ty), ("tz", "utc"), ("show", "ratio")]);
        ctx_go(&Budget, s, &c, Some(&cache), &o).map(|o| plain(&o))
    };
    assert_eq!(go("today", &s), Some("$5.00/$10".into()));
    assert_eq!(go("month", &s), Some("$15.00/$100".into()));
    // no rate-limit window: the block the hours open (H-3, once H-8's ended)
    assert_eq!(go("block", &s), Some("$1.50/$4".into()));
    // Claude Code's 5-hour window decides when it is there
    let five = |resets_at| Session {
        five_hour: Some(Window {
            used_percentage: 10.0,
            resets_at,
        }),
        ..Session::default()
    };
    assert_eq!(go("block", &five(NOW + 4 * 3600)), Some("$0.50/$4".into()));
    assert_eq!(go("block", &five(NOW + 2 * 3600)), Some("$1.50/$4".into()));
    // an expired window falls back to the hours
    assert_eq!(go("block", &five(NOW - 60)), Some("$1.50/$4".into()));
    // cold cache: nothing
    let cold = Cache::with_exe(&t.path().join("st2"), "/usr/bin/false".into());
    let o = opts(&[("type", "today"), ("tz", "utc")]);
    assert_eq!(ctx_go(&Budget, &s, &c, Some(&cold), &o), None);
}

#[test]
fn spendhist_amounts_take_their_budget_colours() {
    let t = Tmp::new();
    let (cache, c) = warm(&t, "set budget-today $5\nset budget-month $100");
    let out = ctx_go(
        &SpendHist,
        &Session::default(),
        &c,
        Some(&cache),
        &opts(&[("tz", "utc")]),
    )
    .unwrap();
    assert_eq!(plain(&out), "today $5.00 · mon $15.00");
    assert!(
        out.contains(&format!("\x1b[38;5;{}m$5.00", c.colors.bad)),
        "{out:?}"
    );
    assert!(
        out.contains(&format!("\x1b[38;5;{}m$15.00", c.ramp[10])),
        "{out:?}"
    );
}

#[test]
fn check_validates_settings_options_and_missing_budgets() {
    use hop_bar::check::{check_text, Level};
    let issues = |text: &str| {
        check_text(text, None, &[])
            .issues
            .into_iter()
            .map(|i| (i.level, i.line, i.message))
            .collect::<Vec<_>>()
    };
    let ok = "set budget-today $20\nset budget-warn 75%\nset left \"#{budget[type=today,show=ratio,pct,bar,glyphs=dots,width=8,count=left,tz=utc]}\"\n";
    assert!(issues(ok).is_empty(), "{:?}", issues(ok));
    let bad = issues("set budget-today twenty\nset budget-warn 120%\n");
    assert_eq!(bad.len(), 2, "{bad:?}");
    assert!(bad.iter().all(|(l, _, _)| *l == Level::Error));
    let opts = issues("set budget-session $5\nset left \"#{budget[type=week]} #{budget[show=nope]} #{budget[count=all]}\"\n");
    assert_eq!(opts.len(), 3, "{opts:?}");
    // a budget the layout shows but the config does not set
    let missing = issues("set left \"#{budget[type=block]} #{budget}\"\n");
    assert_eq!(missing.len(), 2, "{missing:?}");
    assert!(missing[0].2.contains("set budget-block"), "{missing:?}");
    assert!(missing[1].2.contains("set budget-session"), "{missing:?}");
    assert!(missing
        .iter()
        .all(|(l, n, _)| *l == Level::Warning && *n == 1));
    // the type may come from a default line
    let via_default =
        issues("set budget-month $100\ndefault budget type=month\nset left \"#{budget}\"\n");
    assert!(via_default.is_empty(), "{via_default:?}");
}

#[test]
fn an_uncertain_block_start_marks_the_block_spend() {
    let t = Tmp::new();
    let claude = t.path().join("cl");
    let cache = Cache::with_exe(&t.path().join("st"), "/usr/bin/false".into());
    // every three hours from H-22: the block may have opened before the
    // kept hours, so where the running one opened is a guess
    let hours: Vec<String> = (0..8)
        .map(|k| {
            format!(
                "\"{}\":{{\"claude-opus-4-5\":[100000,0,0,0]}}",
                H - 22 + 3 * k
            )
        })
        .collect();
    let agg = format!(r#"{{"v":1,"days":{{}},"hours":{{{}}}}}"#, hours.join(","));
    cache.store(&cache_key(&claude, Tz::Utc), &agg).unwrap();
    let mut c = cfg("set budget-block $4");
    c.claude_dir = Some(claude.to_string_lossy().into_owned());
    let o = opts(&[("type", "block"), ("tz", "utc"), ("show", "ratio")]);
    let got = ctx_go(&Budget, &Session::default(), &c, Some(&cache), &o).map(|o| plain(&o));
    assert_eq!(got, Some("$1.00~/$4".into()));
}
