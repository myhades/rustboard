use anyhow::{Result, ensure};
use rustboard::core::{MAX_CHUNK, ReadAt};
use std::sync::atomic::{AtomicU64, Ordering};

/// Deterministic virtual content for the OLE acceptance harness; no backing file.
pub struct PatternSource {
    pub size: u64,
    pub bytes_read: AtomicU64,
    pub calls: AtomicU64,
}
impl PatternSource {
    pub fn new(size: u64) -> Self {
        Self {
            size,
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
        for (i, byte) in buffer[..n].iter_mut().enumerate() {
            *byte = ((offset + i as u64) % 251) as u8;
        }
        self.bytes_read.fetch_add(n as u64, Ordering::Relaxed);
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(n)
    }
}
