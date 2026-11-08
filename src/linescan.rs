//! Incremental line reader shared by the transcript and spend-history
//! scanners: a `Mark` remembers where the last scan stopped and whether
//! the file was replaced since.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::sync::OnceLock;
use std::time::Instant;

use memchr::memmem::Finder;

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
    crate::platform::mtime_ns(meta)
}

impl Mark {
    /// A changed inode, a smaller file, or an mtime that moved without the
    /// size growing means the file was replaced: scan from the start.
    pub fn replaced_by(&self, meta: &fs::Metadata) -> bool {
        let (size, ns) = (meta.len(), mtime_ns(meta));
        self.ino != crate::platform::file_id(meta)
            || size < self.size
            || ns < self.mtime_ns
            || (size == self.size && ns != self.mtime_ns)
    }

    pub fn fresh(meta: &fs::Metadata) -> Mark {
        Mark {
            ino: crate::platform::file_id(meta),
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
            let chunk = &buf[..n];
            let mut start = 0;
            for nl in memchr::memchr_iter(b'\n', chunk) {
                let seg = &chunk[start..=nl];
                start = nl + 1;
                pos += seg.len() as u64;
                // a line wholly inside the chunk is handed over uncopied
                if !skipping && line.len() + seg.len() <= MAX_LINE {
                    if line.is_empty() {
                        on_line(seg);
                    } else {
                        line.extend_from_slice(seg);
                        on_line(&line);
                    }
                }
                line.clear();
                skipping = false;
                self.offset = pos;
            }
            let rest = &chunk[start..];
            pos += rest.len() as u64;
            if !skipping && !rest.is_empty() {
                if line.len() + rest.len() > MAX_LINE {
                    skipping = true;
                    line.clear();
                } else {
                    line.extend_from_slice(rest);
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
    static MARK: Needle = Needle::new(b"\"usage\"");
    let mut at = 0;
    // every occurrence, overlapping ones included
    while let Some(i) = MARK.find(&line[at..]) {
        let end = at + i + MARK.len();
        if line[end..].iter().find(|b| !b.is_ascii_whitespace()) == Some(&b':') {
            return true;
        }
        at += i + 1;
    }
    false
}

/// A substring searcher built on first use and kept, so a per-line check
/// does not rebuild it.
pub struct Needle {
    needle: &'static [u8],
    finder: OnceLock<Finder<'static>>,
}

impl Needle {
    pub const fn new(needle: &'static [u8]) -> Needle {
        Needle {
            needle,
            finder: OnceLock::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.needle.len()
    }

    pub fn is_empty(&self) -> bool {
        self.needle.is_empty()
    }

    /// Offset of the first occurrence in `hay`.
    pub fn find(&self, hay: &[u8]) -> Option<usize> {
        self.finder
            .get_or_init(|| Finder::new(self.needle))
            .find(hay)
    }

    pub fn is_in(&self, hay: &[u8]) -> bool {
        self.find(hay).is_some()
    }
}
