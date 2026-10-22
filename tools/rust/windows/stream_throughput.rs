//! Real IDataObject/IStream -> disk diagnostic; does not change the clipboard.
#[cfg(windows)]
#[path = "../../../tests/support/pattern.rs"]
mod pattern;
#[cfg(windows)]
use pattern::PatternSource;
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use rustboard::core::{engine::*, transport::*, *};
    use rustboard::platform::{PresentedFile, windows::virtual_files::*};
    use std::{
        collections::HashMap,
        io::Write,
        net::TcpListener,
        sync::{Arc, atomic::Ordering},
        time::Instant,
    };
    use windows::{
        Win32::System::{Com::*, DataExchange::RegisterClipboardFormatW, Ole::*},
        core::w,
    };
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 3,
        "stream_throughput <size> <new output file>"
    );
    let size: u64 = args[1].parse()?;
    anyhow::ensure!(size > 0 && size <= 1073741824, "invalid size");
    let a = Node::new(Identity::generate()?, vec![]);
    let b = Node::new(Identity::generate()?, vec![]);
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let peer = Peer {
        address: listener.local_addr()?,
        fingerprint: a.identity.pin(),
    };
    a.peers.lock().unwrap().push(Peer {
        address: "127.0.0.1:1".parse()?,
        fingerprint: b.identity.pin(),
    });
    b.peers.lock().unwrap().push(peer.clone());
    let source = Arc::new(PatternSource::new(size));
    let id = new_id();
    let file = FileEntry {
        directory: false,
        modified_ms: None,
        executable: None,
        file_id: id.clone(),
        relative_path: "pattern.bin".into(),
        size: Some(size),
        version: "pattern-v1".into(),
        media_type: "application/octet-stream".into(),
    };
    let offer = a.engine.lock().unwrap().local(
        Content::Files {
            files: vec![file.clone()],
        },
        HashMap::from([(id, source.clone() as Arc<dyn ReadAt>)]),
    )?;
    let server = a.serve(listener)?;
    let result = (|| -> anyhow::Result<()> {
        b.sync_once(&peer)?;
        unsafe { OleInitialize(None)? };
        struct OleGuard;
        impl Drop for OleGuard {
            fn drop(&mut self) {
                unsafe { OleUninitialize() }
            }
        }
        let _ole = OleGuard;
        let remote = Arc::new(RemoteSource::new(
            b.clone(),
            peer,
            offer.offer_id.clone(),
            file,
        ));
        let object = create(
            vec![PresentedFile {
                directory: false,
                modified_ms: None,
                name: "pattern.bin".into(),
                source: remote,
            }],
            None,
            false,
        )?;
        let stream = unsafe {
            let mut descriptor = object.GetData(&format(
                RegisterClipboardFormatW(w!("FileGroupDescriptorW")) as u16,
                -1,
                TYMED_HGLOBAL,
            ))?;
            ReleaseStgMedium(&mut descriptor);
            let mut medium = object.GetData(&format(
                RegisterClipboardFormatW(w!("FileContents")) as u16,
                0,
                TYMED_ISTREAM,
            ))?;
            let stream = medium.u.pstm.as_ref().cloned();
            ReleaseStgMedium(&mut medium);
            stream.ok_or_else(|| anyhow::anyhow!("missing stream"))?
        };
        anyhow::ensure!(
            source.bytes_read.load(Ordering::Relaxed) == 0,
            "pre-read at publication"
        );
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        let mut buffer = vec![0u8; 65536];
        let start = Instant::now();
        let mut offset = 0;
        while offset < size {
            let count = (size - offset).min(buffer.len() as u64) as usize;
            let mut n = 0;
            unsafe {
                stream
                    .Read(buffer.as_mut_ptr().cast(), count as u32, Some(&mut n))
                    .ok()?
            };
            anyhow::ensure!(n as usize == count, "short COM read");
            anyhow::ensure!(
                buffer[..count]
                    .iter()
                    .enumerate()
                    .all(|(i, b)| *b == ((offset + i as u64) % 251) as u8),
                "corrupt stream"
            );
            out.write_all(&buffer[..count])?;
            offset += count as u64;
        }
        out.sync_all()?;
        let seconds = start.elapsed().as_secs_f64();
        anyhow::ensure!(
            source.bytes_read.load(Ordering::Relaxed) == size,
            "source byte mismatch"
        );
        println!(
            "{}",
            serde_json::json!({"mode":"windows_tls_istream_disk","bytes":size,"seconds":seconds,"mib_per_second":size as f64/1048576.0/seconds,"source_bytes":source.bytes_read.load(Ordering::Relaxed),"content_verified":true,"publication_reads":0})
        );
        Ok(())
    })();
    a.stop.store(true, Ordering::Release);
    server
        .join()
        .map_err(|_| anyhow::anyhow!("server panicked"))?;
    result
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows only")
}
