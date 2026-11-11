//! `bar-routellm`: state and toggle of a RouteLLM that sits behind a
//! CLIProxyAPI proxy, as a bar plugin. The binary is a thin shell over
//! these modules.

pub mod cache;
pub mod collect;
pub mod config;
pub mod menu;
pub mod mgmt;
pub mod render;
pub mod toggle;

/// The release version (`BAR_RELEASE_VERSION` at build time), else the
/// crate's.
pub const VERSION: &str = match option_env!("BAR_RELEASE_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};
