//! Windows backend: a notification-area icon with a native popup menu
//! (`tray-icon` over `muda`), driven by a plain message pump.
//!
//! Threads: the main thread owns the icon, the menu and every handle and
//! runs the pump; workers (state polls, action runs, a one-second ticker)
//! only send a [`Msg`] over a channel and wake the pump. Explorer
//! restarts are handled by `tray-icon` itself, which re-adds the icon on
//! `TaskbarCreated`.
//!
//! The menu is refreshed on every open and every 30 s, but Windows keeps
//! the message loop of an open popup to itself, so a snapshot that lands
//! while the menu is open shows from the next open.

#[path = "windows/win32.rs"]
mod win32;

use super::icon::{self, Variant};
use super::rows::{self, Row, RowKind};
use super::spawn;
use crate::model::{self, Click, Diff, Menu, Snapshot, State};
use muda::{CheckMenuItem, IsMenuItem, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// Stable identity of the icon: Windows keeps the user's pin/overflow
/// choice per GUID.
const ICON_GUID: u128 = 0x6b1f_2c4e_8a3d_4d57_9c0e_5f7a_2b9d_1e43;
/// Per logon session; a second copy exits at once.
const MUTEX_NAME: &str = r"Local\bar-tray";
const POLL_EVERY: Duration = Duration::from_secs(30);
const STATE_LIMIT: Duration = Duration::from_secs(15);
/// `bar` caps an action at 120 s itself; this only reaps a hung child.
const RUN_LIMIT: Duration = Duration::from_secs(150);
const TOOLTIP: &str = "bar";

enum Msg {
    /// The user opened the menu.
    Open,
    Clicked(String),
    Snapshot(Result<Snapshot, String>),
    Finished,
    Tick,
}

/// Runs the tray until `Quit`. Nothing can be printed (no console), so a
/// failure is only an exit code.
pub fn run() -> ExitCode {
    let Some(_instance) = win32::single_instance(MUTEX_NAME) else {
        return ExitCode::SUCCESS;
    };
    match App::start() {
        Ok((mut app, rx)) => {
            app.poll();
            win32::pump(|| app.drain(&rx));
            ExitCode::SUCCESS
        }
        Err(_) => ExitCode::FAILURE,
    }
}

enum Handle {
    Check(CheckMenuItem),
    Normal(MenuItem),
    Sub(Submenu),
}

impl Handle {
    fn set(&self, text: &str, checked: bool, enabled: bool) {
        match self {
            Handle::Check(i) => {
                i.set_text(text);
                i.set_checked(checked);
                i.set_enabled(enabled);
            }
            Handle::Normal(i) => {
                i.set_text(text);
                i.set_enabled(enabled);
            }
            Handle::Sub(i) => {
                i.set_text(text);
                i.set_enabled(enabled);
            }
        }
    }

    fn set_checked(&self, checked: bool) {
        if let Handle::Check(i) = self {
            i.set_checked(checked);
        }
    }
}

struct App {
    tray: TrayIcon,
    handles: HashMap<String, Handle>,
    state: State,
    generation: String,
    bar: std::path::PathBuf,
    tx: Sender<Msg>,
    tid: u32,
    variant: Variant,
    last_poll: Instant,
    polling: bool,
}

type Boxed = Box<dyn IsMenuItem>;

impl App {
    fn start() -> Result<(App, Receiver<Msg>), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let (tx, rx) = channel();
        let tid = win32::thread_id();
        forward_events(&tx, tid);
        let variant = Variant::for_taskbar(win32::taskbar_is_light().unwrap_or(false));
        let state = State::new(Menu::build(&Snapshot::default(), 0));
        let tray = TrayIconBuilder::new()
            .with_guid(ICON_GUID)
            .with_icon(icon_of(variant)?)
            .with_tooltip(TOOLTIP)
            .with_menu_on_left_click(true)
            .build()
            .map_err(|e| e.to_string())?;
        let mut app = App {
            tray,
            handles: HashMap::new(),
            state,
            generation: String::new(),
            bar: spawn::bar_path(&exe),
            tx: tx.clone(),
            tid,
            variant,
            last_poll: Instant::now(),
            polling: false,
        };
        let first = app.state.view().clone();
        app.rebuild(&first)?;
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            if tx.send(Msg::Tick).is_err() {
                break;
            }
            win32::wake(tid);
        });
        Ok((app, rx))
    }

    /// Handles everything queued; `false` ends the pump.
    fn drain(&mut self, rx: &Receiver<Msg>) -> bool {
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Open => self.poll(),
                Msg::Clicked(id) => {
                    if !self.click(&id) {
                        return false;
                    }
                }
                Msg::Snapshot(r) => self.snapshot(r),
                Msg::Finished => {
                    let d = self.state.finished(Instant::now());
                    self.apply(d);
                    self.poll();
                }
                Msg::Tick => self.tick(),
            }
        }
        true
    }

    fn tick(&mut self) {
        let d = self.state.tick(Instant::now());
        self.apply(d);
        if self.last_poll.elapsed() >= POLL_EVERY {
            self.poll();
        }
        if let Some(light) = win32::taskbar_is_light() {
            let v = Variant::for_taskbar(light);
            if v != self.variant {
                self.variant = v;
                if let Ok(i) = icon_of(v) {
                    let _ = self.tray.set_icon(Some(i));
                }
            }
        }
    }

    /// Starts a state poll unless one is running.
    fn poll(&mut self) {
        if self.polling {
            return;
        }
        self.polling = true;
        self.last_poll = Instant::now();
        let (bar, tx, tid) = (self.bar.clone(), self.tx.clone(), self.tid);
        std::thread::spawn(move || {
            let r = spawn::run(&bar, &spawn::state_args(), true, STATE_LIMIT)
                .and_then(|t| model::parse(&t).map_err(|e| e.to_string()));
            let _ = tx.send(Msg::Snapshot(r));
            win32::wake(tid);
        });
    }

    fn snapshot(&mut self, r: Result<Snapshot, String>) {
        self.polling = false;
        match r {
            Ok(s) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                let d = self.state.replace(Menu::build(&s, now), Instant::now());
                self.generation = s.generation;
                let _ = self.tray.set_tooltip(Some(TOOLTIP));
                self.apply(d);
            }
            Err(e) => {
                let _ = self.tray.set_tooltip(Some(format!("{TOOLTIP}: {e}")));
            }
        }
    }

    /// `false` when the click was `Quit`.
    fn click(&mut self, id: &str) -> bool {
        let (click, d) = self.state.click(id, Instant::now());
        // the native item flipped its own check mark: put the model's back
        if let (Some(h), Some(m)) = (self.handles.get(id), self.state.view().find(id)) {
            h.set_checked(m.checked);
        }
        self.apply(d);
        match click {
            Click::Quit => return false,
            Click::Run { id, confirmed } => {
                self.spawn(spawn::run_args(&id, confirmed, &self.generation));
            }
            Click::ViewToggle(key) => self.spawn(spawn::toggle_args(&key)),
            Click::Armed(_) | Click::Busy | Click::Ignored => {}
        }
        true
    }

    fn spawn(&self, args: Vec<std::ffi::OsString>) {
        let (bar, tx, tid) = (self.bar.clone(), self.tx.clone(), self.tid);
        std::thread::spawn(move || {
            let _ = spawn::run(&bar, &args, false, RUN_LIMIT);
            let _ = tx.send(Msg::Finished);
            win32::wake(tid);
        });
    }

    fn apply(&mut self, d: Diff) {
        match d {
            Diff::Same => {}
            Diff::Update(changes) => {
                for c in changes {
                    if let Some(h) = self.handles.get(&c.id) {
                        h.set(
                            &rows::text(&c.label, c.detail.as_deref()),
                            c.checked,
                            c.enabled,
                        );
                    }
                }
            }
            Diff::Rebuild(m) => {
                let _ = self.rebuild(&m);
            }
        }
    }

    fn rebuild(&mut self, m: &Menu) -> Result<(), String> {
        let menu = muda::Menu::new();
        let mut handles = HashMap::new();
        for r in rows::rows(m) {
            let item = build(&r, &mut handles).map_err(|e| e.to_string())?;
            menu.append(&*item).map_err(|e| e.to_string())?;
        }
        self.handles = handles;
        self.tray.set_menu(Some(Box::new(menu)));
        Ok(())
    }
}

