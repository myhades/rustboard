use crate::core::{
    Content, FileEntry, LocalFile, ReadAt,
    engine::Engine,
    transport::{Node, RemoteSource},
};
use anyhow::{Result, ensure};
use fuser::*;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsStr,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

struct FilePath {
    path: PathBuf,
    identity: (u64, u64),
    version: String,
    size: u64,
}
impl ReadAt for FilePath {
    fn len(&self) -> u64 {
        self.size
    }
    fn open_reader(&self) -> Result<Option<Arc<dyn ReadAt>>> {
        use std::os::unix::fs::MetadataExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.path)?;
        let metadata = file.metadata()?;
        ensure!(
            (metadata.dev(), metadata.ino()) == self.identity,
            "version_changed"
        );
        let source = LocalFile::from_file(file)?;
        ensure!(source.version() == self.version, "version_changed");
        Ok(Some(Arc::new(source)))
    }
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        self.open_reader()?.unwrap().read_at(offset, buffer)
    }
}
pub fn offer(engine: &mut Engine, paths: Vec<PathBuf>, mount: &Path, cut: bool) -> Result<()> {
    let roots = paths.clone();
    use std::os::unix::fs::MetadataExt;
    let mut files = Vec::new();
    let mut sources: HashMap<String, Arc<dyn ReadAt>> = HashMap::new();
    for (path, entry) in crate::core::files::manifest(paths)? {
        let path = std::fs::canonicalize(path)?;
        ensure!(!path.starts_with(mount), "cannot recapture remote files");
        if !entry.directory {
            let metadata = std::fs::metadata(&path)?;
            sources.insert(
                entry.file_id.clone(),
                Arc::new(FilePath {
                    path,
                    identity: (metadata.dev(), metadata.ino()),
                    version: entry.version.clone(),
                    size: entry.size.unwrap(),
                }),
            );
        }
        files.push(entry);
    }
    engine.local_files(files, sources, roots, cut)?;
    Ok(())
}

pub(crate) fn recycle(paths: &[PathBuf]) -> bool {
    use std::{
        ffi::{CString, c_void},
        os::unix::ffi::OsStrExt,
    };
    #[link(name = "libgio-2.0.so.0", kind = "dylib", modifiers = "+verbatim")]
    unsafe extern "C" {
        fn g_file_new_for_path(path: *const std::ffi::c_char) -> *mut c_void;
        fn g_file_trash(file: *mut c_void, cancel: *mut c_void, error: *mut *mut c_void) -> i32;
    }
    #[link(name = "libgobject-2.0.so.0", kind = "dylib", modifiers = "+verbatim")]
    unsafe extern "C" {
        fn g_object_unref(object: *mut c_void);
    }
    let mut ok = true;
    for path in paths {
        let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
            ok = false;
            continue;
        };
        unsafe {
            let file = g_file_new_for_path(path.as_ptr());
            if file.is_null() {
                ok = false;
                continue;
            }
            ok &= g_file_trash(file, std::ptr::null_mut(), std::ptr::null_mut()) != 0;
            g_object_unref(file);
        }
    }
    ok
}

