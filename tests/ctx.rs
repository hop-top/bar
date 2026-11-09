use hop_bar::compact::{threshold, CompactAt, NATIVE_1M_DEFAULT};
use hop_bar::conf::{self, Config};
use hop_bar::options::Options;
use hop_bar::placeholders::ctx::{render_parts, Sources};
use hop_bar::placeholders::Ctx;
use hop_bar::render::ColdRunner;
use hop_bar::session::{self, CtxUsage, Session};
use hop_bar::style::strip_codes;
use hop_bar::transcript::Stats;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

const STDIN: &str = r#"{"model":{"id":"claude-opus-4-8"},"context_window":{"used_percentage":38,"context_window_size":200000,"total_input_tokens":76000,"current_usage":{"input_tokens":1200,"output_tokens":300,"cache_creation_input_tokens":4800,"cache_read_input_tokens":70000}}}"#;

fn opts(raw: &str) -> Options {
    let mut o = Options::default();
    for kv in raw.split(';').filter(|k| !k.is_empty()) {
        let (k, v) = kv.split_once('=').unwrap();
        o.set(k, v);
    }
    o
}

struct Env {
    vars: HashMap<&'static str, String>,
    settings: Option<Value>,
    stats: Option<Stats>,
    config: Config,
}

impl Env {
    fn new() -> Env {
        Env {
            vars: HashMap::new(),
            settings: None,
            stats: None,
            config: Config::default(),
        }
    }

    fn raw(&self, s: &Session, o: &str, label: bool) -> Option<String> {
        let runner = ColdRunner;
        let ctx = Ctx {
            session: s,
            config: &self.config,
            now: 0,
            runner: &runner,
            cache: None,
            columns: None,
            plugin_dirs: &[],
        };
        let stats = || self.stats.clone();
        let env = |k: &str| self.vars.get(k).cloned();
        let settings = || self.settings.clone();
        let src = Sources {
            state: Path::new("/nonexistent/bar-test"),
            stats: &stats,
            env: &env,
            settings: &settings,
        };
        render_parts(&ctx, &opts(o), &src, label, || unreachable!("no link"))
    }

    fn show(&self, s: &Session, o: &str) -> Option<String> {
        self.raw(s, o, true).map(|t| strip_codes(&t))
    }
}

fn sess() -> Session {
    session::parse(STDIN)
}

#[test]
fn parses_context_window_fields() {
    let s = sess();
    assert_eq!(s.context_size, Some(200_000));
    assert_eq!(s.context_tokens, Some(76_000));
    assert_eq!(
        s.context_usage,
        Some(CtxUsage {
            input: 1_200,
            cache_creation: 4_800,
            cache_read: 70_000
        })
    );
    let s = session::parse(
        r#"{"context_window":{"context_window_size":0,"total_input_tokens":-1,"current_usage":null}}"#,
    );
    assert_eq!(
        (s.context_size, s.context_tokens, s.context_usage),
        (None, None, None)
    );
}

#[test]
fn default_is_the_percent() {
    let e = Env::new();
    assert_eq!(e.show(&sess(), "").unwrap(), "ctx 38%");
    assert_eq!(e.show(&sess(), "show=nope").unwrap(), "ctx 38%");
    assert_eq!(e.show(&Session::default(), ""), None);
}

#[test]
fn bar_and_gauge_use_usage_glyphs_and_width() {
    let e = Env::new();
    assert_eq!(e.show(&sess(), "show=bar").unwrap(), "ctx ██░░░");
    assert_eq!(
        e.show(&sess(), "show=bar,pct;width=10;glyphs=dots")
            .unwrap(),
        "ctx ●●●●○○○○○○ 38%"
    );
    assert_eq!(e.show(&sess(), "show=gauge").unwrap(), "ctx ▄");
    assert_eq!(
        e.show(&sess(), "show=pct,bar;count=left").unwrap(),
        "ctx 62% ███░░"
    );
}

#[test]
fn bar_colours_follow_the_ramp() {
    let e = Env::new();
    let ramp = Config::default().ramp;
    // 38% used: score 59 -> ramp[6]
    let solid = e.raw(&sess(), "show=bar", false).unwrap();
    assert!(
        solid.starts_with(&format!("\x1b[38;5;{}m██", ramp[6])),
        "{solid:?}"
    );
    let grad = e
        .raw(&sess(), "show=bar;width=10;color=gradient", false)
        .unwrap();
    // good end first, the last filled cell matches the solid colour
    assert!(
        grad.starts_with(&format!("\x1b[38;5;{}m", ramp[9])),
        "{grad:?}"
    );
    assert!(
        grad.contains(&format!("\x1b[38;5;{}m█\x1b[39m", ramp[6])),
        "{grad:?}"
    );
}

