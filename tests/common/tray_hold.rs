//! A stand-in for a running tray: holds `tray.lock` as `bar-tray --lock`
//! does and, once signalled, lets go of it after a delay, as a tray
//! finishing its exit some time after SIGTERM or the close request.

use std::path::Path;
use std::sync::mpsc::{channel, Sender};
use std::time::Duration;

pub struct Tray {
    stop: Sender<()>,
}

impl Tray {
    /// Takes the lock at `lock` (it must be free); it is let go `delay`
    /// after [`Tray::signal`], or when the `Tray` is dropped.
    pub fn hold(lock: &Path, delay: Duration) -> Tray {
        let f = bar_tray::lock::acquire(lock)
            .unwrap()
            .expect("the lock is free");
        let (stop, rx) = channel();
        std::thread::spawn(move || {
            if rx.recv().is_ok() {
                std::thread::sleep(delay);
            }
            drop(f);
        });
        Tray { stop }
    }

    /// What a stop request does to a tray: it starts exiting.
    pub fn signal(&self) {
        let _ = self.stop.send(());
    }

    /// A handle a fake `kill` keeps to signal this tray.
    pub fn signaller(&self) -> Sender<()> {
        self.stop.clone()
    }
}
