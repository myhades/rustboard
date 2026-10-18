//! Bounded native clipboard probe for metered links. Only two known 4 KiB fixtures.
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use anyhow::ensure;
    use rustboard::platform::windows::virtual_files::format;
    use windows::{
        Win32::{
            System::{Com::*, DataExchange::RegisterClipboardFormatW, Memory::*, Ole::*},
            UI::Shell::FILEDESCRIPTORW,
        },
        core::w,
    };
    let args: Vec<_> = std::env::args().collect();
    ensure!(
        args.len() == 3,
        "small_batch_probe <new ready-file> <continue-file>"
    );
    ensure!(
        !std::path::Path::new(&args[1]).exists() && !std::path::Path::new(&args[2]).exists(),
        "use fresh marker paths"
    );
    unsafe {
        OleInitialize(None)?;
        let object = OleGetClipboard()?;
        let mut descriptor = object.GetData(&format(
            RegisterClipboardFormatW(w!("FileGroupDescriptorW")) as u16,
            -1,
            TYMED_HGLOBAL,
        ))?;
        let handle = descriptor.u.hGlobal;
        let data = GlobalLock(handle).cast::<u8>();
        ensure!(!data.is_null(), "descriptor unavailable");
        let valid = (|| -> anyhow::Result<()> {
            ensure!(
                GlobalSize(handle) >= 4 + 2 * std::mem::size_of::<FILEDESCRIPTORW>(),
                "short descriptor"
            );
            ensure!(
                std::ptr::read_unaligned(data.cast::<u32>()) == 2,
                "exactly two fixtures required"
            );
            let mut names = Vec::new();
            for i in 0..2 {
                let fd = std::ptr::read_unaligned(
                    data.add(4 + i * std::mem::size_of::<FILEDESCRIPTORW>())
                        .cast::<FILEDESCRIPTORW>(),
                );
                ensure!(
                    fd.nFileSizeHigh == 0 && fd.nFileSizeLow == 4096,
                    "refusing anything except 4 KiB files"
                );
                let name = fd.cFileName;
                let end = name.iter().position(|c| *c == 0).unwrap_or(260);
                names.push(String::from_utf16(&name[..end])?);
            }
            names.sort();
            ensure!(
                names == ["tiny-a.bin", "tiny-b.bin"],
                "unexpected source names"
            );
            Ok(())
        })();
        let _ = GlobalUnlock(handle);
        ReleaseStgMedium(&mut descriptor);
        valid?;
        let mut buffer = [0u8; 256];
        for index in 0..2 {
            let mut medium = object.GetData(&format(
                RegisterClipboardFormatW(w!("FileContents")) as u16,
                index,
                TYMED_ISTREAM,
            ))?;
            let stream = medium
                .u
                .pstm
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("no stream"))?
                .clone();
            ReleaseStgMedium(&mut medium);
            for offset in (0..4096usize).step_by(256) {
                let mut count = 0;
                stream
                    .Read(buffer.as_mut_ptr().cast(), 256, Some(&mut count))
                    .ok()?;
                ensure!(
                    count == 256
                        && buffer
                            .iter()
                            .enumerate()
                            .all(|(i, b)| *b == ((offset + i) % 251) as u8),
                    "data mismatch"
                );
                if index == 0 && offset == 0 {
                    std::fs::write(&args[1], "first 256 bytes verified; replace clipboard now")?;
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
                    while !std::path::Path::new(&args[2]).exists() {
                        ensure!(std::time::Instant::now() < deadline, "continue timeout");
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                }
            }
            drop(stream);
        }
        drop(object);
        OleUninitialize();
    }
    println!(
        "PASS native IDataObject/IStream: two 4 KiB files, 8192 bytes verified; second stream opened after replacement"
    );
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows COM probe only");
}
