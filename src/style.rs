//! SGR colouring, OSC 8 links, text sanitising and visible-width measuring.

use unicode_width::UnicodeWidthStr;

pub fn fg(code: u8, text: &str) -> String {
    format!("\x1b[38;5;{code}m{text}\x1b[39m")
}

pub fn bold(text: &str) -> String {
    format!("\x1b[1m{text}\x1b[22m")
}

/// Muted text colour (256-colour palette); SGR 2 faint renders too dark.
pub const MUTED: u8 = 250;

pub fn dim(text: &str) -> String {
    fg(MUTED, text)
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
/// or reorder what follows it.
pub fn sanitize(text: &str) -> String {
    text.chars()
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
