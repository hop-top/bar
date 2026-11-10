#![allow(dead_code)]
//! A tiny in-process management server for tests: synthetic accounts
//! only, loopback only, every request recorded.

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
