use crate::core::{
    Content, FileEntry, ReadAt,
    engine::{Engine, new_id},
};
use anyhow::{Result, ensure};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct LocalFile {
    source: crate::core::LocalFile,
    background_state: (Vec<u16>, i64),
}
impl LocalFile {
    pub fn version(&self) -> String {
        self.source.version()
    }
}
impl ReadAt for LocalFile {
    fn read_background(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        let check = || -> Result<()> {
            let file = self
                .source
                .file
                .lock()
                .map_err(|_| anyhow::anyhow!("source lock poisoned"))?;
            ensure!(
                background_state(&file)? == self.background_state,
                "version_changed"
            );
            Ok(())
        };
        check()?;
        let n = self.read_at(offset, buffer)?;
        check()?;
        Ok(n)
    }
    fn transfer_guard(&self) -> Result<Option<File>> {
        let file = self
            .source
            .file
            .lock()
            .map_err(|_| anyhow::anyhow!("source lock poisoned"))?;
        Ok(Some(transfer_lock(&file)?))
    }
    fn len(&self) -> u64 {
        self.source.len()
    }
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        self.source.read_at(offset, buffer)
    }
}

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
    Ok(LocalFile {
        background_state: background_state(&file)?,
        source: crate::core::LocalFile::from_file(file)?,
    })
}
/// Reopen the same object, never its possibly replaced pathname.
fn transfer_lock(file: &std::fs::File) -> Result<std::fs::File> {
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
/// Path and NTFS change time detect rename/move/write without denying user access.
fn background_state(file: &std::fs::File) -> Result<(Vec<u16>, i64)> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            FILE_BASIC_INFO, FILE_NAME_NORMALIZED, FileBasicInfo, GetFileInformationByHandleEx,
            GetFinalPathNameByHandleW,
        },
    };
    let handle = HANDLE(file.as_raw_handle());
    let mut path = vec![0u16; 32768];
    let mut info = FILE_BASIC_INFO::default();
    unsafe {
        let n = GetFinalPathNameByHandleW(handle, &mut path, FILE_NAME_NORMALIZED) as usize;
        ensure!(n > 0 && n < path.len(), "source path unavailable");
        path.truncate(n);
        GetFileInformationByHandleEx(
            handle,
            FileBasicInfo,
            (&mut info as *mut FILE_BASIC_INFO).cast(),
            std::mem::size_of::<FILE_BASIC_INFO>() as u32,
        )?;
    }
    Ok((path, info.ChangeTime))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn background_tls_read_allows_rename_and_rejects_changed_source() {
        use crate::core::transport::{Identity, Node, Peer};
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
        let read = |offset| {
            b.local_ipc(crate::core::IpcOperation::ReadBinary {
                background: true,
                request_id: 123,
                offer_id: manifest.offer_id.clone(),
                file_id: file.file_id.clone(),
                version: file.version.clone(),
                offset,
                length: 4,
            })
        };
        assert!(read(0).is_ok());
        std::fs::rename(&path, &moved).unwrap();
        assert!(read(4).is_err());
        drop(b);
        std::fs::rename(&moved, &path).unwrap();
        a.stop.store(true, Ordering::Release);
        server.join().unwrap();
        drop(a);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
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
    fn background_read_rejects_same_size_write_and_move() {
        for rename in [false, true] {
            let dir = std::env::temp_dir().join(new_id());
            std::fs::create_dir(&dir).unwrap();
            let path = dir.join("source.bin");
            let moved = dir.join("moved.bin");
            std::fs::write(&path, b"original").unwrap();
            let source = open(&path).unwrap();
            assert_eq!(source.read_background(0, &mut [0; 4]).unwrap(), 4);
            if rename {
                std::fs::rename(&path, &moved).unwrap();
            } else {
                std::fs::write(&path, b"modified").unwrap();
                // Change detection uses metadata. Make the
                // mutation observable even on coarse filesystem clock ticks.
                let writer = OpenOptions::new().write(true).open(&path).unwrap();
                let modified = writer.metadata().unwrap().modified().unwrap();
                writer
                    .set_modified(modified + std::time::Duration::from_secs(1))
                    .unwrap();
            }
            assert!(source.read_background(4, &mut [0; 4]).is_err());
            drop(source);
            std::fs::remove_file(if rename { moved } else { path }).unwrap();
            std::fs::remove_dir(dir).unwrap();
        }
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
