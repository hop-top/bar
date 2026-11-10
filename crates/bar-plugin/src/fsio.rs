//! Files and reads with limits: capped stream reads, owner-only atomic
//! writes, and reads that refuse a file other users could have touched.

use crate::secret::{wipe, Secret};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Reads at most `cap` bytes; a longer stream is an error, not a
/// truncated body, whatever the server declared up front.
pub fn read_capped(r: impl Read, cap: usize) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    r.take(cap as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("read: {e}"))?;
    if buf.len() > cap {
        return Err(format!("response over {cap} bytes"));
    }
    Ok(buf)
}

/// Creates `dir` (and parents), owner-only (0700) on Unix.
pub fn private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)
    }
}

/// Files opened with `o` are created owner-only (0600) on Unix.
pub fn private_file(o: &mut OpenOptions) -> &mut OpenOptions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600)
    }
    #[cfg(not(unix))]
    {
        o
    }
}

/// Writes `bytes` to `path` through a sibling temp file (0600, flushed to
/// disk) renamed over it, so a reader sees the old or the new content,
/// never a part. The parent directory must exist.
pub fn write_atomic_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let _ = fs::remove_file(&tmp);
    let mut o = OpenOptions::new();
    private_file(o.write(true).create_new(true));
    let r = o.open(&tmp).and_then(|mut f| {
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    });
    if r.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    r
}

/// Why a private file was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    Missing,
    NotRegular,
    /// Owned by another user.
    Owner,
    /// Group or other permission bits set (the mode, e.g. `0o644`).
    Mode(u32),
    TooLarge(usize),
    Io(String),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileError::Missing => write!(f, "not found"),
            FileError::NotRegular => write!(f, "not a regular file"),
            FileError::Owner => write!(f, "not owned by the current user"),
            FileError::Mode(m) => write!(f, "mode {m:04o}, expected 0600"),
            FileError::TooLarge(n) => write!(f, "over {n} bytes"),
            FileError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// `Ok` when `meta` is a regular file owned by the current user with no
/// group or other permission bits. Only Unix has such modes; elsewhere any
/// regular file passes.
pub fn check_private(meta: &fs::Metadata) -> Result<(), FileError> {
    if !meta.is_file() {
        return Err(FileError::NotRegular);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // SAFETY: geteuid has no preconditions and cannot fail
        if meta.uid() != unsafe { libc::geteuid() } {
            return Err(FileError::Owner);
        }
        let mode = meta.permissions().mode() & 0o7777;
        if mode & 0o077 != 0 {
            return Err(FileError::Mode(mode));
        }
    }
    Ok(())
}

/// The content of `path` (at most `cap` bytes) when it passes
/// `check_private`. The check runs on the opened file, so the file read is
/// the file checked. A symlink is followed, and its target must pass.
/// The bytes sit in a `Secret`: wiped when dropped, never reallocated.
pub fn read_private(path: &Path, cap: usize) -> Result<Secret, FileError> {
    let mut f = File::open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => FileError::Missing,
        _ => FileError::Io(e.to_string()),
    })?;
    check_private(&f.metadata().map_err(|e| FileError::Io(e.to_string()))?)?;
    let mut buf = Secret::with_capacity(cap + 1);
    let mut chunk = [0u8; 1024];
    let r = loop {
        match f.read(&mut chunk) {
            Ok(0) => break Ok(()),
            Ok(n) if buf.as_bytes().len() + n > cap => break Err(FileError::TooLarge(cap)),
            Ok(n) => buf.push(&chunk[..n]),
            Err(e) => break Err(FileError::Io(e.to_string())),
        }
    };
    wipe(&mut chunk);
    r.map(|()| buf)
}

/// A path made absolute against `home` for a leading `~/`; others as given.
pub fn expand_home(p: &str, home: Option<&Path>) -> Option<PathBuf> {
    match p.strip_prefix("~/") {
        Some(rest) => Some(home?.join(rest)),
        None => Some(PathBuf::from(p)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bar-plugin-fsio-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn read_capped_refuses_an_overlong_stream() {
        assert_eq!(read_capped(&b"abc"[..], 3).unwrap(), b"abc");
        assert!(read_capped(&b"abcd"[..], 3).is_err());
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp() {
        let d = tmp("atomic");
        let p = d.join("f.json");
        write_atomic_private(&p, b"one").unwrap();
        write_atomic_private(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::read_dir(&d).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn private_reads_refuse_loose_modes_and_big_files() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("private");
        let p = d.join("k");
        fs::write(&p, "secret\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(read_private(&p, 64).unwrap_err(), FileError::Mode(0o644));
        fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
        assert_eq!(read_private(&p, 64).unwrap_err(), FileError::Mode(0o640));
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_private(&p, 64).unwrap().as_bytes(), b"secret\n");
        assert_eq!(read_private(&p, 3).unwrap_err(), FileError::TooLarge(3));
        assert_eq!(read_private(&d, 3).unwrap_err(), FileError::NotRegular);
        assert_eq!(
            read_private(&d.join("none"), 3).unwrap_err(),
            FileError::Missing
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn home_expansion() {
        let h = Path::new("/h");
        assert_eq!(expand_home("~/k", Some(h)), Some(PathBuf::from("/h/k")));
        assert_eq!(expand_home("/a/k", None), Some(PathBuf::from("/a/k")));
        assert_eq!(expand_home("~/k", None), None);
    }
}
