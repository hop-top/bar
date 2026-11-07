mod common;

use hop_bar::cache::{Cache, Request};
use hop_bar::conf::{parse, Config};
use hop_bar::options::Options;
use hop_bar::pace::{metric_score, score};
use hop_bar::placeholders::Ctx;
use hop_bar::plugin::{
    self, cached_info, discover, find, info, info_job, parse_info, parse_output, validate, Found,
    DEFAULT_TTL,
};
use hop_bar::render::{render_in, ColdRunner};
use hop_bar::session::{Session, Window};
use hop_bar::style::{fg, ramp};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static N: AtomicUsize = AtomicUsize::new(0);
const NOW: i64 = 1_800_000_000;
const DAY: i64 = 86_400;

/// Temp root with fixture plugins copied into `bin` (mode 0755) and a
/// private state dir.
struct Fx {
    root: PathBuf,
    bin: PathBuf,
    state: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let root = std::env::temp_dir().join(format!(
            "bar-plugin-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let (bin, state) = (root.join("bin"), root.join("state"));
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugins");
        for e in std::fs::read_dir(src).unwrap().flatten() {
            install(&e.path(), &bin.join(e.file_name()));
        }
        Fx { root, bin, state }
    }

    fn dirs(&self) -> Vec<PathBuf> {
        vec![self.bin.clone()]
    }

    fn found(&self, name: &str) -> Found {
        find(&self.dirs(), name).unwrap()
    }

    /// Never spawns: a cold read stays cold.
    fn cache(&self) -> Cache {
        Cache::with_exe(&self.state, PathBuf::from("/nonexistent/bar"))
    }

    fn file(&self, rel: &str) -> String {
        std::fs::read_to_string(self.bin.join(rel)).unwrap_or_default()
    }

    fn put(&self, rel: &str, body: &str) {
        std::fs::write(self.bin.join(rel), body).unwrap();
    }

    fn info_calls(&self) -> usize {
        self.file("info.log").lines().count()
    }

    /// Stores the interrogation result, as a finished refresh would.
    fn prime_info(&self, name: &str) {
        let (key, job) = info_job(&self.found(name));
        refresh(&self.cache(), key, job);
    }

    /// Primes the info, then runs the render job once.
    fn prime(&self, name: &str, cfg: &Config, o: &[(&str, &str)]) {
        self.prime_in(&session(), name, cfg, o);
    }

    fn prime_in(&self, s: &Session, name: &str, cfg: &Config, o: &[(&str, &str)]) {
        self.prime_info(name);
        let s = s.clone();
        let cache = self.cache();
        let dirs = self.dirs();
        let ctx = ctx(&s, cfg, &cache, &dirs);
        let found = self.found(name);
        let inf = cached_info(&cache, &found).unwrap();
        let p = plugin::plan(&ctx, &found, &inf, &opts(o)).unwrap();
        refresh(&cache, p.key, p.job);
    }

    fn show(&self, name: &str, cfg: &Config, o: &[(&str, &str)]) -> String {
        self.show_in(&session(), name, cfg, o)
    }

    fn show_in(&self, s: &Session, name: &str, cfg: &Config, o: &[(&str, &str)]) -> String {
        let s = s.clone();
        let cache = self.cache();
        let dirs = self.dirs();
        plugin::render(&ctx(&s, cfg, &cache, &dirs), name, &opts(o)).unwrap_or_default()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn install(from: &Path, to: &Path) {
    std::fs::copy(from, to).unwrap();
    std::fs::set_permissions(to, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

fn refresh(cache: &Cache, key: String, job: hop_bar::cache::Job) {
    cache.refresh(
        &Request {
            key,
            job,
            lock: String::new(),
        },
        Duration::from_secs(20),
    );
}

fn opts(o: &[(&str, &str)]) -> Options {
    let mut out = Options::default();
    for (k, v) in o {
        out.set(k, v);
    }
    out
}

fn session() -> Session {
    Session {
        cwd: "/tmp/work".into(),
        model: "Opus".into(),
        session_id: "s-1".into(),
        five_hour: Some(Window {
            used_percentage: 10.0,
            resets_at: NOW + 3600,
        }),
        ..Session::default()
    }
}

fn ctx<'a>(s: &'a Session, cfg: &'a Config, cache: &'a Cache, dirs: &'a [PathBuf]) -> Ctx<'a> {
    Ctx {
        session: s,
        config: cfg,
        now: NOW,
        runner: &ColdRunner,
        cache: Some(cache),
        columns: Some(120),
        plugin_dirs: dirs,
    }
}

fn cfg(src: &str) -> Config {
    parse(src).unwrap()
}

fn paint(out: &str, name: &str, cfg: &Config) -> String {
    plugin::paint(&parse_output(out).unwrap(), name, cfg, NOW).unwrap_or_default()
}

// --- discovery ---

#[test]
fn discovers_prefixed_executables() {
    let fx = Fx::new();
    std::fs::write(fx.bin.join("unrelated"), "#!/bin/sh\n").unwrap();
    let names: Vec<String> = discover(&fx.dirs()).into_iter().map(|f| f.name).collect();
    assert_eq!(names, ["echo", "plain", "slow", "slowinfo"]);
    assert_eq!(fx.found("echo").path, fx.bin.join("bar-echo"));
}

#[test]
fn first_on_path_wins() {
    let fx = Fx::new();
    let second = fx.root.join("bin2");
    std::fs::create_dir_all(&second).unwrap();
    install(&fx.bin.join("bar-echo"), &second.join("bar-echo"));
    install(&fx.bin.join("bar-echo"), &second.join("bar-only2"));
    let dirs = vec![second.clone(), fx.bin.clone()];
    let all = discover(&dirs);
    let echo: Vec<&Found> = all.iter().filter(|f| f.name == "echo").collect();
    assert_eq!(echo.len(), 1);
    assert_eq!(echo[0].path, second.join("bar-echo"));
    assert_eq!(find(&dirs, "echo").unwrap().path, second.join("bar-echo"));
    assert!(all.iter().any(|f| f.name == "only2"));
}

#[test]
fn skips_non_executable() {
    let fx = Fx::new();
    let first = fx.root.join("first");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::copy(fx.bin.join("bar-echo"), first.join("bar-echo")).unwrap();
    std::fs::set_permissions(
        first.join("bar-echo"),
        std::os::unix::fs::PermissionsExt::from_mode(0o644),
    )
    .unwrap();
    std::fs::write(first.join("bar-dud"), "#!/bin/sh\n").unwrap();
    let dirs = vec![first.clone(), fx.bin.clone()];
    let all = discover(&dirs);
    assert!(!all.iter().any(|f| f.name == "dud"));
    assert_eq!(find(&dirs, "echo").unwrap().path, fx.bin.join("bar-echo"));
    assert!(find(&dirs, "dud").is_none());
}

#[test]
fn core_name_wins_over_plugin() {
    let fx = Fx::new();
    assert!(fx.bin.join("bar-account").is_file());
    assert!(!discover(&fx.dirs()).iter().any(|f| f.name == "account"));
    assert!(find(&fx.dirs(), "account").is_none());
    for core in [
        "account", "repo", "branch", "model", "effort", "ctx", "usage", "prs", "issues",
    ] {
        install(
            &fx.bin.join("bar-echo"),
            &fx.bin.join(format!("bar-{core}")),
        );
        assert!(find(&fx.dirs(), core).is_none(), "{core}");
    }
}

#[test]
fn cache_key_includes_cwd() {
    let fx = Fx::new();
    let c = cfg("");
    let a = session();
    let b = Session {
        cwd: "/tmp/other-repo".into(),
        ..session()
    };
    fx.prime_info("echo");
    let (cache, dirs) = (fx.cache(), fx.dirs());
    let found = fx.found("echo");
    let inf = cached_info(&cache, &found).unwrap();
    let key = |s: &Session| {
        plugin::plan(&ctx(s, &c, &cache, &dirs), &found, &inf, &opts(&[]))
            .unwrap()
            .key
    };
    assert_ne!(key(&a), key(&b));
    let same_cwd = Session {
        session_id: "s-2".into(),
        context_used: Some(50.0),
        ..session()
    };
    assert_eq!(key(&a), key(&same_cwd), "only cwd splits the cache");
    fx.put("out.json", r#"{"text":"A"}"#);
    fx.prime_in(&a, "echo", &c, &[]);
    fx.put("out.json", r#"{"text":"B"}"#);
    fx.prime_in(&b, "echo", &c, &[]);
    assert_eq!(fx.show_in(&a, "echo", &c, &[]), "A");
    assert_eq!(fx.show_in(&b, "echo", &c, &[]), "B");
}

#[test]
fn wide_and_invisible_text_measured_through_compose() {
    let fx = Fx::new();
    let c = cfg("set left \"#{echo}\"\n");
    fx.put(
        "out.json",
        r#"{"text":"\u6f22\u5b57\ud83d\ude42\u200b\u2028\u2029x\u0085"}"#,
    );
    fx.prime("echo", &c, &[]);
    let (s, cache, dirs) = (session(), fx.cache(), fx.dirs());
    let out = render_in(&s, &c, Some(120), &ColdRunner, Some(&cache), &dirs);
    let line = out.trim_end();
    assert_eq!(line, "\u{6f22}\u{5b57}\u{1f642}\u{200b}x");
    assert!(!line
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}')));
    assert_eq!(hop_bar::style::visible_width(line), 7);
    assert_eq!(out.lines().count(), 1);
}

#[test]
fn relative_path_dirs_ignored() {
    let dirs = plugin::path_dirs(Some(std::ffi::OsStr::new("/usr/bin:.:rel/bin::/bin")));
    assert_eq!(dirs, [PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
}

// --- ext-info ---

#[test]
fn ext_info_timeout_5s_falls_back() {
    let fx = Fx::new();
    let t = Instant::now();
    let i = info(&fx.found("slowinfo"));
    let took = t.elapsed();
    assert!(took >= Duration::from_millis(4900), "{took:?}");
    assert!(took < Duration::from_secs(7), "{took:?}");
    assert_eq!(i.name, "slowinfo");
    assert_eq!(i.version, "");
    assert!(!i.is_placeholder());
    assert_eq!(i.ttl, DEFAULT_TTL);
}

#[test]
fn ext_info_reads_fields() {
    let fx = Fx::new();
    let i = info(&fx.found("echo"));
    assert_eq!((i.name.as_str(), i.version.as_str()), ("echo", "1.0.0"));
    assert_eq!(i.description, "test plugin");
    assert!(i.is_placeholder());
    assert_eq!(i.options.len(), 4);
}

#[test]
fn ext_info_strings_sanitized() {
    let raw = r#"{"name":"x","version":"1","capabilities":["placeholder"],
      "options":{
        "k\u001b[31m":{"type":"string","default":"d\u001b]8;;\u0007"},
        "m":{"type":"enum","enum":["a\u001b[0m","\u202eb"],"default":"a\u001b[0m"}}}"#;
    let i = parse_info(raw).unwrap();
    let shown = i.to_json().to_string();
    assert!(
        !shown.contains("\\u001b") && !shown.contains('\x1b'),
        "{shown}"
    );
    assert!(
        !shown.contains('\u{202e}') && !shown.contains("\\u0007"),
        "{shown}"
    );
    assert!(i.options.contains_key("k[31m"));
    assert_eq!(i.options["m"].choices, ["a[0m", "b"]);
    assert_eq!(i.options["m"].default, Some(json!("a[0m")));
    assert_eq!(i.options["k[31m"].default, Some(json!("d]8;;")));
    // the sanitised choice is what the user types and what is accepted
    assert!(validate(&i.options, &opts(&[("m", "b")])).is_ok());
}

#[test]
fn ttl_defaults_60s() {
    let base = r#"{"name":"x","version":"1","capabilities":["placeholder"]"#;
    assert_eq!(DEFAULT_TTL, 60);
    assert_eq!(parse_info(&format!("{base}}}")).unwrap().ttl, 60);
    assert_eq!(
        parse_info(&format!(r#"{base},"ttl":300}}"#)).unwrap().ttl,
        300
    );
    assert_eq!(
        parse_info(&format!(r#"{base},"ttl":"5m"}}"#)).unwrap().ttl,
        300
    );
    assert_eq!(
        parse_info(&format!(r#"{base},"ttl":"junk"}}"#))
            .unwrap()
            .ttl,
        60
    );
    assert!(parse_info(r#"{"version":"1"}"#).is_none());
    assert!(parse_info("not json").is_none());
    // the plan uses it, with `plugin <name> ttl=` overriding
    let fx = Fx::new();
    fx.prime_info("echo");
    let (s, cache, dirs) = (session(), fx.cache(), fx.dirs());
    let found = fx.found("echo");
    let inf = cached_info(&cache, &found).unwrap();
    let c = Config::default();
    let p = plugin::plan(&ctx(&s, &c, &cache, &dirs), &found, &inf, &opts(&[])).unwrap();
    assert_eq!(p.ttl, 60);
    let c = cfg("plugin echo ttl=5m\n");
    let p = plugin::plan(&ctx(&s, &c, &cache, &dirs), &found, &inf, &opts(&[])).unwrap();
    assert_eq!(p.ttl, 300);
}

#[test]
fn ext_info_cached_not_per_render() {
    let fx = Fx::new();
    let c = cfg("");
    for _ in 0..3 {
        assert_eq!(fx.show("echo", &c, &[]), "");
    }
    assert_eq!(fx.info_calls(), 0, "cold render must not interrogate");
    fx.prime("echo", &c, &[]);
    assert_eq!(fx.info_calls(), 1);
    for _ in 0..3 {
        assert_eq!(fx.show("echo", &c, &[]), "ok");
    }
    assert_eq!(fx.info_calls(), 1);
}

#[test]
fn plugin_without_placeholder_capability_not_rendered() {
    let fx = Fx::new();
    fx.prime_info("plain");
    let c = Config::default();
    let (s, cache, dirs) = (session(), fx.cache(), fx.dirs());
    let found = fx.found("plain");
    let inf = cached_info(&cache, &found).unwrap();
    assert!(plugin::plan(&ctx(&s, &c, &cache, &dirs), &found, &inf, &opts(&[])).is_none());
}

// --- options ---

#[test]
fn options_validated_against_schema() {
    let i = info(&Fx::new().found("echo"));
    let ok = validate(
        &i.options,
        &opts(&[
            ("limit", "7"),
            ("mode", "b"),
            ("verbose", "true"),
            ("label", "x y"),
        ]),
    )
    .unwrap();
    assert_eq!(
        Value::Object(ok),
        json!({"limit": 7, "mode": "b", "verbose": true, "label": "x y"})
    );
    let defaults = validate(&i.options, &opts(&[])).unwrap();
    assert_eq!(Value::Object(defaults), json!({"limit": 3}));
    for bad in [
        ("limit", "seven"),
        ("mode", "c"),
        ("verbose", "maybe"),
        ("nope", "1"),
    ] {
        assert!(validate(&i.options, &opts(&[bad])).is_err(), "{bad:?}");
    }
    // invalid options omit the segment
    let fx = Fx::new();
    let c = cfg("");
    fx.prime("echo", &c, &[]);
    assert_eq!(fx.show("echo", &c, &[("mode", "zzz")]), "");
}

#[test]
fn render_gets_options_on_stdin() {
    let fx = Fx::new();
    let c = cfg("plugin echo org=acme keychain=entry-name\n");
    fx.prime("echo", &c, &[("mode", "a"), ("verbose", "no")]);
    let input: Value = serde_json::from_str(&fx.file("stdin.json")).unwrap();
    assert_eq!(
        input["options"],
        json!({"limit": 3, "mode": "a", "verbose": false})
    );
    assert_eq!(
        input["config"],
        json!({"org": "acme", "keychain": "entry-name"})
    );
    assert_eq!(input["columns"], json!(120));
    assert_eq!(input["session"]["cwd"], json!("/tmp/work"));
    assert_eq!(input["session"]["model"], json!("Opus"));
    assert_eq!(
        input["session"]["five_hour"]["resets_at"],
        json!(NOW + 3600)
    );
    let mut keys: Vec<&String> = input.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["columns", "config", "options", "session"]);
    assert!(!fx.file("stdin.json").contains("PATH"));
    assert_eq!(
        fx.show("echo", &c, &[("mode", "a"), ("verbose", "no")]),
        "ok"
    );
}

// --- failures ---

#[test]
fn malformed_output_omits_segment() {
    let fx = Fx::new();
    let c = cfg("");
    fx.put("out.json", "not json at all");
    fx.prime("echo", &c, &[]);
    assert_eq!(fx.show("echo", &c, &[]), "");
    // a later malformed run keeps the last good value
    fx.put("out.json", r#"{"text":"good"}"#);
    fx.prime("echo", &c, &[]);
    fx.put("out.json", r#"{"txt":"bad"}"#);
    fx.prime("echo", &c, &[]);
    assert_eq!(fx.show("echo", &c, &[]), "good");
    assert!(parse_output(r#"{"text":5}"#).is_none());
    assert!(parse_output("[]").is_none());
}

#[test]
fn nonzero_exit_omits_segment() {
    let fx = Fx::new();
    let c = cfg("");
    fx.put("exit", "3");
    fx.prime("echo", &c, &[]);
    assert_eq!(fx.show("echo", &c, &[]), "");
}

#[test]
fn render_timeout_kills_plugin() {
    let fx = Fx::new();
    let found = fx.found("slow");
    let job = hop_bar::cache::Job::PluginRender {
        path: found.path.display().to_string(),
        input: "{}".into(),
    };
    let t = Instant::now();
    assert!(job.execute(Duration::from_secs(20)).is_none());
    let took = t.elapsed();
    assert!(took >= Duration::from_millis(2900), "{took:?}");
    assert!(took < Duration::from_secs(5), "{took:?}");
    let pid = fx.file("pid").trim().to_string();
    assert!(!pid.is_empty());
    let alive = std::process::Command::new("kill")
        .args(["-0", &pid])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(!alive, "plugin {pid} still running");
}

#[test]
fn slow_plugin_never_blocks_render() {
    let fx = Fx::new();
    fx.prime_info("slow");
    let cache = Cache::with_exe(&fx.state, PathBuf::from(env!("CARGO_BIN_EXE_bar")));
    let (s, c, dirs) = (
        session(),
        cfg("set left \"#{slow} #{slowinfo}\"\n"),
        fx.dirs(),
    );
    let t = Instant::now();
    let out = render_in(&s, &c, Some(120), &ColdRunner, Some(&cache), &dirs);
    let took = t.elapsed();
    assert_eq!(out, "");
    assert!(took < Duration::from_millis(1000), "{took:?}");
}

// --- output styling ---

#[test]
fn plugin_escape_codes_stripped() {
    let c = Config::default();
    let out = paint(
        r#"{"text":"\u001b[31mred\u001b[0m \u202eevil\u0007","link":"https://x.test/\u001b]8;;"}"#,
        "p",
        &c,
    );
    assert!(!out.contains('\x1b'), "{out:?}");
    assert!(!out.contains('\u{202e}') && !out.contains('\x07'));
    assert!(out.contains("red") && out.contains("evil"));
    let linked = paint(r#"{"text":"t","link":"https://x.test/a"}"#, "p", &c);
    assert_eq!(linked, "\x1b]8;;https://x.test/a\x1b\\t\x1b]8;;\x1b\\");
    assert_eq!(paint(r#"{"text":"\u001b\u0007"}"#, "p", &c), "");
}

#[test]
fn style_maps_to_palette() {
    let c = cfg("set color.good 10\nset color.warn 11\nset color.bad 12\n");
    assert_eq!(
        paint(r#"{"text":"a","style":"good"}"#, "p", &c),
        fg(10, "a")
    );
    assert_eq!(
        paint(r#"{"text":"a","style":"warn"}"#, "p", &c),
        fg(11, "a")
    );
    assert_eq!(paint(r#"{"text":"a","style":"bad"}"#, "p", &c), fg(12, "a"));
    assert_eq!(
        paint(r#"{"text":"a","style":"dim"}"#, "p", &c),
        hop_bar::style::dim("a")
    );
    assert_eq!(paint(r#"{"text":"a","style":"pink"}"#, "p", &c), "a");
}

#[test]
fn score_goes_on_ramp() {
    let c = Config::default();
    for s in [0, 37, 50, 100] {
        assert_eq!(
            paint(&format!(r#"{{"text":"a","score":{s}}}"#), "p", &c),
            fg(ramp(s, &c.ramp), "a")
        );
    }
}

#[test]
fn score_out_of_range_clamped() {
    let c = Config::default();
    let red = fg(c.ramp[0], "a");
    let green = fg(c.ramp[10], "a");
    assert_eq!(paint(r#"{"text":"a","score":-40}"#, "p", &c), red);
    assert_eq!(paint(r#"{"text":"a","score":250}"#, "p", &c), green);
    assert_eq!(paint(r#"{"text":"a","score":1e300}"#, "p", &c), green);
}

#[test]
fn style_beats_score() {
    let c = Config::default();
    assert_eq!(
        paint(r#"{"text":"a","style":"bad","score":100}"#, "p", &c),
        fg(c.colors.bad, "a")
    );
    let metric = format!(
        r#""metric":{{"remaining":0,"granted":100,"expires_at":{}}}"#,
        NOW + DAY
    );
    let c2 = cfg("rule p early pace final=3d bad=$50\n");
    assert_eq!(
        paint(&format!(r#"{{"text":"a","score":0,{metric}}}"#), "p", &c2),
        fg(c2.ramp[0], "a"),
        "score beats metric"
    );
}

fn credits_rule() -> hop_bar::conf::Rule {
    cfg("rule claude-credits early pace final=3d bad=$50\n").rules[0].clone()
}

#[test]
fn rule_applies_to_metric() {
    let rule = credits_rule();
    for (cents, want) in [(0, 100), (2500, 51), (5000, 1), (5001, 0)] {
        assert_eq!(
            metric_score(cents, 20_000, Some(2 * DAY), 0, &rule),
            want,
            "{cents}"
        );
    }
    // through paint: rule target equals the placeholder name
    let c = cfg("rule claude-credits early pace final=3d bad=$50\n");
    let out = format!(
        r#"{{"text":"$25","metric":{{"remaining":2500,"granted":20000,"expires_at":{}}}}}"#,
        NOW + 2 * DAY
    );
    assert_eq!(
        paint(&out, "claude-credits", &c),
        fg(ramp(51, &c.ramp), "$25")
    );
    assert_eq!(paint(&out, "other", &c), "$25", "rule for another name");
}

#[test]
fn rule_pace_against_grant() {
    let rule = credits_rule();
    let (secs, period) = (20 * DAY, 30 * DAY);
    for (cents, pct) in [(10_000, 100), (5_000, 50), (2_000, 20), (1_000, 10)] {
        assert_eq!(
            metric_score(cents, 10_000, Some(secs), period, &rule),
            score(pct, Some(secs), period, &rule),
            "{cents}"
        );
    }
    assert_eq!(metric_score(5_000, 10_000, Some(secs), period, &rule), 75);
    assert_eq!(metric_score(1_000, 10_000, Some(secs), period, &rule), 0);
    // dollar thresholds are read as shares of the grant
    let r = cfg("rule c early warn=$20 bad=$5 final=1d bad=$10\n").rules[0].clone();
    let pct = cfg("rule c early warn=20% bad=5% final=1d bad=30%\n").rules[0].clone();
    assert_eq!(
        metric_score(1_500, 10_000, None, 0, &r),
        score(15, None, 0, &pct)
    );
    // through paint, with granted_at giving the period
    let c = cfg("rule claude-credits early pace final=3d bad=$50\n");
    let out = format!(
        r#"{{"text":"x","metric":{{"remaining":5000,"granted":10000,"expires_at":{},"granted_at":{}}}}}"#,
        NOW + secs,
        NOW + secs - period
    );
    assert_eq!(
        paint(&out, "claude-credits", &c),
        fg(ramp(75, &c.ramp), "x")
    );
}

// --- CLI ---

fn cli(fx: &Fx) -> assert_cmd::Command {
    let (iso, cwd) = common::isolation(&fx.root);
    let mut c = assert_cmd::Command::cargo_bin("bar").unwrap();
    c.envs(iso)
        .current_dir(cwd)
        .env("HOME", &fx.root)
        .env("PATH", format!("{}:/usr/bin:/bin", fx.bin.display()));
    c
}

#[test]
fn plugin_list_and_info_cli() {
    let fx = Fx::new();
    std::fs::remove_file(fx.bin.join("bar-slowinfo")).unwrap();
    let out = cli(&fx)
        .args(["plugin", "list", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).unwrap();
    let names: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["echo", "plain", "slow"]);
    let out = cli(&fx)
        .args(["plugin", "info", "echo", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["version"], json!("1.0.0"));
    assert_eq!(v["ttl"], json!(60));
    assert_eq!(v["options"]["mode"]["enum"], json!(["a", "b"]));
    cli(&fx)
        .args(["plugin", "info", "account"])
        .assert()
        .failure();
}
