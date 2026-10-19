//! Current-user startup registration and background process lifecycle.
use anyhow::{Result, ensure};
use std::{
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::*,
        System::{Diagnostics::ToolHelp::*, Registry::*, Threading::*},
    },
    core::{PCWSTR, PWSTR, w},
};
const KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
fn base() -> Result<PathBuf> {
    ensure!(
        super::ipc::profile()? == "default",
        "service commands target installed default profile only"
    );
    Ok(PathBuf::from(std::env::var("LOCALAPPDATA")?).join("Rustboard"))
}
fn command(name: &str) -> Command {
    let mut c = Command::new(name);
    c.creation_flags(0x08000000);
    c
}
pub fn enabled() -> Result<bool> {
    base()?;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            KEY,
            w!("Rustboard"),
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        )
    };
    match result {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        error => Err(windows::core::Error::from_hresult(error.to_hresult()).into()),
    }
}
pub fn start(register: bool) -> Result<()> {
    let base = base()?;
    let agent = base.join("bin/rustboardd.exe");
    ensure!(agent.is_file(), "Rustboard is not installed");
    if register {
        let value: Vec<u16> = format!("\"{}\"", agent.display())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                KEY,
                w!("Rustboard"),
                REG_SZ.0,
                Some(value.as_ptr().cast()),
                (value.len() * 2) as u32,
            )
            .ok()?;
        }
    }
    if crate::app::cli::request(&["status"]).is_ok() {
        return Ok(());
    }
    let _inheritance = StandardHandles::protect()?;
    let mut c = command(
        agent
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("invalid install path"))?,
    );
    c.stdin(Stdio::null())
        .stdout(std::fs::File::create(base.join("agent.out.log"))?)
        .stderr(std::fs::File::create(base.join("agent.err.log"))?)
        .spawn()?;
    for _ in 0..50 {
        if crate::app::cli::request(&["status"]).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!("Agent did not start; inspect agent.err.log (login startup may remain enabled)")
}
pub fn stop(unregister: bool) -> Result<()> {
    let agent = base()?.join("bin/rustboardd.exe");
    if unregister {
        let result = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, KEY, w!("Rustboard")) };
        if result != ERROR_FILE_NOT_FOUND && result != ERROR_PATH_NOT_FOUND {
            result.ok()?;
        }
    }
    // Keep handles until shutdown completes: an exited PID may be reused.
    let mut processes = Vec::new();
    unsafe {
        let snapshot =
            OwnedHandle::from_raw_handle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?.0);
        let snapshot_handle = HANDLE(snapshot.as_raw_handle());
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut next = Process32FirstW(snapshot_handle, &mut entry);
        while next.is_ok() {
            let length = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..length])
                .eq_ignore_ascii_case("rustboardd.exe")
            {
                // Other users' processes may be inaccessible; processes can also exit here.
                if let Ok(handle) = OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    false,
                    entry.th32ProcessID,
                ) {
                    let process = OwnedHandle::from_raw_handle(handle.0);
                    let mut path = vec![0u16; 32768];
                    let mut length = path.len() as u32;
                    if QueryFullProcessImageNameW(
                        handle,
                        PROCESS_NAME_WIN32,
                        PWSTR(path.as_mut_ptr()),
                        &mut length,
                    )
                    .is_ok()
                        && String::from_utf16_lossy(&path[..length as usize])
                            .eq_ignore_ascii_case(&agent.to_string_lossy().replace('/', "\\"))
                    {
                        processes.push(process);
                    }
                }
            }
            next = Process32NextW(snapshot_handle, &mut entry);
        }
        if let Err(error) = next
            && error.code() != ERROR_NO_MORE_FILES.to_hresult()
        {
            return Err(error.into());
        }
    }
    if processes.is_empty() {
        return Ok(());
    }
    crate::app::cli::request(&["stop"])?;
    let deadline = Instant::now() + Duration::from_secs(2);
    for process in processes {
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as u32;
        match unsafe { WaitForSingleObject(HANDLE(process.as_raw_handle()), timeout) } {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT => anyhow::bail!("Agent did not stop"),
            _ => return Err(windows::core::Error::from_win32().into()),
        }
    }
    Ok(())
}
pub fn edit(path: &Path) -> Result<()> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "notepad.exe".into());
    Command::new(editor).arg(path).spawn()?;
    Ok(())
}

// The background child must not keep a caller's redirected CLI pipes alive.
struct StandardHandles(Vec<windows::Win32::Foundation::HANDLE>);
impl StandardHandles {
    fn protect() -> Result<Self> {
        use windows::Win32::{Foundation::*, System::Console::*};
        let mut guard = Self(Vec::new());
        unsafe {
            for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
                if let Ok(handle) = GetStdHandle(which) {
                    let mut flags = 0;
                    if GetHandleInformation(handle, &mut flags).is_ok()
                        && flags & HANDLE_FLAG_INHERIT.0 != 0
                    {
                        SetHandleInformation(handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0))?;
                        guard.0.push(handle);
                    }
                }
            }
        }
        Ok(guard)
    }
}
impl Drop for StandardHandles {
    fn drop(&mut self) {
        use windows::Win32::Foundation::*;
        unsafe {
            for handle in &self.0 {
                let _ = SetHandleInformation(*handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT);
            }
        }
    }
}
