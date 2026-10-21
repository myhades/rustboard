use super::{FileEntry, MAX_FILES, MAX_MANIFEST, engine::new_id, validate_path};
use anyhow::{Result, ensure};
use std::{fs, path::PathBuf, time::UNIX_EPOCH};

/// Capture names and attributes only. Platform sources decide how to retain contents.
pub fn manifest(roots: Vec<PathBuf>) -> Result<Vec<(PathBuf, FileEntry)>> {
    ensure!(
        !roots.is_empty() && roots.len() <= MAX_FILES,
        "invalid file count"
    );
    let mut pending = Vec::new();
    for path in roots.into_iter().rev() {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid filename"))?
            .to_owned();
        pending.push((path, name));
    }
    let mut entries = Vec::new();
    let mut names = std::collections::HashSet::new();
    let mut bytes = 0;
    while let Some((path, relative_path)) = pending.pop() {
        ensure!(entries.len() < MAX_FILES, "too many files");
        validate_path(&relative_path)?;
        ensure!(
            relative_path.split('/').count() <= 64,
            "directory nesting too deep"
        );
        ensure!(
            names.insert(relative_path.to_lowercase()),
            "duplicate filename"
        );
        let metadata = fs::symlink_metadata(&path)?;
        ensure!(
            !metadata.file_type().is_symlink() && (metadata.is_file() || metadata.is_dir()),
            "only regular files and directories supported"
        );
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            ensure!(
                metadata.file_attributes() & 0x400 == 0,
                "reparse points unsupported"
            );
        }
        let directory = metadata.is_dir();
        let modified = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        #[cfg(unix)]
        let executable = {
            use std::os::unix::fs::PermissionsExt;
            Some(metadata.permissions().mode() & 0o111 != 0)
        };
        #[cfg(not(unix))]
        let executable = None;
        let entry = FileEntry {
            file_id: new_id(),
            relative_path: relative_path.clone(),
            size: Some(if directory { 0 } else { metadata.len() }),
            version: format!("{}-{}", metadata.len(), modified.as_nanos()),
            media_type: if directory {
                "inode/directory"
            } else {
                "application/octet-stream"
            }
            .into(),
            directory,
            modified_ms: Some(modified.as_millis().try_into()?),
            executable,
        };
        bytes += serde_json::to_vec(&entry)?.len() + 1;
        ensure!(bytes < MAX_MANIFEST, "file manifest too large");
        if directory {
            let mut children = Vec::new();
            for child in fs::read_dir(&path)? {
                ensure!(
                    entries.len() + pending.len() + children.len() + 1 < MAX_FILES,
                    "too many files"
                );
                let child = child?;
                let name = child
                    .file_name()
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("invalid filename"))?;
                children.push((child.path(), format!("{relative_path}/{name}")));
            }
            children.sort_by(|a, b| b.1.cmp(&a.1));
            pending.extend(children);
        }
        entries.push((path, entry));
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_and_empty_directories_over_old_limit() {
        let root = std::env::temp_dir().join(new_id());
        fs::create_dir_all(root.join("nested/empty")).unwrap();
        for i in 0..300 {
            fs::write(root.join(format!("nested/{i}.txt")), b"x").unwrap();
        }
        let entries = manifest(vec![root.clone()]).unwrap();
        assert_eq!(entries.len(), 303);
        assert!(
            entries
                .iter()
                .any(|(_, f)| f.directory && f.relative_path.ends_with("/empty"))
        );
        assert_eq!(entries.iter().filter(|(_, f)| !f.directory).count(), 300);
        fs::remove_dir_all(root).unwrap();
    }
}
