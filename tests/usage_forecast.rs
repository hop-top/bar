mod common;
use hop_bar::conf::Config;
use hop_bar::options::Options;
use hop_bar::placeholders::usage::render_with;
use hop_bar::placeholders::Ctx;
use hop_bar::render::ColdRunner;
use hop_bar::session::{Session, Window};
use hop_bar::style::strip_codes;
use std::path::Path;

const NOW: i64 = 1_000_000;
const H: i64 = 3600;

/// 5h window with `used` percent used and `left` seconds to reset; `state`
/// is the toggle directory.
fn run(used: f64, left: i64, o: &[(&str, &str)], state: &Path) -> String {
    let s = Session {
        five_hour: Some(Window {
            used_percentage: used,
            resets_at: NOW + left,
        }),
        ..Session::default()
    };
    let cfg = Config::default();
    let runner = ColdRunner;
    let ctx = Ctx {
        session: &s,
        config: &cfg,
        now: NOW,
        runner: &runner,
        cache: None,
        columns: None,
        plugin_dirs: &[],
        memo: Default::default(),
    };
    let mut opts = Options::default();
    opts.set("type", "5h");
    for (k, v) in o {
        opts.set(k, v);
    }
    render_with(&ctx, &opts, state).unwrap_or_default()
}

fn go(used: f64, left: i64, show: &str) -> String {
    run(
        used,
        left,
        &[("show", show)],
        Path::new("/nonexistent-bar-state"),
    )
}

fn plain(used: f64, left: i64, show: &str) -> String {
    strip_codes(&go(used, left, show))
}

#[test]
fn eta_before_reset_is_red() {
    // 2h elapsed, 60% used: 40% left lasts 80m more, reset in 3h
    let raw = go(60.0, 3 * H, "eta");
    assert_eq!(strip_codes(&raw), "→1h20m");
    assert!(raw.contains("38;5;203m"), "{raw:?}");
}

#[test]
fn eta_after_reset_is_ok_and_muted() {
    // 2h elapsed, 10% used: 18h to exhaustion, reset in 3h
    let raw = go(10.0, 3 * H, "eta");
    assert_eq!(strip_codes(&raw), "→ok");
    assert!(raw.contains("38;5;250m"), "{raw:?}");
}

#[test]
fn eta_hidden_when_unused_or_window_just_started() {
    assert_eq!(plain(0.0, 3 * H, "eta"), "");
    // 3m of 5h elapsed (1%)
    assert_eq!(plain(5.0, 5 * H - 180, "eta"), "");
    // 6m elapsed (2%): shown
    assert_ne!(plain(5.0, 5 * H - 360, "eta"), "");
    // hidden part leaves no stray space
    assert_eq!(plain(0.0, 3 * H, "time,eta,pct"), "3h0m 100%");
}

#[test]
fn eta_exhausted_budget_is_zero() {
    assert_eq!(plain(100.0, 3 * H, "eta"), "→0m");
}

#[test]
fn proj_shows_projected_percent() {
    // 2h of 5h elapsed
    assert_eq!(plain(40.0, 3 * H, "proj"), "p100%");
    assert_eq!(plain(60.0, 3 * H, "proj"), "p150%");
    assert_eq!(plain(10.0, 3 * H, "proj"), "p25%");
}

#[test]
fn proj_clamps_to_999() {
    // 6m elapsed, 90% used -> 4500%
    assert_eq!(plain(90.0, 5 * H - 360, "proj"), "p999%");
}

#[test]
fn proj_hidden_like_eta() {
    assert_eq!(plain(0.0, 3 * H, "proj"), "");
    assert_eq!(plain(5.0, 5 * H - 180, "proj"), "");
}

#[test]
fn proj_colour_follows_projected_remaining() {
    let over = go(60.0, 3 * H, "proj");
    assert!(over.contains("38;5;203m"), "{over:?}");
    let under = go(10.0, 3 * H, "proj");
    assert!(under.contains("38;5;46m"), "{under:?}");
}

#[test]
fn forecast_items_order_freely_with_others() {
    assert_eq!(
        plain(60.0, 3 * H, "eta,proj,pct,time"),
        "→1h20m p150% 40% 3h0m"
    );
    assert_eq!(plain(60.0, 3 * H, "time,bar,proj"), "3h0m ██░░░ p150%");
}

