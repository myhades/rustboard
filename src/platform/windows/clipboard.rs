use super::virtual_files;
use crate::platform::PresentedFile;
use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
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
    node: Option<Arc<crate::core::transport::Node>>,
    remote_object: Option<windows::Win32::System::Com::IDataObject>,
    remote_offer: Option<String>,
    last_sequence: u32,
    self_sequence: u32,
    pending: bool,
    images: PathBuf,
    download: crate::app::image_file::Download,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

pub struct WindowsBackend;
impl WindowsBackend {
    fn publish_files(
        &mut self,
        files: Vec<PresentedFile>,
        node: Arc<crate::core::transport::Node>,
        offer: &crate::core::Offer,
    ) -> anyhow::Result<()> {
        let object = virtual_files::create(files, Some((node, offer.offer_id.clone())), offer.cut)?;
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
    set_rich_text(window, text, None, None)
}
fn set_formats(window: HWND, formats: Vec<(u32, Vec<u8>)>) -> anyhow::Result<()> {
    // Allocate before replacing the clipboard, so allocation failure preserves its contents.
    let mut blocks = Vec::new();
    for (format, bytes) in formats {
        match virtual_files::global_bytes(&bytes) {
            Ok(memory) => blocks.push((format, memory)),
            Err(error) => {
                for (_, memory) in blocks {
                    unsafe {
                        let _ = GlobalFree(Some(memory));
                    }
                }
                return Err(error.into());
            }
        }
    }
    let result = (|| -> anyhow::Result<()> {
        let _guard = ClipboardGuard::open(window)?;
        unsafe {
            EmptyClipboard()?;
            for (format, memory) in &mut blocks {
                SetClipboardData(*format, Some(HANDLE(memory.0)))?;
                *memory = HGLOBAL::default();
            }
        }
        Ok(())
    })();
    for (_, memory) in blocks {
        if !memory.is_invalid() {
            unsafe {
                let _ = GlobalFree(Some(memory));
            }
        }
    }
    if result.is_ok() {
        STATE.with_borrow_mut(|s| {
            s.self_sequence = unsafe { GetClipboardSequenceNumber() };
            s.remote_object = None;
            s.remote_offer = None;
        });
    }
    result
}
fn set_rich_text(
    window: HWND,
    text: &str,
    html: Option<&str>,
    rtf: Option<&str>,
) -> anyhow::Result<()> {
    use base64::Engine;
    let mut formats = vec![(
        13,
        text.encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect(),
    )];
    if let Some(html) = html {
        formats.push((
            unsafe { RegisterClipboardFormatW(w!("HTML Format")) },
            super::html::encode(html),
        ));
    }
    if let Some(rtf) = rtf {
        let mut bytes = base64::prelude::BASE64_STANDARD.decode(rtf)?;
        bytes.push(0);
        formats.push((
            unsafe { RegisterClipboardFormatW(w!("Rich Text Format")) },
            bytes,
        ));
    }
    set_formats(window, formats)
}
fn set_image(window: HWND, mime: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let dib = super::image_file::dib(bytes);
    let format = unsafe {
        RegisterClipboardFormatW(if mime == "image/png" {
            w!("PNG")
        } else {
            w!("JFIF")
        })
    };
    let mut formats = vec![(format, bytes.to_vec())];
    if let Ok(dib) = dib {
        formats.push((17, dib));
    }
    set_formats(window, formats)
}
fn read_bytes(format: u32, limit: usize) -> anyhow::Result<Option<Vec<u8>>> {
    unsafe {
        if IsClipboardFormatAvailable(format).is_err() {
            return Ok(None);
        }
        let handle = HGLOBAL(GetClipboardData(format)?.0);
        let size = GlobalSize(handle);
        anyhow::ensure!(size <= limit, "clipboard representation too large");
        let pointer = GlobalLock(handle);
        anyhow::ensure!(!pointer.is_null(), "cannot lock clipboard");
        let bytes = std::slice::from_raw_parts(pointer.cast::<u8>(), size).to_vec();
        let _ = GlobalUnlock(handle);
        Ok(Some(bytes))
    }
}

/// Captured paths are local only. Never serialize them onto the wire.
pub enum Captured {
    Text(crate::core::Content),
    Files(Vec<PathBuf>, bool),
    Image(super::image_file::Image),
    Unsupported,
}
pub fn capture(window: HWND) -> anyhow::Result<Captured> {
    let _guard = ClipboardGuard::open(window)?;
    unsafe {
        if IsClipboardFormatAvailable(15).is_ok() {
            let handle = GetClipboardData(15)?;
            let drop = HDROP(handle.0);
            let count = DragQueryFileW(drop, u32::MAX, None);
            anyhow::ensure!(count <= crate::core::MAX_FILES as u32, "too many files");
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
            let effect = read_bytes(RegisterClipboardFormatW(w!("Preferred DropEffect")), 64)?;
            let cut = effect
                .as_deref()
                .is_some_and(|b| b.get(..4) == Some(2u32.to_le_bytes().as_slice()));
            return Ok(Captured::Files(paths, cut));
        }
        use base64::Engine;
        let limit = crate::core::MAX_TEXT;
        let html = read_bytes(RegisterClipboardFormatW(w!("HTML Format")), limit)
            .ok()
            .flatten()
            .and_then(|b| super::html::decode(&b).ok());
        let rtf = read_bytes(RegisterClipboardFormatW(w!("Rich Text Format")), limit)
            .ok()
            .flatten()
            .map(|mut b| {
                while b.last() == Some(&0) {
                    b.pop();
                }
                base64::prelude::BASE64_STANDARD.encode(b)
            });
        if html.is_none() && rtf.is_none() {
            match super::image_file::capture() {
                Ok(Some(image)) => return Ok(Captured::Image(image)),
                Err(error) => {
                    log::warn!(target: "clipboard", "clipboard image rejected: {error}");
                    return Ok(Captured::Unsupported);
                }
                Ok(None) => {}
            }
        }
        let bytes = read_bytes(13, limit * 2 + 2)?;
        if bytes.is_some() || html.is_some() || rtf.is_some() {
            let bytes = bytes.unwrap_or_default();
            anyhow::ensure!(bytes.len().is_multiple_of(2), "invalid clipboard text");
            let utf16: Vec<_> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .take_while(|c| *c != 0)
                .collect();
            let text = String::from_utf16(&utf16)?;
            return Ok(Captured::Text(crate::core::Content::Text {
                text,
                html,
                rtf,
            }));
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
                Captured::Text(content) => {
                    STATE.with_borrow(|s| {
                        if let Some(node) = &s.node {
                            let mut engine = node.engine.lock().unwrap();
                            if engine.local(content, Default::default()).is_err() {
                                engine.clear_clipboard();
                            }
                        }
                    });
                }
                Captured::Files(paths, cut) => {
                    STATE.with_borrow(|s| {
                        if let Some(node) = &s.node {
                            let mut engine = node.engine.lock().unwrap();
                            if super::local_files::offer(&mut engine, paths.clone(), cut).is_err() {
                                engine.clear_clipboard();
                                log::warn!(target: "clipboard", "clipboard file offer rejected");
                            }
                        }
                    });
                }
                Captured::Image(image) => {
                    STATE.with_borrow(|s| {
                        if let Some(node) = &s.node {
                            let mut engine = node.engine.lock().unwrap();
                            if engine.text_only {
                                engine.clear_clipboard();
                                return;
                            }
                            let result = image
                                .save(&s.images)
                                .and_then(|path| crate::app::image_file::offer(&mut engine, path));
                            if result.is_err() {
                                engine.clear_clipboard();
                                log::warn!(target: "clipboard", "clipboard image rejected");
                            } else {
                                log::debug!(target: "clipboard", "image captured");
                            }
                        }
                    });
                }
                Captured::Unsupported => {
                    STATE.with_borrow(|s| {
                        if let Some(node) = &s.node {
                            node.engine.lock().unwrap().clear_clipboard();
                        }
                    });
                }
            }
        }
        Err(_) => {
            STATE.with_borrow_mut(|s| {
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
            WM_QUERYENDSESSION => return LRESULT(1),
            WM_ENDSESSION if wparam.0 != 0 => {
                STATE.with_borrow(|s| {
                    if let Some(node) = &s.node {
                        node.engine.lock().unwrap().withdraw();
                    }
                });
                unsafe {
                    let _ = DestroyWindow(window);
                }
            }
            WM_CLIPBOARDUPDATE => update(window),
            WM_TIMER => {
                tick(window);
                if STATE.with_borrow(|s| s.pending) {
                    update(window);
                }
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
    crate::app::logging::init(&dir)?;
    let images = dir.join("images");
    crate::app::image_file::prepare(&images)?;
    STATE.with_borrow_mut(|s| s.images = images);
    let (identity, peers) = crate::app::config::startup(&dir)?;
    let node = crate::core::transport::Node::new(identity, peers);
    node.engine
        .lock()
        .unwrap()
        .persist_clock(dir.join("clock.json"))?;
    let configured = crate::app::config::apply(&dir, &mut node.engine.lock().unwrap())?;
    let listen = if let Some(i) = args.iter().position(|v| v == "--listen") {
        Some(
            args.get(i + 1)
                .ok_or_else(|| anyhow::anyhow!("missing --listen IP:port"))?
                .parse::<std::net::SocketAddr>()?,
        )
    } else {
        configured
    };
    if let Some(address) = listen {
        node.serve(std::net::TcpListener::bind(address)?, |paths| {
            super::recycle::recycle(paths).is_ok()
        })?;
    }
    node.start_sync();
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
    log::info!(target: "agent", "started");
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
    }
    node.shutdown();
    node.engine.lock().unwrap().withdraw();
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
        e.current.is_none()
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
    let incoming = STATE.with_borrow_mut(|s| s.download.poll(&node));
    let incoming = match incoming {
        Ok(value) => value,
        Err(error) => {
            log::warn!(target: "clipboard", "image download failed: {error}");
            return;
        }
    };
    if let Some((offer, peer, image_bytes)) = incoming {
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
            let mut backend = WindowsBackend;
            match &offer.content {
                crate::core::Content::Text { text, html, rtf } => {
                    set_rich_text(window, text, html.as_deref(), rtf.as_deref())?
                }
                crate::core::Content::Image { image } => {
                    set_image(window, &image.media_type, image_bytes.as_deref().unwrap())?
                }
                crate::core::Content::Files { files } => {
                    anyhow::ensure!(
                        files.iter().all(|f| f.size.is_some()),
                        "unknown file sizes unsupported by Windows presentation"
                    );
                    let sources = crate::core::transport::RemoteSource::batch(
                        node.clone(),
                        peer.clone(),
                        offer.clone(),
                    );
                    let files = files
                        .iter()
                        .zip(sources)
                        .map(|(file, source)| PresentedFile {
                            directory: file.directory,
                            modified_ms: file.modified_ms,
                            name: file.relative_path.clone(),
                            source,
                        })
                        .collect();
                    backend.publish_files(files, node.clone(), &offer)?;
                    STATE.with_borrow_mut(|s| {
                        s.remote_offer = Some(offer.offer_id.clone());
                    });
                }
            }
            Ok(())
        })();
        if result.is_err() {
            log::warn!(target: "clipboard", "clipboard publication failed");
        }
    }
}
