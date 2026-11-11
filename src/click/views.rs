//! `bar toggle` from the command line: the keys it accepts, the set form
//! (`bar toggle <key> <value>`) and the view names it prints.

use std::path::{Path, PathBuf};

use super::{hook, session_toggles_dir, toggle_file, write_atomic, write_private, KEYS};

/// `bar toggle <arg> [<value>]`: flips `arg`, or sets it to `value`, and
/// returns the key and its view name (printed `<key>: <view>`). The key
/// is checked as for [`plan`].
pub fn cli<'a>(
    dir: &Path,
    arg: &'a str,
    value: Option<&str>,
) -> Result<(&'a str, &'static str), String> {
    let p = plan(dir, arg, value)?;
    set_in(dir, p.key, p.sid, p.proposed)?;
    Ok((p.key, view_name(p.key, p.proposed)))
}

/// What `bar toggle <arg> [<value>]` would do, writing nothing.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan<'a> {
    pub key: &'a str,
    pub sid: Option<&'a str>,
    /// The state now and after.
    pub current: bool,
    pub proposed: bool,
    /// The file created, updated or removed; `None` when nothing changes.
    pub change: Option<(&'static str, PathBuf)>,
}

/// Plans `bar toggle <arg> [<value>]`. A key typed without a session
/// must name a view or an installed plugin (see [`check_key`]); the
/// handler's `<key>?sid=<id>` form is checked for its shape only, as the
/// link came from a render whose `PATH` may differ from the handler's.
pub fn plan<'a>(dir: &Path, arg: &'a str, value: Option<&str>) -> Result<Plan<'a>, String> {
    let (key, sid) = super::toggle_target(arg)?;
    if sid.is_none() {
        let dirs = crate::plugin::plugin_dirs(std::env::var_os("PATH").as_deref());
        check_key(key, &|n| crate::plugin::find(&dirs, n).is_some(), &|| {
            crate::plugin::discover(&dirs)
                .into_iter()
                .map(|f| f.name)
                .collect()
        })?;
    }
    let f = toggle_file(dir, key).ok_or_else(|| {
        format!("unknown key `{key}` (expected 5h, 7d, a plugin name or <plugin>.<toggle>)")
    })?;
    let current = match sid {
        Some(sid) => super::is_toggled_in(dir, key, sid),
        None => f.exists(),
    };
    let proposed = match value {
        Some(v) => parse_value(key, v)?,
        None => !current,
    };
    let change = match sid {
        None => match (proposed, f.exists()) {
            (true, false) => Some(("create", f)),
            (false, true) => Some(("remove", f)),
            _ => None,
        },
        Some(sid) => {
            let name = f.file_name().unwrap_or_default();
            let own = session_toggles_dir(dir, sid)
                .ok_or_else(|| format!("bad session id `{sid}`"))?
                .join(name);
            match super::session_state(dir, sid, name) {
                Some(s) if s == proposed => None,
                _ if own.exists() => Some(("update", own)),
                _ => Some(("create", own)),
            }
        }
    };
    Ok(Plan {
        key,
        sid,
        current,
        proposed,
        change,
    })
}

/// The view name `bar toggle` prints for `key` in state `on`: `pct` /
/// `bar` for usage, `alt` / `show` for a plugin's text or named toggle.
pub fn view_name(key: &str, on: bool) -> &'static str {
    match (KEYS.contains(&key), on) {
        (true, true) => "pct",
        (true, false) => "bar",
        (false, true) => "alt",
        (false, false) => "show",
    }
}

/// The state `value` asks for `key`: `on` / `off`, or the view name
/// [`view_name`] prints for that kind of key.
pub fn parse_value(key: &str, value: &str) -> Result<bool, String> {
    let (on, off) = (view_name(key, true), view_name(key, false));
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        v if v == on => Ok(true),
        v if v == off => Ok(false),
        _ => Err(format!(
            "bad value `{value}` for `{key}` (expected {on}, {off}, on or off)"
        )),
    }
}

/// Checks a key typed on the command line: `5h`, `7d`, the name of an
/// installed plugin (`known`), or `<plugin>.<toggle>` for one, the toggle
/// a valid name. Plugins are not run: the toggles a plugin names come
/// with its render output, so any valid name is accepted after a known
/// plugin. `installed` lists the plugins for the refusal.
pub fn check_key(
    key: &str,
    known: &dyn Fn(&str) -> bool,
    installed: &dyn Fn() -> Vec<String>,
) -> Result<(), String> {
    let ok = KEYS.contains(&key)
        || match key.split_once('.') {
            Some((name, sub)) => hook::valid_plugin_name(sub) && plugin_ok(name, known),
            None => plugin_ok(key, known),
        };
    if ok {
        return Ok(());
    }
    Err(unknown_key(key, &installed()))
}

