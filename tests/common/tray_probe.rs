//! Where the state budget goes: every sample of each measure (not only
//! the best), and the parts a `bar tray state` run is made of, timed
//! apart: bar's start, bar's runner against a plain `output()`, a
//! process group, a 10 ms `try_wait` poll, the host's sleep accuracy and
//! a freshly copied plugin's first exec.
#![allow(dead_code)]

use super::plugin_fx::{busy_retry, install};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Named rows of samples, in the order first seen.
#[derive(Default)]
pub struct Probe {
    rows: Vec<(String, Vec<Duration>)>,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

impl Probe {
    pub fn add(&mut self, name: &str, d: Duration) {
        match self.rows.iter_mut().find(|(n, _)| n == name) {
            Some((_, v)) => v.push(d),
            None => self.rows.push((name.to_string(), vec![d])),
        }
    }

    /// Times `f` into row `name`.
    pub fn time<R>(&mut self, name: &str, f: impl FnOnce() -> R) -> R {
        let start = Instant::now();
        let r = f();
        self.add(name, start.elapsed());
        r
    }

    /// One line per row: min, median, max, then every sample in order.
    pub fn report(&self) -> String {
        let mut out = String::from("budget probe (ms):\n");
        for (name, v) in &self.rows {
            let mut s = v.clone();
            s.sort();
            let all: Vec<String> = v.iter().map(|d| format!("{:.1}", ms(*d))).collect();
            out.push_str(&format!(
                "  {name:<28} min {:>6.1} med {:>6.1} max {:>6.1} | {}\n",
                ms(s[0]),
                ms(s[s.len() / 2]),
                ms(s[s.len() - 1]),
                all.join(" ")
            ));
        }
        out
    }
}

/// `--ext-info` then `--menu`, each by `run`.
pub fn chain(plugin: &Path, run: impl Fn(Command)) {
    for arg in ["--ext-info", "--menu"] {
        let mut c = Command::new(plugin);
        c.arg(arg);
        run(c);
    }
}

/// `output()` in a process group of its own, as bar starts plugins.
pub fn grouped(mut c: Command) {
    c.process_group(0).stdin(Stdio::null());
    assert!(busy_retry(|| c.output()).status.success());
}

/// Bar's wait: a process group, stdout read on a thread, `try_wait`
/// every 10 ms.
pub fn polled(mut c: Command) {
    c.process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = busy_retry(|| c.spawn());
    let mut out = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out.read_to_end(&mut b);
    });
    while child.try_wait().unwrap().is_none() {
        std::thread::sleep(Duration::from_millis(10));
    }
    reader.join().unwrap();
}

/// How long a `sleep(d)` really takes.
pub fn slept(d: Duration) -> Duration {
    let start = Instant::now();
    std::thread::sleep(d);
    start.elapsed()
}

/// A fresh copy of `src` at `to`, then its first and second
/// `--ext-info`.
pub fn first_exec(p: &mut Probe, src: &Path, to: &Path) {
    install(src, to);
    for row in ["fresh plugin 1st exec", "fresh plugin 2nd exec"] {
        let mut c = Command::new(to);
        c.arg("--ext-info").stdin(Stdio::null());
        p.time(row, || assert!(busy_retry(|| c.output()).status.success()));
    }
}
