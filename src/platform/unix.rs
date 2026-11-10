//! Unix: POSIX modes, inode identity, process groups, `kill`, `localtime_r`.

// Allowed: libc calls: geteuid, gethostname, process-group signals, localtime_r.
#![allow(unsafe_code)]

use std::fs::{self, Metadata, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// `HOME`, when set and non-empty.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// Creates `dir` (and parents) and makes it owner-only (0700).
pub fn private_dir(dir: &Path) -> std::io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

/// Files this opens are created owner-only (0600).
pub fn private_file(o: &mut OpenOptions) -> &mut OpenOptions {
    o.mode(0o600)
}

/// The effective user id.
pub fn current_uid() -> Option<u32> {
    // SAFETY: geteuid has no preconditions and cannot fail
    Some(unsafe { libc::geteuid() })
}

/// The user owning a file.
pub fn owner(m: &Metadata) -> Option<u32> {
    Some(m.uid())
}

/// Identity that changes when the file is replaced: the inode.
pub fn file_id(m: &Metadata) -> u64 {
    m.ino()
}

/// Modification time, nanoseconds since the epoch.
pub fn mtime_ns(m: &Metadata) -> i64 {
    m.mtime()
        .saturating_mul(1_000_000_000)
        .saturating_add(m.mtime_nsec())
}

/// Modification time, seconds since the epoch.
pub fn mtime(m: &Metadata) -> i64 {
    m.mtime()
}

/// A regular file with any execute bit set.
pub fn is_executable(p: &Path) -> bool {
    fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// `dir/name` when it is executable.
pub fn executable_in(dir: &Path, name: &str) -> Option<PathBuf> {
    Some(dir.join(name)).filter(|p| is_executable(p))
}

/// The first absolute `PATH` dir holding an executable `name`. Relative
/// and empty entries (which would resolve against the working directory,
/// often a cloned repository) are skipped. Not found: `name` when `PATH`
/// has no such entry, else a path that cannot run, so `execvp` never
/// falls back to the working directory.
pub fn program(name: &str) -> PathBuf {
    program_in(name, std::env::var_os("PATH").as_deref())
}

fn program_in(name: &str, path: Option<&std::ffi::OsStr>) -> PathBuf {
    if name.contains('/') {
        return PathBuf::from(name);
    }
    let path = path.unwrap_or_else(|| std::ffi::OsStr::new("/usr/bin:/bin"));
    let mut relative = false;
    for d in std::env::split_paths(path) {
        if !d.is_absolute() {
            relative = true;
            continue;
        }
        if let Some(p) = executable_in(&d, name) {
            return p;
        }
    }
    if relative {
        Path::new("/nonexistent").join(name)
    } else {
        PathBuf::from(name)
    }
}

/// The plugin name part of a file name in a `PATH` dir, with the rank of
/// its form (always 0: the file name is the command name).
pub fn command_stem(file: &str) -> Option<(&str, usize)> {
    Some((file, 0))
}

/// `sh -c cmd`.
pub fn shell(cmd: &str) -> Command {
    let mut c = Command::new("sh");
    c.args(["-c", cmd]);
    c
}

/// A child that may be killed with everything it started: its own
/// process group.
pub fn own_group(c: &mut Command) -> &mut Command {
    c.process_group(0)
}

/// No console windows on Unix; nothing to hide.
pub fn hide_window(c: &mut Command) -> &mut Command {
    c
}

/// Kills the child's whole group, so grandchildren holding stdout die too.
pub fn kill_tree(child: &mut Child) {
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Starts `c` detached: its own process group, not waited on. Stdio is
/// the caller's to set.
pub fn spawn_detached(mut c: Command) -> std::io::Result<()> {
    c.process_group(0).spawn().map(drop)
}

/// `kill(pid, 0)`: no signal sent, no process spawned. Permission denied
/// still means the process exists.
pub fn pid_alive(pid: u64) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 only probes for existence.
    unsafe {
        libc::kill(pid, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

/// The parent process id.
pub fn parent_pid() -> Option<u32> {
    Some(std::os::unix::process::parent_id())
}

/// The process's zone (`TZ`, else the system's) at `ts`, in seconds east
/// of UTC; 0 when the C library cannot convert it.
pub fn local_offset(ts: i64) -> i64 {
    let t: libc::time_t = ts;
    // SAFETY: `tm` is plain data for which all-zero is valid, and
    // `localtime_r` writes only into the buffer it is given.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            0
        } else {
            tm.tm_gmtoff
        }
    }
}

/// This machine's host name (`gethostname`), as `hostname` prints it.
pub fn hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer and its length are passed together; the result is
    // read up to the first NUL inside the buffer only
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0)?;
    String::from_utf8(buf[..end].to_vec()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn tool(dir: &Path, name: &str) {
        fs::create_dir_all(dir).unwrap();
        let p = dir.join(name);
        fs::write(&p, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn program_skips_relative_and_empty_path_entries() {
        let root = std::env::temp_dir().join(format!("bar-prog-{}", std::process::id()));
        let abs = root.join("abs");
        tool(&abs, "bar-test-tool");
        let p = |path: &str| program_in("bar-test-tool", Some(OsStr::new(path)));
        let joined = format!(":.:rel:{}", abs.display());
        assert_eq!(p(&joined), abs.join("bar-test-tool"));
        // only relative entries: never the bare name execvp would look up
        for path in ["", ":", ".", "rel:", ":rel"] {
            let got = p(path);
            assert!(got.is_absolute(), "{path:?} -> {got:?}");
            assert!(!got.exists());
        }
        assert_eq!(p("/nonexistent-dir"), PathBuf::from("bar-test-tool"));
        assert_eq!(
            program_in("./x", Some(OsStr::new(""))),
            PathBuf::from("./x")
        );
        let _ = fs::remove_dir_all(&root);
    }
}
