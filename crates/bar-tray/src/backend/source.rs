//! Where snapshots come from: `bar tray state --json`, or a file for
//! showing a menu without `bar` (`--snapshot`).

use super::spawn;
use crate::model::{self, Snapshot};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Re-read on every fetch, so editing the file changes the menu.
    File(PathBuf),
    /// `bar` at this path.
    Bar(PathBuf),
}

impl Source {
    /// The current snapshot, or why there is none.
    pub fn fetch(&self) -> Result<Snapshot, String> {
        let text = match self {
            Source::File(p) => {
                std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?
            }
            Source::Bar(bar) => spawn::run(bar, &spawn::state_args(), true, spawn::QUICK)
                .map_err(|e| format!("bar tray state: {e}"))?,
        };
        model::parse(&text).map_err(|e| format!("invalid snapshot: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_source() {
        let dir = std::env::temp_dir().join(format!("bar-tray-src-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("snap.json");
        std::fs::write(&p, include_str!("../../tests/fixtures/snapshot.json")).unwrap();
        let s = Source::File(p.clone()).fetch().unwrap();
        assert_eq!(s.generation, "7c1f0a9e3b2d4f60");
        std::fs::write(&p, "{\"version\": 2}").unwrap();
        let e = Source::File(p).fetch().unwrap_err();
        assert!(e.starts_with("invalid snapshot: version"), "{e}");
        let e = Source::File(dir.join("none")).fetch().unwrap_err();
        assert!(e.contains("none: "), "{e}");
    }

    #[test]
    fn bar_source_reports_failure() {
        let e = Source::Bar(PathBuf::from("/nonexistent/bar"))
            .fetch()
            .unwrap_err();
        assert!(e.starts_with("bar tray state: /nonexistent/bar: "), "{e}");
    }
}
