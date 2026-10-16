use super::virtual_files;
use crate::PresentedFile;
use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::Instant,
};
use windows::{
    Win32::{
        Foundation::*,
        System::{DataExchange::*, LibraryLoader::GetModuleHandleW, Memory::*, Ole::*},
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, w},
};

#[derive(Default)]
struct State {
    node: Option<Arc<rustboard_core::transport::Node>>,
    remote_object: Option<windows::Win32::System::Com::IDataObject>,
    remote_offer: Option<String>,
    last_sequence: u32,
    self_sequence: u32,
    text_events: u64,
    file_events: u64,
    suppressed: u64,
    failures: u64,
    pending: bool,
    sources: Vec<Arc<rustboard_core::PatternSource>>,
    output: Option<PathBuf>,
    started: Option<Instant>,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

pub struct WindowsBackend {
    window: HWND,
}
impl crate::ClipboardBackend for WindowsBackend {
    fn publish_text(&mut self, text: &str) -> anyhow::Result<()> {
        set_text(self.window, text)
    }
}
impl crate::FilePresentationBackend for WindowsBackend {
    fn publish_files(&mut self, files: Vec<PresentedFile>) -> anyhow::Result<()> {
        let object = virtual_files::create(files)?;
        unsafe {
            OleSetClipboard(&object)?;
        }
        STATE.with_borrow_mut(|s| {
            s.self_sequence = unsafe { GetClipboardSequenceNumber() };
            s.remote_object = Some(object);
        });
        Ok(())
    }
}

struct ClipboardGuard;
impl ClipboardGuard {
    fn open(window: HWND) -> windows::core::Result<Self> {
        unsafe {
            OpenClipboard(Some(window))?;
        }
        Ok(Self)
    }
}
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

pub fn set_text(window: HWND, text: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !text.contains('\0') && text.len() <= 256 * 1024,
        "invalid text"
    );
    let encoded: Vec<u8> = text
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    let _guard = ClipboardGuard::open(window)?;
    unsafe {
        EmptyClipboard()?;
        let memory = virtual_files::global_bytes(&encoded)?;
        if let Err(error) = SetClipboardData(13, Some(HANDLE(memory.0))) {
            let _ = GlobalFree(Some(memory));
            return Err(error.into());
        }
    }
    drop(_guard); // Windows finalizes the clipboard sequence when the owner closes it.
    STATE.with_borrow_mut(|s| s.self_sequence = unsafe { GetClipboardSequenceNumber() });
    Ok(())
}

/// Captured paths are local only. Never serialize them onto the wire.
pub enum Captured {
    Text(String),
    Files(Vec<PathBuf>),
    Unsupported,
}
pub fn capture(window: HWND) -> anyhow::Result<Captured> {
    let _guard = ClipboardGuard::open(window)?;
    unsafe {
        // Explorer's preferred drop effect distinguishes Cut from Copy.
        let effect = RegisterClipboardFormatW(w!("Preferred DropEffect"));
        if IsClipboardFormatAvailable(effect).is_ok() {
            let handle = GetClipboardData(effect)?;
            let global = HGLOBAL(handle.0);
            if GlobalSize(global) >= 4 {
                let pointer = GlobalLock(global);
                if !pointer.is_null() {
                    let flags = pointer.cast::<u32>().read_unaligned();
                    let _ = GlobalUnlock(global);
                    if flags & 2 != 0 {
                        return Ok(Captured::Unsupported);
                    }
                }
            }
        }
        if IsClipboardFormatAvailable(15).is_ok() {
            let handle = GetClipboardData(15)?;
            let drop = HDROP(handle.0);
            let count = DragQueryFileW(drop, u32::MAX, None);
            anyhow::ensure!(count <= rustboard_core::MAX_FILES as u32, "too many files");
            let mut paths = Vec::new();
            for index in 0..count {
                let length = DragQueryFileW(drop, index, None);
                anyhow::ensure!(length < 32768, "path too long");
                let mut buffer = vec![0u16; length as usize + 1];
                DragQueryFileW(drop, index, Some(&mut buffer));
                paths.push(PathBuf::from(String::from_utf16(
                    &buffer[..length as usize],
                )?));
            }
            return Ok(Captured::Files(paths));
        }
        if IsClipboardFormatAvailable(13).is_ok() {
            let handle = HGLOBAL(GetClipboardData(13)?.0);
            let size = GlobalSize(handle);
            anyhow::ensure!(
                size <= 512 * 1024 + 2 && size.is_multiple_of(2),
                "invalid clipboard text size"
            );
            let pointer = GlobalLock(handle);
            anyhow::ensure!(!pointer.is_null(), "cannot lock clipboard");
            let buffer = std::slice::from_raw_parts(pointer.cast::<u16>(), size / 2);
            let end = buffer.iter().position(|&v| v == 0).unwrap_or(buffer.len());
            let text = String::from_utf16(&buffer[..end]);
            let _ = GlobalUnlock(handle);
            return Ok(Captured::Text(text?));
        }
    }
    Ok(Captured::Unsupported)
}

