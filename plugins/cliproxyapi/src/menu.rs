//! `--menu`: every configured instance as a desktop menu, answered from
//! the plugin's own cache files. Nothing is sent to a proxy and no key is
//! printed; a remembered refusal, a disabled management API or a backoff
//! shows as an information row.
//!
//! Per instance: an information row when the proxy cannot be read, then
//! per provider a radio group (the account the proxy prefers checked) and
//! a `next` / `next-soon` button group.

use crate::collect::{self, Status, STALE_AFTER};
use crate::config::{Config, Instance};
use crate::model::Acct;
use crate::pool::sole_top;
use crate::render::countdown;
use crate::switch::instance_id;
use bar_plugin::keysource;
use bar_plugin::menu::{valid_menu_id, Menu, MenuGroup, MenuItem, MenuKind};
use std::path::Path;

fn title(p: &str) -> String {
    let mut c = p.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// `host:port` of an origin, without the scheme.
fn host(inst: &Instance) -> String {
    let s = inst.origin.to_string();
    s.split_once("://").map_or(s.clone(), |(_, h)| h.into())
}

/// A stable id for an account: its index when that is a usable id.
fn item_id(key: &str) -> String {
    if valid_menu_id(key) {
        key.into()
    } else {
        bar_plugin::sha256::hex(key.as_bytes())[..16].into()
    }
}

/// The window that limits the account (the most used one still running),
/// and whether the reading is stale.
fn detail(a: &Acct, now: i64) -> Option<String> {
    let stale = now - a.at > STALE_AFTER;
    let w = a
        .windows
        .iter()
        .filter(|w| w.resets_at <= 0 || w.resets_at > now)
        .max_by(|x, y| x.used.total_cmp(&y.used));
    let mut s = w.map(|w| {
        let mut s = format!("{} {:.0}% used", w.id, w.used);
        if w.resets_at > now {
            s += &format!(" \u{21BB}{}", countdown(w.resets_at - now));
        }
        s
    });
    if stale {
        s = Some(s.map_or("stale".into(), |s| s + " (stale)"));
    }
    s
}

fn account_item(a: &Acct, id: &str, preferred: bool, now: i64) -> MenuItem {
    // the instance named: two instances may list the same index
    let t = [
        ("account", a.key.as_str()),
        ("instance", id),
        ("provider", &a.provider),
    ];
    let mut i = MenuItem::new(&item_id(&a.key), &a.label)
        .action("use", &t)
        .checked(preferred);
    if let Some(d) = detail(a, now) {
        i = i.detail(&d);
    }
    if a.disabled {
        i = i.disabled().detail("disabled").style("dim");
    } else if a.unavailable {
        i = i.detail("cooling down").style("warn");
    } else if now - a.at > STALE_AFTER {
        i = i.style("dim");
    }
    i
}

fn status_row(s: &Status, none: bool) -> Option<MenuItem> {
    let (text, style) = match s {
        Status::Ok if none => ("no accounts cached yet", "dim"),
        Status::Ok => return None,
        Status::AuthStopped(st) => {
            return Some(
                MenuItem::new("auth", &format!("management refused the key ({st})")).style("bad"),
            )
        }
        Status::Off => ("management API is off", "warn"),
        Status::NoKey => ("key unavailable", "bad"),
        Status::Backoff => ("proxy unreachable; retrying later", "warn"),
    };
    Some(MenuItem::new("status", text).style(style))
}

/// The groups of one instance, from its cache.
fn instance(inst: &Instance, state: &Path, now: i64) -> Vec<MenuGroup> {
    let id = instance_id(&inst.origin.to_string());
    let key = keysource::read(&inst.key);
    let snap = match &key {
        Ok(k) => {
            collect::cached(&collect::Ctx {
                inst,
                key: k,
                state_dir: state,
                now,
                fetch: &[],
            })
            .0
        }
        Err(_) => {
            let c = crate::cache::load(&crate::cache::file(state, &inst.origin.to_string()));
            collect::Snapshot {
                status: Status::NoKey,
                accounts: c.accounts,
            }
        }
    };
    let mut out = Vec::new();
    if let Some(row) = status_row(&snap.status, snap.accounts.is_empty()) {
        out.push(
            MenuGroup::new(&format!("info@{id}"), MenuKind::Info)
                .label(&host(inst))
                .item(row),
        );
    }
    for p in &inst.providers {
        let accts: Vec<&Acct> = snap.accounts.iter().filter(|a| &a.provider == p).collect();
        if accts.is_empty() {
            continue;
        }
        let top = sole_top(&accts).map(|a| a.key.as_str());
        let label = format!("{} \u{B7} {}", title(p), host(inst));
        let mut radio = MenuGroup::new(&format!("{p}@{id}"), MenuKind::Radio).label(&label);
        for a in &accts {
            radio = radio.item(account_item(a, &id, top == Some(a.key.as_str()), now));
        }
        let t = [("instance", id.as_str()), ("provider", p.as_str())];
        out.push(radio);
        out.push(
            MenuGroup::new(&format!("go-{p}@{id}"), MenuKind::Buttons)
                .item(MenuItem::new("next", "Next: most quota left").action("next", &t))
                .item(MenuItem::new("next-soon", "Next: resets soonest").action("next-soon", &t)),
        );
    }
    out
}

/// The menu of every configured instance.
pub fn build(cfg: &Config, state: &Path, now: i64) -> Menu {
    Menu {
        groups: cfg
            .instances
            .iter()
            .flat_map(|i| instance(i, state, now))
            .collect(),
    }
    .finish()
}
