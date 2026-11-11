//! The Linux run loop: [`App`] decides, this carries it out over the
//! `ksni` item.
//!
//! Threads: the main thread owns the [`Tray`] and runs the loop; `ksni`
//! has its own thread for D-Bus; a forwarder turns host events into loop
//! messages; the ticker, snapshot fetches and `bar` runs
//! are short workers that only send a message. There is no async runtime
//! beyond the one `ksni`'s blocking API hides.
//!
//! One copy per user: an abstract Unix socket named after the user is
//! bound for the life of the process. The kernel drops it when the
//! process dies, so there is no stale lock to clean up, and nothing is
//! written to disk.

use super::{Event as Host, Tray};
use crate::backend::app::{App, Effect, Event, Tint, TOOLTIP};
use crate::backend::sni_loop::{instance_name, translate, Msg};
use crate::backend::source::Source;
use crate::backend::{spawn, Options};
use std::ffi::OsString;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

/// Runs the tray until `Quit`. A second copy exits at once, quietly.
pub fn run(opts: Options) -> ExitCode {
    let name = instance_name(std::env::var("USER").ok().as_deref());
    let Some(_instance) = single_instance(&name) else {
        return ExitCode::SUCCESS;
    };
    let (tx, rx) = channel();
    let mut app = App::new().poll(opts.poll);
    let (htx, hrx) = channel();
    let tray = match Tray::start(app.view(), htx) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("bar-tray: no session bus or tray service: {e}");
            return ExitCode::FAILURE;
        }
    };
    forward(hrx, tx.clone());
    every(&tx, Duration::from_secs(1), Event::Tick);
    let _ = tx.send(Msg::App(Event::Poll));
    let lp = Loop {
        tray,
        source: opts.source,
        bar: opts.bar,
        tx,
    };
    lp.serve(&mut app, &rx)
}

/// Binds the instance socket; `None` when another copy holds it.
fn single_instance(name: &str) -> Option<UnixListener> {
    let addr = SocketAddr::from_abstract_name(name).ok()?;
    UnixListener::bind_addr(&addr).ok()
}

fn forward(rx: Receiver<Host>, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        while let Ok(h) = rx.recv() {
            if tx.send(translate(h)).is_err() {
                break;
            }
        }
    });
}

fn every(tx: &Sender<Msg>, period: Duration, ev: Event) {
    let tx = tx.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(period);
        if tx.send(Msg::App(ev.clone())).is_err() {
            break;
        }
    });
}

struct Loop {
    tray: Tray,
    source: Source,
    bar: Option<PathBuf>,
    tx: Sender<Msg>,
}

impl Loop {
    fn serve(self, app: &mut App, rx: &Receiver<Msg>) -> ExitCode {
        while let Ok(msg) = rx.recv() {
            match msg {
                Msg::App(ev) => {
                    let tick = ev == Event::Tick;
                    let mut quit = false;
                    for e in app.handle(ev, Instant::now()) {
                        quit |= self.apply(e, app);
                    }
                    if quit {
                        self.tray.shutdown();
                        return ExitCode::SUCCESS;
                    }
                    // the bus went away for good
                    if tick && self.tray.is_closed() {
                        eprintln!("bar-tray: tray service ended");
                        return ExitCode::FAILURE;
                    }
                }
                Msg::Online => {
                    self.tray.show(app.view());
                    let _ = self.tx.send(Msg::App(Event::Opened));
                }
                Msg::Offline => {}
            }
        }
        ExitCode::FAILURE
    }

    /// Carries one effect out; true for `Quit`.
    fn apply(&self, e: Effect, app: &App) -> bool {
        match e {
            // `ksni` diffs against what the host has
            Effect::Apply(_) => {
                self.tray.show(app.view());
            }
            Effect::Fetch => {
                let (source, tx) = (self.source.clone(), self.tx.clone());
                std::thread::spawn(move || {
                    let _ = tx.send(Msg::App(Event::Snapshot(source.fetch())));
                });
            }
            Effect::Spawn(args, limit) => self.spawn(args, limit),
            Effect::Tint(t) => {
                self.tray.set_bad(t == Tint::Bad);
            }
            Effect::Tooltip(s) => {
                // the tooltip is the note; "bar" alone when nothing is wrong
                let _ = self
                    .tray
                    .set_tooltip(if s == TOOLTIP { String::new() } else { s });
            }
            Effect::Quit => return true,
        }
        false
    }

    fn spawn(&self, args: Vec<OsString>, limit: Duration) {
        let tx = self.tx.clone();
        let Some(bar) = self.bar.clone() else {
            let _ = tx.send(Msg::App(Event::Finished(Err(
                "no bar to run: --snapshot mode".into(),
            ))));
            return;
        };
        std::thread::spawn(move || {
            let r = spawn::run(&bar, &args, false, limit).map(|_| ());
            let _ = tx.send(Msg::App(Event::Finished(r)));
        });
    }
}
