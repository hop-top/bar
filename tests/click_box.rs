#[path = "common/fake_env.rs"]
mod fake_env;

use fake_env::{fixture, url, Fake};
use hop_bar::click::{box_is_empty, run, Refusal};
use std::path::Path;

/// A box border `n` cells wide, coloured as Claude Code draws it.
fn rule(n: usize) -> String {
    format!("\x1b[38;5;244m{}\x1b[39m", "─".repeat(n))
}

/// `body` between a 60-cell top and bottom border, with a footer below.
fn boxed(body: &str) -> String {
    format!(
        "{}\n{body}\n{}\n  \x1b[2m? for shortcuts\x1b[22m\n",
        rule(60),
        rule(60)
    )
}

fn refused(f: &Fake, u: &str, want: Refusal) {
    assert_eq!(run(f, u), Err(want), "{u:?}");
    assert!(f.sent().is_empty(), "sent on refusal: {u:?}");
}

// ── Box parsing, one fixture set per Claude Code version ────────────────────

#[test]
fn box_fixtures_per_version() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sessions");
    let mut seen = 0;
    for v in std::fs::read_dir(&root).unwrap() {
        for e in std::fs::read_dir(v.unwrap().path()).unwrap() {
            let p = e.unwrap().path();
            let name = p.file_stem().unwrap().to_string_lossy().to_string();
            let screen = std::fs::read_to_string(&p).unwrap();
            assert_eq!(
                box_is_empty(&screen),
                name.starts_with("empty"),
                "{}",
                p.display()
            );
            seen += 1;
        }
    }
    assert!(seen >= 4);
    assert!(!box_is_empty(""), "no prompt marker refuses");
    assert!(!box_is_empty(&boxed("❯\u{a0}\x1b[0mdraft")));
    assert!(box_is_empty(&boxed(
        "❯\u{a0}\x1b[0m\x1b[2mTry \"x\"\x1b[22m"
    )));
    assert!(
        !box_is_empty(&format!("{}\n❯\u{a0}\n", rule(60))),
        "no border below refuses"
    );
    assert!(
        !box_is_empty(&format!("❯\u{a0}\n{}\n", rule(60))),
        "no border above refuses"
    );
}

#[test]
fn multiline_draft_with_blank_first_line_refused() {
    assert!(!box_is_empty(&fixture("draft-multiline")));
    let f = Fake::new();
    f.screen("%1", &fixture("draft-multiline"));
    refused(&f, &url("model"), Refusal::NotEmpty);
}

#[test]
fn dim_then_typed_text_refused() {
    assert!(!box_is_empty(&boxed("❯ \x1b[2mTry\x1b[22m typed")));
    assert!(!box_is_empty(&boxed("❯ \x1b[2mTry\x1b[0mtyped")));
    assert!(!box_is_empty(&fixture("draft-after-dim")));
    assert!(box_is_empty(&boxed("❯ \x1b[2mTry \"x\"\x1b[22m \x1b[0m")));
    let f = Fake::new();
    f.screen("%1", &fixture("draft-after-dim"));
    refused(&f, &url("model"), Refusal::NotEmpty);
}

#[test]
fn typed_rule_line_does_not_spoof_border() {
    assert!(!box_is_empty(&fixture("draft-typed-rule")));
    // Any width: same as the box, shorter, longer.
    for n in [60, 20, 80] {
        let body = format!("❯\u{a0}\n{}\nrm -rf build", "─".repeat(n));
        assert!(!box_is_empty(&boxed(&body)), "{n}");
    }
    // Bottom border narrower than the top, or a tiny box: refuse.
    assert!(!box_is_empty(&format!(
        "{}\n❯\u{a0}\n{}\n",
        rule(60),
        rule(40)
    )));
    assert!(!box_is_empty(&format!(
        "{}\n❯\u{a0}\n{}\n",
        rule(5),
        rule(5)
    )));
    let f = Fake::new();
    f.screen("%1", &fixture("draft-typed-rule"));
    refused(&f, &url("model"), Refusal::NotEmpty);
}

#[test]
fn zero_padded_reset_ends_dim() {
    for reset in ["00", "000", "0;00"] {
        let body = format!("❯ \x1b[2mTry\x1b[{reset}mtyped");
        assert!(!box_is_empty(&boxed(&body)), "{reset}");
    }
    assert!(box_is_empty(&boxed("❯ \x1b[02mTry \"x\"\x1b[00m")));
}

#[test]
fn reverse_video_inside_dim_refused() {
    assert!(!box_is_empty(&boxed(
        "❯ \x1b[2m\x1b[7mx\x1b[27mTry\x1b[22m"
    )));
    assert!(!box_is_empty(&boxed("❯ \x1b[2;7mx\x1b[0m")));
    assert!(!box_is_empty(&boxed("❯ \x1b[7m\x1b[2mx\x1b[0m")));
    assert!(box_is_empty(&boxed("❯ \x1b[2m\x1b[7m\x1b[27mTry\x1b[22m")));
}

/// Real top border and marker, then a typed rule and a typed marker that
/// would pose as a second, empty box above the real bottom border.
#[test]
fn typed_marker_after_typed_rule_refused() {
    let spoof = format!(
        "{}\n❯\u{a0}\n{}\n❯\u{a0}\n{}\n  \x1b[2m? for shortcuts\x1b[22m\n",
        rule(60),
        "─".repeat(60),
        rule(60)
    );
    assert!(!box_is_empty(&spoof));
    assert!(!box_is_empty(&fixture("draft-typed-box")));
    // A marker below the box (a menu or dialog) is ambiguous too.
    assert!(!box_is_empty(&format!("{}\n ❯ 1. Yes\n", boxed("❯\u{a0}"))));
    let f = Fake::new();
    f.screen("%1", &fixture("draft-typed-box"));
    refused(&f, &url("model"), Refusal::NotEmpty);
}

#[test]
fn typed_marker_after_shifted_typed_rule_refused() {
    // Draft `x`, `foo`, a rule, `❯`: the fake box sits lines below the real top.
    for indent in ["", "  "] {
        let body = format!(
            "❯\u{a0}x\n{indent}foo\n{indent}{}\n{indent}❯",
            "─".repeat(60)
        );
        assert!(!box_is_empty(&boxed(&body)), "{indent:?}");
    }
}

#[test]
fn draft_of_n_lines_ending_in_typed_box_refused() {
    for n in [1, 3, 10] {
        let mut body = String::from("❯\u{a0}first");
        for k in 0..n {
            body.push_str(&format!("\nline {k}"));
        }
        body.push_str(&format!("\n{}\n❯\u{a0}", "─".repeat(60)));
        assert!(!box_is_empty(&boxed(&body)), "{n}");
    }
}
