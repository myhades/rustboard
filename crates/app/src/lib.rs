#[cfg(any(windows, target_os = "macos"))]
pub mod cli;
#[cfg(any(windows, target_os = "macos"))]
mod config;
#[cfg(any(windows, target_os = "macos"))]
mod image_file;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(any(windows, target_os = "macos"))]
mod pairing;
#[cfg(windows)]
pub mod windows;

/// Clipboard and file presentation are separate responsibilities. Native implementations
/// own their event loop; content sources are portable, synchronous bounded ReadAt objects.
pub struct PresentedFile {
    pub name: String,
    pub source: std::sync::Arc<dyn rustboard_core::ReadAt>,
}

#[cfg(all(test, any(windows, target_os = "macos")))]
#[path = "../../../tests/support/pattern.rs"]
mod test_pattern;
