//! Backends draw the menu the model describes. The modules here that
//! carry no OS calls (`rows`, `icon`, `spawn`) build and test anywhere;
//! each OS backend sits behind its `cfg`.

pub mod icon;
pub mod rows;
pub mod spawn;

#[cfg(windows)]
pub mod windows;
