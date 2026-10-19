mod clipboard;
pub(crate) mod credential;
mod files;
pub mod ipc;
pub(crate) mod service;

use crate::core::{Content, transport::Node};
use anyhow::Result;
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
            fuser::MountOption::RO,
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
        node.serve(std::net::TcpListener::bind(address)?)?;
    }
    server.spawn(node.clone());
    node.start_sync();
    log::info!(target: "agent", "started");
    let mut presented = None;
    while !node.stop.load(Ordering::Acquire) && !STOP.load(Ordering::Relaxed) {
        match clipboard.poll() {
            Ok(Some(captured)) => {
                let mut engine = node.engine.lock().unwrap();
                // Clipboard managers may take ownership of the same remote content.
                let echoed = engine.current.as_ref().is_some_and(|offer| {
                    offer.source_device != engine.device_id
                        && match (&captured, &offer.content) {
                            (clipboard::Captured::Text(value), Content::Text { text }) => {
                                value == text
                            }
                            (clipboard::Captured::Files(paths), Content::Files { files }) => {
                                paths.len() == files.len()
                                    && files.iter().all(|file| {
                                        paths.contains(
                                            &mount.join(&offer.offer_id).join(&file.relative_path),
                                        )
                                    })
                            }
                            _ => false,
                        }
                });
                if echoed {
                    continue;
                }
                let result = match captured {
                    clipboard::Captured::Text(text) => engine
                        .local(Content::Text { text }, Default::default())
                        .map(|_| ()),
                    clipboard::Captured::Files(paths) if !engine.text_only => {
                        files::offer(&mut engine, paths, &mount)
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
        if presented.is_some() && presented != current {
            clipboard.clear_owned()?;
            presented = None;
        }
        let incoming = node.incoming.lock().unwrap().take();
        if let Some((offer, _)) = incoming
            && current.as_ref() == Some(&offer.offer_id)
        {
            match &offer.content {
                Content::Text { text } => clipboard.publish_text(text)?,
                Content::Files { files } => clipboard.publish_files(
                    &files
                        .iter()
                        .map(|f| mount.join(&offer.offer_id).join(&f.relative_path))
                        .collect::<Vec<_>>(),
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
