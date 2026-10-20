//! launchd owns both immediate lifetime and login enablement.
use anyhow::{Result, ensure};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
fn domain() -> String {
    format!("gui/{}", unsafe { libc::getuid() })
}
fn label() -> String {
    format!("{}/org.rustboard.agent", domain())
}
fn plist() -> Result<PathBuf> {
    ensure!(
        super::ipc::profile()? == "default",
        "service commands target installed default profile only"
    );
    Ok(
        PathBuf::from(std::env::var("HOME")?)
            .join("Library/LaunchAgents/org.rustboard.agent.plist"),
    )
}
fn launch(args: &[&str]) -> Result<()> {
    let o = Command::new("/bin/launchctl").args(args).output()?;
    ensure!(
        o.status.success(),
        "launchctl: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    Ok(())
}
pub fn enabled() -> Result<bool> {
    if !plist()?.exists() {
        return Ok(false);
    }
    let o = Command::new("/bin/launchctl")
        .args(["print-disabled", &domain()])
        .output()?;
    ensure!(o.status.success(), "cannot query launchd");
    Ok(!disabled(&String::from_utf8_lossy(&o.stdout)))
}
pub fn start(register: bool) -> Result<()> {
    let p = plist()?;
    let home = PathBuf::from(std::env::var("HOME")?);
    let base = home.join("Library/Application Support/Rustboard");
    let app = home.join("Applications/Rustboard.app");
    ensure!(
        base.join("bin/rustboardd").is_file() && app.join("Contents/MacOS/Rustboard").is_file(),
        "agent is not installed"
    );
    if crate::app::cli::request(&["status"]).is_err() {
        let output = Command::new("/usr/bin/pluginkit")
            .arg("-a")
            .arg(app.join("Contents/PlugIns/RustboardProvider.appex"))
            .output()?;
        ensure!(
            output.status.success(),
            "File Provider registration failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        let output = Command::new(app.join("Contents/MacOS/Rustboard"))
            .arg("register")
            .output()?;
        ensure!(
            output.status.success(),
            "File Provider initialization failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        let output = Command::new("python3")
            .args([
                "-c",
                include_str!("../../../platforms/macos/scripts/launchd.py"),
            ])
            .arg(&base)
            .arg(&app)
            .arg(&p)
            .output()?;
        ensure!(
            output.status.success(),
            "cannot create login service: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    if register {
        launch(&["enable", &label()])?;
    }
    let loaded = Command::new("/bin/launchctl")
        .args(["print", &label()])
        .output()?
        .status
        .success();
    if !loaded {
        launch(&[
            "bootstrap",
            &domain(),
            p.to_str()
                .ok_or_else(|| anyhow::anyhow!("invalid plist path"))?,
        ])?;
    }
    if crate::app::cli::request(&["status"]).is_err() {
        launch(&["kickstart", &label()])?;
    }
    for _ in 0..50 {
        if crate::app::cli::request(&["status"]).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!("agent failed to start")
}
pub fn stop(unregister: bool) -> Result<()> {
    plist()?;
    if unregister {
        launch(&["disable", &label()])?;
    }
    let loaded = Command::new("/bin/launchctl")
        .args(["print", &label()])
        .output()?
        .status
        .success();
    if loaded {
        launch(&["bootout", &label()])?;
    }
    for _ in 0..50 {
        if crate::app::cli::request(&["status"]).is_err() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!("agent failed to stop")
}
pub fn edit(path: &Path) -> Result<()> {
    if let Ok(editor) = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")) {
        Command::new(editor).arg(path).spawn()?;
    } else {
        ensure!(
            Command::new("/usr/bin/open")
                .arg("-t")
                .arg(path)
                .status()?
                .success(),
            "cannot open text editor"
        );
    }
    Ok(())
}

fn disabled(output: &str) -> bool {
    output
        .lines()
        .filter_map(|line| line.split_once("=>"))
        .any(|(name, value)| {
            name.trim() == "\"org.rustboard.agent\"" && matches!(value.trim(), "true" | "disabled")
        })
}
static PAIR_INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn pair_interrupt(_: libc::c_int) {
    PAIR_INTERRUPTED.store(true, std::sync::atomic::Ordering::Relaxed);
}
pub(crate) struct PairInterrupt(libc::sighandler_t);
impl PairInterrupt {
    pub(crate) fn install() -> Result<Self> {
        PAIR_INTERRUPTED.store(false, std::sync::atomic::Ordering::Relaxed);
        let previous = unsafe {
            libc::signal(
                libc::SIGINT,
                pair_interrupt as *const () as libc::sighandler_t,
            )
        };
        ensure!(
            previous != libc::SIG_ERR,
            "cannot handle interrupt: {}",
            std::io::Error::last_os_error()
        );
        Ok(Self(previous))
    }
    pub(crate) fn interrupted(&self) -> bool {
        PAIR_INTERRUPTED.load(std::sync::atomic::Ordering::Relaxed)
    }
}
impl Drop for PairInterrupt {
    fn drop(&mut self) {
        unsafe {
            libc::signal(libc::SIGINT, self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launchd_disabled_formats() {
        assert!(disabled("\"org.rustboard.agent\" => disabled"));
        assert!(disabled("\"org.rustboard.agent\" => true"));
        assert!(!disabled("\"org.rustboard.agent\" => enabled"));
        assert!(!disabled("\"other\" => disabled"));
    }
}
