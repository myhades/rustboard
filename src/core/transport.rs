//! TLS 1.3 with explicit SHA-256 certificate pins and proof-of-possession signatures.
//! Pairing is bilateral and out-of-band: no unauthenticated clipboard endpoint exists.
use crate::core::{engine::Engine, *};
#[cfg(test)]
use crate::test_pattern::PatternSource;
use anyhow::{Result, ensure};
use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    server::danger::{ClientCertVerified, ClientCertVerifier},
    *,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

// Active sequential reads only. Independent of file size; no work at publication.
const READ_WINDOW: usize = 4 * 1024 * 1024;

type IpcSourceKey = (String, String, String, String, u64, bool);
fn tcp_keepalive(socket: &TcpStream) -> Result<()> {
    socket2::SockRef::from(socket).set_tcp_keepalive(
        &socket2::TcpKeepalive::new()
            .with_time(Duration::from_secs(30))
            .with_interval(Duration::from_secs(10))
            .with_retries(3),
    )?;
    Ok(())
}

pub struct Node {
    pub identity: Identity,
    pub engine: Mutex<Engine>,
    pub peers: Mutex<Vec<Peer>>,
    pub stop: AtomicBool,
    /// Only the latest accepted offer is queued: bounded even if clipboard STA is busy.
    pub incoming: Mutex<Option<(Offer, Peer)>>,
    active: AtomicUsize,
    peer_connections: Mutex<std::collections::HashMap<SocketAddr, sync::PeerSync>>,
    // Bounded transport cache only: no file bytes, and every IPC read reauthorizes.
    ipc_connections: Mutex<Vec<(IpcSourceKey, SharedConnection)>>,
}
impl Node {
    pub fn new(identity: Identity, peers: Vec<Peer>) -> Arc<Self> {
        Arc::new(Self {
            engine: Mutex::new(Engine::new(identity.device())),
            identity,
            peers: Mutex::new(peers),
            stop: AtomicBool::new(false),
            incoming: Mutex::new(None),
            active: AtomicUsize::new(0),
            peer_connections: Mutex::new(std::collections::HashMap::new()),
            ipc_connections: Mutex::new(Vec::new()),
        })
    }
    fn trusted(&self, pin: &str) -> bool {
        self.peers
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.fingerprint == pin)
    }
    pub fn accept(&self, offer: Offer, peer: Peer) -> Result<()> {
        ensure!(self.trusted(&peer.fingerprint), "peer revoked");
        let accepted = {
            let mut engine = self.engine.lock().unwrap();
            if engine.accept(offer, &peer.device())? {
                engine.current.clone()
            } else {
                None
            }
        };
        if let Some(offer) = accepted {
            *self.incoming.lock().unwrap() = Some((offer, peer));
        }
        Ok(())
    }
    pub fn serve(
        self: &Arc<Self>,
        listener: TcpListener,
        recycle: fn(&[PathBuf]) -> bool,
    ) -> Result<std::thread::JoinHandle<()>> {
        listener.set_nonblocking(true)?;
        let node = self.clone();
        Ok(std::thread::spawn(move || {
            while !node.stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((socket, _)) => {
                        if node.active.fetch_add(1, Ordering::AcqRel) >= 8 {
                            node.active.fetch_sub(1, Ordering::AcqRel);
                            continue;
                        }
                        let n = node.clone();
                        std::thread::spawn(move || {
                            let result = n.session(socket, recycle);
                            n.active.fetch_sub(1, Ordering::AcqRel);
                            if let Ok(Some((peer, connection))) = &result {
                                n.receive_control(peer, connection);
                            }
                            if let Err(e) = result {
                                let closed = e
                                    .chain()
                                    .find_map(|cause| cause.downcast_ref::<std::io::Error>())
                                    .is_some_and(|error| {
                                        matches!(
                                            error.kind(),
                                            std::io::ErrorKind::UnexpectedEof
                                                | std::io::ErrorKind::ConnectionReset
                                                | std::io::ErrorKind::ConnectionAborted
                                                | std::io::ErrorKind::TimedOut
                                                | std::io::ErrorKind::WouldBlock
                                        )
                                    });
                                if closed {
                                    log::debug!(target: "network", "session closed: {}", network_error(&e));
                                } else {
                                    static LAST_WARNING: std::sync::atomic::AtomicU64 =
                                        std::sync::atomic::AtomicU64::new(0);
                                    let now = crate::core::engine::now_ms();
                                    let last = LAST_WARNING.load(Ordering::Relaxed);
                                    if now.saturating_sub(last) >= 60_000
                                        && LAST_WARNING
                                            .compare_exchange(
                                                last,
                                                now,
                                                Ordering::Relaxed,
                                                Ordering::Relaxed,
                                            )
                                            .is_ok()
                                    {
                                        log::warn!(target: "network", "session failed: {}", network_error(&e));
                                    }
                                }
                            }
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(50))
                    }
                    Err(_) => break,
                }
            }
        }))
    }
    fn session(
        self: &Arc<Self>,
        socket: TcpStream,
        recycle: fn(&[PathBuf]) -> bool,
    ) -> Result<Option<(Peer, Arc<sync::Control>)>> {
        socket.set_nonblocking(false)?;
        socket.set_read_timeout(Some(Duration::from_secs(5)))?;
        socket.set_write_timeout(Some(Duration::from_secs(5)))?;
        socket.set_nodelay(true)?;
        let pins = self
            .peers
            .lock()
            .unwrap()
            .iter()
            .map(|p| p.fingerprint.clone())
            .collect();
        let provider = tls::crypto_provider();
        let prefer_server =
            provider.cipher_suites[0].suite() == CipherSuite::TLS13_CHACHA20_POLY1305_SHA256;
        let mut config = ServerConfig::builder_with_provider(Arc::new(provider))
            .with_protocol_versions(&[&version::TLS13])?
            .with_client_cert_verifier(Arc::new(Pins {
                pins,
                pairing: false,
            }))
            .with_single_cert(
                vec![CertificateDer::from(self.identity.cert.clone())],
                self.identity.key(),
            )?;
        config.alpn_protocols = vec![ALPN_V2.to_vec()];
        config.ignore_client_order = prefer_server;
        let mut stream = StreamOwned::new(ServerConnection::new(Arc::new(config))?, socket);
        {
            let mut io = tls::HandshakeIo::new(&mut stream.sock);
            while stream.conn.is_handshaking() {
                stream.conn.complete_io(&mut io)?;
            }
        }
        ensure!(
            matches!(stream.conn.alpn_protocol(), Some(ALPN_V2)),
            "ALPN mismatch"
        );
        let cert = &stream
            .conn
            .peer_certificates()
            .ok_or_else(|| anyhow::anyhow!("no identity"))?[0];
        let pin = fingerprint(cert.as_ref());
        // A connection reads one file at a time. Keep its platform lock across
        // chunks; EOF, cancellation, failure or idle disconnect releases it.
        let mut transfer = None;
        let mut read_buffer = Vec::new();
        while !self.stop.load(Ordering::Acquire) {
            let message: Message = read_frame(&mut stream)?;
            ensure!(self.trusted(&pin), "peer revoked");
            let peer = self
                .peers
                .lock()
                .unwrap()
                .iter()
                .find(|p| p.fingerprint == pin)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("unpaired"))?;
            let response = match message {
                Message::Recycle { offer_id } => {
                    let paths = self
                        .engine
                        .lock()
                        .unwrap()
                        .take_recycle_roots(&offer_id, &peer.device())?;
                    if !paths.is_empty() && recycle(&paths) {
                        let mut engine = self.engine.lock().unwrap();
                        if engine
                            .current
                            .as_ref()
                            .is_some_and(|o| o.offer_id == offer_id)
                        {
                            engine.clear_clipboard();
                        }
                    }
                    Message::Ack
                }
                Message::Hello {
                    protocol,
                    device_id,
                } => {
                    ensure!(
                        protocol == PROTOCOL_VERSION && device_id == peer.device(),
                        "identity or protocol mismatch"
                    );
                    write_frame(
                        &mut stream,
                        &Message::Hello {
                            protocol: PROTOCOL_VERSION,
                            device_id: self.identity.device(),
                        },
                    )?;
                    return Ok(self
                        .control(
                            &peer,
                            rustls::Connection::Server(stream.conn),
                            stream.sock,
                            false,
                        )?
                        .map(|connection| (peer, connection)));
                }
                Message::Retain { offer_id } => {
                    let _retained = {
                        let mut engine = self.engine.lock().unwrap();
                        ensure!(
                            engine.batch_device(&offer_id)? == engine.device_id,
                            "source is not local"
                        );
                        engine.retain_batch(&offer_id)?
                    };
                    write_frame(&mut stream, &Message::Ack)?;
                    tcp_keepalive(&stream.sock)?;
                    stream.sock.set_read_timeout(None)?;
                    // This channel carries ownership only; EOF releases the token.
                    let _: Message = read_frame(&mut stream)?;
                    return Ok(None);
                }
                Message::Read {
                    background,
                    request_id,
                    source_device,
                    offer_id,
                    file_id,
                    version,
                    offset,
                    length,
                } => {
                    let source = self.engine.lock().unwrap().source(
                        &source_device,
                        &offer_id,
                        &file_id,
                        &version,
                    );
                    match source {
                        Ok(source)
                            if length > 0
                                && length as usize <= READ_WINDOW
                                && offset <= source.len() =>
                        {
                            let key = (
                                source_device.clone(),
                                offer_id.clone(),
                                file_id.clone(),
                                version.clone(),
                                background,
                            );
                            if transfer.as_ref().is_none_or(|(active, _)| active != &key) {
                                drop(transfer.take());
                                let reader =
                                    source.open_reader()?.unwrap_or_else(|| source.clone());
                                let guard = if background {
                                    None
                                } else {
                                    reader.transfer_guard()?
                                };
                                transfer = Some((key, (reader, guard)));
                            }
                            let source = &transfer.as_ref().unwrap().1.0;
                            if background && source.is_empty() {
                                source.read_background(0, &mut [])?;
                            }
                            let length = (length as u64).min(source.len() - offset) as usize;
                            let eof = offset + length as u64 == source.len();
                            read_buffer.resize(MAX_CHUNK, 0);
                            framing::write_binary_header(
                                &mut stream,
                                request_id,
                                offset,
                                eof,
                                length,
                            )?;
                            let mut total = 0;
                            while total < length {
                                ensure!(
                                    self.trusted(&pin) && !self.stop.load(Ordering::Acquire),
                                    "revoked"
                                );
                                self.engine.lock().unwrap().source(
                                    &source_device,
                                    &offer_id,
                                    &file_id,
                                    &version,
                                )?;
                                let limit = (length - total).min(MAX_CHUNK);
                                let n = if background {
                                    source.read_background(
                                        offset + total as u64,
                                        &mut read_buffer[..limit],
                                    )?
                                } else {
                                    source
                                        .read_at(offset + total as u64, &mut read_buffer[..limit])?
                                };
                                ensure!(n > 0 && n <= limit, "unexpected read length");
                                stream.write_all(&read_buffer[..n])?;
                                total += n;
                            }
                            stream.flush()?;
                            if eof {
                                transfer = None;
                            }
                            self.engine.lock().unwrap().sent(
                                &offer_id,
                                &peer.device(),
                                &file_id,
                                offset,
                                length as u64,
                            );
                            continue;
                        }
                        _ => Message::Error {
                            request_id: Some(request_id),
                            code: ErrorCode::InvalidRequest,
                        },
                    }
                }
                // One bounded request per connection at a time. Cancel retires it between chunks.
                Message::Cancel { .. } => {
                    transfer = None;
                    Message::Ack
                }
                _ => Message::Error {
                    request_id: None,
                    code: ErrorCode::InvalidRequest,
                },
            };
            write_v2_response(&mut stream, &response)?;
        }
        Ok(None)
    }
}

mod sync;

pub mod pairing;

mod framing;
mod ipc;
#[cfg(test)]
mod tests;
mod tls;
use framing::{DataReply, read_v2_data, write_binary_data, write_v2_response};
pub use ipc::{IpcData, IpcReply, write_ipc_reply};
use tls::{ALPN_V2, Pins};
pub use tls::{Client, Identity, Peer, connect, device_from_pin, fingerprint};

mod transfer;
#[cfg(target_os = "linux")]
pub(crate) use transfer::BatchRead;
pub use transfer::RemoteSource;
pub(crate) use transfer::SharedBatchRead;
use transfer::{ReadConnection, SharedConnection};

// Do not put a remote message's JSON values into diagnostic output.
fn network_error(error: &anyhow::Error) -> String {
    for cause in error.chain() {
        if let Some(error) = cause.downcast_ref::<std::io::Error>() {
            return error.to_string();
        }
        if let Some(error) = cause.downcast_ref::<rustls::Error>() {
            return error.to_string();
        }
    }
    "invalid protocol message or peer identity".into()
}
