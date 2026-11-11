//! Backends draw the menu the model describes. The modules here that
//! carry no OS calls beyond starting and killing processes (`app`,
//! `rows`, `icon`, `spawn`, `source`, `sni_map`) build and test anywhere; `menu` is the muda menu the
//! macOS and Windows backends share; each OS backend sits behind its
//! `cfg`.

pub mod app;
pub mod icon;
pub mod rows;
pub mod source;
pub mod spawn;

/// What a backend's `run` takes.
pub struct Options {
    pub source: source::Source,
    /// `bar` to run for clicks; `None` shows the menu only (`--snapshot`
    /// without `--bar`).
    pub bar: Option<std::path::PathBuf>,
    /// `--poll`: the idle poll, whatever the snapshot says.
    pub poll: Option<std::time::Duration>,
}

#[cfg(any(target_os = "macos", windows))]
pub mod menu;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(windows)]
pub mod windows;

#[cfg(any(all(target_os = "linux", feature = "linux-sni"), test))]
pub mod sni_map;

#[cfg(any(all(target_os = "linux", feature = "linux-sni"), test))]
pub mod sni_loop;

#[cfg(all(target_os = "linux", feature = "linux-sni"))]
pub mod linux;
