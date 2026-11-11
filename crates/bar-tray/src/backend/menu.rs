//! The model's rows as muda items (NSMenuItems or Win32 menu items
//! underneath), with a handle per row id so a [`Change`] updates the item
//! in place. Every call here is a main-thread call: muda items are not
//! `Send`.

use crate::backend::rows::{self, Row, RowKind};
use crate::model::{Change, Menu};
use muda::{CheckMenuItem, IsMenuItem, MenuItem, PredefinedMenuItem, Submenu};
use std::collections::HashMap;

enum Handle {
    Check(CheckMenuItem),
    Normal(MenuItem),
    Sub(Submenu),
}

impl Handle {
    fn set(&self, text: &str, checked: bool, enabled: bool) {
        match self {
            Handle::Check(i) => {
                i.set_text(text);
                i.set_checked(checked);
                i.set_enabled(enabled);
            }
            Handle::Normal(i) => {
                i.set_text(text);
                i.set_enabled(enabled);
            }
            Handle::Sub(i) => {
                i.set_text(text);
                i.set_enabled(enabled);
            }
        }
    }

    fn set_checked(&self, checked: bool) {
        if let Handle::Check(i) = self {
            i.set_checked(checked);
        }
    }
}

/// A built menu and its rows by id.
pub struct Native {
    pub menu: muda::Menu,
    handles: HashMap<String, Handle>,
}

type Boxed = Box<dyn IsMenuItem>;

/// Builds the whole menu.
pub fn build(m: &Menu) -> muda::Result<Native> {
    let menu = muda::Menu::new();
    let mut handles = HashMap::new();
    for r in rows::rows(m) {
        let item = item(&r, &mut handles)?;
        menu.append(&*item)?;
    }
    Ok(Native { menu, handles })
}

fn item(r: &Row, handles: &mut HashMap<String, Handle>) -> muda::Result<Boxed> {
    let id = r.id.as_str();
    Ok(match &r.kind {
        RowKind::Separator => Box::new(PredefinedMenuItem::separator()),
        RowKind::Normal => {
            let i = MenuItem::with_id(id, &r.text, r.enabled, None);
            handles.insert(r.id.clone(), Handle::Normal(i.clone()));
            Box::new(i)
        }
        RowKind::Check => {
            let i = CheckMenuItem::with_id(id, &r.text, r.enabled, r.checked, None);
            handles.insert(r.id.clone(), Handle::Check(i.clone()));
            Box::new(i)
        }
        RowKind::Submenu(children) => {
            let s = Submenu::with_id(id, &r.text, r.enabled);
            for c in children {
                s.append(&*item(c, handles)?)?;
            }
            handles.insert(r.id.clone(), Handle::Sub(s.clone()));
            Box::new(s)
        }
    })
}

impl Native {
    /// Updates one row in place; a row this menu does not have is
    /// ignored (the next rebuild carries it).
    pub fn update(&self, c: &Change) {
        if let Some(h) = self.handles.get(&c.id) {
            // a submenu always opens, whatever its rows say
            let enabled = c.enabled || matches!(h, Handle::Sub(_));
            h.set(
                &rows::text(&c.label, c.detail.as_deref()),
                c.checked,
                enabled,
            );
        }
    }

    /// Puts the model's check mark back on row `id` after a click: a
    /// native check item flips its own mark, whatever the model decided.
    pub fn sync_checked(&self, id: &str, view: &Menu) {
        if let (Some(h), Some(row)) = (self.handles.get(id), view.find(id)) {
            h.set_checked(row.checked);
        }
    }
}
