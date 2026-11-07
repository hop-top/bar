#[cfg(not(unix))]
compile_error!("bar supports Unix only (macOS, Linux); Windows is not supported");

pub mod cache;
pub mod check;
pub mod click;
pub mod conf;
pub mod expand;
pub mod expr;
pub mod format;
pub mod gitfs;
pub mod layout;
pub mod options;
pub mod pace;
pub mod placeholders;
pub mod plugin;
pub mod preview;
pub mod render;
pub mod session;
pub mod style;
