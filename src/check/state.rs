//! What `bar check --format json` reports as `state`: the files in bar's
//! state dir that change what renders, read without writing anything or
//! starting a background job.

use crate::cache::{hash_hex, modified_age, Cache};
use crate::click::{self, SESSION_TOGGLES};
use crate::update;
use serde_json::{json, Value};
use std::path::Path;

/// The `state` object for state dir `dir`; `update_check` is the
/// config's `set update-check`.
pub fn snapshot(dir: &Path, update_check: bool) -> Value {
    json!({
        "dir": dir.display().to_string(),
        "toggles": toggles(dir),
        "sessions": sessions(dir),
        "cache": cache(dir),
        "update": update_state(
            &Cache::new(dir),
            update::current(),
            update_check,
            &std::env::current_exe().unwrap_or_else(|_| "bar".into()),
        ),
    })
}

/// The view a toggle shows, in `bar toggle`'s words.
fn view(key: &str, on: bool) -> &'static str {
    match (click::is_usage_key(key), on) {
        (true, true) => "pct",
        (true, false) => "bar",
        (false, true) => "alt",
        (false, false) => "show",
    }
}

/// The key a toggle file stands for: `pct-<5h|7d>` or `toggle-<key>`.
fn toggle_key(file: &str) -> Option<&str> {
    let key = file
        .strip_prefix("pct-")
        .filter(|k| click::is_usage_key(k))
        .or_else(|| file.strip_prefix("toggle-"))?;
    (!key.is_empty() && !key.starts_with('.')).then_some(key)
}

/// File names in `dir`, sorted; none when it cannot be read.
fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    out.sort();
    out
}

/// The global toggles: `5h` and `7d` always, plugins while on.
fn toggles(dir: &Path) -> Vec<Value> {
    let files = names(dir);
    let on: Vec<&str> = files.iter().filter_map(|f| toggle_key(f)).collect();
    let mut out: Vec<Value> = ["5h", "7d"]
        .iter()
        .map(|k| json!({"key": k, "view": view(k, on.contains(k))}))
        .collect();
    out.extend(
        on.iter()
            .filter(|k| !click::is_usage_key(k))
            .map(|k| json!({"key": k, "view": view(k, true)})),
    );
    out
}

/// Sessions with toggles of their own, each with what it set.
fn sessions(dir: &Path) -> Vec<Value> {
    let root = dir.join(SESSION_TOGGLES);
    names(&root)
        .into_iter()
        .filter_map(|sid| {
            let own = click::session_toggles_dir(dir, &sid)?;
            let toggles: Vec<Value> = names(&own)
                .iter()
                .filter_map(|f| {
                    let key = toggle_key(f)?;
                    let on = match std::fs::read_to_string(own.join(f)).ok()?.trim() {
                        "on" => true,
                        "off" => false,
                        _ => return None,
                    };
                    Some(json!({"key": key, "view": view(key, on)}))
                })
                .collect();
            Some(json!({"session": sid, "toggles": toggles}))
        })
        .collect()
}

/// Entries in the background cache and refreshes that failed lately.
fn cache(dir: &Path) -> Value {
    let cache = dir.join("cache");
    let files = names(&cache);
    let hex = |s: &str| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit());
    let entries = files.iter().filter(|f| hex(f)).count();
    let failing = files
        .iter()
        .filter(|f| f.strip_suffix(".fail").is_some_and(hex))
        .count();
    json!({
        "dir": cache.display().to_string(),
        "entries": entries,
        "failing": failing,
    })
}

