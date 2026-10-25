//! Encoded clipboard images use the same bounded transfer as files.
#[cfg(not(windows))]
use crate::core::LocalFile;
use crate::core::{
    Content, FileEntry, Offer, ReadAt,
    engine::{Engine, new_id},
};
#[cfg(windows)]
use crate::platform::windows::local_files::LocalFile;
use anyhow::{Result, ensure};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

struct OwnedImage {
    source: Option<LocalFile>,
    path: PathBuf,
}
impl Drop for OwnedImage {
    fn drop(&mut self) {
        drop(self.source.take());
        let _ = std::fs::remove_file(&self.path);
    }
}
impl ReadAt for OwnedImage {
    fn len(&self) -> u64 {
        self.source.as_ref().unwrap().len()
    }
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        self.source.as_ref().unwrap().read_at(offset, buffer)
    }
}
pub fn prepare(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    ensure!(
        !std::fs::symlink_metadata(dir)?.file_type().is_symlink(),
        "unsafe image directory"
    );
    // Called only after acquiring the profile singleton. Never traverse directories.
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with("clipboard-")
        {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}
pub fn offer(engine: &mut Engine, path: PathBuf) -> Result<Offer> {
    let result = (|| {
        #[cfg(windows)]
        let source = crate::platform::windows::local_files::open(&path)?;
        #[cfg(not(windows))]
        let source = LocalFile::open(&path)?;
        ensure!(
            source.len() > 0 && source.len() <= (MAX_IMAGE_BYTES + 14) as u64,
            "image size limit"
        );
        let version = source.version();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("image name"))?
            .to_owned();
        let source = Arc::new(OwnedImage {
            source: Some(source),
            path: path.clone(),
        });
        let id = new_id();
        engine.local(
            Content::Image {
                image: FileEntry {
                    directory: false,
                    modified_ms: None,
                    executable: None,
                    file_id: id.clone(),
                    relative_path: name,
                    size: Some(source.len()),
                    version,
                    media_type: match path.extension().and_then(|s| s.to_str()) {
                        Some("png") => "image/png",
                        Some("jpg" | "jpeg") => "image/jpeg",
                        _ => anyhow::bail!("unsupported image encoding"),
                    }
                    .into(),
                },
            },
            [(id, source as Arc<dyn ReadAt>)].into(),
        )
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

type ImageTask = (
    Offer,
    crate::core::transport::Peer,
    std::thread::JoinHandle<Result<Vec<u8>>>,
);
pub type Publication = (Offer, crate::core::transport::Peer, Option<Vec<u8>>);

#[derive(Default)]
pub struct Download {
    task: Option<ImageTask>,
}
impl Download {
    pub fn poll(
        &mut self,
        node: &Arc<crate::core::transport::Node>,
    ) -> Result<Option<Publication>> {
        if self
            .task
            .as_ref()
            .is_some_and(|(_, _, task)| task.is_finished())
        {
            let (offer, peer, task) = self.task.take().unwrap();
            let bytes = task
                .join()
                .map_err(|_| anyhow::anyhow!("image download stopped"))?;
            if node
                .engine
                .lock()
                .unwrap()
                .current
                .as_ref()
                .map(|o| &o.offer_id)
                == Some(&offer.offer_id)
            {
                return Ok(Some((offer, peer, Some(bytes?))));
            }
        }
        let mut incoming = node.incoming.lock().unwrap();
        if incoming
            .as_ref()
            .is_some_and(|(o, _)| matches!(o.content, Content::Image { .. }))
        {
            if self.task.is_none() {
                let (offer, peer) = incoming.take().unwrap();
                let Content::Image { image } = &offer.content else {
                    unreachable!()
                };
                let source = crate::core::transport::RemoteSource::new(
                    node.clone(),
                    peer.clone(),
                    offer.offer_id.clone(),
                    image.clone(),
                );
                let id = offer.offer_id.clone();
                let node = node.clone();
                let task = std::thread::spawn(move || {
                    let mut bytes = vec![0; source.len() as usize];
                    let mut offset = 0;
                    while offset < bytes.len() {
                        ensure!(
                            !node.stop.load(std::sync::atomic::Ordering::Acquire)
                                && node
                                    .engine
                                    .lock()
                                    .unwrap()
                                    .current
                                    .as_ref()
                                    .map(|o| &o.offer_id)
                                    == Some(&id),
                            "image replaced"
                        );
                        let end = (offset + crate::core::MAX_CHUNK).min(bytes.len());
                        let n = source.read_at(offset as u64, &mut bytes[offset..end])?;
                        ensure!(n > 0, "incomplete image");
                        offset += n;
                    }
                    Ok(bytes)
                });
                self.task = Some((offer, peer, task));
            }
            Ok(None)
        } else {
            Ok(incoming.take().map(|(offer, peer)| (offer, peer, None)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_download_is_complete_and_does_not_publish_replaced_content() {
        use crate::core::transport::{Identity, Node, Peer};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::{Duration, Instant};
        struct Source {
            started: AtomicBool,
            release: AtomicBool,
        }
        impl ReadAt for Source {
            fn len(&self) -> u64 {
                131072
            }
            fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
                self.started.store(true, Ordering::Release);
                let deadline = Instant::now() + Duration::from_secs(5);
                while !self.release.load(Ordering::Acquire) {
                    ensure!(Instant::now() < deadline, "test timed out");
                    std::thread::sleep(Duration::from_millis(1));
                }
                let length = buffer.len().min((self.len() - offset) as usize);
                buffer[..length].fill(42);
                Ok(length)
            }
        }
        let a = Node::new(Identity::generate().unwrap(), vec![]);
        let b = Node::new(Identity::generate().unwrap(), vec![]);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let peer = Peer {
            address: listener.local_addr().unwrap(),
            fingerprint: a.identity.pin(),
        };
        a.peers.lock().unwrap().push(Peer {
            address: peer.address,
            fingerprint: b.identity.pin(),
        });
        b.peers.lock().unwrap().push(peer.clone());
        let server = a.serve(listener, |_| false).unwrap();
        let mut download = Download::default();
        for replaced in [false, true] {
            let source = Arc::new(Source {
                started: AtomicBool::new(false),
                release: AtomicBool::new(false),
            });
            let id = new_id();
            a.engine
                .lock()
                .unwrap()
                .local(
                    Content::Image {
                        image: FileEntry {
                            file_id: id.clone(),
                            relative_path: "image.png".into(),
                            size: Some(source.len()),
                            version: "image".into(),
                            media_type: "image/png".into(),
                            directory: false,
                            modified_ms: None,
                            executable: None,
                        },
                    },
                    [(id, source.clone() as Arc<dyn ReadAt>)].into(),
                )
                .unwrap();
            crate::test_network::sync(&a, &b);
            assert!(download.poll(&b).unwrap().is_none());
            let deadline = Instant::now() + Duration::from_secs(5);
            while !source.started.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            if replaced {
                b.engine
                    .lock()
                    .unwrap()
                    .local(Content::text("new text"), Default::default())
                    .unwrap();
            }
            source.release.store(true, Ordering::Release);
            let mut result = None;
            while download.task.is_some() {
                assert!(Instant::now() < deadline);
                result = download.poll(&b).unwrap().or(result);
                std::thread::sleep(Duration::from_millis(1));
            }
            if replaced {
                assert!(result.is_none());
            } else {
                assert_eq!(result.unwrap().2.unwrap(), vec![42; 131072]);
            }
        }
        a.shutdown();
        b.shutdown();
        server.join().unwrap();
    }
    #[test]
    fn owned_image_is_removed_on_withdraw_and_rejection() {
        let dir = std::env::temp_dir().join(new_id());
        prepare(&dir).unwrap();
        let path = dir.join("clipboard-test.png");
        std::fs::write(&path, b"synthetic encoded bytes").unwrap();
        let mut engine = Engine::new(new_id());
        offer(&mut engine, path.clone()).unwrap();
        assert!(path.exists());
        engine.withdraw();
        assert!(!path.exists());
        std::fs::write(&path, b"synthetic encoded bytes").unwrap();
        engine.text_only = true;
        assert!(offer(&mut engine, path.clone()).is_err());
        assert!(!path.exists());
        std::fs::remove_dir(&dir).unwrap();
    }
}
