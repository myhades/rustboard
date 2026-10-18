//! OLE virtual-file presentation implementation.
#![allow(non_snake_case)]
use crate::PresentedFile;
#[cfg(test)]
use crate::test_pattern::PatternSource;
use rustboard_core::{MAX_CHUNK, ReadAt};
use std::{
    ffi::c_void,
    mem::{ManuallyDrop, size_of},
    sync::{Arc, Mutex},
};
use windows::{
    Win32::{
        Foundation::*,
        Storage::FileSystem::FILE_ATTRIBUTE_NORMAL,
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
}
pub fn create(files: Vec<PresentedFile>) -> anyhow::Result<IDataObject> {
    anyhow::ensure!(
        !files.is_empty() && files.len() <= rustboard_core::MAX_FILES,
        "invalid file count"
    );
    let mut names = std::collections::HashSet::new();
    for file in &files {
        rustboard_core::validate_path(&file.name)?;
        anyhow::ensure!(
            !file.name.contains('/') && file.name.encode_utf16().count() < 260,
            "only flat files currently supported"
        );
        anyhow::ensure!(names.insert(file.name.to_lowercase()), "duplicate filename");
    }
    unsafe {
        Ok(VirtualFiles {
            files,
            descriptor: RegisterClipboardFormatW(w!("FileGroupDescriptorW")) as u16,
            contents: RegisterClipboardFormatW(w!("FileContents")) as u16,
            effect: RegisterClipboardFormatW(w!("Preferred DropEffect")) as u16,
            origin: RegisterClipboardFormatW(w!("Rustboard.Origin.v1")) as u16,
        }
        .into())
    }
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
                let mut fd = FILEDESCRIPTORW {
                    dwFlags: FD_FILESIZE.0 as u32 | FD_ATTRIBUTES.0 as u32 | FD_PROGRESSUI.0 as u32,
                    dwFileAttributes: FILE_ATTRIBUTE_NORMAL.0,
                    nFileSizeHigh: (file.source.len() >> 32) as u32,
                    nFileSizeLow: file.source.len() as u32,
                    ..Default::default()
                };
                let mut name = [0u16; 260];
                for (out, c) in name.iter_mut().zip(file.name.encode_utf16()) {
                    *out = c;
                }
                fd.cFileName = name;
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
            1u32.to_le_bytes().to_vec()
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
    fn SetData(&self, _: *const FORMATETC, _: *const STGMEDIUM, _: BOOL) -> Result<()> {
        Err(E_NOTIMPL.into())
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
    fn com_batch_over_tls_survives_new_file_and_text_clipboards() {
        use rustboard_core::{
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
        let server = source_node.serve(listener).unwrap();
        target_node.sync_once(&peer).unwrap();
        let object = create(
            files
                .iter()
                .map(|f| PresentedFile {
                    name: f.relative_path.clone(),
                    source: Arc::new(RemoteSource::new(
                        target_node.clone(),
                        peer.clone(),
                        manifest.clone(),
                        f.clone(),
                    )),
                })
                .collect(),
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
                        target_node.sync_once(&peer).unwrap();
                        target_node
                            .engine
                            .lock()
                            .unwrap()
                            .local(
                                Content::Text {
                                    text: "replacement".into(),
                                },
                                Default::default(),
                            )
                            .unwrap();
                    }
                }
                windows::Win32::System::Ole::ReleaseStgMedium(&mut medium);
            }
        }
        source_node.stop.store(true, Ordering::Release);
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
        let object = create(vec![PresentedFile {
            name: "test.bin".into(),
            source: source.clone(),
        }])
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