fn update(window: HWND) {
    let sequence = unsafe { GetClipboardSequenceNumber() };
    let skip = STATE.with_borrow_mut(|s| {
        if sequence == s.last_sequence {
            return true;
        }
        if sequence == s.self_sequence {
            s.last_sequence = sequence;
            s.suppressed += 1;
            return true;
        }
        false
    });
    if skip {
        return;
    }
    match capture(window) {
        Ok(content) => {
            STATE.with_borrow_mut(|s| {
                s.last_sequence = sequence;
                s.pending = false;
                s.remote_object = None;
                s.remote_offer = None;
            });
            match content {
                Captured::Text(text) => {
                    STATE.with_borrow_mut(|s| {
                        s.text_events += 1;
                        if let Some(node) = &s.node {
                            let _ = node.engine.lock().unwrap().local(
                                rustboard_core::Content::Text { text: text.clone() },
                                Default::default(),
                            );
                        }
                    });
                    eprintln!("clipboard text event bytes={}", text.len());
                }
                Captured::Files(paths) => {
                    STATE.with_borrow(|s| {
                        if let Some(node) = &s.node {
                            let mut engine = node.engine.lock().unwrap();
                            if super::local_files::offer(&mut engine, paths.clone()).is_err() {
                                engine.withdraw();
                                eprintln!("clipboard file offer rejected");
                            }
                        }
                    });
                    let mut sizes = Vec::new();
                    for path in paths {
                        if let Ok(file) = rustboard_core::LocalFile::open(&path) {
                            sizes.push(rustboard_core::ReadAt::len(&file));
                        }
                    }
                    STATE.with_borrow_mut(|s| s.file_events += 1);
                    eprintln!(
                        "clipboard file event count={} sizes={sizes:?} content_bytes_read=0",
                        sizes.len()
                    );
                }
                Captured::Unsupported => {
                    STATE.with_borrow(|s| {
                        if let Some(node) = &s.node {
                            node.engine.lock().unwrap().withdraw();
                        }
                    });
                    // Inspect OLE formats without materializing FileContents.
                    unsafe {
                        if let Ok(object) = OleGetClipboard()
                            && let Ok(formats) = object
                                .EnumFormatEtc(windows::Win32::System::Com::DATADIR_GET.0 as u32)
                        {
                            let mut count = 0;
                            let mut f = [Default::default()];
                            while count < 64 && formats.Next(&mut f, None) == S_OK {
                                count += 1;
                                if !f[0].ptd.is_null() {
                                    windows::Win32::System::Com::CoTaskMemFree(Some(
                                        f[0].ptd.cast(),
                                    ));
                                    f[0].ptd = std::ptr::null_mut();
                                }
                            }
                            eprintln!("clipboard unsupported formats_inspected={count}");
                        }
                    }
                }
            }
        }
        Err(_) => {
            STATE.with_borrow_mut(|s| {
                s.failures += 1;
                s.pending = true;
            });
        }
    }
}

unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Never unwind through Win32.
    let result = std::panic::catch_unwind(|| {
        match message {
            WM_CLIPBOARDUPDATE => update(window),
            WM_TIMER => {
                tick(window);
                if STATE.with_borrow(|s| s.pending) {
                    update(window);
                }
                STATE.with_borrow(|s| {
                    if let Some(path) = &s.output {
                        let value = serde_json::json!({ "elapsed_ms": s.started.map(|v| v.elapsed().as_millis()), "text_events": s.text_events, "file_events": s.file_events, "suppressed": s.suppressed, "failures": s.failures, "sources": s.sources.iter().map(|v| serde_json::json!({"size":v.size,"read_calls":v.calls.load(Ordering::Relaxed),"bytes_read":v.bytes_read.load(Ordering::Relaxed)})).collect::<Vec<_>>() });
                        let _ = std::fs::write(path, value.to_string());
                    }
                });
            }
            WM_DESTROY => unsafe {
                PostQuitMessage(0);
            },
            _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
        }
        LRESULT(0)
    });
    result.unwrap_or(LRESULT(0))
}

