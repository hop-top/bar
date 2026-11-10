//! SGR colouring, OSC 8 links, text sanitising and visible-width measuring.

use crate::color::{Color, Depth};
use std::fmt::Write;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn fg(code: impl Into<Color>, text: &str) -> String {
    format!("{}{text}\x1b[39m", fg_code(code))
}

/// The SGR that starts foreground `code`.
pub fn fg_code(code: impl Into<Color>) -> String {
    let mut s = String::with_capacity(12);
    push_sgr(&mut s, code.into(), false);
    s
}

/// Appends the SGR that starts foreground `c`.
pub fn push_fg(out: &mut String, c: Color) {
    push_sgr(out, c, false);
}

/// The SGR for `c` at the depth in effect: codes 0-15 as ANSI 16-colour
/// SGR under the terminal palette or at depth 16, RGB as `38;2` only at
/// truecolor, everything else quantised (`Color::at`).
fn push_sgr(out: &mut String, c: Color, bg: bool) {
    let d = depth();
    let c = c.at(d);
    let _ = match c {
        Color::Idx(n) if n < 16 && (d == Depth::Ansi16 || ANSI_NOW.with(std::cell::Cell::get)) => {
            let base = if bg { 40 } else { 30 };
            write!(
                out,
                "\x1b[{}m",
                if n < 8 { base + n } else { base + 52 + n }
            )
        }
        Color::Idx(n) if bg => write!(out, "\x1b[48;5;{n}m"),
        Color::Idx(n) => write!(out, "\x1b[38;5;{n}m"),
        Color::Rgb([r, g, b]) if bg => write!(out, "\x1b[48;2;{r};{g};{b}m"),
        Color::Rgb([r, g, b]) => write!(out, "\x1b[38;2;{r};{g};{b}m"),
    };
}

/// Accent for names (repo): 256-colour blue, or ANSI cyan under the terminal
/// palette.
pub fn accent() -> u8 {
    if ANSI_NOW.with(std::cell::Cell::get) {
        6
    } else {
        75
    }
}

/// Text colour for values with no configured colour: 250, or the muted
/// colour under the terminal palette.
pub fn neutral() -> Color {
    if ANSI_NOW.with(std::cell::Cell::get) {
        muted()
    } else {
        Color::Idx(250)
    }
}

pub fn bold(text: &str) -> String {
    format!("\x1b[1m{text}\x1b[22m")
}

/// Default muted text colour (256-colour palette); SGR 2 faint renders too
/// dark. A palette may pick another for the duration of a render.
pub const MUTED: u8 = 250;

thread_local! {
    static MUTED_NOW: std::cell::Cell<Color> = const { std::cell::Cell::new(Color::Idx(MUTED)) };
    static ANSI_NOW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static DEPTH_NOW: std::cell::Cell<Depth> = const { std::cell::Cell::new(Depth::Ansi256) };
}

/// Restores the previous depth when dropped.
pub struct DepthGuard(Depth);

impl Drop for DepthGuard {
    fn drop(&mut self) {
        DEPTH_NOW.with(|d| d.set(self.0));
    }
}

/// Draws colours at `depth` on this thread until the guard drops.
pub fn use_depth(depth: Depth) -> DepthGuard {
    DepthGuard(DEPTH_NOW.with(|d| d.replace(depth)))
}

/// The colour depth in effect on this thread.
pub fn depth() -> Depth {
    DEPTH_NOW.with(std::cell::Cell::get)
}

/// Restores the previous ANSI mode when dropped.
pub struct AnsiGuard(bool);

impl Drop for AnsiGuard {
    fn drop(&mut self) {
        ANSI_NOW.with(|a| a.set(self.0));
    }
}

/// Draws codes 0-15 as ANSI 16-colour SGR on this thread until the guard
/// drops (`on`), so the terminal theme picks the colours.
pub fn use_ansi(on: bool) -> AnsiGuard {
    AnsiGuard(ANSI_NOW.with(|a| a.replace(on)))
}

/// Restores the previous muted colour when dropped.
pub struct MutedGuard(Color);

