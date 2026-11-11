//! Backends draw the menu the model describes. The modules here that
//! carry no OS calls (`rows`, `icon`, `spawn`, `sni_map`) build and test
//! anywhere; each OS backend sits behind its `cfg`.

pub mod icon;
pub mod rows;
pub mod spawn;

#[cfg(windows)]
pub mod windows;

#[cfg(any(all(target_os = "linux", feature = "linux-sni"), test))]
pub mod sni_map;

#[cfg(all(target_os = "linux", feature = "linux-sni"))]
pub mod linux;
