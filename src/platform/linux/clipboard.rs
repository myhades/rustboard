mod gnome;
mod x11;

use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};

pub enum Captured {
    Text(String),
    Files(Vec<PathBuf>),
    Image(Vec<u8>, &'static str),
    Unsupported,
}

pub enum Clipboard {
    X11(Box<x11::Clipboard>),
    Gnome(Box<gnome::Clipboard>),
}
impl Clipboard {
    pub fn new() -> Result<Self> {
        if std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland") {
            Ok(Self::Gnome(Box::new(gnome::Clipboard::new()?)))
        } else {
            Ok(Self::X11(Box::new(x11::Clipboard::new()?)))
        }
    }
    pub fn poll(&mut self) -> Result<Option<Captured>> {
        match self {
            Self::X11(c) => c.poll(),
            Self::Gnome(c) => c.poll(),
        }
    }
    pub fn publish_text(&mut self, text: &str) -> Result<()> {
        match self {
            Self::X11(c) => c.publish_text(text),
            Self::Gnome(c) => c.publish_text(text),
        }
    }
    pub fn publish_files(&mut self, paths: &[PathBuf]) -> Result<()> {
        match self {
            Self::X11(c) => c.publish_files(paths),
            Self::Gnome(c) => c.publish_files(paths),
        }
    }
    pub fn clear_owned(&mut self) -> Result<()> {
        match self {
            Self::X11(c) => c.clear_owned(),
            Self::Gnome(c) => c.clear_owned(),
        }
    }
}

fn capture(mime: &str, bytes: Vec<u8>) -> Result<Captured> {
    ensure!(
        bytes.len() <= crate::app::image_file::MAX_IMAGE_BYTES,
        "clipboard data too large"
    );
    match mime {
        "image/png" => return Ok(Captured::Image(bytes, "png")),
        "image/jpeg" => return Ok(Captured::Image(bytes, "jpg")),
        _ => {}
    }
    let text = String::from_utf8(bytes)?;
    if matches!(mime, "text/uri-list" | "x-special/gnome-copied-files") {
        let paths = text
            .lines()
            .filter(|s| !s.is_empty() && !s.starts_with('#') && *s != "copy" && *s != "cut")
            .map(parse_uri)
            .collect::<Result<Vec<_>>>()?;
        ensure!(paths.len() <= 256, "too many files");
        return Ok(Captured::Files(paths));
    }
    ensure!(
        text.len() <= 262144 && !text.contains('\0'),
        "invalid clipboard text"
    );
    Ok(Captured::Text(text))
}

pub fn uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            out.push(b as char);
        } else {
            use std::fmt::Write;
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}
fn parse_uri(value: &str) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let value = value
        .strip_prefix("file://")
        .ok_or_else(|| anyhow::anyhow!("local file URI required"))?;
    let value = value
        .strip_prefix("localhost/")
        .map(|v| format!("/{v}"))
        .unwrap_or_else(|| value.into());
    ensure!(value.starts_with('/'), "remote URI unsupported");
    let mut bytes = Vec::new();
    let mut i = 0;
    while i < value.len() {
        if value.as_bytes()[i] == b'%' {
            let hex = value
                .get(i + 1..i + 3)
                .ok_or_else(|| anyhow::anyhow!("invalid URI escape"))?;
            bytes.push(u8::from_str_radix(hex, 16)?);
            i += 3;
        } else {
            bytes.push(value.as_bytes()[i]);
            i += 1;
        }
    }
    ensure!(!bytes.contains(&0), "invalid URI");
    Ok(std::ffi::OsString::from_vec(bytes).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_uri_roundtrip_and_rejection() {
        let path = Path::new("/tmp/中文 # % image.png");
        assert_eq!(parse_uri(&uri(path)).unwrap(), path);
        for bad in [
            "https://host/a",
            "file://host/a",
            "file:///a%",
            "file:///a%00",
        ] {
            assert!(parse_uri(bad).is_err());
        }
    }
}
