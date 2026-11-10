//! Unmerged paths from the index file (versions 2, 3 and 4).

use super::{read, read_bytes, Repo, Unsupported};

/// Larger indexes are not parsed during a render; callers fall back to the
/// cached `git status`.
pub const MAX_INDEX: u64 = 2 * 1024 * 1024;

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn be16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

/// Paths with a stage above 0, each counted once. `hash` is the object id
/// length (20 or 32). `None` for anything this parser does not model,
/// including a split index, whose entries live in another file.
pub fn unmerged(d: &[u8], hash: usize) -> Option<usize> {
    if d.get(..4)? != b"DIRC" {
        return None;
    }
    let version = be32(d, 4)?;
    if !(2..=4).contains(&version) {
        return None;
    }
    let entries = be32(d, 8)?;
    let mut pos = 12;
    let mut path: Vec<u8> = Vec::new();
    let mut last: Option<Vec<u8>> = None;
    let mut count = 0;
    for _ in 0..entries {
        let (next, stage) = entry(d, version, hash, pos, &mut path)?;
        pos = next;
        if stage > 0 && last.as_deref() != Some(path.as_slice()) {
            count += 1;
            last = Some(path.clone());
        }
    }
    extensions_fit(d, hash, pos)?.then_some(count)
}

/// One entry at `pos`: where the next one starts and the entry's stage,
/// with `path` set to its path. `None` for a truncated entry, or an
/// extended one in a version 2 index.
fn entry(
    d: &[u8],
    version: u32,
    hash: usize,
    pos: usize,
    path: &mut Vec<u8>,
) -> Option<(usize, u16)> {
    let flags = be16(d, pos + 40 + hash)?;
    let mut name_at = pos + 40 + hash + 2;
    if flags & 0x4000 != 0 {
        if version < 3 {
            return None;
        }
        name_at += 2;
    }
    let stage = (flags >> 12) & 3;
    let next = if version == 4 {
        // strip count as git's offset varint, then a NUL-terminated suffix
        let (i, strip) = varint(d, name_at)?;
        let len = memchr::memchr(0, d.get(i..)?)?;
        path.truncate(path.len().checked_sub(strip)?);
        path.extend_from_slice(&d[i..i + len]);
        i + len + 1
    } else {
        let len = memchr::memchr(0, d.get(name_at..)?)?;
        path.clear();
        path.extend_from_slice(&d[name_at..name_at + len]);
        // entry padded with 1 to 8 NULs to a multiple of 8 bytes
        pos + ((name_at - pos + len + 8) & !7)
    };
    (next <= d.len()).then_some((next, stage))
}

/// Git's offset varint at `at`: the position after it and its value.
fn varint(d: &[u8], at: usize) -> Option<(usize, usize)> {
    let mut i = at;
    let mut c = *d.get(i)?;
    i += 1;
    let mut v = usize::from(c & 0x7f);
    while c & 0x80 != 0 {
        c = *d.get(i)?;
        i += 1;
        v = v.checked_add(1)?.checked_mul(128)? + usize::from(c & 0x7f);
    }
    Some((i, v))
}

/// Walks the extensions from `pos` (4-byte signature, 4-byte size, then
/// the trailing checksum): `true` when the last one ends where the
/// checksum starts, `false` for one running into it or bytes left over.
/// `None` for a split index (`link`): its entries live in another file.
fn extensions_fit(d: &[u8], hash: usize, mut pos: usize) -> Option<bool> {
    let end = d.len().checked_sub(hash)?;
    while pos.checked_add(8)? <= end {
        if d.get(pos..pos + 4)? == b"link" {
            return None;
        }
        let size = usize::try_from(be32(d, pos + 4)?).ok()?;
        pos = pos.checked_add(8)?.checked_add(size)?;
    }
    Some(pos == end)
}

impl Repo {
    /// Object id length of the repository: 32 with `objectFormat = sha256`.
    fn hash_len(&self) -> Result<usize, Unsupported> {
        let sha256 = read(&self.common_dir.join("config"))?.is_some_and(|c| {
            c.lines().any(|l| {
                let l = l.trim().to_ascii_lowercase();
                l.starts_with("objectformat") && l.contains("sha256")
            })
        });
        Ok(if sha256 { 32 } else { 20 })
    }

    /// Identity, mtime and size of the index file: equal stamps, same
    /// contents. `None` without an index, or with `GIT_INDEX_FILE` set.
    pub fn index_stamp(&self) -> Option<String> {
        if std::env::var_os("GIT_INDEX_FILE").is_some() {
            return None;
        }
        let m = std::fs::metadata(self.git_dir.join("index")).ok()?;
        let p = crate::platform::file_id(&m);
        Some(format!("{p}:{}:{}", crate::platform::mtime_ns(&m), m.len()))
    }

    /// Unmerged paths in the index, as `git status` counts them.
    pub fn conflicts(&self) -> Result<usize, Unsupported> {
        if std::env::var_os("GIT_INDEX_FILE").is_some() {
            return Err(Unsupported);
        }
        let Some(data) = read_bytes(&self.git_dir.join("index"), MAX_INDEX)? else {
            return Ok(0);
        };
        unmerged(&data, self.hash_len()?).ok_or(Unsupported)
    }
}
