//! WebAssembly (the web configurator's renderer): no home, files,
//! processes or local zone. Every call answers "nothing there" so the
//! pure render path builds; nothing here is reached by it.

use std::fs::{Metadata, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

fn unsupported() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Unsupported, "not available in wasm")
}

/// No home directory.
pub fn home() -> Option<PathBuf> {
    None
}

/// No file system.
pub fn private_dir(_dir: &Path) -> std::io::Result<()> {
    Err(unsupported())
}

/// No numeric user ids: owners are not compared.
pub fn current_uid() -> Option<u32> {
    None
}

/// No numeric user ids.
pub fn owner(_m: &std::fs::Metadata) -> Option<u32> {
    None
}

/// Unchanged: there are no files to open.
pub fn private_file(o: &mut OpenOptions) -> &mut OpenOptions {
    o
}

/// No file identity.
pub fn file_id(_m: &Metadata) -> u64 {
    0
}

/// No modification times.
pub fn mtime_ns(_m: &Metadata) -> i64 {
    0
}

/// No modification times.
pub fn mtime(_m: &Metadata) -> i64 {
    0
}

/// Nothing is executable.
pub fn is_executable(_p: &Path) -> bool {
    false
}

/// Nothing is executable.
pub fn executable_in(_dir: &Path, _name: &str) -> Option<PathBuf> {
    None
}

/// `name` as given.
pub fn program(name: &str) -> PathBuf {
    PathBuf::from(name)
}

/// No plugin files.
pub fn command_stem(_file: &str) -> Option<(&str, usize)> {
    None
}

/// A command that cannot start.
pub fn shell(cmd: &str) -> Command {
    Command::new(cmd)
}

/// No processes.
pub fn claim_private_console() {}

/// Unchanged.
pub fn own_group(c: &mut Command) -> &mut Command {
    c
}

/// Unchanged.
pub fn hide_window(c: &mut Command) -> &mut Command {
    c
}

/// No processes to kill.
pub fn kill_tree(_child: &mut Child) {}

/// No processes.
pub fn spawn_detached(_c: Command) -> std::io::Result<()> {
    Err(unsupported())
}

/// No processes.
pub fn pid_alive(_pid: u64) -> bool {
    false
}

/// No parent process.
pub fn parent_pid() -> Option<u32> {
    None
}

/// UTC.
pub fn local_offset(_ts: i64) -> i64 {
    0
}

/// No host.
pub fn hostname() -> Option<String> {
    None
}
