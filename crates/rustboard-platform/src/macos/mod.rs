pub mod ipc;
pub mod local_files;
use anyhow::{Result, ensure};
use rustboard_core::{Content, transport::Node};
use std::{
    ffi::{CStr, CString},
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
};
unsafe extern "C" {
    fn rb_init();
    fn rb_change_count() -> i64;
    fn rb_adopt_files(sequence: i64, count: i64) -> i32;
    fn rb_clear_owned();
    fn rb_pump();
    fn rb_capture() -> *mut libc::c_char;
    fn rb_capture_image_file(stem: *const libc::c_char) -> i32;
    fn rb_publish(json: *const libc::c_char) -> i32;
}
pub struct MacBackend {
    pub node: Arc<Node>,
}
impl crate::ClipboardBackend for MacBackend {
    fn publish_text(&mut self, text: &str) -> Result<()> {
        ensure!(text.len() <= 262144 && !text.contains('\0'), "invalid text");
        publish(serde_json::json!({"text":text}))
    }
}
impl crate::FilePresentationBackend for MacBackend {
    fn publish_files(&mut self, files: Vec<crate::PresentedFile>) -> Result<()> {
        let offer = self
            .node
            .engine
            .lock()
            .unwrap()
            .current
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no current offer"))?;
        let Content::Files { files: manifest } = &offer.content else {
            anyhow::bail!("not a file offer")
        };
        ensure!(
            manifest.len() == files.len()
                && manifest
                    .iter()
                    .zip(&files)
                    .all(|(m, f)| m.relative_path == f.name && m.size == Some(f.source.len())),
            "manifest mismatch"
        );
        let sequence = unsafe { rb_change_count() };
        let mut output = None;
        for attempt in 0..3 {
            ensure!(
                sequence == unsafe { rb_change_count() },
                "clipboard changed during publication"
            );
            ensure!(
                self.node
                    .engine
                    .lock()
                    .unwrap()
                    .current
                    .as_ref()
                    .map(|o| &o.offer_id)
                    == Some(&offer.offer_id),
                "offer replaced"
            );
            match host_command(&["urls", &offer.offer_id, &sequence.to_string()]) {
                Ok(value) => {
                    output = Some(value);
                    break;
                }
                Err(error) if attempt == 2 => return Err(error),
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(250)),
            }
        }
        let out = output.ok_or_else(|| anyhow::anyhow!("file publication unavailable"))?;
        let value: serde_json::Value = serde_json::from_slice(&out.stdout)?;
        let published = value["change_count"]
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("missing publication sequence"))?;
        ensure!(
            unsafe { rb_adopt_files(published, files.len() as i64) } == 0,
            "clipboard changed during publication"
        );
        let e = self.node.engine.lock().unwrap();
        if e.current.as_ref().map(|o| &o.offer_id) != Some(&offer.offer_id)
            || offer.expires_unix_ms <= rustboard_core::engine::now_ms()
        {
            unsafe {
                rb_clear_owned();
            }
        }
        ensure!(
            e.current.as_ref().map(|o| &o.offer_id) == Some(&offer.offer_id)
                && offer.expires_unix_ms > rustboard_core::engine::now_ms(),
            "offer replaced"
        );
        Ok(())
    }
}
fn host_command(args: &[&str]) -> Result<std::process::Output> {
    let host = std::env::var_os("RUSTBOARD_HOST")
        .ok_or_else(|| anyhow::anyhow!("RUSTBOARD_HOST required"))?;
    let out = std::process::Command::new(host).args(args).output()?;
    if !out.status.success() {
        // The host emits stage/domain/numeric codes only, never clipboard bodies.
        eprintln!(
            "File Provider: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        anyhow::bail!("File Provider operation failed");
    }
    Ok(out)
}
fn publish(value: serde_json::Value) -> Result<()> {
    let json = CString::new(serde_json::to_vec(&value)?)?;
    ensure!(
        unsafe { rb_publish(json.as_ptr()) } == 0,
        "clipboard publication failed"
    );
    Ok(())
}
pub fn run() -> Result<()> {
    unsafe {
        libc::umask(0o077);
        rb_init();
    }
    let dir = ipc::data_dir()?;
    let server = ipc::Server::bind(dir.clone())?;
    let images = dir.join("images");
    ipc::protect_dir(&images)?;
    crate::image_file::prepare(&images)?;
    let snapshots = dir.join("snapshots");
    ipc::protect_dir(&snapshots)?;
    // A singleton owns this private directory. No offer survives an Agent restart.
    for entry in std::fs::read_dir(&snapshots)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            std::fs::remove_file(entry.path())?;
        }
    }
    let (identity, peers) = crate::config::startup(&dir)?;
    let node = Node::new(identity, peers);
    node.engine
        .lock()
        .unwrap()
        .persist_clock(dir.join("clock.json"))?;
    let configured = crate::config::apply(&dir, &mut node.engine.lock().unwrap())?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let listen = if !args.is_empty() {
        ensure!(
            args.len() == 2 && args[0] == "--listen",
            "usage: rustboardd [--listen IP:port]"
        );
        Some(args[1].parse::<std::net::SocketAddr>()?)
    } else {
        configured
    };
    if let Some(address) = listen {
        node.serve(std::net::TcpListener::bind(address)?)?;
    }
    server.spawn(node.clone());
    node.start_sync();
    let mut backend = MacBackend { node: node.clone() };
    let _ = host_command(&["revoke"]);
    let mut presented: Option<String> = None;
    while !node.stop.load(Ordering::Acquire) {
        unsafe {
            rb_pump();
        }
        let raw = unsafe { rb_capture() };
        if !raw.is_null() {
            let bytes = unsafe { CStr::from_ptr(raw).to_bytes().to_vec() };
            unsafe {
                libc::free(raw.cast());
            }
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            let mut engine = node.engine.lock().unwrap();
            let result = match value["kind"].as_str() {
                Some("text") => engine.local(
                    Content::Text {
                        text: value["text"].as_str().unwrap_or_default().into(),
                    },
                    Default::default(),
                ),
                Some("image") => {
                    if engine.paused || engine.text_only {
                        engine.withdraw();
                        continue;
                    }
                    let stem =
                        images.join(format!("clipboard-{}", rustboard_core::engine::new_id()));
                    let cpath = CString::new(stem.as_os_str().as_encoded_bytes())?;
                    match unsafe { rb_capture_image_file(cpath.as_ptr()) } {
                        1 => crate::image_file::offer(&mut engine, stem.with_extension("png")),
                        2 => crate::image_file::offer(&mut engine, stem.with_extension("tiff")),
                        _ => Err(anyhow::anyhow!("image file capture failed")),
                    }
                }
                Some("files") => {
                    let paths: Vec<PathBuf> = serde_json::from_value(value["paths"].clone())?;
                    local_files::offer(&mut engine, paths, &snapshots)
                }
                _ => {
                    engine.withdraw();
                    continue;
                }
            };
            if result.is_err() {
                engine.withdraw();
                eprintln!("clipboard capture rejected");
            } else {
                if value["kind"] == "image" {
                    eprintln!("clipboard image published as ordinary file");
                } else {
                    eprintln!("local offer published content_bytes_read=0");
                }
            }
        }
        node.engine.lock().unwrap().expire();
        let current_id = node
            .engine
            .lock()
            .unwrap()
            .current
            .as_ref()
            .map(|o| o.offer_id.clone());
        if presented.is_some() && presented != current_id {
            if host_command(&["revoke"]).is_err() {
                eprintln!("provider eviction failed");
            }
            unsafe {
                rb_clear_owned();
            }
            presented = None;
        }
        let incoming = node.incoming.lock().unwrap().take();
        if let Some((offer, peer)) = incoming {
            let current = node.engine.lock().unwrap().current.clone();
            if current.as_ref().map(|o| &o.offer_id) != Some(&offer.offer_id) {
                continue;
            }
            match &offer.content {
                Content::Text { text } => {
                    use crate::ClipboardBackend;
                    backend.publish_text(text)?;
                    eprintln!("native text publication complete");
                }
                Content::Files { files } => {
                    use crate::FilePresentationBackend;
                    let presented_files = files
                        .iter()
                        .map(|file| crate::PresentedFile {
                            name: file.relative_path.clone(),
                            source: Arc::new(rustboard_core::transport::RemoteSource::new(
                                node.clone(),
                                peer.clone(),
                                offer.clone(),
                                file.clone(),
                            )),
                        })
                        .collect();
                    if backend.publish_files(presented_files).is_err() {
                        eprintln!("file publication failed");
                    } else {
                        presented = Some(offer.offer_id.clone());
                        eprintln!("native file publication complete");
                    }
                }
            }
        }
    }
    node.engine.lock().unwrap().withdraw();
    let _ = host_command(&["revoke"]);
    Ok(())
}

pub(crate) mod credential;
pub(crate) mod service;
