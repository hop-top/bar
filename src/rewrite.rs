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

/// Copies `path` to a unique `<name>.bak-<secs>-<nanos>-<pid>` beside it.
pub fn backup(path: &Path) -> std::io::Result<PathBuf> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let b = path.with_file_name(format!(
        "{name}.bak-{}-{:09}-{}",
        now.as_secs(),
        now.subsec_nanos(),
        std::process::id()
    ));
    std::fs::copy(path, &b)?;
    Ok(b)
}

/// Writes `body` to `path` (already a [`target`]): backs an existing file
/// up first, creates missing parent directories, writes atomically and
/// keeps the old file's permissions. Returns the backup, if any.
pub fn replace(path: &Path, body: &str) -> std::io::Result<Option<PathBuf>> {
    let old = std::fs::metadata(path).ok();
    let bak = match &old {
        Some(_) => Some(backup(path)?),
        None => None,
    };
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    crate::click::write_atomic(path, body)?;
    if let Some(m) = old {
        std::fs::set_permissions(path, m.permissions())?;
    }
    Ok(bak)
}

#[cfg(test)]
mod tests {
    use super::{replace, target};

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
        assert_eq!(replace(&f, "x\n").unwrap(), None);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "x\n");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn existing_file_is_backed_up() {
        let d = tmp();
        let f = d.join("f.conf");
        std::fs::write(&f, "old\n").unwrap();
        let bak = replace(&f, "new\n").unwrap().unwrap();
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
        replace(&t, "new\n").unwrap();
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
}
