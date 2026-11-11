//! The subcommands' handlers, one module per command family.

pub mod check;
pub mod click;
pub mod external;
pub mod init;
pub mod install;
pub mod plugin;
pub mod preview;
pub mod prices;
pub mod render;
pub mod toggle;
pub mod update;
pub mod web;

pub fn cwd() -> Option<std::path::PathBuf> {
    std::env::current_dir().ok()
}
