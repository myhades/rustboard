use crate::core::{IpcOperation, IpcRequest, read_frame, transport::Node, write_frame};
use anyhow::{Result, ensure};
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
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
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
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(std::env::var("HOME")?).join(".config"),
    };
    ensure!(base.is_absolute(), "config directory must be absolute");
    Ok(base.join("rustboard").join(profile()?))
}
fn authenticate(stream: &UnixStream) -> Result<()> {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of_val(&credentials) as libc::socklen_t;
    ensure!(
        unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut size,
            )
        } == 0
            && credentials.uid == unsafe { libc::getuid() },
        "IPC identity denied"
    );
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    Ok(())
}
pub fn request(args: Vec<String>) -> Result<serde_json::Value> {
    let mut stream = UnixStream::connect(data_dir()?.join("agent.sock")).map_err(|error| {
        match error.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                anyhow::anyhow!("agent unavailable")
            }
            _ => error.into(),
        }
    })?;
    authenticate(&stream)?;
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
            "agent already running"
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
                        authenticate(&stream)?;
                        let req: IpcRequest = read_frame(&mut stream)?;
                        ensure!(req.protocol == 1, "unsupported protocol");
                        let IpcOperation::Control { args } = req.request else {
                            anyhow::bail!("control request required");
                        };
                        let reply = server.handle(&node, args).unwrap_or_else(
                            |e| serde_json::json!({"ok":false,"error":format!("{e:#}")}),
                        );
                        write_frame(&mut stream, &reply)?;
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
            "status" => return crate::app::config::status(node, &self.dir),
            "pair-begin" | "pair-poll" | "pair-approve" | "pair-cancel" => {
                return crate::app::pairing::control(node, &self.dir, &args);
            }
            "unpair-ip" => {
                ensure!(args.len() == 2, "address required");
                crate::app::config::unpair_ip(&self.dir, args[1].parse()?)?;
            }
            "stop" => node.shutdown(),
            _ => anyhow::bail!("unknown command"),
        }
        Ok(serde_json::json!({"ok":true}))
    }
}
