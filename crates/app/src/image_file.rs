//! Clipboard image bytes become an ordinary, offer-owned file. No image wire type.
use anyhow::{Result, ensure};
use rustboard_core::{
    Content, FileEntry, LocalFile, Offer, ReadAt,
    engine::{Engine, new_id},
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

struct OwnedImage {
    source: Option<LocalFile>,
    path: PathBuf,
}
impl Drop for OwnedImage {
    fn drop(&mut self) {
        drop(self.source.take());
        let _ = std::fs::remove_file(&self.path);
    }
}
impl ReadAt for OwnedImage {
    fn len(&self) -> u64 {
        self.source.as_ref().unwrap().len()
    }
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        self.source.as_ref().unwrap().read_at(offset, buffer)
    }
}
pub fn prepare(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    ensure!(
        !std::fs::symlink_metadata(dir)?.file_type().is_symlink(),
        "unsafe image directory"
    );
    // Called only after acquiring the profile singleton. Never traverse directories.
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with("clipboard-")
        {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}
pub fn offer(engine: &mut Engine, path: PathBuf) -> Result<Offer> {
    let result = (|| {
        #[cfg(windows)]
        let source = crate::windows::local_files::open(&path)?;
        #[cfg(not(windows))]
        let source = LocalFile::open(&path)?;
        ensure!(
            source.len() > 0 && source.len() <= (MAX_IMAGE_BYTES + 14) as u64,
            "image size limit"
        );
        let version = source.version();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("image name"))?
            .to_owned();
        let source = Arc::new(OwnedImage {
            source: Some(source),
            path: path.clone(),
        });
        let id = new_id();
        engine.local(
            Content::Files {
                files: vec![FileEntry {
                    file_id: id.clone(),
                    relative_path: name,
                    size: Some(source.len()),
                    version,
                    media_type: "application/octet-stream".into(),
                }],
            },
            [(id, source as Arc<dyn ReadAt>)].into(),
        )
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_image_is_removed_on_withdraw_and_rejection() {
        let dir = std::env::temp_dir().join(new_id());
        prepare(&dir).unwrap();
        let path = dir.join("clipboard-test.png");
        std::fs::write(&path, b"synthetic encoded bytes").unwrap();
        let mut engine = Engine::new(new_id());
        offer(&mut engine, path.clone()).unwrap();
        assert!(path.exists());
        engine.withdraw();
        assert!(!path.exists());
        std::fs::write(&path, b"synthetic encoded bytes").unwrap();
        engine.paused = true;
        assert!(offer(&mut engine, path.clone()).is_err());
        assert!(!path.exists());
        std::fs::remove_dir(&dir).unwrap();
    }
}
