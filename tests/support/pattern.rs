use anyhow::{Result, ensure};
use rustboard::core::{MAX_CHUNK, ReadAt};
use std::sync::atomic::{AtomicU64, Ordering};

/// Deterministic virtual content for the OLE acceptance harness; no backing file.
pub struct PatternSource {
    pub size: u64,
    pattern: Vec<u8>,
    pub bytes_read: AtomicU64,
    pub calls: AtomicU64,
}
impl PatternSource {
    pub fn new(size: u64) -> Self {
        Self {
            size,
            pattern: (0..MAX_CHUNK + 250).map(|i| (i % 251) as u8).collect(),
            bytes_read: AtomicU64::new(0),
            calls: AtomicU64::new(0),
        }
    }
}
impl ReadAt for PatternSource {
    fn len(&self) -> u64 {
        self.size
    }
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        ensure!(
            buffer.len() <= MAX_CHUNK && offset <= self.size,
            "invalid range"
        );
        let n = buffer
            .len()
            .min((self.size - offset).min(MAX_CHUNK as u64) as usize);
        let start = (offset % 251) as usize;
        buffer[..n].copy_from_slice(&self.pattern[start..start + n]);
        self.bytes_read.fetch_add(n as u64, Ordering::Relaxed);
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(n)
    }
}
