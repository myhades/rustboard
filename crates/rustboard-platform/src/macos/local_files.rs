use anyhow::{Result, ensure};
use rustboard_core::{
    Content, FileEntry, LocalFile, Offer, ReadAt,
    engine::{Engine, new_id},
};
use std::{
    collections::HashMap,
    ffi::CString,
    fs::{self, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::Arc,
};

unsafe extern "C" {
    fn fclonefileat(src: i32, dst_dir: i32, dst: *const libc::c_char, flags: u32) -> i32;
}
struct Snapshot {
    source: LocalFile,
    path: PathBuf,
}
impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
impl ReadAt for Snapshot {
    fn len(&self) -> u64 {
        self.source.len()
    }
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        self.source.read_at(offset, buffer)
    }
}
pub fn offer(engine: &mut Engine, paths: Vec<PathBuf>, dir: &Path) -> Result<Offer> {
    ensure!(
        !paths.is_empty() && paths.len() <= 256,
        "invalid file count"
    );
    let mut files = Vec::new();
    let mut sources: HashMap<String, Arc<dyn ReadAt>> = HashMap::new();
    for path in paths {
        // Opening a File Provider placeholder can itself trigger hydration. Reject
        // it using metadata first, then recheck the opened inode against races.
        let metadata = fs::symlink_metadata(&path)?;
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "regular file required"
        );
        ensure!(
            std::os::macos::fs::MetadataExt::st_flags(&metadata) & 0x4000_0000 == 0,
            "dataless sources unsupported"
        );
        let input = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)?;
        ensure!(input.metadata()?.is_file(), "regular file required");
        // Reject dataless provider sources instead of causing hydration during capture.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        ensure!(
            unsafe { libc::fstat(input.as_raw_fd(), &mut stat) } == 0,
            "source stat failed"
        );
        ensure!(
            stat.st_flags & 0x4000_0000 == 0,
            "dataless sources unsupported"
        );
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid name"))?
            .to_owned();
        rustboard_core::validate_path(&name)?;
        let id = new_id();
        let snapshot = dir.join(&id);
        let cpath = CString::new(snapshot.as_os_str().as_bytes())?;
        // Atomic APFS COW clone; no byte-copy fallback on unsupported filesystems.
        ensure!(
            unsafe { fclonefileat(input.as_raw_fd(), libc::AT_FDCWD, cpath.as_ptr(), 0) } == 0,
            "APFS clone required"
        );
        let source = match LocalFile::open(&snapshot) {
            Ok(s) => s,
            Err(e) => {
                let _ = fs::remove_file(&snapshot);
                return Err(e);
            }
        };
        let source = Arc::new(Snapshot {
            source,
            path: snapshot,
        });
        files.push(FileEntry {
            file_id: id.clone(),
            relative_path: name,
            size: Some(source.len()),
            version: id.clone(),
            media_type: "application/octet-stream".into(),
        });
        sources.insert(id, source);
    }
    engine.local(Content::Files { files }, sources)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clone_survives_source_mutation_and_cleans_up() {
        let dir = std::env::temp_dir().join(new_id());
        fs::create_dir(&dir).unwrap();
        let input = dir.join("input.bin");
        fs::write(&input, b"original bytes").unwrap();
        let mut engine = Engine::new(new_id());
        let o = offer(&mut engine, vec![input.clone()], &dir).unwrap();
        let Content::Files { files } = o.content else {
            panic!()
        };
        fs::write(&input, b"changed bytes!").unwrap();
        let s = engine
            .source(
                &o.source_device,
                &o.offer_id,
                &files[0].file_id,
                &files[0].version,
            )
            .unwrap();
        let mut b = [0; 14];
        assert_eq!(s.read_at(0, &mut b).unwrap(), 14);
        assert_eq!(&b, b"original bytes");
        engine.withdraw();
        drop(s);
        assert!(!dir.join(&files[0].file_id).exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
