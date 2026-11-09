//! Opt-in Claude service status: `#{status}`, `set service-status`, the
//! background job with a fake `curl` and synthetic Statuspage JSON.

mod common;
#[path = "common/render_fx.rs"]
mod render_fx;

use hop_bar::cache::{Cache, Job, Request};
use hop_bar::conf::{parse, Config};
use hop_bar::options::Options;
use hop_bar::placeholders::status::Status;
use hop_bar::placeholders::{Ctx, Placeholder};
use hop_bar::render::ColdRunner;
use hop_bar::session::Session;
use hop_bar::status::{self, Impact, Snapshot};
use render_fx::Tmp;

const NOW: i64 = 1_800_000_000;
const H: i64 = 3600;

fn iso(t: i64) -> String {
    // civil date from unix seconds (UTC)
    let (days, secs) = (t.div_euclid(86400), t.rem_euclid(86400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

fn summary(indicator: &str, comps: &[(&str, &str)], incidents: &str) -> String {
    let c: Vec<String> = comps
        .iter()
        .map(|(n, s)| format!(r#"{{"name":"{n}","status":"{s}","group":false}}"#))
        .collect();
    format!(
        r#"{{"page":{{"url":"https://status.claude.com"}},"components":[{}],"incidents":[{incidents}],"status":{{"indicator":"{indicator}"}}}}"#,
        c.join(",")
    )
}

fn incident(name: &str, impact: &str, start: i64, end: Option<i64>, comps: &[&str]) -> String {
    let c: Vec<String> = comps
        .iter()
        .map(|n| format!(r#"{{"name":"{n}"}}"#))
        .collect();
    let end = end.map_or("null".to_string(), |e| format!("\"{}\"", iso(e)));
    format!(
        r#"{{"name":"{name}","impact":"{impact}","started_at":"{}","resolved_at":{end},"components":[{}]}}"#,
        iso(start),
        c.join(",")
    )
}

const COMPS: [(&str, &str); 3] = [
    ("claude.ai", "degraded_performance"),
    ("Claude API (api.anthropic.com)", "operational"),
    ("Claude Code", "operational"),
];

fn snap(indicator: &str, comps: &[(&str, &str)], incidents: &[String]) -> Snapshot {
    let raw = status::compact(
        summary(indicator, comps, &incidents.join(",")).as_bytes(),
        None,
        NOW,
    )
    .unwrap();
    Snapshot::parse(&raw).unwrap()
}

fn go(cache: &Cache, cfg: &Config, opts: &str) -> Option<String> {
    let s = Session::default();
    let ctx = Ctx {
        session: &s,
        config: cfg,
        now: NOW,
        runner: &ColdRunner,
        cache: Some(cache),
        columns: None,
        plugin_dirs: &[],
    };
    let mut o = Options::default();
    for kv in opts.split(';').filter(|k| !k.is_empty()) {
        let (k, v) = kv.split_once('=').unwrap();
        o.set(k, v);
    }
    Status.render(&ctx, &o)
}

fn plain(s: Option<String>) -> Option<String> {
    s.map(|s| hop_bar::style::strip_codes(&s))
}

fn on() -> Config {
    parse("set service-status on\n").unwrap()
}

fn stored(t: &Tmp, strip: bool, body: &str) -> Cache {
    let cache = Cache::with_exe(t.path(), "/nonexistent/bar".into());
    cache.store(status::cache_key(strip), body).unwrap();
    cache
}

#[test]
fn off_by_default_and_touches_nothing() {
    let t = Tmp::new();
    let cache = stored(&t, false, "{}");
    assert!(!Config::default().service_status);
    assert_eq!(go(&cache, &Config::default(), ""), None);
    assert!(!cache.lock_path(status::cache_key(false)).exists());
    assert!(!cache.lock_path(status::cache_key(true)).exists());
}

#[test]
fn nothing_when_operational_a_check_on_request() {
    let t = Tmp::new();
    let ok: Vec<_> = COMPS.iter().map(|(n, _)| (*n, "operational")).collect();
    let body = status::compact(summary("none", &ok, "").as_bytes(), None, NOW).unwrap();
    let cache = stored(&t, false, &body);
    assert_eq!(go(&cache, &on(), ""), None);
    assert_eq!(
        plain(go(&cache, &on(), "ok=1")).as_deref(),
        Some("status ✓")
    );
}

#[test]
fn shows_the_indicator_coloured_and_linked() {
    let t = Tmp::new();
    let cfg = on();
    for (ind, color) in [("minor", cfg.colors.warn), ("major", cfg.colors.bad)] {
        let body = status::compact(summary(ind, &COMPS, "").as_bytes(), None, NOW).unwrap();
        let cache = stored(&t, false, &body);
        let out = go(&cache, &cfg, "").unwrap();
        assert_eq!(hop_bar::style::strip_codes(&out), format!("status {ind}"));
        assert!(out.contains(&format!("38;5;{color}")), "{out:?}");
        assert!(
            out.contains("\x1b]8;;https://status.claude.com\x1b\\"),
            "{out:?}"
        );
        let plain_out = go(&cache, &cfg, "link=false").unwrap();
        assert!(!plain_out.contains("\x1b]8"), "{plain_out:?}");
    }
}

#[test]
fn component_filter_ignores_the_others() {
    let t = Tmp::new();
    let body = status::compact(summary("minor", &COMPS, "").as_bytes(), None, NOW).unwrap();
    let cache = stored(&t, false, &body);
    assert_eq!(plain(go(&cache, &on(), "components=code|API")), None);
    assert_eq!(
        plain(go(&cache, &on(), "components=code|CLAUDE.AI")).as_deref(),
        Some("status minor")
    );
}

#[test]
fn shows_the_active_incident_truncated() {
    let t = Tmp::new();
    let inc = [
        incident(
            "Old trouble",
            "major",
            NOW - 9 * H,
            Some(NOW - 8 * H),
            &["claude.ai"],
        ),
        incident(
            "Elevated errors on claude.ai",
            "minor",
            NOW - H,
            None,
            &["claude.ai"],
        ),
        incident("Slow Claude Code", "minor", NOW - H, None, &["Claude Code"]),
    ];
    let s = snap("minor", &COMPS, &inc);
    assert_eq!(s.active(&[]).unwrap().title, "Slow Claude Code");
    let body = status::compact(
        summary("minor", &COMPS, &inc.join(",")).as_bytes(),
        None,
        NOW,
    )
    .unwrap();
    let cache = stored(&t, false, &body);
    assert_eq!(
        plain(go(&cache, &on(), "show=text;components=claude.ai")).as_deref(),
        Some("Elevated errors on claude.ai")
    );
    assert_eq!(
        plain(go(
            &cache,
            &on(),
            "show=state,text;components=claude.ai;max=12"
        ))
        .as_deref(),
        Some("status minor Elevated er…")
    );
    // the incident text cannot inject terminal codes
    let evil = [incident("a\\u001b[31mred", "minor", NOW - H, None, &[])];
    let s = snap("minor", &COMPS, &evil);
    assert_eq!(s.active(&[]).unwrap().title, "a\u{1b}[31mred");
    let cache = stored(
        &t,
        false,
        &status::compact(
            summary("minor", &COMPS, &evil.join(",")).as_bytes(),
            None,
            NOW,
        )
        .unwrap(),
    );
    assert!(!go(&cache, &on(), "show=text").unwrap().contains("\x1b[31m"));
}

#[test]
fn strip_cells_take_the_worst_overlapping_impact() {
    let inc = [
        // 8 cells of 6 h end at NOW; the last covers NOW-6h..NOW
        incident("a", "minor", NOW - 40 * H, Some(NOW - 38 * H), &[]),
        incident(
            "b",
            "critical",
            NOW - 20 * H,
            Some(NOW - 19 * H),
            &["claude.ai"],
        ),
        incident("c", "major", NOW - 2 * H, None, &["Claude Code"]),
        incident("none", "none", NOW - 30 * H, Some(NOW - 29 * H), &[]),
    ];
    let s = snap("minor", &COMPS, &inc);
    use Impact as I;
    assert_eq!(
        s.strip(&[], NOW, 6),
        [
            I::None,
            I::Minor,
            I::None,
            I::None,
            I::Critical,
            I::None,
            I::None,
            I::Major
        ]
    );
    let code = vec!["code".to_string()];
    let cells = s.strip(&code, NOW, 6);
    assert_eq!(cells[7], I::Major);
    assert_eq!(cells[4], I::None, "claude.ai incident filtered out");
    assert_eq!(cells[1], I::Minor, "an incident with all components stays");
    // other cell sizes: ceil(48 / h) cells
    assert_eq!(s.strip(&[], NOW, 4).len(), 12);
    assert_eq!(s.strip(&[], NOW, 5).len(), 10);
    assert_eq!(s.strip(&[], NOW, 48).len(), 1);
}

#[test]
fn strip_renders_and_hides_when_clear() {
    let t = Tmp::new();
    let cfg = on();
    let inc = [incident("c", "major", NOW - 2 * H, Some(NOW - H), &[])];
    let body = status::compact(
        summary("none", &COMPS, &inc.join(",")).as_bytes(),
        Some(format!(r#"{{"incidents":[{}]}}"#, inc.join(",")).as_bytes()),
        NOW,
    )
    .unwrap();
    let cache = stored(&t, true, &body);
    let out = go(&cache, &cfg, "show=strip").unwrap();
    assert_eq!(hop_bar::style::strip_codes(&out), "▁▁▁▁▁▁▁█");
    assert!(out.contains(&format!("38;5;{}", cfg.colors.bad)));
    let clear = status::compact(
        summary("none", &COMPS, "").as_bytes(),
        Some(br#"{"incidents":[]}"#),
        NOW,
    )
    .unwrap();
    let cache = stored(&t, true, &clear);
    assert_eq!(go(&cache, &cfg, "show=strip"), None);
    assert_eq!(
        plain(go(&cache, &cfg, "show=strip;ok=1;hours=12")).as_deref(),
        Some("▁▁▁▁")
    );
}

#[test]
fn compact_drops_old_incidents_and_rejects_bad_json() {
    let old = incident("old", "major", NOW - 200 * H, Some(NOW - 199 * H), &[]);
    let s = snap("none", &COMPS, &[old]);
    assert!(s.incidents.is_empty());
    assert!(status::compact(b"nope", None, NOW).is_none());
    assert!(status::compact(summary("none", &[], "").as_bytes(), Some(b"{}"), NOW).is_none());
    assert_eq!(Snapshot::parse(""), None);
}

#[test]
fn config_values_guard_and_check() {
    assert!(!parse("set service-status off\n").unwrap().service_status);
    let e = parse("set service-status yes\n").unwrap_err();
    assert!(e[0].message.contains("service-status"), "{e:?}");
    for l in [
        "set left \"#{status}\"\n",
        "set left \"#{status[show=state,text,strip,components=code|api,ok=1,hours=4,max=30,link=false]}\"\n",
    ] {
        let r = hop_bar::check::check_text(l, None, &[]);
        assert!(!r.has_errors(), "{}", r.to_text());
    }
    for bad in ["#{status[show=x]}", "#{status[hours=0]}", "#{status[ok=2]}"] {
        let r = hop_bar::check::check_text(&format!("set left \"{bad}\"\n"), None, &[]);
        assert!(r.has_errors(), "{bad}");
    }
}

#[test]
fn job_round_trips() {
    for strip in [false, true] {
        let req = Request {
            key: status::cache_key(strip).into(),
            job: Job::Status { strip },
            lock: "l".into(),
        };
        assert_eq!(Request::from_json(&req.to_json()).unwrap(), req);
    }
}

#[cfg(unix)]
mod fake_curl {
    use super::*;
    use assert_cmd::Command;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::time::{Duration, Instant};

    fn fx(t: &Tmp, curl_body: &str) -> Command {
        let (envs, cwd) = common::isolation(t.path());
        let fake = t.path().join("fake-bin/curl");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\necho \"$@\" >> \"{}/argv\"\n{curl_body}\n",
                t.path().display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut c = Command::cargo_bin("bar").unwrap();
        c.envs(envs).current_dir(cwd);
        c
    }

    fn wait_for(p: &Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !p.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// A curl that answers by URL from files in the temp dir.
    fn serve(t: &Tmp) -> String {
        let inc = incident("Elevated errors", "minor", NOW - H, None, &["claude.ai"]);
        t.write("summary.json", &summary("minor", &COMPS, &inc));
        t.write("incidents.json", &format!(r#"{{"incidents":[{inc}]}}"#));
        let d = t.path().display();
        format!(
            "for a in \"$@\"; do u=$a; done\ncase $u in\n*summary.json) cat \"{d}/summary.json\";;\n*incidents.json) cat \"{d}/incidents.json\";;\n*) exit 22;;\nesac"
        )
    }

    #[test]
    fn off_runs_no_curl() {
        let t = Tmp::new();
        let body = serve(&t);
        t.write("xdg/bar/bar.conf", "set left \"#{status}\"\n");
        fx(&t, &body)
            .arg("render")
            .write_stdin("{}")
            .assert()
            .success();
        std::thread::sleep(Duration::from_millis(300));
        assert!(!t.path().join("argv").exists(), "curl ran while off");
    }

    #[test]
    fn render_starts_a_job_that_caches_the_summary() {
        let t = Tmp::new();
        let body = serve(&t);
        t.write(
            "xdg/bar/bar.conf",
            "set service-status on\nset left \"#{status}\"\n",
        );
        fx(&t, &body)
            .arg("render")
            .write_stdin("{}")
            .assert()
            .success();
        let cache = Cache::new(&t.path().join("state"));
        let path = cache.path_for(status::cache_key(false));
        wait_for(&path);
        let s = Snapshot::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(s.level(&[]), Impact::Minor);
        assert_eq!(s.active(&[]).unwrap().title, "Elevated errors");
        let argv = std::fs::read_to_string(t.path().join("argv")).unwrap();
        assert!(argv.contains("https://status.claude.com/api/v2/summary.json"));
        assert!(!argv.contains("incidents.json"), "history not requested");
        assert!(argv.contains("--proto =https"));
        // the next render shows it
        let o = fx(&t, &body)
            .arg("render")
            .write_stdin("{}")
            .output()
            .unwrap();
        let out = hop_bar::style::strip_codes(&String::from_utf8_lossy(&o.stdout));
        assert!(out.contains("status minor"), "{out}");
    }

    #[test]
    fn strip_reads_the_incidents_too() {
        let t = Tmp::new();
        let body = serve(&t);
        t.write(
            "xdg/bar/bar.conf",
            "set service-status on\nset left \"#{status[show=strip]}\"\n",
        );
        fx(&t, &body)
            .arg("render")
            .write_stdin("{}")
            .assert()
            .success();
        let path = Cache::new(&t.path().join("state")).path_for(status::cache_key(true));
        wait_for(&path);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("Elevated errors"));
        let argv = std::fs::read_to_string(t.path().join("argv")).unwrap();
        assert!(argv.contains("api/v2/incidents.json"), "{argv}");
    }

    #[test]
    fn a_failed_fetch_backs_off_and_shows_nothing() {
        let t = Tmp::new();
        t.write(
            "xdg/bar/bar.conf",
            "set service-status on\nset left \"#{status}\"\n",
        );
        let o = fx(&t, "exit 7")
            .arg("render")
            .write_stdin("{}")
            .output()
            .unwrap();
        assert!(o.status.success());
        let cache = Cache::new(&t.path().join("state"));
        let fail = cache.fail_path(status::cache_key(false));
        wait_for(&fail);
        assert!(fail.exists(), "failure marker");
        assert!(!cache.path_for(status::cache_key(false)).exists());
    }
}

#[test]
fn incident_times_with_a_zone_offset() {
    // Statuspage writes times with the page's zone offset
    let utc = iso(NOW - 2 * H).replace(".000Z", "");
    let (date, time) = utc.split_once('T').unwrap();
    let hh: i64 = time[..2].parse().unwrap();
    let shifted = format!("{date}T{:02}{}.000-00:00", hh, &time[2..]);
    let east = format!("{date}T{:02}{}.000+02:00", (hh + 2) % 24, &time[2..]);
    for start in [shifted, east] {
        if start.contains("+02:00") && hh + 2 >= 24 {
            continue;
        }
        let inc = format!(
            r#"{{"name":"x","impact":"minor","started_at":"{start}","resolved_at":null,"components":[]}}"#
        );
        let s = snap("minor", &COMPS, &[inc]);
        assert_eq!(s.incidents[0].start, NOW - 2 * H, "{start}");
    }
}
