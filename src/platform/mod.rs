//! Operating-system differences in one place: home directory, private
//! files, file identity, executables, child processes, process liveness
//! and the local zone offset. Unix and Windows provide the same functions.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

/// `PATHEXT` when it is unset or empty.
pub const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// Extensions `CreateProcess` can start without an interpreter named on
/// the command line (`.bat`/`.cmd` run through `cmd.exe`). Others listed
/// in `PATHEXT` (`.ps1`, `.vbs`, `.js`) need one and are skipped.
const RUNNABLE: [&str; 4] = [".com", ".exe", ".bat", ".cmd"];

/// The runnable entries of a `PATHEXT` value, lower-case, in its order.
pub fn exec_exts(pathext: Option<&str>) -> Vec<String> {
    let raw = pathext
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(DEFAULT_PATHEXT);
    let mut out: Vec<String> = Vec::new();
    for e in raw.split(';').map(|e| e.trim().to_ascii_lowercase()) {
        if RUNNABLE.contains(&e.as_str()) && !out.contains(&e) {
            out.push(e);
        }
    }
    out
}

/// `file` without its executable extension, and that extension's rank in
/// `exts` (lower wins); `None` when the extension is not in `exts`.
/// Matching ignores ASCII case, as Windows file names do.
pub fn split_exec_name<'a>(file: &'a str, exts: &[String]) -> Option<(&'a str, usize)> {
    exts.iter().enumerate().find_map(|(rank, ext)| {
        let cut = file.len().checked_sub(ext.len())?;
        let tail = file.get(cut..)?;
        (cut > 0 && tail.eq_ignore_ascii_case(ext)).then(|| (&file[..cut], rank))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_exts_keeps_runnable_entries_in_order() {
        assert_eq!(exec_exts(None), [".com", ".exe", ".bat", ".cmd"]);
        assert_eq!(exec_exts(Some("  ")), exec_exts(None));
        assert_eq!(
            exec_exts(Some(".CMD;.PS1;.Exe;.VBS;.cmd;")),
            [".cmd", ".exe"]
        );
    }

    #[test]
    fn split_exec_name_strips_a_known_extension() {
        let exts = exec_exts(None);
        assert_eq!(split_exec_name("bar-foo.exe", &exts), Some(("bar-foo", 1)));
        assert_eq!(split_exec_name("bar-foo.CMD", &exts), Some(("bar-foo", 3)));
        assert_eq!(split_exec_name("bar-foo.ps1", &exts), None);
        assert_eq!(split_exec_name("bar-foo", &exts), None);
        assert_eq!(split_exec_name(".exe", &exts), None);
    }
}
