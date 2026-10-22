mod data_control;
mod gnome;
mod pipe;
mod x11;

use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};

pub enum Captured {
    Text(crate::core::Content),
    Files(Vec<PathBuf>, bool),
    Image(Vec<u8>, &'static str),
    Unsupported,
}

pub enum Clipboard {
    X11(Box<x11::Clipboard>),
    Gnome(Box<gnome::Clipboard>),
    DataControl(Box<data_control::Clipboard>),
}
impl Clipboard {
    pub fn new() -> Result<Self> {
        if std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland") {
            match data_control::Clipboard::connect()? {
                Some(clipboard) => Ok(Self::DataControl(Box::new(clipboard))),
                None => match gnome::Clipboard::connect()? {
                    Some(clipboard) => Ok(Self::Gnome(Box::new(clipboard))),
                    None => anyhow::bail!(
                        "desktop must provide ext-data-control-v1 or GNOME clipboard access"
                    ),
                },
            }
        } else {
            Ok(Self::X11(Box::new(x11::Clipboard::new()?)))
        }
    }
    pub fn poll(&mut self) -> Result<Option<Captured>> {
        match self {
            Self::X11(c) => c.poll(),
            Self::Gnome(c) => c.poll(),
            Self::DataControl(c) => c.poll(),
        }
    }
    pub fn publish_content(
        &mut self,
        content: &crate::core::Content,
        image: Option<Vec<u8>>,
    ) -> Result<()> {
        use base64::Engine;
        use std::{collections::HashMap, sync::Arc};
        let mut formats = HashMap::new();
        match content {
            crate::core::Content::Text { text, html, rtf } => {
                let bytes = Arc::new(text.as_bytes().to_vec());
                for mime in ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"] {
                    formats.insert(mime.into(), bytes.clone());
                }
                if let Some(html) = html {
                    formats.insert("text/html".into(), Arc::new(html.as_bytes().to_vec()));
                }
                if let Some(rtf) = rtf {
                    let bytes = Arc::new(base64::prelude::BASE64_STANDARD.decode(rtf)?);
                    formats.insert("text/rtf".into(), bytes.clone());
                    formats.insert("application/rtf".into(), bytes);
                }
            }
            crate::core::Content::Image { image: entry } => {
                formats.insert(entry.media_type.clone(), Arc::new(image.unwrap()));
            }
            _ => anyhow::bail!("not clipboard data"),
        }
        match self {
            Self::X11(c) => c.publish_formats(formats),
            Self::Gnome(c) => c.publish_formats(formats),
            Self::DataControl(c) => c.publish_formats(formats),
        }
    }
    pub fn publish_files(&mut self, paths: &[PathBuf], cut: bool) -> Result<()> {
        match self {
            Self::X11(c) => c.publish_files(paths, cut),
            Self::Gnome(c) => c.publish_files(paths, cut),
            Self::DataControl(c) => c.publish_files(paths, cut),
        }
    }
    pub fn clear_owned(&mut self) -> Result<()> {
        match self {
            Self::X11(c) => c.clear_owned(),
            Self::Gnome(c) => c.clear_owned(),
            Self::DataControl(c) => c.clear_owned(),
        }
    }
}

