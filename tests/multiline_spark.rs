mod common;
#[path = "common/render_fx.rs"]
mod render_fx;
use render_fx::*;

use hop_bar::conf::{parse, LoadResult};
use hop_bar::history::sample;
use hop_bar::style::strip_codes;

const SPARK: &str = "▁▂▃▄▅▆▇█";
const SID: &str = "a1b2c3d4-0000-4000-8000-000000000000";
const SECS: i64 = 2 * 3600;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Five 5h samples in `t`'s state dir, and a session JSON for that window.
fn seeded(t: &Tmp) -> String {
    let (n, resets) = (now(), now() + SECS);
    let state = t.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    for (i, u) in [10.0, 30.0, 50.0, 70.0, 90.0].iter().enumerate() {
        sample(&state, "5h", resets, n - 60 - (4 - i as i64) * 600, *u);
    }
    format!(
        r#"{{"session_id":"{SID}","model":{{"display_name":"Claude Opus 4.7"}},"rate_limits":{{"five_hour":{{"used_percentage":90,"resets_at":{resets}}}}}}}"#
    )
}

/// Longest run of adjacent spark glyphs in `row`.
fn spark_run(row: &str) -> usize {
    let (mut best, mut cur) = (0, 0);
    for c in strip_codes(row).chars() {
        cur = if SPARK.contains(c) { cur + 1 } else { 0 };
        best = best.max(cur);
    }
    best
}

fn spark_cells(row: &str) -> usize {
    strip_codes(row)
        .chars()
        .filter(|c| SPARK.contains(*c))
        .count()
}

/// Distinct 256-colour foregrounds in `row`.
fn colours(row: &str) -> usize {
    let mut v: Vec<&str> = row
        .split("\x1b[38;5;")
        .skip(1)
        .filter_map(|s| s.split('m').next())
        .collect();
    v.sort_unstable();
    v.dedup();
    v.len()
}

const SPARK_G: &str = "#{usage[type=5h,show=spark,color=gradient]}";
const BAR_G: &str = "#{usage[type=5h,show=gauge,bar,color=gradient,width=8]}";

#[test]
fn spark_gauge_and_gradient_render_on_every_line() {
    let t = Tmp::new();
    let stdin = seeded(&t);
    let conf = format!(
        "set left \"#{{sid}} │ #{{model}}\"\nset left@2 \"{SPARK_G}\"\nset right@3 \"{BAR_G}\"\n"
    );
    let o = run(&t).conf(&conf).stdin(&stdin).env("COLUMNS", "80").go();
    let rows: Vec<&str> = o.raw.trim_end_matches('\n').split('\n').collect();
    assert_eq!(rows.len(), 3, "{:?}", o.raw);
    assert!(
        strip_codes(rows[0]).starts_with("a1b2c3d4"),
        "{:?}",
        rows[0]
    );
    assert_eq!(spark_cells(rows[1]), 5, "{:?}", rows[1]);
    assert!(colours(rows[1]) > 1, "gradient spark: {:?}", rows[1]);
    // right-aligned on line 3: gauge cell plus an 8-cell gradient bar
    let r3 = strip_codes(rows[2]);
    assert!(
        r3.starts_with(' ') && r3.trim_start().chars().count() >= 9,
        "{r3:?}"
    );
    assert!(colours(rows[2]) > 1, "gradient bar: {:?}", rows[2]);
}

#[test]
fn spark_and_gradient_stay_whole_when_wrapped() {
    let t = Tmp::new();
    let stdin = seeded(&t);
    let conf = format!("set left \"#{{sid}} │ {SPARK_G} │ {BAR_G} │ #{{model}}\"\nset wrap on\n");
    for skin in ["", "set skin powerline\nset skin-glyphs ascii\n"] {
        let o = run(&t)
            .conf(&format!("{skin}{conf}"))
            .stdin(&stdin)
            .env("COLUMNS", "20")
            .go();
        let rows: Vec<&str> = o.raw.trim_end_matches('\n').split('\n').collect();
        assert!(rows.len() > 1, "{skin}: {:?}", o.raw);
        // the whole spark (5 cells in a row) sits on one row, in more than one colour
        let with_spark: Vec<&&str> = rows.iter().filter(|r| spark_run(r) >= 5).collect();
        assert_eq!(with_spark.len(), 1, "{skin}: {rows:?}");
        assert!(
            rows.iter().all(|r| spark_run(r) == 5 || spark_run(r) < 3),
            "{skin}: {rows:?}"
        );
        assert!(colours(with_spark[0]) > 1, "{skin}: {:?}", with_spark[0]);
        for r in &rows {
            assert!(visible(r) <= 16, "{skin}: {r:?}");
        }
    }
}

fn visible(r: &str) -> usize {
    hop_bar::style::visible_width(r)
}

#[test]
fn niche_placeholders_on_extra_lines_and_wrapped() {
    let src = "set left \"#{model}\"\nset left@2 \"#{sid} │ #{tmux}\"\nset right@2 \"#{compactions} │ #{skills}\"\n";
    let c = parse(src).unwrap();
    let out = hop_bar::preview::render("early", LoadResult::Ok(c), Some(80), 0);
    let rows: Vec<String> = out.split('\n').map(strip_codes).collect();
    assert_eq!(rows.len(), 2, "{out:?}");
    for want in ["a1b2c3d4", "work:1", "compact 2", "skills 3"] {
        assert!(rows[1].contains(want), "{want}: {rows:?}");
    }
    let c = parse(&format!("{src}set wrap on\n")).unwrap();
    let out = hop_bar::preview::render("early", LoadResult::Ok(c), Some(22), 0);
    let rows: Vec<String> = out.split('\n').map(strip_codes).collect();
    assert!(rows.len() > 2, "{rows:?}");
    let all = rows.join("\n");
    for want in ["a1b2c3d4", "work:1", "compact 2", "skills 3"] {
        assert_eq!(all.matches(want).count(), 1, "{want}: {rows:?}");
    }
}
