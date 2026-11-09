//! SGR colouring, OSC 8 links, text sanitising and visible-width measuring.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn fg(code: u8, text: &str) -> String {
    format!("{}{text}\x1b[39m", fg_code(code))
}

/// The SGR that starts foreground `code`.
pub fn fg_code(code: u8) -> String {
    match ansi16(code) {
        Some(n) => format!("\x1b[{}m", if n < 8 { 30 + n } else { 82 + n }),
        None => format!("\x1b[38;5;{code}m"),
    }
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
pub fn neutral() -> u8 {
    if ANSI_NOW.with(std::cell::Cell::get) {
        muted()
    } else {
        250
    }
}

/// The code when it is one of the 16 and the terminal palette is in effect.
fn ansi16(code: u8) -> Option<u8> {
    (code < 16 && ANSI_NOW.with(std::cell::Cell::get)).then_some(code)
}

pub fn bold(text: &str) -> String {
    format!("\x1b[1m{text}\x1b[22m")
}

/// Default muted text colour (256-colour palette); SGR 2 faint renders too
/// dark. A palette may pick another for the duration of a render.
pub const MUTED: u8 = 250;

thread_local! {
    static MUTED_NOW: std::cell::Cell<u8> = const { std::cell::Cell::new(MUTED) };
    static ANSI_NOW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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
pub struct MutedGuard(u8);

impl Drop for MutedGuard {
    fn drop(&mut self) {
        MUTED_NOW.with(|m| m.set(self.0));
    }
}

/// Uses `code` for `dim` on this thread until the guard drops.
pub fn use_muted(code: u8) -> MutedGuard {
    MutedGuard(MUTED_NOW.with(|m| m.replace(code)))
}

/// The muted colour in effect on this thread.
pub fn muted() -> u8 {
    MUTED_NOW.with(std::cell::Cell::get)
}

pub fn dim(text: &str) -> String {
    fg(muted(), text)
}

pub fn bg_code(code: u8) -> String {
    match ansi16(code) {
        Some(n) => format!("\x1b[{}m", if n < 8 { 40 + n } else { 92 + n }),
        None => format!("\x1b[48;5;{code}m"),
    }
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
pub fn ramp(score: i32, ramp: &[u8; 11]) -> u8 {
    ramp[((score.clamp(0, 100) + 9) / 10) as usize]
}

/// Drops ESC, C0, DEL and C1 controls, line/paragraph separators and bidi
/// overrides/isolates, so external text cannot inject codes, break the line
/// or reorder what follows it. Tabs, line feeds and carriage returns read
/// as spaces, so words on two lines stay apart.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| if matches!(c, '\n' | '\t' | '\r') { ' ' } else { c })
        .filter(|c| {
            !c.is_control()
                && !matches!(c, '\u{2028}' | '\u{2029}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
        .collect()
}

/// Cells the text occupies once SGR and OSC 8 sequences are removed.
pub fn visible_width(text: &str) -> usize {
    visible_width_with(text, 1)
}

fn is_private_use(c: char) -> bool {
    matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{FFFFD}')
}

/// As `visible_width`, counting private-use glyphs (Nerd Font icons) as
/// `icon_width` cells; fonts draw them one or two cells wide.
pub fn visible_width_with(text: &str, icon_width: usize) -> usize {
    let plain = strip_codes(text);
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
