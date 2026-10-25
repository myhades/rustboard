//! OLE virtual-file presentation implementation.
#![allow(non_snake_case)]
use crate::core::{MAX_CHUNK, ReadAt};
use crate::platform::PresentedFile;
#[cfg(test)]
use crate::test_pattern::PatternSource;
use std::{
    ffi::c_void,
    mem::{ManuallyDrop, size_of},
    sync::{Arc, Mutex},
};
use windows::{
    Win32::{
        Foundation::*,
        Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL},
        System::{Com::*, DataExchange::RegisterClipboardFormatW, Memory::*},
        UI::Shell::*,
    },
    core::{BOOL, HRESULT, Ref, Result, implement, w},
};

#[implement(IStream)]
struct ContentStream {
    source: Arc<dyn ReadAt>,
    position: Mutex<u64>,
}
fn stream(source: Arc<dyn ReadAt>, position: u64) -> IStream {
    ContentStream {
        source,
        position: Mutex::new(position),
    }
    .into()
}
impl ISequentialStream_Impl for ContentStream_Impl {
    fn Read(&self, pv: *mut c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
        unsafe {
            if !pcbread.is_null() {
                *pcbread = 0;
            }
        }
        if cb == 0 {
            return S_OK;
        }
        if pv.is_null() {
            return E_POINTER;
        }
        let Ok(mut position) = self.position.lock() else {
            return E_FAIL;
        };
        let mut total = 0usize;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while total < cb as usize && *position < self.source.len() {
            if std::time::Instant::now() >= deadline {
                return STG_E_READFAULT;
            }
            let count = (cb as usize - total).min(MAX_CHUNK);
            // COM caller guarantees writable cb bytes. Source never receives more than MAX_CHUNK.
            let buffer =
                unsafe { std::slice::from_raw_parts_mut(pv.cast::<u8>().add(total), count) };
            match self.source.read_at(*position, buffer) {
                Ok(0) => break,
                Ok(n) if n <= count => {
                    *position += n as u64;
                    total += n;
                }
                _ => {
                    unsafe {
                        if !pcbread.is_null() {
                            *pcbread = total as u32;
                        }
                    }
                    return STG_E_READFAULT;
                }
            }
        }
        unsafe {
            if !pcbread.is_null() {
                *pcbread = total as u32;
            }
        }
        if total == cb as usize { S_OK } else { S_FALSE }
    }
    fn Write(&self, _: *const c_void, _: u32, written: *mut u32) -> HRESULT {
        unsafe {
            if !written.is_null() {
                *written = 0;
            }
        }
        STG_E_ACCESSDENIED
    }
}
impl IStream_Impl for ContentStream_Impl {
    fn Seek(&self, delta: i64, origin: STREAM_SEEK, new: *mut u64) -> Result<()> {
        let mut position = self
            .position
            .lock()
            .map_err(|_| windows::core::Error::from(E_FAIL))?;
        let base = match origin {
            STREAM_SEEK_SET => 0,
            STREAM_SEEK_CUR => *position,
            STREAM_SEEK_END => self.source.len(),
            _ => return Err(STG_E_INVALIDFUNCTION.into()),
        };
        let value = (base as i128) + (delta as i128);
        if !(0..=u64::MAX as i128).contains(&value) {
            return Err(STG_E_INVALIDFUNCTION.into());
        }
        *position = value as u64;
        unsafe {
            if !new.is_null() {
                *new = *position;
            }
        }
        Ok(())
    }
    fn SetSize(&self, _: u64) -> Result<()> {
        Err(STG_E_ACCESSDENIED.into())
    }
    fn CopyTo(
        &self,
        target: Ref<'_, IStream>,
        count: u64,
        read: *mut u64,
        written: *mut u64,
    ) -> Result<()> {
        unsafe {
            if !read.is_null() {
                *read = 0;
            }
            if !written.is_null() {
                *written = 0;
            }
        }
        let target = target.ok()?;
        let mut buffer = [0u8; MAX_CHUNK];
        let mut total = 0u64;
        let mut output = 0u64;
        while total < count {
            let mut n = 0;
            let mut m = 0;
            let hr = self.Read(
                buffer.as_mut_ptr().cast(),
                (count - total).min(MAX_CHUNK as u64) as u32,
                &mut n,
            );
            hr.ok()?;
            if n == 0 {
                break;
            }
            total += n as u64;
            unsafe {
                target.Write(buffer.as_ptr().cast(), n, Some(&mut m)).ok()?;
            }
            output += m as u64;
            unsafe {
                if !read.is_null() {
                    *read = total;
                }
                if !written.is_null() {
                    *written = output;
                }
            }
            if m != n {
                return Err(STG_E_WRITEFAULT.into());
            }
            if hr == S_FALSE {
                break;
            }
        }
        Ok(())
    }
    fn Commit(&self, _: &STGC) -> Result<()> {
        Ok(())
    }
    fn Revert(&self) -> Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }
    fn LockRegion(&self, _: u64, _: u64, _: &LOCKTYPE) -> Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }
    fn UnlockRegion(&self, _: u64, _: u64, _: u32) -> Result<()> {
        Err(STG_E_INVALIDFUNCTION.into())
    }
    fn Stat(&self, stat: *mut STATSTG, _: &STATFLAG) -> Result<()> {
        if stat.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            *stat = STATSTG {
                r#type: STGTY_STREAM.0 as u32,
                cbSize: self.source.len(),
                grfMode: STGM_READ,
                ..Default::default()
            };
        }
        Ok(())
    }
    fn Clone(&self) -> Result<IStream> {
        Ok(stream(
            self.source.clone(),
            *self
                .position
                .lock()
                .map_err(|_| windows::core::Error::from(E_FAIL))?,
        ))
    }
}