impl Drop for MutedGuard {
    fn drop(&mut self) {
        MUTED_NOW.with(|m| m.set(self.0));
    }
}

/// Uses `code` for `dim` on this thread until the guard drops.
pub fn use_muted(code: Color) -> MutedGuard {
    MutedGuard(MUTED_NOW.with(|m| m.replace(code)))
}

/// The muted colour in effect on this thread.
pub fn muted() -> Color {
    MUTED_NOW.with(std::cell::Cell::get)
}

pub fn dim(text: &str) -> String {
    fg(muted(), text)
}

pub fn bg_code(code: impl Into<Color>) -> String {
    let mut s = String::with_capacity(12);
    push_sgr(&mut s, code.into(), true);
    s
}

/// OSC 8 hyperlink; no SGR underline, the terminal draws it. A URL holding
/// control characters could end the sequence early, so it yields plain text.
pub fn link(url: &str, text: &str) -> String {
    if url.chars().any(char::is_control) {
        return text.to_string();
    }
    format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
}

/// Picks `ramp[(score + 9) / 10]` with the score clamped to 0..=100.
pub fn ramp(score: i32, ramp: &[Color; 11]) -> Color {
    ramp[((score.clamp(0, 100) + 9) / 10) as usize]
}

/// Drops ESC, C0, DEL and C1 controls, line/paragraph separators and
/// format characters (Unicode `Cf`: bidi marks, overrides and isolates,
/// zero-width spaces, soft hyphens, BOM, ...), so external text cannot
/// inject codes, break the line, hide text or reorder what follows it.
/// Kept: the joiners U+200C and U+200D and the tag characters
/// U+E0020-U+E007F, which emoji sequences need. Tabs, line feeds and
/// carriage returns read as spaces, so words on two lines stay apart.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if matches!(c, '\n' | '\t' | '\r') {
                ' '
            } else {
                c
            }
        })
        .filter(|&c| !c.is_control() && !matches!(c, '\u{2028}' | '\u{2029}') && !dropped_format(c))
        .collect()
}

/// A `Cf` (format) character other than the joiners and emoji tags
/// (Unicode 16).
fn dropped_format(c: char) -> bool {
    matches!(c,
        '\u{00AD}'
        | '\u{0600}'..='\u{0605}'
        | '\u{061C}'
        | '\u{06DD}'
        | '\u{070F}'
        | '\u{0890}'..='\u{0891}'
        | '\u{08E2}'
        | '\u{180E}'
        | '\u{200B}'
        | '\u{200E}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{206F}'
        | '\u{FEFF}'
        | '\u{FFF9}'..='\u{FFFB}'
        | '\u{110BD}'
        | '\u{110CD}'
        | '\u{13430}'..='\u{1343F}'
        | '\u{1BCA0}'..='\u{1BCA3}'
        | '\u{1D173}'..='\u{1D17A}'
        | '\u{E0001}')
}

/// Cells the text occupies once SGR and OSC 8 sequences are removed.
/// Marks, followed by one space, where a plugin's output may wrap between
/// its items; zero cells wide, removed before anything is printed.
pub const ITEM_BREAK: char = '\u{1f}';

pub fn visible_width(text: &str) -> usize {
    visible_width_with(text, 1)
}

fn is_private_use(c: char) -> bool {
    matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{FFFFD}')
}

/// As `visible_width`, counting private-use glyphs (Nerd Font icons) as
/// `icon_width` cells; fonts draw them one or two cells wide.
pub fn visible_width_with(text: &str, icon_width: usize) -> usize {
    plain_width(&strip_codes(text), icon_width)
}

/// Cells of text without escape sequences, private-use glyphs `icon_width`
/// cells each.
pub fn plain_width(plain: &str, icon_width: usize) -> usize {
    plain
        .split(is_private_use)
        .map(|run| run.width())
        .sum::<usize>()
        + plain.chars().filter(|&c| is_private_use(c)).count() * icon_width
}

