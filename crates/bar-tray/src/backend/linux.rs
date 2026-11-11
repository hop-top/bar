//! Linux backend: a StatusNotifierItem with a dbusmenu, over `ksni`'s
//! blocking API (its own thread, `async-io`, no GTK or tokio).
//!
//! The backend is passive. The app owns the model: it feeds [`Tray::show`]
//! the menu to display and reacts to the [`Event`]s sent on the channel
//! given to [`Tray::start`]:
//!
//! - [`Event::Click`] carries a model row id; pass it to `State::click`.
//! - [`Event::AboutToShow`] fires when a host is about to open the menu;
//!   fetch a fresh snapshot, `State::replace`, then [`Tray::show`]. Hosts
//!   differ on redrawing an open menu, so the refresh is best effort.
//!
//! Startup before the panel (autostart) or a panel restart is covered by
//! `ksni` itself: with no `StatusNotifierWatcher` yet the item is kept
//! and registered when one appears ([`Event::WatcherOnline`] follows).

use super::sni_map::{self, Choice, Entry, Tone};
use crate::model::Menu;
use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{
    CheckmarkItem, Disposition, MenuItem, RadioGroup, RadioItem, StandardItem, SubMenu,
};
use std::sync::mpsc::Sender;

pub use super::sni_loop::Event;

#[path = "linux/run.rs"]
mod run;
pub use run::run;

/// The running item.
pub struct Tray {
    handle: Handle<Sni>,
}

/// `ksni` failed to start: no session bus, or a watcher error other
/// than "no watcher yet".
pub type Error = ksni::Error;

impl Tray {
    /// Shows `menu` and starts serving it.
    pub fn start(menu: &Menu, events: Sender<Event>) -> Result<Tray, Error> {
        let sni = Sni {
            entries: sni_map::entries(menu),
            events,
            bad: false,
            tip: String::new(),
        };
        // a missing watcher is not an error: the panel may start later
        let handle = sni.assume_sni_available(true).spawn()?;
        Ok(Tray { handle })
    }

    /// Replaces the displayed menu. `ksni` diffs it against the one the
    /// host has and signals only what changed. `false`: the service ended.
    pub fn show(&self, menu: &Menu) -> bool {
        let entries = sni_map::entries(menu);
        self.handle.update(|t| t.entries = entries).is_some()
    }

    /// Marks the item as needing attention (a glyph with a dot, hosts
    /// may also emphasise it) or back to idle.
    pub fn set_bad(&self, bad: bool) -> bool {
        self.handle.update(|t| t.bad = bad).is_some()
    }

    /// Sets the hover text.
    pub fn set_tooltip(&self, tip: String) -> bool {
        self.handle.update(|t| t.tip = tip).is_some()
    }

    /// The service ended (bus gone, shut down).
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    /// Takes the item off the bus.
    pub fn shutdown(self) {
        self.handle.shutdown().wait();
    }
}

/// The `ksni` item: the rows, and where events go.
pub struct Sni {
    entries: Vec<Entry>,
    events: Sender<Event>,
    bad: bool,
    tip: String,
}

impl Sni {
    fn send(&self, e: Event) {
        // the receiver is gone only while the app exits
        let _ = self.events.send(e);
    }
}

impl ksni::Tray for Sni {
    // left click opens the menu, as there is no window to raise
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "bar-tray".into()
    }

    fn title(&self) -> String {
        "bar".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn icon_name(&self) -> String {
        sni_map::ICON_NAME.into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        sni_map::PIXMAP_SIZES
            .iter()
            .map(|&s| ksni::Icon {
                width: s as i32,
                height: s as i32,
                data: sni_map::pixmap(s),
            })
            .collect()
    }

    fn status(&self) -> ksni::Status {
        if self.bad {
            ksni::Status::NeedsAttention
        } else {
            ksni::Status::Active
        }
    }

    fn attention_icon_pixmap(&self) -> Vec<ksni::Icon> {
        sni_map::PIXMAP_SIZES
            .iter()
            .map(|&s| ksni::Icon {
                width: s as i32,
                height: s as i32,
                data: sni_map::pixmap_bad(s),
            })
            .collect()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.tip.clone(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        self.entries.iter().map(item).collect()
    }

    fn menu_about_to_show(&mut self) {
        self.send(Event::AboutToShow);
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        self.send(Event::WatcherOffline);
        true
    }

    fn watcher_online(&self) {
        self.send(Event::WatcherOnline);
    }
}

fn disposition(t: Tone) -> Disposition {
    match t {
        Tone::Normal => Disposition::Normal,
        Tone::Informative => Disposition::Informative,
        Tone::Warning => Disposition::Warning,
        Tone::Alert => Disposition::Alert,
    }
}

fn click(id: &str) -> Box<dyn Fn(&mut Sni) + Send + Sync> {
    let id = id.to_string();
    Box::new(move |t| t.send(Event::Click(id.clone())))
}

fn item(e: &Entry) -> MenuItem<Sni> {
    match e {
        Entry::Separator => MenuItem::Separator,
        Entry::Item {
            id,
            label,
            enabled,
            tone,
        } => StandardItem {
            label: label.clone(),
            enabled: *enabled,
            disposition: disposition(*tone),
            activate: click(id),
            ..Default::default()
        }
        .into(),
        Entry::Check {
            id,
            label,
            enabled,
            checked,
            tone,
        } => CheckmarkItem {
            label: label.clone(),
            enabled: *enabled,
            checked: *checked,
            disposition: disposition(*tone),
            activate: click(id),
            ..Default::default()
        }
        .into(),
        Entry::Radio { selected, options } => {
            let ids: Vec<String> = options.iter().map(|o| o.id.clone()).collect();
            RadioGroup {
                selected: *selected,
                select: Box::new(move |t: &mut Sni, i| {
                    if let Some(id) = ids.get(i) {
                        t.send(Event::Click(id.clone()));
                    }
                }),
                options: options.iter().map(radio_item).collect(),
            }
            .into()
        }
        Entry::Sub {
            label,
            tone,
            children,
        } => SubMenu {
            label: label.clone(),
            disposition: disposition(*tone),
            submenu: children.iter().map(item).collect(),
            ..Default::default()
        }
        .into(),
    }
}

fn radio_item(c: &Choice) -> RadioItem {
    RadioItem {
        label: c.label.clone(),
        enabled: c.enabled,
        disposition: disposition(c.tone),
        ..Default::default()
    }
}
