//! Small, isolated desktop acceptance peer. Never accesses the OS clipboard.
#[path = "../../../tests/support/pattern.rs"]
mod pattern;
use rustboard::core::{engine::*, transport::*, *};
use std::{
    collections::HashMap,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 5,
        "clipboard_peer <agent pin> <agent address> <listen address> <output>"
    );
    let peer = Peer {
        address: args[2].parse()?,
        fingerprint: args[1].clone(),
    };
    device_from_pin(&peer.fingerprint)?;
    let node = Node::new(Identity::generate()?, vec![peer.clone()]);
    let out = std::path::Path::new(&args[4]);
    std::fs::create_dir_all(out)?;
    let clock = out.join("clock.json");
    std::fs::write(
        &clock,
        serde_json::to_vec(&serde_json::json!({"clock":now_ms(),"watermark":null}))?,
    )?;
    node.engine.lock().unwrap().persist_clock(clock)?;
    std::fs::write(out.join("peer-pin.txt"), node.identity.pin())?;
    node.serve(std::net::TcpListener::bind(&args[3])?)?;
    let mut mode = String::new();
    let mut sources: Vec<Arc<pattern::PatternSource>> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(1800);
    while Instant::now() < deadline {
        let command = std::fs::read_to_string(out.join("command.txt")).unwrap_or_default();
        if command != mode {
            let mut engine = node.engine.lock().unwrap();
            match command.trim() {
                "text" => {
                    let text = std::fs::read_to_string(out.join("text.txt"))
                        .unwrap_or_else(|_| "Rustboard Linux Unicode 验证 🦀".into());
                    anyhow::ensure!(text.len() <= 262144, "acceptance size limit");
                    engine.local(Content::Text { text }, HashMap::new())?;
                }
                "files" => {
                    let mut files = Vec::new();
                    let mut map: HashMap<String, Arc<dyn ReadAt>> = HashMap::new();
                    sources.clear();
                    for size in [4096, 8192] {
                        let id = new_id();
                        let source = Arc::new(pattern::PatternSource::new(size));
                        files.push(FileEntry {
                            file_id: id.clone(),
                            relative_path: format!("sample-{size}.bin"),
                            size: Some(size),
                            version: "fixture".into(),
                            media_type: "application/octet-stream".into(),
                        });
                        map.insert(id, source.clone());
                        sources.push(source);
                    }
                    engine.local(Content::Files { files }, map)?;
                }
                "stop" => break,
                _ => {}
            }
            std::fs::write(
                out.join("published.json"),
                serde_json::to_vec(&engine.current)?,
            )?;
            mode = command;
        }
        let _ = node.sync_once(&peer);
        if let Some((offer, _)) = node.incoming.lock().unwrap().take() {
            std::fs::write(out.join("received.json"), serde_json::to_vec(&offer)?)?;
            if let Content::Files { files } = &offer.content {
                let received = (|| -> anyhow::Result<()> {
                    for file in files {
                        anyhow::ensure!(
                            file.size.unwrap_or(u64::MAX) <= 262144,
                            "acceptance size limit"
                        );
                        let source = RemoteSource::new(
                            node.clone(),
                            peer.clone(),
                            offer.clone(),
                            file.clone(),
                        );
                        let mut bytes = Vec::new();
                        let mut chunk = [0; MAX_CHUNK];
                        while (bytes.len() as u64) < source.len() {
                            let n = source.read_at(bytes.len() as u64, &mut chunk)?;
                            anyhow::ensure!(n > 0, "truncated");
                            bytes.extend_from_slice(&chunk[..n]);
                        }
                        std::fs::write(out.join(format!("received-{}", file.file_id)), &bytes)?;
                    }
                    Ok(())
                })();
                if let Err(error) = received {
                    eprintln!("read interrupted: {error:#}");
                }
            }
        }
        std::fs::write(
            out.join("reads.json"),
            serde_json::to_vec(
                &sources
                    .iter()
                    .map(|s| s.bytes_read.load(Ordering::Relaxed))
                    .collect::<Vec<_>>(),
            )?,
        )?;
        std::thread::sleep(Duration::from_millis(100));
    }
    node.stop.store(true, Ordering::Release);
    Ok(())
}
