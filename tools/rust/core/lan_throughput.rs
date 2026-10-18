//! Explicit two-machine diagnostic. Never captures the clipboard; only generated data.
//! init DIR; serve DIR ADDRESS PEER_PIN SIZE; receive DIR ADDRESS PEER_PIN [core|ipc]
#[path = "../../../tests/support/pattern.rs"]
mod pattern;
#[path = "../../../tests/support/identity.rs"]
mod test_identity;
use anyhow::{Context, Result, ensure};
use pattern::PatternSource;
use rustboard_core::{engine::*, transport::*, *};
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
        let id = new_id();
        let file = FileEntry {
            file_id: id.clone(),
            relative_path: "pattern.bin".into(),
            size: Some(size),
            version: "pattern-v1".into(),
            media_type: "application/octet-stream".into(),
        };
        node.engine.lock().unwrap().local(
            Content::Files { files: vec![file] },
            HashMap::from([(id, source.clone() as Arc<dyn ReadAt>)]),
        )?;
        let listener = TcpListener::bind(peer.address)?;
        let server = node.serve(listener)?;
        println!(
            "{}",
            serde_json::json!({"ready":true,"pin":node.identity.pin(),"source_bytes":source.bytes_read.load(Ordering::Relaxed)})
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
            serde_json::json!({"source_bytes":source.bytes_read.load(Ordering::Relaxed)})
        );
        return Ok(());
    }
    ensure!(mode == "receive", "unknown mode");
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
    let remote = RemoteSource::new(node.clone(), peer, offer.clone(), file.clone());
    let ipc = args.get(5).is_some_and(|s| s == "ipc");
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
                        .iter()
                        .enumerate()
                        .all(|(i, b)| *b == ((offset + i as u64) % 251) as u8),
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
        serde_json::json!({"mode":if ipc {"typed_ipc_encode"} else {"core_tls"},"bytes":offset,"seconds":seconds,"read_seconds":read_seconds,"encode_seconds":encode_seconds,"encoded_bytes":encoded_bytes,"mib_s":offset as f64/1048576.0/seconds,"read_ms_p50":percentile(0.5),"read_ms_p95":percentile(0.95),"read_ms_p99":percentile(0.99),"validated":true})
    );
    Ok(())
}
