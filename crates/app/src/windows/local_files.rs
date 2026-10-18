use anyhow::{Result, ensure};
use rustboard_core::{
    Content, FileEntry, LocalFile, ReadAt,
    engine::{Engine, new_id},
};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Deny write/delete sharing for the whole offer lifetime. Refuse all reparse ancestors.
/// No absolute paths leave this module. The open handle is the source identity.
pub fn open(path: &Path) -> Result<LocalFile> {
    ensure!(path.is_absolute(), "expected absolute Explorer path");
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let m = std::fs::symlink_metadata(ancestor)?;
        ensure!(
            m.file_attributes() & 0x400 == 0,
            "reparse points are not supported"
        );
    }
    let file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(0x00200000)
        .open(path)?;
    ensure!(
        file.metadata()?.file_attributes() & 0x400 == 0,
        "reparse file rejected"
    );
    LocalFile::from_file(file)
}
pub fn offer(engine: &mut Engine, paths: Vec<PathBuf>) -> Result<()> {
    ensure!(
        !paths.is_empty() && paths.len() <= rustboard_core::MAX_FILES,
        "invalid file count"
    );
    let mut files = Vec::new();
    let mut sources: HashMap<String, Arc<dyn ReadAt>> = HashMap::new();
    for path in paths {
        let source = Arc::new(open(&path)?);
        let id = new_id();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid filename"))?;
        files.push(FileEntry {
            file_id: id.clone(),
            relative_path: name.to_string(),
            size: Some(source.len()),
            version: source.version(),
            media_type: "application/octet-stream".into(),
        });
        sources.insert(id, source);
    }
    engine.local(Content::Files { files }, sources)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offered_file_denies_mutation_until_release() {
        let path = std::env::temp_dir().join(format!("rustboard-{}", new_id()));
        std::fs::write(&path, b"locked").unwrap();
        let source = open(&path).unwrap();
        assert!(std::fs::write(&path, b"mutate").is_err());
        assert!(std::fs::remove_file(&path).is_err());
        drop(source);
        std::fs::remove_file(path).unwrap();
    }
}
