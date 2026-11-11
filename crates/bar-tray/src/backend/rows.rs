//! The model as rows a native menu toolkit takes: one text per row (label
//! and detail), a check mark for toggles and choices, children for a
//! submenu. Pure data, so the mapping is tested without a desktop.

use crate::model::{Kind, Menu, MenuItem};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKind {
    Normal,
    /// Carries a check mark (toggle or one choice of a picker).
    Check,
    Submenu(Vec<Row>),
    Separator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub text: String,
    pub kind: RowKind,
    pub checked: bool,
    pub enabled: bool,
}

/// Top-level rows of a menu, in order.
pub fn rows(menu: &Menu) -> Vec<Row> {
    menu.items.iter().map(row).collect()
}

fn row(m: &MenuItem) -> Row {
    let kind = match &m.kind {
        Kind::Separator => RowKind::Separator,
        Kind::Toggle | Kind::Choice => RowKind::Check,
        Kind::Info | Kind::Action => RowKind::Normal,
        Kind::Submenu(children) => RowKind::Submenu(children.iter().map(row).collect()),
    };
    Row {
        id: m.id.clone(),
        text: text(&m.label, m.detail.as_deref()),
        kind,
        checked: m.checked,
        // a separator or an info row is never live; a submenu always opens
        enabled: m.enabled || matches!(m.kind, Kind::Submenu(_)),
    }
}

/// `label — detail`, with `&` doubled: Windows reads a single `&` as an
/// accelerator mark and would swallow it.
pub fn text(label: &str, detail: Option<&str>) -> String {
    let mut out = String::with_capacity(label.len());
    escape(label, &mut out);
    if let Some(d) = detail.filter(|d| !d.is_empty()) {
        out.push_str(" — ");
        escape(d, &mut out);
    }
    out
}

fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        if c == '&' {
            out.push('&');
        }
        out.push(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::parse;

    fn fixture() -> Vec<Row> {
        let s = parse(include_str!("../../tests/fixtures/snapshot.json")).unwrap();
        rows(&Menu::build(&s, 1_762_800_000))
    }

    #[test]
    fn text_joins_detail_and_escapes() {
        assert_eq!(text("a", None), "a");
        assert_eq!(text("a", Some("")), "a");
        assert_eq!(text("R&D", Some("5h & 7d")), "R&&D — 5h && 7d");
    }

    #[test]
    fn fixture_shape() {
        let r = fixture();
        let RowKind::Submenu(children) = &r[0].kind else {
            panic!("first row is a picker");
        };
        assert!(r[0].enabled);
        assert_eq!(children[0].kind, RowKind::Check);
        assert!(children[0].checked);
        assert_eq!(children[0].text, "alice — 5h 59% ↻2h10m · 7d 71%");
        let last = r.last().unwrap();
        assert_eq!(
            (last.id.as_str(), &last.kind),
            ("tray.quit", &RowKind::Normal)
        );
        assert!(last.enabled);
    }

    #[test]
    fn info_and_separator_disabled() {
        let r = fixture();
        let info = r.iter().find(|x| x.id == "cliproxyapi.info.auth").unwrap();
        assert!(!info.enabled);
        assert_eq!(info.kind, RowKind::Normal);
        assert!(r
            .iter()
            .filter(|x| x.kind == RowKind::Separator)
            .all(|x| !x.enabled));
    }
}
