mod app;
mod doc;
mod group;
mod model;
mod ui;

use super::app::{App, InfoLoader};
use super::model::Editor;
use std::path::PathBuf;

/// A path that cannot be written: its parent is a regular file.
pub fn unwritable() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml/bar.conf")
}

/// An editor on `text` (`None`: no file yet) at a path that is never written.
pub fn editor(text: Option<&str>) -> Editor {
    Editor::new(unwritable(), text.map(str::to_string))
}

/// An app with no plugins.
pub fn app(text: &str) -> App {
    let load: InfoLoader = Box::new(|_| None);
    App::new(editor(Some(text)), Vec::new(), load)
}
