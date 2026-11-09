//! Replacing a file the user owns (`settings.json`, `bar.conf`): written
//! through a symlink to its target, the old file backed up beside it,
//! the new one written atomically with the old file's permissions.

use std::path::{Path, PathBuf};

/// The file actually written for `path`: a symlink's target, never the
/// link itself; a link whose target is missing is refused.
pub fn target(path: &Path) -> Result<PathBuf, String> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path)
            .map_err(|e| format!("{}: symlink target unusable: {e}", path.display())),
        _ => Ok(path.to_path_buf()),
    }
}

/// Backups kept per file; a new backup removes the older ones.
pub const KEEP: usize = 5;

/// Where a file's backups go: beside it, or in a directory of their own
/// (for files whose directory may be committed or shared).
#[derive(Debug, Clone, Copy)]
pub enum Backups<'a> {
    Beside,
    In(&'a Path),
}

/// `<name>.bak-` beside `path`, or `<dir>/<name>-<hash>.bak-` with a hash
/// of the whole path, so files of the same name stay apart.
fn backup_prefix(path: &Path, to: Backups) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    match to {
        Backups::Beside => path.with_file_name(format!("{name}.bak-")),
        Backups::In(dir) => {
            let h = crate::cache::hash_hex(&path.display().to_string());
            dir.join(format!("{name}-{}.bak-", &h[..12]))
        }
    }
}

/// Copies `path` to a unique `<prefix><secs>-<nanos>-<pid>` (see
/// [`Backups`]), then removes all but the newest [`KEEP`] backups of it.
/// A backup in a directory of its own is private (0600 on Unix).
pub fn backup(path: &Path, to: Backups) -> std::io::Result<PathBuf> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let prefix = backup_prefix(path, to);
    let b = PathBuf::from(format!(
        "{}{}-{:09}-{}",
        prefix.display(),
        now.as_secs(),
        now.subsec_nanos(),
        std::process::id()
    ));
    match to {
        Backups::Beside => {
            std::fs::copy(path, &b)?;
        }
        Backups::In(_) => {
            let body = std::fs::read(path)?;
            if let Some(dir) = b.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let mut o = std::fs::OpenOptions::new();
            crate::platform::private_file(o.write(true).create_new(true));
            std::io::Write::write_all(&mut o.open(&b)?, &body)?;
        }
    }
    prune(&prefix);
    Ok(b)
}

/// The time and pid of a backup name's tail (`<secs>-<nanos>-<pid>`).
fn stamp(tail: &str) -> Option<(u64, u32, u32)> {
    let mut p = tail.split('-');
    let t = (
        p.next()?.parse().ok()?,
        p.next()?.parse().ok()?,
        p.next()?.parse().ok()?,
    );
    p.next().is_none().then_some(t)
}

/// Removes all but the newest [`KEEP`] files named `<prefix><stamp>`;
/// other files are never touched.
fn prune(prefix: &Path) {
    let (Some(dir), Some(head)) = (prefix.parent(), prefix.file_name()) else {
        return;
    };
    let head = head.to_string_lossy();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut found: Vec<((u64, u32, u32), PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let t = stamp(name.strip_prefix(head.as_ref())?)?;
            e.file_type().ok()?.is_file().then(|| (t, e.path()))
        })
        .collect();
    found.sort();
    let extra = found.len().saturating_sub(KEEP);
    for (_, p) in &found[..extra] {
        let _ = std::fs::remove_file(p);
    }
}

/// Writes `body` to `path` (already a [`target`]): creates missing parent
/// directories, writes atomically and keeps an existing file's
/// permissions.
pub fn write(path: &Path, body: &str) -> std::io::Result<()> {
    let mode = std::fs::metadata(path).ok().map(|m| m.permissions());
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    crate::click::write_atomic(path, body)?;
    if let Some(m) = mode {
        std::fs::set_permissions(path, m)?;
    }
    Ok(())
}

/// [`write`], after backing an existing file up; returns the backup, if
/// any.
pub fn replace(path: &Path, body: &str, to: Backups) -> std::io::Result<Option<PathBuf>> {
    let bak = match std::fs::metadata(path) {
        Ok(_) => Some(backup(path, to)?),
        Err(_) => None,
    };
    write(path, body)?;
    Ok(bak)
}

#[cfg(test)]
mod tests {
    use super::{replace, target, Backups, KEEP};

    fn tmp() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "bar-rewrite-{}-{}",
            std::process::id(),
            crate::cache::nonce()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn new_file_has_no_backup_and_gets_its_dirs() {
        let d = tmp();
        let f = d.join("a/b/f.conf");
        assert_eq!(replace(&f, "x\n", Backups::Beside).unwrap(), None);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "x\n");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn existing_file_is_backed_up() {
        let d = tmp();
        let f = d.join("f.conf");
        std::fs::write(&f, "old\n").unwrap();
        let bak = replace(&f, "new\n", Backups::Beside).unwrap().unwrap();
        assert!(bak
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("f.conf.bak-"));
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), "old\n");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "new\n");
        let _ = std::fs::remove_dir_all(d);
    }

    #[cfg(unix)]
    #[test]
    fn mode_is_kept_and_symlink_written_through() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp();
        let real = d.join("real.conf");
        std::fs::write(&real, "old\n").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o644)).unwrap();
        let link = d.join("link.conf");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let t = target(&link).unwrap();
        replace(&t, "new\n", Backups::Beside).unwrap();
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "new\n");
        let mode = std::fs::metadata(&real).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
        let _ = std::fs::remove_dir_all(d);
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_is_refused() {
        let d = tmp();
        let link = d.join("link.conf");
        std::os::unix::fs::symlink(d.join("missing"), &link).unwrap();
        assert!(target(&link)
            .unwrap_err()
            .contains("symlink target unusable"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn backups_in_a_dir_of_their_own() {
        let d = tmp();
        let f = d.join("proj/.claude/settings.local.json");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, "old\n").unwrap();
        let store = d.join("state/backups");
        let bak = replace(&f, "new\n", Backups::In(&store)).unwrap().unwrap();
        assert_eq!(bak.parent().unwrap(), store);
        let name = bak.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("settings.local.json-"), "{name}");
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), "old\n");
        // nothing new beside the file
        let beside: Vec<_> = std::fs::read_dir(f.parent().unwrap()).unwrap().collect();
        assert_eq!(beside.len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&bak).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn only_the_newest_backups_are_kept() {
        let d = tmp();
        let f = d.join("f.conf");
        std::fs::write(&f, "0\n").unwrap();
        let keep = d.join("f.conf.bak-notes.txt");
        std::fs::write(&keep, "mine").unwrap();
        let old = d.join("f.conf.bak-1-000000000-1");
        std::fs::write(&old, "ancient").unwrap();
        let mut last = None;
        for i in 1..=KEEP + 2 {
            last = replace(&f, &format!("{i}\n"), Backups::Beside).unwrap();
        }
        let baks: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("f.conf.bak-") && n != "f.conf.bak-notes.txt")
            .collect();
        assert_eq!(baks.len(), KEEP, "{baks:?}");
        assert!(!old.exists());
        assert!(keep.exists());
        assert!(last.unwrap().exists());
        let _ = std::fs::remove_dir_all(d);
    }
}
