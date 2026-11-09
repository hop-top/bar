//! Opt-in usage API: `set usage-api`, `usage[model=...]`, `extra`, and the
//! background job against a fake `curl` and `security`. No network.

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use assert_cmd::Command;
use hop_bar::cache::{Cache, Job, Request};
use hop_bar::conf::{parse, Config};
use hop_bar::options::Options;
use hop_bar::placeholders::{lookup, Ctx};
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use hop_bar::style::strip_codes;
use hop_bar::usageapi::{self, Data, Extra, Model};
use render_fx::Tmp;
use std::path::Path;

const FIXTURE: &str = include_str!("fixtures/usage-api/usage.json");
const TOKEN: &str = "sk-ant-oat01-SYNTHETIC-test-token-0000";

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
        "#{usage[type=7d,model=sonnet,show=time,pct]}",
        "#{extra}",
        "#{extra[show=used,limit,left,today,daily,pct,bar,gauge,time,count=left,width=8]}",
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
        };
        lookup("usage")
            .unwrap()
            .render(&ctx, &opts(&[("model", "opus"), ("show", "pct")]))
            .unwrap()
    };
    let base = render("set usage-api on\n");
    // a model rule that scores 82% left as bad changes the colour
    let model = render(
        "set usage-api on\nrule usage[type=7d,model=opus] early warn=95% bad=90% final=1h bad=30%\n",
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
        key: "usage-api:/x".into(),
        job: Job::UsageApi {
            dir: "/x/.claude".into(),
            hashed: true,
            prev: "/x/cache/abc".into(),
        },
        lock: "l".into(),
    };
    assert_eq!(Request::from_json(&req.to_json()).unwrap(), req);
    let rel = r#"{"key":"k","job":{"kind":"usage-api","dir":"rel","prev":"/p"},"lock":""}"#;
    assert!(Request::from_json(rel).is_err());
}

// ---- the background job, end to end through `bar refresh` ----

#[cfg(unix)]
mod job {
    use super::*;

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

    #[test]
    fn rate_limit_keeps_the_numbers_and_waits() {
        let t = Tmp::new();
        let (envs, cwd) = setup(&t, "429", "120");
        t.write("claude/.credentials.json", &creds((now() + 3600) * 1000));
        let cache = Cache::new(&t.path().join("state"));
        let key = usageapi::cache_key(&t.path().join("claude").to_string_lossy());
        let old = sample(now() - 600);
        cache.store(&key, &old.to_json()).unwrap();
        refresh(&t, &envs, &cwd);
        let d = Data::from_json(&cache.read_fresh(&key, 3600).unwrap()).unwrap();
        assert_eq!(d.models, old.models);
        let wait = d.retry_at - now();
        assert!((100..=121).contains(&wait), "{wait}");
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
    fn server_error_keeps_the_last_value() {
        let t = Tmp::new();
        let (envs, cwd) = setup(&t, "500", "");
        t.write("claude/.credentials.json", &creds((now() + 3600) * 1000));
        let cache = Cache::new(&t.path().join("state"));
        let key = usageapi::cache_key(&t.path().join("claude").to_string_lossy());
        let old = sample(now() - 600);
        cache.store(&key, &old.to_json()).unwrap();
        refresh(&t, &envs, &cwd);
        assert_eq!(
            Data::from_json(&cache.read_fresh(&key, 3600).unwrap()),
            Some(old)
        );
        assert!(cache.fail_path(&key).exists());
    }
}
