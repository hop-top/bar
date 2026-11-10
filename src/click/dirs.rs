//! The dirs the `bar://` handler uses. LaunchServices starts the handler
//! app with the login session's environment, not the shell's, so
//! `bar click install` bakes the dir variables of the installing process
//! into the handler and records the dirs they resolve to in the bundle;
//! `bar check` compares that record with the current environment.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Every variable that moves a dir `bar toggle`, `bar click run` or
/// `bar click action` reads: the state dir, the XDG bases, Claude Code's
/// config dir (its `sessions/`), tmux's socket dir, and `PATH`, where an
/// action finds its plugin.
pub const HANDLER_ENV: [&str; 7] = [
    "BAR_STATE_DIR",
    "XDG_STATE_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "CLAUDE_CONFIG_DIR",
    "TMUX_TMPDIR",
    "PATH",
];

/// The variables that pick the state dir; with neither set the handler
/// follows the renderer's pointer (see `pointer`).
const STATE_ENV: [&str; 2] = ["BAR_STATE_DIR", "XDG_STATE_HOME"];

/// A variable and its value; `None` when unset or empty.
pub type EnvVar = (String, Option<String>);

/// `HANDLER_ENV` as `get` sees it; empty counts as unset.
pub fn handler_env(get: &dyn Fn(&str) -> Option<String>) -> Vec<EnvVar> {
    HANDLER_ENV
        .iter()
        .map(|k| (k.to_string(), get(k).filter(|v| !v.is_empty())))
        .collect()
}

/// POSIX single quotes: nothing inside is special but `'` itself.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `unset <unset vars>; <VAR='value' …> '<bar>'`, the prefix of every
/// command the handler runs: variables set at install are set, the rest
/// unset, whatever the handler's own environment holds.
pub fn shell_command(bar: &Path, env: &[EnvVar]) -> String {
    let unset: Vec<&str> = env
        .iter()
        .filter(|(_, v)| v.is_none())
        .map(|(k, _)| k.as_str())
        .collect();
    let mut out = String::new();
    if !unset.is_empty() {
        out.push_str(&format!("unset {}; ", unset.join(" ")));
    }
    for (k, v) in env {
        if let Some(v) = v {
            out.push_str(&format!("{k}={} ", sh_quote(v)));
        }
    }
    out.push_str(&sh_quote(&bar.display().to_string()));
    out
}

/// Whether a handler with `env` follows the pointer: no state variable.
pub fn follows_pointer(env: &[EnvVar]) -> bool {
    env.iter()
        .all(|(k, v)| v.is_none() || !STATE_ENV.contains(&k.as_str()))
}

/// What `bar click install` records in the handler bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerDirs {
    pub state_dir: PathBuf,
    pub claude_dir: Option<PathBuf>,
    pub follows_pointer: bool,
}

impl HandlerDirs {
    pub fn path(app: &Path) -> PathBuf {
        app.join("Contents/Resources/bar-dirs.json")
    }

    pub fn to_json(&self) -> String {
        let v = json!({
            "state_dir": self.state_dir.display().to_string(),
            "claude_dir": self.claude_dir.as_ref().map(|p| p.display().to_string()),
            "follows_pointer": self.follows_pointer,
        });
        format!("{v}\n")
    }

    /// `None` when the bundle has no record or it does not parse.
    pub fn read(app: &Path) -> Option<HandlerDirs> {
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(Self::path(app)).ok()?).ok()?;
        Some(HandlerDirs {
            state_dir: PathBuf::from(v.get("state_dir")?.as_str()?),
            claude_dir: v.get("claude_dir")?.as_str().map(PathBuf::from),
            follows_pointer: v.get("follows_pointer")?.as_bool()?,
        })
    }
}

const REINSTALL: &str = "re-run `bar click install` from the environment Claude Code runs in";

/// Why clicks on the handler at `app` may not reach this environment's
/// dirs (`state`, `claude`) or those of the last `bar render`
/// (`pointer`). Empty when no handler is installed there.
pub fn handler_warnings(
    app: &Path,
    state: &Path,
    claude: Option<&Path>,
    pointer: Option<&Path>,
) -> Vec<String> {
    if !app.exists() {
        return Vec::new();
    }
    let Some(r) = HandlerDirs::read(app) else {
        return vec![format!(
            "the bar:// handler {} records no dirs, so its toggles may land elsewhere; {REINSTALL}",
            app.display()
        )];
    };
    let handler = r.state_dir.as_path();
    let mut out = Vec::new();
    let mut differs = |what: &str, dir: &Path| {
        out.push(format!(
            "the bar:// handler uses state dir {}, but {what} uses {}: toggles will not apply; {REINSTALL}",
            handler.display(),
            dir.display()
        ));
    };
    // a following handler uses the renderer's dir whenever there is a pointer
    let follows = r.follows_pointer && pointer.is_some();
    if !follows && handler != state {
        differs("this environment", state);
    } else if let Some(p) = pointer.filter(|p| !follows && *p != handler) {
        differs("`bar render`", p);
    }
    if r.claude_dir.as_deref() != claude {
        let show = |d: Option<&Path>| d.map_or("none".to_string(), |d| d.display().to_string());
        out.push(format!(
            "the bar:// handler reads Claude Code sessions from {}, but this environment uses {}: command links will refuse; {REINSTALL}",
            show(r.claude_dir.as_deref()),
            show(claude)
        ));
    }
    out
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn the_install_path_is_baked_into_the_handler() {
        let env = handler_env(&|k| (k == "PATH").then(|| "/home/u/.local/bin:/usr/bin".into()));
        let cmd = shell_command(Path::new("/b/bar"), &env);
        assert!(cmd.contains("PATH='/home/u/.local/bin:/usr/bin' "), "{cmd}");
    }
}
