//! Fake executables that a test can run as soon as they are written.
//!
//! A file this process writes is open for writing for a moment; a child
//! another test thread starts in that moment holds a copy of the handle
//! until it execs. Linux refuses to exec a file open for writing ("text
//! file busy", ETXTBSY), so a test that runs a fake it just wrote fails
//! at random under parallel load. Here the file is written by a child
//! process (`cat`, `cp`) instead: this process never holds it open, so
//! no sibling's child can either. Renaming a file written in-process
//! into place would not help: the handle is to the file, whatever its
//! name.
#![cfg(unix)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// `body` at `path`, mode 0755, replacing what is there.
pub fn write(path: &Path, body: impl AsRef<[u8]>) {
    let mut c = Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\"", "sh"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    // the pipe closes when the handle drops, ending `cat`
    c.stdin.take().unwrap().write_all(body.as_ref()).unwrap();
    assert!(c.wait().unwrap().success(), "writing {}", path.display());
    chmod(path);
}

/// A copy of `from` at `to`, mode 0755.
pub fn copy(from: &Path, to: &Path) {
    cp(&[from], to);
    chmod(to);
}

/// Copies of `files` in `dir`, under their own names, mode 0755; one
/// `cp` for all.
pub fn copy_into(files: &[&Path], dir: &Path) {
    cp(files, dir);
    for f in files {
        chmod(&dir.join(f.file_name().unwrap()));
    }
}

fn cp(from: &[&Path], to: &Path) {
    let ok = Command::new("/bin/cp")
        .args(from)
        .arg(to)
        .status()
        .unwrap()
        .success();
    assert!(ok, "copying {from:?} to {}", to.display());
}

fn chmod(path: &Path) {
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}
