use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME: usize = 32 * 1024 * 1024;
pub const MAX_MANIFEST: usize = 8 * 1024 * 1024;
pub const MAX_TEXT: usize = 4 * 1024 * 1024;
pub const MAX_CHUNK: usize = 64 * 1024;
pub const MAX_FILES: usize = 10_000;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IpcRequest {
    pub protocol: u16,
    pub request: IpcOperation,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum IpcOperation {
    Control {
        args: Vec<String>,
    },
    GetOffer,
    GetFileOffer {
        offer_id: String,
    },
    GetFileOffers,
    FileReceived {
        offer_id: String,
        file_id: String,
    },
    MoveItem {
        offer_id: String,
        file_id: String,
    },
    Retain {
        offer_id: String,
    },
    ReadBinary {
        background: bool,
        request_id: u64,
        offer_id: String,
        file_id: String,
        version: String,
        offset: u64,
        length: u32,
    },
    Cancel {
        request_id: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub directory: bool,
    pub modified_ms: Option<u64>,
    pub executable: Option<bool>,
    pub file_id: String,
    pub relative_path: String,
    pub size: Option<u64>,
    pub version: String,
    pub media_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Content {
    Text {
        text: String,
        html: Option<String>,
        rtf: Option<String>,
    },
    Image {
        image: FileEntry,
    },
    Files {
        files: Vec<FileEntry>,
    },
}

impl Content {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            html: None,
            rtf: None,
        }
    }
    pub fn entries(&self) -> &[FileEntry] {
        match self {
            Self::Files { files } => files,
            Self::Image { image } => std::slice::from_ref(image),
            Self::Text { .. } => &[],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub cut: bool,
    pub protocol: u16,
    pub source_device: String,
    pub offer_id: String,
    /// Lamport clock. Tie broken by source_device, then offer_id.
    pub event_sequence: u64,
    pub expires_unix_ms: u64,
    pub content: Content,
}

pub fn validate_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty() && path.len() <= 1024 && path.split('/').count() <= 64,
        "invalid path length"
    );
    for part in path.split('/') {
        ensure!(
            !part.is_empty() && part != "." && part != "..",
            "unsafe path component"
        );
        ensure!(!part.contains('\0'), "unsafe path character");
    }
    Ok(())
}

impl Offer {
    pub fn validate(&self, now: u64) -> Result<()> {
        ensure!(
            !self.cut || matches!(self.content, Content::Files { .. }),
            "invalid cut offer"
        );
        ensure!(self.protocol == PROTOCOL_VERSION, "unsupported protocol");
        uuid::Uuid::parse_str(&self.source_device)?;
        uuid::Uuid::parse_str(&self.offer_id)?;
        ensure!(
            self.expires_unix_ms > now && self.expires_unix_ms - now <= 86_400_000,
            "expired or excessive lifetime"
        );
        match &self.content {
            Content::Text { text, html, rtf } => {
                use base64::Engine;
                let rich = rtf
                    .as_ref()
                    .map(|v| base64::prelude::BASE64_STANDARD.decode(v))
                    .transpose()?;
                ensure!(
                    text.len()
                        + html.as_ref().map_or(0, String::len)
                        + rich.as_ref().map_or(0, Vec::len)
                        <= MAX_TEXT
                        && !text.contains('\0')
                        && !html.as_ref().is_some_and(|v| v.contains('\0')),
                    "invalid text"
                );
            }
            Content::Image { image } => {
                ensure!(
                    !image.directory
                        && image.size.is_some_and(|n| n > 0 && n <= 64 * 1024 * 1024)
                        && matches!(image.media_type.as_str(), "image/png" | "image/jpeg"),
                    "invalid image"
                );
                uuid::Uuid::parse_str(&image.file_id)?;
                ensure!(
                    !image.version.is_empty() && image.version.len() <= 128,
                    "invalid version"
                );
                validate_path(&image.relative_path)?;
            }
            Content::Files { files } => {
                ensure!(
                    !files.is_empty() && files.len() <= MAX_FILES,
                    "invalid file count"
                );
                ensure!(
                    serde_json::to_vec(files)?.len() <= MAX_MANIFEST,
                    "file manifest too large"
                );
                let mut ids = std::collections::HashSet::new();
                let mut paths = std::collections::HashSet::new();
                let entries: std::collections::HashMap<_, _> = files
                    .iter()
                    .map(|f| (f.relative_path.as_str(), f))
                    .collect();
                for file in files {
                    if let Some((parent, _)) = file.relative_path.rsplit_once('/') {
                        ensure!(
                            entries.get(parent).is_some_and(|p| p.directory),
                            "missing directory"
                        );
                    }
                    validate_path(&file.relative_path)?;
                    ensure!(
                        file.modified_ms.is_none_or(|v| v <= 253402300799999)
                            && file.size.is_none_or(|v| v <= i64::MAX as u64)
                            && file.media_type.len() <= 128,
                        "invalid file metadata"
                    );
                    ensure!(
                        !file.directory || file.size == Some(0),
                        "invalid directory size"
                    );
                    uuid::Uuid::parse_str(&file.file_id)?;
                    ensure!(
                        ids.insert(&file.file_id) && paths.insert(&file.relative_path),
                        "duplicate file"
                    );
                    ensure!(
                        !file.version.is_empty() && file.version.len() <= 128,
                        "invalid version"
                    );
                }
            }
        }
        Ok(())
    }
    pub fn order_key(&self) -> (u64, &str, &str) {
        (self.event_sequence, &self.source_device, &self.offer_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Message {
    Recycle {
        offer_id: String,
    },
    Retain {
        offer_id: String,
    },
    Ack,
    Offer {
        offer: Offer,
    },
    OfferRejected {
        offer_id: String,
        code: ErrorCode,
    },
    Ping {
        request_id: u64,
    },
    Pong {
        request_id: u64,
    },
    Read {
        background: bool,
        request_id: u64,
        source_device: String,
        offer_id: String,
        file_id: String,
        version: String,
        offset: u64,
        length: u32,
    },
    Data {
        request_id: u64,
        offset: u64,
        bytes: Vec<u8>,
        eof: bool,
    },
    Cancel {
        request_id: u64,
    },
    Error {
        request_id: Option<u64>,
        code: ErrorCode,
    },
    Hello {
        protocol: u16,
        device_id: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unauthorized,
    Expired,
    NotFound,
    VersionChanged,
    InvalidRequest,
    Cancelled,
    Busy,
    Io,
    Unsupported,
}

pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<()> {
    // One write prevents a separate tiny TLS record/TCP packet for the prefix.
    let mut bytes = vec![0; 4];
    serde_json::to_writer(&mut bytes, value)?;
    let length = bytes.len() - 4;
    ensure!(length <= MAX_FRAME, "frame too large");
    bytes[..4].copy_from_slice(&(length as u32).to_be_bytes());
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
pub fn read_frame<R: Read, T: serde::de::DeserializeOwned>(reader: &mut R) -> Result<T> {
    let mut size = [0; 4];
    reader.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if size == 0 || size > MAX_FRAME {
        bail!("invalid frame length");
    }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn offer(content: Content) -> Offer {
        Offer {
            cut: false,
            protocol: PROTOCOL_VERSION,
            source_device: super::super::engine::new_id(),
            offer_id: super::super::engine::new_id(),
            event_sequence: 1,
            expires_unix_ms: 1000,
            content,
        }
    }
    fn file(path: &str, directory: bool) -> FileEntry {
        FileEntry {
            file_id: super::super::engine::new_id(),
            relative_path: path.into(),
            size: Some(0),
            version: "v".into(),
            media_type: "application/octet-stream".into(),
            directory,
            modified_ms: None,
            executable: None,
        }
    }
    #[test]
    fn directory_manifest_bounds_and_parent_identity() {
        let mut files = vec![
            file("root", true),
            file("root/empty", true),
            file("root/file", false),
        ];
        offer(Content::Files {
            files: files.clone(),
        })
        .validate(0)
        .unwrap();
        files[0].directory = false;
        assert!(
            offer(Content::Files {
                files: files.clone()
            })
            .validate(0)
            .is_err()
        );
        files[0].directory = true;
        files[2].relative_path = "ROOT/file".into();
        assert!(offer(Content::Files { files }).validate(0).is_err());
        let mut files: Vec<_> = (0..MAX_FILES)
            .map(|i| file(&format!("{i}.txt"), false))
            .collect();
        offer(Content::Files {
            files: files.clone(),
        })
        .validate(0)
        .unwrap();
        files.push(file("one-too-many", false));
        assert!(offer(Content::Files { files }).validate(0).is_err());
    }
    #[test]
    fn rich_text_and_image_limits_are_validated_before_reading() {
        use base64::Engine;
        let content = Content::Text {
            text: "中文".into(),
            html: Some("<b>中文</b>".into()),
            rtf: Some(base64::prelude::BASE64_STANDARD.encode(b"{\\rtf1 text}")),
        };
        let offer = offer(content);
        offer.validate(0).unwrap();
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &offer).unwrap();
        assert_eq!(read_frame::<_, Offer>(&mut &bytes[..]).unwrap(), offer);
        let text = super::tests::offer(Content::text("\u{1}".repeat(MAX_TEXT)));
        text.validate(0).unwrap();
        write_frame(&mut Vec::new(), &text).unwrap();
        assert!(
            super::tests::offer(Content::text("x".repeat(MAX_TEXT + 1)))
                .validate(0)
                .is_err()
        );
        let mut image = file("image.png", false);
        image.media_type = "image/png".into();
        image.size = Some(64 * 1024 * 1024 + 1);
        assert!(
            super::tests::offer(Content::Image { image })
                .validate(0)
                .is_err()
        );
    }
    #[test]
    fn unsafe_paths() {
        for path in ["", "/etc/passwd", "../secret", "a/../b", "a\0b", "a//b"] {
            assert!(validate_path(path).is_err(), "{path}");
        }
        validate_path("folder/中文.txt").unwrap();
    }
    #[test]
    fn platform_names_are_preserved_in_offers() {
        let files = [
            "a:b", "a\\b", "CON.txt", "name.", "name ", "a?.txt", "A.txt", "a.txt",
        ]
        .map(|name| file(name, false))
        .to_vec();
        offer(Content::Files {
            files: files.clone(),
        })
        .validate(0)
        .unwrap();
        let mut duplicates = files;
        duplicates.push(file("a.txt", false));
        assert!(
            offer(Content::Files { files: duplicates })
                .validate(0)
                .is_err()
        );
    }
    #[test]
    fn framing_bound() {
        assert!(read_frame::<_, Message>(&mut &u32::MAX.to_be_bytes()[..]).is_err());
        let value = Message::Cancel { request_id: 42 };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &value).unwrap();
        assert!(matches!(
            read_frame(&mut &bytes[..]).unwrap(),
            Message::Cancel { request_id: 42 }
        ));
    }
}
