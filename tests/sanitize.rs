//! `style::sanitize`: line breaks and tabs read as spaces; invisible
//! format characters dropped; emoji sequences kept.

use hop_bar::style::sanitize;

#[test]
fn line_breaks_and_tabs_become_spaces() {
    assert_eq!(sanitize("Reading\nsrc/conf.rs"), "Reading src/conf.rs");
    assert_eq!(sanitize("a\tb\r\nc"), "a b  c");
    assert_eq!(
        sanitize("a\x1b[31mb\x07c"),
        "a[31mbc",
        "other controls still dropped"
    );
}

#[test]
fn format_characters_are_dropped() {
    let hidden = "a\u{200B}b\u{200E}c\u{200F}d\u{FEFF}e\u{061C}f\u{00AD}g\u{2060}h\u{2064}i\u{180E}j\u{FFF9}k\u{E0001}l";
    assert_eq!(sanitize(hidden), "abcdefghijkl");
}

#[test]
fn emoji_sequences_survive() {
    // family (ZWJ), keycap and heart with VS16, England flag (tags)
    for e in [
        "👨\u{200D}👩\u{200D}👧",
        "1\u{FE0F}\u{20E3}",
        "❤\u{FE0F}",
        "🏴\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}",
        "✏️",
    ] {
        assert_eq!(sanitize(e), e, "{e:?}");
    }
}
