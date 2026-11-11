//! Linux setup for `bar tray install|uninstall`: the icon in the user's
//! hicolor theme, then autostart through an XDG autostart entry (any
//! session that runs them: GNOME, Plasma, XFCE, Cinnamon, LXQt) or, with
//! `--systemd`, a user unit bound to `graphical-session.target` (sessions
//! that run no autostart entries but start that target, such as
//! Hyprland under uwsm). One of the two at a time; the tray starts now.

use super::process::{self, Layout, Sys};
use super::xdg::{self, Dirs, UNIT};
use std::io::Write;
use std::path::Path;

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
    if s.dry_run {
        writeln!(out, "write {}", d.icon().display()).map_err(io)?;
    } else {
        crate::click::write_private(&d.icon(), xdg::ICON_SVG)
            .map_err(|e| format!("{}: {e}", d.icon().display()))?;
    }
    if s.autostart {
        if s.systemd {
            drop_entry(d, s.dry_run, out)?;
            write(&d.unit(), &xdg::unit(s.argv, s.env)?, s.dry_run, out)?;
            run(&xdg::systemctl(&["daemon-reload"]), s.dry_run, sys, out)?;
            run(&xdg::systemctl(&["enable", UNIT]), s.dry_run, sys, out)?;
        } else {
            drop_unit(d, s.dry_run, sys, out)?;
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

/// Removes the unit (disabled first), the autostart entry and the icon.
pub fn uninstall(d: &Dirs, sys: &mut dyn Sys, out: &mut dyn Write) -> Result<(), String> {
    drop_unit(d, false, sys, out)?;
    remove(&d.desktop_entry(), out)?;
    remove(&d.icon(), out)
}
