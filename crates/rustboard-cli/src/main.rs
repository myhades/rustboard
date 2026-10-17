fn main() -> anyhow::Result<()> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        rustboard_platform::cli::run(std::env::args().skip(1).collect())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    anyhow::bail!("Native platform unavailable");
}
