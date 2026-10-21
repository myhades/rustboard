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
    encoding: &'static str,
}
/// Caller holds OpenClipboard. Copy bounded encoded bytes; do not decode pixels.
pub fn capture() -> Result<Option<Image>> {
    unsafe {
        let png = RegisterClipboardFormatW(w!("PNG"));
        for (format, encoding) in [
            (png, "png"),
            (RegisterClipboardFormatW(w!("JFIF")), "jpg"),
            (17, "dib"),
            (8, "dib"),
        ] {
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
            return Ok(Some(Image { bytes, encoding }));
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
        let header = if self.encoding != "dib" {
            ensure!(
                (self.encoding == "png" && self.bytes.starts_with(b"\x89PNG\r\n\x1a\n"))
                    || (self.encoding == "jpg" && self.bytes.starts_with(&[0xff, 0xd8, 0xff])),
                "invalid image signature"
            );
            None
        } else {
            Some(bitmap_header(&self.bytes)?)
        };
        let bytes = if let Some(header) = header {
            let mut bmp = header.to_vec();
            bmp.extend_from_slice(&self.bytes);
            encode_png(&bmp)?
        } else {
            self.bytes
        };
        let extension = if self.encoding == "jpg" { "jpg" } else { "png" };
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
            file.write_all(&bytes)?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = std::fs::remove_file(&path);
            return Err(error);
        }
        Ok(path)
    }
}
fn decode(
    bytes: &[u8],
) -> Result<(
    windows::Win32::Graphics::Imaging::IWICImagingFactory,
    windows::Win32::Graphics::Imaging::IWICBitmapFrameDecode,
    u32,
    u32,
)> {
    use windows::Win32::{Graphics::Imaging::*, System::Com::*};
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = factory.CreateStream()?;
        stream.InitializeFromMemory(bytes)?;
        let decoder = factory.CreateDecoderFromStream(
            &stream,
            std::ptr::null(),
            WICDecodeMetadataCacheOnLoad,
        )?;
        let frame = decoder.GetFrame(0)?;
        let (mut width, mut height) = (0, 0);
        frame.GetSize(&mut width, &mut height)?;
        ensure!(
            width > 0
                && height > 0
                && u64::from(width) * u64::from(height) * 4
                    <= crate::app::image_file::MAX_IMAGE_BYTES as u64,
            "image dimensions too large"
        );
        Ok((factory, frame, width, height))
    }
}
fn encode_png(bytes: &[u8]) -> Result<Vec<u8>> {
    use windows::Win32::{
        Graphics::Imaging::*,
        System::Com::{StructuredStorage::*, *},
    };
    let (factory, source, width, height) = decode(bytes)?;
    unsafe {
        let stream = CreateStreamOnHGlobal(HGLOBAL::default(), true)?;
        let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
        let frame = frame.unwrap();
        frame.Initialize(None)?;
        frame.SetSize(width, height)?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut format)?;
        frame.WriteSource(&source, std::ptr::null())?;
        frame.Commit()?;
        encoder.Commit()?;
        let mut stat = STATSTG::default();
        stream.Stat(&mut stat, STATFLAG_NONAME)?;
        ensure!(
            stat.cbSize <= crate::app::image_file::MAX_IMAGE_BYTES as u64,
            "encoded image too large"
        );
        stream.Seek(0, STREAM_SEEK_SET, None)?;
        let mut bytes = vec![0; stat.cbSize as usize];
        let mut read = 0;
        stream
            .Read(
                bytes.as_mut_ptr().cast(),
                bytes.len() as u32,
                Some(&mut read),
            )
            .ok()?;
        ensure!(read as usize == bytes.len(), "incomplete image encoding");
        Ok(bytes)
    }
}
/// DIBV5 is supplied alongside encoded PNG/JPEG for native applications such as Paint.
pub fn dib(bytes: &[u8]) -> Result<Vec<u8>> {
    use windows::Win32::Graphics::Imaging::*;
    let (factory, source, width, height) = decode(bytes)?;
    let mut dib = vec![0; 124 + width as usize * height as usize * 4];
    for (offset, value) in [
        (0, 124u32),
        (4, width),
        (8, (-(height as i32)) as u32),
        (16, 3),
        (40, 0x00ff0000),
        (44, 0x0000ff00),
        (48, 0x000000ff),
        (52, 0xff000000),
        (56, 0x73524742),
    ] {
        dib[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    dib[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib[14..16].copy_from_slice(&32u16.to_le_bytes());
    unsafe {
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            &source,
            &GUID_WICPixelFormat32bppBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        converter.CopyPixels(std::ptr::null(), width * 4, &mut dib[124..])?;
    }
    Ok(dib)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_codec_roundtrip_preserves_alpha_and_orientation() {
        use windows::Win32::System::Com::*;
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap();
        }
        let mut bitmap = vec![0; 124 + 16];
        for (offset, value) in [
            (0, 124u32),
            (4, 2),
            (8, (-2i32) as u32),
            (16, 3),
            (40, 0x00ff0000),
            (44, 0x0000ff00),
            (48, 0xff),
            (52, 0xff000000),
            (56, 0x73524742),
        ] {
            bitmap[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bitmap[12..14].copy_from_slice(&1u16.to_le_bytes());
        bitmap[14..16].copy_from_slice(&32u16.to_le_bytes());
        let pixels = [0, 0, 255, 255, 0, 255, 0, 128, 255, 0, 0, 255, 0, 0, 0, 0];
        bitmap[124..].copy_from_slice(&pixels);
        let mut bmp = bitmap_header(&bitmap).unwrap().to_vec();
        bmp.extend(bitmap);
        let png = encode_png(&bmp).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let roundtrip = dib(&png).unwrap();
        assert_eq!(&roundtrip[124..], &pixels);
        unsafe {
            CoUninitialize();
        }
    }
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
