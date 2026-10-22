#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;

pub(crate) fn recycle(paths: &[std::path::PathBuf]) -> bool {
    #[cfg(windows)]
    {
        windows::recycle::recycle(paths).is_ok()
    }
    #[cfg(target_os = "macos")]
    {
        macos::local_files::recycle(paths)
    }
    #[cfg(target_os = "linux")]
    {
        linux::files::recycle(paths)
    }
}

pub struct PresentedFile {
    pub name: String,
    pub directory: bool,
    pub modified_ms: Option<u64>,
    pub source: std::sync::Arc<dyn crate::core::ReadAt>,
}
