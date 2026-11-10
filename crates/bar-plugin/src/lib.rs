//! Building blocks shared by bar and its plugins: the plugin protocol's
//! JSON types, secret bytes, `curl` requests whose credentials never reach
//! argv or the environment, capped reads, private files and key sources.
//! No proxy-specific code lives here.

pub mod dirs;
pub mod fsio;
pub mod http;
pub mod keysource;
pub mod proto;
pub mod secret;
pub mod sha256;
pub mod url;
