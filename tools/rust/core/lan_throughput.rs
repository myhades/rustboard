//! Explicit two-machine diagnostic. Never captures the clipboard; uses a generated pattern or supplied file.
//! init DIR; serve DIR ADDRESS PEER_PIN SIZE [SOURCE_FILE]; receive DIR ADDRESS PEER_PIN [core|ipc|com OUTPUT]
#[path = "../../../tests/support/pattern.rs"]
mod pattern;
#[path = "../../../tests/support/identity.rs"]
mod test_identity;
use anyhow::{Context, Result, ensure};
use pattern::PatternSource;
use rustboard::core::{engine::*, transport::*, *};
use std::{
    collections::HashMap,
    net::TcpListener,
    path::Path,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).context("init/serve/receive required")?;
    let dir = Path::new(
        args.get(2)
            .context("private diagnostic directory required")?,
    );
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let identity = test_identity::load(dir)?;
    if mode == "init" {
        println!("{}", identity.pin());
        return Ok(());
    }
    let peer = Peer {
        address: args.get(3).context("address required")?.parse()?,
        fingerprint: args.get(4).context("peer pin required")?.clone(),
    };
    device_from_pin(&peer.fingerprint)?;
    let node = Node::new(identity, vec![peer.clone()]);
    if mode == "serve" {
        let size: u64 = args.get(5).context("size required")?.parse()?;
        ensure!(size > 0 && size <= 1073741824, "size out of bounds");
        let source = Arc::new(PatternSource::new(size));
        let reader: Arc<dyn ReadAt> = if let Some(path) = args.get(6) {
            let file = LocalFile::open(Path::new(path))?;
            ensure!(file.len() == size, "source file size mismatch");
            Arc::new(file)
        } else {
            source.clone()
        };
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
        node.engine.lock().unwrap().local(
            Content::Files { files: vec![file] },
            HashMap::from([(id, reader)]),
        )?;
        let listener = TcpListener::bind(peer.address)?;
        let server = node.serve(listener)?;
        println!(
            "{}",
            serde_json::json!({"ready":true,"pin":node.identity.pin(),"source_bytes":args.get(6).is_none().then(|| source.bytes_read.load(Ordering::Relaxed))})
        );
        // A marker allows deterministic cleanup without killing an unrelated process.
        let stop = dir.join("stop");
        for _ in 0..6000 {
            if stop.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        node.stop.store(true, Ordering::Release);
        server.join().unwrap();
        println!(
            "{}",
            serde_json::json!({"source_bytes":args.get(6).is_none().then(|| source.bytes_read.load(Ordering::Relaxed))})
        );
        return Ok(());
    }
    ensure!(mode == "receive", "unknown mode");
    let cipher = connect(&node.identity, &peer)?
        .conn
        .negotiated_cipher_suite()
        .map(|s| format!("{:?}", s.suite()));
    node.sync_once(&peer)?;
    let offer = node
        .engine
        .lock()
        .unwrap()
        .current
        .clone()
        .context("no offer")?;
    let Content::Files { files } = &offer.content else {
        anyhow::bail!("not files")
    };
    let file = files
        .iter()
        .max_by_key(|f| f.size)
        .context("empty offer")?
        .clone();
    let remote = RemoteSource::new(node.clone(), peer, offer.offer_id.clone(), file.clone());
    #[cfg(windows)]
    if args.get(5).is_some_and(|s| s == "com") {
        return receive_com(
            remote,
            Path::new(args.get(6).context("output file required")?),
        );
    }
    let ipc = args.get(5).is_some_and(|s| s == "ipc");
    let expected: Vec<u8> = (0..MAX_CHUNK + 250).map(|i| (i % 251) as u8).collect();
    let mut latencies = Vec::new();
    let mut buffer = vec![0; MAX_CHUNK];
    let mut offset = 0;
    let mut read_seconds = 0.0;
    let mut encode_seconds = 0.0;
    let mut encoded_bytes = 0;
    let start = Instant::now();
    while offset < remote.len() {
        let length = (remote.len() - offset).min(MAX_CHUNK as u64) as usize;
        let t = Instant::now();
        if ipc {
            let reply = node.local_ipc_reply(IpcOperation::ReadBinary {
                background: false,
                request_id: offset / MAX_CHUNK as u64,
                offer_id: offer.offer_id.clone(),
                file_id: file.file_id.clone(),
                version: file.version.clone(),
                offset,
                length: length as u32,
            })?;
            read_seconds += t.elapsed().as_secs_f64();
            let e = Instant::now();
            let encoded = serde_json::to_vec(&reply)?;
            encode_seconds += e.elapsed().as_secs_f64();
            encoded_bytes += encoded.len();
            // Validation is timed separately from the read and encode stages.
            let value: serde_json::Value = serde_json::from_slice(&encoded)?;
            let bytes = value["response"]["bytes"]
                .as_array()
                .context("missing bytes")?;
            ensure!(
                bytes.len() == length
                    && bytes
                        .iter()
                        .enumerate()
                        .all(|(i, b)| b.as_u64() == Some((offset + i as u64) % 251)),
                "corrupt data"
            );
        } else {
            let n = remote.read_at(offset, &mut buffer[..length])?;
            let elapsed = t.elapsed().as_secs_f64();
            read_seconds += elapsed;
            latencies.push(elapsed);
            ensure!(
                n == length
                    && buffer[..n]
                        == expected[(offset % 251) as usize..(offset % 251) as usize + n],
                "corrupt data"
            );
        }
        offset += length as u64;
    }
    let seconds = start.elapsed().as_secs_f64();
    latencies.sort_by(f64::total_cmp);
    let percentile = |p: f64| {
        latencies
            .get(((latencies.len().saturating_sub(1)) as f64 * p) as usize)
            .copied()
            .unwrap_or(0.0)
            * 1000.0
    };
    println!(
        "{}",
        serde_json::json!({"cipher":cipher,"mode":if ipc {"typed_ipc_encode"} else {"core_tls"},"bytes":offset,"seconds":seconds,"read_seconds":read_seconds,"encode_seconds":encode_seconds,"encoded_bytes":encoded_bytes,"mib_s":offset as f64/1048576.0/seconds,"read_ms_p50":percentile(0.5),"read_ms_p95":percentile(0.95),"read_ms_p99":percentile(0.99),"validated":true})
    );
    Ok(())
}

