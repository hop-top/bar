//! Windows backend: a notification-area icon with a native popup menu
//! (`tray-icon` over `muda`), driven by a plain message pump.
//!
//! Threads: the main thread owns the icon, the menu and every handle and
//! runs the pump; workers (snapshot fetches, `bar` runs, the ticker and
//! the idle poll) only send an [`Event`] over a channel and wake the
//! pump. [`App`] decides, this module carries out. Explorer restarts are
//! handled by `tray-icon` itself, which re-adds the icon on
//! `TaskbarCreated`.
//!
//! The menu is refreshed on every open and every idle poll, but Windows
//! keeps the message loop of an open popup to itself, so a snapshot that
//! lands while the menu is open shows from the next open. The icon has
//! no `bad` variant here: a refusal shows in the tooltip.

#[path = "windows/win32.rs"]
mod win32;

use super::app::{App, Effect, Event, TOOLTIP};
use super::icon::{self, Variant};
use super::menu;
use super::source::Source;
use super::spawn;
use super::Options;
use muda::MenuEvent;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// Stable identity of the icon: Windows keeps the user's pin/overflow
/// choice per GUID.
const ICON_GUID: u128 = 0x6b1f_2c4e_8a3d_4d57_9c0e_5f7a_2b9d_1e43;
/// Per logon session; a second copy exits at once.
const MUTEX_NAME: &str = r"Local\bar-tray";

/// Runs the tray until `Quit`. Nothing can be printed (no console), so a
/// failure is only an exit code.
pub fn run(opts: Options) -> ExitCode {
    let Some(_instance) = win32::single_instance(MUTEX_NAME) else {
        return ExitCode::SUCCESS;
    };
    match Tray::start(opts) {
        Ok((mut tray, rx)) => {
            tray.handle(Event::Poll);
            win32::pump(|| tray.drain(&rx));
            ExitCode::SUCCESS
        }
        Err(_) => ExitCode::FAILURE,
    }
}

/// A worker's way back to the pump: the channel plus the thread to wake.
#[derive(Clone)]
struct Post {
    tx: Sender<Event>,
    tid: u32,
}

impl Post {
    fn send(&self, ev: Event) {
        let _ = self.tx.send(ev);
        win32::wake(self.tid);
    }

    /// Sends `ev` every `period` until the process ends.
    fn every(&self, period: Duration, ev: Event) {
        let post = self.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(period);
            post.send(ev.clone());
        });
    }
}

/// The tray on the main thread: the app's decisions, the native menu
/// and icon, where snapshots and runs come from.
struct Tray {
    app: App,
    icon: TrayIcon,
    menu: menu::Native,
    source: Source,
    bar: Option<PathBuf>,
    post: Post,
    variant: Variant,
}

impl Tray {
    fn start(opts: Options) -> Result<(Tray, Receiver<Event>), String> {
        let (tx, rx) = channel();
        let post = Post {
            tx,
            tid: win32::thread_id(),
        };
        forward_events(&post);
        let variant = Variant::for_taskbar(win32::taskbar_is_light().unwrap_or(false));
        let app = App::new();
        let native = menu::build(app.view()).map_err(|e| e.to_string())?;
        let icon = TrayIconBuilder::new()
            .with_guid(ICON_GUID)
            .with_icon(icon_of(variant)?)
            .with_menu(Box::new(native.menu.clone()))
            .with_tooltip(TOOLTIP)
            .with_menu_on_left_click(true)
            .build()
            .map_err(|e| e.to_string())?;
        post.every(Duration::from_secs(1), Event::Tick);
        post.every(opts.poll, Event::Poll);
        let tray = Tray {
            app,
            icon,
            menu: native,
            source: opts.source,
            bar: opts.bar,
            post,
            variant,
        };
        Ok((tray, rx))
    }

    /// Handles everything queued; `false` ends the pump.
    fn drain(&mut self, rx: &Receiver<Event>) -> bool {
        while let Ok(ev) = rx.try_recv() {
            if !self.handle(ev) {
                return false;
            }
        }
        true
    }

    /// Lets the app decide, then carries its effects out; `false` for
    /// `Quit`.
    fn handle(&mut self, ev: Event) -> bool {
        if ev == Event::Tick {
            self.follow_theme();
        }
        let clicked = match &ev {
            Event::Click(id) => Some(id.clone()),
            _ => None,
        };
        let mut quit = false;
        for e in self.app.handle(ev, Instant::now()) {
            quit |= self.apply(e);
        }
        if let Some(id) = clicked {
            self.menu.sync_checked(&id, self.app.view());
        }
        !quit
    }

    /// Swaps the glyph when the taskbar theme flips.
    fn follow_theme(&mut self) {
        let Some(light) = win32::taskbar_is_light() else {
            return;
        };
        let v = Variant::for_taskbar(light);
        if v != self.variant {
            self.variant = v;
            if let Ok(i) = icon_of(v) {
                let _ = self.icon.set_icon(Some(i));
            }
        }
    }

    /// Carries one effect out; true for `Quit`.
    fn apply(&mut self, e: Effect) -> bool {
        match e {
            Effect::Apply(d) => self.apply_diff(d),
            Effect::Fetch => {
                let (source, post) = (self.source.clone(), self.post.clone());
                std::thread::spawn(move || post.send(Event::Snapshot(source.fetch())));
            }
            Effect::Spawn(args, limit) => self.spawn(args, limit),
            // the tooltip carries the note; the glyph follows the theme only
            Effect::Tint(_) => {}
            Effect::Tooltip(s) => {
                let _ = self.icon.set_tooltip(Some(s));
            }
            Effect::Quit => return true,
        }
        false
    }

    fn apply_diff(&mut self, d: crate::model::Diff) {
        use crate::model::Diff;
        match d {
            Diff::Same => {}
            Diff::Update(changes) => {
                for c in &changes {
                    self.menu.update(c);
                }
            }
            Diff::Rebuild(m) => {
                if let Ok(native) = menu::build(&m) {
                    self.icon.set_menu(Some(Box::new(native.menu.clone())));
                    self.menu = native;
                }
            }
        }
    }

    fn spawn(&self, args: Vec<OsString>, limit: Duration) {
        let Some(bar) = self.bar.clone() else {
            self.post.send(Event::Finished(
                Err("no bar to run: --snapshot mode".into()),
            ));
            return;
        };
        let post = self.post.clone();
        std::thread::spawn(move || {
            let r = spawn::run(&bar, &args, false, limit).map(|_| ());
            post.send(Event::Finished(r));
        });
    }
}

fn icon_of(v: Variant) -> Result<Icon, String> {
    Icon::from_rgba(icon::rgba(v), icon::SIZE, icon::SIZE).map_err(|e| e.to_string())
}

/// Icon and menu events reach the pump as messages: the handlers run
/// inside the window procedure, so they only queue and wake.
fn forward_events(post: &Post) {
    let p = post.clone();
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left | MouseButton::Right,
            button_state: MouseButtonState::Down,
            ..
        } = e
        {
            p.send(Event::Opened);
        }
    }));
    let p = post.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| p.send(Event::Click(e.id.0))));
}
