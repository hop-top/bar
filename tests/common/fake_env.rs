//! Fake `click::Env`: temp sessions/state dirs, a scripted process table and
//! tmux. Nothing here runs a real command.
#![allow(dead_code)]

use hop_bar::click::Env;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

pub const SID: &str = "3f2a9c1e-0b4d-4e6f-8a7b-1c2d3e4f5a6b";
pub const TTY: &str = "ttys004";
pub const VERSION: &str = "2.1.319";
/// Hand-made pane captures; real ones go in per-version dirs.
pub const FIXTURES: &str = "synthetic";
const PID: u32 = 4242;

static N: AtomicUsize = AtomicUsize::new(0);

pub fn url(cmd: &str) -> String {
    format!("bar://cmd/{cmd}?tty={TTY}&sid={SID}")
}

pub fn fixture(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sessions")
        .join(FIXTURES)
        .join(format!("{name}.ansi"));
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

type Hook = Box<dyn FnMut(&Fake)>;

pub struct Fake {
    pub root: PathBuf,
    pub alive: Cell<bool>,
    pub tty: RefCell<Option<String>>,
    /// (pane tty, pane id)
    pub panes: RefCell<Vec<(String, String)>>,
    pub screens: RefCell<HashMap<String, String>>,
    pub calls: RefCell<Vec<Vec<String>>>,
    pub sleeps: RefCell<Vec<Duration>>,
    pub on_sleep: RefCell<Option<Hook>>,
    pub notes: RefCell<Vec<(String, String)>>,
    pub logs: RefCell<Vec<String>>,
}

impl Fake {
    /// Idle live session on `TTY`, own pane `%1` with an empty box.
    pub fn new() -> Fake {
        let root = std::env::temp_dir().join(format!(
            "bar-click-env-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(root.join("sessions")).unwrap();
        std::fs::create_dir_all(root.join("state")).unwrap();
        let f = Fake {
            root,
            alive: Cell::new(true),
            tty: RefCell::new(Some(TTY.into())),
            panes: RefCell::new(vec![(format!("/dev/{TTY}"), "%1".into())]),
            screens: RefCell::new(HashMap::new()),
            calls: RefCell::new(Vec::new()),
            sleeps: RefCell::new(Vec::new()),
            on_sleep: RefCell::new(None),
            notes: RefCell::new(Vec::new()),
            logs: RefCell::new(Vec::new()),
        };
        f.set_status("idle");
        f.screen("%1", &fixture("empty"));
        f
    }

    pub fn state(&self) -> PathBuf {
        self.root.join("state")
    }

    pub fn session_file(&self) -> PathBuf {
        self.root.join("sessions").join(format!("{PID}.json"))
    }

    pub fn set_status(&self, status: &str) {
        let body = format!(
            r#"{{"pid":{PID},"sessionId":"{SID}","status":"{status}","version":"{VERSION}"}}"#
        );
        std::fs::write(self.session_file(), body).unwrap();
    }

    pub fn screen(&self, pane: &str, text: &str) {
        self.screens
            .borrow_mut()
            .insert(pane.to_string(), text.to_string());
    }

    pub fn sent(&self) -> Vec<Vec<String>> {
        self.calls
            .borrow()
            .iter()
            .filter(|c| c[0] == "send-keys")
            .cloned()
            .collect()
    }
}

impl Default for Fake {
    fn default() -> Fake {
        Fake::new()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Env for Fake {
    fn sessions_dir(&self) -> PathBuf {
        self.root.join("sessions")
    }
    fn state_dir(&self) -> PathBuf {
        self.state()
    }
    fn alive(&self, pid: u32) -> bool {
        pid == PID && self.alive.get()
    }
    fn tty_of(&self, pid: u32) -> Option<String> {
        (pid == PID).then(|| self.tty.borrow().clone())?
    }
    fn tmux(&self, args: &[&str]) -> Option<String> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        let target = || {
            let i = args.iter().position(|a| *a == "-t")?;
            args.get(i + 1).map(|s| s.to_string())
        };
        match *args.first()? {
            "list-panes" => Some(
                self.panes
                    .borrow()
                    .iter()
                    .map(|(t, id)| format!("{t} {id}\n"))
                    .collect(),
            ),
            "capture-pane" => self.screens.borrow().get(&target()?).cloned(),
            "send-keys" => Some(String::new()),
            _ => None,
        }
    }
    fn sleep(&self, d: Duration) {
        self.sleeps.borrow_mut().push(d);
        let hook = self.on_sleep.borrow_mut().take();
        if let Some(mut h) = hook {
            h(self);
        }
    }
    fn now(&self) -> String {
        "T0".into()
    }
    fn notify(&self, title: &str, message: &str) {
        self.notes
            .borrow_mut()
            .push((title.to_string(), message.to_string()));
    }
    fn log(&self, line: &str) {
        self.logs.borrow_mut().push(line.to_string());
    }
}
