//! The pure part of the Linux event loop: what the item reports, and how
//! each report reaches the app. No D-Bus, so it tests anywhere.

use super::app;

/// What the host did, as the app must see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A row was activated; the model row id.
    Click(String),
    /// The root menu is about to open.
    AboutToShow,
    /// No `StatusNotifierWatcher` (panel gone or not started yet).
    WatcherOffline,
    /// A watcher is back and the item is registered again.
    WatcherOnline,
}

/// What the main loop receives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    /// For the app controller.
    App(app::Event),
    /// A watcher is back: show the menu again.
    Online,
    /// The watcher is gone; nothing to do, the item stays registered
    /// with `ksni` and returns by itself.
    Offline,
}

/// The loop message for a host event.
pub fn translate(e: Event) -> Msg {
    match e {
        Event::Click(id) => Msg::App(app::Event::Click(id)),
        Event::AboutToShow => Msg::App(app::Event::Opened),
        Event::WatcherOnline => Msg::Online,
        Event::WatcherOffline => Msg::Offline,
    }
}

/// Name of the abstract socket that marks the running copy: per user,
/// as the abstract namespace is shared by every user of the machine.
pub fn instance_name(user: Option<&str>) -> String {
    let user: String = user
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        .collect();
    format!("bar-tray.{user}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_events_reach_the_app() {
        assert_eq!(
            translate(Event::Click("a.b".into())),
            Msg::App(app::Event::Click("a.b".into()))
        );
        assert_eq!(translate(Event::AboutToShow), Msg::App(app::Event::Opened));
        assert_eq!(translate(Event::WatcherOnline), Msg::Online);
        assert_eq!(translate(Event::WatcherOffline), Msg::Offline);
    }

    #[test]
    fn instance_name_is_per_user_and_plain() {
        assert_eq!(instance_name(Some("jad")), "bar-tray.jad");
        assert_ne!(instance_name(Some("a")), instance_name(Some("b")));
        assert_eq!(instance_name(Some("a/../b\0")), "bar-tray.a..b");
        assert_eq!(instance_name(None), "bar-tray.");
    }
}
