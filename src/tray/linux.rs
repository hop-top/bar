//! Linux setup for `bar tray install|uninstall`: the icon in the user's
//! hicolor theme, then autostart through an XDG autostart entry (any
//! session that runs them: GNOME, Plasma, XFCE, Cinnamon, LXQt) or, with
//! `--systemd`, a user unit bound to `graphical-session.target` (sessions
//! that run no autostart entries but start that target, such as
//! Hyprland under uwsm). One of the two at a time; the tray starts now.
//! Both files are per user, not per state dir: install and uninstall
//! leave another install's alone (see `owner`).

use super::owner;
use super::process::{self, Layout, Sys};
use super::xdg::{self, Dirs, UNIT};
use crate::click::pointer::TRAY_MARKER;
use std::io::Write;
use std::path::{Path, PathBuf};

/// What the Linux part of install sets up.
pub struct Setup<'a> {
    pub dirs: &'a Dirs,
    /// The tray's command line.
    pub argv: &'a [String],
    /// The variables set at install, baked into the autostart file.
    pub env: &'a [(String, String)],
    pub autostart: bool,
    pub systemd: bool,
    pub dry_run: bool,
}

/// `bar-tray --backend` exits 0 only when the build shows a tray here:
/// on Linux one built with the `linux-sni` feature.
pub fn has_backend(tray: &Path, sys: &mut dyn Sys) -> bool {
    sys.exec(&[tray.display().to_string(), "--backend".into()])
        .is_ok()
}

pub fn no_backend(tray: &Path) -> String {
    format!(
        "{} has no Linux tray backend (build it with the cargo feature linux-sni); \
         `bar waybar` shows the menu in waybar without it",
        tray.display()
    )
}

fn io(e: std::io::Error) -> String {
    e.to_string()
}

fn write(path: &Path, body: &str, dry_run: bool, out: &mut dyn Write) -> Result<(), String> {
    if dry_run {
        return write!(out, "write {}:\n{body}", path.display()).map_err(io);
    }
    crate::click::write_private(path, body).map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(out, "installed {}", path.display()).map_err(io)
}

fn run(
    cmd: &[String],
    dry_run: bool,
    sys: &mut dyn Sys,
    out: &mut dyn Write,
) -> Result<(), String> {
    if dry_run {
        return writeln!(out, "{}", cmd.join(" ")).map_err(io);
    }
    sys.exec(cmd).map_err(|e| format!("{}: {e}", cmd.join(" ")))
}

