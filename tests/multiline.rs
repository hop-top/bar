use hop_bar::check::{check_text, Level};
use hop_bar::conf::{parse, Config, LoadResult, Wrap};
use hop_bar::palette;
use hop_bar::presets;
use hop_bar::render::{render, ColdRunner};
use hop_bar::session::Session;
use hop_bar::style::{bg_code, strip_codes, visible_width};

const A: &str = "aaaaaaaaaa";
const B: &str = "bbbbbbbbbb";
const C: &str = "cccccccccc";
const D: &str = "dddddddddd";
const E: &str = "eeeeeeeeee";

/// Five 10-cell text segments: three left, one centre, one right.
fn abcde() -> String {
    format!("set left \"{A} │ {B} │ {C}\"\nset center \"{D}\"\nset right \"{E}\"\n")
}

fn cfg(src: &str) -> Config {
    parse(src).unwrap_or_else(|e| panic!("{src}: {e:?}"))
}

fn raw(src: &str, cols: Option<usize>) -> String {
    render(&Session::default(), &cfg(src), cols, &ColdRunner)
}

fn rows(src: &str, cols: Option<usize>) -> Vec<String> {
    let out = raw(src, cols);
    if out.is_empty() {
        return Vec::new();
    }
    out.split('\n').map(strip_codes).collect()
}

fn joined(segs: &[&str]) -> String {
    segs.join(" │ ")
}

// --- config ---

#[test]
fn line_keys_parse() {
    let c = cfg("set left@2 l2\nset center@3 c3\nset right@4 r4\nset left@1 l1\n");
    assert_eq!(c.left, "l1");
    let lines = c.lines();
    assert_eq!(lines[0], ["l1", "", ""]);
    assert_eq!(lines[1], ["l2", "", ""]);
    assert_eq!(lines[2], ["", "c3", ""]);
    assert_eq!(lines[3], ["", "", "r4"]);
    for bad in ["left@0", "left@5", "center@9", "right@x", "right@", "top@2"] {
        assert!(parse(&format!("set {bad} v")).is_err(), "{bad}");
    }
}

#[test]
fn wrap_parses_off_by_default() {
    assert_eq!(Config::default().wrap, Wrap::Off);
    assert_eq!(cfg("set wrap on").wrap, Wrap::On);
    assert_eq!(cfg("set wrap off").wrap, Wrap::Off);
    // `auto` predates `on`/`off` and keeps today's layout
    assert_eq!(cfg("set wrap auto").wrap, Wrap::Off);
    assert!(parse("set wrap sideways").is_err());
}

// --- lines ---

#[test]
fn two_lines() {
    assert_eq!(rows("set left aa\nset left@2 bb\n", Some(80)), ["aa", "bb"]);
}

#[test]
fn three_lines_each_laid_out_alone() {
    let src = "set left aa\nset left@2 l2\nset center@2 c2\nset right@2 r2\nset right@3 zz\n";
    let got = rows(src, Some(40));
    assert_eq!(got.len(), 3, "{got:?}");
    assert_eq!(got[0], "aa");
    // line 2 is placed exactly as line 1 would place the same groups
    let alone = rows("set left l2\nset center c2\nset right r2\n", Some(40));
    assert_eq!(got[1], alone[0]);
    assert_eq!(got[2], format!("{}zz", " ".repeat(34)));
}

#[test]
fn empty_line_omitted() {
    assert_eq!(rows("set left aa\nset left@3 cc\n", Some(80)), ["aa", "cc"]);
    assert_eq!(rows("set left@3 cc\n", Some(80)), ["cc"]);
    // a line whose placeholders all render empty is dropped too
    let c = cfg("set left \"#{model}\"\nset left@2 \"#{dirty}\"\nset left@3 \"#{ctx}\"\n");
    let out = hop_bar::preview::render("final", LoadResult::Ok(c), Some(80), 0);
    assert_eq!(out.lines().count(), 2, "{out:?}");
}

#[test]
fn wrap_off_caps_at_four_rows() {
    // every line pushes its centre below: 2 rows per line
    let mut src = String::new();
    for n in 1..=4 {
        src.push_str(&format!(
            "set left@{n} \"l{n}{A}\"\nset center@{n} \"c{n}{B}\"\n"
        ));
    }
    let got = rows(&src, Some(20));
    assert_eq!(
        got,
        [
            format!("l1{A}"),
            format!("c1{B}"),
            format!("l2{A}"),
            format!("c2{B}")
        ]
    );
}

// --- wrap ---