fn plugin_ok(name: &str, known: &dyn Fn(&str) -> bool) -> bool {
    hook::valid_plugin_name(name) && !KEYS.contains(&name) && known(name)
}

/// The refusal for an unknown key, naming the valid ones.
pub fn unknown_key(key: &str, plugins: &[String]) -> String {
    let usage = KEYS.join(", ");
    if plugins.is_empty() {
        return format!("unknown key `{key}`; valid keys: {usage} (no plugin installed)");
    }
    format!(
        "unknown key `{key}`; valid keys: {usage}, {}, or <plugin>.<toggle> for one of those plugins",
        plugins.join(", ")
    )
}

/// Sets `key` on or off, for session `sid` only or (without one) the
/// global default; the same call twice leaves the same state.
pub fn set_in(dir: &Path, key: &str, sid: Option<&str>, on: bool) -> Result<(), String> {
    let f = toggle_file(dir, key).ok_or_else(|| {
        format!("unknown key `{key}` (expected 5h, 7d, a plugin name or <plugin>.<toggle>)")
    })?;
    let Some(sid) = sid else {
        return match (on, f.exists()) {
            (true, _) => write_private(&f, ""),
            (false, true) => std::fs::remove_file(&f),
            (false, false) => Ok(()),
        }
        .map_err(|e| e.to_string());
    };
    let own = session_toggles_dir(dir, sid).ok_or_else(|| format!("bad session id `{sid}`"))?;
    let root = dir.join(super::SESSION_TOGGLES);
    // a planted symlink must not redirect the write
    for d in [&root, &own] {
        if std::fs::symlink_metadata(d).is_ok() && !hook::is_real_dir(d) {
            return Err(format!("{} is not a directory", d.display()));
        }
    }
    super::prune_sessions(&root, sid);
    let name = f.file_name().unwrap_or_default();
    write_atomic(&own.join(name), if on { "on" } else { "off" }).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> Vec<String> {
        Vec::new()
    }

    #[test]
    fn keys_of_a_known_plugin_pass() {
        let known = |n: &str| n == "tlc";
        for k in ["5h", "7d", "tlc", "tlc.scope"] {
            assert_eq!(check_key(k, &known, &none), Ok(()), "{k}");
        }
        for k in ["bogus", "bogus.x", "tlc.", "tlc.a.b", "5h.x", "", "../tlc"] {
            assert!(check_key(k, &known, &none).is_err(), "{k}");
        }
    }

    #[test]
    fn refusal_names_valid_keys() {
        assert_eq!(
            unknown_key("x", &[]),
            "unknown key `x`; valid keys: 5h, 7d (no plugin installed)"
        );
        assert_eq!(
            unknown_key("x", &["a".into(), "b".into()]),
            "unknown key `x`; valid keys: 5h, 7d, a, b, or <plugin>.<toggle> for one of those plugins"
        );
    }

    #[test]
    fn plan_writes_nothing_and_names_the_file() {
        let dir = std::env::temp_dir().join(format!("bar-views-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = plan(&dir, "5h", None).unwrap();
        assert_eq!((p.current, p.proposed), (false, true));
        assert_eq!(p.change, Some(("create", dir.join("pct-5h"))));
        assert!(!dir.exists());
        let sid = "0123abcd-0000-0000-0000-000000000000";
        let arg = format!("5h?sid={sid}");
        let p = plan(&dir, &arg, Some("pct")).unwrap();
        let own = dir
            .join(crate::click::SESSION_TOGGLES)
            .join(sid)
            .join("pct-5h");
        assert_eq!(p.change, Some(("create", own.clone())));
        set_in(&dir, "5h", Some(sid), true).unwrap();
        assert_eq!(plan(&dir, &arg, Some("pct")).unwrap().change, None);
        assert_eq!(
            plan(&dir, &arg, None).unwrap().change,
            Some(("update", own))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn values_by_kind() {
        assert_eq!(parse_value("5h", "pct"), Ok(true));
        assert_eq!(parse_value("5h", "bar"), Ok(false));
        assert_eq!(parse_value("tlc", "alt"), Ok(true));
        assert_eq!(parse_value("tlc.scope", "show"), Ok(false));
        assert_eq!(parse_value("tlc", "on"), Ok(true));
        assert!(parse_value("5h", "alt").is_err());
        assert!(parse_value("tlc", "pct").is_err());
    }
}
