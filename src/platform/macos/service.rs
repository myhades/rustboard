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
    ensure!(p.exists(), "Rustboard is not installed");
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
    anyhow::bail!("Agent did not start; inspect agent.err.log")
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
    anyhow::bail!("Agent did not stop")
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
