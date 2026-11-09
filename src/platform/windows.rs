//! Windows: `PATHEXT` executables, creation flags instead of process
//! groups, `taskkill` for process trees, Win32 zone conversion.

use std::fs::{self, Metadata, OpenOptions};
use std::os::windows::fs::MetadataExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::UNIX_EPOCH;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, SetHandleInformation, ERROR_ACCESS_DENIED, FILETIME,
    HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, STILL_ACTIVE, SYSTEMTIME,
};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP,
    CREATE_NO_WINDOW, DETACHED_PROCESS, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::System::Time::{
    FileTimeToSystemTime, GetDynamicTimeZoneInformation, SystemTimeToFileTime,
    SystemTimeToTzSpecificLocalTimeEx, DYNAMIC_TIME_ZONE_INFORMATION,
};

use super::{exec_exts, split_exec_name};

/// The Windows system directory (`C:\Windows\System32`), from the API
/// rather than `SystemRoot`, which the environment can change.
pub fn system_dir() -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    let mut buf = [0u16; 260];
    // SAFETY: the buffer and its length in u16 units are passed together
    let n = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
            buf.as_mut_ptr(),
            buf.len() as u32,
        )
    } as usize;
    if n == 0 || n >= buf.len() {
        return PathBuf::from(r"C:\Windows\System32");
    }
    PathBuf::from(std::ffi::OsString::from_wide(&buf[..n]))
}

/// `USERPROFILE` (where Claude Code keeps `.claude`), else `HOME`.
pub fn home() -> Option<PathBuf> {
    ["USERPROFILE", "HOME"]
        .iter()
        .find_map(|k| std::env::var_os(k).filter(|h| !h.is_empty()))
        .map(PathBuf::from)
}

/// Creates `dir` (and parents). Access follows the parent's ACL; the
/// per-user profile and `%LOCALAPPDATA%` are already private to the user.
pub fn private_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)
}

/// No numeric user ids: owners are not compared.
pub fn current_uid() -> Option<u32> {
    None
}

/// No numeric user ids.
pub fn owner(_m: &std::fs::Metadata) -> Option<u32> {
    None
}

/// Files inherit the directory's ACL; no mode to set.
pub fn private_file(o: &mut OpenOptions) -> &mut OpenOptions {
    o
}

/// Identity that changes when the file is replaced: its creation time.
pub fn file_id(m: &Metadata) -> u64 {
    m.creation_time()
}

/// Modification time, nanoseconds since the epoch.
pub fn mtime_ns(m: &Metadata) -> i64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
}

/// Modification time, seconds since the epoch.
pub fn mtime(m: &Metadata) -> i64 {
    mtime_ns(m) / 1_000_000_000
}

fn exts() -> &'static [String] {
    static EXTS: OnceLock<Vec<String>> = OnceLock::new();
    EXTS.get_or_init(|| exec_exts(std::env::var("PATHEXT").ok().as_deref()))
}

/// A regular file whose extension is a runnable `PATHEXT` entry.
pub fn is_executable(p: &Path) -> bool {
    p.file_name()
        .and_then(|f| f.to_str())
        .is_some_and(|f| split_exec_name(f, exts()).is_some())
        && fs::metadata(p).is_ok_and(|m| m.is_file())
}

/// `dir/name<ext>` for the first runnable `PATHEXT` extension that exists.
pub fn executable_in(dir: &Path, name: &str) -> Option<PathBuf> {
    exts()
        .iter()
        .map(|e| dir.join(format!("{name}{e}")))
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file()))
}

