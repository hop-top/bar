#![allow(dead_code)]
//! Shared setup: an instance wired to a fake proxy and a fake RouteLLM.

use crate::fake::{cpa, entry, routellm, Cpa, Server};
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
