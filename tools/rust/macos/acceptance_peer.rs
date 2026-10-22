//! macOS acceptance peer, derived from core loopback_peer. Synthetic loopback only.
//! RUSTBOARD_TEST_SIZES selects the shared-vector sizes; defaults to all three.
#[path = "../../../tests/support/pattern.rs"]
mod pattern;
use pattern::PatternSource;
use rustboard::core::{engine::*, transport::*, *};
use std::{
    collections::HashMap,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 5,
        "loopback_peer <agent fingerprint> <agent port> <own port> <output directory>"
    );
    let peer = Peer {
        address: format!("127.0.0.1:{}", args[2]).parse()?,
        fingerprint: args[1].clone(),
    };
    device_from_pin(&peer.fingerprint)?;
    let node = Node::new(Identity::generate()?, vec![peer.clone()]);
    let out = std::path::PathBuf::from(&args[4]);
    std::fs::create_dir_all(&out)?;
    // Independent fixture restarts must advance beyond the receiver's persisted watermark.
    std::fs::write(out.join("fixture-clock.json"), now_ms().to_string())?;
    node.engine
        .lock()
        .unwrap()
        .persist_clock(out.join("fixture-clock.json"))?;
    std::fs::write(out.join("peer-pin.txt"), node.identity.pin())?;
    node.serve(
        std::net::TcpListener::bind(format!("127.0.0.1:{}", args[3]))?,
        |_| false,
    )?;
    let deadline = Instant::now() + Duration::from_secs(1800);
    let mut last = String::new();
    let mut mode = String::new();
    let mut sources: Vec<Arc<PatternSource>> = vec![];
    while Instant::now() < deadline {
        let command = std::fs::read_to_string(out.join("command.txt")).unwrap_or_default();
        if command != mode {
            match command.trim() {
                "text" => {
                    node.engine.lock().unwrap().local(
                        Content::Text {
                            text: "Rustboard TLS Unicode 验证 🦀".into(),
                            html: None,
                            rtf: None,
                        },
                        HashMap::new(),
                    )?;
                }
                "files" => {
                    let mut map: HashMap<String, Arc<dyn ReadAt>> = HashMap::new();
                    let mut files = vec![];
                    sources.clear();
                    for size in std::fs::read_to_string(out.join("sizes.txt"))
                        .unwrap_or_else(|_| {
                            std::env::var("RUSTBOARD_TEST_SIZES")
                                .unwrap_or_else(|_| "4096,67108864,1073741824".into())
                        })
                        .split(',')
                        .map(str::parse::<u64>)
                    {
                        let size = size?;
                        anyhow::ensure!(
                            [4096, 67108864, 1073741824].contains(&size),
                            "unsupported fixture size"
                        );
                        let id = new_id();
                        let source = Arc::new(PatternSource::new(size));
                        files.push(FileEntry {
                            directory: false,
                            modified_ms: None,
                            executable: None,
                            file_id: id.clone(),
                            relative_path: format!("rustboard-tls-{size}.bin"),
                            size: Some(size),
                            version: "pattern-v1".into(),
                            media_type: "application/octet-stream".into(),
                        });
                        map.insert(id, source.clone());
                        sources.push(source);
                    }
                    let mut engine = node.engine.lock().unwrap();
                    engine.local(Content::Files { files }, map)?;
                    let ttl = std::fs::read_to_string(out.join("ttl-ms.txt"))
                        .ok()
                        .and_then(|value| value.trim().parse::<u64>().ok())
                        .unwrap_or(600_000);
                    anyhow::ensure!((1..=600_000).contains(&ttl), "fixture TTL out of range");
                    engine.current.as_mut().unwrap().expires_unix_ms = now_ms() + ttl;
                }
                "stop" => break,
                _ => {}
            }
            mode = command;
        }
        let _ = node.sync_once(&peer);
        let incoming = node.incoming.lock().unwrap().take();
        if let Some((offer, _)) = incoming
            && offer.offer_id != last
        {
            last = offer.offer_id.clone();
            std::fs::write(
                out.join("received.json"),
                serde_json::to_vec_pretty(&offer)?,
            )?;
        }
        if out.join("read-received").exists() && !out.join("received-body.bin").exists() {
            let current = node.engine.lock().unwrap().current.clone();
            if let Some(offer) = current
                && offer.source_device == peer.device()
                && let Content::Files { files } = &offer.content
            {
                anyhow::ensure!(
                    files.len() == 1 && files[0].size == Some(4096),
                    "fixture accepts only synthetic 4KiB"
                );
                let remote = RemoteSource::new(
                    node.clone(),
                    peer.clone(),
                    offer.offer_id.clone(),
                    files[0].clone(),
                );
                let mut bytes = vec![0; 4096];
                let count = remote.read_at(0, &mut bytes)?;
                anyhow::ensure!(count == bytes.len(), "short fixture read");
                std::fs::write(out.join("received-body.bin"), bytes)?;
            }
        }

        std::fs::write(out.join("reads.json"),serde_json::to_vec(&sources.iter().map(|s|serde_json::json!({"size":s.size,"bytes":s.bytes_read.load(Ordering::Relaxed)})).collect::<Vec<_>>())?)?;
        std::thread::sleep(Duration::from_millis(250));
    }
    node.stop.store(true, Ordering::Release);
    Ok(())
}
