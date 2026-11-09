//! Replacing a file the user owns (`settings.json`, `bar.conf`): written
//! through a symlink to its target, the old file backed up (see
//! [`Backups`]), the new one written atomically and flushed to disk,
//! with the old file's permissions.

use std::path::{Path, PathBuf};

/// The file actually written for `path`: a symlink's target, never the
/// link itself; a link whose target is missing is refused.
pub fn target(path: &Path) -> Result<PathBuf, String> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path)
            .map(|p| plain_path(&p))
            .map_err(|e| format!("{}: symlink target unusable: {e}", path.display())),
        _ => Ok(path.to_path_buf()),
    }
}

/// `p` without the Windows verbatim prefix `canonicalize` adds:
/// `\\?\C:\x` becomes `C:\x`, `\\?\UNC\host\share` becomes
/// `\\host\share`. Paths that need the prefix (too long, or a final
/// part Windows would change) keep it.
pub fn plain_path(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    let plain = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s
        .strip_prefix(r"\\?\")
        .filter(|r| r.as_bytes().get(1) == Some(&b':') && r.as_bytes()[0].is_ascii_alphabetic())
    {
        rest.to_string()
    } else {
        return p.to_path_buf();
    };
    let reserved = plain
        .rsplit('\\')
        .next()
        .is_some_and(|n| n.ends_with(['.', ' ']));
    if plain.len() >= 260 || reserved {
        return p.to_path_buf();
    }
    PathBuf::from(plain)
}

/// Refuses when `path` (a [`target`]) no longer holds `expected`, the
/// text it was read as (`None`: it did not exist), so a change made by
/// someone else since then is never overwritten.
pub fn unchanged(path: &Path, expected: Option<&str>) -> Result<(), String> {
    let now = match std::fs::read_to_string(path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    if now.as_deref() == expected {
        Ok(())
    } else {
        Err(format!(
            "{} changed since it was read; nothing written",
            path.display()
        ))
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
    write_durable(path, body)?;
    if let Some(m) = mode {
        std::fs::set_permissions(path, m)?;
    }
    Ok(())
}

/// Writes a sibling temp file (0600), flushes it to disk, renames it
/// over `path` and, on Unix, flushes the directory, so a crash leaves
/// the old file or the new one, never an empty one.
fn write_durable(path: &Path, body: &str) -> std::io::Result<()> {
    use std::io::Write;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", crate::cache::nonce()));
    let mut o = std::fs::OpenOptions::new();
    crate::platform::private_file(o.write(true).create_new(true));
    let written = o.open(&tmp).and_then(|mut f| {
        f.write_all(body.as_bytes())?;
        f.sync_all()
    });
    written
        .and_then(|()| std::fs::rename(&tmp, path))
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })?;
    #[cfg(unix)]
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        if let Ok(d) = std::fs::File::open(dir) {
            let _ = d.sync_all();
        }
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
    use super::{plain_path, replace, unchanged, Backups, KEEP};
    // only the symlink tests, which are Unix-only, resolve targets
    #[cfg(unix)]
    use super::target;

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
        // no temp file left behind
        let names: Vec<_> = std::fs::read_dir(f.parent().unwrap()).unwrap().collect();
        assert_eq!(names.len(), 1);
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

    #[test]
    fn unchanged_compares_with_the_text_read() {
        let d = tmp();
        let f = d.join("f.conf");
        assert!(unchanged(&f, None).is_ok());
        std::fs::write(&f, "a\n").unwrap();
        assert!(unchanged(&f, Some("a\n")).is_ok());
        let e = unchanged(&f, None).unwrap_err();
        assert!(e.contains("changed since it was read"), "{e}");
        std::fs::write(&f, "b\n").unwrap();
        assert!(unchanged(&f, Some("a\n")).is_err());
        std::fs::remove_file(&f).unwrap();
        assert!(unchanged(&f, Some("a\n")).is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn verbatim_prefix_is_dropped() {
        use std::path::{Path, PathBuf};
        let p = |s: &str| plain_path(Path::new(s));
        assert_eq!(
            p(r"\\?\C:\Users\me\bar.conf"),
            PathBuf::from(r"C:\Users\me\bar.conf")
        );
        assert_eq!(p(r"\\?\UNC\host\share\f"), PathBuf::from(r"\\host\share\f"));
        assert_eq!(p("/home/me/bar.conf"), PathBuf::from("/home/me/bar.conf"));
        let long = format!(r"\\?\C:\{}", "a".repeat(300));
        assert_eq!(p(&long), PathBuf::from(&long));
        assert_eq!(p(r"\\?\C:\x\name."), PathBuf::from(r"\\?\C:\x\name."));
        assert_eq!(p(r"\\?\Volume{x}\f"), PathBuf::from(r"\\?\Volume{x}\f"));
    }
}
