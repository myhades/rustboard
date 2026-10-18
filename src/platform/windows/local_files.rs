use crate::core::{
    Content, FileEntry, LocalFile, ReadAt,
    engine::{Engine, new_id},
};
use anyhow::{Result, ensure};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Publication does not lock user operations. Refuse reparse ancestors.
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
        .share_mode(1 | 2 | 4) // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        .custom_flags(0x00200000)
        .open(path)?;
    ensure!(
        file.metadata()?.file_attributes() & 0x400 == 0,
        "reparse file rejected"
    );
    LocalFile::from_file(file)
}
/// Reopen the same object, never its possibly replaced pathname.
pub fn transfer_lock(file: &std::fs::File) -> Result<std::fs::File> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, ReOpenFile},
    };
    let handle = unsafe {
        ReOpenFile(
            HANDLE(file.as_raw_handle()),
            0x80000000,
            FILE_SHARE_READ,
            FILE_FLAGS_AND_ATTRIBUTES(0),
        )?
    };
    Ok(unsafe { std::fs::File::from_raw_handle(handle.0) })
}
pub fn offer(engine: &mut Engine, paths: Vec<PathBuf>) -> Result<()> {
    ensure!(
        !paths.is_empty() && paths.len() <= crate::core::MAX_FILES,
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
    fn tls_first_read_locks_source_and_eof_releases_it() {
        use crate::core::transport::{Identity, Node, Peer, RemoteSource};
        use std::{net::TcpListener, sync::atomic::Ordering};
        let dir = std::env::temp_dir().join(new_id());
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("source.bin");
        let moved = dir.join("moved.bin");
        std::fs::write(&path, b"abcdefgh").unwrap();
        let a = Node::new(Identity::generate().unwrap(), vec![]);
        let b = Node::new(Identity::generate().unwrap(), vec![]);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let peer = Peer {
            address: listener.local_addr().unwrap(),
            fingerprint: a.identity.pin(),
        };
        a.peers.lock().unwrap().push(Peer {
            address: peer.address,
            fingerprint: b.identity.pin(),
        });
        b.peers.lock().unwrap().push(peer.clone());
        offer(&mut a.engine.lock().unwrap(), vec![path.clone()]).unwrap();
        let manifest = a.engine.lock().unwrap().current.clone().unwrap();
        let Content::Files { files } = &manifest.content else {
            panic!()
        };
        let file = files[0].clone();
        let server = a.serve(listener).unwrap();
        b.sync_once(&peer).unwrap();
        std::fs::rename(&path, &moved).unwrap();
        let source = RemoteSource::new(b, peer, manifest, file);
        let mut bytes = [0; 4];
        source.read_at(0, &mut bytes).unwrap();
        assert_eq!(&bytes, b"abcd");
        assert!(std::fs::rename(&moved, &path).is_err());
        assert!(std::fs::write(&moved, b"changed").is_err());
        assert!(std::fs::remove_file(&moved).is_err());
        source.read_at(4, &mut bytes).unwrap();
        assert_eq!(&bytes, b"efgh");
        std::fs::rename(&moved, &path).unwrap();
        drop(source);
        a.stop.store(true, Ordering::Release);
        server.join().unwrap();
        drop(a);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn offered_file_allows_move_and_delete_without_redirecting_reads() {
        let dir = std::env::temp_dir().join(format!("rustboard-{}", new_id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("source.bin");
        let moved = dir.join("moved.bin");
        std::fs::write(&path, b"original").unwrap();
        let source = open(&path).unwrap();
        // Merely advertising the file must not block a writer.
        drop(OpenOptions::new().write(true).open(&path).unwrap());
        let guard = source.transfer_guard().unwrap();
        assert!(std::fs::write(&path, b"mutate").is_err());
        assert!(std::fs::rename(&path, &moved).is_err());
        assert!(std::fs::remove_file(&path).is_err());
        drop(guard);
        std::fs::rename(&path, &moved).unwrap();
        std::fs::write(&path, b"replacement").unwrap();
        let mut bytes = [0; 8];
        assert_eq!(source.read_at(0, &mut bytes).unwrap(), 8);
        assert_eq!(&bytes, b"original");
        std::fs::remove_file(&moved).unwrap();
        assert_eq!(source.read_at(0, &mut bytes).unwrap(), 8);
        assert_eq!(&bytes, b"original");
        drop(source);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
