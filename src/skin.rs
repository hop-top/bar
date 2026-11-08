//! Skins: segments painted on background colours, joined by Powerline
//! arrows or wrapped in capsules. Every separator glyph is one cell.

use crate::conf::{Skin, SkinGlyphs};
use crate::layout::Group;
use crate::options::Options;
use crate::style::{bg_code, fg, fg_code, muted};

/// A rendered segment with its visible width and background.
pub struct Seg {
    pub text: String,
    pub width: usize,
    pub bg: u8,
}

/// The skins that paint backgrounds; `plain` is drawn by the layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    Powerline,
    Capsule,
}

impl Paint {
    pub fn of(skin: Skin) -> Option<Paint> {
        match skin {
            Skin::Plain => None,
            Skin::Powerline => Some(Paint::Powerline),
            Skin::Capsule => Some(Paint::Capsule),
        }
    }
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
    right_thin: char,
    left_thin: char,
    cap_open: char,
    cap_close: char,
}

fn glyphs(set: SkinGlyphs) -> Glyphs {
    match set {
        SkinGlyphs::Nerd => Glyphs {
            right_arrow: '\u{e0b0}',
            left_arrow: '\u{e0b2}',
            right_thin: '\u{e0b1}',
            left_thin: '\u{e0b3}',
            cap_open: '\u{e0b6}',
            cap_close: '\u{e0b4}',
        },
        SkinGlyphs::Ascii => Glyphs {
            right_arrow: '>',
            left_arrow: '<',
            right_thin: '|',
            left_thin: '|',
            cap_open: '(',
            cap_close: ')',
        },
    }
}

fn fg_glyph(code: u8, g: char) -> String {
    fg(code, &g.to_string())
}

/// Whether an SGR parameter list (the text between `ESC [` and `m`) resets
/// the background: a `0` or empty parameter, or `49`. Colour arguments of
/// `38`/`48`/`58` are skipped, so `38;5;0` is a colour, not a reset.
fn resets_bg(params: &str) -> bool {
    let p: Vec<&str> = params.split(';').collect();
    let mut i = 0;
    while i < p.len() {
        match p[i].trim_start_matches('0') {
            "" | "49" => return true,
            "38" | "48" | "58" => {
                i += match p.get(i + 1).copied() {
                    Some("5") => 3,
                    Some("2") => 5,
                    _ => 1,
                };
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// Segment text whose resets put the background back, so a styled value
/// ending mid-segment leaves no hole.
fn keep_bg(text: &str, bg: u8) -> String {
    let b = bg_code(bg);
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("\x1b[") {
        out.push_str(&rest[..at]);
        let seq = &rest[at..];
        let end = seq[2..]
            .find(|c: char| ('\x40'..='\x7e').contains(&c))
            .map(|e| e + 2);
        let Some(end) = end else {
            out.push_str(seq);
            return out;
        };
        let sgr = seq.as_bytes()[end] == b'm';
        if sgr && &seq[2..end] == "49" {
            // a bare background reset becomes the segment background
            out.push_str(&b);
        } else {
            out.push_str(&seq[..=end]);
            if sgr && resets_bg(&seq[2..end]) {
                out.push_str(&b);
            }
        }
        rest = &seq[end + 1..];
    }
    out.push_str(rest);
    out
}

/// One group's segments painted with `paint`. Neighbours sharing a
/// background are split by a thin muted separator instead of an arrow.
pub fn paint(paint: Paint, set: SkinGlyphs, segs: &[Seg], side: Side) -> Group {
    if segs.is_empty() {
        return Group::default();
    }
    let g = glyphs(set);
    let thin = |c: char| fg_glyph(muted(), c);
    let same = |i: usize| i > 0 && segs[i - 1].bg == segs[i].bg;
    let mut text = String::new();
    let content: usize = segs.iter().map(|s| s.width + 2).sum();
    let width = match (paint, side) {
        (Paint::Capsule, _) => {
            for (i, s) in segs.iter().enumerate() {
                if i > 0 {
                    text.push(' ');
                }
                text.push_str(&format!("{}{}", fg_code(s.bg), g.cap_open));
                text.push_str(&bg_code(s.bg));
                text.push_str("\x1b[39m");
                text.push_str(&keep_bg(&s.text, s.bg));
                text.push_str("\x1b[49m");
                text.push_str(&fg_glyph(s.bg, g.cap_close));
            }
            content + segs.len() - 1
        }
        (Paint::Powerline, Side::Right) => {
            // the arrow points left, coloured as the segment it opens
            for (i, s) in segs.iter().enumerate() {
                if same(i) {
                    text.push_str(&thin(g.left_thin));
                } else {
                    text.push_str(&fg_glyph(s.bg, g.left_arrow));
                    text.push_str(&bg_code(s.bg));
                }
                text.push_str(&format!(" {} ", keep_bg(&s.text, s.bg)));
            }
            text.push_str("\x1b[49m");
            content + segs.len()
        }
        (Paint::Powerline, _) => {
            for (i, s) in segs.iter().enumerate() {
                if i == 0 {
                    text.push_str(&bg_code(s.bg));
                } else if same(i) {
                    text.push_str(&thin(g.right_thin));
                } else {
                    text.push_str(&fg_code(segs[i - 1].bg));
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
