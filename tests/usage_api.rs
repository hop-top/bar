//! Opt-in usage API: `set usage-api`, `usage[model=...]`, `extra`, and the
//! background job against a fake `curl` and `security`. No network.

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use hop_bar::cache::{Cache, Job, Request};
use hop_bar::conf::{parse, Config};
use hop_bar::options::Options;
use hop_bar::placeholders::{lookup, Ctx};
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use hop_bar::style::strip_codes;
use hop_bar::usageapi::{self, Data, Extra, Model};
use render_fx::Tmp;

const FIXTURE: &str = include_str!("fixtures/usage-api/usage.json");

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn opts(pairs: &[(&str, &str)]) -> Options {
    let mut o = Options::default();
    for (k, v) in pairs {
        o.set(k, v);
    }
    o
}

fn go(cache: &Cache, cfg: &Config, name: &str, o: &Options, at: i64) -> Option<String> {
    let s = Session::default();
    let ctx = Ctx {
        session: &s,
        config: cfg,
        now: at,
        runner: &ColdRunner,
        cache: Some(cache),
        columns: None,
        plugin_dirs: &[],
        memo: Default::default(),
    };
    lookup(name)?.render(&ctx, o).map(|x| strip_codes(&x))
}

/// The cache key the render path uses for this process's environment.
fn key() -> String {
    usageapi::cache_key(&usageapi::config_dir().unwrap().0)
}

fn sample(at: i64) -> Data {
    Data {
        at,
        models: vec![
            Model {
                name: "opus".into(),
                used: 18.5,
                resets_at: at + 3 * 86_400,
            },
            Model {
                name: "sonnet".into(),
                used: 12.0,
                resets_at: at + 3 * 86_400,
            },
        ],
        extra: Some(Extra {
            enabled: true,
            limit: 5000,
            used: 1234,
            util: 24.68,
            currency: "USD".into(),
        }),
        day: hop_bar::tz::day_of(at, hop_bar::tz::Tz::Local),
        base: Some(1000),
        retry_at: 0,
        fails: 0,
    }
}

#[test]
fn config_values_and_project_refusal() {
    assert!(!Config::default().usage_api);
    assert!(parse("set usage-api on\n").unwrap().usage_api);
    assert!(!parse("set usage-api off\n").unwrap().usage_api);
    let e = parse("set usage-api yes\n").unwrap_err();
    assert!(e[0].message.contains("usage-api"), "{e:?}");
    for ok in [
        "#{usage[model=opus]}",
        "#{usage[type=7d;model=sonnet;show=time,pct]}",
        "#{extra}",
        "#{extra[show=used,limit,left,today,daily,pct,bar,gauge,time;count=left;width=8]}",
        "#{spend[show=time,used,limit]}",
    ] {
        let r = hop_bar::check::check_text(&format!("set left \"{ok}\"\n"), None, &[]);
        assert!(!r.has_errors(), "{ok}: {}", r.to_text());
    }
    for bad in [
        "#{extra[show=nope]}",
        "#{extra[count=x]}",
        "#{spend[show=eta]}",
    ] {
        let r = hop_bar::check::check_text(&format!("set left \"{bad}\"\n"), None, &[]);
        assert!(r.has_errors(), "{bad}");
    }
}

#[test]
fn check_warns_when_the_usage_api_is_off() {
    let warn = "no `set usage-api on`";
    for (src, warns) in [
        ("#{extra}", true),
        ("#{usage[model=opus]}", true),
        ("#{usage[type=7d]}", false),
        ("#{spend}", false),
    ] {
        let off = hop_bar::check::check_text(&format!("set left \"{src}\"\n"), None, &[]);
        assert_eq!(
            off.to_text().contains(warn),
            warns,
            "{src}: {}",
            off.to_text()
        );
        assert!(!off.has_errors());
        let on = hop_bar::check::check_text(
            &format!("set usage-api on\nset left \"{src}\"\n"),
            None,
            &[],
        );
        assert!(!on.to_text().contains(warn), "{src}: {}", on.to_text());
    }
    // a model from a default line counts too
    let r = hop_bar::check::check_text(
        "default usage model=opus\nset left \"#{usage}\"\n",
        None,
        &[],
    );
    assert!(r.to_text().contains(warn), "{}", r.to_text());
}