#[test]
fn wrap_off_is_todays_layout() {
    for cols in [None, Some(10), Some(20), Some(30), Some(40), Some(80)] {
        let src = abcde();
        let off = raw(&format!("{src}set wrap off\n"), cols);
        assert_eq!(off, raw(&src, cols), "{cols:?}");
    }
    // 40 columns: the centre goes below, left and right joined
    assert_eq!(
        rows(&abcde(), Some(40)),
        [joined(&[A, B, C, E]), D.to_string()]
    );
}

#[test]
fn wrap_off_matches_plain_goldens() {
    let l = "set left \"#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=3,author=@me]} │ #{issues[limit=3,assignee=@me]}\"\nset center \"#{dirty} │ #{ahead}\"\nset right \"#{account}\"\nset wrap off\n";
    for cols in [200, 120, 80, 50, 20] {
        let p = format!(
            "{}/tests/fixtures/skins/plain-base-{cols}.txt",
            env!("CARGO_MANIFEST_DIR")
        );
        let want = std::fs::read_to_string(p).unwrap();
        let got = hop_bar::preview::render("early", LoadResult::Ok(cfg(l)), Some(cols), 0);
        assert_eq!(got, want, "{cols}");
    }
}

#[test]
fn wrap_on_fitting_line_is_unchanged() {
    let src = abcde();
    let on = raw(&format!("{src}set wrap on\n"), Some(80));
    assert_eq!(on, raw(&src, Some(80)));
    assert!(!on.contains('\n'));
}

#[test]
fn wrap_on_breaks_at_segment_boundaries() {
    let src = format!("{}set wrap on\n", abcde());
    let cases: [(usize, Vec<Vec<&str>>); 5] = [
        (40, vec![vec![A, B, C], vec![D, E]]),
        (30, vec![vec![A, B], vec![C, D], vec![E]]),
        // five rows needed: four printed, the last segment dropped
        (20, vec![vec![A], vec![B], vec![C], vec![D]]),
        (14, vec![vec![A], vec![B], vec![C], vec![D]]),
        // a segment wider than the row stands alone
        (8, vec![vec![A], vec![B], vec![C], vec![D]]),
    ];
    for (cols, want) in cases {
        let want: Vec<String> = want.iter().map(|r| joined(r)).collect();
        let got = rows(&src, Some(cols));
        assert_eq!(got, want, "COLUMNS={cols}");
        for r in &got {
            let single = !r.contains(" │ ");
            assert!(single || visible_width(r) <= cols - 4, "{cols}: {r:?}");
        }
    }
}

#[test]
fn wrap_on_without_columns_keeps_todays_layout() {
    let src = abcde();
    assert_eq!(raw(&format!("{src}set wrap on\n"), None), raw(&src, None));
}

#[test]
fn wrap_on_shares_the_four_row_cap_across_lines() {
    let two = format!(
        "set left \"{A} │ {B} │ {C}\"\nset left@2 \"{D} │ {E}\"\nset left@3 xx\nset wrap on\n"
    );
    // line 1 takes three rows, line 2 gets the last (what fits), line 3 is cut
    assert_eq!(
        rows(&two, Some(20)),
        [A.to_string(), B.to_string(), C.to_string(), D.to_string()]
    );
    let src = format!("{}set left@2 ff\nset wrap on\n", abcde());
    assert_eq!(rows(&src, Some(20)).len(), 4);
}

#[test]
fn wrap_on_fourth_row_fits_and_drops_the_rest() {
    // ten 3-cell segments, two per 10-cell row: eight shown, two dropped
    let segs: Vec<String> = (0..10).map(|i| format!("s{i:02}")).collect();
    let src = format!("set left \"{}\"\nset wrap on\n", segs.join(" │ "));
    let got = rows(&src, Some(14));
    let want: Vec<String> = segs[..8].chunks(2).map(|p| p.join(" │ ")).collect();
    assert_eq!(got, want);
    for r in &got {
        assert!(visible_width(r) <= 10, "{r:?}");
    }
}

#[test]
fn wrap_on_flows_exactly_when_wrap_off_overflows() {
    // the fit test is the placement itself: wrap on changes nothing while
    // the wrap-off layout is one row within COLUMNS - 4
    let configs = [
        abcde(),
        format!("set left \"{A} │ {B}\"\nset right \"{E}\"\n"),
        format!("set center \"{D}\"\nset right \"{E}\"\n"),
        format!("set left \"{A}\"\nset center \"{D}\"\n"),
        format!("set skin powerline\nset skin-glyphs ascii\n{}", abcde()),
        format!("set skin capsule\nset skin-glyphs ascii\n{}", abcde()),
    ];
    for src in &configs {
        for cols in (6..=120).step_by(1) {
            let off = raw(src, Some(cols));
            let on = raw(&format!("{src}set wrap on\n"), Some(cols));
            let fits = !off.contains('\n') && visible_width(&off) <= cols - 4;
            if fits {
                assert_eq!(on, off, "COLUMNS={cols} {src:?}");
            } else {
                // flowed: every row within width unless it is one segment
                for r in on.split('\n').map(strip_codes) {
                    let one = !r.contains(" │ ")
                        && !r.trim_end_matches('>').contains('>')
                        && r.matches('(').count() <= 1;
                    assert!(one || visible_width(&r) <= cols - 4, "COLUMNS={cols} {r:?}");
                }
            }
        }
    }
}

