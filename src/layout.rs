//! Places the left, centre and right groups on one or two lines.

use crate::style::{dim, visible_width_with};

const MARGIN: i64 = 4;

fn join(segs: &[String]) -> String {
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
/// with the separator when not.
fn edges(left: &str, right: &str, width: i64, gap: i64, iw: usize) -> String {
    let pad = width - visible_width_with(left, iw) as i64 - visible_width_with(right, iw) as i64;
    if right.is_empty() {
        left.to_string()
    } else if left.is_empty() {
        format!("{}{right}", spaces(pad))
    } else if pad >= gap {
        format!("{left}{}{right}", spaces(pad))
    } else {
        join(&[left.to_string(), right.to_string()])
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
    let (l, c, r) = (join(left), join(center), join(right));
    let gap = gap as i64;
    let width = columns.map_or(0, |n| n as i64) - MARGIN;
    let (lw, cw, rw) = (
        visible_width_with(&l, icon_width) as i64,
        visible_width_with(&c, icon_width) as i64,
        visible_width_with(&r, icon_width) as i64,
    );
    let spare = width - lw - cw - rw;

    if c.is_empty() {
        return edges(&l, &r, width, gap, icon_width);
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
    let first = edges(&l, &r, width, gap, icon_width);
    if first.is_empty() {
        c
    } else {
        format!("{first}\n{c}")
    }
}
