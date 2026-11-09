//! Gradients interpolated in OKLab, and recolouring rendered text cluster
//! by cluster without touching its escape sequences, links or width.

use crate::color::{Color, Depth};
use crate::style::{depth, plain_width, push_fg};
use unicode_segmentation::UnicodeSegmentation;

/// Named gradients: name and stops.
pub const PRESETS: [(&str, &[[u8; 3]]); 6] = [
    ("ember", &[[0xd7, 0x26, 0x3d], [0xf4, 0x9d, 0x37]]),
    ("lagoon", &[[0x1b, 0x99, 0x8b], [0xb8, 0xf2, 0xe6]]),
    ("dusk", &[[0x5f, 0x4b, 0xb6], [0xe0, 0x60, 0x7e]]),
    ("moss", &[[0x3a, 0x6b, 0x35], [0xc5, 0xd8, 0x6d]]),
    ("frost", &[[0x4a, 0x6f, 0xa5], [0xdb, 0xe9, 0xf6]]),
    ("ash", &[[0x5c, 0x5c, 0x5c], [0xd0, 0xd0, 0xd0]]),
];

/// Most stops a gradient takes.
pub const MAX_STOPS: usize = 8;

pub fn preset_names() -> impl Iterator<Item = &'static str> {
    PRESETS.iter().map(|(n, _)| *n)
}

/// Stops of a preset, or of `from:to` (2 to 8 colours split by `:`, each
/// `0`-`255`, `#rrggbb` or an ANSI name); `None` when invalid.
pub fn parse_stops(s: &str) -> Option<Vec<Color>> {
    let s = s.trim();
    if let Some((_, stops)) = PRESETS.iter().find(|(n, _)| *n == s) {
        return Some(stops.iter().map(|&c| Color::Rgb(c)).collect());
    }
    let stops: Option<Vec<Color>> = s.split(':').map(Color::parse).collect();
    stops.filter(|v| (2..=MAX_STOPS).contains(&v.len()))
}

fn to_linear(v: u8) -> f32 {
    let v = v as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

fn oklab(c: [u8; 3]) -> [f32; 3] {
    let [r, g, b] = c.map(to_linear);
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn from_oklab([l, a, b]: [f32; 3]) -> [u8; 3] {
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_,
        -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_,
        -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_,
    ]
    .map(to_srgb)
}

/// Evenly spaced stops in OKLab.
pub struct Gradient {
    stops: Vec<[f32; 3]>,
}

impl Gradient {
    pub fn new(stops: &[Color]) -> Gradient {
        Gradient {
            stops: stops.iter().map(|c| oklab(c.rgb())).collect(),
        }
    }

    /// The colour at `t` (0 to 1).
    pub fn at(&self, t: f32) -> Color {
        let n = self.stops.len();
        if n == 1 {
            return Color::Rgb(from_oklab(self.stops[0]));
        }
        let x = t.clamp(0.0, 1.0) * (n - 1) as f32;
        let i = (x as usize).min(n - 2);
        let f = x - i as f32;
        let (a, b) = (self.stops[i], self.stops[i + 1]);
        Color::Rgb(from_oklab([0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * f)))
    }
}

/// What text is recoloured with.
pub enum Ink<'a> {
    Solid(Color),
    /// A gradient along `extent`, the text starting at cell `start` of it;
    /// private-use glyphs are `icon_width` cells.
    Ramp {
        gradient: &'a Gradient,
        start: usize,
        extent: Extent,
        icon_width: usize,
    },
}

/// Where a gradient runs: its cells, and the centres of its first and last
/// visible clusters, which get the first and last stop.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Extent {
    pub cells: usize,
    first: f32,
    last: f32,
}

impl Extent {
    /// The extent of `texts` drawn one after the other.
    pub fn of<'a>(texts: impl IntoIterator<Item = &'a str>, icon_width: usize) -> Extent {
        let mut e = Extent::default();
        let mut seen = false;
        for text in texts {
            for w in cluster_cells(text, icon_width) {
                if w == 0 {
                    continue;
                }
                let centre = e.cells as f32 + w as f32 / 2.0;
                if !seen {
                    e.first = centre;
                    seen = true;
                }
                e.last = centre;
                e.cells += w;
            }
        }
        e
    }

    /// Where along the gradient (0 to 1) a cluster `w` cells wide at cell
    /// `pos` falls.
    fn t(&self, pos: usize, w: usize) -> f32 {
        if self.last > self.first {
            (pos as f32 + w as f32 / 2.0 - self.first) / (self.last - self.first)
        } else {
            0.0
        }
    }
}