#[test]
fn alt_accepts_forecast_items() {
    let dir = common::tmp::Dir::new(&format!("bar-uf-{}", std::process::id()));
    std::fs::write(dir.join("pct-5h"), "").unwrap();
    let o = [("show", "time,bar"), ("alt", "eta,proj")];
    let out = strip_codes(&run(60.0, 3 * H, &o, &dir));
    assert_eq!(out, "→1h20m p150%");
}

#[test]
fn eta_and_proj_agree_at_the_boundary() {
    // 2h of 5h elapsed: used 39/40/41 -> p98/p100/p103 (rounded 97.5, 100, 102.5)
    for (used, red) in [(39.0, false), (40.0, false), (41.0, true)] {
        let eta = go(used, 3 * H, "eta");
        let proj = go(used, 3 * H, "proj");
        assert_eq!(eta.contains("38;5;203m"), red, "eta {used}: {eta:?}");
        assert_eq!(proj.contains("38;5;203m"), red, "proj {used}: {proj:?}");
        if !red {
            assert_eq!(strip_codes(&eta), "→ok");
            assert!(
                !proj.contains("38;5;196m") && !proj.contains("38;5;250m"),
                "{proj:?}"
            );
        }
    }
    assert_eq!(plain(39.0, 3 * H, "proj"), "p98%");
    assert_eq!(plain(40.0, 3 * H, "proj"), "p100%");
    assert_eq!(plain(41.0, 3 * H, "proj"), "p103%");
}

#[test]
fn rounding_up_to_100_is_not_red() {
    // used 40.1 -> projected 100.25 rounds to p100%: no red eta
    let eta = go(40.1, 3 * H, "eta");
    assert_eq!(strip_codes(&eta), "→ok");
    assert!(!go(40.1, 3 * H, "proj").contains("38;5;203m"));
}

#[test]
fn custom_bad_colour_applies_to_eta_and_proj() {
    let s = Session {
        five_hour: Some(Window {
            used_percentage: 60.0,
            resets_at: NOW + 3 * H,
        }),
        ..Session::default()
    };
    let mut cfg = Config::default();
    cfg.colors.bad = 160.into();
    let runner = ColdRunner;
    let ctx = Ctx {
        session: &s,
        config: &cfg,
        now: NOW,
        runner: &runner,
        cache: None,
        columns: None,
        plugin_dirs: &[],
        memo: Default::default(),
    };
    let mut opts = Options::default();
    opts.set("show", "eta,proj");
    let out = render_with(&ctx, &opts, Path::new("/nonexistent-bar-state")).unwrap();
    assert_eq!(out.matches("38;5;160m").count(), 2, "{out:?}");
}

#[test]
fn seven_day_window() {
    let s = Session {
        seven_day: Some(Window {
            used_percentage: 60.0,
            resets_at: NOW + 4 * 86400,
        }),
        ..Session::default()
    };
    let cfg = Config::default();
    let runner = ColdRunner;
    let ctx = Ctx {
        session: &s,
        config: &cfg,
        now: NOW,
        runner: &runner,
        cache: None,
        columns: None,
        plugin_dirs: &[],
        memo: Default::default(),
    };
    let mut opts = Options::default();
    opts.set("type", "7d");
    opts.set("show", "eta,proj");
    // 3d of 7d elapsed: 60% -> p140%, 40% left lasts 2d
    let out = render_with(&ctx, &opts, Path::new("/nonexistent-bar-state")).unwrap();
    assert_eq!(strip_codes(&out), "→2d p140%");
}

#[test]
fn only_bar_and_pct_are_linked() {
    let dir = common::tmp::Dir::new(&format!("bar-uf-link-{}", std::process::id()));
    std::fs::write(dir.join("click-installed"), "").unwrap();
    let o = [("show", "time,eta,bar,proj,pct")];
    let raw = run(60.0, 3 * H, &o, &dir);
    let open = "\x1b]8;;bar://toggle/5h\x1b\\";
    assert_eq!(raw.matches(open).count(), 2, "{raw:?}");
    for part in ["→1h20m", "p150%"] {
        let at = raw.find(part).unwrap();
        let before = &raw[..at];
        let opens = before.matches(open).count();
        let closes = before.matches("\x1b]8;;\x1b\\").count();
        assert_eq!(opens, closes, "{part} inside a link: {raw:?}");
    }
}
