//! `bar waybar`: one line of waybar custom-module JSON built from the
//! tray snapshot (see `hop_bar::tray::waybar`).

use hop_bar::{click, conf, tray};
use std::io::Write;

pub fn run() {
    // user config only, like `bar tray state`; a bad config still yields a line
    let config = match conf::load(None) {
        conf::LoadResult::Ok(c) => c,
        conf::LoadResult::Missing => conf::Config::default(),
        conf::LoadResult::Invalid(_) => {
            let v = serde_json::json!({"text": "bar", "tooltip": "config error: run bar check",
                                       "class": "bad", "alt": "bad"});
            let _ = writeln!(std::io::stdout().lock(), "{v}");
            return;
        }
    };
    let dirs = tray::handler_plugin_dirs(std::env::var_os("PATH").as_deref());
    let state = click::handler_state_dir();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let snap = tray::state::run(&state, &config, &dirs)
        .unwrap_or_else(|_| tray::state::snapshot(&state, &config, &dirs, now));
    let _ = writeln!(std::io::stdout().lock(), "{}", tray::waybar::module(&snap));
}
