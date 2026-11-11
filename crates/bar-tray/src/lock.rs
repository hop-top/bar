//! One tray per state dir: `--lock <file>` takes an exclusive lock on the
//! file for the life of the process and writes its pid there, so
//! `bar tray status` and `bar tray stop` can find it. The lock goes with
//! the process, however it ends, so a stale file never blocks a start.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::Path;

/// Locks `path` (created 0600 when missing) and writes this pid into it;
/// `Ok(None)` when another process holds the lock. Keep the file open:
/// dropping it releases the lock.
pub fn acquire(path: &Path) -> std::io::Result<Option<File>> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut o = OpenOptions::new();
    // never truncate before holding the lock: the file holds the owner's pid
    o.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let mut f = o.open(path)?;
    match f.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Error(e)) => return Err(e),
    }
    f.set_len(0)?;
    writeln!(f, "{}", std::process::id())?;
    f.flush()?;
    Ok(Some(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("bar-tray-lock-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("state/tray.lock")
    }

    #[test]
    fn second_holder_is_refused_until_the_first_lets_go() {
        let p = tmp("two");
        let first = acquire(&p).unwrap().expect("first takes the lock");
        assert_eq!(
            std::fs::read_to_string(&p).unwrap().trim(),
            std::process::id().to_string()
        );
        assert!(acquire(&p).unwrap().is_none(), "second is refused");
        // a refused start leaves the owner's pid alone
        assert_eq!(
            std::fs::read_to_string(&p).unwrap().trim(),
            std::process::id().to_string()
        );
        drop(first);
        assert!(acquire(&p).unwrap().is_some(), "free again once dropped");
        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_stale_pid_is_replaced() {
        let p = tmp("stale");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "999999999\nleftover\n").unwrap();
        let _held = acquire(&p).unwrap().unwrap();
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            format!("{}\n", std::process::id())
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // an existing file keeps its mode; a new one is 0600
            let q = tmp("mode");
            let _h = acquire(&q).unwrap().unwrap();
            let mode = std::fs::metadata(&q).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
            let _ = std::fs::remove_dir_all(q.parent().unwrap().parent().unwrap());
        }
        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
    }
}
