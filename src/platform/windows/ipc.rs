use crate::core::{
    read_frame,
    transport::{IpcReply, Node},
    write_frame,
};
use anyhow::{Result, ensure};
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
    process_sid(unsafe { GetCurrentProcess() })
}
fn process_sid(process: HANDLE) -> Result<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token)?;
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
    protect_profile(&dir)?;
    Ok(dir)
}
fn protect_profile(dir: &std::path::Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let sddl = wide(&format!("D:P(A;OICI;FA;;;{})", user_sid()?));
    unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )?;
        let result = (|| -> Result<()> {
            let mut present = false.into();
            let mut defaulted = false.into();
            let mut dacl = std::ptr::null_mut();
            GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted)?;
            // Replace the complete DACL, including explicit grants on existing state files.
            // The encrypted identity is stored in config.json.
            for name in ["", "config.json", "config.lock", "clock.json"] {
                let path = dir.join(name);
                let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                let status = SetNamedSecurityInfoW(
                    PCWSTR(path.as_ptr()),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    None,
                    None,
                    Some(dacl),
                    None,
                );
                if !name.is_empty() && status == ERROR_FILE_NOT_FOUND {
                    continue;
                }
                status.ok()?;
            }
            Ok(())
        })();
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        result
    }
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
fn authenticate_server(pipe: HANDLE) -> Result<()> {
    unsafe {
        let mut pid = 0;
        GetNamedPipeServerProcessId(pipe, &mut pid)?;
        let process = Handle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?);
        ensure!(
            process_sid(process.0)? == user_sid()?,
            "untrusted IPC server"
        );
    }
    Ok(())
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
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                None,
            )
        };
        match result {
            Ok(h) => break h,
            Err(error)
                if error.code() == ERROR_PIPE_BUSY.to_hresult() && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) if error.code() == ERROR_FILE_NOT_FOUND.to_hresult() => {
                anyhow::bail!("agent unavailable");
            }
            Err(error) => return Err(error.into()),
        }
    };
    let mut pipe = Pipe::new(h);
    authenticate_server(h)?;
    unsafe {
        SetNamedPipeHandleState(h, Some(&PIPE_NOWAIT), None, None)?;
    }
    write_frame(
        &mut pipe,
        &crate::core::IpcRequest {
            protocol: 1,
            request: crate::core::IpcOperation::Control { args },
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
                "agent already running or pipe unavailable: {}",
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
                read_frame::<_, crate::core::IpcRequest>(&mut self.pipe).and_then(|request| {
                    ensure!(request.protocol == 1, "unsupported IPC version");
                    match request.request {
                        crate::core::IpcOperation::Control { args } => {
                            self.handle(&node, args).map(IpcReply::Other)
                        }
                        operation => node.local_ipc_reply(operation),
                    }
                });
            let value = reply
                .unwrap_or_else(|error| IpcReply::Other(serde_json::json!({"ok":false,"protocol":1,"code":"io","error":error.to_string()})));
            let _ = crate::core::transport::write_ipc_reply(&mut self.pipe, &value);
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
            "status" => return crate::app::config::status(node, &self.dir),
            "pair-begin" | "pair-poll" | "pair-approve" | "pair-cancel" => {
                return crate::app::pairing::control(node, &self.dir, &args);
            }
            "unpair-ip" => {
                ensure!(args.len() == 2, "address required");
                crate::app::config::unpair_ip(&self.dir, args[1].parse()?)?;
            }
            "stop" => node.stop.store(true, Ordering::Release),
            _ => anyhow::bail!("unknown command"),
        }
        Ok(serde_json::json!({"ok":true}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dacl_text(path: &std::path::Path) -> (String, String) {
        use std::os::windows::ffi::OsStrExt;
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            GetNamedSecurityInfoW(
                PCWSTR(path.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                &mut descriptor,
            )
            .ok()
            .unwrap();
            let mut present = windows::core::BOOL::default();
            let mut defaulted = windows::core::BOOL::default();
            let mut acl = std::ptr::null_mut();
            GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted).unwrap();
            assert!(present.as_bool() && !acl.is_null());
            let mut ace = std::ptr::null_mut();
            GetAce(acl, 0, &mut ace).unwrap();
            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            let mut sid = PWSTR::null();
            ConvertSidToStringSidW(
                PSID(std::ptr::addr_of!(ace.SidStart).cast_mut().cast()),
                &mut sid,
            )
            .unwrap();
            let sid_value = sid.to_string().unwrap();
            let _ = LocalFree(Some(HLOCAL(sid.0.cast())));
            let mut text = PWSTR::null();
            let result = ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                None,
            );
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            result.unwrap();
            let value = text.to_string();
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
            (value.unwrap(), sid_value)
        }
    }
    #[test]
    fn profile_acl_replaces_explicit_grants_and_protects_new_files() {
        use std::os::windows::ffi::OsStrExt;
        let dir = std::env::temp_dir().join(format!("rustboard-acl-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        // Fresh initialization must also work before state files exist.
        protect_profile(&dir).unwrap();
        let sid = user_sid().unwrap();
        for name in ["config.json", "config.lock", "clock.json"] {
            std::fs::write(dir.join(name), b"unchanged").unwrap();
        }
        let sddl = wide(&format!("D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;WD)"));
        unsafe {
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
            .unwrap();
            for name in ["", "config.json", "config.lock", "clock.json"] {
                let path = dir.join(name);
                let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                assert!(
                    SetFileSecurityW(
                        PCWSTR(path.as_ptr()),
                        DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                        descriptor,
                    )
                    .as_bool()
                );
            }
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        }
        assert!(dacl_text(&dir.join("config.json")).0.contains(";;;WD)"));
        protect_profile(&dir).unwrap();
        protect_profile(&dir).unwrap();
        for name in ["", "config.json", "config.lock", "clock.json"] {
            let (acl, trustee) = dacl_text(&dir.join(name));
            assert!(acl.starts_with("D:P"), "{acl}");
            assert_eq!(acl.matches('(').count(), 1, "{acl}");
            assert_eq!(trustee, sid, "{acl}");
            if !name.is_empty() {
                assert_eq!(std::fs::read(dir.join(name)).unwrap(), b"unchanged");
            }
        }
        let new_file = dir.join("new.tmp");
        std::fs::write(&new_file, b"new").unwrap();
        let (acl, trustee) = dacl_text(&new_file);
        assert_eq!(acl.matches('(').count(), 1, "{acl}");
        assert_eq!(trustee, sid, "{acl}");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn pipe_server_identity_is_checked() {
        unsafe {
            let name = wide(&format!(
                r"\\.\pipe\Rustboard-test-{}",
                uuid::Uuid::new_v4()
            ));
            let server = Handle(CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                4096,
                4096,
                1000,
                None,
            ));
            assert!(!server.0.is_invalid());
            let client = Handle(
                CreateFileW(
                    PCWSTR(name.as_ptr()),
                    GENERIC_READ.0 | GENERIC_WRITE.0,
                    FILE_SHARE_MODE::default(),
                    None,
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    None,
                )
                .unwrap(),
            );
            authenticate_server(client.0).unwrap();
            assert!(authenticate_server(INVALID_HANDLE_VALUE).is_err());
        }
    }
}
