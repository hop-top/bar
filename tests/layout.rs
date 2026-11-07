use hop_bar::layout::compose;
use hop_bar::style::{bold, dim, fg, link, ramp, sanitize, visible_width};

const RAMP: [u8; 11] = [196, 202, 208, 214, 220, 226, 190, 154, 118, 82, 46];

fn s(n: usize) -> Vec<String> {
    vec!["x".repeat(n)]
}

fn pad_to_line(line: &str, col: usize) -> usize {
    line.chars().take_while(|c| *c == ' ').count().min(col)
}

#[test]
fn visible_width_ignores_sgr_and_osc8() {
    let t = format!(
        "{} {}{}",
        fg(114, "ok"),
        link("https://x.io/a", "repo"),
        bold(&dim("z"))
    );
    assert_eq!(visible_width(&t), "ok repoz".len());
    assert_eq!(visible_width("\x1b]8;;u\x07hi\x1b]8;;\x07"), 2);
}

#[test]
fn link_adds_no_underline_code() {
    let l = link("https://x.io", "t");
    assert!(l.starts_with("\x1b]8;;https://x.io\x1b\\"));
    assert!(l.ends_with("\x1b]8;;\x1b\\"));
    assert!(!l.contains("\x1b[4"));
}

#[test]
fn ramp_steps() {
    for (score, want) in [(0, 196), (1, 202), (50, 226), (90, 82), (91, 46), (100, 46)] {
        assert_eq!(ramp(score, &RAMP), want, "score {score}");
    }
    assert_eq!(ramp(-29, &RAMP), 196);
    assert_eq!(ramp(130, &RAMP), 46);
}

#[test]
fn visible_width_counts_cjk_and_emoji_as_two() {
    assert_eq!(visible_width("日本"), 4);
    assert_eq!(visible_width("🚀"), 2);
    assert_eq!(visible_width("a🚀日"), 5);
}

#[test]
fn sanitize_strips_injected_osc() {
    let out = sanitize("a\x1b]0;pwn\x07b\x1b[31mc");
    assert!(!out.contains('\x1b'));
    assert!(!out.contains('\x07'));
    assert!(out.starts_with('a'));
}

#[test]
fn c1_controls_stripped() {
    assert_eq!(sanitize("a\u{9b}31mb\u{9d}c\u{85}"), "a31mbc");
}

#[test]
fn link_url_with_control_chars_rejected() {
    for u in ["https://x\x1b\\evil", "https://x\x07", "https://x\u{9c}y"] {
        let l = link(u, "text");
        assert_eq!(l, "text");
    }
}

#[test]
fn center_is_true_centre_when_room() {
    let out = compose(&s(40), &s(50), &s(11), Some(190), 3);
    assert!(!out.contains('\n'));
    assert_eq!(visible_width(&out), 186);
    // centre block starts after the left group and its gap
    let start = out[40..].find('x').unwrap() + 40;
    assert_eq!(start + 25, 93);
}

#[test]
fn spare_splits_evenly_when_left_is_long() {
    let out = compose(&s(70), &s(40), &s(10), Some(130), 3);
    assert!(!out.contains('\n'));
    assert_eq!(out.matches("   ").count(), 2);
    assert!(out.contains(&format!(
        "{}   {}   {}",
        "x".repeat(70),
        "x".repeat(40),
        "x".repeat(10)
    )));
}

#[test]
fn wraps_when_spare_below_two_gaps() {
    // usable 100, widths 40+40+15 -> spare 5 < 6
    let out = compose(&s(40), &s(40), &s(15), Some(104), 3);
    let lines: Vec<&str> = out.split('\n').collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1], "x".repeat(40));
}

#[test]
fn second_line_has_no_padding() {
    let out = compose(&s(40), &s(40), &s(15), Some(104), 3);
    let l2 = out.split('\n').nth(1).unwrap();
    assert_eq!(pad_to_line(l2, 5), 0);
    assert!(!l2.ends_with(' '));
}

#[test]
fn first_line_pads_left_and_right_when_wrapped() {
    let out = compose(&s(40), &s(40), &s(15), Some(104), 3);
    let l1 = out.split('\n').next().unwrap();
    assert_eq!(visible_width(l1), 100);
    assert_eq!(
        l1,
        format!("{}{}{}", "x".repeat(40), " ".repeat(45), "x".repeat(15))
    );
}

#[test]
fn first_line_joins_with_separator_when_gap_too_small() {
    // usable 100; left 50 right 49 -> pad 1 < gap
    let out = compose(&s(50), &s(40), &s(49), Some(104), 3);
    let l1 = out.split('\n').next().unwrap();
    assert_eq!(
        l1,
        format!(
            "{}{}{}",
            "x".repeat(50),
            format!(" {} ", dim("│")),
            "x".repeat(49)
        )
    );
}

#[test]
fn missing_columns_takes_two_lines() {
    let out = compose(&s(5), &s(5), &s(5), None, 3);
    assert_eq!(out.split('\n').count(), 2);
    assert_eq!(out.split('\n').nth(1).unwrap(), "xxxxx");
}

#[test]
fn no_center_pads_left_and_right() {
    let out = compose(&s(10), &[], &s(10), Some(54), 3);
    assert_eq!(
        out,
        format!("{}{}{}", "x".repeat(10), " ".repeat(30), "x".repeat(10))
    );
}

#[test]
fn no_center_joins_with_separator_when_tight() {
    let out = compose(&s(25), &[], &s(24), Some(54), 3);
    assert!(!out.contains('\n'));
    assert!(out.contains('│'));
}

#[test]
fn center_only_is_centred() {
    let out = compose(&[], &s(10), &[], Some(54), 3);
    assert_eq!(out, format!("{}{}", " ".repeat(20), "x".repeat(10)));
}

#[test]
fn empty_segments_drop_their_separator() {
    let segs = vec!["a".to_string(), String::new(), "b".to_string()];
    let out = compose(&segs, &[], &[], Some(80), 3);
    assert_eq!(out, format!("a {} b", dim("│")));
}