/// Visible cells of `text`, escape sequences skipped, as the layout counts
/// them (`style::visible_width_with`).
pub fn span(text: &str, icon_width: usize) -> usize {
    cluster_cells(text, icon_width).sum()
}

/// Cells of each grapheme cluster of `text`, escape sequences skipped.
fn cluster_cells(text: &str, icon_width: usize) -> impl Iterator<Item = usize> + '_ {
    pieces(text)
        .filter_map(|p| match p {
            Piece::Text(t) => Some(t),
            Piece::Esc(_) => None,
        })
        .flat_map(|t| t.graphemes(true))
        .map(move |g| plain_width(g, icon_width))
}

enum Piece<'a> {
    /// A CSI or OSC sequence, or ESC and the character after it.
    Esc(&'a str),
    /// Text up to the next escape sequence.
    Text(&'a str),
}

/// The text as escape sequences and runs of text, in order.
fn pieces(text: &str) -> impl Iterator<Item = Piece<'_>> {
    let b = text.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        if i >= b.len() {
            return None;
        }
        let start = i;
        if b[i] != 0x1b {
            i = memchr::memchr(0x1b, &b[i..]).map_or(b.len(), |n| i + n);
            return Some(Piece::Text(&text[start..i]));
        }
        i += 1;
        match b.get(i) {
            Some(b'[') => {
                i += 1;
                while i < b.len() && !(0x40..=0x7e).contains(&b[i]) {
                    i += 1;
                }
                i = (i + 1).min(b.len());
            }
            Some(b']') => {
                i += 1;
                while i < b.len() && b[i] != 0x07 && !(b[i] == 0x1b && b.get(i + 1) == Some(&b'\\'))
                {
                    i += 1;
                }
                i = (i + if b.get(i) == Some(&0x07) { 1 } else { 2 }).min(b.len());
            }
            Some(_) => i += text[i..].chars().next().map_or(1, char::len_utf8),
            None => {}
        }
        Some(Piece::Esc(&text[start..i]))
    })
}

/// Whether `seq` is an SGR that only sets the foreground colour, so an ink
/// drawn over it makes it redundant.
fn fg_only(seq: &str) -> bool {
    let Some(params) = seq.strip_prefix("\x1b[").and_then(|s| s.strip_suffix('m')) else {
        return false;
    };
    let p: Vec<&str> = params.split(';').collect();
    let mut i = 0;
    while i < p.len() {
        i += match p[i] {
            "38" => match p.get(i + 1).copied() {
                Some("5") => 3,
                Some("2") => 5,
                _ => return false,
            },
            "39" => 1,
            x if matches!(x.parse::<u8>(), Ok(30..=37 | 90..=97)) => 1,
            _ => return false,
        };
    }
    i == p.len()
}

/// `text` with every visible cluster drawn in `ink`, if any, and bold when
/// `bold`. Escape sequences stay where they are, except foreground colours
/// the ink replaces; the ink is drawn again after each other CSI, so colours
/// inside the text give way to it. A grapheme cluster (emoji sequence, flag,
/// letter and combining marks) takes one colour.
pub fn recolor(text: &str, ink: Option<&Ink>, bold: bool) -> String {
    let d = depth();
    let ink = ink.filter(|_| d != Depth::None);
    if ink.is_none() && !bold {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut pos = 0usize;
    let mut fresh = true;
    let mut last: Option<Color> = None;
    let mut drew = false;
    for piece in pieces(text) {
        let run = match piece {
            Piece::Esc(seq) => {
                if ink.is_some() && fg_only(seq) {
                    continue;
                }
                out.push_str(seq);
                fresh |= seq.starts_with("\x1b[");
                continue;
            }
            Piece::Text(t) => t,
        };
        for g in run.graphemes(true) {
            let w = match ink {
                Some(Ink::Ramp { icon_width, .. }) => plain_width(g, *icon_width),
                _ => plain_width(g, 1),
            };
            if w > 0 {
                if fresh && bold {
                    out.push_str("\x1b[1m");
                }
                if let Some(ink) = ink {
                    let color = match ink {
                        Ink::Solid(c) => c.at(d),
                        Ink::Ramp {
                            gradient,
                            start,
                            extent,
                            ..
                        } => gradient.at(extent.t(start + pos, w)).at(d),
                    };
                    if fresh || last != Some(color) {
                        push_fg(&mut out, color);
                        last = Some(color);
                    }
                }
                fresh = false;
                drew = true;
                pos += w;
            }
            out.push_str(g);
        }
    }
    if drew {
        if ink.is_some() {
            out.push_str("\x1b[39m");
        }
        if bold {
            out.push_str("\x1b[22m");
        }
    }
    out
}
