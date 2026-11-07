use hop_bar::conf::Config;
use hop_bar::options::Options;
use hop_bar::pace::{builtin_rule, score};
use hop_bar::placeholders::usage::{pct_left, render_with, time_left};
use hop_bar::placeholders::Ctx;
use hop_bar::render::ColdRunner;
use hop_bar::session::{Session, Window};
use hop_bar::style::ramp;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);
const NOW: i64 = 1_000_000;

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        let p = std::env::temp_dir().join(format!(
            "bar-usage-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('[') => {
                for n in it.by_ref() {
                    if ('\x40'..='\x7e').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(n) = it.next() {
                    if n == '\x1b' {
                        it.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Colour codes of the filled cells, in order.
fn cell_colors(raw: &str) -> Vec<u8> {
    raw.split("\x1b[38;5;")
        .skip(1)
        .filter_map(|p| {
            let (code, rest) = p.split_once('m')?;
            rest.starts_with('█').then(|| code.parse().unwrap())
        })
        .collect()
}

fn opts(pairs: &[(&str, &str)]) -> Options {
    let mut o = Options::default();
    for (k, v) in pairs {
        o.set(k, v);
    }
    o
}

fn session(five_used: f64, five_secs: i64, seven_used: f64, seven_secs: i64) -> Session {
    Session {
        five_hour: Some(Window {
            used_percentage: five_used,
            resets_at: NOW + five_secs,
        }),
        seven_day: Some(Window {
            used_percentage: seven_used,
            resets_at: NOW + seven_secs,
        }),
        ..Session::default()
    }
}

fn go(s: &Session, o: &[(&str, &str)], state: &Path) -> Option<String> {
    let cfg = Config::default();
    let runner = ColdRunner;
    let ctx = Ctx {
        session: s,
        config: &cfg,
        now: NOW,
        runner: &runner,
        cache: None,
        columns: None,
        plugin_dirs: &[],
    };
    render_with(&ctx, &opts(o), state)
}

fn fresh() -> Session {
    session(4.0, 18000, 0.0, 604800)
}

#[test]
fn view_bar_has_width_cells() {
    let t = Tmp::new();
    for w in [3usize, 5, 8] {
        let out = go(
            &fresh(),
            &[("type", "5h"), ("width", &w.to_string())],
            t.path(),
        )
        .unwrap();
        let cells = strip(&out).chars().filter(|c| "█░".contains(*c)).count();
        assert_eq!(cells, w);
    }
    let out = go(&fresh(), &[("type", "5h")], t.path()).unwrap();
    assert_eq!(strip(&out).chars().filter(|c| "█░".contains(*c)).count(), 5);
}

#[test]
fn bar_cells_share_exact_value_colour() {
    let t = Tmp::new();
    for used in [4.0, 13.0, 35.0, 60.0, 85.0] {
        let s = session(used, 18000, 0.0, 604800);
        let left = pct_left(used);
        let want = ramp(
            score(left, Some(18000), 5 * 3600, &builtin_rule("5h")),
            &Config::default().ramp,
        );
        let colors = cell_colors(&go(&s, &[("type", "5h")], t.path()).unwrap());
        assert!(!colors.is_empty());
        assert!(
            colors.iter().all(|c| *c == want),
            "{used}: {colors:?} != {want}"
        );
    }
}

#[test]
fn final_stretch_bar_single_colour() {
    let t = Tmp::new();
    let s = session(43.0, 18000, 43.0, 12 * 3600 + 50 * 60);
    let colors = cell_colors(&go(&s, &[("type", "7d")], t.path()).unwrap());
    assert!(!colors.is_empty());
    assert!(colors.iter().all(|c| *c == 196), "{colors:?}");
}

#[test]
fn empty_cells_dim() {
    let t = Tmp::new();
    let s = session(60.0, 18000, 0.0, 604800);
    let out = go(&s, &[("type", "5h")], t.path()).unwrap();
    assert!(out.contains("\x1b[38;5;250m░░░\x1b[39m"), "{out:?}");
}

#[test]
fn view_pct_matches_bar_colour() {
    let t = Tmp::new();
    let s = session(13.0, 18000, 0.0, 604800);
    let bar = go(&s, &[("type", "5h")], t.path()).unwrap();
    let pct = go(&s, &[("type", "5h"), ("view", "pct")], t.path()).unwrap();
    let c = cell_colors(&bar)[0];
    assert!(pct.contains(&format!("\x1b[38;5;{c}m87%")), "{pct:?}");
}

#[test]
fn view_pct() {
    let t = Tmp::new();
    let s = session(13.0, 2 * 3600 + 13 * 60, 0.0, 604800);
    let out = go(&s, &[("type", "5h"), ("view", "pct")], t.path()).unwrap();
    assert_eq!(strip(&out), "2h13m 87%");
}

#[test]
fn view_time_only() {
    let t = Tmp::new();
    let s = session(13.0, 2 * 3600 + 13 * 60, 0.0, 604800);
    let out = go(&s, &[("type", "5h"), ("view", "time")], t.path()).unwrap();
    assert_eq!(strip(&out), "2h13m");
}

#[test]
fn time_left_formats() {
    assert_eq!(time_left(3 * 86400 + 4 * 3600 + 7), "3d4h");
    assert_eq!(time_left(3 * 86400 + 59), "3d");
    assert_eq!(time_left(2 * 3600 + 13 * 60), "2h13m");
    assert_eq!(time_left(3600), "1h0m");
    assert_eq!(time_left(45 * 60), "45m");
    assert_eq!(time_left(30), "0m");
}

#[test]
fn pct_left_rounds_half_up() {
    assert_eq!(pct_left(33.5), 67);
    assert_eq!(pct_left(33.4), 67);
    assert_eq!(pct_left(33.6), 66);
    assert_eq!(pct_left(120.0), 0);
    assert_eq!(pct_left(-5.0), 100);
    let t = Tmp::new();
    let s = session(33.5, 18000, 0.0, 604800);
    let out = go(&s, &[("type", "5h"), ("view", "pct")], t.path()).unwrap();
    assert!(strip(&out).ends_with("67%"));
}

#[test]
fn expired_window_is_omitted() {
    let t = Tmp::new();
    let s = session(10.0, 0, 10.0, -5);
    assert!(go(&s, &[("type", "5h")], t.path()).is_none());
    assert!(go(&s, &[("type", "7d")], t.path()).is_none());
    assert!(go(&Session::default(), &[("type", "5h")], t.path()).is_none());
}

#[test]
fn type_d_means_7d_and_h_means_5h() {
    let t = Tmp::new();
    let s = session(10.0, 3600 + 120, 50.0, 3 * 86400 + 4 * 3600);
    let v = |ty| strip(&go(&s, &[("type", ty), ("view", "time")], t.path()).unwrap());
    assert_eq!(v("h"), v("5h"));
    assert_eq!(v("5h"), "1h2m");
    assert_eq!(v("d"), v("7d"));
    assert_eq!(v("7d"), "3d4h");
    assert!(go(&s, &[("type", "x")], t.path()).is_none());
}

#[test]
fn state_file_switches_view() {
    let t = Tmp::new();
    let s = fresh();
    let before = strip(&go(&s, &[("type", "5h")], t.path()).unwrap());
    assert!(before.contains('█'));
    std::fs::write(t.path().join("pct-5h"), "").unwrap();
    let after = strip(&go(&s, &[("type", "5h")], t.path()).unwrap());
    assert!(after.ends_with("96%"), "{after}");
    // other key unaffected
    let seven = strip(&go(&s, &[("type", "7d")], t.path()).unwrap());
    assert!(seven.contains('█'));
}

#[test]
fn click_link_needs_installed_handler() {
    let t = Tmp::new();
    let s = fresh();
    let none = go(&s, &[("type", "5h"), ("click", "toggle")], t.path()).unwrap();
    assert!(!none.contains("\x1b]8;"), "no marker, no link");
    std::fs::write(t.path().join("click-installed"), "").unwrap();
    let linked = go(&s, &[("type", "5h"), ("click", "toggle")], t.path()).unwrap();
    assert!(
        linked.contains("\x1b]8;;bar://toggle/5h\x1b\\"),
        "{linked:?}"
    );
    assert!(!linked.contains("\x1b[4m"));
    let seven = go(&s, &[("type", "d"), ("click", "toggle")], t.path()).unwrap();
    assert!(seven.contains("bar://toggle/7d"));
}

#[test]
fn click_none_emits_no_link() {
    let t = Tmp::new();
    std::fs::write(t.path().join("click-installed"), "").unwrap();
    let out = go(&fresh(), &[("type", "5h"), ("click", "none")], t.path()).unwrap();
    assert!(!out.contains("\x1b]8;"));
}

#[test]
fn rule_in_config_overrides_builtin() {
    use hop_bar::conf::{Amount, Early, Rule};
    let t = Tmp::new();
    let mut cfg = Config::default();
    cfg.rules.push(Rule {
        target: "usage[type=5h]".into(),
        early: Early::Thresholds {
            warn: Amount::Percent(90),
            bad: Amount::Percent(80),
        },
        final_secs: 3600,
        final_bad: Amount::Percent(30),
    });
    let s = session(30.0, 18000, 0.0, 604800); // 70% left, below the 80% bad floor
    let runner = ColdRunner;
    let ctx = Ctx {
        session: &s,
        config: &cfg,
        now: NOW,
        runner: &runner,
        cache: None,
        columns: None,
        plugin_dirs: &[],
    };
    let out = render_with(&ctx, &opts(&[("type", "5h"), ("view", "pct")]), t.path()).unwrap();
    assert!(out.contains("\x1b[38;5;196m70%"), "{out:?}");
}