fn build(r: &Row, handles: &mut HashMap<String, Handle>) -> muda::Result<Boxed> {
    let id = r.id.as_str();
    Ok(match &r.kind {
        RowKind::Separator => Box::new(PredefinedMenuItem::separator()),
        RowKind::Normal => {
            let i = MenuItem::with_id(id, &r.text, r.enabled, None);
            handles.insert(r.id.clone(), Handle::Normal(i.clone()));
            Box::new(i)
        }
        RowKind::Check => {
            let i = CheckMenuItem::with_id(id, &r.text, r.enabled, r.checked, None);
            handles.insert(r.id.clone(), Handle::Check(i.clone()));
            Box::new(i)
        }
        RowKind::Submenu(children) => {
            let s = Submenu::with_id(id, &r.text, r.enabled);
            for c in children {
                s.append(&*build(c, handles)?)?;
            }
            handles.insert(r.id.clone(), Handle::Sub(s.clone()));
            Box::new(s)
        }
    })
}

fn icon_of(v: Variant) -> Result<Icon, String> {
    Icon::from_rgba(icon::rgba(v), icon::SIZE, icon::SIZE).map_err(|e| e.to_string())
}

/// Icon and menu events reach the pump as messages: the handlers run
/// inside the window procedure, so they only queue and wake.
fn forward_events(tx: &Sender<Msg>, tid: u32) {
    let t = tx.clone();
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left | MouseButton::Right,
            button_state: MouseButtonState::Down,
            ..
        } = e
        {
            let _ = t.send(Msg::Open);
            win32::wake(tid);
        }
    }));
    let t = tx.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = t.send(Msg::Clicked(e.id.0));
        win32::wake(tid);
    }));
}
