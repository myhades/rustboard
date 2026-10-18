//! Release-only diagnostic: TLS and local_ipc + JSON, without OS clipboard or disk I/O.
#[path = "../../../tests/support/pattern.rs"]
mod pattern;
use pattern::PatternSource;
use rustboard_core::{engine::*, transport::*, *};
use std::{
    collections::HashMap,
    net::TcpListener,
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

fn main() -> anyhow::Result<()> {
    let size: u64 = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "67108864".into())
        .parse()?;
    anyhow::ensure!(size > 0 && size <= 1073741824, "size must be 1..=1GiB");
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
        anyhow::ensure!(
            source.bytes_read.load(Ordering::Relaxed) == 0,
            "unexpected pre-read"
        );
        #[cfg(unix)]
        if let Some(path) = std::env::args().nth(2) {
            return socket_probe(&b, &source, size, &path);
        }
        for mode in ["core_tls", "local_ipc_json"] {
            let remote = RemoteSource::new(b.clone(), peer.clone(), offer.clone(), file.clone());
            let before = source.bytes_read.load(Ordering::Relaxed);
            let start = Instant::now();
            let mut offset = 0;
            let mut serialization_seconds = 0.0;
            let mut wire_bytes = 0;
            let mut buffer = vec![0; 65536];
            while offset < size {
                let length = (size - offset).min(65536) as usize;
                if mode == "core_tls" {
                    let n = remote.read_at(offset, &mut buffer[..length])?;
                    anyhow::ensure!(n == length, "short read");
                    anyhow::ensure!(
                        buffer[..n]
                            .iter()
                            .enumerate()
                            .all(|(i, b)| *b == ((offset + i as u64) % 251) as u8),
                        "corrupt data"
                    );
                } else {
                    let reply = b.local_ipc(IpcOperation::ReadBinary {
                        request_id: offset / 65536,
                        offer_id: offer.offer_id.clone(),
                        file_id: file.file_id.clone(),
                        version: file.version.clone(),
                        offset,
                        length: length as u32,
                    })?;
                    let bytes = reply["response"]["bytes"]
                        .as_array()
                        .ok_or_else(|| anyhow::anyhow!("missing bytes"))?;
                    anyhow::ensure!(
                        bytes.len() == length
                            && bytes
                                .iter()
                                .enumerate()
                                .all(|(i, b)| b.as_u64() == Some((offset + i as u64) % 251)),
                        "corrupt data"
                    );
                    let encode = Instant::now();
                    wire_bytes += serde_json::to_vec(&reply)?.len();
                    serialization_seconds += encode.elapsed().as_secs_f64();
                }
                offset += length as u64;
            }
            let seconds = start.elapsed().as_secs_f64();
            let read = source.bytes_read.load(Ordering::Relaxed) - before;
            anyhow::ensure!(read == size, "unexpected source reads");
            println!(
                "{}",
                serde_json::json!({"mode":mode,"bytes":size,"seconds":seconds,"mib_per_second":size as f64 / 1048576.0 / seconds,"json_serialization_seconds":serialization_seconds,"ipc_json_bytes":wire_bytes,"source_bytes":read,"content_verified":true})
            );
        }
        Ok(())
    })();
    a.stop.store(true, Ordering::Release);
    server
        .join()
        .map_err(|_| anyhow::anyhow!("server panicked"))?;
    result
}

// Diagnostic endpoint only, never part of Agent IPC. No production authentication
// bypass: this lives in an example, uses fresh test peers and an explicit local path.
#[cfg(unix)]
fn socket_probe(
    node: &Arc<Node>,
    source: &PatternSource,
    size: u64,
    path: &str,
) -> anyhow::Result<()> {
    use std::{
        os::unix::{fs::PermissionsExt, net::UnixListener},
        time::Duration,
    };
    let listener = UnixListener::bind(path)?;
    struct SocketPath<'a>(&'a str);
    impl Drop for SocketPath<'_> {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0);
        }
    }
    let _cleanup = SocketPath(path);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(e) => return Err(e.into()),
        }
    };
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let mut node_seconds = 0.0;
    let mut write_seconds = 0.0;
    let mut requests = 0;
    loop {
        let request: IpcRequest = read_frame(&mut stream)?;
        anyhow::ensure!(request.protocol == 1, "protocol");
        let done = matches!(request.request, IpcOperation::Cancel { .. });

        let start = Instant::now();
        let reply = node.local_ipc_reply(request.request)?;
        node_seconds += start.elapsed().as_secs_f64();
        let start = Instant::now();
        write_ipc_reply(&mut stream, &reply)?;
        write_seconds += start.elapsed().as_secs_f64();
        requests += 1;
        if done {
            break;
        }
    }
    let read = source.bytes_read.load(Ordering::Relaxed);
    anyhow::ensure!(read == size, "source byte count mismatch");
    println!(
        "{}",
        serde_json::json!({"mode":"diagnostic_unix_server","node_seconds":node_seconds,"serialize_and_write_seconds":write_seconds,"requests":requests,"source_bytes":read})
    );
    Ok(())
}
