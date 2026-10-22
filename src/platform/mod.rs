#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;

pub struct PresentedFile {
    pub name: String,
    pub directory: bool,
    pub modified_ms: Option<u64>,
    pub source: std::sync::Arc<dyn crate::core::ReadAt>,
}
