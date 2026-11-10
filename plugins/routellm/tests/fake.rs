#![allow(dead_code)]
//! A tiny in-process proxy and RouteLLM for tests: synthetic data only,
//! loopback only, every request recorded.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct Rec {
    pub method: String,
    pub path: String,
    pub auth: String,
    pub body: String,
}

pub struct Reply {
    pub status: u16,
    pub body: String,
}

pub fn ok(body: impl Into<String>) -> Reply {
    Reply {
        status: 200,
        body: body.into(),
    }
}

pub fn status(s: u16) -> Reply {
    Reply {
        status: s,
        body: "{}".into(),
    }
}

pub struct Server {
    pub port: u16,
    pub log: Arc<Mutex<Vec<Rec>>>,
    stop: Arc<AtomicBool>,
}

type Handler = dyn Fn(&Rec) -> Reply + Send + Sync;

impl Server {
    pub fn start(handler: impl Fn(&Rec) -> Reply + Send + Sync + 'static) -> Server {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let h: Arc<Handler> = Arc::new(handler);
        let (log2, stop2) = (log.clone(), stop.clone());
        std::thread::spawn(move || {
            for s in l.incoming() {
                if stop2.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(s) = s else { continue };
                let (h, log) = (h.clone(), log2.clone());
                std::thread::spawn(move || serve(s, &*h, &log));
            }
        });
        Server { port, log, stop }
    }

    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn count(&self) -> usize {
        self.log.lock().unwrap().len()
    }

    pub fn paths(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.path.clone())
            .collect()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn serve(mut s: TcpStream, h: &Handler, log: &Mutex<Vec<Rec>>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, path) = (
        first.next().unwrap_or("").to_string(),
        first.next().unwrap_or("").to_string(),
    );
    let mut auth = String::new();
    let mut len = 0usize;
    for l in lines {
        let l = l.to_ascii_lowercase();
        if let Some(v) = l.strip_prefix("authorization:") {
            auth = v.trim().to_string();
        }
        if let Some(v) = l.strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    while buf.len() < head_end + len {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let body = String::from_utf8_lossy(&buf[head_end..]).to_string();
    let rec = Rec {
        method,
        path,
        auth,
        body,
    };
    log.lock().unwrap().push(rec.clone());
    let r = h(&rec);
    let out = format!(
        "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        r.status,
        r.body.len()
    );
    let _ = s.write_all(out.as_bytes());
    let _ = s.write_all(r.body.as_bytes());
}

use serde_json::{json, Value};

pub const SECRET: &str = "SECRET-UPSTREAM-KEY-1";
pub const HEADER_SECRET: &str = "SECRET-HEADER-VALUE";

/// The proxy's provider entry for RouteLLM, with the secret fields the real
/// listing carries.
pub fn entry(rl_port: u16, disabled: bool, priority: i64) -> Value {
    json!({"name":"routellm","priority":priority,"disabled":disabled,
        "base-url":format!("http://127.0.0.1:{rl_port}/v1"),
        "api-key-entries":[{"api-key":SECRET,"auth-index":"ab12"}],
        "headers":{"X-Secret":HEADER_SECRET},
        "models":[{"name":"auto","alias":"route-auto"},{"name":"private","alias":"route-private"}]})
}

/// A proxy whose v0 management routes behave as the real ones: the list
/// carries secrets, `PATCH` merges the given fields of the entry named,
/// 404 for an unknown name, an empty `base-url` deletes the entry. Only
/// `key` is accepted. `apply` = false: the proxy answers ok and changes
/// nothing (a change that did not land).
pub struct Cpa {
    pub server: Server,
    pub entries: Arc<Mutex<Value>>,
    pub apply: Arc<AtomicBool>,
}

pub fn cpa(entries: Value, key: &str) -> Cpa {
    let state = Arc::new(Mutex::new(entries));
    let apply = Arc::new(AtomicBool::new(true));
    let (st, ap, want) = (
        state.clone(),
        apply.clone(),
        format!("bearer {}", key.to_ascii_lowercase()),
    );
    let server = Server::start(move |r| {
        if r.auth != want {
            return status(401);
        }
        let mut es = st.lock().unwrap();
        match (r.method.as_str(), r.path.as_str()) {
            ("GET", "/v0/management/openai-compatibility") => {
                ok(json!({"openai-compatibility": *es}).to_string())
            }
            ("GET", "/v0/management/oauth-model-alias") => ok(json!({"oauth-model-alias":{
                "claude":[{"name":"claude-sonnet-5-5","alias":"route-auto","fork":true}]}})
            .to_string()),
            ("PATCH", "/v0/management/openai-compatibility") => {
                let b: Value = serde_json::from_str(&r.body).unwrap_or_default();
                let arr = es.as_array_mut().unwrap();
                let Some(pos) = arr.iter().position(|e| e["name"] == b["name"]) else {
                    return Reply {
                        status: 404,
                        body: r#"{"error":"item not found"}"#.into(),
                    };
                };
                if b["value"]["base-url"] == "" {
                    arr.remove(pos);
                } else if ap.load(Ordering::SeqCst) {
                    for f in ["disabled", "priority"] {
                        if !b["value"][f].is_null() {
                            arr[pos][f] = b["value"][f].clone();
                        }
                    }
                }
                ok(r#"{"status":"ok"}"#)
            }
            _ => status(404),
        }
    });
    Cpa {
        server,
        entries: state,
        apply,
    }
}

/// RouteLLM: `/health` and `/v1/models` listing `tiers`.
pub fn routellm(online: bool, tiers: &[&str]) -> Server {
    let data: Vec<Value> = tiers
        .iter()
        .map(|t| json!({"id":t,"object":"model","owned_by":"routellm",
            "routellm":{"kind":"tier","capabilities":{"vision":true,"tools":true,"context":200000}}}))
        .collect();
    Server::start(move |r| match r.path.as_str() {
        "/health" if online => ok(r#"{"status":"online"}"#),
        "/v1/models" if online => ok(json!({"object":"list","data":data}).to_string()),
        _ => status(503),
    })
}

impl Server {
    pub fn patches(&self) -> Vec<Rec> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.method == "PATCH")
            .cloned()
            .collect()
    }
}
