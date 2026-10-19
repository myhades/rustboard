use crate::core::{
    Content, FileEntry, LocalFile, ReadAt,
    engine::{Engine, new_id},
    transport::{Node, RemoteSource},
};
use anyhow::{Result, ensure};
use fuser::*;
use std::{
    collections::HashMap,
    ffi::OsStr,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

pub fn offer(engine: &mut Engine, paths: Vec<PathBuf>, mount: &Path) -> Result<()> {
    ensure!(
        !paths.is_empty() && paths.len() <= 256,
        "invalid file count"
    );
    let mut files = Vec::new();
    let mut sources: HashMap<String, Arc<dyn ReadAt>> = HashMap::new();
    for path in paths {
        let path = std::fs::canonicalize(path)?;
        ensure!(!path.starts_with(mount), "cannot recapture remote files");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)?;
        let source = Arc::new(LocalFile::from_file(file)?);
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid filename"))?
            .to_string();
        let id = new_id();
        files.push(FileEntry {
            file_id: id.clone(),
            relative_path: name,
            size: Some(source.len()),
            version: source.version(),
            media_type: "application/octet-stream".into(),
        });
        sources.insert(id, source);
    }
    engine.local(Content::Files { files }, sources)?;
    Ok(())
}

struct Entry {
    ino: u64,
    parent: u64,
    name: String,
    batch: String,
    file: Option<FileEntry>,
}
struct OpenFile {
    source: RemoteSource,
    _batch: Arc<()>,
}
pub struct Files {
    node: Arc<Node>,
    entries: HashMap<String, Entry>,
    handles: HashMap<u64, OpenFile>,
    next: u64,
}
impl Files {
    pub fn new(node: Arc<Node>) -> Self {
        Self {
            node,
            entries: HashMap::new(),
            handles: HashMap::new(),
            next: 2,
        }
    }
    fn sync(&mut self) {
        let offers = self.node.engine.lock().unwrap().retained_offers();
        self.entries
            .retain(|_, e| offers.iter().any(|o| o.offer_id == e.batch));
        for offer in offers {
            if offer.source_device == self.node.identity.device() {
                continue;
            }
            let Content::Files { files } = offer.content else {
                continue;
            };
            for file in files {
                let path = format!("{}/{}", offer.offer_id, file.relative_path);
                let parts: Vec<_> = path.split('/').collect();
                let mut parent = 1;
                for index in 0..parts.len() {
                    let key = parts[..=index].join("/");
                    let entry = self.entries.entry(key).or_insert_with(|| {
                        let ino = self.next;
                        self.next += 1;
                        Entry {
                            ino,
                            parent,
                            name: parts[index].into(),
                            batch: offer.offer_id.clone(),
                            file: if index + 1 == parts.len() {
                                Some(file.clone())
                            } else {
                                None
                            },
                        }
                    });
                    parent = entry.ino;
                }
            }
        }
    }
    fn attr(&self, ino: u64) -> Option<FileAttr> {
        let size = if ino == 1 {
            None
        } else {
            self.entries
                .values()
                .find(|e| e.ino == ino)?
                .file
                .as_ref()
                .map(|f| f.size.unwrap_or(0))
        };
        Some(FileAttr {
            ino,
            size: size.unwrap_or(0),
            blocks: 0,
            atime: UNIX_EPOCH,
            mtime: UNIX_EPOCH,
            ctime: UNIX_EPOCH,
            crtime: UNIX_EPOCH,
            kind: if size.is_some() {
                FileType::RegularFile
            } else {
                FileType::Directory
            },
            perm: if size.is_some() { 0o400 } else { 0o500 },
            nlink: 1,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            blksize: 65536,
            flags: 0,
        })
    }
}
impl Filesystem for Files {
    fn lookup(&mut self, _: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        self.sync();
        if let Some(e) = self
            .entries
            .values()
            .find(|e| e.parent == parent && OsStr::new(&e.name) == name)
        {
            reply.entry(&Duration::ZERO, &self.attr(e.ino).unwrap(), 0);
        } else {
            reply.error(libc::ENOENT);
        }
    }
    fn getattr(&mut self, _: &Request<'_>, ino: u64, _: Option<u64>, reply: ReplyAttr) {
        self.sync();
        if let Some(attr) = self.attr(ino) {
            reply.attr(&Duration::ZERO, &attr);
        } else {
            reply.error(libc::ENOENT);
        }
    }
    fn readdir(
        &mut self,
        _: &Request<'_>,
        ino: u64,
        _: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        self.sync();
        let mut entries = vec![
            (ino, FileType::Directory, ".".to_owned()),
            (1, FileType::Directory, "..".to_owned()),
        ];
        let mut children: Vec<_> = self.entries.values().filter(|e| e.parent == ino).collect();
        children.sort_by_key(|e| &e.name);
        entries.extend(children.into_iter().map(|e| {
            (
                e.ino,
                if e.file.is_some() {
                    FileType::RegularFile
                } else {
                    FileType::Directory
                },
                e.name.clone(),
            )
        }));
        for (i, (id, kind, name)) in entries.into_iter().enumerate().skip(offset.max(0) as usize) {
            if reply.add(id, (i + 1) as i64, kind, name) {
                break;
            }
        }
        reply.ok();
    }
    fn open(&mut self, _: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        if flags & libc::O_ACCMODE != libc::O_RDONLY {
            reply.error(libc::EROFS);
            return;
        }
        self.sync();
        let result = (|| -> Result<OpenFile> {
            ensure!(self.handles.len() < 8, "read capacity reached");
            let e = self
                .entries
                .values()
                .find(|e| e.ino == ino)
                .ok_or_else(|| anyhow::anyhow!("file expired"))?;
            let file = e
                .file
                .clone()
                .ok_or_else(|| anyhow::anyhow!("not a file"))?;
            let (offer, token) = {
                let mut engine = self.node.engine.lock().unwrap();
                (engine.file_offer(&e.batch)?, engine.retain_batch(&e.batch)?)
            };
            let peer = self
                .node
                .peers
                .lock()
                .unwrap()
                .iter()
                .find(|p| p.device() == offer.source_device)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("peer unavailable"))?;
            Ok(OpenFile {
                source: RemoteSource::new(self.node.clone(), peer, offer, file),
                _batch: token,
            })
        })();
        match result {
            Ok(file) => {
                let fh = self.next;
                self.next += 1;
                self.handles.insert(fh, file);
                reply.opened(fh, 1);
            }
            Err(_) => reply.error(libc::EIO),
        }
    }
    fn read(
        &mut self,
        _: &Request<'_>,
        _: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _: i32,
        _: Option<u64>,
        reply: ReplyData,
    ) {
        let Some(file) = self.handles.get(&fh) else {
            reply.error(libc::EBADF);
            return;
        };
        if offset < 0 {
            reply.error(libc::EINVAL);
            return;
        }
        let remaining = file.source.len().saturating_sub(offset as u64);
        let length = u64::from(size).min(remaining).min(1024 * 1024) as usize;
        let mut bytes = vec![0; length];
        let mut total = 0;
        while total < bytes.len() {
            let end = (total + crate::core::MAX_CHUNK).min(bytes.len());
            match file
                .source
                .read_at(offset as u64 + total as u64, &mut bytes[total..end])
            {
                Ok(0) => break,
                Ok(n) => total += n,
                Err(error) => {
                    log::warn!(target: "files", "read failed: {error:#}");
                    reply.error(libc::EIO);
                    return;
                }
            }
        }
        reply.data(&bytes[..total]);
    }
    fn release(
        &mut self,
        _: &Request<'_>,
        _: u64,
        fh: u64,
        _: i32,
        _: Option<u64>,
        _: bool,
        reply: ReplyEmpty,
    ) {
        self.handles.remove(&fh);
        self.sync();
        reply.ok();
    }
}
