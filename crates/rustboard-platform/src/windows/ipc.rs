use anyhow::{Result, ensure};
use rustboard_core::{
    read_frame,
    transport::{IpcReply, Node},
    write_frame,
};
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::*,
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
        System::{Pipes::*, Threading::*},
    },
    core::{PCWSTR, PWSTR},
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
struct Handle(HANDLE);
// Owned kernel handle; ownership transfers to one IPC thread and is never concurrently used.
unsafe impl Send for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub fn user_sid() -> Result<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let token = Handle(token);
        let mut size = 0;
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut size);
        let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        )?;
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut output = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut output)?;
        let result = output.to_string();
        let _ = LocalFree(Some(HLOCAL(output.0.cast())));
        Ok(result?)
    }
}
pub fn profile() -> Result<String> {
    let profile = std::env::var("RUSTBOARD_PROFILE").unwrap_or_else(|_| "default".into());
    ensure!(
        !profile.is_empty()
            && profile.len() <= 32
            && profile
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "invalid RUSTBOARD_PROFILE"
    );
    Ok(profile)
}
pub fn data_dir() -> Result<PathBuf> {
    Ok(PathBuf::from(std::env::var("LOCALAPPDATA")?)
        .join("Rustboard")
        .join(profile()?))
}
pub fn prepare_dir() -> Result<PathBuf> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)?;
    // Restrict inherited permissions BEFORE storing identity material. icacls is a system tool.
    use std::os::windows::process::CommandExt;
    let status = std::process::Command::new("icacls.exe")
        .arg(&dir)
        .args(["/inheritance:r", "/grant:r"])
        .arg(format!("*{}:(OI)(CI)F", user_sid()?))
        .creation_flags(0x08000000)
        .output()?;
    ensure!(
        status.status.success(),
        "cannot protect configuration directory"
    );
    Ok(dir)
}
fn pipe_name() -> Result<Vec<u16>> {
    Ok(wide(&format!(
        r"\\.\pipe\Rustboard-{}-{}",
        user_sid()?,
        profile()?
    )))
}

struct Pipe {
    handle: Handle,
    deadline: Instant,
}
impl Pipe {
    fn new(h: HANDLE) -> Self {
        Self {
            handle: Handle(h),
            deadline: Instant::now() + Duration::from_secs(10),
        }
    }
}
impl Read for Pipe {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        loop {
            let mut n = 0;
            let result = unsafe { ReadFile(self.handle.0, Some(buffer), Some(&mut n), None) };
            match result {
                Ok(()) if n > 0 => return Ok(n as usize),
                Err(e) if e.code() != ERROR_NO_DATA.to_hresult() => {
                    return Err(std::io::Error::other(e));
                }
                _ => {}
            }
            if Instant::now() >= self.deadline {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Write for Pipe {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        loop {
            let mut n = 0;
            unsafe { WriteFile(self.handle.0, Some(buffer), Some(&mut n), None) }
                .map_err(std::io::Error::other)?;
            if n > 0 {
                return Ok(n as usize);
            }
            if Instant::now() >= self.deadline {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub fn request(args: Vec<String>) -> Result<serde_json::Value> {
    let name = pipe_name()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let h = loop {
        let result = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_MODE::default(),
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES::default(),
                None,
            )
        };
        match result {
            Ok(h) => break h,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => anyhow::bail!("Agent unavailable; start rustboardd in this user session"),
        }
    };
    let mut pipe = Pipe::new(h);
    unsafe {
        SetNamedPipeHandleState(h, Some(&PIPE_NOWAIT), None, None)?;
    }
    write_frame(
        &mut pipe,
        &rustboard_core::IpcRequest {
            protocol: 1,
            request: rustboard_core::IpcOperation::Control { args },
        },
    )?;
    read_frame(&mut pipe)
}

/// Holds the first named-pipe instance for the entire Agent lifetime: also singleton lock.
pub struct Server {
    pipe: Pipe,
    dir: PathBuf,
}
impl Server {
    pub fn bind(dir: PathBuf) -> Result<Self> {
        let name = pipe_name()?;
        let sddl = wide(&format!("D:P(A;;GA;;;{})", user_sid()?));
        unsafe {
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )?;
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor.0,
                bInheritHandle: false.into(),
            };
            let h = CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                65536,
                65536,
                1000,
                Some(&attributes),
            );
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            ensure!(
                !h.is_invalid(),
                "Agent already running or pipe unavailable: {}",
                windows::core::Error::from_win32()
            );
            Ok(Self {
                pipe: Pipe::new(h),
                dir,
            })
        }
    }
    pub fn spawn(self, node: Arc<Node>) {
        std::thread::spawn(move || self.run(node));
    }
    fn run(mut self, node: Arc<Node>) {
        while !node.stop.load(Ordering::Acquire) {
            let connected = unsafe { ConnectNamedPipe(self.pipe.handle.0, None) };
            if connected
                .as_ref()
                .is_err_and(|e| e.code() != ERROR_PIPE_CONNECTED.to_hresult())
            {
                std::thread::sleep(Duration::from_millis(25));
                continue;
            }
            self.pipe.deadline = Instant::now() + Duration::from_secs(10);

            let reply =
                read_frame::<_, rustboard_core::IpcRequest>(&mut self.pipe).and_then(|request| {
                    ensure!(request.protocol == 1, "unsupported IPC version");
                    match request.request {
                        rustboard_core::IpcOperation::Control { args } => {
                            self.handle(&node, args).map(IpcReply::Other)
                        }
                        operation => node.local_ipc_reply(operation),
                    }
                });
            let value = reply
                .unwrap_or_else(|error| IpcReply::Other(serde_json::json!({"ok":false,"protocol":1,"code":"io","error":error.to_string()})));
            let _ = rustboard_core::transport::write_ipc_reply(&mut self.pipe, &value);
            // Wait for client to consume response before disconnecting (no blocking FlushFileBuffers).
            let mut dummy = [0u8; 1];
            let _ = self.pipe.read(&mut dummy);
            unsafe {
                let _ = DisconnectNamedPipe(self.pipe.handle.0);
            }
        }
    }
    fn handle(&self, node: &Arc<Node>, args: Vec<String>) -> Result<serde_json::Value> {
        ensure!(
            !args.is_empty() && args.len() <= 5 && args.iter().all(|s| s.len() < 256),
            "invalid command"
        );
        match args[0].as_str() {
            "status" => return crate::config::status(node, &self.dir),
            "pair-begin" | "pair-poll" | "pair-approve" | "pair-cancel" => {
                return crate::pairing::control(node, &self.dir, &args);
            }
            "unpair-address" => {
                ensure!(args.len() == 2, "address required");
                crate::config::unpair_address(&self.dir, args[1].parse()?)?;
            }
            "stop" => node.stop.store(true, Ordering::Release),
            _ => anyhow::bail!("unknown command"),
        }
        Ok(serde_json::json!({"ok":true}))
    }
}
