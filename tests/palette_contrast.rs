//! Text a skin paints on a segment background stays readable: every
//! palette's text colours against every background in its list.
//!
//! Floor: 3:1, the WCAG level for large text and graphics. Status-line
//! segments are short tokens read at a glance, and in the 256-colour cube a
//! 4.5:1 floor for red text leaves only near-black backgrounds. The usage
//! ramp is left out: it runs from pure red to pure green by design, and the
//! number beside the bar keeps the neutral colour. `terminal` is left out
//! too: the terminal theme decides its colours.

use hop_bar::palette::{self, Palette};
use hop_bar::style::MUTED;

/// Text colours every palette shares: values with no colour of their own
/// (`style::neutral`) and repo names (`style::accent`).
const NEUTRAL: u8 = MUTED;
const ACCENT: u8 = 75;
const FLOOR: f64 = 3.0;

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
fn luminance(code: u8) -> f64 {
    let ch = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.039_28 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    let [r, g, b] = rgb(code);
    0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
}

fn contrast(a: u8, b: u8) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn text_colours(p: &Palette) -> [(&'static str, u8); 6] {
    [
        ("muted", p.muted),
        ("good", p.good),
        ("warn", p.warn),
        ("bad", p.bad),
        ("neutral", NEUTRAL),
        ("accent", ACCENT),
    ]
}

#[test]
fn xterm_colours_and_contrast() {
    assert_eq!(rgb(16), [0, 0, 0]);
    assert_eq!(rgb(67), [0x5f, 0x87, 0xaf]);
    assert_eq!(rgb(231), [255, 255, 255]);
    assert_eq!(rgb(248), [0xa8, 0xa8, 0xa8]);
    assert!((contrast(16, 231) - 21.0).abs() < 0.01);
    // nord's old muted text on its old frost background
    assert!((contrast(248, 67) - 1.59).abs() < 0.01);
}

#[test]
fn segment_text_meets_the_contrast_floor() {
    let mut low = Vec::new();
    for p in palette::PALETTES.iter().filter(|p| !p.ansi) {
        for &bg in p.bgs {
            for (what, fg) in text_colours(p) {
                let r = contrast(fg, bg);
                if r < FLOOR {
                    low.push(format!("{} {what} {fg} on {bg}: {r:.2}", p.name));
                }
            }
        }
    }
    assert!(low.is_empty(), "below {FLOOR}:1\n{}", low.join("\n"));
}

#[test]
fn neighbouring_backgrounds_differ() {
    for p in palette::PALETTES.iter() {
        for w in p.bgs.windows(2) {
            assert_ne!(w[0], w[1], "{}", p.name);
        }
        let (first, last) = (p.bgs[0], p.bgs[p.bgs.len() - 1]);
        assert!(p.bgs.len() < 2 || first != last, "{}", p.name);
    }
}
