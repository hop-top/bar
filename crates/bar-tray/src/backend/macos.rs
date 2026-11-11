//! macOS: a status item with a native menu, through `tray-icon` and
//! `muda`, inside AppKit's own run loop (`NSApplication::run`) with the
//! Accessory activation policy, so there is no Dock icon and no app
//! menu. The icon is a template image: the menu bar tints it.
//!
//! Everything that touches the menu runs on the main thread. Clicks
//! arrive there from AppKit; fetches and `bar` runs happen on their own
//! threads and come back through the main dispatch queue, as do the
//! ticks and polls. [`App`] decides, this module carries out.

use super::app::{App, Effect, Event, Tint, TOOLTIP};
use super::icon::{self, Variant};
use super::menu;
use super::source::Source;
use super::spawn;
use super::Options;
use dispatch2::DispatchQueue;
use muda::MenuEvent;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use std::cell::RefCell;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};
use tray_icon::{Icon, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// The icon is drawn at 2x and shown at [`icon::SIZE`] points.
const SCALE: u32 = 2;

/// The tray on the main thread: the app's decisions, the native menu
/// and icon, where snapshots and runs come from.
struct Tray {
    app: App,
    icon: TrayIcon,
    menu: menu::Native,
    source: Source,
    bar: Option<PathBuf>,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

/// Shows the status item and runs until `Quit`.
pub fn run(opts: Options) -> ExitCode {
    match start(opts) {
        Ok(ns) => {
            ns.run();
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("bar-tray: {e}");
            ExitCode::FAILURE
        }
    }
}

fn start(opts: Options) -> Result<objc2::rc::Retained<NSApplication>, String> {
    let mtm = MainThreadMarker::new().ok_or("the tray must start on the main thread")?;
    let ns = NSApplication::sharedApplication(mtm);
    ns.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let app = App::new();
    let native = menu::build(app.view()).map_err(|e| e.to_string())?;
    let icon = TrayIconBuilder::new()
        .with_icon_templated(icon_of(Variant::Template)?)
        .with_menu(Box::new(native.menu.clone()))
        .with_tooltip(TOOLTIP)
        .with_menu_on_left_click(true)
        .build()
        .map_err(|e| e.to_string())?;
    TRAY.with(|t| {
        *t.borrow_mut() = Some(Tray {
            app,
            icon,
            menu: native,
            source: opts.source,
            bar: opts.bar,
        });
    });
    MenuEvent::set_event_handler(Some(|e: MenuEvent| post(Event::Click(e.id.0))));
    TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button_state: MouseButtonState::Down,
            ..
        } = e
        {
            post(Event::Opened);
        }
    }));
    every(Duration::from_secs(1), Event::Tick);
    every(opts.poll, Event::Poll);
    post(Event::Poll);
    Ok(ns)
}

fn icon_of(v: Variant) -> Result<Icon, String> {
    let side = icon::SIZE * SCALE;
    Icon::from_rgba(icon::rgba_scaled(v, SCALE), side, side).map_err(|e| e.to_string())
}

/// Reports `ev` on the main thread, from any thread.
fn post(ev: Event) {
    DispatchQueue::main().exec_async(move || dispatch(ev));
}

/// Posts `ev` every `period` until the process ends.
fn every(period: Duration, ev: Event) {
    std::thread::spawn(move || loop {
        std::thread::sleep(period);
        post(ev.clone());
    });
}

/// Main thread: lets the app decide, then carries its effects out.
fn dispatch(ev: Event) {
    let quit = TRAY.with(|t| {
        let mut slot = t.borrow_mut();
        let Some(tray) = slot.as_mut() else {
            return false;
        };
        let clicked = match &ev {
            Event::Click(id) => Some(id.clone()),
            _ => None,
        };
        let effects = tray.app.handle(ev, Instant::now());
        let mut quit = false;
        for e in effects {
            quit |= tray.apply(e);
        }
        if let Some(id) = clicked {
            tray.menu.sync_checked(&id, tray.app.view());
        }
        quit
    });
    if quit {
        // dropping the tray removes the status item
        drop(TRAY.with(|t| t.borrow_mut().take()));
        std::process::exit(0);
    }
}

impl Tray {
    /// Carries one effect out; true for `Quit`.
    fn apply(&mut self, e: Effect) -> bool {
        match e {
            Effect::Apply(d) => self.apply_diff(d),
            Effect::Fetch => {
                let source = self.source.clone();
                std::thread::spawn(move || post(Event::Snapshot(source.fetch())));
            }
            Effect::Spawn(args, limit) => self.spawn(args, limit),
            Effect::Tint(t) => {
                let v = match t {
                    Tint::Normal => Variant::Template,
                    Tint::Bad => Variant::TemplateBad,
                };
                if let Ok(i) = icon_of(v) {
                    let _ = self.icon.set_icon_templated(Some(i));
                }
            }
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
            Diff::Rebuild(m) => match menu::build(&m) {
                Ok(native) => {
                    self.icon.set_menu(Some(Box::new(native.menu.clone())));
                    self.menu = native;
                }
                Err(e) => eprintln!("bar-tray: menu: {e}"),
            },
        }
    }

    fn spawn(&self, args: Vec<OsString>, limit: Duration) {
        let Some(bar) = self.bar.clone() else {
            post(Event::Finished(
                Err("no bar to run: --snapshot mode".into()),
            ));
            return;
        };
        std::thread::spawn(move || {
            let r = spawn::run(&bar, &args, false, limit).map(|_| ());
            post(Event::Finished(r));
        });
    }
}
