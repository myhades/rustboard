use anyhow::{Result, ensure};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn systemctl(args: &[&str]) -> Result<()> {
    let out = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()?;
    ensure!(
        out.status.success(),
        "systemctl: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(())
}
fn config_home() -> Result<PathBuf> {
    Ok(super::ipc::data_dir()?
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf())
}
pub fn enabled() -> Result<bool> {
    Ok(Command::new("systemctl")
        .args(["--user", "is-enabled", "rustboard.service"])
        .output()?
        .status
        .success())
}
pub fn start(register: bool) -> Result<()> {
    ensure!(
        super::ipc::profile()? == "default",
        "service commands target the default profile"
    );
    ensure!(
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some(),
        "desktop session required"
    );
    let agent = std::env::current_exe()?
        .parent()
        .unwrap()
        .join("rustboardd");
    ensure!(agent.is_file(), "agent is not installed");
    let home = config_home()?;
    let units = home.join("systemd/user");
    std::fs::create_dir_all(&units)?;
    let path = agent
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid installation path"))?;
    ensure!(
        !path.contains(['\n', '\r', '"', '\\']),
        "unsupported installation path"
    );
    std::fs::write(
        units.join("rustboard.service"),
        format!(
            "[Unit]\nDescription=Rustboard clipboard synchronization\nPartOf=graphical-session.target\n\n[Service]\nExecStart=\"{}\"\nRestart=on-failure\nRestartSec=2\nUMask=0077\nTimeoutStopSec=10\n\n[Install]\nWantedBy=graphical-session.target\n",
            path.replace('%', "%%")
        ),
    )?;
    systemctl(&["daemon-reload"])?;
    for name in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_SESSION_TYPE",
    ] {
        if std::env::var_os(name).is_some() {
            systemctl(&["import-environment", name])?;
        }
    }
    if register {
        systemctl(&["enable", "rustboard.service"])?;
        // Xfce does not start graphical-session.target; its XDG autostart imports the session environment.
        let autostart = home.join("autostart");
        std::fs::create_dir_all(&autostart)?;
        let cli = std::env::current_exe()?;
        std::fs::write(
            autostart.join("rustboard.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nName=Rustboard\nExec=\"{}\" start\nNoDisplay=true\nTerminal=false\n",
                cli.display().to_string().replace('%', "%%")
            ),
        )?;
    }
    systemctl(&["start", "rustboard.service"])?;
    for _ in 0..100 {
        if crate::app::cli::request(&["status"]).is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    anyhow::bail!("agent failed to start")
}
pub fn stop(unregister: bool) -> Result<()> {
    ensure!(
        super::ipc::profile()? == "default",
        "service commands target the default profile"
    );
    if unregister {
        let path = config_home()?.join("autostart/rustboard.desktop");
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        if enabled()? {
            systemctl(&["disable", "rustboard.service"])?;
        }
    }
    if config_home()?
        .join("systemd/user/rustboard.service")
        .exists()
    {
        systemctl(&["stop", "rustboard.service"])?;
    }
    Ok(())
}
pub fn edit(path: &Path) -> Result<()> {
    if let Ok(editor) = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")) {
        ensure!(
            Command::new(editor).arg(path).status()?.success(),
            "editor failed"
        );
    } else {
        ensure!(
            Command::new("xdg-open").arg(path).status()?.success(),
            "cannot open text editor"
        );
    }
    Ok(())
}

static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
extern "C" fn interrupt(_: libc::c_int) {
    INTERRUPTED.store(true, std::sync::atomic::Ordering::Relaxed);
}
pub(crate) struct PairInterrupt(libc::sighandler_t);
impl PairInterrupt {
    pub(crate) fn install() -> Result<Self> {
        INTERRUPTED.store(false, std::sync::atomic::Ordering::Relaxed);
        let previous =
            unsafe { libc::signal(libc::SIGINT, interrupt as *const () as libc::sighandler_t) };
        ensure!(previous != libc::SIG_ERR, "cannot handle interrupt");
        Ok(Self(previous))
    }
    pub(crate) fn interrupted(&self) -> bool {
        INTERRUPTED.load(std::sync::atomic::Ordering::Relaxed)
    }
}
impl Drop for PairInterrupt {
    fn drop(&mut self) {
        unsafe {
            libc::signal(libc::SIGINT, self.0);
        }
    }
}
