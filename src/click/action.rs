//! `bar://action/<plugin>.<action>?<token>=<value>&k=<entry>&n=<nonce>`:
//! links that run a plugin's declared action, and the handler behind them.
//! Any page or app can open a `bar://` URL, so every check here fails
//! closed: the per-install nonce, the user's `action` line, the plugin's
//! own declaration, and token values that are in the cached output the
//! link was drawn from.

use super::hook::valid_plugin_name;
use crate::cache::{run_child, Cache};
use crate::conf::Config;
use crate::plugin::{self, parse_output};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Per-install secret, mode 0600 in the state dir, written by `bar click
/// install`. Its presence also says the installed handler routes actions.
pub const NONCE_FILE: &str = "action-nonce";
/// One line: when and how the last action ended.
pub const STATUS_FILE: &str = "action-status";
const PREFIX: &str = "bar://action/";
/// Limit for a plugin's `--action` run.
pub const ACTION_TIMEOUT: Duration = Duration::from_secs(10);

/// Token names: `[a-z0-9_-]`, 1 to 32.
pub fn valid_token_name(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Token values: `[A-Za-z0-9._@-]`, 1 to 64; nothing that needs escaping
/// in a URL or means anything to a shell.
pub fn valid_token_value(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'@' | b'-'))
}

fn hex32(s: &str) -> bool {
    s.len() == 32
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The install's nonce; `None` when the handler was not installed with
/// action support (or the file is not a nonce).
pub fn nonce(state: &Path) -> Option<String> {
    let path = state.join(NONCE_FILE);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // a secret other users can read is no secret
        if std::fs::metadata(&path).ok()?.permissions().mode() & 0o077 != 0 {
            return None;
        }
    }
    let s = std::fs::read_to_string(path).ok()?;
    let s = s.trim();
    hex32(s).then(|| s.to_string())
}

/// Whether links may be drawn: the handler routes actions.
pub fn handler_routes_actions(state: &Path) -> bool {
    nonce(state).is_some()
}

/// Creates the nonce when there is none (mode 0600) and returns it. Kept
/// across installs, so links already drawn stay valid.
pub fn ensure_nonce(state: &Path) -> Result<String, String> {
    if let Some(n) = nonce(state) {
        return Ok(n);
    }
    let n = random_hex()?;
    // a fresh 0600 file renamed into place: a loose file already there
    // does not keep its mode
    super::write_atomic(&state.join(NONCE_FILE), &n).map_err(|e| e.to_string())?;
    Ok(n)
}