/// The first `PATH` match for `name` with a runnable `PATHEXT` extension
/// (`gh.exe`, `gh.cmd`), else `name`: `Command` alone tries `.exe` only.
pub fn program(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .filter(|d| d.is_absolute())
        .find_map(|d| executable_in(&d, name))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// The plugin name part of a file name in a `PATH` dir (extension
/// removed), with the extension's `PATHEXT` rank (lower wins).
pub fn command_stem(file: &str) -> Option<(&str, usize)> {
    split_exec_name(file, exts())
}

/// `cmd.exe /D /S /C "cmd"`: the command line goes in verbatim, as typed
/// at a `cmd` prompt (`/S` strips only the outer quotes).
pub fn shell(cmd: &str) -> Command {
    let mut c = Command::new("cmd.exe");
    c.args(["/D", "/S", "/C"]).raw_arg(format!("\"{cmd}\""));
    c
}

/// A child that may be killed with everything it started: its own
/// process group, and no console window of its own.
pub fn own_group(c: &mut Command) -> &mut Command {
    c.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
}

/// No console window for a console child of a windowless process.
pub fn hide_window(c: &mut Command) -> &mut Command {
    c.creation_flags(CREATE_NO_WINDOW)
}

/// `taskkill /T /F` ends the child and its descendants; the child itself
/// is killed directly too, in case `taskkill` is unavailable.
pub fn kill_tree(child: &mut Child) {
    let _ = Command::new("taskkill.exe")
        .args(["/T", "/F", "/PID", &child.id().to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    let _ = child.kill();
}

/// This process's standard handles stay out of children: a detached
/// child holding a copy of the pipe Claude Code reads would delay its
/// end-of-file until the child exits. Child stdio is set explicitly, so
/// nothing else relies on inheriting them.
fn unshare_std_handles() {
    for id in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: plain Win32 calls on this process's own handles; a
        // null or invalid handle is skipped.
        unsafe {
            let h = GetStdHandle(id);
            if !h.is_null() && h != INVALID_HANDLE_VALUE {
                SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

/// Starts `c` detached: no console, its own process group, out of the
/// caller's job object when the job allows it, not waited on. Stdio is
/// the caller's to set.
pub fn spawn_detached(mut c: Command) -> std::io::Result<()> {
    unshare_std_handles();
    let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    match c.creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB).spawn() {
        Ok(_) => Ok(()),
        // a job without breakaway rights refuses the flag
        Err(_) => c.creation_flags(flags).spawn().map(drop),
    }
}

/// The process exists: it opens and has not exited, or opening it is
/// denied (it exists but belongs to someone else).
pub fn pid_alive(pid: u64) -> bool {
    let Ok(pid) = u32::try_from(pid) else {
        return false;
    };
    if pid == 0 {
        return false;
    }
    // SAFETY: the handle is checked before use and closed after.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return GetLastError() == ERROR_ACCESS_DENIED;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0;
        CloseHandle(h);
        ok && code == STILL_ACTIVE as u32
    }
}

/// Not needed on Windows: only the macOS click handler asks.
pub fn parent_pid() -> Option<u32> {
    None
}

/// Seconds between 1601-01-01 (FILETIME epoch) and 1970-01-01.
const FILETIME_EPOCH: i64 = 11_644_473_600;
const TICKS: i64 = 10_000_000;

fn to_filetime(ticks: i64) -> FILETIME {
    FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    }
}

/// The system zone at `ts`, in seconds east of UTC, with the zone's
/// year-specific daylight rules; 0 when Windows cannot convert it.
/// `TZ` is not consulted.
pub fn local_offset(ts: i64) -> i64 {
    let Some(ticks) = ts
        .checked_add(FILETIME_EPOCH)
        .and_then(|s| s.checked_mul(TICKS))
        .filter(|&t| t >= 0)
    else {
        return 0;
    };
    // SAFETY: the structs are plain data for which all-zero is valid, and
    // each call writes only into the buffer it is given.
    unsafe {
        let mut utc: SYSTEMTIME = std::mem::zeroed();
        if FileTimeToSystemTime(&to_filetime(ticks), &mut utc) == 0 {
            return 0;
        }
        let mut zone: DYNAMIC_TIME_ZONE_INFORMATION = std::mem::zeroed();
        if GetDynamicTimeZoneInformation(&mut zone) == u32::MAX {
            return 0;
        }
        let mut local: SYSTEMTIME = std::mem::zeroed();
        if SystemTimeToTzSpecificLocalTimeEx(&zone, &utc, &mut local) == 0 {
            return 0;
        }
        let mut lft: FILETIME = std::mem::zeroed();
        if SystemTimeToFileTime(&local, &mut lft) == 0 {
            return 0;
        }
        let l = (i64::from(lft.dwHighDateTime) << 32) | i64::from(lft.dwLowDateTime);
        (l - ticks) / TICKS
    }
}

/// This machine's DNS host name (`GetComputerNameExW`).
pub fn hostname() -> Option<String> {
    use windows_sys::Win32::System::SystemInformation::{
        ComputerNameDnsHostname, GetComputerNameExW,
    };
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    // SAFETY: the buffer and its length in u16 units are passed together;
    // on success `len` holds the characters written, without the NUL
    let ok = unsafe { GetComputerNameExW(ComputerNameDnsHostname, buf.as_mut_ptr(), &mut len) };
    if ok == 0 || len as usize > buf.len() {
        return None;
    }
    String::from_utf16(&buf[..len as usize]).ok()
}