#[implement(IDataObject)]
struct VirtualFiles {
    files: Vec<PresentedFile>,
    descriptor: u16,
    contents: u16,
    effect: u16,
    origin: u16,
    performed: u16,
    pasted: u16,
    cut: bool,
    move_batch: Option<(Arc<crate::core::transport::Node>, String)>,
    moved: std::sync::atomic::AtomicBool,
}
pub fn create(
    files: Vec<PresentedFile>,
    move_batch: Option<(Arc<crate::core::transport::Node>, String)>,
    cut: bool,
) -> anyhow::Result<IDataObject> {
    anyhow::ensure!(
        !files.is_empty() && files.len() <= crate::core::MAX_FILES,
        "invalid file count"
    );
    let mut names = std::collections::HashSet::new();
    for file in &files {
        crate::core::validate_path(&file.name)?;
        anyhow::ensure!(names.insert(&file.name), "duplicate filename");
    }
    unsafe {
        Ok(VirtualFiles {
            files,
            descriptor: RegisterClipboardFormatW(w!("FileGroupDescriptorW")) as u16,
            contents: RegisterClipboardFormatW(w!("FileContents")) as u16,
            effect: RegisterClipboardFormatW(w!("Preferred DropEffect")) as u16,
            origin: RegisterClipboardFormatW(w!("Rustboard.Origin.v1")) as u16,
            performed: RegisterClipboardFormatW(w!("Performed DropEffect")) as u16,
            pasted: RegisterClipboardFormatW(w!("Paste Succeeded")) as u16,
            cut,
            move_batch,
            moved: std::sync::atomic::AtomicBool::new(false),
        }
        .into())
    }
}
// Leave spelling changes to Shell, but reject path/device aliases even after
// its filename cleanup. The original spelling is returned unchanged.
fn descriptor_name(path: &str) -> Result<Vec<u16>> {
    for part in path.split('/') {
        if part.ends_with([' ', '.']) || part.chars().any(|c| c.is_control() || "\\:".contains(c)) {
            return Err(HRESULT::from_win32(ERROR_INVALID_NAME.0).into());
        }
        let cleaned: String = part.chars().filter(|c| !"<>\"|?*".contains(*c)).collect();
        let cleaned = cleaned.trim_end_matches([' ', '.']);
        let stem = cleaned
            .split('.')
            .next()
            .unwrap()
            .trim_end_matches(' ')
            .to_ascii_uppercase();
        let numbered_device = stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| suffix.chars().count() == 1 && "123456789¹²³".contains(suffix));
        if cleaned.is_empty()
            || ["CON", "PRN", "AUX", "NUL", "CLOCK$", "CONIN$", "CONOUT$"].contains(&stem.as_str())
            || numbered_device
        {
            return Err(HRESULT::from_win32(ERROR_INVALID_NAME.0).into());
        }
    }
    let name: Vec<_> = path.replace('/', r"\").encode_utf16().collect();
    if name.len() >= 260 {
        return Err(HRESULT::from_win32(ERROR_FILENAME_EXCED_RANGE.0).into());
    }
    Ok(name)
}
pub fn format(id: u16, index: i32, medium: TYMED) -> FORMATETC {
    FORMATETC {
        cfFormat: id,
        dwAspect: DVASPECT_CONTENT.0,
        lindex: index,
        tymed: medium.0 as u32,
        ..Default::default()
    }
}