/// The update check, as `bar update --format json` and `bar check`'s
/// `state.update` report it: `check` (whether it is on), `status` (`off`,
/// `pending` before a first result, `failed` when the last check found
/// nothing, `ok`), `current`, `channel`, `latest` (the newest release of
/// the running version's channel, else null), `age_secs` (age of that
/// result, else null), `stale` (older than a day), `newer` (the newer
/// version, else null), `dev` (a development build, never compared) and
/// `upgrade` (how to upgrade the binary `exe`). Reads the cache only.
pub fn update_state(cache: &Cache, current: &str, on: bool, exe: &Path) -> Value {
    let pre = update::channel_is_prerelease(current);
    let key = update::cache_key(pre);
    let found = if on {
        cache.read_entry(&hash_hex(key))
    } else {
        None
    };
    let age = found
        .as_ref()
        .and_then(|_| modified_age(&cache.path_for(key)))
        .map(|a| a.as_secs());
    let latest = found.as_deref().map(str::trim).filter(|t| !t.is_empty());
    let status = match (on, &found, latest) {
        (false, ..) => "off",
        (true, None, _) => "pending",
        (true, Some(_), None) => "failed",
        (true, Some(_), Some(_)) => "ok",
    };
    let newer = latest.and_then(|t| update::newer(current, t));
    let target = if newer.is_some() { latest } else { None };
    json!({
        "check": on,
        "status": status,
        "current": current,
        "channel": if pre { "prerelease" } else { "stable" },
        "latest": latest,
        "age_secs": age,
        // the cache refreshes from this age on
        "stale": age.is_some_and(|a| a >= update::TTL),
        "newer": newer,
        "dev": update::is_dev_build(current),
        "upgrade": update::upgrade_hint(exe, target),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = "/usr/local/bin/bar";

    #[test]
    fn toggle_files_map_to_keys() {
        assert_eq!(toggle_key("pct-5h"), Some("5h"));
        assert_eq!(toggle_key("pct-1d"), None);
        assert_eq!(toggle_key("toggle-ci"), Some("ci"));
        assert_eq!(toggle_key("toggle-ci.icon"), Some("ci.icon"));
        assert_eq!(toggle_key("toggle-"), None);
        assert_eq!(toggle_key("click-installed"), None);
    }

    #[test]
    fn prerelease_builds_read_their_channel() {
        let dir = std::env::temp_dir().join(format!("bar-upd-state-{}", std::process::id()));
        let c = Cache::new(&dir);
        c.store("update:prerelease", "v1.0.0-rc.1").unwrap();
        let v = update_state(&c, "1.0.0-beta.0", true, Path::new(EXE));
        assert_eq!(v["channel"], "prerelease");
        assert_eq!(v["latest"], "v1.0.0-rc.1");
        assert_eq!(
            update_state(&c, "1.0.0", true, Path::new(EXE))["status"],
            "pending"
        );
        assert_eq!(
            update_state(&c, "1.0.0-beta.0", false, Path::new(EXE))["latest"],
            Value::Null
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_states() {
        let dir = std::env::temp_dir().join(format!("bar-upd-states-{}", std::process::id()));
        let c = Cache::new(&dir);
        let v = update_state(&c, "1.0.0", false, Path::new(EXE));
        assert_eq!((&v["check"], &v["status"]), (&false.into(), &"off".into()));
        assert_eq!(v["age_secs"], Value::Null);
        assert_eq!(v["stale"], false);
        assert_eq!(
            update_state(&c, "1.0.0", true, Path::new(EXE))["status"],
            "pending"
        );
        c.store("update:stable", "").unwrap();
        assert_eq!(
            update_state(&c, "1.0.0", true, Path::new(EXE))["status"],
            "failed"
        );
        c.store("update:stable", "v1.2.0").unwrap();
        let v = update_state(&c, "1.0.0", true, Path::new(EXE));
        assert_eq!(v["status"], "ok");
        assert_eq!(v["latest"], "v1.2.0");
        assert_eq!(v["newer"], "1.2.0");
        assert_eq!(v["dev"], false);
        assert_eq!(v["stale"], false);
        assert!(v["upgrade"].as_str().unwrap().contains("v1.2.0"), "{v}");
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(update::TTL + 1);
        std::fs::File::options()
            .write(true)
            .open(c.path_for("update:stable"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        let v = update_state(&c, "1.0.0", true, Path::new(EXE));
        assert!(
            v["age_secs"].as_u64().is_some_and(|a| a > update::TTL),
            "{v}"
        );
        assert_eq!(v["stale"], true);
        let v = update_state(&c, "0.0.0", true, Path::new(EXE));
        assert_eq!((&v["dev"], &v["newer"]), (&true.into(), &Value::Null));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