#[test]
fn off_renders_nothing_and_starts_nothing() {
    let t = Tmp::new();
    let cache = Cache::with_exe(t.path(), "/nonexistent/bar".into());
    let at = now();
    cache.store(&key(), &sample(at).to_json()).unwrap();
    let cfg = Config::default();
    assert_eq!(go(&cache, &cfg, "extra", &Options::default(), at), None);
    let m = opts(&[("model", "opus")]);
    assert_eq!(go(&cache, &cfg, "usage", &m, at), None);
    assert!(!cache.lock_path(&key()).exists());
}

#[test]
fn per_model_weekly_window() {
    let t = Tmp::new();
    let cache = Cache::with_exe(t.path(), "/nonexistent/bar".into());
    let at = now();
    cache.store(&key(), &sample(at).to_json()).unwrap();
    let cfg = parse("set usage-api on\n").unwrap();
    let r = |pairs: &[(&str, &str)]| go(&cache, &cfg, "usage", &opts(pairs), at);
    // 81.5% left, rounded half up
    assert_eq!(
        r(&[("model", "opus"), ("show", "time,pct")]).as_deref(),
        Some("3d 82%")
    );
    assert_eq!(
        r(&[
            ("model", "Sonnet"),
            ("type", "7d"),
            ("show", "pct"),
            ("count", "used")
        ])
        .as_deref(),
        Some("12%")
    );
    assert!(r(&[("model", "opus")]).unwrap().starts_with("3d "));
    // weekly only; unknown model hidden
    assert_eq!(r(&[("model", "opus"), ("type", "5h")]), None);
    assert_eq!(r(&[("model", "fable")]), None);
    // stale numbers are not shown
    let late = at + usageapi::STALE + 1;
    let o = opts(&[("model", "opus")]);
    assert_eq!(go(&cache, &cfg, "usage", &o, late), None);
}

#[test]
fn per_model_rule_and_its_weekly_fallback() {
    let t = Tmp::new();
    let cache = Cache::with_exe(t.path(), "/nonexistent/bar".into());
    let at = now();
    cache.store(&key(), &sample(at).to_json()).unwrap();
    let render = |conf: &str| {
        let cfg = parse(conf).unwrap();
        let s = Session::default();
        let ctx = Ctx {
            session: &s,
            config: &cfg,
            now: at,
            runner: &ColdRunner,
            cache: Some(&cache),
            columns: None,
            plugin_dirs: &[],
            memo: Default::default(),
        };
        lookup("usage")
            .unwrap()
            .render(&ctx, &opts(&[("model", "opus"), ("show", "pct")]))
            .unwrap()
    };
    let base = render("set usage-api on\n");
    // a model rule that scores 82% left as bad changes the colour
    let model = render(
        "set usage-api on\nrule usage[type=7d;model=opus] early warn=95% bad=90% final=1h bad=30%\n",
    );
    assert_ne!(base, model);
    // the weekly rule applies when there is no model rule
    let weekly =
        render("set usage-api on\nrule usage[type=7d] early warn=95% bad=90% final=1h bad=30%\n");
    assert_eq!(weekly, model);
}

#[test]
fn extra_usage_components() {
    let t = Tmp::new();
    let cache = Cache::with_exe(t.path(), "/nonexistent/bar".into());
    let at = now();
    cache.store(&key(), &sample(at).to_json()).unwrap();
    let cfg = parse("set usage-api on\n").unwrap();
    let r = |show: &str| go(&cache, &cfg, "extra", &opts(&[("show", show)]), at);
    assert_eq!(
        go(&cache, &cfg, "extra", &Options::default(), at).as_deref(),
        Some("$12.34/$50.00")
    );
    assert_eq!(r("left").as_deref(), Some("$37.66 left"));
    assert_eq!(r("today").as_deref(), Some("$2.34 today"));
    assert_eq!(r("pct").as_deref(), Some("25%"));
    let o = opts(&[("show", "pct"), ("count", "left")]);
    assert_eq!(go(&cache, &cfg, "extra", &o, at).as_deref(), Some("75%"));
    // the rest of the limit over the days left in the month
    let (_, end) = hop_bar::tz::month_bounds(at, hop_bar::tz::Tz::Local);
    let days = ((end - at).max(1) + 86_399) / 86_400;
    let daily = hop_bar::placeholders::money::amount(3766 / days, "USD");
    assert_eq!(r("daily"), Some(format!("{daily}/d")));
    assert!(r("time").is_some_and(|s| !s.is_empty()));
    // a day not yet seen from its start shows no today figure
    let mut d = sample(at);
    d.base = None;
    cache.store(&key(), &d.to_json()).unwrap();
    assert_eq!(r("today"), None);
    assert_eq!(r("used,today").as_deref(), Some("$12.34"));
}

