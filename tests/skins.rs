use hop_bar::conf::{parse, Config, LoadResult, Skin, SkinGlyphs};
use hop_bar::palette;
use hop_bar::skin::{paint, Paint, Seg, Side};
use hop_bar::style::{strip_codes, visible_width_with};

/// Layout the plain goldens in `tests/fixtures/skins` were captured with,
/// before skins existed.
const L: &str = "set left \"#{model}#{effort} │ #{ctx} │ #{repo}:#{branch} │ #{prs[limit=3,author=@me]} │ #{issues[limit=3,assignee=@me]}\"\nset center \"#{dirty} │ #{ahead}\"\nset right \"#{account}\"\n";
const EXTRAS: [(&str, &str); 3] = [
    ("base", ""),
    ("icons", "set icons nerdfont\nset icon-width 2\n"),
    (
        "emoji",
        "set icons emoji\nset right \"#{agents} │ #{account}\"\n",
    ),
];
const WIDTHS: [usize; 5] = [200, 120, 80, 50, 20];

fn preview(conf: &str, cols: usize) -> String {
    let c = parse(conf).unwrap_or_else(|e| panic!("{conf}: {e:?}"));
    hop_bar::preview::render("early", LoadResult::Ok(c), Some(cols), 0)
}

fn golden(name: &str, cols: usize) -> String {
    let p = format!(
        "{}/tests/fixtures/skins/plain-{name}-{cols}.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(p).unwrap()
}

/// Cells on screen: skin glyphs one cell, Nerd Font icons `iw` cells.
fn cells(line: &str, iw: usize) -> usize {
    let one: String = line
        .chars()
        .map(|c| match c {
            '\u{e0b0}' | '\u{e0b2}' | '\u{e0b4}' | '\u{e0b6}' => '>',
            c => c,
        })
        .collect();
    visible_width_with(&one, iw)
}

fn seg(text: &str, bg: u8) -> Seg {
    Seg {
        text: text.into(),
        width: strip_codes(text).chars().count(),
        bg: bg.into(),
    }
}

// --- config ---

#[test]
fn skin_keys_parse() {
    let d = Config::default();
    assert_eq!((d.skin, d.skin_glyphs), (Skin::Plain, SkinGlyphs::Nerd));
    for (v, want) in [
        ("plain", Skin::Plain),
        ("powerline", Skin::Powerline),
        ("capsule", Skin::Capsule),
    ] {
        assert_eq!(parse(&format!("set skin {v}")).unwrap().skin, want);
    }
    let a = parse("set skin-glyphs ascii").unwrap();
    assert_eq!(a.skin_glyphs, SkinGlyphs::Ascii);
    assert!(parse("set skin fancy").is_err());
    assert!(parse("set skin-glyphs emoji").is_err());
}

#[test]
fn every_palette_parses() {
    for name in palette::NAMES {
        let c = parse(&format!("set palette {name}")).unwrap();
        let p = palette::by_name(name).unwrap();
        assert_eq!(c.muted, p.muted, "{name}");
        assert_eq!(
            (c.colors.good, c.colors.warn, c.colors.bad),
            (p.good.into(), p.warn.into(), p.bad.into()),
            "{name}"
        );
        assert_eq!(c.ramp, p.ramp, "{name}");
        assert_eq!(c.bgs, p.bgs, "{name}");
        assert!(!p.bgs.is_empty(), "{name}");
    }
    assert!(parse("set palette neon").is_err());
}

#[test]
fn default_palette_is_todays_colours() {
    let d = Config::default();
    let p = parse("set palette default").unwrap();
    assert_eq!(d.muted, 250);
    assert_eq!((p.colors, p.ramp, p.muted), (d.colors, d.ramp, d.muted));
}

#[test]
fn explicit_colours_beat_palette_in_any_order() {
    let nord = palette::by_name("nord").unwrap();
    let ramp = "set ramp \"1 2 3 4 5 6 7 8 9 10 11\"";
    for src in [
        format!("set color.good 1\n{ramp}\nset palette nord"),
        format!("set palette nord\nset color.good 1\n{ramp}"),
    ] {
        let c = parse(&src).unwrap();
        assert_eq!(c.colors.good, 1);
        assert_eq!(
            (c.colors.warn, c.colors.bad),
            (nord.warn.into(), nord.bad.into())
        );
        assert_eq!(c.ramp, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
        assert_eq!(c.muted, nord.muted);
    }
}

// --- plain is byte-identical ---

#[test]
fn plain_default_is_byte_identical() {
    let explicit = "set skin plain\nset skin-glyphs nerd\nset palette default\n";
    for (name, extra) in EXTRAS {
        for cols in WIDTHS {
            let want = golden(name, cols);
            assert_eq!(preview(&format!("{L}{extra}"), cols), want, "{name} {cols}");
            let set = preview(&format!("{L}{extra}{explicit}"), cols);
            assert_eq!(set, want, "{name} {cols} explicit");
        }
    }
}

#[test]
fn plain_ignores_bg_option() {
    let with_bg = L.replace("#{ctx}", "#{ctx[bg=200]}");
    assert_eq!(preview(&with_bg, 120), golden("base", 120));
}

// --- painting ---

#[test]
fn powerline_left_and_center_transitions() {
    let segs = [seg("a", 1), seg("b", 2)];
    let want = "\x1b[48;5;1m a \x1b[38;5;1m\x1b[48;5;2m\u{e0b0}\x1b[39m b \x1b[49m\x1b[38;5;2m\u{e0b0}\x1b[39m";
    for side in [Side::Left, Side::Center] {
        let g = paint(Paint::Powerline, SkinGlyphs::Nerd, &segs, side);
        assert_eq!(g.text, want);
        assert_eq!(g.width, 8);
    }
}

#[test]
fn powerline_right_transitions() {
    let segs = [seg("a", 1), seg("b", 2)];
    let g = paint(Paint::Powerline, SkinGlyphs::Nerd, &segs, Side::Right);
    let want = "\x1b[38;5;1m\u{e0b2}\x1b[39m\x1b[48;5;1m a \x1b[38;5;2m\u{e0b2}\x1b[39m\x1b[48;5;2m b \x1b[49m";
    assert_eq!(g.text, want);
    assert_eq!(g.width, 8);
}

#[test]
fn capsule_wraps_each_segment() {
    let segs = [seg("a", 1), seg("b", 2)];
    let cap = |bg: u8, t: &str, l: char, r: char| {
        format!("\x1b[38;5;{bg}m{l}\x1b[48;5;{bg}m\x1b[39m{t}\x1b[49m\x1b[38;5;{bg}m{r}\x1b[39m")
    };
    for side in [Side::Left, Side::Center, Side::Right] {
        let g = paint(Paint::Capsule, SkinGlyphs::Nerd, &segs, side);
        let want = format!(
            "{} {}",
            cap(1, "a", '\u{e0b6}', '\u{e0b4}'),
            cap(2, "b", '\u{e0b6}', '\u{e0b4}')
        );
        assert_eq!(g.text, want);
        assert_eq!(g.width, 7);
    }
    let g = paint(Paint::Capsule, SkinGlyphs::Ascii, &segs[..1], Side::Left);
    assert_eq!(g.text, cap(1, "a", '(', ')'));
}

#[test]
fn ascii_powerline_glyphs() {
    let segs = [seg("a", 1), seg("b", 2)];
    let l = paint(Paint::Powerline, SkinGlyphs::Ascii, &segs, Side::Left);
    let r = paint(Paint::Powerline, SkinGlyphs::Ascii, &segs, Side::Right);
    assert_eq!(strip_codes(&l.text), " a > b >");
    assert_eq!(strip_codes(&r.text), "< a < b ");
    assert!(!l.text.chars().chain(r.text.chars()).any(|c| c > '\u{e000}'));
    assert_eq!((l.width, r.width), (8, 8));
}

#[test]
fn empty_group_paints_nothing() {
    for skin in [Paint::Powerline, Paint::Capsule] {
        for side in [Side::Left, Side::Center, Side::Right] {
            let g = paint(skin, SkinGlyphs::Nerd, &[], side);
            assert_eq!((g.text.as_str(), g.width), ("", 0));
        }
    }
}

#[test]
fn no_bg_bleed_inside_segment() {
    let text = "\x1b[38;5;114mok\x1b[39m \x1b[0mx\x1b[m y\x1b[49mz";
    let s = Seg {
        text: text.into(),
        width: 7,
        bg: 5.into(),
    };
    for skin in [Paint::Powerline, Paint::Capsule] {
        let g = paint(skin, SkinGlyphs::Nerd, std::slice::from_ref(&s), Side::Left);
        assert!(
            g.text.contains("\x1b[0m\x1b[48;5;5mx"),
            "{skin:?}: {:?}",
            g.text
        );
        assert!(
            g.text.contains("\x1b[m\x1b[48;5;5m y"),
            "{skin:?}: {:?}",
            g.text
        );
        assert!(g.text.contains("y\x1b[48;5;5mz"), "{skin:?}: {:?}", g.text);
        // 49 only where the segment ends
        let ends = g.text.matches("\x1b[49m").count();
        assert_eq!(ends, 1, "{skin:?}: {:?}", g.text);
        let tail = &g.text[g.text.find("\x1b[49m").unwrap()..];
        assert!(!tail.contains('z'), "{skin:?}: {:?}", g.text);
    }
}

// --- through the renderer ---

#[test]
fn powerline_line_uses_palette_bgs_and_override() {
    let mono = palette::by_name("mono").unwrap();
    let conf = "set skin powerline\nset palette mono\nset left \"#{model} │ #{ctx[bg=200]} │ #{repo}\"\nset right \"#{account}\"\n";
    let out = preview(conf, 120);
    let bg = |b: u8| format!("\x1b[48;5;{b}m");
    let (a, b, c) = (bg(mono.bgs[0]), bg(200), bg(mono.bgs[2]));
    let (ia, ib, ic) = (
        out.find(&a).unwrap(),
        out.find(&b).unwrap(),
        out.find(&c).unwrap(),
    );
    assert!(ia < ib && ib < ic, "{out:?}");
    // right group continues the cycle: fourth segment
    assert!(
        out.contains(&format!("\x1b[38;5;{}m\u{e0b2}", mono.bgs[3])),
        "{out:?}"
    );
    assert!(out.ends_with("\x1b[49m"), "{out:?}");
}

#[test]
fn bg_cycles_through_palette() {
    let p = palette::by_name("nord").unwrap();
    let n = p.bgs.len();
    let left = (0..=n).map(|_| "#{model}").collect::<Vec<_>>().join(" │ ");
    let out = preview(
        &format!("set skin capsule\nset palette nord\nset left \"{left}\""),
        200,
    );
    let first = format!("\x1b[48;5;{}m", p.bgs[0]);
    assert_eq!(out.matches(&first).count(), 2, "{out:?}");
}

#[test]
fn palette_muted_applies_to_dim_text() {
    let nord = palette::by_name("nord").unwrap();
    let out = preview(&format!("{L}set palette nord\n"), 200);
    assert!(
        out.contains(&format!("\x1b[38;5;{}m", nord.muted)),
        "{out:?}"
    );
    assert!(!out.contains("\x1b[38;5;250m"), "{out:?}");
    // a later default render is unaffected
    assert_eq!(preview(L, 200), golden("base", 200));
}

#[test]
fn skinned_width_fits_columns() {
    for skin in ["powerline", "capsule"] {
        let conf = format!("{L}set icons nerdfont\nset icon-width 2\nset skin {skin}\n");
        let wide = preview(&conf, 200);
        assert!(!wide.contains('\n'), "{skin}: {wide:?}");
        assert_eq!(cells(&wide, 2), 196, "{skin}: {wide:?}");
        let narrow = preview(&conf, 50);
        let lines: Vec<&str> = narrow.lines().collect();
        assert_eq!(lines.len(), 2, "{skin}: {narrow:?}");
        // left and right do not fit beside each other: joined by one space
        assert!(!strip_codes(lines[0]).contains("  "), "{skin}: {narrow:?}");
        let mid = preview(&conf, 120);
        for l in mid.lines() {
            assert!(cells(l, 2) <= 116, "{skin}: {mid:?}");
        }
    }
}

#[test]
fn compound_resets_restore_bg() {
    let s = Seg {
        text: "\x1b[0;1mx\x1b[22;49my\x1b[1;38;5;2mz\x1b[;1mw".into(),
        width: 4,
        bg: 5.into(),
    };
    let g = paint(
        Paint::Powerline,
        SkinGlyphs::Nerd,
        std::slice::from_ref(&s),
        Side::Left,
    );
    assert!(g.text.contains("\x1b[0;1m\x1b[48;5;5mx"), "{:?}", g.text);
    assert!(g.text.contains("\x1b[22;49m\x1b[48;5;5my"), "{:?}", g.text);
    assert!(g.text.contains("\x1b[1;38;5;2mz"), "{:?}", g.text);
    assert!(!g.text.contains("\x1b[1;38;5;2m\x1b[48"), "{:?}", g.text);
    assert!(g.text.contains("\x1b[;1m\x1b[48;5;5mw"), "{:?}", g.text);
}

#[test]
fn same_bg_neighbours_get_thin_separator() {
    let segs = [seg("a", 7), seg("b", 7), seg("c", 8)];
    let muted = hop_bar::style::MUTED;
    let l = paint(Paint::Powerline, SkinGlyphs::Nerd, &segs, Side::Left);
    let thin = format!("\x1b[38;5;{muted}m\u{e0b1}\x1b[39m");
    assert!(
        l.text.starts_with(&format!("\x1b[48;5;7m a {thin} b ")),
        "{:?}",
        l.text
    );
    assert!(
        l.text.contains("\x1b[38;5;7m\x1b[48;5;8m\u{e0b0}"),
        "{:?}",
        l.text
    );
    assert_eq!(l.width, 12);
    let r = paint(Paint::Powerline, SkinGlyphs::Nerd, &segs, Side::Right);
    let thin_r = format!("\x1b[38;5;{muted}m\u{e0b3}\x1b[39m");
    assert!(r.text.contains(&format!(" a {thin_r} b ")), "{:?}", r.text);
    assert_eq!(r.width, 12);
    let a = paint(Paint::Powerline, SkinGlyphs::Ascii, &segs, Side::Left);
    assert_eq!(strip_codes(&a.text), " a | b > c >");
    let ar = paint(Paint::Powerline, SkinGlyphs::Ascii, &segs, Side::Right);
    assert_eq!(strip_codes(&ar.text), "< a | b < c ");
}
