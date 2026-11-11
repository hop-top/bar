//! `bar-tray`: a desktop menu bar / tray companion for bar. It holds no
//! logic of its own: the menu comes from a snapshot `bar tray state --json`
//! prints, every click becomes a `bar tray run <id>` spawn.
//!
//! [`model`] is the platform-neutral part: the snapshot contract, the menu
//! it builds, the diff a backend applies between two snapshots and the
//! click state machine (two-step confirmation, one action in flight).
//! [`backend`] holds what a desktop backend needs and what is testable
//! anywhere (the event loop's decisions, rows, icons, child processes),
//! plus the per-OS backends.

pub mod backend;
pub mod model;
