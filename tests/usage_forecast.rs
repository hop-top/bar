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
    assert!(over.contains("38;5;196m"), "{over:?}");
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
    let dir = std::env::temp_dir().join(format!("bar-uf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pct-5h"), "").unwrap();
    let o = [("show", "time,bar"), ("alt", "eta,proj")];
    let out = strip_codes(&run(60.0, 3 * H, &o, &dir));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(out, "→1h20m p150%");
}
