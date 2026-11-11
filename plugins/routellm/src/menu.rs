//! `--menu`: one check item per configured instance, from the plugin's own
//! cache. No request is made; a key the proxy refused is not used again,
//! and no key is ever printed.

use crate::cache::Problem;
use crate::collect::{Snapshot, Status};
use crate::config::{instance_id, Instance};
use crate::render::{stale, state_word};
use bar_plugin::menu::{Menu, MenuGroup, MenuItem, MenuKind};

/// The routing group's id.
pub const GROUP: &str = "routing";
/// The group of rows that say why an instance cannot be switched.
pub const NOTES: &str = "notes";

/// What one instance contributes: its check item, and a note when it
/// cannot be switched or its state is not known.
pub fn entry(inst: &Instance, snap: Option<&Snapshot>, now: i64) -> (MenuItem, Option<MenuItem>) {
    let id = instance_id(&inst.origin);
    let label = inst.origin.to_string();
    let item = MenuItem::new(&id, &label);
    let Some(s) = snap else {
        return (
            item.action("toggle", &[("instance", &id), ("to", "on")])
                .disabled(),
            Some(note(&id, &label, "no key; routing state unknown", "bad")),
        );
    };
    let (why, style) = match (&s.status, s.cache.problem) {
        (Status::AuthStopped(st), _) => (
            Some(format!(
                "management refused the key ({st}); no calls until it changes"
            )),
            "bad",
        ),
        (Status::NoKey, _) => (Some("no key; routing state unknown".into()), "bad"),
        (Status::Off, _) => (Some("management API off".into()), "warn"),
        (_, Some(Problem::Unconfigured)) => (
            Some(format!("no `{}` provider entry", inst.provider)),
            "warn",
        ),
        (_, Some(Problem::Mismatch)) => (
            Some(format!(
                "`{}` entry does not point at {}",
                inst.provider, inst.routellm
            )),
            "warn",
        ),
        _ if s.cache.entry.is_none() => (Some("not read yet".into()), "dim"),
        _ if state_word(s) == "down" => (Some("on, RouteLLM not answering".into()), "warn"),
        _ => (None, ""),
    };
    let on = state_word(s) == "on" || state_word(s) == "down";
    // the click states the target, the opposite of what is shown
    let to = if on { "off" } else { "on" };
    let mut item = item
        .checked(on)
        .action("toggle", &[("instance", &id), ("to", to)]);
    if s.cache.entry.is_none()
        || s.cache.problem.is_some()
        || matches!(s.status, Status::AuthStopped(_) | Status::NoKey)
    {
        item = item.disabled();
    }
    if stale(&s.cache, now) {
        item = item.detail("stale").style("dim");
    } else if on {
        item = item.style("good");
    }
    (item, why.map(|w| note(&id, &label, &w, style)))
}

fn note(id: &str, label: &str, why: &str, style: &str) -> MenuItem {
    MenuItem::new(id, &format!("{label}: {why}")).style(style)
}

/// The menu of every instance; `snaps` is parallel to `insts` (`None`: no
/// key could be read).
pub fn build(insts: &[Instance], snaps: &[Option<Snapshot>], now: i64) -> Menu {
    let mut routing = MenuGroup::new(GROUP, MenuKind::Check).label("RouteLLM");
    let mut notes = MenuGroup::new(NOTES, MenuKind::Info);
    for (i, s) in insts.iter().zip(snaps) {
        let (item, n) = entry(i, s.as_ref(), now);
        routing = routing.item(item);
        if let Some(n) = n {
            notes = notes.item(n);
        }
    }
    Menu {
        groups: vec![routing, notes],
    }
    .finish()
}
