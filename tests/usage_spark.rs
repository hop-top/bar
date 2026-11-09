mod common;
use hop_bar::conf::Config;
use hop_bar::history::sample;
use hop_bar::options::Options;
use hop_bar::pace::{builtin_rule, score};
use hop_bar::placeholders::usage::render_with;
use hop_bar::placeholders::Ctx;
use hop_bar::render::ColdRunner;
use hop_bar::session::{Session, Window};
use hop_bar::style::{dim, fg, fg_code, ramp, strip_codes};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);
const NOW: i64 = 1_000_000;
const SECS: i64 = 2 * 3600 + 13 * 60;
const P5: i64 = 5 * 3600;

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        let p = crate::common::tmp::root().join(format!(
            "bar-spark-{}-{}",
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
        crate::common::tmp::remove(&self.0);
    }
}

fn go_at(used: f64, secs: i64, o: &[(&str, &str)], state: &Path) -> String {
    let s = Session {
        five_hour: Some(Window {
            used_percentage: used,
            resets_at: NOW + secs,
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

fn go(used: f64, o: &[(&str, &str)], state: &Path) -> String {
    go_at(used, SECS, o, state)
}

/// Seeds a series for the window `go` renders.
fn seed(state: &Path, used: &[f64]) {
    let n = used.len() as i64;
    for (i, u) in used.iter().enumerate() {
        let ts = NOW - 60 - (n - 1 - i as i64) * 600;
        sample(state, "5h", NOW + SECS, ts, *u);
    }
}

fn col(left: i64, secs: i64) -> hop_bar::color::Color {
    ramp(
        score(left, Some(secs), P5, &builtin_rule("5h")),
        &Config::default().ramp,
    )
}

// --- sampling through render ---

#[test]
fn render_with_spark_records_a_sample_per_step() {
    let t = Tmp::new();
    go(10.0, &[("show", "spark")], t.path());
    // same instant: no second sample, so the spark stays hidden
    assert_eq!(strip_codes(&go(20.0, &[("show", "spark")], t.path())), "");
}

#[test]
fn no_spark_item_records_nothing() {
    let t = Tmp::new();
    go(10.0, &[("show", "time,bar")], t.path());
    assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 0);
}

// --- spark ---

#[test]
fn spark_hidden_until_two_samples() {
    let t = Tmp::new();
    seed(t.path(), &[10.0]);
    assert_eq!(
        strip_codes(&go(10.0, &[("show", "time,spark")], t.path())),
        "2h13m"
    );
}

#[test]
fn spark_levels_over_used_percent() {
    let t = Tmp::new();
    seed(t.path(), &[0.0, 14.0, 29.0, 50.0, 71.0, 86.0, 100.0]);
    // width 7: one cell per sample, used% mapped onto ▁..█
    assert_eq!(
        strip_codes(&go(100.0, &[("show", "spark"), ("width", "7")], t.path())),
        "▁▂▃▅▆▇█"
    );
}

#[test]
fn spark_takes_the_last_n_samples() {
    let t = Tmp::new();
    seed(t.path(), &[0.0, 0.0, 0.0, 100.0, 100.0]);
    assert_eq!(
        strip_codes(&go(100.0, &[("show", "spark"), ("width", "2")], t.path())),
        "██"
    );
    // fewer samples than width: only what exists
    assert_eq!(
        strip_codes(&go(100.0, &[("show", "spark"), ("width", "9")], t.path())),
        "▁▁▁██"
    );
}

#[test]
fn spark_solid_uses_the_current_colour() {
    let t = Tmp::new();
    seed(t.path(), &[10.0, 20.0, 30.0]);
    let out = go(30.0, &[("show", "spark")], t.path());
    let c = col(70, SECS);
    assert_eq!(out, fg(c, "▂▂▃"));
}

#[test]
fn spark_gradient_colours_each_cell_by_its_own_value_without_time_input() {
    let t = Tmp::new();
    seed(t.path(), &[90.0, 50.0, 10.0]);
    let out = go(10.0, &[("show", "spark"), ("color", "gradient")], t.path());
    let pure = |left: i64| {
        ramp(
            score(left, None, P5, &builtin_rule("5h")),
            &Config::default().ramp,
        )
    };
    let want = format!(
        "{}{}{}",
        fg(pure(10), "▇"),
        fg(pure(50), "▅"),
        fg(pure(90), "▂")
    );
    assert_eq!(out, want);
}

#[test]
fn spark_and_gauge_in_alt_order_and_with_others() {
    let t = Tmp::new();
    seed(t.path(), &[0.0, 100.0]);
    assert_eq!(
        strip_codes(&go(100.0, &[("show", "pct,spark,gauge,time")], t.path())),
        "0% ▁█ ▁ 2h13m"
    );
}

// --- gauge ---

#[test]
fn gauge_levels_for_count_left_and_used() {
    let t = Tmp::new();
    for (used, left_glyph, used_glyph) in [
        (100.0, "▁", "█"),
        (87.0, "▂", "▇"),
        (50.0, "▅", "▅"),
        (13.0, "▇", "▂"),
        (0.0, "█", "▁"),
    ] {
        assert_eq!(
            strip_codes(&go(used, &[("show", "gauge")], t.path())),
            left_glyph,
            "left {used}"
        );
        assert_eq!(
            strip_codes(&go(used, &[("show", "gauge"), ("count", "used")], t.path())),
            used_glyph,
            "used {used}"
        );
    }
}

#[test]
fn gauge_is_one_cell_in_the_value_colour() {
    let t = Tmp::new();
    let out = go(13.0, &[("show", "gauge")], t.path());
    assert_eq!(out, fg(col(87, SECS), "▇"));
}

// --- gradient bar ---

/// Filled cells ramp from the bad end to the current score; equal
/// neighbours share one SGR run.
fn expected_bar(filled: usize, w: usize, used: bool, left: i64, secs: i64) -> String {
    let codes: Vec<hop_bar::color::Color> = (0..filled)
        .map(|i| {
            let reach = |v: i64| v * (i as i64 + 1) / filled as i64;
            col(
                if used {
                    100 - reach(100 - left)
                } else {
                    reach(left)
                },
                secs,
            )
        })
        .collect();
    let mut s = String::new();
    let mut i = 0;
    while i < codes.len() {
        let n = codes[i..].iter().take_while(|&&c| c == codes[i]).count();
        s.push_str(&fg(codes[i], &"█".repeat(n)));
        i += n;
    }
    s.push_str(&dim(&"░".repeat(w - filled)));
    s
}

#[test]
fn solid_is_default_and_unchanged() {
    let t = Tmp::new();
    assert_eq!(
        go(13.0, &[("show", "bar")], t.path()),
        go(13.0, &[("show", "bar"), ("color", "solid")], t.path())
    );
    assert_eq!(
        go(13.0, &[("show", "bar")], t.path()),
        format!("{}{}", fg(col(87, SECS), "████"), dim("░"))
    );
}

#[test]
fn gradient_bar_colours_cells_along_the_ramp() {
    let t = Tmp::new();
    let out = go(13.0, &[("show", "bar"), ("color", "gradient")], t.path());
    assert_eq!(out, expected_bar(4, 5, false, 87, SECS));
    assert_eq!(strip_codes(&out), "████░");
}

#[test]
fn gradient_bar_respects_count_used() {
    let t = Tmp::new();
    let out = go(
        60.0,
        &[("show", "bar"), ("color", "gradient"), ("count", "used")],
        t.path(),
    );
    assert_eq!(out, expected_bar(3, 5, true, 40, SECS));
}

#[test]
fn gradient_final_stretch_inverts() {
    let t = Tmp::new();
    let secs = 20 * 60;
    let o = [("show", "bar"), ("color", "gradient"), ("width", "10")];
    let out = go_at(80.0, secs, &o, t.path());
    assert_eq!(out, expected_bar(2, 10, false, 20, secs));
    // score falls as the cell reaches more remaining, so the tip is the worse colour
    let (first, tip) = (col(10, secs), col(20, secs));
    assert_ne!(first, tip);
    assert!(out.starts_with(&fg_code(first)), "{out:?}");
    assert!(out.contains(&fg_code(tip)), "{out:?}");
    // the tip is the solid colour
    let solid = go_at(80.0, secs, &[("show", "bar"), ("width", "10")], t.path());
    assert!(solid.starts_with(&fg_code(tip)), "{solid:?}");
    // early in the window the same bar ramps differently
    assert_ne!(out, go(80.0, &o, t.path()));
}

#[test]
fn gradient_bar_keeps_width_and_glyph_sets() {
    let t = Tmp::new();
    for g in [
        "blocks", "shades", "dots", "squares", "bars", "braille", "eighths",
    ] {
        let solid = go(33.0, &[("show", "bar"), ("glyphs", g)], t.path());
        let grad = go(
            33.0,
            &[("show", "bar"), ("glyphs", g), ("color", "gradient")],
            t.path(),
        );
        assert_eq!(strip_codes(&solid), strip_codes(&grad), "{g}");
    }
}
