#![allow(dead_code)]
//! Shared setup: an instance wired to a fake proxy and a fake RouteLLM.

use crate::fake::{cpa, entry, routellm, Cpa, Server};
use bar_plugin::keyguard::Guard;
use bar_routellm::config::{self, Instance};
use serde_json::json;
use std::path::PathBuf;

pub const T0: i64 = 1_800_000_000;
pub const KEY: &str = "mgmt-key-AAAA";

pub struct Env {
    pub dir: PathBuf,
    pub inst: Instance,
    pub proxy: Cpa,
    pub rl: Server,
}

impl Env {
    pub fn new(name: &str, disabled: bool, rl: Server, priority: i64) -> Env {
        let proxy = cpa(json!([entry(rl.port, disabled, priority)]), KEY);
        Env::with(name, proxy, rl)
    }

    pub fn with(name: &str, proxy: Cpa, rl: Server) -> Env {
        let dir = std::env::temp_dir().join(format!("rl-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let text = format!(
            "instance {}\n routellm {}\n key-file /k\n",
            proxy.server.origin(),
            rl.origin()
        );
        let mut cfg = config::parse(&text, None);
        assert!(cfg.errors.is_empty(), "{:?}", cfg.errors);
        Env {
            dir,
            inst: cfg.instances.remove(0),
            proxy,
            rl,
        }
    }

    pub fn cache_path(&self) -> PathBuf {
        bar_routellm::cache::file(&self.dir, &self.inst.origin.to_string())
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn healthy() -> Server {
    routellm(true, &["auto", "private", "vision"])
}

/// Records, at every `/health`, whether any management lock is held.
pub fn rl_watching_locks(
    cell: std::sync::Arc<std::sync::Mutex<Option<Vec<std::path::PathBuf>>>>,
) -> (Server, std::sync::Arc<std::sync::Mutex<Vec<bool>>>) {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let s2 = seen.clone();
    let srv = Server::start(move |r| {
        if r.path == "/health" {
            let held = cell.lock().unwrap().iter().flatten().any(|p| {
                bar_plugin::fsio::lock(p, std::time::Duration::ZERO)
                    .map(|l| l.is_none())
                    .unwrap_or(false)
            });
            s2.lock().unwrap().push(held);
            return crate::fake::ok(r#"{"status":"online"}"#);
        }
        crate::fake::ok(r#"{"data":[{"id":"auto"},{"id":"private"}]}"#)
    });
    (srv, seen)
}

pub fn lock_paths(e: &Env) -> Vec<std::path::PathBuf> {
    vec![
        Guard {
            state_dir: &e.dir,
            mgmt: &e.inst.management,
        }
        .lock_file(),
        bar_routellm::cache::lock_file(&e.cache_path()),
    ]
}
