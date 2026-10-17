#[cfg(any(windows, target_os = "macos"))]
mod config;
#[cfg(any(windows, target_os = "macos"))]
mod image_file;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;

/// Clipboard and file presentation are separate responsibilities. Native implementations
/// own their event loop; content sources are portable, synchronous bounded ReadAt objects.
pub struct PresentedFile {
    pub name: String,
    pub source: std::sync::Arc<dyn rustboard_core::ReadAt>,
}

/// Implement on the platform's clipboard thread (STA on Windows, AppKit main thread on macOS).
/// Native listener feeds local changes into Engine; publishing must suppress its own echo.
pub trait ClipboardBackend {
    fn publish_text(&mut self, text: &str) -> anyhow::Result<()>;
}

/// Deliberately separate from text. Windows exports streams; macOS publishes dataless URLs.
pub trait FilePresentationBackend {
    fn publish_files(&mut self, files: Vec<PresentedFile>) -> anyhow::Result<()>;
}
