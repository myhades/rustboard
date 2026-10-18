use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME: usize = 1024 * 1024;
pub const MAX_CHUNK: usize = 64 * 1024;
pub const MAX_FILES: usize = 256;

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
    ReadBinary {
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
    pub file_id: String,
    pub relative_path: String,
    pub size: Option<u64>,
    pub version: String,
    pub media_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Content {
    Text { text: String },
    Files { files: Vec<FileEntry> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Offer {
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
        !path.is_empty() && path.len() <= 1024,
        "invalid path length"
    );
    for part in path.split('/') {
        ensure!(
            !part.is_empty() && part != "." && part != "..",
            "unsafe path component"
        );
        ensure!(!part.ends_with([' ', '.']), "unsafe path suffix");
        ensure!(
            !part
                .chars()
                .any(|c| c.is_control() || "\\:<>\"|?*".contains(c)),
            "unsafe path character"
        );
        let stem = part.split('.').next().unwrap().to_ascii_uppercase();
        let numbered_device = stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| suffix.chars().count() == 1 && "123456789¹²³".contains(suffix));
        ensure!(
            !["CON", "PRN", "AUX", "NUL", "CLOCK$", "CONIN$", "CONOUT$"].contains(&stem.as_str())
                && !numbered_device,
            "reserved device name"
        );
    }
    Ok(())
}

impl Offer {
    pub fn validate(&self, now: u64) -> Result<()> {
        ensure!(self.protocol == PROTOCOL_VERSION, "unsupported protocol");
        uuid::Uuid::parse_str(&self.source_device)?;
        uuid::Uuid::parse_str(&self.offer_id)?;
        ensure!(
            self.expires_unix_ms > now && self.expires_unix_ms - now <= 86_400_000,
            "expired or excessive lifetime"
        );
        match &self.content {
            Content::Text { text } => ensure!(
                text.len() <= 256 * 1024 && !text.contains('\0'),
                "invalid text"
            ),
            Content::Files { files } => {
                ensure!(
                    !files.is_empty() && files.len() <= MAX_FILES,
                    "invalid file count"
                );
                let mut ids = std::collections::HashSet::new();
                let mut paths = std::collections::HashSet::new();
                for file in files {
                    validate_path(&file.relative_path)?;
                    uuid::Uuid::parse_str(&file.file_id)?;
                    ensure!(
                        ids.insert(&file.file_id)
                            && paths.insert(file.relative_path.to_lowercase()),
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
    Poll,
    Idle,
    Ack,
    Offer {
        offer: Offer,
    },
    Read {
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
    #[test]
    fn unsafe_paths() {
        for path in [
            "",
            "/etc/passwd",
            "../secret",
            "a/../b",
            "C:/x",
            "a\\b",
            "a:b",
            "a\0b",
            "CON.txt",
            "COM¹.txt",
            "CONIN$",
            "x.",
            "a//b",
        ] {
            assert!(validate_path(path).is_err(), "{path}");
        }
        validate_path("folder/中文.txt").unwrap();
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
