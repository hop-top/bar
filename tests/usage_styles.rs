use hop_bar::conf::Config;
use hop_bar::options::Options;
use hop_bar::placeholders::usage::render_with;
use hop_bar::placeholders::Ctx;
use hop_bar::render::ColdRunner;
use hop_bar::session::{Session, Window};
use hop_bar::style::{strip_codes, visible_width};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);
const NOW: i64 = 1_000_000;
const SECS: i64 = 2 * 3600 + 13 * 60;

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        let p = std::env::temp_dir().join(format!(
            "bar-ustyle-{}-{}",
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

/// 5h window with `used` percent used, 2h13m to reset.
fn go(used: f64, o: &[(&str, &str)], state: &Path) -> String {
    let s = Session {
        five_hour: Some(Window {
            used_percentage: used,
            resets_at: NOW + SECS,
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
    render_with(&ctx, &opts, state).unwrap()
}

fn plain(used: f64, o: &[(&str, &str)]) -> String {
    let t = Tmp::new();
    strip_codes(&go(used, o, t.path()))
}

#[test]
fn show_orders_components() {
    // 13% used: 87% left, 5 cells -> 4 filled
    assert_eq!(plain(13.0, &[("show", "time,bar")]), "2h13m ████░");
    assert_eq!(plain(13.0, &[("show", "bar,time")]), "████░ 2h13m");
    assert_eq!(plain(13.0, &[("show", "pct,time")]), "87% 2h13m");
    assert_eq!(plain(13.0, &[("show", "bar,pct")]), "████░ 87%");
    assert_eq!(plain(13.0, &[("show", "time,bar,pct")]), "2h13m ████░ 87%");
    assert_eq!(plain(13.0, &[("show", "pct")]), "87%");
    assert_eq!(plain(13.0, &[("show", "bar")]), "████░");
    assert_eq!(plain(13.0, &[("show", "time")]), "2h13m");
}

#[test]
fn show_ignores_unknown_and_repeats() {
    assert_eq!(plain(13.0, &[("show", "pct,zzz,pct")]), "87%");
    assert_eq!(plain(13.0, &[("show", "zzz")]), "2h13m ████░");
}

#[test]
fn view_alias_is_byte_identical_to_show() {
    let t = Tmp::new();
    for (view, show) in [("bar", "time,bar"), ("pct", "time,pct"), ("time", "time")] {
        for used in [0.0, 4.0, 13.0, 50.0, 97.0, 100.0] {
            assert_eq!(
                go(used, &[("view", view)], t.path()),
                go(used, &[("show", show)], t.path()),
                "{view} {used}"
            );
        }
    }
    assert_eq!(
        go(13.0, &[], t.path()),
        go(13.0, &[("show", "time,bar")], t.path())
    );
    // show wins over view
    assert_eq!(plain(13.0, &[("view", "pct"), ("show", "bar")]), "████░");
}

#[test]
fn count_used_flips_pct_and_fill() {
    assert_eq!(plain(13.0, &[("show", "pct"), ("count", "used")]), "13%");
    assert_eq!(plain(13.0, &[("show", "pct"), ("count", "left")]), "87%");
    assert_eq!(plain(60.0, &[("show", "bar"), ("count", "used")]), "███░░");
    assert_eq!(plain(60.0, &[("show", "bar")]), "██░░░");
}

#[test]
fn count_used_keeps_colour_from_remaining() {
    let t = Tmp::new();
    let left = go(85.0, &[("show", "pct")], t.path());
    let used = go(85.0, &[("show", "pct"), ("count", "used")], t.path());
    let code = |s: &str| s.split("\x1b[38;5;").nth(1).unwrap()[..3].to_string();
    assert_eq!(code(&left), code(&used));
    assert!(strip_codes(&used).ends_with("85%"));
}

#[test]
fn glyph_sets_fill_and_empty() {
    // 60% used -> 40% left -> 2 of 5 cells
    for (g, full, empty) in [
        ("blocks", '█', '░'),
        ("shades", '▓', '░'),
        ("dots", '●', '○'),
        ("squares", '■', '□'),
        ("bars", '▰', '▱'),
        ("braille", '⣿', '⣀'),
    ] {
        let want = format!("{0}{0}{1}{1}{1}", full, empty);
        assert_eq!(plain(60.0, &[("show", "bar"), ("glyphs", g)]), want, "{g}");
    }
}

#[test]
fn unknown_glyphs_fall_back_to_blocks() {
    assert_eq!(plain(60.0, &[("show", "bar"), ("glyphs", "nope")]), "██░░░");
}

#[test]
fn glyph_widths_stay_exact() {
    let t = Tmp::new();
    for g in [
        "blocks", "shades", "dots", "squares", "bars", "braille", "eighths",
    ] {
        for used in [0.0, 7.0, 33.0, 50.0, 91.0, 100.0] {
            let out = go(
                used,
                &[("show", "bar"), ("glyphs", g), ("width", "8")],
                t.path(),
            );
            assert_eq!(visible_width(&out), 8, "{g} {used}: {out:?}");
        }
    }
}

#[test]
fn eighths_partial_cell() {
    let e = |used: f64, extra: &[(&str, &str)]| {
        let mut o = vec![("show", "bar"), ("glyphs", "eighths")];
        o.extend_from_slice(extra);
        plain(used, &o)
    };
    assert_eq!(e(50.0, &[]), "██▌░░"); // 20 eighths: 2 full + 4
    assert_eq!(e(4.0, &[]), "████▊"); // 96% -> 38 eighths: 4 full + 6
    assert_eq!(e(100.0, &[]), "░░░░░");
    assert_eq!(e(0.0, &[]), "█████");
    assert_eq!(e(4.0, &[("count", "used")]), "▎░░░░"); // 4% -> 2 eighths
                                                       // each partial glyph at width 1: 12.5% steps
    for (used, g) in [
        (87.0, '▏'),
        (75.0, '▎'),
        (62.0, '▍'),
        (50.0, '▌'),
        (37.0, '▋'),
        (25.0, '▊'),
        (12.0, '▉'),
    ] {
        assert_eq!(e(used, &[("width", "1")]), g.to_string(), "{used}");
    }
}

#[test]
fn eighths_partial_cell_shares_bar_colour() {
    let t = Tmp::new();
    let out = go(50.0, &[("show", "bar"), ("glyphs", "eighths")], t.path());
    // full and partial cells sit in one colour run
    let run = out.split("\x1b[38;5;").nth(1).unwrap();
    assert!(
        run.split_once('m').unwrap().1.starts_with("██▌\x1b[39m"),
        "{out:?}"
    );
}

#[test]
fn alt_and_toggle() {
    let t = Tmp::new();
    let base = [("show", "time,bar"), ("alt", "pct,time")];
    assert_eq!(strip_codes(&go(13.0, &base, t.path())), "2h13m ████░");
    std::fs::write(t.path().join("pct-5h"), "").unwrap();
    assert_eq!(strip_codes(&go(13.0, &base, t.path())), "87% 2h13m");
}

#[test]
fn default_alt_swaps_bar_and_pct() {
    let t = Tmp::new();
    std::fs::write(t.path().join("pct-5h"), "").unwrap();
    let s = |o: &[(&str, &str)]| strip_codes(&go(13.0, o, t.path()));
    assert_eq!(s(&[("show", "time,bar")]), "2h13m 87%");
    assert_eq!(s(&[("show", "pct,time")]), "████░ 2h13m");
    assert_eq!(s(&[("show", "time")]), "2h13m");
    assert_eq!(s(&[("view", "bar")]), "2h13m 87%");
    assert_eq!(s(&[("view", "pct")]), "2h13m ████░");
}

#[test]
fn link_wraps_only_bar_or_pct() {
    let t = Tmp::new();
    std::fs::write(t.path().join("click-installed"), "").unwrap();
    let out = go(13.0, &[("show", "pct,time,bar")], t.path());
    let open = "\x1b]8;;bar://toggle/5h\x1b\\";
    let close = "\x1b]8;;\x1b\\";
    assert!(out.starts_with(&format!("{open}\x1b[38;5;")), "{out:?}");
    assert_eq!(out.matches(open).count(), 2, "{out:?}");
    let time_at = out.find("2h13m").unwrap();
    let before = &out[..time_at];
    assert_eq!(before.matches(open).count(), before.matches(close).count());
    let only_time = go(13.0, &[("show", "time")], t.path());
    assert!(!only_time.contains("\x1b]8;"));
}

#[test]
fn unknown_only_alt_falls_back_to_swap() {
    let t = Tmp::new();
    std::fs::write(t.path().join("pct-5h"), "").unwrap();
    let out = go(13.0, &[("show", "time,bar"), ("alt", "zzz")], t.path());
    assert_eq!(strip_codes(&out), "2h13m 87%");
}

#[test]
fn bar_toggle_cli_flips_to_alt_and_back() {
    let t = Tmp::new();
    let o = [("show", "time,bar"), ("alt", "pct,time")];
    let flip = || {
        assert_cmd::Command::cargo_bin("bar")
            .unwrap()
            .args(["toggle", "5h"])
            .env("BAR_STATE_DIR", t.path())
            .assert()
            .success();
    };
    assert_eq!(strip_codes(&go(13.0, &o, t.path())), "2h13m ████░");
    flip();
    assert_eq!(strip_codes(&go(13.0, &o, t.path())), "87% 2h13m");
    flip();
    assert_eq!(strip_codes(&go(13.0, &o, t.path())), "2h13m ████░");
}
