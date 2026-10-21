pub mod ipc;
pub mod local_files;
use crate::core::{Content, transport::Node};
use anyhow::{Result, ensure};
use std::{
    ffi::{CStr, CString},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
unsafe extern "C" {
    fn rb_init();
    fn rb_wake_on_receive() -> i32;
    fn rb_change_count() -> i64;
    fn rb_adopt_files(sequence: i64, count: i64) -> i32;
    fn rb_clear_owned();
    fn rb_pump();
    fn rb_capture() -> *mut libc::c_char;
    fn rb_capture_image_file(stem: *const libc::c_char) -> i32;
    fn rb_publish(json: *const libc::c_char) -> i32;
}
// A short-lived host schedules native downloads without blocking clipboard polling.
struct Prefetch {
    child: std::process::Child,
    offer_id: String,
}
impl Drop for Prefetch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
pub struct MacBackend {
    pub node: Arc<Node>,
}
impl MacBackend {
    fn publish_text(&mut self, text: &str) -> Result<()> {
        ensure!(text.len() <= 262144 && !text.contains('\0'), "invalid text");
        publish(serde_json::json!({"text":text}))
    }
}
impl MacBackend {
    fn publish_files(&mut self, offer: &crate::core::Offer) -> Result<()> {
        let Content::Files { files } = &offer.content else {
            anyhow::bail!("not a file offer")
        };
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
            || offer.expires_unix_ms <= crate::core::engine::now_ms()
        {
            unsafe {
                rb_clear_owned();
            }
        }
        ensure!(
            e.current.as_ref().map(|o| &o.offer_id) == Some(&offer.offer_id)
                && offer.expires_unix_ms > crate::core::engine::now_ms(),
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
        log::warn!(target: "clipboard",
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
static SHUTDOWN: AtomicBool = AtomicBool::new(false);
extern "C" fn shutdown_signal(_: libc::c_int) {
    SHUTDOWN.store(true, Ordering::Relaxed);
}
struct Cleanup(Arc<Node>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        self.0.stop.store(true, Ordering::Release);
        self.0.engine.lock().unwrap().withdraw();
        unsafe {
            rb_clear_owned();
        }
        if host_command(&["reset"]).is_err() {
            log::warn!(target: "clipboard", "File Provider cleanup incomplete; retry at next startup");
        }
    }
}
pub fn run() -> Result<()> {
    unsafe {
        libc::umask(0o077);
        libc::signal(
            libc::SIGTERM,
            shutdown_signal as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGINT,
            shutdown_signal as *const () as libc::sighandler_t,
        );
        rb_init();
    }
    let dir = ipc::data_dir()?;
    ipc::protect_dir(&dir)?;
    let server = ipc::Server::bind(dir.clone())?;
    crate::app::logging::init(&dir)?;
    let images = dir.join("images");
    ipc::protect_dir(&images)?;
    crate::app::image_file::prepare(&images)?;
    let snapshots = dir.join("snapshots");
    ipc::protect_dir(&snapshots)?;
    // A singleton owns this private directory. No offer survives an Agent restart.
    for entry in std::fs::read_dir(&snapshots)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            std::fs::remove_file(entry.path())?;
        }
    }
    let (identity, peers) = crate::app::config::startup(&dir)?;
    let config = crate::app::config::load(&dir)?;
    let eager_download = config["macos"]["eager_download"].as_bool().unwrap_or(false);
    let wake_on_receive = config["macos"]["wake_on_receive"]
        .as_bool()
        .unwrap_or(false);
    let node = Node::new(identity, peers);
    let _cleanup = Cleanup(node.clone());
    // Reset before accepting peers or publishing any new offer. No old transfer survives restart.
    host_command(&["reset"])?;
    node.engine
        .lock()
        .unwrap()
        .persist_clock(dir.join("clock.json"))?;
    let configured = crate::app::config::apply(&dir, &mut node.engine.lock().unwrap())?;
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
    log::info!(target: "agent", "started");
    let mut backend = MacBackend { node: node.clone() };
    let mut presented: Option<String> = None;
    let mut pending: Option<Prefetch> = None;
    let mut last_prune = std::time::Instant::now();
    let mut retained = Vec::new();
    while !node.stop.load(Ordering::Acquire) && !SHUTDOWN.load(Ordering::Relaxed) {
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
                    if engine.text_only {
                        engine.clear_clipboard();
                        continue;
                    }
                    let stem = images.join(format!("clipboard-{}", crate::core::engine::new_id()));
                    let cpath = CString::new(stem.as_os_str().as_encoded_bytes())?;
                    match unsafe { rb_capture_image_file(cpath.as_ptr()) } {
                        1 => crate::app::image_file::offer(&mut engine, stem.with_extension("png")),
                        2 => {
                            crate::app::image_file::offer(&mut engine, stem.with_extension("tiff"))
                        }
                        _ => Err(anyhow::anyhow!("image file capture failed")),
                    }
                }
                Some("files") => {
                    let paths: Vec<PathBuf> = serde_json::from_value(value["paths"].clone())?;
                    local_files::offer(&mut engine, paths, &snapshots)
                }
                _ => {
                    engine.clear_clipboard();
                    continue;
                }
            };
            if result.is_err() {
                engine.clear_clipboard();
                log::warn!(target: "clipboard", "clipboard capture rejected");
            } else {
                if value["kind"] == "image" {
                    log::debug!(target: "clipboard", "clipboard image published as ordinary file");
                } else {
                    log::debug!(target: "clipboard", "local offer published content_bytes_read=0");
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
        let mut live: Vec<_> = node
            .engine
            .lock()
            .unwrap()
            .retained_offers()
            .into_iter()
            .map(|o| o.offer_id)
            .collect();
        live.sort();
        if live != retained || last_prune.elapsed() >= std::time::Duration::from_secs(10) {
            let _ = host_command(&["revoke"]);
            retained = live;
            last_prune = std::time::Instant::now();
        }
        if presented.is_some() && presented != current_id {
            unsafe {
                rb_clear_owned();
            }
            presented = None;
        }
        if let Some(task) = pending.as_mut() {
            if current_id.as_ref() != Some(&task.offer_id) {
                pending = None;
            } else if let Some(status) = task.child.try_wait()? {
                pending = None;
                if !status.success() {
                    log::warn!(target: "clipboard", "background prefetch failed; files remain available on demand");
                }
            }
        }
        let incoming = node.incoming.lock().unwrap().take();
        if let Some((offer, _peer)) = incoming {
            let current = node.engine.lock().unwrap().current.clone();
            if current.as_ref().map(|o| &o.offer_id) != Some(&offer.offer_id) {
                continue;
            }
            match &offer.content {
                Content::Text { text } => {
                    backend.publish_text(text)?;
                    log::debug!(target: "clipboard", "native text publication complete");
                }
                Content::Files { .. } => {
                    if backend.publish_files(&offer).is_err() {
                        log::warn!(target: "clipboard", "file publication failed");
                        continue;
                    } else {
                        presented = Some(offer.offer_id.clone());
                        log::debug!(target: "clipboard", "native file publication complete");
                        if eager_download {
                            pending = None;
                            let host = std::env::var_os("RUSTBOARD_HOST")
                                .ok_or_else(|| anyhow::anyhow!("RUSTBOARD_HOST required"))?;
                            match std::process::Command::new(host)
                                .args(["prefetch", &offer.offer_id])
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .spawn()
                            {
                                Ok(child) => {
                                    pending = Some(Prefetch {
                                        child,
                                        offer_id: offer.offer_id.clone(),
                                    })
                                }
                                Err(_) => {
                                    log::warn!(target: "clipboard", "background prefetch host unavailable")
                                }
                            }
                        }
                    }
                }
            }
            if wake_on_receive && unsafe { rb_wake_on_receive() } != 0 {
                log::warn!(target: "clipboard", "user activity declaration failed");
            }
        }
    }
    drop(pending);
    Ok(())
}

pub(crate) mod credential;
pub(crate) mod service;
