//! Skins: segments painted on background colours, joined by Powerline
//! arrows or wrapped in capsules. Every separator glyph is one cell.

use crate::conf::{Skin, SkinGlyphs};
use crate::layout::{join, Group};
use crate::options::Options;
use crate::style::bg_code;

/// A rendered segment with its visible width and background.
pub struct Seg {
    pub text: String,
    pub width: usize,
    pub bg: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Center,
    Right,
}

/// The `bg` option split off; the placeholder never sees it. A value that
/// is not 0-255 is dropped.
pub fn split_bg(opts: &Options) -> (Option<u8>, Options) {
    let mut rest = Options::default();
    let mut bg = None;
    for (k, v) in opts.iter() {
        if k == "bg" {
            bg = v.trim().parse().ok();
        } else {
            rest.set(k, v);
        }
    }
    (bg, rest)
}

/// Whether `raw` is acceptable as `bg=`.
pub fn valid_bg(raw: &str) -> bool {
    raw.trim().parse::<u8>().is_ok()
}

struct Glyphs {
    right_arrow: char,
    left_arrow: char,
    cap_open: char,
    cap_close: char,
}

fn glyphs(set: SkinGlyphs) -> Glyphs {
    match set {
        SkinGlyphs::Nerd => Glyphs {
            right_arrow: '\u{e0b0}',
            left_arrow: '\u{e0b2}',
            cap_open: '\u{e0b6}',
            cap_close: '\u{e0b4}',
        },
        SkinGlyphs::Ascii => Glyphs {
            right_arrow: '>',
            left_arrow: '<',
            cap_open: '(',
            cap_close: ')',
        },
    }
}

fn fg_glyph(code: u8, g: char) -> String {
    format!("\x1b[38;5;{code}m{g}\x1b[39m")
}

/// Segment text whose resets put the background back, so a styled value
/// ending mid-segment leaves no hole.
fn keep_bg(text: &str, bg: u8) -> String {
    let b = bg_code(bg);
    text.replace("\x1b[49m", &b)
        .replace("\x1b[0m", &format!("\x1b[0m{b}"))
        .replace("\x1b[00m", &format!("\x1b[00m{b}"))
        .replace("\x1b[m", &format!("\x1b[m{b}"))
}

/// One group's segments drawn with `skin`; plain joins them with ` │ `.
pub fn paint(skin: Skin, set: SkinGlyphs, segs: &[Seg], side: Side) -> Group {
    if segs.is_empty() {
        return Group::default();
    }
    let g = glyphs(set);
    let mut text = String::new();
    let content: usize = segs.iter().map(|s| s.width + 2).sum();
    let width = match (skin, side) {
        (Skin::Plain, _) => {
            let texts: Vec<String> = segs.iter().map(|s| s.text.clone()).collect();
            let joined = segs.iter().map(|s| s.width).sum::<usize>() + 3 * (segs.len() - 1);
            return Group {
                text: join(&texts),
                width: joined,
            };
        }
        (Skin::Capsule, _) => {
            for (i, s) in segs.iter().enumerate() {
                if i > 0 {
                    text.push(' ');
                }
                text.push_str(&format!("\x1b[38;5;{}m{}", s.bg, g.cap_open));
                text.push_str(&bg_code(s.bg));
                text.push_str("\x1b[39m");
                text.push_str(&keep_bg(&s.text, s.bg));
                text.push_str("\x1b[49m");
                text.push_str(&fg_glyph(s.bg, g.cap_close));
            }
            content + segs.len() - 1
        }
        (Skin::Powerline, Side::Right) => {
            // the arrow points left, coloured as the segment it opens
            for s in segs {
                text.push_str(&fg_glyph(s.bg, g.left_arrow));
                text.push_str(&bg_code(s.bg));
                text.push_str(&format!(" {} ", keep_bg(&s.text, s.bg)));
            }
            text.push_str("\x1b[49m");
            content + segs.len()
        }
        (Skin::Powerline, _) => {
            for (i, s) in segs.iter().enumerate() {
                if i == 0 {
                    text.push_str(&bg_code(s.bg));
                } else {
                    text.push_str(&format!("\x1b[38;5;{}m", segs[i - 1].bg));
                    text.push_str(&bg_code(s.bg));
                    text.push(g.right_arrow);
                    text.push_str("\x1b[39m");
                }
                text.push_str(&format!(" {} ", keep_bg(&s.text, s.bg)));
            }
            text.push_str("\x1b[49m");
            text.push_str(&fg_glyph(segs[segs.len() - 1].bg, g.right_arrow));
            content + segs.len()
        }
    };
    Group { text, width }
}
