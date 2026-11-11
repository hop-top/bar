//! `--dirs <file>`: what `bar tray install` recorded in `tray-dirs.json`
//! (the `bar` to run, the state dir and the dir variables as set at
//! install), for a tray started where variables cannot be set, as the
//! Windows `Run` value starts it. The tray then runs `bar`, takes its
//! lock in that state dir and gives its children those variables.

use crate::backend::spawn;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The parts of `tray-dirs.json` the tray uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub bar: PathBuf,
    pub state_dir: PathBuf,
    /// Each recorded variable the tray passes on: `None` was unset at
    /// install. Names outside `spawn::HANDLER_ENV` are left out.
    pub env: Vec<(String, Option<String>)>,
}

#[derive(Deserialize)]
struct Raw {
    bar: String,
    state_dir: String,
    #[serde(default)]
    env: BTreeMap<String, Option<String>>,
}

/// Parses the record; both paths must be absolute.
pub fn parse(text: &str) -> Result<Record, String> {
    let raw: Raw = serde_json::from_str(text).map_err(|e| format!("tray-dirs.json: {e}"))?;
    let abs = |k: &str, s: String| {
        let p = PathBuf::from(&s);
        if p.is_absolute() {
            Ok(p)
        } else {
            Err(format!("tray-dirs.json: {k} is not absolute: {s}"))
        }
    };
    Ok(Record {
        bar: abs("bar", raw.bar)?,
        state_dir: abs("state_dir", raw.state_dir)?,
        env: raw
            .env
            .into_iter()
            .filter(|(k, _)| spawn::handler_var(k))
            .map(|(k, v)| (k, v.filter(|v| !v.is_empty())))
            .collect(),
    })
}

/// Reads and parses the record at `path`.
pub fn read(path: &Path) -> Result<Record, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&text)
}

impl Record {
    /// The single-instance lock in the recorded state dir.
    pub fn lock(&self) -> PathBuf {
        self.state_dir.join("tray.lock")
    }

    /// Makes this process's variables the recorded ones (set, or removed
    /// when unset at install), so every child sees what install saw.
    /// Call before any thread starts.
    pub fn apply_env(&self) {
        for (k, v) in &self.env {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `p`, absolute on this OS: Windows wants a drive.
    fn abs(p: &str) -> String {
        if cfg!(windows) {
            format!("C:{p}")
        } else {
            p.to_string()
        }
    }

    /// The JSON `text` with each string value starting `/` made [`abs`].
    fn json(text: &str) -> String {
        text.replace("\"/", &format!("\"{}", abs("/")))
    }

    const REC: &str = r#"{"bar":"/opt/bar/bar","tray":"/opt/bar/bar-tray",
        "state_dir":"/s/bar","claude_dir":"/h/.claude","follows_pointer":false,
        "env":{"BAR_STATE_DIR":"/s/bar","XDG_STATE_HOME":null,"PATH":"/usr/bin",
               "CLAUDE_CONFIG_DIR":"","EVIL":"x"}}"#;

    #[test]
    fn parses_paths_and_handler_variables_only() {
        let r = parse(&json(REC)).unwrap();
        assert_eq!(r.bar, PathBuf::from(abs("/opt/bar/bar")));
        assert_eq!(r.state_dir, PathBuf::from(abs("/s/bar")));
        assert_eq!(r.lock(), PathBuf::from(abs("/s/bar/tray.lock")));
        assert_eq!(
            r.env,
            [
                ("BAR_STATE_DIR".to_string(), Some(abs("/s/bar"))),
                ("CLAUDE_CONFIG_DIR".to_string(), None),
                ("PATH".to_string(), Some(abs("/usr/bin"))),
                ("XDG_STATE_HOME".to_string(), None),
            ]
        );
    }

    #[test]
    fn refuses_relative_or_missing_paths() {
        let p = |text: &str| parse(&json(text));
        assert!(p(r#"{"bar":"bar","state_dir":"/s"}"#)
            .unwrap_err()
            .contains("bar is not absolute"));
        assert!(p(r#"{"bar":"/b","state_dir":"s"}"#)
            .unwrap_err()
            .contains("state_dir is not absolute"));
        assert!(p(r#"{"bar":"/b"}"#).is_err());
        assert!(p("not json").is_err());
        assert_eq!(p(r#"{"bar":"/b","state_dir":"/s"}"#).unwrap().env, []);
    }

    /// A root without a drive is not absolute on Windows.
    #[cfg(windows)]
    #[test]
    fn a_path_without_a_drive_is_refused() {
        assert!(parse(r#"{"bar":"/b","state_dir":"C:/s"}"#)
            .unwrap_err()
            .contains("bar is not absolute"));
    }

    #[test]
    fn apply_env_sets_and_removes() {
        // names only this test uses would be dropped by the filter, so
        // build the record by hand with a test-only variable
        let k = "BAR_TRAY_RECORD_TEST";
        std::env::set_var(k, "old");
        let r = Record {
            bar: "/b".into(),
            state_dir: "/s".into(),
            env: vec![(k.into(), None)],
        };
        r.apply_env();
        assert!(std::env::var_os(k).is_none());
        Record {
            env: vec![(k.into(), Some("new".into()))],
            ..r
        }
        .apply_env();
        assert_eq!(std::env::var(k).unwrap(), "new");
        std::env::remove_var(k);
    }
}