/// `text` cut to at most `cells` visible cells, ending in `…`; as it is
/// when it fits. CSI and OSC sequences are copied whole and never counted;
/// an OSC 8 link left open is closed, and any SGR is reset at the end.
/// Private-use glyphs count `icon_width` cells.
pub fn clip(text: &str, cells: usize, icon_width: usize) -> String {
    if visible_width_with(text, icon_width) <= cells {
        return text.to_string();
    }
    let budget = cells.saturating_sub(1);
    let (mut out, mut used, mut sgr, mut linked) = (String::new(), 0, false, false);
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            let mut seq = String::from(c);
            match it.peek().copied() {
                Some('[') => {
                    seq.extend(it.next());
                    for n in it.by_ref() {
                        seq.push(n);
                        if ('\x40'..='\x7e').contains(&n) {
                            break;
                        }
                    }
                    sgr = true;
                }
                Some(']') => {
                    seq.extend(it.next());
                    while let Some(n) = it.next() {
                        seq.push(n);
                        if n == '\x07' {
                            break;
                        }
                        if n == '\x1b' {
                            seq.extend(it.next());
                            break;
                        }
                    }
                    if let Some(body) = seq.strip_prefix("\x1b]8;") {
                        let uri = body.split_once(';').map_or("", |(_, u)| u);
                        let uri = uri.trim_end_matches('\x07').trim_end_matches("\x1b\\");
                        linked = !uri.is_empty();
                    }
                }
                _ => {}
            }
            out.push_str(&seq);
            continue;
        }
        let w = if is_private_use(c) {
            icon_width
        } else {
            c.width().unwrap_or(0)
        };
        if used + w > budget {
            break;
        }
        used += w;
        out.push(c);
    }
    out.push('…');
    if linked {
        out.push_str("\x1b]8;;\x1b\\");
    }
    if sgr {
        out.push_str("\x1b[0m");
    }
    out
}

/// The text with CSI (SGR) and OSC sequences removed.
pub fn strip_codes(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            plain.push(c);
            continue;
        }
        match it.peek() {
            Some('[') => {
                it.next();
                for n in it.by_ref() {
                    if ('\x40'..='\x7e').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                it.next();
                while let Some(n) = it.next() {
                    if n == '\x07' || (n == '\x1b' && it.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    plain
}

/// The text with SGR colour parameters removed (`NO_COLOR`); other
/// attributes and OSC 8 links stay, and an SGR left empty is dropped.
pub fn strip_colors(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("\x1b[") {
        out.push_str(&rest[..at]);
        let seq = &rest[at + 2..];
        let Some(end) = seq.find(|c: char| ('\x40'..='\x7e').contains(&c)) else {
            out.push_str(&rest[at..]);
            return out;
        };
        if seq.as_bytes()[end] == b'm' {
            let kept = without_colors(&seq[..end]);
            if !kept.is_empty() || seq[..end].is_empty() {
                out.push_str("\x1b[");
                out.push_str(&kept);
                out.push('m');
            }
        } else {
            out.push_str(&rest[at..at + 2 + end + 1]);
        }
        rest = &seq[end + 1..];
    }
    out.push_str(rest);
    out
}

/// An SGR parameter list without its colour parameters.
fn without_colors(params: &str) -> String {
    let p: Vec<&str> = params.split(';').collect();
    let mut kept: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < p.len() {
        // colon form: one parameter holds the whole colour (`38:2::r:g:b`)
        if let Some((head, _)) = p[i].split_once(':') {
            if !matches!(head, "38" | "48" | "58") {
                kept.push(p[i]);
            }
            i += 1;
            continue;
        }
        match p[i].parse::<u16>().unwrap_or(0) {
            38 | 48 | 58 => {
                i += match p.get(i + 1).copied() {
                    Some("5") => 3,
                    Some("2") => 5,
                    _ => 1,
                };
                continue;
            }
            30..=37 | 39 | 40..=47 | 49 | 59 | 90..=97 | 100..=107 => {}
            _ => kept.push(p[i]),
        }
        i += 1;
    }
    kept.join(";")
}
