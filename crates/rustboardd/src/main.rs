#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        rustboard_platform::windows::clipboard::run()
    }
    #[cfg(not(windows))]
    anyhow::bail!("Native backend not installed on this platform");
}
