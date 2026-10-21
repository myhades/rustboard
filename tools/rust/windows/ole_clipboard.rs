//! Standalone synthetic OLE clipboard fixture; never runs inside rustboardd.
#[cfg(windows)]
#[path = "../../../tests/support/pattern.rs"]
mod pattern;
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use rustboard::platform::{PresentedFile, windows::virtual_files};
    use std::{
        sync::{Arc, atomic::Ordering},
        time::{Duration, Instant},
    };
    use windows::{
        Win32::{System::Ole::*, UI::WindowsAndMessaging::*},
        core::BOOL,
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let size = args.first().map(|s| s.parse()).transpose()?.unwrap_or(4096);
    let seconds = args.get(1).map(|s| s.parse()).transpose()?.unwrap_or(30u64);
    let source = Arc::new(pattern::PatternSource::new(size));
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
    let object = virtual_files::create(vec![PresentedFile {
        directory: false,
        modified_ms: None,
        name: format!("rustboard-{size}.bin"),
        source: source.clone(),
    }])?;
    unsafe {
        OleSetClipboard(&object)?;
    }
    println!(
        "Synthetic clipboard ready: {size} bytes, {seconds} seconds; no Agent or peer required"
    );
    let until = Instant::now() + Duration::from_secs(seconds);
    let mut msg = MSG::default();
    while Instant::now() < until {
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) != BOOL(0) {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    unsafe {
        if OleIsCurrentClipboard(&object).is_ok() {
            OleSetClipboard(None)?;
        }
    }
    println!(
        "read_calls={} bytes_read={}",
        source.calls.load(Ordering::Relaxed),
        source.bytes_read.load(Ordering::Relaxed)
    );
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows OLE fixture requires Windows");
}