#[test]
fn tokens_against_window_or_compact_point() {
    let mut e = Env::new();
    assert_eq!(e.show(&sess(), "show=tokens").unwrap(), "ctx 76k/200k");
    e.vars
        .insert("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "100k".into());
    assert_eq!(
        e.show(&sess(), "show=pct,tokens;of=compact").unwrap(),
        "ctx 76% 76k/100k"
    );
    assert_eq!(
        e.show(&sess(), "show=pct;of=compact;count=left").unwrap(),
        "ctx 24%"
    );
}

#[test]
fn tokens_derived_from_percent_when_total_missing() {
    let s =
        session::parse(r#"{"context_window":{"used_percentage":50,"context_window_size":200000}}"#);
    assert_eq!(Env::new().show(&s, "show=tokens").unwrap(), "ctx 100k/200k");
}

#[test]
fn left_is_room_before_auto_compaction() {
    let mut e = Env::new();
    // a 200k model compacts at its limit
    assert_eq!(e.show(&sess(), "show=left").unwrap(), "ctx 124k left");
    e.config = conf::parse("set compact-at 150k").unwrap();
    assert_eq!(e.show(&sess(), "show=left").unwrap(), "ctx 74k left");
    e.config = conf::parse("set compact-at 50%").unwrap();
    assert_eq!(e.show(&sess(), "show=left").unwrap(), "ctx 24k left");
    // past the point: no room
    e.config = conf::parse("set compact-at 60000").unwrap();
    assert_eq!(e.show(&sess(), "show=left").unwrap(), "ctx 0 left");
}

#[test]
fn left_hidden_when_auto_compaction_is_off() {
    let mut e = Env::new();
    e.settings = Some(json!({"autoCompactEnabled": false}));
    assert_eq!(e.show(&sess(), "show=left"), None);
    assert_eq!(e.show(&sess(), "show=pct;of=compact").unwrap(), "ctx 38%");
    // an explicit point still applies
    e.config = conf::parse("set compact-at 100k").unwrap();
    assert_eq!(e.show(&sess(), "show=left").unwrap(), "ctx 24k left");
}

#[test]
fn split_shows_cache_read_write_and_new_input() {
    let e = Env::new();
    assert_eq!(
        e.show(&sess(), "show=split").unwrap(),
        "ctx r 70k w 4.8k new 1.2k"
    );
    let s = Session {
        context_usage: Some(CtxUsage::default()),
        ..sess()
    };
    assert_eq!(e.show(&s, "show=split"), None);
}

fn grown(first: u64, prev: u64, last: u64, reqs: u64) -> Stats {
    Stats {
        ctx_first: first,
        ctx_prev: prev,
        ctx_last: last,
        ctx_reqs: reqs,
        ..Stats::default()
    }
}

#[test]
fn growth_and_trend_from_transcript_requests() {
    let mut e = Env::new();
    e.stats = Some(grown(40_000, 72_800, 76_000, 5));
    assert_eq!(
        e.show(&sess(), "show=growth,trend").unwrap(),
        "ctx +3.2k +9k/req"
    );
    e.stats = Some(grown(80_000, 81_000, 60_000, 3));
    assert_eq!(
        e.show(&sess(), "show=growth,trend").unwrap(),
        "ctx -21k -10k/req"
    );
    e.stats = Some(grown(40_000, 0, 40_000, 1));
    assert_eq!(e.show(&sess(), "show=growth,trend"), None);
    e.stats = None;
    assert_eq!(e.show(&sess(), "show=growth"), None);
}

#[test]
fn icon_label_off_keeps_parts() {
    let e = Env::new();
    let out = e.raw(&sess(), "show=pct,tokens", false).unwrap();
    assert_eq!(strip_codes(&out), "38% 76k/200k");
}

fn thr(window: u64, vars: &[(&str, &str)], settings: Option<Value>) -> Option<u64> {
    thr_m(window, "claude-opus-5-5", vars, settings)
}

fn thr_m(window: u64, model: &str, vars: &[(&str, &str)], settings: Option<Value>) -> Option<u64> {
    let env = |k: &str| {
        vars.iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| v.to_string())
    };
    threshold(window, model, None, &env, &|| settings.clone())
}

#[test]
fn threshold_defaults_per_window() {
    assert_eq!(thr(200_000, &[], None), Some(200_000));
    assert_eq!(thr(1_000_000, &[], None), Some(NATIVE_1M_DEFAULT));
    assert_eq!(thr(0, &[], None), None);
}

#[test]
fn threshold_env_window_wins_and_is_clamped() {
    let s = Some(json!({"autoCompactWindow": 300000}));
    let w = "CLAUDE_CODE_AUTO_COMPACT_WINDOW";
    assert_eq!(thr(1_000_000, &[(w, "500k")], s.clone()), Some(500_000));
    assert_eq!(thr(1_000_000, &[(w, "1.5M")], None), Some(1_000_000));
    // a bare number is tokens: 200 tokens reads as the 100k minimum
    assert_eq!(thr(1_000_000, &[(w, "200")], None), Some(100_000));
    // unusable: ignored, the setting applies
    assert_eq!(thr(1_000_000, &[(w, "big")], s), Some(300_000));
    // capped at the model's window
    assert_eq!(thr(200_000, &[(w, "500000")], None), Some(200_000));
}

#[test]
fn threshold_settings_per_model_then_global() {
    let s = json!({
        "autoCompactWindow": 300000,
        "modelSettings": {"claude-opus-5-5": {"autoCompactWindow": 400000}}
    });
    assert_eq!(thr(1_000_000, &[], Some(s)), Some(400_000));
    let s = json!({
        "autoCompactWindow": 300000,
        "modelSettings": {"claude-opus-5-5": {"autoCompactWindow": "auto"}}
    });
    assert_eq!(thr(1_000_000, &[], Some(s)), Some(NATIVE_1M_DEFAULT));
    let s = json!({"autoCompactWindow": 300000, "modelSettings": {"other": {}}});
    assert_eq!(thr(1_000_000, &[], Some(s)), Some(300_000));
}

#[test]
fn threshold_model_settings_match_the_canonical_name() {
    let s = json!({
        "autoCompactWindow": 300000,
        "modelSettings": {
            "claude-opus-4-6": {"autoCompactWindow": 400000},
            "claude-sonnet-4-5": {"autoCompactWindow": 250000}
        }
    });
    for id in [
        "claude-opus-4-6",
        "claude-opus-4-6[1m]",
        "claude-opus-4-6[1M]",
        "claude-opus-4-6-20260101",
        "claude-opus-4-6@20260101",
        "us.anthropic.claude-opus-4-6-v1:0",
        "anthropic/claude-opus-4-6",
    ] {
        assert_eq!(
            thr_m(1_000_000, id, &[], Some(s.clone())),
            Some(400_000),
            "{id}"
        );
    }
    let id = "claude-sonnet-4-5-20250929[1m]";
    assert_eq!(thr_m(1_000_000, id, &[], Some(s.clone())), Some(250_000));
    // another model falls through to the top-level window
    let id = "claude-opus-4-7";
    assert_eq!(thr_m(1_000_000, id, &[], Some(s)), Some(300_000));
}

#[test]
fn threshold_off_switches() {
    let on = Some(json!({"autoCompactEnabled": true}));
    assert_eq!(thr(200_000, &[], on), Some(200_000));
    let off = Some(json!({"autoCompactEnabled": false}));
    assert_eq!(thr(200_000, &[], off), None);
    assert_eq!(thr(200_000, &[("DISABLE_AUTO_COMPACT", "1")], None), None);
    assert_eq!(thr(200_000, &[("DISABLE_COMPACT", "1")], None), None);
    assert_eq!(
        thr(200_000, &[("DISABLE_AUTO_COMPACT", "0")], None),
        Some(200_000)
    );
}

#[test]
fn threshold_pct_override_only_lowers_an_early_point() {
    let p = "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE";
    assert_eq!(
        thr(1_000_000, &[(p, "50")], None),
        Some(NATIVE_1M_DEFAULT / 2)
    );
    // compaction at the limit: the override does not apply
    assert_eq!(thr(200_000, &[(p, "50")], None), Some(200_000));
    assert_eq!(thr(1_000_000, &[(p, "0")], None), Some(NATIVE_1M_DEFAULT));
}

#[test]
fn compact_at_beats_everything() {
    let env = |_: &str| Some("1".to_string());
    let at = |c| threshold(200_000, "m", Some(c), &env, &|| None);
    assert_eq!(at(CompactAt::Tokens(150_000)), Some(150_000));
    assert_eq!(at(CompactAt::Tokens(900_000)), Some(200_000));
    assert_eq!(at(CompactAt::Pct(80)), Some(160_000));
}

#[test]
fn compact_at_parses() {
    assert_eq!(CompactAt::parse("200k"), Some(CompactAt::Tokens(200_000)));
    assert_eq!(CompactAt::parse("1M"), Some(CompactAt::Tokens(1_000_000)));
    assert_eq!(CompactAt::parse("150000"), Some(CompactAt::Tokens(150_000)));
    assert_eq!(CompactAt::parse("90%"), Some(CompactAt::Pct(90)));
    for bad in ["0", "0%", "101%", "-5k", "abc", "", "%"] {
        assert_eq!(CompactAt::parse(bad), None, "{bad}");
    }
    assert!(conf::parse("set compact-at lots").is_err());
}

#[test]
fn check_validates_ctx_options() {
    use hop_bar::check::check_text;
    let ok = "set left \"#{ctx[show=bar,pct,tokens,left,split,growth,trend,gauge,of=compact,count=left,glyphs=eighths,color=gradient,width=8,click=none]}\"\nset compact-at 90%\n";
    assert!(check_text(ok, None, &[]).issues.is_empty());
    for bad in [
        "show=pct,nope",
        "of=model",
        "count=both",
        "glyphs=stars",
        "color=rainbow",
        "width=x",
    ] {
        let text = format!("set left \"#{{ctx[{bad}]}}\"\n");
        assert_eq!(check_text(&text, None, &[]).issues.len(), 1, "{bad}");
    }
    assert_eq!(check_text("set compact-at 0\n", None, &[]).issues.len(), 1);
}
