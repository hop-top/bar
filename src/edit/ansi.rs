//! bar's own output (SGR colours, bold, OSC 8 links) as ratatui text, so the
//! preview is the renderer's real output.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

fn indexed(n: u16) -> Option<Color> {
    u8::try_from(n).ok().map(Color::Indexed)
}

/// Applies one SGR parameter list to `st`.
fn sgr(st: &mut Style, params: &str) {
    let nums: Vec<u16> = params.split(';').map(|p| p.parse().unwrap_or(0)).collect();
    let mut i = 0;
    while i < nums.len() {
        match nums[i] {
            0 => *st = Style::default(),
            1 => *st = st.add_modifier(Modifier::BOLD),
            2 => *st = st.add_modifier(Modifier::DIM),
            22 => *st = st.remove_modifier(Modifier::BOLD | Modifier::DIM),
            39 => st.fg = None,
            49 => st.bg = None,
            n @ 30..=37 => st.fg = indexed(n - 30),
            n @ 90..=97 => st.fg = indexed(n - 82),
            n @ 40..=47 => st.bg = indexed(n - 40),
            n @ 100..=107 => st.bg = indexed(n - 92),
            n @ (38 | 48) if nums.get(i + 1) == Some(&5) => {
                let c = nums.get(i + 2).and_then(|&c| indexed(c));
                if n == 38 {
                    st.fg = c;
                } else {
                    st.bg = c;
                }
                i += 2;
            }
            n @ (38 | 48) if nums.get(i + 1) == Some(&2) => {
                let ch = |k: usize| nums.get(i + k).and_then(|&v| u8::try_from(v).ok());
                let c = match (ch(2), ch(3), ch(4)) {
                    (Some(r), Some(g), Some(b)) => Some(Color::Rgb(r, g, b)),
                    _ => None,
                };
                if n == 38 {
                    st.fg = c;
                } else {
                    st.bg = c;
                }
                i += 4;
            }
            _ => {}
        }
        i += 1;
    }
}

/// One `Line` per row of `text`; escape sequences other than SGR are dropped.
pub fn lines(text: &str) -> Vec<Line<'static>> {
    text.split('\n').map(line).collect()
}

fn line(row: &str) -> Line<'static> {
    let mut spans = Vec::new();
    let mut st = Style::default();
    let mut cur = String::new();
    let mut it = row.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            cur.push(c);
            continue;
        }
        match it.peek() {
            Some('[') => {
                it.next();
                let mut params = String::new();
                let mut fin = ' ';
                for n in it.by_ref() {
                    if ('\x40'..='\x7e').contains(&n) {
                        fin = n;
                        break;
                    }
                    params.push(n);
                }
                if fin == 'm' {
                    if !cur.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut cur), st));
                    }
                    sgr(&mut st, &params);
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
    if !cur.is_empty() {
        spans.push(Span::styled(cur, st));
    }
    Line::from(spans)
}
