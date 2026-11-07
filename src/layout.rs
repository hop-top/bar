//! Places the left, centre and right groups on one or two lines, or flows
//! segments onto rows when wrapping.

use crate::style::{dim, visible_width, visible_width_with};
use std::ops::Range;

const MARGIN: i64 = 4;

/// A group's text and the cells it takes on screen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Group {
    pub text: String,
    pub width: usize,
}

/// Placed groups: the text, and whether it is one row within the usable
/// width (always true without `COLUMNS`, where nothing is measured).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub text: String,
    pub fits: bool,
}

/// The muted ` │ ` drawn between plain segments.
pub fn plain_joiner() -> String {
    format!(" {} ", dim("│"))
}

/// Segments joined with the muted ` │ ` separator, empty ones dropped.
pub fn join(segs: &[String]) -> String {
    segs.iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(&plain_joiner())
}

fn spaces(n: i64) -> String {
    " ".repeat(n.max(0) as usize)
}

/// Left and right on the first line: padded apart when `gap` fits, joined
/// with `joiner` when not. Returns the text and the cells it takes.
fn edges(left: &Group, right: &Group, width: i64, gap: i64, joiner: &str) -> (String, i64) {
    let (l, r) = (&left.text, &right.text);
    let (lw, rw) = (left.width as i64, right.width as i64);
    let pad = width - lw - rw;
    if r.is_empty() {
        (l.to_string(), lw)
    } else if l.is_empty() {
        (format!("{}{r}", spaces(pad)), pad.max(0) + rw)
    } else if pad >= gap {
        (format!("{l}{}{r}", spaces(pad)), lw + pad.max(0) + rw)
    } else {
        let jw = visible_width(joiner) as i64;
        (format!("{l}{joiner}{r}"), lw + jw + rw)
    }
}

pub fn compose(
    left: &[String],
    center: &[String],
    right: &[String],
    columns: Option<usize>,
    gap: usize,
) -> String {
    compose_with(left, center, right, columns, gap, 1)
}

/// As `compose`, with private-use icon glyphs `icon_width` cells wide.
pub fn compose_with(
    left: &[String],
    center: &[String],
    right: &[String],
    columns: Option<usize>,
    gap: usize,
    icon_width: usize,
) -> String {
    let group = |segs: &[String]| {
        let text = join(segs);
        Group {
            width: visible_width_with(&text, icon_width),
            text,
        }
    };
    compose_groups(
        &group(left),
        &group(center),
        &group(right),
        columns,
        gap,
        &plain_joiner(),
    )
}

/// Places pre-joined groups; `joiner` sits between left and right when
/// they share the first line without room for `gap`.
pub fn compose_groups(
    left: &Group,
    center: &Group,
    right: &Group,
    columns: Option<usize>,
    gap: usize,
    joiner: &str,
) -> String {
    place_groups(left, center, right, columns, gap, joiner).text
}

/// As `compose_groups`, also saying whether the result fits one row.
pub fn place_groups(
    left: &Group,
    center: &Group,
    right: &Group,
    columns: Option<usize>,
    gap: usize,
    joiner: &str,
) -> Placed {
    let (l, c, r) = (&left.text, &center.text, &right.text);
    let gap = gap as i64;
    let width = columns.map_or(0, |n| n as i64) - MARGIN;
    let (lw, cw, rw) = (left.width as i64, center.width as i64, right.width as i64);
    let spare = width - lw - cw - rw;
    let one_row = |text: String, cells: i64| Placed {
        text,
        fits: columns.is_none() || cells <= width,
    };

    if c.is_empty() {
        let (text, cells) = edges(left, right, width, gap, joiner);
        return one_row(text, cells);
    }
    if columns.is_some() && spare >= 2 * gap {
        let mut lgap = (width - cw) / 2 - lw;
        let mut rgap = spare - lgap;
        if lgap < gap || rgap < gap {
            lgap = spare / 2;
            rgap = spare - lgap;
        }
        if r.is_empty() {
            rgap = 0;
        }
        let text = format!("{l}{}{c}{}{r}", spaces(lgap), spaces(rgap));
        return one_row(text, lw + lgap + cw + rgap + rw);
    }
    let (first, _) = edges(left, right, width, gap, joiner);
    if first.is_empty() {
        return one_row(c.to_string(), cw);
    }
    Placed {
        text: format!("{first}\n{c}"),
        fits: columns.is_none(),
    }
}

/// Splits `n` segments, in order, into rows whose `measure` stays within
/// the usable width of `columns`; a segment too wide for any row gets its
/// own. At most `max_rows` rows; segments that do not fit are dropped.
pub fn flow(
    n: usize,
    columns: usize,
    max_rows: usize,
    measure: impl Fn(Range<usize>) -> usize,
) -> Vec<Range<usize>> {
    let width = (columns as i64 - MARGIN).max(0) as usize;
    let mut rows = Vec::new();
    let mut start = 0;
    while start < n && rows.len() < max_rows {
        let mut end = start + 1;
        while end < n && measure(start..end + 1) <= width {
            end += 1;
        }
        rows.push(start..end);
        start = end;
    }
    rows
}
