mod clipboard;
pub(crate) mod credential;
pub(crate) mod files;
pub mod ipc;
pub(crate) mod service;

use crate::core::{Content, transport::Node};
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}
struct Cleanup(Arc<Node>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        self.0.stop.store(true, Ordering::Release);
        self.0.engine.lock().unwrap().withdraw();
    }
}

pub fn run() -> Result<()> {
    unsafe {
        libc::umask(0o077);
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
    }
    let dir = ipc::data_dir()?;
    ipc::protect_dir(&dir)?;
    let server = ipc::Server::bind(dir.clone())?;
    crate::app::logging::init(&dir)?;
    let mut clipboard = clipboard::Clipboard::new()?;
    let (identity, peers) = crate::app::config::startup(&dir)?;
    let node = Node::new(identity, peers);
    let _cleanup = Cleanup(node.clone());
    node.engine
        .lock()
        .unwrap()
        .persist_clock(dir.join("clock.json"))?;
    let listen = crate::app::config::apply(&dir, &mut node.engine.lock().unwrap())?;
    let mount = dir.join("files");
    // A killed FUSE daemon leaves a disconnected mount, never persistent file content.
    let _ = std::process::Command::new("fusermount3")
        .arg("-u")
        .arg(&mount)
        .output();
    ipc::protect_dir(&mount)?;
    let mounted = fuser::spawn_mount2(
        files::Files::new(node.clone()),
        &mount,
        &[
            fuser::MountOption::NoDev,
            fuser::MountOption::NoSuid,
            fuser::MountOption::NoExec,
            fuser::MountOption::FSName("rustboard".into()),
            fuser::MountOption::DefaultPermissions,
        ],
    )?;
    let images = dir.join("images");
    ipc::protect_dir(&images)?;
    crate::app::image_file::prepare(&images)?;
    if let Some(address) = listen {
        node.serve(std::net::TcpListener::bind(address)?, files::recycle)?;
    }
    server.spawn(node.clone());
    node.start_sync();
    log::info!(target: "agent", "started");
    let mut presented = None;
    let mut image_digest: Option<(String, [u8; 32])> = None;
    let mut download = crate::app::image_file::Download::default();
    while !node.stop.load(Ordering::Acquire) && !STOP.load(Ordering::Relaxed) {
        match clipboard.poll() {
            // A file manager may update a moved selection one URL at a time.
            // Never walk our own FUSE mount while capturing clipboard files.
            Ok(Some(clipboard::Captured::Files(paths, _)))
                if paths.iter().any(|path| path.starts_with(&mount)) => {}
            Ok(Some(captured)) => {
                let mut engine = node.engine.lock().unwrap();
                // Clipboard managers may take ownership of the same remote content.
                let echoed = engine.current.as_ref().is_some_and(|offer| {
                    offer.source_device != engine.device_id
                        && match (&captured, &offer.content) {
                            (clipboard::Captured::Text(value), content @ Content::Text { .. }) => {
                                value == content
                            }
                            (clipboard::Captured::Image(bytes, _), Content::Image { .. }) => {
                                image_digest.as_ref().is_some_and(|(id, digest)| {
                                    id == &offer.offer_id
                                        && *digest == <[u8; 32]>::from(Sha256::digest(bytes))
                                })
                            }
                            _ => false,
                        }
                });
                if echoed {
                    continue;
                }
                let result = match captured {
                    // File managers can clear a cut selection before opening its files.
                    clipboard::Captured::Cleared
                        if engine.current.as_ref().is_some_and(|offer| {
                            offer.source_device != engine.device_id
                                && matches!(offer.content, Content::Files { .. })
                        }) =>
                    {
                        Ok(())
                    }
                    clipboard::Captured::Text(text) => {
                        engine.local(text, Default::default()).map(|_| ())
                    }
                    clipboard::Captured::Files(paths, cut) if !engine.text_only => {
                        files::offer(&mut engine, paths, &mount, cut)
                    }
                    clipboard::Captured::Image(bytes, extension) if !engine.text_only => {
                        let path = images.join(format!(
                            "clipboard-{}.{}",
                            crate::core::engine::new_id(),
                            extension
                        ));
                        std::fs::write(&path, bytes)?;
                        crate::app::image_file::offer(&mut engine, path).map(|_| ())
                    }
                    _ => {
                        engine.clear_clipboard();
                        Ok(())
                    }
                };
                if let Err(error) = result {
                    engine.clear_clipboard();
                    log::warn!(target: "clipboard", "capture failed: {error:#}");
                }
                presented = None;
            }
            Err(error) => {
                log::warn!(target: "clipboard", "capture failed: {error:#}");
            }
            _ => {}
        }
        let current = {
            let mut engine = node.engine.lock().unwrap();
            engine.expire();
            engine.current.as_ref().map(|o| o.offer_id.clone())
        };
        if presented.is_some() && current.is_none() {
            clipboard.clear_owned()?;
            presented = None;
        }
        let incoming = match download.poll(&node) {
            Ok(value) => value,
            Err(error) => {
                log::warn!(target: "clipboard", "image download failed: {error}");
                None
            }
        };
        if let Some((offer, _, image)) = incoming
            && current.as_ref() == Some(&offer.offer_id)
        {
            image_digest = image
                .as_ref()
                .map(|bytes| (offer.offer_id.clone(), Sha256::digest(bytes).into()));
            match &offer.content {
                Content::Text { .. } | Content::Image { .. } => {
                    clipboard.publish_content(&offer.content, image)?
                }
                Content::Files { files } => clipboard.publish_files(
                    &files
                        .iter()
                        .filter(|f| !f.relative_path.contains('/'))
                        .map(|f| mount.join(&offer.offer_id).join(&f.relative_path))
                        .collect::<Vec<_>>(),
                    offer.cut,
                )?,
            }
            presented = Some(offer.offer_id);
            log::debug!(target: "clipboard", "publication complete");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    clipboard.clear_owned()?;
    node.stop.store(true, Ordering::Release);
    drop(mounted);
    Ok(())
}
