mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
#[allow(unused_imports)]
use render_fx::*;

use hop_bar::cache::Cache;
use hop_bar::conf::Config;
use hop_bar::options::Options;
use hop_bar::placeholders::spendhist::{self, SpendHist};
use hop_bar::placeholders::{Ctx, Placeholder};
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use hop_bar::spendhist::{cache_key, Tz};
use std::time::{Duration, Instant};

// 2026-11-07T12:00:00Z
const NOW: i64 = 1_794_052_800;
const OPUS: &str = "claude-opus-4-5-20251101";

fn line(id: &str, ts: &str, input: u64) -> String {
    format!(
        "{{\"timestamp\":\"{ts}\",\"message\":{{\"id\":\"{id}\",\"model\":\"{OPUS}\",\"usage\":{{\"input_tokens\":{input},\"output_tokens\":0}}}}}}\n"
    )
}

fn opts(kv: &[(&str, &str)]) -> Options {
    let mut o = Options::default();
    for (k, v) in kv {
        o.set(k, v);
    }
    o
}

fn go(cache: Option<&Cache>, cfg: &Config, o: &Options) -> Option<String> {
    let s = Session::default();
    let runner = ColdRunner;
    let ctx = Ctx {
        session: &s,
        config: cfg,
        now: NOW,
        runner: &runner,
        cache,
        columns: None,
        plugin_dirs: &[],
    };
    SpendHist.render(&ctx, o)
}

fn plain(s: &str) -> String {
    hop_bar::style::strip_codes(s)
}

fn cfg_in(dir: &std::path::Path) -> Config {
    Config {
        claude_dir: Some(dir.to_string_lossy().into_owned()),
        ..Config::default()
    }
}

#[test]
fn format_both_today_month_and_tilde() {
    let both = spendhist::fmt("both", 3.12, false, 84.1, false);
    assert_eq!(plain(&both), "today $3.12 · mon $84.10");
    assert!(both.contains("\x1b[38;5;250mtoday\x1b[39m"), "{both:?}");
    assert_eq!(
        plain(&spendhist::fmt("today", 3.12, false, 84.1, false)),
        "$3.12"
    );
    assert_eq!(
        plain(&spendhist::fmt("month", 3.12, false, 84.1, false)),
        "$84.10"
    );
    assert_eq!(
        plain(&spendhist::fmt("both", 3.12, true, 84.1, true)),
        "today $3.12~ · mon $84.10~"
    );
    assert_eq!(
        plain(&spendhist::fmt("both", 0.0, false, 0.004, false)),
        "today $0.00 · mon $0.00"
    );
}

#[test]
fn render_reads_the_stored_aggregate_and_never_scans() {
    let t = Tmp::new();
    let claude = t.path().join("cl");
    std::fs::create_dir_all(claude.join("projects/p")).unwrap();
    // a transcript the render must not read
    std::fs::write(
        claude.join("projects/p/a.jsonl"),
        line("m", "2026-11-07T01:00:00Z", 9_000_000),
    )
    .unwrap();
    let state = t.path().join("st");
    let cache = Cache::with_exe(&state, "/usr/bin/false".into());
    let cfg = cfg_in(&claude);
    // cold: empty, and nothing was scanned (no state written)
    assert_eq!(go(Some(&cache), &cfg, &opts(&[("tz", "utc")])), None);
    assert!(!state.join("spendhist").exists());
    // warm: the stored value renders
    let key = cache_key(&claude, Tz::Utc);
    let agg = r#"{"v":1,"days":{"2026-11-06":{"claude-opus-4-5":[2000000,0,0,0]},"2026-11-07":{"claude-opus-4-5":[1000000,0,0,0]},"2026-10-31":{"claude-opus-4-5":[100000000,0,0,0]}}}"#;
    cache.store(&key, agg).unwrap();
    let out = go(Some(&cache), &cfg, &opts(&[("tz", "utc")])).unwrap();
    assert_eq!(plain(&out), "today $5.00 · mon $15.00");
    let out = go(
        Some(&cache),
        &cfg,
        &opts(&[("tz", "utc"), ("period", "today")]),
    )
    .unwrap();
    assert_eq!(plain(&out), "$5.00");
    assert!(!state.join("spendhist").exists());
}

#[test]
fn render_applies_price_override_and_unknown_tilde() {
    let t = Tmp::new();
    let claude = t.path().join("cl");
    let cache = Cache::with_exe(&t.path().join("st"), "/usr/bin/false".into());
    let mut cfg = cfg_in(&claude);
    let agg = r#"{"v":1,"days":{"2026-11-07":{"claude-opus-4-5":[1000000,0,0,0],"zz-new":[1000000,0,0,0]}}}"#;
    cache.store(&cache_key(&claude, Tz::Utc), agg).unwrap();
    let o = opts(&[("tz", "utc"), ("period", "today")]);
    assert_eq!(plain(&go(Some(&cache), &cfg, &o).unwrap()), "$5.00~");
    cfg.prices
        .push(hop_bar::prices::Price::parse("zz", "2", "0", "0", "0").unwrap());
    assert_eq!(plain(&go(Some(&cache), &cfg, &o).unwrap()), "$7.00");
}

#[test]
fn cli_end_to_end_with_background_job() {
    let t = Tmp::new();
    let claude = t.path().join("cl");
    std::fs::create_dir_all(claude.join("projects/p")).unwrap();
    let today = hop_bar::spendhist::date_of(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
        Tz::Utc,
    );
    std::fs::write(
        claude.join("projects/p/a.jsonl"),
        line("m", &format!("{today}T00:00:01Z"), 1_000_000),
    )
    .unwrap();
    let conf = format!(
        "set claude-dir \"{}\"\nset left \"#{{spendhist[tz=utc,period=today]}}\"",
        claude.display()
    );
    let first = run(&t).conf(&conf).go().plain.trim().to_string();
    assert_eq!(first, "", "cold cache renders empty");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let out = run(&t).conf(&conf).go().plain.trim().to_string();
        if out == "$5.00" {
            break;
        }
        assert!(Instant::now() < deadline, "never finished: {out:?}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn config_parses_claude_dir_and_price() {
    let c = hop_bar::conf::parse(
        "set claude-dir /x/y\nset price claude-foo 1.5 2 3.75 0.1\nset price bar 1 1 1 1\n",
    )
    .unwrap();
    assert_eq!(c.claude_dir.as_deref(), Some("/x/y"));
    assert_eq!(c.prices.len(), 2);
    assert_eq!(c.prices[0].prefix, "claude-foo");
    assert_eq!(
        c.prices[0].rates,
        [1_500_000, 2_000_000, 3_750_000, 100_000]
    );
    for bad in [
        "set price x 1 2 3",
        "set price x a b c d",
        "set price x 1 2 3 4 5",
        "set claude-dir",
    ] {
        assert!(hop_bar::conf::parse(bad).is_err(), "{bad}");
    }
}
