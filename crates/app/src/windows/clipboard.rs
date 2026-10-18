use super::virtual_files;
use crate::PresentedFile;
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
    node: Option<Arc<rustboard_core::transport::Node>>,
    remote_object: Option<windows::Win32::System::Com::IDataObject>,
    remote_offer: Option<String>,
    last_sequence: u32,
    self_sequence: u32,
    pending: bool,
    images: PathBuf,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

pub struct WindowsBackend {
    window: HWND,
}
impl WindowsBackend {
    fn publish_text(&mut self, text: &str) -> anyhow::Result<()> {
        set_text(self.window, text)
    }
}
impl WindowsBackend {
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
        match super::image_file::capture() {
            Ok(Some(image)) => return Ok(Captured::Image(image)),
            Err(_) => {
                eprintln!("clipboard image rejected: unsupported or larger than 64 MiB");
                return Ok(Captured::Unsupported);
            }
            Ok(None) => {}
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
                                engine.clear_clipboard();
                                eprintln!("clipboard file offer rejected");
                            }
                        }
                    });
                }
                Captured::Image(image) => {
                    STATE.with_borrow(|s| {
                        if let Some(node) = &s.node {
                            let mut engine = node.engine.lock().unwrap();
                            if engine.paused || engine.text_only {
                                engine.clear_clipboard();
                                return;
                            }
                            let result = image
                                .save(&s.images)
                                .and_then(|path| crate::image_file::offer(&mut engine, path));
                            if result.is_err() {
                                engine.clear_clipboard();
                                eprintln!("clipboard image-to-file rejected");
                            } else {
                                eprintln!("clipboard image published as ordinary file");
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
    let images = dir.join("images");
    crate::image_file::prepare(&images)?;
    STATE.with_borrow_mut(|s| s.images = images);
    let (identity, peers) = crate::config::startup(&dir)?;
    let node = rustboard_core::transport::Node::new(identity, peers);
    node.engine
        .lock()
        .unwrap()
        .persist_clock(dir.join("clock.json"))?;
    let configured = crate::config::apply(&dir, &mut node.engine.lock().unwrap())?;
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
        node.serve(std::net::TcpListener::bind(address)?)?;
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
            eprintln!("clipboard publication failed");
        }
    }
}
