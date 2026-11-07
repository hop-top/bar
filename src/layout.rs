//! Places the left, centre and right groups on one or two lines, or flows
//! segments onto rows when wrapping.

use crate::style::{dim, visible_width_with};
use std::ops::Range;

const MARGIN: i64 = 4;

/// A group's text and the cells it takes on screen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Group {
    pub text: String,
    pub width: usize,
}

/// Segments joined with the muted ` │ ` separator, empty ones dropped.
pub fn join(segs: &[String]) -> String {
    let sep = format!(" {} ", dim("│"));
    segs.iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(&sep)
}

fn spaces(n: i64) -> String {
    " ".repeat(n.max(0) as usize)
}

/// Left and right on the first line: padded apart when `gap` fits, joined
/// with `joiner` when not.
fn edges(left: &Group, right: &Group, width: i64, gap: i64, joiner: &str) -> String {
    let (l, r) = (&left.text, &right.text);
    let pad = width - left.width as i64 - right.width as i64;
    if r.is_empty() {
        l.to_string()
    } else if l.is_empty() {
        format!("{}{r}", spaces(pad))
    } else if pad >= gap {
        format!("{l}{}{r}", spaces(pad))
    } else {
        format!("{l}{joiner}{r}")
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
    let joiner = format!(" {} ", dim("│"));
    compose_groups(
        &group(left),
        &group(center),
        &group(right),
        columns,
        gap,
        &joiner,
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
    let (l, c, r) = (&left.text, &center.text, &right.text);
    let gap = gap as i64;
    let width = columns.map_or(0, |n| n as i64) - MARGIN;
    let (lw, cw, rw) = (left.width as i64, center.width as i64, right.width as i64);
    let spare = width - lw - cw - rw;

    if c.is_empty() {
        return edges(left, right, width, gap, joiner);
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
        return format!("{l}{}{c}{}{r}", spaces(lgap), spaces(rgap));
    }
    let first = edges(left, right, width, gap, joiner);
    if first.is_empty() {
        c.to_string()
    } else {
        format!("{first}\n{c}")
    }
}

/// Whether `compose_groups` puts the groups on one row no wider than the
/// usable width. Without `columns` nothing is measured, so it is true.
pub fn fits_one_row(
    left: &Group,
    center: &Group,
    right: &Group,
    columns: Option<usize>,
    gap: usize,
    joiner_width: usize,
) -> bool {
    let Some(cols) = columns else {
        return true;
    };
    let width = cols as i64 - MARGIN;
    let (lw, cw, rw) = (left.width as i64, center.width as i64, right.width as i64);
    if !center.text.is_empty() {
        return width - lw - cw - rw >= 2 * gap as i64;
    }
    if left.text.is_empty() || right.text.is_empty() {
        return lw.max(rw) <= width;
    }
    width - lw - rw >= gap as i64 || lw + joiner_width as i64 + rw <= width
}

/// Splits `n` segments, in order, into rows whose `measure` stays within
/// the usable width of `columns`; a segment too wide for any row gets its
/// own. At most `max_rows` rows: the last takes whatever is left.
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
        if rows.len() + 1 == max_rows {
            rows.push(start..n);
            break;
        }
        let mut end = start + 1;
        while end < n && measure(start..end + 1) <= width {
            end += 1;
        }
        rows.push(start..end);
        start = end;
    }
    rows
}