#[test]
fn extra_usage_turned_off_renders_nothing() {
    let t = Tmp::new();
    let cache = Cache::with_exe(t.path(), "/nonexistent/bar".into());
    let at = now();
    let mut d = sample(at);
    if let Some(e) = d.extra.as_mut() {
        e.enabled = false;
    }
    cache.store(&key(), &d.to_json()).unwrap();
    let cfg = parse("set usage-api on\n").unwrap();
    assert_eq!(go(&cache, &cfg, "extra", &Options::default(), at), None);
    let o = opts(&[("show", "used,limit,left,pct,bar")]);
    assert_eq!(go(&cache, &cfg, "extra", &o, at), None);
}

#[test]
fn spend_components_from_the_statusline_json() {
    let s = hop_bar::session::parse(&format!(
        r#"{{"rate_limits":{{"spend_limit":{{"used_percentage":62.8,"resets_at":{},"used_usd":314.12,"limit_usd":500,"period":"monthly"}}}}}}"#,
        1_000_000 + 10 * 86_400
    ));
    let cfg = Config::default();
    let ctx = Ctx {
        session: &s,
        config: &cfg,
        now: 1_000_000,
        runner: &ColdRunner,
        cache: None,
        columns: None,
        plugin_dirs: &[],
        memo: Default::default(),
    };
    let r = |show: &str| {
        lookup("spend")
            .unwrap()
            .render(&ctx, &opts(&[("show", show)]))
            .map(|x| strip_codes(&x))
    };
    assert_eq!(r("").as_deref(), Some("10d"));
    assert_eq!(r("time").as_deref(), Some("10d"));
    assert_eq!(r("used,limit").as_deref(), Some("$314.12/$500.00"));
    assert_eq!(r("left,daily").as_deref(), Some("$185.88 left $18.58/d"));
    assert_eq!(r("pct,time").as_deref(), Some("63% 10d"));
    // no today figure for a spend limit
    assert_eq!(r("today"), None);
}

