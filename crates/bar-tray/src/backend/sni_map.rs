//! Menu model to StatusNotifierItem rows, without the D-Bus library: the
//! pure part of the Linux backend, so it is testable anywhere. Rows keep
//! the model id they came from; the backend turns a click on a row (or a
//! radio index) back into that id.
//!
//! dbusmenu has no "section header" or "info" row type: info rows become
//! disabled standard rows, a `Style` becomes the row's `disposition`
//! hint (hosts may ignore it).

use crate::model::{Kind, Menu, MenuItem, Style};

/// Freedesktop dbusmenu `disposition`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Informative,
    Warning,
    Alert,
}

impl Tone {
    fn of(style: Option<Style>) -> Tone {
        match style {
            None | Some(Style::Good) => Tone::Normal,
            Some(Style::Warn) => Tone::Warning,
            Some(Style::Bad) => Tone::Alert,
            Some(Style::Dim) => Tone::Informative,
        }
    }
}

/// One option of a [`Entry::Radio`] group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub tone: Tone,
}

/// A row as the SNI backend shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// Plain row; disabled for info and in-flight rows.
    Item {
        id: String,
        label: String,
        enabled: bool,
        tone: Tone,
    },
    Check {
        id: String,
        label: String,
        enabled: bool,
        checked: bool,
        tone: Tone,
    },
    /// Consecutive exclusive choices; `selected` is the checked option's
    /// index, `usize::MAX` when none is checked (shows no selection).
    Radio {
        selected: usize,
        options: Vec<Choice>,
    },
    Sub {
        label: String,
        tone: Tone,
        children: Vec<Entry>,
    },
    Separator,
}

/// The rows for a menu, in order.
pub fn entries(menu: &Menu) -> Vec<Entry> {
    rows(&menu.items)
}

fn rows(items: &[MenuItem]) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < items.len() {
        if items[i].kind == Kind::Choice {
            let n = items[i..]
                .iter()
                .take_while(|m| m.kind == Kind::Choice)
                .count();
            out.push(radio(&items[i..i + n]));
            i += n;
        } else {
            out.push(entry(&items[i]));
            i += 1;
        }
    }
    out
}

fn radio(group: &[MenuItem]) -> Entry {
    Entry::Radio {
        selected: group.iter().position(|m| m.checked).unwrap_or(usize::MAX),
        options: group
            .iter()
            .map(|m| Choice {
                id: m.id.clone(),
                label: label(m),
                enabled: m.enabled,
                tone: Tone::of(m.style),
            })
            .collect(),
    }
}

fn entry(m: &MenuItem) -> Entry {
    let tone = Tone::of(m.style);
    match &m.kind {
        Kind::Separator => Entry::Separator,
        Kind::Submenu(children) => Entry::Sub {
            label: label(m),
            tone,
            children: rows(children),
        },
        Kind::Toggle => Entry::Check {
            id: m.id.clone(),
            label: label(m),
            enabled: m.enabled,
            checked: m.checked,
            tone,
        },
        // a choice is grouped by `rows`; the model already disables `Info`
        Kind::Info | Kind::Action | Kind::Choice => Entry::Item {
            id: m.id.clone(),
            label: label(m),
            enabled: m.enabled,
            tone,
        },
    }
}

/// `label — detail`, with dbusmenu mnemonics escaped: a lone `_` marks an
/// access key and is dropped, `__` shows one `_`.
fn label(m: &MenuItem) -> String {
    let text = match &m.detail {
        Some(d) => format!("{} — {d}", m.label),
        None => m.label.clone(),
    };
    text.replace('_', "__")
}

/// Name of the freedesktop icon: symbolic, so themes recolour it.
pub const ICON_NAME: &str = "bar-tray-symbolic";

/// Square sizes of the fallback pixmaps.
pub const PIXMAP_SIZES: [u32; 3] = [22, 32, 48];

/// ARGB32 (network byte order) status-line glyph: three left-aligned
/// horizontal bars of different length, mid grey so it reads on light
/// and dark panels. Hosts use it only when `ICON_NAME` does not resolve.
pub fn pixmap(size: u32) -> Vec<u8> {
    let n = size as usize;
    let mut px = vec![0u8; n * n * 4];
    let pad = n / 6;
    let bar = (n / 6).max(2);
    let gap = (n - 2 * pad - 3 * bar) / 2;
    for (k, tenths) in [10, 7, 9].into_iter().enumerate() {
        let top = pad + k * (bar + gap);
        let width = (n - 2 * pad) * tenths / 10;
        for y in top..top + bar {
            for x in pad..pad + width {
                let o = (y * n + x) * 4;
                px[o..o + 4].copy_from_slice(&[0xFF, 0x90, 0x90, 0x90]);
            }
        }
    }
    px
}

#[cfg(test)]
mod tests;
