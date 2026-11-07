//! Places the left, centre and right groups on one or two lines.

use crate::style::{dim, visible_width_with};

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