pub fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let dir = super::ipc::prepare_dir()?;
    let ipc = super::ipc::Server::bind(dir.clone())?;
    let identity = rustboard_core::transport::Identity::load(&dir)?;
    let peers_path = dir.join("peers.json");
    let peers: Vec<rustboard_core::transport::Peer> = if peers_path.exists() {
        serde_json::from_slice(&std::fs::read(peers_path)?)?
    } else {
        vec![]
    };
    anyhow::ensure!(peers.len() <= 16, "too many peers");
    for peer in &peers {
        rustboard_core::transport::device_from_pin(&peer.fingerprint)?;
    }
    let node = rustboard_core::transport::Node::new(identity, peers);
    node.engine
        .lock()
        .unwrap()
        .persist_clock(dir.join("clock.json"))?;
    if let Some(i) = args.iter().position(|v| v == "--listen") {
        let address: std::net::SocketAddr = args
            .get(i + 1)
            .ok_or_else(|| anyhow::anyhow!("missing --listen IP:port"))?
            .parse()?;
        node.serve(std::net::TcpListener::bind(address)?)?;
        node.start_sync();
    }
    ipc.spawn(node.clone());
    STATE.with_borrow_mut(|s| s.node = Some(node.clone()));
    unsafe {
        OleInitialize(None)?;
    }
    struct OleGuard;
    impl Drop for OleGuard {
        fn drop(&mut self) {
            unsafe {
                OleUninitialize();
            }
        }
    }
    let _ole = OleGuard;
    let instance = unsafe { GetModuleHandleW(None)? };
    let class = w!("Rustboard.Clipboard.v1");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(procedure),
        hInstance: instance.into(),
        lpszClassName: class,
        ..Default::default()
    };
    anyhow::ensure!(unsafe { RegisterClassW(&wc) } != 0, "window class failed");
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            PCWSTR::null(),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?
    };
    unsafe {
        AddClipboardFormatListener(window)?;
        SetTimer(Some(window), 1, 500, None);
    }
    STATE.with_borrow_mut(|s| {
        s.started = Some(Instant::now());
        s.output = args
            .iter()
            .position(|v| v == "--metrics")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from);
    });
    let mut object = None;
    if args.iter().any(|s| s == "--ole-demo") {
        let sizes = args
            .iter()
            .position(|s| s == "--sizes")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("4096,67108864,1073741824");
        let mut files = Vec::new();
        for (i, value) in sizes.split(',').enumerate() {
            let size = value.parse::<u64>()?;
            let source = Arc::new(rustboard_core::PatternSource::new(size));
            files.push(PresentedFile {
                name: format!("rustboard-{i}-{size}.bin"),
                source: source.clone(),
            });
            STATE.with_borrow_mut(|s| s.sources.push(source));
        }
        let data = virtual_files::create(files)?;
        unsafe {
            OleSetClipboard(&data)?;
        }
        STATE.with_borrow_mut(|s| s.self_sequence = unsafe { GetClipboardSequenceNumber() });
        eprintln!("OLE published; source bytes_read=0; ready for Explorer paste");
        object = Some(data);
    }
    if let Some(i) = args.iter().position(|v| v == "--text-demo") {
        set_text(
            window,
            args.get(i + 1)
                .ok_or_else(|| anyhow::anyhow!("missing text"))?,
        )?;
    }
    let mut msg = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut msg, None, 0, 0) }.0;
        if result <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    unsafe {
        let _ = RemoveClipboardFormatListener(window);
        if let Some(data) = &object
            && OleIsCurrentClipboard(data).is_ok()
        {
            OleSetClipboard(None)?;
        }
    }
    drop(object);
    node.stop.store(true, Ordering::Release);
    STATE.with_borrow_mut(|s| {
        s.remote_object = None;
        s.node = None;
    });
    Ok(())
}

fn tick(window: HWND) {
    let node = STATE.with_borrow(|s| s.node.clone());
    let Some(node) = node else { return };
    if node.stop.load(Ordering::Acquire) {
        unsafe {
            let _ = DestroyWindow(window);
        }
        return;
    }
    let expired = {
        let mut e = node.engine.lock().unwrap();
        e.expire();
        e.paused || e.current.is_none()
    };
    if expired {
        STATE.with_borrow_mut(|s| {
            if let Some(object) = s.remote_object.take() {
                unsafe {
                    if OleIsCurrentClipboard(&object).is_ok() {
                        let _ = OleSetClipboard(None);
                    }
                }
            }
            s.remote_offer = None;
        });
    }
    let incoming = node.incoming.lock().unwrap().take();
    if let Some((offer, peer)) = incoming {
        let current = node
            .engine
            .lock()
            .unwrap()
            .current
            .as_ref()
            .map(|o| o.offer_id.clone());
        if current.as_deref() != Some(&offer.offer_id) {
            return;
        }
        let result = (|| -> anyhow::Result<()> {
            use crate::{ClipboardBackend, FilePresentationBackend};
            let mut backend = WindowsBackend { window };
            match &offer.content {
                rustboard_core::Content::Text { text } => backend.publish_text(text)?,
                rustboard_core::Content::Files { files } => {
                    anyhow::ensure!(
                        files.iter().all(|f| f.size.is_some()),
                        "unknown file sizes unsupported by Windows presentation"
                    );
                    let files = files
                        .iter()
                        .map(|file| PresentedFile {
                            name: file.relative_path.clone(),
                            source: Arc::new(rustboard_core::transport::RemoteSource::new(
                                node.clone(),
                                peer.clone(),
                                offer.clone(),
                                file.clone(),
                            ))
                                as Arc<dyn rustboard_core::ReadAt>,
                        })
                        .collect();
                    backend.publish_files(files)?;
                    STATE.with_borrow_mut(|s| {
                        s.remote_offer = Some(offer.offer_id.clone());
                    });
                }
            }
            Ok(())
        })();
        if result.is_err() {
            STATE.with_borrow_mut(|s| s.failures += 1);
        }
    }
}