fn capture_types(types: &[&str]) -> Vec<&'static str> {
    let first = |names: &[&'static str]| names.iter().copied().find(|m| types.contains(m));
    if let Some(mime) = first(&["x-special/gnome-copied-files"]) {
        return vec![mime];
    }
    if let Some(mime) = first(&["text/uri-list"]) {
        return std::iter::once(mime)
            .chain(first(&["application/x-kde-cutselection"]))
            .collect();
    }
    let html = first(&["text/html"]);
    let rtf = first(&["text/rtf", "application/rtf"]);
    if html.is_none()
        && rtf.is_none()
        && let Some(mime) = first(&["image/png", "image/jpeg"])
    {
        return vec![mime];
    }
    first(&["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"])
        .into_iter()
        .chain(html)
        .chain(rtf)
        .collect()
}
fn merge(captured: &mut Option<Captured>, value: Captured) {
    if let (Some(Captured::Files(paths, cut)), Captured::Files(other, moving)) =
        (&mut *captured, &value)
    {
        if !other.is_empty() {
            *paths = other.clone();
        }
        *cut |= moving;
        return;
    }
    if let (
        Some(Captured::Text(crate::core::Content::Text { text, html, rtf })),
        Captured::Text(crate::core::Content::Text {
            text: other,
            html: h,
            rtf: r,
        }),
    ) = (&mut *captured, &value)
    {
        if !other.is_empty() {
            *text = other.clone();
        }
        if h.is_some() {
            *html = h.clone();
        }
        if r.is_some() {
            *rtf = r.clone();
        }
    } else {
        *captured = Some(value);
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
    if matches!(mime, "text/rtf" | "application/rtf") {
        use base64::Engine;
        ensure!(bytes.len() <= crate::core::MAX_TEXT, "rich text too large");
        return Ok(Captured::Text(crate::core::Content::Text {
            text: String::new(),
            html: None,
            rtf: Some(base64::prelude::BASE64_STANDARD.encode(bytes)),
        }));
    }
    let text = String::from_utf8(bytes)?;
    if mime == "application/x-kde-cutselection" {
        return Ok(Captured::Files(Vec::new(), text.trim() == "1"));
    }
    if mime == "text/html" {
        ensure!(
            text.len() <= crate::core::MAX_TEXT && !text.contains('\0'),
            "HTML too large"
        );
        return Ok(Captured::Text(crate::core::Content::Text {
            text: String::new(),
            html: Some(text),
            rtf: None,
        }));
    }
    if matches!(mime, "text/uri-list" | "x-special/gnome-copied-files") {
        let paths = text
            .lines()
            .filter(|s| !s.is_empty() && !s.starts_with('#') && *s != "copy" && *s != "cut")
            .map(parse_uri)
            .collect::<Result<Vec<_>>>()?;
        ensure!(paths.len() <= crate::core::MAX_FILES, "too many files");
        return Ok(Captured::Files(
            paths,
            mime == "x-special/gnome-copied-files" && text.lines().next() == Some("cut"),
        ));
    }
    ensure!(
        text.len() <= crate::core::MAX_TEXT && !text.contains('\0'),
        "invalid clipboard text"
    );
    Ok(Captured::Text(crate::core::Content::text(text)))
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
    fn cut_markers_are_captured_with_their_file_list() {
        assert_eq!(
            capture_types(&["text/uri-list", "x-special/gnome-copied-files"]),
            ["x-special/gnome-copied-files"]
        );
        let Captured::Files(paths, cut) = capture(
            "x-special/gnome-copied-files",
            b"cut\nfile:///tmp/test".to_vec(),
        )
        .unwrap() else {
            panic!()
        };
        assert!(cut);
        assert_eq!(paths, [PathBuf::from("/tmp/test")]);
        let mut captured = Some(capture("text/uri-list", b"file:///tmp/test".to_vec()).unwrap());
        merge(
            &mut captured,
            capture("application/x-kde-cutselection", b"1".to_vec()).unwrap(),
        );
        assert!(matches!(captured, Some(Captured::Files(paths, true)) if paths.len() == 1));
    }
    #[test]
    fn representations_are_grouped_without_converting_content() {
        let types = capture_types(&["image/png", "text/html", "text/plain", "text/rtf"]);
        assert_eq!(types, ["text/plain", "text/html", "text/rtf"]);
        assert_eq!(
            capture_types(&["text/uri-list", "image/png"]),
            ["text/uri-list"]
        );
        assert_eq!(capture_types(&["image/png", "text/plain"]), ["image/png"]);
        let mut result = None;
        for (mime, bytes) in [
            ("text/html", b"<b>hello</b>".as_slice()),
            ("text/plain", b"hello"),
            ("text/rtf", b"{\\rtf1 hello}"),
        ] {
            merge(&mut result, capture(mime, bytes.to_vec()).unwrap());
        }
        let Some(Captured::Text(crate::core::Content::Text { text, html, rtf })) = result else {
            panic!()
        };
        assert_eq!(text, "hello");
        assert_eq!(html.as_deref(), Some("<b>hello</b>"));
        assert!(rtf.is_some());
    }
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
