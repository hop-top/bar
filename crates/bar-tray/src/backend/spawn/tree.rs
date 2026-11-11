//! Ending a child together with everything it started, so no descendant
//! keeps running, or keeps the tray's pipes open, past the limit. Unix:
//! the child leads its own process group and the group is killed as
//! one. Windows: `taskkill /T /F` from the system dir ends the child's
//! process tree.
#![cfg_attr(unix, allow(unsafe_code))] // one kill(2) call: std cannot signal a process group

use std::process::{Child, Command};

/// `CREATE_NO_WINDOW`: a console of its own, without a window.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The Windows creation flags of every child of the tray, a GUI process
/// with no console: a hidden console of its own. Set in one call, since
/// `creation_flags` replaces what was set before.
#[cfg(windows)]
pub const CHILD_FLAGS: u32 = CREATE_NO_WINDOW;

/// Makes the child of `c` killable with its descendants and, on
/// Windows, gives it [`CHILD_FLAGS`]: nothing else sets creation flags.
pub fn own_group(c: &mut Command) -> &mut Command {
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(c, 0);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(c, CHILD_FLAGS);
    c
}

/// Kills `child` and every process in its group. Call before reaping
/// it: until then its pid, which is the group id, cannot be reused.
#[cfg(unix)]
pub fn kill_tree(child: &mut Child) {
    if let Ok(pid) = libc::pid_t::try_from(child.id()) {
        // SAFETY: sends a signal; no memory is involved. `-pid` names the
        // group the unreaped child leads (spawned through `own_group`).
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
}

/// Kills `child` and its descendants; the child itself directly too,
/// in case `taskkill` cannot run.
#[cfg(windows)]
pub fn kill_tree(child: &mut Child) {
    use std::process::Stdio;
    if let Some(root) = std::env::var_os("SYSTEMROOT") {
        let mut tk = Command::new(std::path::Path::new(&root).join(r"System32\taskkill.exe"));
        tk.args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let _ = own_group(&mut tk).status();
    }
    let _ = child.kill();
}

#[cfg(not(any(unix, windows)))]
pub fn kill_tree(child: &mut Child) {
    let _ = child.kill();
}
