//! `bar tray run <id>`: runs one item of the menu snapshot. The tray
//! passes an id and flags, never a token value or a secret: the plugin,
//! action and tokens all come from `tray-menu.json`, which only `bar tray
//! state` writes. Every check fails closed, in this order: the id is well
//! formed; the snapshot is there, private, this user's and recent, and is
//! the one the tray saw (`--generation`); the item exists, is enabled and
//! names an action with well-formed tokens; a `tray` line grants the
//! action; the plugin is found and declares the action with those token
//! names; an action declared with `confirm` was confirmed. Then the
//! action runs exactly as a `bar://action` link would, and the snapshot
//! is spent: its modification time goes to zero, so the same click cannot
//! run twice before the tray fetches a new menu.

use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{plugins, valid_name, SNAPSHOT_FILE};
use crate::cache::Cache;
use crate::click::action::{self, valid_token_name, valid_token_value, Request};
use crate::conf::Config;
use crate::plugin::{menu::valid_menu_id, MAX_TOKENS};

/// Oldest snapshot an item may run from. A live tray refreshes the
/// snapshot on every menu open and at least every `tray-poll` (10 min at
/// most), so a snapshot older than that was left by a tray that is gone
/// or by a hand-run `bar tray state`; its tokens (account indices,
/// instance ids) may since have changed meaning.
pub const MAX_AGE: Duration = Duration::from_secs(600);
/// Longest snapshot read; the writer stays far below.
const MAX_BYTES: u64 = 4 << 20;
const STALE: &str = "menu is stale, reopen it";

/// The three parts of a dispatch id `<plugin>.<group>.<item>`, each in
/// its own character set; `None` for anything else.
pub fn parse_id(id: &str) -> Option<(&str, &str, &str)> {
    if id.len() > plugins::MAX_ID {
        return None;
    }
    let mut parts = id.split('.');
    let (p, g, i) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || !valid_name(p) || !valid_menu_id(g) || !valid_menu_id(i) {
        return None;
    }
    Some((p, g, i))
}

/// Reads the snapshot as `uid` would: a plain file in `state`, mode 0600
/// (Unix), owned by `uid` (when both are known), modified within
/// [`MAX_AGE`], a version 1 object within [`MAX_BYTES`].
pub fn load_as(state: &Path, uid: Option<u32>, now: SystemTime) -> Result<Value, String> {
    let path = state.join(SNAPSHOT_FILE);
    let f = std::fs::File::open(&path).map_err(|_| "no menu, reopen it".to_string())?;
    let meta = f.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("menu file is not a file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err("menu file is not private".into());
        }
    }
    if let (Some(uid), Some(owner)) = (uid, crate::platform::owner(&meta)) {
        if owner != uid {
            return Err("menu file is owned by another user".into());
        }
    }
    let age = meta
        .modified()
        .ok()
        .and_then(|m| now.duration_since(m).ok())
        .ok_or(STALE)?;
    if age > MAX_AGE {
        return Err(STALE.into());
    }
    // a bigger file is cut short and so never parses
    let mut raw = String::new();
    f.take(MAX_BYTES)
        .read_to_string(&mut raw)
        .map_err(|_| STALE.to_string())?;
    let v: Value = serde_json::from_str(&raw).map_err(|_| STALE.to_string())?;
    if v["version"] != super::state::VERSION || !v["generation"].is_string() {
        return Err(STALE.into());
    }
    Ok(v)
}

/// [`load_as`] for the current user, now.
pub fn load(state: &Path) -> Result<Value, String> {
    load_as(state, crate::platform::current_uid(), SystemTime::now())
}

/// One item of the snapshot, as a run needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub action: String,
    pub tokens: BTreeMap<String, String>,
    pub confirm: bool,
}

/// The item `(plugin, group, item)` names, enabled and with an action
/// whose tokens are all well formed.
pub fn item(snapshot: &Value, id: (&str, &str, &str)) -> Result<Item, String> {
    let find = |list: &Value, key: &str, id: &str| -> Option<Value> {
        list.as_array()?.iter().find(|x| x[key] == id).cloned()
    };
    let (p, g, i) = id;
    let it = find(&snapshot["plugins"], "name", p)
        .and_then(|p| find(&p["groups"], "id", g))
        .and_then(|g| find(&g["items"], "id", i))
        .ok_or("item is not in the menu")?;
    if it.get("enabled").is_some_and(|e| e != true) {
        return Err("item is disabled".into());
    }
    let action = it["action"]
        .as_str()
        .filter(|a| valid_token_name(a))
        .ok_or("item has no action")?;
    let tokens = it["tokens"]
        .as_object()
        .filter(|o| o.len() <= MAX_TOKENS)
        .ok_or("bad tokens in the menu")?;
    let mut out = BTreeMap::new();
    for (k, v) in tokens {
        let v = v.as_str().filter(|v| valid_token_value(v));
        match v {
            Some(v) if valid_token_name(k) => out.insert(k.clone(), v.to_string()),
            _ => return Err("bad tokens in the menu".into()),
        };
    }
    Ok(Item {
        action: action.to_string(),
        tokens: out,
        confirm: it["confirm"] == true,
    })
}

/// Makes the snapshot older than [`MAX_AGE`] without changing its
/// contents: the next `bar tray run` is refused until `bar tray state`
/// writes a new one.
fn spend(path: &Path) {
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = f.set_modified(SystemTime::UNIX_EPOCH);
    }
}

/// What the tray asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pick<'a> {
    pub id: &'a str,
    /// The snapshot the tray built its menu from; `None` skips the check.
    pub generation: Option<&'a str>,
    pub confirmed: bool,
}

/// The tray door, up to the shared checks.
fn checked(
    state: &Path,
    config: &Config,
    dirs: &[PathBuf],
    cache: &Cache,
    pick: &Pick,
) -> Result<action::Verified, String> {
    let id = parse_id(pick.id).ok_or("bad item id")?;
    let snapshot = load(state)?;
    if pick.generation.is_some_and(|g| snapshot["generation"] != g) {
        return Err("menu changed, reopen it".into());
    }
    let it = item(&snapshot, id)?;
    let plugin = id.0;
    if !plugins::granted(&config.tray, plugin, &it.action) {
        return Err(format!("no `tray` line for {plugin}.{}", it.action));
    }
    let v = action::verify(
        cache,
        dirs,
        Request {
            plugin: plugin.to_string(),
            action: it.action,
            tokens: it.tokens,
        },
    )?;
    if (v.spec.confirm || it.confirm) && !pick.confirmed {
        return Err("needs confirmation".into());
    }
    Ok(v)
}

/// Checks the pick, runs the item's action (recording the outcome in
/// `action-status` as a link would) and spends the snapshot. `Ok` is the
/// first line the action printed; `Err` says why nothing ran, or that
/// the plugin failed.
pub fn run(state: &Path, config: &Config, dirs: &[PathBuf], pick: &Pick) -> Result<String, String> {
    let cache = Cache::new(state);
    let r = checked(state, config, dirs, &cache, pick);
    let path = state.join(SNAPSHOT_FILE);
    action::finish(state, config, r.map(|v| (v, move || spend(&path))))
}
