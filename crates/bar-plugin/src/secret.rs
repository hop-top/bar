//! Bytes holding a credential, overwritten with zeros when dropped so the
//! token does not linger in freed memory of the background job.

// Allowed: volatile write that wipes the secret.
#![allow(unsafe_code)]

use std::sync::atomic::{compiler_fence, Ordering};

/// Zeros `b` with volatile writes the optimiser cannot remove.
pub fn wipe(b: &mut [u8]) {
    for x in b.iter_mut() {
        // SAFETY: `x` is a valid, exclusive reference into `b`
        unsafe { std::ptr::write_volatile(x, 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

/// Owned secret bytes, wiped on drop.
#[derive(Default)]
pub struct Secret(Vec<u8>);

impl Secret {
    pub fn new(bytes: Vec<u8>) -> Secret {
        Secret(bytes)
    }

    /// Room for `n` bytes, so pushing up to `n` never reallocates (and
    /// leaves no unwiped copy behind).
    pub fn with_capacity(n: usize) -> Secret {
        Secret(Vec::with_capacity(n))
    }

    pub fn push(&mut self, bytes: &[u8]) {
        if self.0.len() + bytes.len() > self.0.capacity() {
            // grow by hand: the old buffer is wiped before it is freed
            let mut grown = Vec::with_capacity((self.0.len() + bytes.len()) * 2);
            grown.extend_from_slice(&self.0);
            wipe(&mut self.0);
            self.0 = grown;
        }
        self.0.extend_from_slice(bytes);
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret({} bytes)", self.0.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wipe_zeros_every_byte() {
        let mut b = *b"sk-ant-secret";
        wipe(&mut b);
        assert!(b.iter().all(|&x| x == 0));
    }

    #[test]
    fn push_keeps_the_bytes_across_growth() {
        let mut s = Secret::with_capacity(2);
        s.push(b"ab");
        s.push(b"cdef");
        assert_eq!(s.as_bytes(), b"abcdef");
        assert_eq!(format!("{s:?}"), "Secret(6 bytes)");
    }
}
