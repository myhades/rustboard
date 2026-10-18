use crate::MAX_CHUNK;
#[cfg(test)]
use crate::test_pattern::PatternSource;
use anyhow::{Result, ensure};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::SystemTime,
};

/// Each call is bounded; implementations must return an error after cancellation/version loss.
pub trait ReadAt: Send + Sync {
    fn len(&self) -> u64;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize>;
}

/// Keeps an open handle, so replacing the pathname cannot redirect the offered file.
/// Metadata checked before AND after every read; not a cryptographic snapshot.
pub struct LocalFile {
    file: Mutex<File>,
    len: u64,
    modified: SystemTime,
    cancelled: AtomicBool,
    pub bytes_read: AtomicU64,
}
impl LocalFile {
    pub fn open(path: &Path) -> Result<Self> {
        let metadata = std::fs::symlink_metadata(path)?;
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "only regular non-symlink files supported"
        );
        let file = File::open(path)?;
        Self::from_file(file)
    }
    pub fn from_file(file: File) -> Result<Self> {
        let metadata = file.metadata()?;
        ensure!(metadata.is_file(), "only regular files supported");
        Ok(Self {
            len: metadata.len(),
            modified: metadata.modified()?,
            file: Mutex::new(file),
            cancelled: AtomicBool::new(false),
            bytes_read: AtomicU64::new(0),
        })
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn version(&self) -> String {
        format!(
            "{}-{}",
            self.len,
            self.modified
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    }
}
impl ReadAt for LocalFile {
    fn len(&self) -> u64 {
        self.len
    }
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        ensure!(!self.cancelled.load(Ordering::Acquire), "cancelled");
        ensure!(
            buffer.len() <= MAX_CHUNK && offset <= self.len,
            "invalid range"
        );
        let mut file = self
            .file
            .lock()
            .map_err(|_| anyhow::anyhow!("source lock poisoned"))?;
        let check = |file: &File| -> Result<()> {
            let m = file.metadata()?;
            ensure!(
                m.len() == self.len && m.modified()? == self.modified,
                "version_changed"
            );
            Ok(())
        };
        check(&file)?;
        file.seek(SeekFrom::Start(offset))?;
        let limit = buffer
            .len()
            .min((self.len - offset).min(MAX_CHUNK as u64) as usize);
        let n = file.read(&mut buffer[..limit])?;
        check(&file)?;
        self.bytes_read.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn random_access_and_bounds() {
        let source = PatternSource::new(4096);
        let mut b = [0; 20];
        assert_eq!(source.read_at(4090, &mut b).unwrap(), 6);
        assert_eq!(b[0], (4090 % 251) as u8);
        assert_eq!(source.read_at(4096, &mut b).unwrap(), 0);
        assert!(source.read_at(u64::MAX, &mut b).is_err());
        assert!(source.read_at(0, &mut vec![0; MAX_CHUNK + 1]).is_err());
    }
    #[test]
    fn local_mutation_and_cancel() {
        let path = std::env::temp_dir().join(format!("rustboard-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"original").unwrap();
        let source = LocalFile::open(&path).unwrap();
        assert_eq!(source.bytes_read.load(Ordering::Relaxed), 0);
        std::fs::write(&path, b"changed size").unwrap();
        assert!(source.read_at(0, &mut [0; 4]).is_err());
        source.cancel();
        assert!(source.read_at(0, &mut []).is_err());
        drop(source);
        std::fs::remove_file(path).unwrap();
    }
}
