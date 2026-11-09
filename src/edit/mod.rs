//! `bar edit`: full-screen editor for `bar.conf` with a live preview drawn
//! by the renderer itself. Behind the `edit` cargo feature; nothing on the
//! render path reaches this module.

mod ansi;
pub mod app;
pub mod doc;
pub mod fuzzy;
pub mod group;
pub mod model;
pub mod schema;
pub mod ui;

#[cfg(test)]
mod tests;

use crate::plugin;
use app::{App, Flow};
use crossterm::event::{self, Event, KeyEventKind};
use model::Editor;
use std::io::IsTerminal;
use std::path::PathBuf;

/// The config file to edit: the one in use, else where `bar init` would
/// write one.
pub fn config_path() -> Option<PathBuf> {
    let home = crate::platform::home().unwrap_or_default();
    let env = |k: &str| std::env::var(k).ok();
    let kit = crate::platform::dirs::config();
    crate::conf::find_path(&env, &home, &kit).or_else(|| {
        crate::conf::search_paths(&env, &home, &kit)
            .into_iter()
            .next()
            .filter(|p| p.is_absolute())
    })
}

/// Opens the editor on the terminal; returns when the user quits.
pub fn run() -> Result<(), String> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err("needs a terminal".into());
    }
    let path = config_path().ok_or("home dir is unset or relative; set it or XDG_CONFIG_HOME")?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let dirs = plugin::path_dirs(std::env::var_os("PATH").as_deref());
    let plugins = plugin::discover(&dirs)
        .into_iter()
        .map(|f| f.name)
        .collect();
    // as `bar web plugins`: the cached `--ext-info` when fresh, else run now
    let cache = crate::cache::Cache::new(&crate::click::state_dir());
    let load: app::InfoLoader = Box::new(move |name| {
        let f = plugin::find(&dirs, name)?;
        Some(plugin::info_now(&cache, &f))
    });
    let mut app = App::new(Editor::new(path, text), plugins, load);
    let mut term = ratatui::try_init().map_err(|e| e.to_string())?;
    let r = (|| -> std::io::Result<()> {
        loop {
            term.draw(|f| ui::draw(f, &mut app))?;
            if let Event::Key(k) = event::read()? {
                // Windows reports releases too
                if k.kind != KeyEventKind::Release && app.on_key(k) == Flow::Exit {
                    return Ok(());
                }
            }
        }
    })();
    ratatui::restore();
    r.map_err(|e| e.to_string())
}
