//! `tray <plugin>[.<action>]`, `set tray-views`, `set tray-poll`: what the
//! tray app may show and run. User config only.

use super::{duration, Config};

/// Seconds between idle polls unless `set tray-poll` says otherwise.
pub const DEFAULT_POLL: u64 = 30;
const POLL_RANGE: std::ops::RangeInclusive<u64> = 10..=600;

/// One `tray` line: a plugin's menu actions, all of them (`action` is
/// `None`) or one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayGrant {
    pub plugin: String,
    pub action: Option<String>,
}

pub(super) fn apply(c: &mut Config, args: &[String]) -> Result<(), String> {
    let usage = "usage: tray <plugin>[.<action>]";
    let [target] = args else {
        return Err(usage.into());
    };
    let ok = |n: &str| crate::click::valid_toggle_name(n);
    let (plugin, action) = match target.split_once('.') {
        Some((p, a)) => (p, Some(a)),
        None => (target.as_str(), None),
    };
    if !ok(plugin) || !action.is_none_or(ok) {
        return Err(format!(
            "invalid tray target `{target}`: names are letters, digits, `-` and `_`; {usage}"
        ));
    }
    let g = TrayGrant {
        plugin: plugin.into(),
        action: action.map(Into::into),
    };
    if !c.tray.contains(&g) {
        c.tray.push(g);
    }
    Ok(())
}

pub(super) fn views(val: &str) -> Result<bool, String> {
    match val {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(format!("invalid tray-views `{val}` (on|off)")),
    }
}

pub(super) fn poll(val: &str) -> Result<u64, String> {
    duration(val)
        .ok()
        .filter(|s| POLL_RANGE.contains(s))
        .ok_or_else(|| format!("invalid tray-poll `{val}` (a duration like 30s, 10s..10m)"))
}