#[test]
fn spend_limits_of_every_period() {
    let at = 1_000_000;
    let render = |limit: &str, show: &str| {
        let s = hop_bar::session::parse(&format!(r#"{{"rate_limits":{{"spend_limit":{limit}}}}}"#));
        let cfg = Config::default();
        let ctx = Ctx {
            session: &s,
            config: &cfg,
            now: at,
            runner: &ColdRunner,
            cache: None,
            columns: None,
            plugin_dirs: &[],
            memo: Default::default(),
        };
        lookup("spend")
            .unwrap()
            .render(&ctx, &opts(&[("show", show)]))
            .map(|x| strip_codes(&x))
    };
    let lim = |period: &str, secs: i64| {
        format!(
            r#"{{"used_percentage":40,"resets_at":{},"used_usd":4,"limit_usd":10{period}}}"#,
            at + secs
        )
    };
    let daily = lim(r#","period":"daily""#, 5 * 3600);
    assert_eq!(render(&daily, "").as_deref(), Some("5h0m"));
    assert_eq!(
        render(&daily, "used,limit").as_deref(),
        Some("$4.00/$10.00")
    );
    let weekly = lim(r#","period":"weekly""#, 3 * 86_400);
    assert_eq!(
        render(&weekly, "left,daily").as_deref(),
        Some("$6.00 left $2.00/d")
    );
    // the dollar fields and the period come later than the percentage
    let bare = format!(r#"{{"used_percentage":40,"resets_at":{}}}"#, at + 3600);
    assert_eq!(render(&bare, "").as_deref(), Some("1h0m"));
    // money components wait for `limit_usd`
    assert_eq!(render(&bare, "pct"), None);
    assert_eq!(render(&lim(r#","period":"yearly""#, 3600), ""), None);
}

#[test]
fn spend_pace_period_follows_the_limit() {
    use hop_bar::placeholders::spend::period_secs;
    use hop_bar::tz::days_from_civil;
    let day = 86_400;
    let mar1 = days_from_civil(2026, 3, 1) * day;
    assert_eq!(
        period_secs(Some("monthly"), mar1, mar1 - day),
        Some(28 * day)
    );
    let may15 = days_from_civil(2026, 5, 15) * day + 3600;
    assert_eq!(
        period_secs(Some("monthly"), may15, may15 - day),
        Some(30 * day)
    );
    // a reset on the 31st steps back to the end of a shorter month
    let mar31 = days_from_civil(2026, 3, 31) * day;
    assert_eq!(
        period_secs(Some("monthly"), mar31, mar31 - day),
        Some(31 * day)
    );
    assert_eq!(period_secs(Some("weekly"), 0, 0), Some(7 * day));
    assert_eq!(period_secs(Some("daily"), 0, 0), Some(day));
    // without a period: the shortest that fits the time left
    assert_eq!(period_secs(None, 10 * 3600, 0), Some(day));
    assert_eq!(period_secs(None, 3 * day, 0), Some(7 * day));
    assert_eq!(
        period_secs(None, 20 * day, 0),
        period_secs(Some("monthly"), 20 * day, 0)
    );
    assert_eq!(period_secs(Some("yearly"), 0, 0), None);
}

#[test]
fn parses_the_endpoint_json() {
    let (models, extra) = usageapi::parse(FIXTURE.as_bytes()).unwrap();
    let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
    // `limits` first, then the legacy buckets that have numbers
    assert_eq!(names, ["sonnet", "fable", "opus"]);
    assert_eq!(models[2].used, 18.5);
    assert_eq!(
        models[0].resets_at,
        hop_bar::transcript::parse_ts("2026-11-13T09:00:00Z").unwrap()
    );
    let e = extra.unwrap();
    assert_eq!((e.limit, e.used, e.currency.as_str()), (5000, 1234, "USD"));
    assert!((e.util - 24.68).abs() < 1e-9);
    // utilization computed when absent; no extra usage without a limit
    let (_, e) =
        usageapi::parse(br#"{"extra_usage":{"monthly_limit":200,"used_credits":50}}"#).unwrap();
    assert_eq!(e.unwrap().util, 25.0);
    let (m, e) = usageapi::parse(br#"{"extra_usage":null}"#).unwrap();
    assert!(m.is_empty() && e.is_none());
    assert!(usageapi::parse(b"[]").is_none());
}

#[test]
fn job_round_trips() {
    let req = Request {
        key: format!("usage-api:{}", common::abs("/x")),
        job: Job::UsageApi {
            dir: common::abs("/x/.claude"),
            hashed: true,
            prev: common::abs("/x/cache/abc"),
        },
        lock: "l".into(),
    };
    assert_eq!(Request::from_json(&req.to_json()).unwrap(), req);
    let rel = format!(
        r#"{{"key":"k","job":{{"kind":"usage-api","dir":"rel","prev":"{}"}},"lock":""}}"#,
        common::abs("/p")
    );
    assert!(Request::from_json(&rel).is_err());
}

// ---- the background job, end to end through `bar refresh` ----

// fakes `curl` and `security` on `PATH`: without `test-tools` the job
// runs the system tools, which would read the real keychain
#[cfg(all(unix, feature = "test-tools"))]
mod job {
    use super::*;
    use assert_cmd::Command;
    use std::path::Path;

    const TOKEN: &str = "sk-ant-oat01-SYNTHETIC-test-token-0000";

    fn exe(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
    }

    fn creds(exp_ms: i64) -> String {
        format!(r#"{{"claudeAiOauth":{{"accessToken":"{TOKEN}","expiresAt":{exp_ms}}}}}"#)
    }

    /// A fake `curl` logging argv, stdin and env, answering `status`.
    fn setup(
        t: &Tmp,
        status: &str,
        retry: &str,
    ) -> (Vec<(&'static str, std::ffi::OsString)>, std::path::PathBuf) {
        let (envs, cwd) = common::isolation(t.path());
        let d = t.path();
        t.write("resp/body.json", FIXTURE);
        t.write("resp/status", status);
        t.write("resp/retry", retry);
        exe(
            &d.join("fake-bin/curl"),
            &format!(
                "#!/bin/sh\nD='{}'\nprintf '%s\\n' \"$@\" > \"$D/curl.argv\"\ncat > \"$D/curl.stdin\"\nenv > \"$D/curl.env\"\ncat \"$D/resp/body.json\"\nprintf '\\n%s\\n%s' \"$(cat \"$D/resp/status\")\" \"$(cat \"$D/resp/retry\")\"\n",
                d.display()
            ),
        );
        (envs, cwd)
    }

    fn refresh(
        t: &Tmp,
        envs: &[(&'static str, std::ffi::OsString)],
        cwd: &Path,
    ) -> (Cache, String) {
        let cache = Cache::new(&t.path().join("state"));
        let dir = t.path().join("claude");
        let key = usageapi::cache_key(&dir.to_string_lossy());
        let req = Request {
            key: key.clone(),
            job: Job::UsageApi {
                dir: dir.to_string_lossy().into(),
                hashed: true,
                prev: cache.path_for(&key).to_string_lossy().into(),
            },
            lock: String::new(),
        };
        let o = Command::cargo_bin("bar")
            .unwrap()
            .args(["refresh", &req.to_json()])
            .envs(envs.iter().cloned())
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(o.status.success(), "{o:?}");
        (cache, key)
    }

    /// `refresh`, with the clock before and after it. The job reads its
    /// clock in between, so a `retry_at` is exactly `wait` past a second in
    /// `before..=after`, however long the run took.
    fn timed(
        t: &Tmp,
        envs: &[(&'static str, std::ffi::OsString)],
        cwd: &Path,
    ) -> (Cache, String, (i64, i64)) {
        let before = now();
        let (cache, key) = refresh(t, envs, cwd);
        (cache, key, (before, now()))
    }

    fn assert_retry(retry_at: i64, (before, after): (i64, i64), wait: i64, what: &str) {
        let ok = (before + wait..=after + wait).contains(&retry_at);
        assert!(
            ok,
            "{what}: {retry_at} not in {}..={}",
            before + wait,
            after + wait
        );
    }

    fn token_nowhere(t: &Tmp, cache: &Cache, key: &str) {
        let d = t.path();
        let argv = std::fs::read_to_string(d.join("curl.argv")).unwrap();
        let env = std::fs::read_to_string(d.join("curl.env")).unwrap();
        let stdin = std::fs::read_to_string(d.join("curl.stdin")).unwrap();
        assert!(!argv.contains(TOKEN) && !env.contains(TOKEN));
        assert_eq!(
            stdin,
            format!("header = \"Authorization: Bearer {TOKEN}\"\n")
        );
        assert!(
            argv.lines()
                .any(|l| l == "anthropic-beta: oauth-2025-04-20"),
            "{argv}"
        );
        assert!(
            argv.ends_with("--\nhttps://api.anthropic.com/api/oauth/usage\n"),
            "{argv}"
        );
        assert!(argv.starts_with("-q\n"));
        let stored = std::fs::read_to_string(cache.path_for(key)).unwrap_or_default();
        assert!(!stored.contains(TOKEN));
        // derived numbers only: no identifiers from the response
        assert!(
            !stored.contains("account_uuid") && !stored.contains("00000000"),
            "{stored}"
        );
    }

    #[test]
    fn fetches_with_the_credentials_file_and_caches_numbers_only() {
        let t = Tmp::new();
        let (envs, cwd) = setup(&t, "200", "");
        t.write("claude/.credentials.json", &creds((now() + 3600) * 1000));
        let (cache, key) = refresh(&t, &envs, &cwd);
        token_nowhere(&t, &cache, &key);
        let d = Data::from_json(&cache.read_fresh(&key, 3600).unwrap()).unwrap();
        assert_eq!(d.models.len(), 3);
        assert_eq!(d.extra.unwrap().used, 1234);
        assert_eq!(d.retry_at, 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_the_keychain_entry_claude_code_names() {
        let t = Tmp::new();
        let (envs, cwd) = setup(&t, "200", "");
        let d = t.path();
        t.write("security.out", &creds((now() + 3600) * 1000));
        exe(
            &d.join("fake-bin/security"),
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{0}/security.argv'\ncat '{0}/security.out'\n",
                d.display()
            ),
        );
        let (cache, key) = refresh(&t, &envs, &cwd);
        token_nowhere(&t, &cache, &key);
        let argv = std::fs::read_to_string(d.join("security.argv")).unwrap();
        let dir = d.join("claude");
        let svc = hop_bar::usageapi::creds::service(&dir.to_string_lossy(), true);
        assert!(
            argv.starts_with(&format!("find-generic-password\n-s\n{svc}\n-a\n")),
            "{argv}"
        );
        assert!(argv.ends_with("-w\n"));
        assert!(cache.read_fresh(&key, 3600).is_some());
    }

    /// Order: the keychain entry for the user's account, the credentials
    /// file, then the keychain entry of any account.
    #[cfg(target_os = "macos")]
    #[test]
    fn credentials_file_comes_before_any_account() {
        let t = Tmp::new();
        let (envs, cwd) = setup(&t, "200", "");
        let d = t.path();
        let other = "sk-ant-oat01-OTHER-account-token-0000";
        t.write(
            "security.out",
            &creds((now() + 3600) * 1000).replace(TOKEN, other),
        );
        // answers only the lookup without an account
        exe(
            &d.join("fake-bin/security"),
            &format!(
                "#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = -a ] && exit 44; done\ncat '{0}/security.out'\n",
                d.display()
            ),
        );
        t.write("claude/.credentials.json", &creds((now() + 3600) * 1000));
        let (cache, key) = refresh(&t, &envs, &cwd);
        token_nowhere(&t, &cache, &key);
        // without the file, the any-account entry is used
        std::fs::remove_file(d.join("claude/.credentials.json")).unwrap();
        std::fs::remove_file(cache.path_for(&key)).unwrap();
        refresh(&t, &envs, &cwd);
        let stdin = std::fs::read_to_string(d.join("curl.stdin")).unwrap();
        assert!(stdin.contains(other), "{stdin}");
    }

    /// A keychain read that hangs (a prompt nobody answers) waits an hour.
    #[cfg(target_os = "macos")]
    #[test]
    fn blocked_keychain_waits_an_hour() {
        let t = Tmp::new();
        let (mut envs, cwd) = setup(&t, "200", "");
        envs.push(("BAR_REFRESH_TIMEOUT", "2".into()));
        let d = t.path();
        exe(&d.join("fake-bin/security"), "#!/bin/sh\nsleep 5\n");
        let (cache, key, span) = timed(&t, &envs, &cwd);
        assert!(!d.join("curl.argv").exists());
        let v = Data::from_json(&cache.read_fresh(&key, 3600).unwrap()).unwrap();
        assert_retry(v.retry_at, span, 3600, "blocked keychain");
    }

    /// The keychain read and the request share the refresh time limit, so
    /// a slow keychain leaves curl only what is left of it.
    #[cfg(target_os = "macos")]
    #[test]
    fn keychain_time_counts_against_the_request_limit() {
        let t = Tmp::new();
        let (mut envs, cwd) = setup(&t, "200", "");
        envs.push(("BAR_REFRESH_TIMEOUT", "8".into()));
        let d = t.path();
        t.write("security.out", &creds((now() + 3600) * 1000));
        exe(
            &d.join("fake-bin/security"),
            &format!("#!/bin/sh\nsleep 2\ncat '{0}/security.out'\n", d.display()),
        );
        refresh(&t, &envs, &cwd);
        let argv = std::fs::read_to_string(d.join("curl.argv")).unwrap();
        let lines: Vec<&str> = argv.lines().collect();
        let at = lines.iter().position(|l| *l == "--max-time").unwrap();
        let max: u64 = lines[at + 1].parse().unwrap();
        // 8 s less the 2 s the keychain took (slack for a loaded machine)
        assert!((1..=6).contains(&max), "--max-time {max}");
    }

    #[test]
    fn rate_limit_keeps_the_numbers_and_waits() {
        let t = Tmp::new();
        let (envs, cwd) = setup(&t, "429", "120");
        t.write("claude/.credentials.json", &creds((now() + 3600) * 1000));
        let cache = Cache::new(&t.path().join("state"));
        let key = usageapi::cache_key(&t.path().join("claude").to_string_lossy());
        let old = sample(now() - 600);
        cache.store(&key, &old.to_json()).unwrap();
        let (_, _, span) = timed(&t, &envs, &cwd);
        let d = Data::from_json(&cache.read_fresh(&key, 3600).unwrap()).unwrap();
        assert_eq!(d.models, old.models);
        assert_retry(d.retry_at, span, 120, "Retry-After");
        // while waiting, the job does not call curl again
        std::fs::remove_file(t.path().join("curl.argv")).unwrap();
        refresh(&t, &envs, &cwd);
        assert!(!t.path().join("curl.argv").exists());
    }

    #[test]
    fn expired_or_missing_token_never_calls_curl() {
        let t = Tmp::new();
        let (envs, cwd) = setup(&t, "200", "");
        let (cache, key) = refresh(&t, &envs, &cwd);
        assert!(!t.path().join("curl.argv").exists());
        assert!(cache.read_fresh(&key, 3600).is_none());
        assert!(cache.fail_path(&key).exists());
        t.write("claude/.credentials.json", &creds((now() - 60) * 1000));
        refresh(&t, &envs, &cwd);
        assert!(!t.path().join("curl.argv").exists());
    }

    #[test]
    fn failures_keep_the_last_value_and_back_off() {
        let t = Tmp::new();
        let (envs, cwd) = setup(&t, "500", "");
        t.write("claude/.credentials.json", &creds((now() + 3600) * 1000));
        let cache = Cache::new(&t.path().join("state"));
        let key = usageapi::cache_key(&t.path().join("claude").to_string_lossy());
        let old = sample(now() - 600);
        cache.store(&key, &old.to_json()).unwrap();
        // 5 minutes after the first failure, doubling, at most an hour
        for (fails, wait) in [
            (0, 300),
            (1, 600),
            (2, 1200),
            (3, 2400),
            (4, 3600),
            (9, 3600),
        ] {
            let mut prev = old.clone();
            prev.fails = fails;
            cache.store(&key, &prev.to_json()).unwrap();
            let (_, _, span) = timed(&t, &envs, &cwd);
            let d = Data::from_json(&cache.read_fresh(&key, 3600).unwrap()).unwrap();
            assert_eq!(
                (&d.models, &d.extra, d.at),
                (&old.models, &old.extra, old.at)
            );
            assert_eq!(d.fails, fails + 1);
            assert_retry(d.retry_at, span, wait, &format!("after {fails}"));
        }
        // a curl failure backs off the same way
        t.write("resp/status", "not-a-status");
        cache.store(&key, &old.to_json()).unwrap();
        let (_, _, span) = timed(&t, &envs, &cwd);
        let d = Data::from_json(&cache.read_fresh(&key, 3600).unwrap()).unwrap();
        assert_eq!(d.fails, 1);
        assert_retry(d.retry_at, span, 300, "curl failure");
        // success clears the count
        t.write("resp/status", "200");
        let mut prev = old.clone();
        prev.fails = 3;
        cache.store(&key, &prev.to_json()).unwrap();
        refresh(&t, &envs, &cwd);
        let d = Data::from_json(&cache.read_fresh(&key, 3600).unwrap()).unwrap();
        assert_eq!((d.fails, d.retry_at), (0, 0));
    }
}