#[cfg(windows)]
fn receive_com(remote: RemoteSource, output: &Path) -> Result<()> {
    use rustboard::platform::{
        PresentedFile,
        windows::virtual_files::{create, format},
    };
    use std::io::Write;
    use windows::{
        Win32::System::{Com::*, DataExchange::RegisterClipboardFormatW, Ole::*},
        core::w,
    };
    unsafe {
        OleInitialize(None)?;
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe {
                OleUninitialize();
            }
        }
    }
    let _guard = Guard;
    let size = remote.len();
    let object = create(vec![PresentedFile {
        directory: false,
        modified_ms: None,
        name: "pattern.bin".into(),
        source: Arc::new(remote),
    }])?;
    let stream = unsafe {
        let mut medium = object.GetData(&format(
            RegisterClipboardFormatW(w!("FileContents")) as u16,
            0,
            TYMED_ISTREAM,
        ))?;
        let stream = medium.u.pstm.as_ref().cloned();
        ReleaseStgMedium(&mut medium);
        stream.context("missing stream")?
    };
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut buffer = vec![0u8; MAX_CHUNK];
    let mut offset = 0;
    let start = Instant::now();
    while offset < size {
        let mut count = 0;
        unsafe {
            stream
                .Read(
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    Some(&mut count),
                )
                .ok()?;
        }
        ensure!(count > 0, "truncated COM stream");
        out.write_all(&buffer[..count as usize])?;
        offset += u64::from(count);
    }
    out.sync_all()?;
    let seconds = start.elapsed().as_secs_f64();
    ensure!(offset == size, "incorrect size");
    println!(
        "{}",
        serde_json::json!({"mode":"com_to_disk", "bytes":offset, "seconds":seconds, "mib_s":offset as f64/1048576.0/seconds})
    );
    Ok(())
}
