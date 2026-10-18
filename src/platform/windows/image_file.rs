use anyhow::{Result, ensure};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        Foundation::HGLOBAL,
        System::{DataExchange::*, Memory::*},
    },
    core::w,
};

pub struct Image {
    bytes: Vec<u8>,
    png: bool,
}
/// Caller holds OpenClipboard. Copy bounded encoded bytes; do not decode pixels.
pub fn capture() -> Result<Option<Image>> {
    unsafe {
        let png = RegisterClipboardFormatW(w!("PNG"));
        for format in [png, 17, 8] {
            if format == 0 || IsClipboardFormatAvailable(format).is_err() {
                continue;
            }
            let handle = HGLOBAL(GetClipboardData(format)?.0);
            let size = GlobalSize(handle);
            ensure!(
                size > 0 && size <= crate::app::image_file::MAX_IMAGE_BYTES,
                "clipboard image exceeds 64 MiB"
            );
            let pointer = GlobalLock(handle);
            ensure!(!pointer.is_null(), "cannot lock image");
            let bytes = std::slice::from_raw_parts(pointer.cast::<u8>(), size).to_vec();
            let _ = GlobalUnlock(handle);
            return Ok(Some(Image {
                bytes,
                png: format == png,
            }));
        }
    }
    Ok(None)
}
fn bitmap_header(dib: &[u8]) -> Result<[u8; 14]> {
    ensure!(dib.len() >= 40, "short DIB");
    let u32_at = |n| u32::from_le_bytes(dib[n..n + 4].try_into().unwrap());
    let header = u32_at(0) as usize;
    ensure!(
        [40, 52, 56, 108, 124].contains(&header) && dib.len() >= header,
        "unsupported DIB header"
    );
    let width = u32_at(4) as i32;
    let height = u32_at(8) as i32;
    let planes = u16::from_le_bytes(dib[12..14].try_into().unwrap());
    let bits = u16::from_le_bytes(dib[14..16].try_into().unwrap()) as usize;
    let compression = u32_at(16);
    ensure!(
        width > 0
            && height != 0
            && height != i32::MIN
            && planes == 1
            && [1, 4, 8, 16, 24, 32].contains(&bits),
        "invalid DIB dimensions"
    );
    ensure!(
        compression == 0 || ([3, 6].contains(&compression) && [16, 32].contains(&bits)),
        "compressed DIB unsupported"
    );
    if header == 124 {
        ensure!(
            u32_at(112) == 0 && u32_at(116) == 0,
            "embedded DIB profiles unsupported"
        );
    }
    let colors = u32_at(32) as usize;
    let colors = if colors == 0 && bits <= 8 {
        1 << bits
    } else {
        colors
    };
    ensure!(colors <= 256, "DIB palette limit");
    let masks = if header == 40 {
        match compression {
            3 => 12,
            6 => 16,
            _ => 0,
        }
    } else {
        0
    };
    let offset = header + masks + colors * 4;
    let stride = (width as u64 * bits as u64).div_ceil(32) * 4;
    ensure!(
        offset as u64 + stride * height.unsigned_abs() as u64 <= dib.len() as u64,
        "truncated DIB pixels"
    );
    let mut result = [0; 14];
    result[..2].copy_from_slice(b"BM");
    result[2..6].copy_from_slice(&u32::try_from(dib.len() + 14)?.to_le_bytes());
    result[10..14].copy_from_slice(&u32::try_from(offset + 14)?.to_le_bytes());
    Ok(result)
}
impl Image {
    pub fn save(self, dir: &Path) -> Result<PathBuf> {
        let header = if self.png {
            ensure!(
                self.bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
                "invalid PNG signature"
            );
            None
        } else {
            Some(bitmap_header(&self.bytes)?)
        };
        let extension = if self.png { "png" } else { "bmp" };
        let path = dir.join(format!(
            "clipboard-{}.{}",
            crate::core::engine::new_id(),
            extension
        ));
        let result = (|| -> Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            if let Some(header) = header {
                file.write_all(&header)?;
            }
            file.write_all(&self.bytes)?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = std::fs::remove_file(&path);
            return Err(error);
        }
        Ok(path)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dib_file_header_and_bounds() {
        let mut dib = vec![0; 44];
        dib[..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&1i32.to_le_bytes());
        dib[8..12].copy_from_slice(&(-1i32).to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&32u16.to_le_bytes());
        let h = bitmap_header(&dib).unwrap();
        assert_eq!(&h[..2], b"BM");
        assert_eq!(u32::from_le_bytes(h[10..14].try_into().unwrap()), 54);
        assert!(bitmap_header(&dib[..43]).is_err());
        dib[4..8].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(bitmap_header(&dib).is_err());
    }
}
