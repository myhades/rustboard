//! Isolated acceptance peer. Loopback-only; never touches the OS clipboard.
#[path = "../../../tests/support/pattern.rs"]
mod pattern;
use pattern::PatternSource;
use rustboard_core::{engine::*, transport::*, *};
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
    std::fs::write(out.join("peer-pin.txt"), node.identity.pin())?;
    node.serve(std::net::TcpListener::bind(format!(
        "127.0.0.1:{}",
        args[3]
    ))?)?;
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
                        },
                        HashMap::new(),
                    )?;
                }
                "files" => {
                    let mut map: HashMap<String, Arc<dyn ReadAt>> = HashMap::new();
                    let mut files = vec![];
                    sources.clear();
                    for size in [4096, 67108864, 1073741824] {
                        let id = new_id();
                        let source = Arc::new(PatternSource::new(size));
                        files.push(FileEntry {
                            file_id: id.clone(),
                            relative_path: format!("rustboard-tls-{size}.bin"),
                            size: Some(size),
                            version: "pattern-v1".into(),
                            media_type: "application/octet-stream".into(),
                        });
                        map.insert(id, source.clone());
                        sources.push(source);
                    }
                    node.engine
                        .lock()
                        .unwrap()
                        .local(Content::Files { files }, map)?;
                }
                "stop" => break,
                _ => {}
            }
            mode = command;
        }
        let _ = node.sync_once(&peer);
        if let Some((offer, _)) = node.incoming.lock().unwrap().take()
            && offer.offer_id != last
        {
            last = offer.offer_id.clone();
            std::fs::write(
                out.join("received.json"),
                serde_json::to_vec_pretty(&offer)?,
            )?;
        }
        std::fs::write(out.join("reads.json"),serde_json::to_vec(&sources.iter().map(|s|serde_json::json!({"size":s.size,"bytes":s.bytes_read.load(Ordering::Relaxed)})).collect::<Vec<_>>())?)?;
        std::thread::sleep(Duration::from_millis(250));
    }
    node.stop.store(true, Ordering::Release);
    Ok(())
}