/// Ownership of the HGLOBAL passes to the receiving STGMEDIUM/clipboard owner.
pub fn global_bytes(bytes: &[u8]) -> Result<HGLOBAL> {
    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len())?;
        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            let _ = GlobalFree(Some(handle));
            return Err(E_OUTOFMEMORY.into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast(), bytes.len());
        let _ = GlobalUnlock(handle);
        Ok(handle)
    }
}
impl VirtualFiles_Impl {
    fn formats(&self) -> [FORMATETC; 4] {
        [
            format(self.descriptor, -1, TYMED_HGLOBAL),
            format(self.contents, -1, TYMED_ISTREAM),
            format(self.effect, -1, TYMED_HGLOBAL),
            format(self.origin, -1, TYMED_HGLOBAL),
        ]
    }
    fn validate(&self, f: &FORMATETC) -> HRESULT {
        if f.dwAspect != DVASPECT_CONTENT.0 {
            return DV_E_DVASPECT;
        }
        if !f.ptd.is_null() {
            return DV_E_DVTARGETDEVICE;
        }
        if f.cfFormat == self.contents {
            if f.tymed & TYMED_ISTREAM.0 as u32 == 0 {
                return DV_E_TYMED;
            }
            if f.lindex < 0 || f.lindex as usize >= self.files.len() {
                return DV_E_LINDEX;
            }
        } else if [self.descriptor, self.effect, self.origin].contains(&f.cfFormat) {
            if f.tymed & TYMED_HGLOBAL.0 as u32 == 0 {
                return DV_E_TYMED;
            }
            if f.lindex != -1 {
                return DV_E_LINDEX;
            }
        } else {
            return DV_E_FORMATETC;
        }
        S_OK
    }
}
impl IDataObject_Impl for VirtualFiles_Impl {
    fn GetData(&self, input: *const FORMATETC) -> Result<STGMEDIUM> {
        if input.is_null() {
            return Err(E_POINTER.into());
        }
        let f = unsafe { &*input };
        self.validate(f).ok()?;
        if f.cfFormat == self.contents {
            if self.files[f.lindex as usize].directory {
                return Err(DV_E_FORMATETC.into());
            }
            return Ok(STGMEDIUM {
                tymed: TYMED_ISTREAM.0 as u32,
                u: STGMEDIUM_0 {
                    pstm: ManuallyDrop::new(Some(stream(
                        self.files[f.lindex as usize].source.clone(),
                        0,
                    ))),
                },
                ..Default::default()
            });
        }
        let bytes = if f.cfFormat == self.descriptor {
            let mut bytes = vec![0u8; 4 + size_of::<FILEDESCRIPTORW>() * self.files.len()];
            bytes[..4].copy_from_slice(&(self.files.len() as u32).to_le_bytes());
            for (i, file) in self.files.iter().enumerate() {
                let name = descriptor_name(&file.name).inspect_err(|error| {
                    log::warn!(target: "clipboard", "file name cannot be represented by Windows: {error}");
                })?;
                let mut fd = FILEDESCRIPTORW {
                    dwFlags: (if file.directory {
                        0
                    } else {
                        FD_FILESIZE.0 as u32
                    }) | FD_ATTRIBUTES.0 as u32
                        | FD_PROGRESSUI.0 as u32,
                    dwFileAttributes: if file.directory {
                        FILE_ATTRIBUTE_DIRECTORY.0
                    } else {
                        FILE_ATTRIBUTE_NORMAL.0
                    },
                    nFileSizeHigh: (file.source.len() >> 32) as u32,
                    nFileSizeLow: file.source.len() as u32,
                    ..Default::default()
                };
                if let Some(ms) = file.modified_ms {
                    let ticks = ms
                        .saturating_mul(10_000)
                        .saturating_add(116_444_736_000_000_000);
                    fd.dwFlags |= FD_WRITESTIME.0 as u32;
                    fd.ftLastWriteTime = FILETIME {
                        dwLowDateTime: ticks as u32,
                        dwHighDateTime: (ticks >> 32) as u32,
                    };
                }
                let mut filename = [0u16; 260];
                filename[..name.len()].copy_from_slice(&name);
                fd.cFileName = filename;
                unsafe {
                    std::ptr::write_unaligned(
                        bytes
                            .as_mut_ptr()
                            .add(4 + i * size_of::<FILEDESCRIPTORW>())
                            .cast::<FILEDESCRIPTORW>(),
                        fd,
                    );
                }
            }
            bytes
        } else if f.cfFormat == self.effect {
            (if self.cut { 2u32 } else { 1u32 }).to_le_bytes().to_vec()
        } else {
            b"rustboard-v1\0".to_vec()
        };
        Ok(STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 {
                hGlobal: global_bytes(&bytes)?,
            },
            ..Default::default()
        })
    }
    fn GetDataHere(&self, _: *const FORMATETC, _: *mut STGMEDIUM) -> Result<()> {
        Err(DV_E_FORMATETC.into())
    }
    fn QueryGetData(&self, f: *const FORMATETC) -> HRESULT {
        if f.is_null() {
            E_POINTER
        } else {
            self.validate(unsafe { &*f })
        }
    }
    fn GetCanonicalFormatEtc(&self, _: *const FORMATETC, out: *mut FORMATETC) -> HRESULT {
        if !out.is_null() {
            unsafe {
                (*out).ptd = std::ptr::null_mut();
            }
        }
        E_NOTIMPL
    }
    fn SetData(
        &self,
        format: *const FORMATETC,
        medium: *const STGMEDIUM,
        release: BOOL,
    ) -> Result<()> {
        if format.is_null() || medium.is_null() {
            return Err(E_POINTER.into());
        }
        let (format, medium) = unsafe { (&*format, &*medium) };
        if ![self.performed, self.pasted].contains(&format.cfFormat)
            || medium.tymed != TYMED_HGLOBAL.0 as u32
        {
            return Err(DV_E_FORMATETC.into());
        }
        let effect = unsafe {
            let memory = medium.u.hGlobal;
            if GlobalSize(memory) < 4 {
                return Err(DV_E_FORMATETC.into());
            }
            let pointer = GlobalLock(memory);
            if pointer.is_null() {
                return Err(E_FAIL.into());
            }
            let value = std::ptr::read_unaligned(pointer.cast::<u32>());
            let _ = GlobalUnlock(memory);
            value
        };
        if effect == 2
            && let Some((node, id)) = &self.move_batch
            && !self.moved.swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            let (node, id) = (node.clone(), id.clone());
            let sources: Vec<_> = self.files.iter().map(|f| f.source.clone()).collect();
            std::thread::spawn(move || {
                let _ = node.recycle_remote(&id);
                drop(sources);
            });
        }
        if release.as_bool() {
            unsafe {
                let mut owned = std::ptr::read(medium);
                windows::Win32::System::Ole::ReleaseStgMedium(&mut owned);
            }
        }
        Ok(())
    }
    fn EnumFormatEtc(&self, direction: u32) -> Result<IEnumFORMATETC> {
        if direction != DATADIR_GET.0 as u32 {
            return Err(E_NOTIMPL.into());
        }
        unsafe { SHCreateStdEnumFmtEtc(&self.formats()) }
    }
    fn DAdvise(&self, _: *const FORMATETC, _: u32, _: Ref<'_, IAdviseSink>) -> Result<u32> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
    fn DUnadvise(&self, _: u32) -> Result<()> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    #[test]
    fn unsafe_windows_names_fail_when_ole_requests_descriptors() {
        let source = Arc::new(PatternSource::new(16));
        for name in [
            "a:b",
            "a\\b",
            "CON.txt",
            "COM¹.txt",
            "name.",
            "name ",
            "name:stream",
            "C?ON.txt",
            "CON .txt",
            "LPT*1.txt",
            "..? ",
            "?*",
            " . . ",
            "folder/..?. /file.txt",
        ] {
            let object = create(
                vec![PresentedFile {
                    name: name.into(),
                    directory: false,
                    modified_ms: None,
                    source: source.clone(),
                }],
                None,
                false,
            )
            .unwrap();
            let request = format(
                unsafe { RegisterClipboardFormatW(w!("FileGroupDescriptorW")) } as u16,
                -1,
                TYMED_HGLOBAL,
            );
            unsafe {
                object.QueryGetData(&request).ok().unwrap();
                assert_eq!(
                    object.GetData(&request).err().unwrap().code(),
                    HRESULT::from_win32(ERROR_INVALID_NAME.0)
                );
            }
        }
        assert_eq!(
            descriptor_name(&"a".repeat(260)).err().unwrap().code(),
            HRESULT::from_win32(ERROR_FILENAME_EXCED_RANGE.0)
        );
        assert_eq!(source.bytes_read.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn shell_receives_original_filename_spelling() {
        for name in ["a?.txt", "a*b.txt", "a<\"b>|.txt", "folder?/a.txt"] {
            assert_eq!(
                String::from_utf16(&descriptor_name(name).unwrap()).unwrap(),
                name.replace('/', r"\")
            );
        }
    }
    #[test]
    fn shell_move_finishes_the_batch_before_recycling_sources() {
        use crate::core::transport::{Identity, Node, Peer, RemoteSource};
        use windows::Win32::System::Ole::*;
        use windows::core::PCWSTR;
        let root = std::env::temp_dir().join(crate::core::engine::new_id());
        let tree = root.join("source-tree");
        let extra = root.join("source-note.txt");
        let destination = root.join("destination");
        std::fs::create_dir_all(tree.join("empty")).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(tree.join("file.txt"), b"nested file").unwrap();
        std::fs::write(&extra, b"second root").unwrap();
        let source = Node::new(Identity::generate().unwrap(), vec![]);
        let target = Node::new(Identity::generate().unwrap(), vec![]);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let peer = Peer {
            address: listener.local_addr().unwrap(),
            fingerprint: source.identity.pin(),
        };
        source.peers.lock().unwrap().push(Peer {
            address: "127.0.0.1:1".parse().unwrap(),
            fingerprint: target.identity.pin(),
        });
        target.peers.lock().unwrap().push(peer.clone());
        super::super::local_files::offer(
            &mut source.engine.lock().unwrap(),
            vec![tree.clone(), extra.clone()],
            true,
        )
        .unwrap();
        let offer = source.engine.lock().unwrap().current.clone().unwrap();
        let server = source
            .serve(listener, |paths| {
                super::super::recycle::recycle(paths).is_ok()
            })
            .unwrap();
        crate::test_network::sync(&source, &target);
        let sources = RemoteSource::batch(target.clone(), peer, offer.clone());
        let object = create(
            offer
                .content
                .entries()
                .iter()
                .zip(sources)
                .map(|(file, source)| PresentedFile {
                    name: file.relative_path.clone(),
                    directory: file.directory,
                    modified_ms: file.modified_ms,
                    source,
                })
                .collect(),
            Some((target.clone(), offer.offer_id.clone())),
            true,
        )
        .unwrap();
        assert!(target.recycle_remote(&offer.offer_id).is_err());
        assert!(tree.exists() && extra.exists());
        unsafe {
            OleInitialize(None).unwrap();
            let path: Vec<_> = destination
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let item: IShellItem =
                SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None).unwrap();
            let drop_target: IDropTarget = item.BindToHandler(None, &BHID_SFUIObject).unwrap();
            let mut effect = DROPEFFECT_MOVE;
            drop_target
                .DragEnter(&object, Default::default(), POINTL::default(), &mut effect)
                .unwrap();
            drop_target
                .Drop(&object, Default::default(), POINTL::default(), &mut effect)
                .unwrap();
            assert_eq!(effect, DROPEFFECT_MOVE);
            drop(drop_target);
            drop(item);
            drop(object);
            OleUninitialize();
        }
        for _ in 0..300 {
            if !tree.exists() && !extra.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(destination.join("source-tree/empty").is_dir());
        assert_eq!(
            std::fs::read(destination.join("source-tree/file.txt")).unwrap(),
            b"nested file"
        );
        assert_eq!(
            std::fs::read(destination.join("source-note.txt")).unwrap(),
            b"second root"
        );
        assert!(
            !tree.exists() && !extra.exists(),
            "Shell did not complete the move"
        );
        source.shutdown();
        target.shutdown();
        server.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn shell_copies_nested_and_empty_virtual_directories() {
        use windows::Win32::System::Ole::*;
        use windows::core::PCWSTR;
        let root = std::env::temp_dir().join(crate::core::engine::new_id());
        std::fs::create_dir(&root).unwrap();
        let source = Arc::new(PatternSource::new(31));
        let mut files = vec![
            PresentedFile {
                name: "tree".into(),
                directory: true,
                modified_ms: None,
                source: source.clone(),
            },
            PresentedFile {
                name: "tree/empty".into(),
                directory: true,
                modified_ms: None,
                source: source.clone(),
            },
            PresentedFile {
                name: "tree/nested".into(),
                directory: true,
                modified_ms: None,
                source: source.clone(),
            },
            PresentedFile {
                name: "tree/nested/文件.bin".into(),
                directory: false,
                modified_ms: Some(1700000000000),
                source: source.clone(),
            },
        ];
        for name in [
            "tree/question?.txt",
            "tree/case.txt",
            "tree/CASE.txt",
            "tree/star*.txt",
            "tree/angle<>.txt",
            "tree/quote\".txt",
            "tree/pipe|.txt",
        ] {
            files.push(PresentedFile {
                name: name.into(),
                directory: false,
                modified_ms: None,
                source: source.clone(),
            });
        }
        let object = create(files, None, false).unwrap();
        assert_eq!(
            source.bytes_read.load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        unsafe {
            OleInitialize(None).unwrap();
            let path: Vec<u16> = root
                .as_os_str()
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let destination: IShellItem =
                SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None).unwrap();
            let target: IDropTarget = destination.BindToHandler(None, &BHID_SFUIObject).unwrap();
            let mut effect = DROPEFFECT_COPY;
            target
                .DragEnter(&object, Default::default(), POINTL::default(), &mut effect)
                .unwrap();
            assert_eq!(effect, DROPEFFECT_COPY);
            target
                .Drop(&object, Default::default(), POINTL::default(), &mut effect)
                .unwrap();
            assert_eq!(effect, DROPEFFECT_COPY);
            drop(target);
            drop(destination);
            drop(object);
            OleUninitialize();
        }
        assert!(root.join("tree/empty").is_dir());
        assert_eq!(
            std::fs::read(root.join("tree/nested/文件.bin")).unwrap(),
            (0..31).collect::<Vec<u8>>()
        );
        let modified = std::fs::metadata(root.join("tree/nested/文件.bin"))
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        assert_eq!(modified, 1700000000000);
        let copied: Vec<_> = std::fs::read_dir(root.join("tree"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_file())
            .collect();
        assert_eq!(copied.len(), 7);
        for path in copied {
            assert_eq!(std::fs::read(path).unwrap(), (0..31).collect::<Vec<u8>>());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn com_batch_over_tls_survives_new_file_and_text_clipboards() {
        use crate::core::{
            Content, FileEntry,
            engine::new_id,
            transport::{Identity, Node, Peer, RemoteSource},
        };
        let source_node = Node::new(Identity::generate().unwrap(), vec![]);
        let target_node = Node::new(Identity::generate().unwrap(), vec![]);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let peer = Peer {
            address: listener.local_addr().unwrap(),
            fingerprint: source_node.identity.pin(),
        };
        source_node.peers.lock().unwrap().push(Peer {
            address: "127.0.0.1:1".parse().unwrap(),
            fingerprint: target_node.identity.pin(),
        });
        target_node.peers.lock().unwrap().push(peer.clone());
        let size = 5 * 1024 * 1024 + 17;
        let source = Arc::new(PatternSource::new(size));
        let files: Vec<_> = (0..2)
            .map(|i| FileEntry {
                directory: false,
                modified_ms: None,
                executable: None,
                file_id: new_id(),
                relative_path: format!("file-{i}.bin"),
                size: Some(size),
                version: "v".into(),
                media_type: "application/octet-stream".into(),
            })
            .collect();
        let manifest = source_node
            .engine
            .lock()
            .unwrap()
            .local(
                Content::Files {
                    files: files.clone(),
                },
                files
                    .iter()
                    .map(|f| (f.file_id.clone(), source.clone() as Arc<dyn ReadAt>))
                    .collect(),
            )
            .unwrap();
        let server = source_node.serve(listener, |_| false).unwrap();
        crate::test_network::sync(&source_node, &target_node);
        let object = create(
            files
                .iter()
                .map(|f| PresentedFile {
                    directory: false,
                    modified_ms: None,
                    name: f.relative_path.clone(),
                    source: Arc::new(RemoteSource::new(
                        target_node.clone(),
                        peer.clone(),
                        manifest.offer_id.clone(),
                        f.clone(),
                    )),
                })
                .collect(),
            None,
            false,
        )
        .unwrap();
        assert_eq!(source.bytes_read.load(Ordering::Relaxed), 0);
        unsafe {
            let mut buffer = vec![0; 65536];
            for index in 0..2 {
                let mut medium = object
                    .GetData(&format(
                        RegisterClipboardFormatW(w!("FileContents")) as u16,
                        index,
                        TYMED_ISTREAM,
                    ))
                    .unwrap();
                let content = medium.u.pstm.as_ref().unwrap();
                let mut offset = 0;
                while offset < size {
                    let count = (size - offset).min(buffer.len() as u64) as u32;
                    let mut actual = 0;
                    content
                        .Read(buffer.as_mut_ptr().cast(), count, Some(&mut actual))
                        .ok()
                        .unwrap();
                    assert_eq!(actual, count);
                    assert!(
                        buffer[..actual as usize]
                            .iter()
                            .enumerate()
                            .all(|(i, b)| *b == ((offset + i as u64) % 251) as u8)
                    );
                    offset += actual as u64;
                    if index == 0 && offset == 65536 {
                        // New file batch, then new text at the receiver. The old
                        // second file's IStream has not even been requested yet.
                        source_node
                            .engine
                            .lock()
                            .unwrap()
                            .local(
                                Content::Files {
                                    files: files.clone(),
                                },
                                files
                                    .iter()
                                    .map(|f| (f.file_id.clone(), source.clone() as Arc<dyn ReadAt>))
                                    .collect(),
                            )
                            .unwrap();
                        crate::test_network::sync(&source_node, &target_node);
                        target_node
                            .engine
                            .lock()
                            .unwrap()
                            .local(
                                Content::Text {
                                    text: "replacement".into(),
                                    html: None,
                                    rtf: None,
                                },
                                Default::default(),
                            )
                            .unwrap();
                    }
                }
                windows::Win32::System::Ole::ReleaseStgMedium(&mut medium);
            }
        }
        source_node.shutdown();
        target_node.shutdown();
        server.join().unwrap();
    }
    #[test]
    fn com_stream_seek_clone_and_eof() {
        let source = Arc::new(crate::test_pattern::PatternSource::new(4096));
        let s = stream(source.clone(), 0);
        assert_eq!(source.bytes_read.load(Ordering::Relaxed), 0);
        unsafe {
            s.Seek(4090, STREAM_SEEK_SET, None).unwrap();
            let clone = s.Clone().unwrap();
            let mut buffer = [0u8; 20];
            let mut n = 0;
            assert_eq!(
                s.Read(buffer.as_mut_ptr().cast(), 20, Some(&mut n)),
                S_FALSE
            );
            assert_eq!(n, 6);
            assert_eq!(buffer[0], (4090 % 251) as u8);
            assert_eq!(
                clone.Read(buffer.as_mut_ptr().cast(), 6, Some(&mut n)),
                S_OK
            );
            assert!(s.Seek(-1, STREAM_SEEK_SET, None).is_err());
        }
    }
    #[test]
    fn descriptor_does_not_read_content_and_validates_index() {
        let source = Arc::new(crate::test_pattern::PatternSource::new(1 << 30));
        let object = create(
            vec![PresentedFile {
                directory: false,
                modified_ms: None,
                name: "test.bin".into(),
                source: source.clone(),
            }],
            None,
            false,
        )
        .unwrap();
        unsafe {
            let mut medium = object
                .GetData(&format(
                    RegisterClipboardFormatW(w!("FileGroupDescriptorW")) as u16,
                    -1,
                    TYMED_HGLOBAL,
                ))
                .unwrap();
            windows::Win32::System::Ole::ReleaseStgMedium(&mut medium);
            assert_eq!(source.bytes_read.load(Ordering::Relaxed), 0);
            assert!(
                object
                    .QueryGetData(&format(
                        RegisterClipboardFormatW(w!("FileContents")) as u16,
                        1,
                        TYMED_ISTREAM
                    ))
                    .is_err()
            );
        }
    }
}
