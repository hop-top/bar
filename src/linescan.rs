//! Incremental line reader shared by the transcript and spend-history
//! scanners: a `Mark` remembers where the last scan stopped and whether
//! the file was replaced since.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::time::Instant;

/// Longer lines are skipped unparsed.
pub const MAX_LINE: usize = 512 * 1024;
const CHUNK: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mark {
    pub ino: u64,
    pub size: u64,
    pub mtime_ns: i64,
    /// Start of the first unconsumed line.
    pub offset: u64,
    /// Inside an over-long line: drop bytes up to the next newline.
    pub skip: bool,
}

pub fn mtime_ns(meta: &fs::Metadata) -> i64 {
    meta.mtime()
        .saturating_mul(1_000_000_000)
        .saturating_add(meta.mtime_nsec())
}

impl Mark {
    /// A changed inode, a smaller file, or an mtime that moved without the
    /// size growing means the file was replaced: scan from the start.
    pub fn replaced_by(&self, meta: &fs::Metadata) -> bool {
        let (size, ns) = (meta.len(), mtime_ns(meta));
        self.ino != meta.ino()
            || size < self.size
            || ns < self.mtime_ns
            || (size == self.size && ns != self.mtime_ns)
    }

    pub fn fresh(meta: &fs::Metadata) -> Mark {
        Mark {
            ino: meta.ino(),
            ..Mark::default()
        }
    }

    /// Bytes remain to be read.
    pub fn pending(&self, size: u64) -> bool {
        self.offset < size || self.skip
    }

    /// Records the size and mtime seen; true when they changed.
    pub fn stamp(&mut self, meta: &fs::Metadata) -> bool {
        let (size, ns) = (meta.len(), mtime_ns(meta));
        let changed = self.size != size || self.mtime_ns != ns;
        self.size = size;
        self.mtime_ns = ns;
        changed
    }

    /// Feeds each complete line from `offset` to `on_line`, reading at most
    /// `cap` bytes and stopping at `deadline`. True when it reached the end
    /// of the file. Only newline-terminated lines advance the offset.
    pub fn scan(
        &mut self,
        file: &mut fs::File,
        size: u64,
        cap: u64,
        deadline: Option<Instant>,
        mut on_line: impl FnMut(&[u8]),
    ) -> bool {
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return true;
        }
        let mut pos = self.offset;
        let mut used = 0u64;
        let mut line: Vec<u8> = Vec::new();
        let mut skipping = self.skip;
        let mut buf = vec![0u8; CHUNK];
        let eof = loop {
            if pos >= size {
                break true;
            }
            if used >= cap || deadline.is_some_and(|d| Instant::now() >= d) {
                break false;
            }
            let want = (CHUNK as u64).min(cap - used).min(size - pos) as usize;
            let n = match file.read(&mut buf[..want]) {
                Ok(0) | Err(_) => break true,
                Ok(n) => n,
            };
            used += n as u64;
            for seg in buf[..n].split_inclusive(|b| *b == b'\n') {
                pos += seg.len() as u64;
                if !skipping {
                    if line.len() + seg.len() > MAX_LINE {
                        skipping = true;
                        line.clear();
                    } else {
                        line.extend_from_slice(seg);
                    }
                }
                if seg.last() == Some(&b'\n') {
                    if !skipping {
                        on_line(&line);
                    }
                    line.clear();
                    skipping = false;
                    self.offset = pos;
                }
            }
        };
        self.skip = skipping;
        if skipping {
            self.offset = pos;
        }
        eof
    }
}

/// A `"usage"` key: the quoted word followed by optional spaces and a
/// colon. Inside a string the quotes are escaped, so it cannot match there.
pub fn has_usage_key(line: &[u8]) -> bool {
    const MARK: &[u8] = b"\"usage\"";
    line.windows(MARK.len()).enumerate().any(|(i, w)| {
        w == MARK
            && line[i + MARK.len()..]
                .iter()
                .find(|b| !b.is_ascii_whitespace())
                == Some(&b':')
    })
}
