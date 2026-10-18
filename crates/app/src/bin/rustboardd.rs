#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        rustboard_app::windows::clipboard::run()
    }
    #[cfg(target_os = "macos")]
    {
        rustboard_app::macos::run()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    anyhow::bail!("Native backend not installed on this platform");
}