// --- skins ---

#[test]
fn skins_paint_every_line() {
    let src = "set skin powerline\nset skin-glyphs ascii\nset left aa\nset left@2 bb\n";
    let out = raw(src, Some(80));
    let lines: Vec<&str> = out.split('\n').collect();
    assert_eq!(lines.len(), 2);
    let first = bg_code(palette::default().bgs[0]);
    for l in &lines {
        assert!(l.starts_with(&first), "{l:?}");
    }
    assert_eq!(rows(src, Some(80)), [" aa >", " bb >"]);
}

#[test]
fn wrapped_rows_inherit_the_skin() {
    let pl = format!(
        "set skin powerline\nset skin-glyphs ascii\nset wrap on\n{}",
        abcde()
    );
    let p = |s: &str| format!(" {s} ");
    assert_eq!(
        rows(&pl, Some(30)),
        [
            format!("{}>{}>", p(A), p(B)),
            format!("{}>{}>", p(C), p(D)),
            // a right-group segment flows left-aligned, arrow pointing right
            format!("{}>", p(E)),
        ]
    );
    // backgrounds keep cycling across the wrapped rows
    let out = raw(&pl, Some(30));
    let second = out.split('\n').nth(1).unwrap();
    let bgs = palette::default().bgs;
    assert!(
        second.starts_with(&bg_code(bgs[2 % bgs.len()])),
        "{second:?}"
    );

    let cap = format!(
        "set skin capsule\nset skin-glyphs ascii\nset wrap on\n{}",
        abcde()
    );
    let c = |s: &str| format!("({s})");
    assert_eq!(
        rows(&cap, Some(30)),
        [
            format!("{} {}", c(A), c(B)),
            format!("{} {}", c(C), c(D)),
            c(E)
        ]
    );
}

#[test]
fn centre_line_under_skin_with_wrap_on() {
    let src = format!("set skin powerline\nset wrap on\n{}", abcde());
    // fits: centred, right group drawn with left-pointing arrows
    let wide = raw(&src, Some(80));
    assert!(
        !wide.contains('\n') && wide.contains('\u{e0b2}'),
        "{wide:?}"
    );
    assert!(strip_codes(&wide).contains(&" ".repeat(5)), "{wide:?}");
    // wrapped: every row a left group that opens and closes its background
    let out = raw(&src, Some(30));
    let bgs = palette::default().bgs;
    let rows: Vec<&str> = out.split('\n').collect();
    assert_eq!(rows.len(), 3, "{out:?}");
    for (i, r) in rows.iter().enumerate() {
        assert!(r.starts_with(&bg_code(bgs[(2 * i) % bgs.len()])), "{r:?}");
        assert!(r.contains("\x1b[49m\x1b[38;5;"), "{r:?}");
        assert!(r.ends_with("\u{e0b0}\x1b[39m"), "{r:?}");
        assert!(!r.contains('\u{e0b2}'), "{r:?}");
    }
    // the centre segment starts row 2, left-aligned
    assert!(strip_codes(rows[1]).starts_with(&format!(" {C} ")));
    assert!(strip_codes(rows[1]).contains(D));
}

// --- check, preview, presets ---

#[test]
fn check_validates_line_keys() {
    let r = check_text("set left@5 \"#{ctx}\"\n", None, &[]);
    assert!(
        r.issues
            .iter()
            .any(|i| i.level == Level::Error && i.line == 1 && i.message.contains("1-4")),
        "{:?}",
        r.issues
    );
    let r = check_text("set wrap on\nset center@2 \"#{nope}\"\n", None, &[]);
    assert!(
        r.issues
            .iter()
            .any(|i| i.line == 2 && i.message.contains("unknown placeholder `nope`")),
        "{:?}",
        r.issues
    );
    let r = check_text(
        "set left@2 \"#{ctx}\"\nset right@4 \"#{model}\"\n",
        None,
        &[],
    );
    assert!(r.issues.is_empty(), "{:?}", r.issues);
}

#[test]
fn preview_shows_every_line_of_two_line_preset() {
    assert!(presets::NAMES.contains(&"two-line"));
    let c = cfg(presets::get("two-line").unwrap());
    let out = hop_bar::preview::render("early", LoadResult::Ok(c), Some(200), 0);
    assert_eq!(out.lines().count(), 2, "{out:?}");
}
