//! Text a skin paints on a segment background stays readable: each
//! palette's text colours against every background in its list, as
//! `powerline` and `capsule` both draw them, at 256 colours (hex values
//! quantised) and at truecolor. Covers `nord` and every palette added after
//! the first six; `default`, `gruvbox`, `dracula`, `solarized` and `mono`
//! keep their original colours, and `terminal` is drawn by the terminal
//! theme.
//!
//! Floor: 3:1, the WCAG level for large text and graphics. Status-line
//! segments are short tokens read at a glance, and in the 256-colour cube a
//! 4.5:1 floor for red text leaves only near-black backgrounds. The usage
//! ramp is left out: it runs from pure red to pure green by design, and the
//! number beside the bar keeps the neutral colour.

use hop_bar::color::{Color, Depth};
use hop_bar::palette::{self, Palette};
use hop_bar::style::MUTED;

/// Text colours every palette shares: values with no colour of their own
/// (`style::neutral`) and repo names (`style::accent`).
const NEUTRAL: u8 = MUTED;
const ACCENT: u8 = 75;
const FLOOR: f64 = 3.0;

/// Palettes outside the floor: their original colours, and `terminal`.
const EXEMPT: [&str; 6] = [
    "default",
    "gruvbox",
    "dracula",
    "solarized",
    "mono",
    "terminal",
];

/// The colour as drawn at `depth`, as RGB.
fn drawn(c: Color, depth: Depth) -> [u8; 3] {
    match c.at(depth) {
        Color::Idx(n) => rgb(n),
        Color::Rgb(v) => v,
    }
}

/// The xterm 256-colour palette entry for `code` (16 and up).
fn rgb(code: u8) -> [u8; 3] {
    match code {
        0..=15 => panic!("ANSI colour {code} depends on the terminal theme"),
        16..=231 => {
            let c = code - 16;
            let level = |n: u8| if n == 0 { 0 } else { 55 + 40 * n };
            [level(c / 36), level(c / 6 % 6), level(c % 6)]
        }
        _ => {
            let v = 8 + (code - 232) * 10;
            [v, v, v]
        }
    }
}

/// WCAG relative luminance.
fn luminance([r, g, b]: [u8; 3]) -> f64 {
    let ch = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.039_28 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
}

fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn text_colours(p: &Palette) -> [(&'static str, Color); 6] {
    [
        ("muted", p.muted),
        ("good", p.good),
        ("warn", p.warn),
        ("bad", p.bad),
        ("neutral", Color::Idx(NEUTRAL)),
        ("accent", Color::Idx(ACCENT)),
    ]
}

fn covered() -> impl Iterator<Item = &'static Palette> {
    palette::PALETTES
        .iter()
        .filter(|p| !EXEMPT.contains(&p.name.as_ref()))
}

#[test]
fn xterm_colours_and_contrast() {
    assert_eq!(rgb(16), [0, 0, 0]);
    assert_eq!(rgb(67), [0x5f, 0x87, 0xaf]);
    assert_eq!(rgb(231), [255, 255, 255]);
    assert_eq!(rgb(248), [0xa8, 0xa8, 0xa8]);
    assert!((contrast(rgb(16), rgb(231)) - 21.0).abs() < 0.01);
    // nord's old muted text on its old frost background
    assert!((contrast(rgb(248), rgb(67)) - 1.59).abs() < 0.01);
}

#[test]
fn nord_and_the_added_palettes_are_covered() {
    let names: Vec<&str> = covered().map(|p| p.name.as_ref()).collect();
    assert!(names.contains(&"nord"), "{names:?}");
    assert_eq!(names.len(), palette::PALETTES.len() - EXEMPT.len());
}

#[test]
fn segment_text_meets_the_contrast_floor() {
    let mut low = Vec::new();
    for p in covered() {
        for depth in [Depth::Ansi256, Depth::True] {
            for &bg in p.bgs.iter() {
                for (what, fg) in text_colours(p) {
                    let r = contrast(drawn(fg, depth), drawn(bg, depth));
                    if r < FLOOR {
                        low.push(format!("{} {depth:?}: {what} {fg} on {bg}: {r:.2}", p.name));
                    }
                }
            }
        }
    }
    assert!(low.is_empty(), "below {FLOOR}:1\n{}", low.join("\n"));
}

#[test]
fn neighbouring_backgrounds_differ() {
    for p in palette::PALETTES.iter() {
        for depth in [Depth::Ansi256, Depth::True] {
            let bgs: Vec<Color> = p.bgs.iter().map(|b| b.at(depth)).collect();
            for w in bgs.windows(2) {
                assert_ne!(w[0], w[1], "{} {depth:?}", p.name);
            }
            let (first, last) = (bgs[0], bgs[bgs.len() - 1]);
            assert!(bgs.len() < 2 || first != last, "{} {depth:?}", p.name);
        }
    }
}
