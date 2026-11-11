//! What changed between two menus. A backend keeps native handles per
//! row id; when only row properties changed it updates those rows in
//! place, when the tree changed (rows added, removed, moved, re-kinded)
//! it rebuilds the menu.

use super::{Menu, Style};

/// New values for one row. Every field is the row's current value, so a
/// backend may set them all or compare with what it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
    pub checked: bool,
    pub enabled: bool,
    pub style: Option<Style>,
    pub confirm: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diff {
    /// Nothing to do.
    Same,
    /// Same tree, these rows changed.
    Update(Vec<Change>),
    /// The tree changed: build this menu from scratch.
    Rebuild(Menu),
}

/// The diff that turns `old` into `new`.
pub fn diff(old: &Menu, new: &Menu) -> Diff {
    let a = old.flatten();
    let b = new.flatten();
    let same_shape = a.len() == b.len()
        && a.iter().zip(&b).all(|((x, dx), (y, dy))| {
            x.id == y.id
                && dx == dy
                && std::mem::discriminant(&x.kind) == std::mem::discriminant(&y.kind)
        });
    if !same_shape {
        return Diff::Rebuild(new.clone());
    }
    let changes: Vec<Change> = a
        .iter()
        .zip(&b)
        .filter(|((x, _), (y, _))| {
            x.label != y.label
                || x.detail != y.detail
                || x.checked != y.checked
                || x.enabled != y.enabled
                || x.style != y.style
                || x.confirm != y.confirm
        })
        .map(|(_, (y, _))| Change {
            id: y.id.clone(),
            label: y.label.clone(),
            detail: y.detail.clone(),
            checked: y.checked,
            enabled: y.enabled,
            style: y.style,
            confirm: y.confirm,
        })
        .collect();
    if changes.is_empty() {
        Diff::Same
    } else {
        Diff::Update(changes)
    }
}

impl Menu {
    /// Applies a diff; afterwards this menu equals the one the diff was
    /// made against. `Err` names a changed row this menu does not have.
    pub fn apply(&mut self, d: &Diff) -> Result<(), String> {
        match d {
            Diff::Same => Ok(()),
            Diff::Rebuild(m) => {
                *self = m.clone();
                Ok(())
            }
            Diff::Update(changes) => {
                for c in changes {
                    let row = self
                        .find_mut(&c.id)
                        .ok_or_else(|| format!("no row {}", c.id))?;
                    row.label.clone_from(&c.label);
                    row.detail.clone_from(&c.detail);
                    row.checked = c.checked;
                    row.enabled = c.enabled;
                    row.style = c.style;
                    row.confirm = c.confirm;
                }
                Ok(())
            }
        }
    }
}
