//! Current-user startup registration and background process lifecycle.
use anyhow::{Result, ensure};
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
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
    let out = command("reg.exe")
        .args(["query", KEY, "/v", "Rustboard"])
        .output()?;
    Ok(out.status.success())
}
pub fn start(register: bool) -> Result<()> {
    let base = base()?;
    let agent = base.join("bin/rustboardd.exe");
    ensure!(agent.is_file(), "Rustboard is not installed");
    if register {
        ensure!(
            command("reg.exe")
                .args([
                    "add",
                    KEY,
                    "/v",
                    "Rustboard",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &format!("\"{}\"", agent.display()),
                    "/f"
                ])
                .output()?
                .status
                .success(),
            "failed to enable login startup"
        );
    }
    if crate::cli::request(&["status"]).is_ok() {
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
        if crate::cli::request(&["status"]).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!("Agent did not start; inspect agent.err.log (login startup may remain enabled)")
}
fn running() -> Result<bool> {
    let script = "$p=Join-Path $env:LOCALAPPDATA 'Rustboard/bin/rustboardd.exe'; @(Get-Process rustboardd -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq $p}).Count";
    let out = command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()?;
    ensure!(out.status.success(), "cannot inspect Agent processes");
    Ok(String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<usize>()?
        > 0)
}
pub fn stop(unregister: bool) -> Result<()> {
    base()?;
    if unregister && enabled()? {
        ensure!(
            command("reg.exe")
                .args(["delete", KEY, "/v", "Rustboard", "/f"])
                .output()?
                .status
                .success(),
            "failed to disable login startup"
        );
    }
    if running()? {
        crate::cli::request(&["stop"])?;
    }
    for _ in 0..10 {
        if !running()? {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    anyhow::bail!("Agent did not stop")
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
