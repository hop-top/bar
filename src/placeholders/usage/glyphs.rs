//! Bar drawing and the `show`/`alt` component lists of the usage placeholder.

use crate::style::{dim, fg};

/// Components `show` and `alt` accept.
pub const SHOW_ITEMS: [&str; 5] = ["time", "bar", "pct", "eta", "proj"];

/// Names `glyphs` accepts; the first is the default.
pub const GLYPH_SETS: [&str; 7] = [
    "blocks", "shades", "dots", "squares", "bars", "braille", "eighths",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Time,
    Bar,
    Pct,
    Eta,
    Proj,
}

/// Known items in order, repeats dropped; unknown ones are skipped.
pub fn parse_list(raw: &str) -> Vec<Part> {
    let mut out = Vec::new();
    for item in raw.split(',') {
        let part = match item.trim() {
            "time" => Part::Time,
            "bar" => Part::Bar,
            "pct" => Part::Pct,
            "eta" => Part::Eta,
            "proj" => Part::Proj,
            _ => continue,
        };
        if !out.contains(&part) {
            out.push(part);
        }
    }
    out
}

/// What `view` meant before `show` existed.
pub fn view_alias(view: &str) -> Vec<Part> {
    match view {
        "pct" => vec![Part::Time, Part::Pct],
        "time" => vec![Part::Time],
        _ => vec![Part::Time, Part::Bar],
    }
}

/// `show` with bar and pct trading places.
pub fn swapped(show: &[Part]) -> Vec<Part> {
    show.iter()
        .map(|p| match p {
            Part::Bar => Part::Pct,
            Part::Pct => Part::Bar,
            other => *other,
        })
        .collect()
}

/// (filled, empty) cells of a single-cell glyph set.
fn cells(name: &str) -> (&'static str, &'static str) {
    match name {
        "shades" => ("▓", "░"),
        "dots" => ("●", "○"),
        "squares" => ("■", "□"),
        "bars" => ("▰", "▱"),
        "braille" => ("⣿", "⣀"),
        _ => ("█", "░"),
    }
}

const PARTIAL: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];

/// `value` percent of `width` cells in one colour; empty cells muted.
pub fn bar(glyphs: &str, width: i64, value: i64, color: u8) -> String {
    let width = width as usize;
    let (full, empty) = cells(glyphs);
    let (filled, part) = if glyphs == "eighths" {
        let eighths = (value as usize * width * 8 + 50) / 100;
        (eighths / 8, PARTIAL[eighths % 8])
    } else {
        ((value as usize * width + 50) / 100, "")
    };
    let used = filled + usize::from(!part.is_empty());
    let mut out = String::new();
    if used > 0 {
        let lit = format!("{}{part}", full.repeat(filled));
        out.push_str(&fg(color, &lit));
    }
    if width > used {
        out.push_str(&dim(&empty.repeat(width - used)));
    }
    out
}
