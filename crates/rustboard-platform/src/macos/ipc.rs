use anyhow::{Result, ensure};
use rustboard_core::{
    IpcOperation, IpcRequest, read_frame,
    transport::{IpcReply, Node, Peer, device_from_pin},
    write_frame,
};
use std::{
    fs::{self, File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

unsafe extern "C" {
    fn rb_authenticate(fd: i32, server: i32) -> i32;
    fn rb_group_path() -> *mut libc::c_char;
}
pub fn profile() -> Result<String> {
    let p = std::env::var("RUSTBOARD_PROFILE").unwrap_or_else(|_| "default".into());
    ensure!(
        !p.is_empty() && p.len() <= 32 && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "invalid profile"
    );
    Ok(p)
}
pub fn protect_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        fs::DirBuilder::new().mode(0o700).create(dir)?;
    }
    let m = fs::symlink_metadata(dir)?;
    ensure!(
        m.is_dir() && !m.file_type().is_symlink() && m.uid() == unsafe { libc::getuid() },
        "unsafe directory"
    );
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub fn data_dir() -> Result<PathBuf> {
    let raw = unsafe { rb_group_path() };
    ensure!(!raw.is_null(), "signed app-group access required");
    let path = unsafe { std::ffi::CStr::from_ptr(raw).to_str()?.to_owned() };
    unsafe {
        libc::free(raw.cast());
    }
    let dir = PathBuf::from(path).join(profile()?);
    protect_dir(&dir)?;
    Ok(dir)
}
fn authenticate(stream: &UnixStream, server: bool) -> Result<()> {
    // Darwin accept inherits listener O_NONBLOCK. Framed requests must block
    // with bounded timeouts rather than racing the client handshake.
    stream.set_nonblocking(false)?;
    ensure!(
        unsafe { rb_authenticate(stream.as_raw_fd(), i32::from(server)) } == 0,
        "IPC identity denied"
    );
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    Ok(())
}
pub fn request(args: Vec<String>) -> Result<serde_json::Value> {
    let mut stream = UnixStream::connect(data_dir()?.join("agent.sock"))?;
    authenticate(&stream, true)?;
    write_frame(
        &mut stream,
        &IpcRequest {
            protocol: 1,
            request: IpcOperation::Control { args },
        },
    )?;
    read_frame(&mut stream)
}
pub struct Server {
    listener: UnixListener,
    _lock: File,
    dir: PathBuf,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.dir.join("agent.sock"));
    }
}
impl Server {
    pub fn bind(dir: PathBuf) -> Result<Self> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(dir.join("agent.lock"))?;
        ensure!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "Agent already running"
        );
        let path = dir.join("agent.sock");
        if let Ok(m) = fs::symlink_metadata(&path) {
            use std::os::unix::fs::FileTypeExt;
            ensure!(
                m.file_type().is_socket() && m.uid() == unsafe { libc::getuid() },
                "unsafe socket path"
            );
            fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            _lock: lock,
            dir,
        })
    }
    pub fn spawn(self, node: Arc<Node>) {
        let server = Arc::new(self);
        let active = Arc::new(AtomicUsize::new(0));
        std::thread::spawn(move || {
            while !node.stop.load(Ordering::Acquire) {
                let mut poll = libc::pollfd {
                    fd: server.listener.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                if unsafe { libc::poll(&mut poll, 1, 200) } <= 0 {
                    continue;
                }
                let Ok((mut stream, _)) = server.listener.accept() else {
                    continue;
                };
                if active.fetch_add(1, Ordering::AcqRel) >= 8 {
                    active.fetch_sub(1, Ordering::AcqRel);
                    continue;
                }
                let (server, node, active) = (server.clone(), node.clone(), active.clone());
                std::thread::spawn(move || {
                    let run = || -> Result<()> {
                        authenticate(&stream, false)?;
                        while !node.stop.load(Ordering::Acquire) {
                            let req: IpcRequest = read_frame(&mut stream)?;
                            let reply = (|| -> Result<IpcReply> {
                                ensure!(req.protocol == 1, "unsupported protocol");
                                match req.request {
                                    IpcOperation::Control { args } => server.handle(&node, args).map(IpcReply::Other),
                                    op => node.local_ipc_reply(op),
                                }
                            })().unwrap_or_else(|_| IpcReply::Other(serde_json::json!({"ok":false,"protocol":1,"code":"io","error":"request failed"})));
                            write_frame(&mut stream, &reply)?;
                        }
                        Ok(())
                    };
                    let mut run = run;
                    let _ = run();
                    active.fetch_sub(1, Ordering::AcqRel);
                });
            }
        });
    }
    fn handle(&self, node: &Arc<Node>, args: Vec<String>) -> Result<serde_json::Value> {
        ensure!(
            !args.is_empty() && args.len() <= 5 && args.iter().all(|s| s.len() < 256),
            "invalid command"
        );
        match args[0].as_str() {
            "status" | "doctor" => {
                let e = node.engine.lock().unwrap();
                return Ok(
                    serde_json::json!({"ok":true,"version":env!("CARGO_PKG_VERSION"),"protocol":1,"device_id":node.identity.device(),"fingerprint":node.identity.pin(),"paused":e.paused,"mode":if e.text_only{"text"}else{"all"},"offer_id":e.current.as_ref().map(|o|&o.offer_id),"peers":node.peers.lock().unwrap().len(),"native":"macos-appkit-fileprovider","network":"TLS1.3 certificate pins; listener opt-in"}),
                );
            }
            "peers" => {
                return Ok(serde_json::json!({"ok":true,"peers":*node.peers.lock().unwrap()}));
            }
            "pause" => {
                let mut e = node.engine.lock().unwrap();
                e.paused = true;
                e.withdraw();
                *node.incoming.lock().unwrap() = None;
            }
            "resume" => node.engine.lock().unwrap().paused = false,
            "mode" => {
                ensure!(
                    args.len() == 2 && ["text", "all"].contains(&args[1].as_str()),
                    "mode text|all"
                );
                let mut e = node.engine.lock().unwrap();
                e.text_only = args[1] == "text";
                if e.text_only
                    && e.current
                        .as_ref()
                        .is_some_and(|o| matches!(o.content, rustboard_core::Content::Files { .. }))
                {
                    e.withdraw();
                }
            }
            "pair" => {
                ensure!(
                    args.len() == 4 && args[2] == "--fingerprint",
                    "pair <IP:port> --fingerprint <64 hex chars copied from peer doctor>; configure BOTH peers"
                );
                let pin = args[3].to_lowercase();
                device_from_pin(&pin)?;
                ensure!(pin != node.identity.pin(), "cannot pair self");
                let address = args[1].parse()?;
                let mut peers = node.peers.lock().unwrap();
                ensure!(peers.len() < 16, "peer limit reached");
                let mut next = peers.clone();
                next.retain(|p| p.fingerprint != pin);
                next.push(Peer {
                    address,
                    fingerprint: pin,
                });
                self.save_peers(&next)?;
                *peers = next;
            }
            "unpair" => {
                ensure!(args.len() == 2, "unpair <device_id or fingerprint>");
                let mut peers = node.peers.lock().unwrap();
                let mut next = peers.clone();
                next.retain(|p| p.device() != args[1] && p.fingerprint != args[1]);
                self.save_peers(&next)?;
                *peers = next;
            }
            "stop" => node.stop.store(true, Ordering::Release),
            _ => anyhow::bail!("unknown command"),
        }
        Ok(serde_json::json!({"ok":true}))
    }
    fn save_peers(&self, peers: &[Peer]) -> Result<()> {
        let path = self.dir.join("peers.json");
        let temporary = self.dir.join("peers.new");
        std::fs::write(&temporary, serde_json::to_vec_pretty(peers)?)?;
        std::fs::rename(temporary, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustboard_core::{Content, FileEntry, PatternSource, engine::new_id, transport::Identity};
    #[test]
    fn native_operations_use_shared_node_and_revoke_sources() {
        let node = Node::new(Identity::generate().unwrap(), vec![]);
        let source = Arc::new(PatternSource::new(4096));
        let id = new_id();
        let file = FileEntry {
            file_id: id.clone(),
            relative_path: "sample.bin".into(),
            size: Some(4096),
            version: "v1".into(),
            media_type: "application/octet-stream".into(),
        };
        let offer = node
            .engine
            .lock()
            .unwrap()
            .local(
                Content::Files { files: vec![file] },
                [(
                    id.clone(),
                    source.clone() as Arc<dyn rustboard_core::ReadAt>,
                )]
                .into(),
            )
            .unwrap();
        assert_eq!(source.bytes_read.load(Ordering::Relaxed), 0);
        let metadata = node.local_ipc(IpcOperation::GetOffer).unwrap();
        assert_eq!(metadata["offer"]["offer_id"], offer.offer_id);
        assert_eq!(source.bytes_read.load(Ordering::Relaxed), 0);
        let read = |version: &str, offset, length| IpcOperation::Read {
            request_id: 7,
            offer_id: offer.offer_id.clone(),
            file_id: id.clone(),
            version: version.into(),
            offset,
            length,
        };
        for _ in 0..2 {
            let result = node.local_ipc(read("v1", 249, 5)).unwrap();
            assert_eq!(
                result["response"]["bytes"],
                serde_json::json!([249, 250, 0, 1, 2])
            );
        }
        assert!(node.local_ipc(read("wrong", 0, 1)).is_err());
        assert!(node.local_ipc(read("v1", 0, 65537)).is_err());
        assert!(node.local_ipc(read("v1", 4097, 1)).is_err());
        assert_eq!(
            node.local_ipc(IpcOperation::Cancel { request_id: 7 })
                .unwrap()["ok"],
            true
        );
        node.engine
            .lock()
            .unwrap()
            .current
            .as_mut()
            .unwrap()
            .expires_unix_ms = 0;
        assert!(node.local_ipc(read("v1", 0, 1)).is_err());
        assert!(node.local_ipc(IpcOperation::GetOffer).unwrap()["offer"].is_null());
    }
}
