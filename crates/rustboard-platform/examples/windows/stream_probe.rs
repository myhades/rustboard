//! Out-of-process COM lifetime probe; never stages file contents.
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use windows::{
        Win32::System::{Com::*, DataExchange::RegisterClipboardFormatW, Ole::*},
        core::w,
    };
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(args.len() == 3, "stream_probe <ready-file> <continue-file>");
    unsafe {
        OleInitialize(None)?;
        let object = OleGetClipboard()?;
        let format = rustboard_platform::windows::virtual_files::format(
            RegisterClipboardFormatW(w!("FileContents")) as u16,
            0,
            TYMED_ISTREAM,
        );
        let mut medium = object.GetData(&format)?;
        let stream = medium
            .u
            .pstm
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no stream"))?
            .clone();
        ReleaseStgMedium(&mut medium);
        drop(object);
        std::fs::write(&args[1], "ready")?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !std::path::Path::new(&args[2]).exists() {
            anyhow::ensure!(std::time::Instant::now() < deadline, "probe timeout");
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let mut buffer = [0u8; 16];
        let mut count = 0;
        let hr = stream.Read(buffer.as_mut_ptr().cast(), 16, Some(&mut count));
        println!(
            "{}",
            serde_json::json!({"hresult":format!("{:08x}",hr.0 as u32),"success":hr.is_ok(),"bytes":count,"pattern_correct":buffer.iter().enumerate().all(|(i,b)|*b==i as u8)})
        );
        drop(stream);
        OleUninitialize();
    }
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows COM probe only");
}