fn remove(path: &Path, out: &mut dyn Write) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => writeln!(out, "removed {}", path.display()).map_err(io),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Disables and removes an installed unit; failures of `systemctl` (no
/// user manager reachable) do not stop the removal.
fn drop_unit(
    d: &Dirs,
    dry_run: bool,
    sys: &mut dyn Sys,
    out: &mut dyn Write,
) -> Result<(), String> {
    let unit = d.unit();
    if !unit.is_file() {
        return Ok(());
    }
    let disable = xdg::systemctl(&["disable", UNIT]);
    let reload = xdg::systemctl(&["daemon-reload"]);
    if dry_run {
        writeln!(out, "{}", disable.join(" ")).map_err(io)?;
        writeln!(out, "remove {}", unit.display()).map_err(io)?;
        return writeln!(out, "{}", reload.join(" ")).map_err(io);
    }
    let _ = sys.exec(&disable);
    remove(&unit, out)?;
    let _ = sys.exec(&reload);
    Ok(())
}

fn drop_entry(d: &Dirs, dry_run: bool, out: &mut dyn Write) -> Result<(), String> {
    let entry = d.desktop_entry();
    match (entry.is_file(), dry_run) {
        (false, _) => Ok(()),
        (true, true) => writeln!(out, "remove {}", entry.display()).map_err(io),
        (true, false) => remove(&entry, out),
    }
}

/// Writes the icon and the autostart file, then (re)starts the tray so
/// it runs with the command line just written. Run after the record and
/// the marker are in place.
pub fn install(
    s: &Setup,
    l: &Layout,
    sys: &mut dyn Sys,
    out: &mut dyn Write,
) -> Result<(), String> {
    let d = s.dirs;
    let mut users = icon_users(d);
    if !users.contains(&l.state_dir) {
        users.push(l.state_dir.clone());
    }
    if s.dry_run {
        writeln!(out, "write {}", d.icon().display()).map_err(io)?;
        writeln!(out, "write {}", d.icon_users().display()).map_err(io)?;
    } else {
        crate::click::write_private(&d.icon(), xdg::ICON_SVG)
            .map_err(|e| format!("{}: {e}", d.icon().display()))?;
        write_users(d, &users)?;
    }
    let state = &l.state_dir;
    if s.autostart {
        // the other kind stays when it is another install's
        if s.systemd {
            if owner::entry(d, state).ours() {
                drop_entry(d, s.dry_run, out)?;
            }
            write(&d.unit(), &xdg::unit(s.argv, s.env)?, s.dry_run, out)?;
            run(&xdg::systemctl(&["daemon-reload"]), s.dry_run, sys, out)?;
            run(&xdg::systemctl(&["enable", UNIT]), s.dry_run, sys, out)?;
        } else {
            if owner::unit(d, state).ours() {
                drop_unit(d, s.dry_run, sys, out)?;
            }
            write(
                &d.desktop_entry(),
                &xdg::desktop_entry(s.argv, s.env)?,
                s.dry_run,
                out,
            )?;
        }
        restart(s, l, sys, out)?;
    }
    if d.gnome() {
        writeln!(out, "{}", xdg::GNOME_HINT).map_err(io)?;
    }
    if s.autostart && s.systemd && d.cinnamon() {
        writeln!(out, "{}", xdg::CINNAMON_HINT).map_err(io)?;
    }
    Ok(())
}

/// Stops a tray still running an older command line, then starts the
/// new one: through the unit with `--systemd`, else in the background.
fn restart(s: &Setup, l: &Layout, sys: &mut dyn Sys, out: &mut dyn Write) -> Result<(), String> {
    if s.dry_run {
        let start = xdg::systemctl(&["start", UNIT]);
        let cmd = if s.systemd { &start[..] } else { s.argv };
        writeln!(out, "stop a running tray").map_err(io)?;
        return writeln!(out, "{}", cmd.join(" ")).map_err(io);
    }
    process::stop(l, sys, &mut std::io::sink())?;
    process::start(l, s.argv, sys, out)
}

/// The state dirs listed in [`Dirs::icon_users`].
fn icon_users(d: &Dirs) -> Vec<PathBuf> {
    std::fs::read_to_string(d.icon_users())
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn write_users(d: &Dirs, users: &[PathBuf]) -> Result<(), String> {
    let body: String = users.iter().map(|u| format!("{}\n", u.display())).collect();
    crate::click::write_private(&d.icon_users(), &body)
        .map_err(|e| format!("{}: {e}", d.icon_users().display()))
}

/// Removes the unit (disabled first) and the autostart entry unless
/// another install's (see `owner`), and the icon unless another install
/// still uses it: one listed with [`Dirs::icon_users`] whose state dir
/// still holds the tray marker (a state dir removed without an
/// uninstall no longer counts).
pub fn uninstall(
    d: &Dirs,
    state: &Path,
    sys: &mut dyn Sys,
    out: &mut dyn Write,
) -> Result<(), String> {
    match owner::unit(d, state).other() {
        Some(o) => writeln!(out, "{}", owner::kept(&d.unit(), o)).map_err(io)?,
        None => drop_unit(d, false, sys, out)?,
    }
    match owner::entry(d, state).other() {
        Some(o) => writeln!(out, "{}", owner::kept(&d.desktop_entry(), o)).map_err(io)?,
        None => remove(&d.desktop_entry(), out)?,
    }
    let others: Vec<PathBuf> = icon_users(d)
        .into_iter()
        .filter(|u| u != state && u.join(TRAY_MARKER).is_file())
        .collect();
    let Some(other) = others.first() else {
        remove(&d.icon_users(), out)?;
        return remove(&d.icon(), out);
    };
    write_users(d, &others)?;
    writeln!(
        out,
        "kept {}: used by the tray install in {}",
        d.icon().display(),
        other.display()
    )
    .map_err(io)
}