#[cfg(unix)]
fn random_hex() -> Result<String, String> {
    use std::io::Read;
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|e| format!("no random source: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

#[cfg(not(unix))]
fn random_hex() -> Result<String, String> {
    Err("action links need macOS".into())
}

/// The link for `plugin.action` with `tokens`, drawn from cache entry
/// `entry`; `None` without a nonce or with a token that is not valid.
pub fn build(
    state: &Path,
    plugin: &str,
    action: &str,
    tokens: &BTreeMap<String, String>,
    entry: &str,
) -> Option<String> {
    let n = nonce(state)?;
    if !valid_plugin_name(plugin) || !valid_plugin_name(action) || !hex32(entry) {
        return None;
    }
    let mut q = String::new();
    for (k, v) in tokens {
        if !valid_token_name(k) || !valid_token_value(v) || matches!(k.as_str(), "k" | "n") {
            return None;
        }
        q.push_str(&format!("{k}={v}&"));
    }
    Some(format!("{PREFIX}{plugin}.{action}?{q}k={entry}&n={n}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionUrl {
    pub plugin: String,
    pub action: String,
    pub tokens: BTreeMap<String, String>,
    pub entry: String,
    pub nonce: String,
}

/// Strict: exact scheme and path shape; token, `k` and `n` keys once each
/// and nothing else; every name and value in its character set.
pub fn parse(url: &str) -> Result<ActionUrl, String> {
    let bad = || "malformed link".to_string();
    let rest = url.strip_prefix(PREFIX).ok_or_else(bad)?;
    let (head, query) = rest.split_once('?').ok_or_else(bad)?;
    let (plugin, action) = head.split_once('.').ok_or_else(bad)?;
    if !valid_plugin_name(plugin) || !valid_plugin_name(action) {
        return Err(bad());
    }
    let (mut tokens, mut entry, mut nonce) = (BTreeMap::new(), None, None);
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').ok_or_else(bad)?;
        let dup = match k {
            "k" if hex32(v) => entry.replace(v).is_some(),
            "n" if hex32(v) => nonce.replace(v).is_some(),
            _ if valid_token_name(k) && k != "k" && k != "n" && valid_token_value(v) => {
                tokens.insert(k.to_string(), v.to_string()).is_some()
            }
            _ => return Err(bad()),
        };
        if dup || tokens.len() > crate::plugin::MAX_TOKENS {
            return Err(bad());
        }
    }
    Ok(ActionUrl {
        plugin: plugin.into(),
        action: action.into(),
        tokens,
        entry: entry.ok_or_else(bad)?.into(),
        nonce: nonce.ok_or_else(bad)?.into(),
    })
}

/// Checks the link, then runs `bar-<plugin> --action <action>` with
/// `{"action": .., "tokens": {..}}` on stdin (10 s, no shell, in the state
/// dir). Records the outcome in [`STATUS_FILE`] and ages the cache entry
/// the link came from, so the change shows at the next render. `Err` says
/// why nothing ran, or that the plugin failed.
pub fn run(state: &Path, url: &str, config: &Config, dirs: &[PathBuf]) -> Result<(), String> {
    let r = checked(state, url, config, dirs).and_then(|(u, found, cache)| {
        let input = serde_json::json!({"action": u.action, "tokens": u.tokens}).to_string();
        let mut c = std::process::Command::new(&found.path);
        c.arg("--action")
            .arg(&u.action)
            .env("BAR_STATE_DIR", state)
            .current_dir(crate::cache::outside_project(state));
        let ran = run_child(c, Some(input.into_bytes()), ACTION_TIMEOUT);
        cache.age_entry(&u.entry);
        ran.map(drop)
            .ok_or_else(|| format!("{}.{} failed", u.plugin, u.action))
    });
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let line = match &r {
        Ok(()) => format!("{at} ok"),
        Err(e) => format!("{at} refused: {e}"),
    };
    let _ = super::write_atomic(&state.join(STATUS_FILE), &format!("{line}\n"));
    r
}

fn checked(
    state: &Path,
    url: &str,
    config: &Config,
    dirs: &[PathBuf],
) -> Result<(ActionUrl, plugin::Found, Cache), String> {
    let u = parse(url)?;
    if nonce(state).as_deref() != Some(u.nonce.as_str()) {
        return Err("link was not made by this install".into());
    }
    let bound = config
        .actions
        .iter()
        .any(|b| b.plugin == u.plugin && b.action == u.action);
    if !bound {
        return Err(format!("no `action` line for {}.{}", u.plugin, u.action));
    }
    let found = plugin::find(dirs, &u.plugin).ok_or("plugin not found")?;
    let cache = Cache::new(state);
    let info = plugin::info_now(&cache, &found);
    let spec = info
        .actions
        .get(&u.action)
        .ok_or("the plugin does not declare that action")?;
    let given: Vec<&String> = u.tokens.keys().collect();
    if given != spec.tokens.iter().collect::<Vec<_>>() {
        return Err("tokens do not match the action's declaration".into());
    }
    let cached = cache
        .read_entry(&u.entry)
        .and_then(|raw| parse_output(&raw))
        .ok_or("the output the link came from is gone")?;
    if !cached.offers_tokens(&u.tokens) {
        return Err("token values are not in the plugin's current output".into());
    }
    Ok((u, found, cache))
}
