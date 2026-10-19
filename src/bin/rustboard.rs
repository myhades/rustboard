fn main() -> std::process::ExitCode {
    #[cfg(any(windows, target_os = "macos"))]
    let result = rustboard::app::cli::run(std::env::args().skip(1).collect());
    #[cfg(not(any(windows, target_os = "macos")))]
    let result: anyhow::Result<()> = Err(anyhow::anyhow!("native platform unavailable"));
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rustboard: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