struct Entry {
    parent: u64,
    batch: String,
    file: Option<FileEntry>,
}
struct OpenFile {
    source: RemoteSource,
    _batch: Arc<()>,
}
pub struct Files {
    node: Arc<Node>,
    entries: HashMap<u64, Entry>,
    children: HashMap<u64, BTreeMap<String, u64>>,
    batches: HashSet<String>,
    directories: HashMap<u64, (String, Arc<crate::core::transport::BatchRead>)>,
    handles: HashMap<u64, OpenFile>,
    next: u64,
}
impl Files {
    fn remove_export(
        &mut self,
        parent: u64,
        name: &OsStr,
        directory: bool,
    ) -> std::result::Result<(), i32> {
        let name = name.to_str().ok_or(libc::EINVAL)?;
        let ino = *self
            .children
            .get(&parent)
            .and_then(|c| c.get(name))
            .ok_or(libc::ENOENT)?;
        let entry = self.entries.get(&ino).ok_or(libc::ENOENT)?;
        let file = entry.file.as_ref().ok_or(libc::EPERM)?;
        if file.directory != directory {
            return Err(libc::EINVAL);
        }
        if directory && self.children.get(&ino).is_some_and(|c| !c.is_empty()) {
            return Err(libc::ENOTEMPTY);
        }
        let _ = self.node.item_moved(&entry.batch, &file.file_id);
        self.entries.remove(&ino);
        self.children.remove(&ino);
        self.children.get_mut(&parent).unwrap().remove(name);
        Ok(())
    }
    pub fn new(node: Arc<Node>) -> Self {
        Self {
            node,
            entries: HashMap::new(),
            children: HashMap::new(),
            batches: HashSet::new(),
            directories: HashMap::new(),
            handles: HashMap::new(),
            next: 2,
        }
    }
    fn sync(&mut self) {
        let live: HashSet<_> = self
            .node
            .engine
            .lock()
            .unwrap()
            .retained_ids()
            .into_iter()
            .collect();
        if live == self.batches {
            return;
        }
        self.entries.retain(|_, e| live.contains(&e.batch));
        self.children
            .retain(|parent, _| *parent == 1 || self.entries.contains_key(parent));
        for children in self.children.values_mut() {
            children.retain(|_, ino| self.entries.contains_key(ino));
        }
        let added: Vec<_> = live.difference(&self.batches).cloned().collect();
        for id in added {
            let Ok(offer) = self.node.engine.lock().unwrap().file_offer(&id) else {
                continue;
            };
            if offer.source_device == self.node.identity.device() {
                continue;
            }
            let Content::Files { mut files } = offer.content else {
                continue;
            };
            let root = self.insert(1, id.clone(), id.clone(), None);
            let mut paths = HashMap::from([(String::new(), root)]);
            files.sort_by_key(|f| f.relative_path.matches('/').count());
            for file in files {
                let (parent, name) = file
                    .relative_path
                    .rsplit_once('/')
                    .unwrap_or(("", &file.relative_path));
                let Some(&parent) = paths.get(parent) else {
                    continue;
                };
                let path = file.relative_path.clone();
                let ino = self.insert(parent, name.into(), id.clone(), Some(file));
                paths.insert(path, ino);
            }
        }
        self.batches = live;
    }
    fn insert(&mut self, parent: u64, name: String, batch: String, file: Option<FileEntry>) -> u64 {
        let ino = self.next;
        self.next += 1;
        self.children
            .entry(parent)
            .or_default()
            .insert(name.clone(), ino);
        self.entries.insert(
            ino,
            Entry {
                parent,
                batch,
                file,
            },
        );
        ino
    }
    fn attr(&self, ino: u64) -> Option<FileAttr> {
        let size = if ino == 1 {
            None
        } else {
            self.entries
                .get(&ino)?
                .file
                .as_ref()
                .filter(|f| !f.directory)
                .map(|f| f.size.unwrap_or(0))
        };
        let metadata = self.entries.get(&ino).and_then(|e| e.file.as_ref());
        let modified = metadata
            .and_then(|f| f.modified_ms)
            .map(|ms| UNIX_EPOCH + Duration::from_millis(ms))
            .unwrap_or(UNIX_EPOCH);
        Some(FileAttr {
            ino,
            size: size.unwrap_or(0),
            blocks: 0,
            atime: UNIX_EPOCH,
            mtime: modified,
            ctime: UNIX_EPOCH,
            crtime: UNIX_EPOCH,
            kind: if size.is_some() {
                FileType::RegularFile
            } else {
                FileType::Directory
            },
            perm: if size.is_none() || metadata.is_some_and(|f| f.executable == Some(true)) {
                0o700
            } else {
                0o600
            },
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
    fn unlink(&mut self, _: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        match self.remove_export(parent, name, false) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }
    fn rmdir(&mut self, _: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        match self.remove_export(parent, name, true) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }
    fn lookup(&mut self, _: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        self.sync();
        if let Some(ino) = name
            .to_str()
            .and_then(|n| self.children.get(&parent)?.get(n))
        {
            reply.entry(&Duration::ZERO, &self.attr(*ino).unwrap(), 0);
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
    fn opendir(&mut self, _: &Request<'_>, ino: u64, _: i32, reply: ReplyOpen) {
        self.sync();
        if ino == 1 {
            reply.opened(0, 0);
            return;
        }
        let result = (|| -> Result<_> {
            ensure!(self.directories.len() < 128, "directory capacity reached");
            let entry = self
                .entries
                .get(&ino)
                .ok_or_else(|| anyhow::anyhow!("directory expired"))?;
            ensure!(
                entry.file.as_ref().is_none_or(|f| f.directory),
                "not a directory"
            );
            let batch = self
                .directories
                .values()
                .find(|(id, _)| id == &entry.batch)
                .map(|(_, batch)| batch.clone());
            Ok((
                entry.batch.clone(),
                match batch {
                    Some(batch) => batch,
                    None => Arc::new(self.node.begin_batch(&entry.batch)?),
                },
            ))
        })();
        match result {
            Ok(batch) => {
                let handle = self.next;
                self.next += 1;
                self.directories.insert(handle, batch);
                reply.opened(handle, 0);
            }
            Err(_) => reply.error(libc::EIO),
        }
    }
    fn releasedir(&mut self, _: &Request<'_>, _: u64, handle: u64, _: i32, reply: ReplyEmpty) {
        self.directories.remove(&handle);
        self.sync();
        reply.ok();
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
            (
                self.entries.get(&ino).map_or(1, |e| e.parent),
                FileType::Directory,
                "..".to_owned(),
            ),
        ];
        if let Some(children) = self.children.get(&ino) {
            entries.extend(children.iter().map(|(name, ino)| {
                let e = &self.entries[ino];
                (
                    *ino,
                    if e.file.as_ref().is_some_and(|f| !f.directory) {
                        FileType::RegularFile
                    } else {
                        FileType::Directory
                    },
                    name.clone(),
                )
            }));
        }
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
                .get(&ino)
                .ok_or_else(|| anyhow::anyhow!("file expired"))?;
            let file = e
                .file
                .clone()
                .ok_or_else(|| anyhow::anyhow!("not a file"))?;
            ensure!(!file.directory, "not a file");
            let (device, token) = {
                let mut engine = self.node.engine.lock().unwrap();
                (
                    engine.batch_device(&e.batch)?,
                    engine.retain_batch(&e.batch)?,
                )
            };
            let peer = self
                .node
                .peers
                .lock()
                .unwrap()
                .iter()
                .find(|p| p.device() == device)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("peer unavailable"))?;
            Ok(OpenFile {
                source: RemoteSource::new(self.node.clone(), peer, e.batch.clone(), file),
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
