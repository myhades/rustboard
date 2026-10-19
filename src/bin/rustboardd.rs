#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    let result = rustboard::platform::windows::clipboard::run();
    #[cfg(target_os = "macos")]
    let result = rustboard::platform::macos::run();
    #[cfg(not(any(windows, target_os = "macos")))]
    let result: anyhow::Result<()> = Err(anyhow::anyhow!(
        "Native backend not installed on this platform"
    ));
    match result {
        Ok(()) => {
            log::info!(target: "agent", "stopped");
            log::logger().flush();
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            if log::max_level() == log::LevelFilter::Off {
                eprintln!("rustboardd: {error:#}");
            } else {
                log::error!(target: "agent", "{error:#}");
                log::logger().flush();
            }
            std::process::ExitCode::FAILURE
        }
    }
}
