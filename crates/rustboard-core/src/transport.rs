//! TLS 1.3 with explicit SHA-256 certificate pins and proof-of-possession signatures.
//! Pairing is bilateral and out-of-band: no unauthenticated clipboard endpoint exists.
use crate::{
    engine::{Engine, now_ms},
    *,
};
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
    io::Write,
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

pub fn fingerprint(cert: &[u8]) -> String {
    format!("{:x}", Sha256::digest(cert))
}
pub fn device_from_pin(pin: &str) -> Result<String> {
    ensure!(
        pin.len() == 64 && pin.bytes().all(|b| b.is_ascii_hexdigit()),
        "expected 64 hexadecimal fingerprint characters"
    );
    let mut bytes = [0u8; 16];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&pin[i * 2..i * 2 + 2], 16)?;
    }
    Ok(uuid::Uuid::from_bytes(bytes).to_string())
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Identity {
    pub cert: Vec<u8>,
    key: Vec<u8>,
}
impl Identity {
    pub fn generate() -> Result<Self> {
        let key = rcgen::generate_simple_self_signed(vec!["rustboard.local".into()])?;
        Ok(Self {
            cert: key.cert.der().to_vec(),
            key: key.signing_key.serialize_der(),
        })
    }
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join("identity.json");
        if path.exists() {
            return Ok(serde_json::from_slice(&std::fs::read(path)?)?);
        }
        let identity = Self::generate()?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(&serde_json::to_vec(&identity)?)?;
        file.sync_all()?;
        Ok(identity)
    }
    pub fn pin(&self) -> String {
        fingerprint(&self.cert)
    }
    pub fn device(&self) -> String {
        device_from_pin(&self.pin()).expect("SHA-256 pin")
    }
    fn key(&self) -> rustls::pki_types::PrivateKeyDer<'static> {
        PrivatePkcs8KeyDer::from(self.key.clone()).into()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub address: SocketAddr,
    pub fingerprint: String,
}
impl Peer {
    pub fn device(&self) -> String {
        device_from_pin(&self.fingerprint).expect("validated peer")
    }
}

#[derive(Debug)]
struct Pins {
    pins: Vec<String>,
}
fn verify_pin(
    cert: &CertificateDer<'_>,
    pins: &[String],
) -> std::result::Result<(), rustls::Error> {
    if pins.contains(&fingerprint(cert.as_ref())) {
        Ok(())
    } else {
        Err(rustls::Error::General("unpaired certificate".into()))
    }
}
fn signature(
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
    rustls::crypto::verify_tls13_signature(
        message,
        cert,
        dss,
        &rustls::crypto::ring::default_provider().signature_verification_algorithms,
    )
}
impl ServerCertVerifier for Pins {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        verify_pin(cert, &self.pins)?;
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 disabled".into()))
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        signature(m, c, d)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
impl ClientCertVerifier for Pins {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> std::result::Result<ClientCertVerified, rustls::Error> {
        verify_pin(cert, &self.pins)?;
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 disabled".into()))
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        signature(m, c, d)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub type Client = StreamOwned<ClientConnection, TcpStream>;
pub fn connect(identity: &Identity, peer: &Peer) -> Result<Client> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pins {
            pins: vec![peer.fingerprint.clone()],
        }))
        .with_client_auth_cert(
            vec![CertificateDer::from(identity.cert.clone())],
            identity.key(),
        )?;
    config.alpn_protocols = vec![b"rustboard/1".to_vec()];
    let socket = TcpStream::connect_timeout(&peer.address, Duration::from_secs(3))?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    socket.set_write_timeout(Some(Duration::from_secs(5)))?;
    socket.set_nodelay(true)?;
    let conn = ClientConnection::new(Arc::new(config), ServerName::try_from("rustboard.local")?)?;
    let mut stream = StreamOwned::new(conn, socket);
    while stream.conn.is_handshaking() {
        stream.conn.complete_io(&mut stream.sock)?;
    }
    ensure!(
        stream.conn.alpn_protocol() == Some(b"rustboard/1"),
        "ALPN mismatch"
    );
    Ok(stream)
}

pub struct Node {
    pub identity: Identity,
    pub engine: Mutex<Engine>,
    pub peers: Mutex<Vec<Peer>>,
    pub stop: AtomicBool,
    /// Only the latest accepted offer is queued: bounded even if clipboard STA is busy.
    pub incoming: Mutex<Option<(Offer, Peer)>>,
    active: AtomicUsize,
}
impl Node {
    /// Portable File Provider/FUSE IPC operations; OS adapter authenticates the local caller.
    pub fn local_ipc(self: &Arc<Self>, operation: IpcOperation) -> Result<serde_json::Value> {
        match operation {
            IpcOperation::GetOffer => {
                let mut engine = self.engine.lock().unwrap();
                engine.expire();
                ensure!(!engine.paused, "paused");
                Ok(serde_json::json!({"ok":true,"protocol":1,"offer":engine.current}))
            }
            IpcOperation::Read {
                request_id,
                offer_id,
                file_id,
                version,
                offset,
                length,
            } => {
                ensure!(length > 0 && length as usize <= MAX_CHUNK, "invalid range");
                let offer = {
                    let mut engine = self.engine.lock().unwrap();
                    engine.expire();
                    ensure!(!engine.paused, "paused");
                    engine
                        .current
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("expired"))?
                };
                ensure!(offer.offer_id == offer_id, "expired");
                let Content::Files { files } = &offer.content else {
                    anyhow::bail!("not file offer")
                };
                let file = files
                    .iter()
                    .find(|f| f.file_id == file_id && f.version == version)
                    .ok_or_else(|| anyhow::anyhow!("version_changed"))?
                    .clone();
                let source: Arc<dyn ReadAt> = if offer.source_device == self.identity.device() {
                    self.engine.lock().unwrap().source(
                        &offer.source_device,
                        &offer_id,
                        &file_id,
                        &version,
                    )?
                } else {
                    let peer = self
                        .peers
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|p| p.device() == offer.source_device)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("unauthorized"))?;
                    ensure!(file.size.is_some(), "unknown size unsupported");
                    Arc::new(RemoteSource::new(self.clone(), peer, offer, file))
                };
                ensure!(offset <= source.len(), "invalid range");
                let mut bytes = vec![0; length as usize];
                let n = source.read_at(offset, &mut bytes)?;
                bytes.truncate(n);
                Ok(
                    serde_json::json!({"ok":true,"protocol":1,"response":Message::Data{request_id,offset,eof:offset+n as u64==source.len(),bytes}}),
                )
            }
            IpcOperation::Cancel { request_id } => {
                Ok(serde_json::json!({"ok":true,"protocol":1,"request_id":request_id}))
            }
            IpcOperation::Control { .. } => {
                anyhow::bail!("control operation belongs to native adapter")
            }
        }
    }
    pub fn new(identity: Identity, peers: Vec<Peer>) -> Arc<Self> {
        Arc::new(Self {
            engine: Mutex::new(Engine::new(identity.device())),
            identity,
            peers: Mutex::new(peers),
            stop: AtomicBool::new(false),
            incoming: Mutex::new(None),
            active: AtomicUsize::new(0),
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
        if self
            .engine
            .lock()
            .unwrap()
            .accept(offer.clone(), &peer.device())?
        {
            *self.incoming.lock().unwrap() = Some((offer, peer));
        }
        Ok(())
    }
    pub fn serve(self: &Arc<Self>, listener: TcpListener) -> Result<std::thread::JoinHandle<()>> {
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
                            if let Err(e) = n.session(socket) {
                                #[cfg(test)]
                                eprintln!("session failure: {e:?}");
                                #[cfg(not(test))]
                                let _ = e;
                            }
                            n.active.fetch_sub(1, Ordering::AcqRel);
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
    fn session(&self, socket: TcpStream) -> Result<()> {
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
        let mut config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&version::TLS13])?
                .with_client_cert_verifier(Arc::new(Pins { pins }))
                .with_single_cert(
                    vec![CertificateDer::from(self.identity.cert.clone())],
                    self.identity.key(),
                )?;
        config.alpn_protocols = vec![b"rustboard/1".to_vec()];
        let mut stream = StreamOwned::new(ServerConnection::new(Arc::new(config))?, socket);
        while stream.conn.is_handshaking() {
            stream.conn.complete_io(&mut stream.sock)?;
        }
        ensure!(
            stream.conn.alpn_protocol() == Some(b"rustboard/1"),
            "ALPN mismatch"
        );
        let cert = &stream
            .conn
            .peer_certificates()
            .ok_or_else(|| anyhow::anyhow!("no identity"))?[0];
        let pin = fingerprint(cert.as_ref());
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
                Message::Hello {
                    protocol,
                    device_id,
                } => {
                    ensure!(
                        protocol == PROTOCOL_VERSION && device_id == peer.device(),
                        "identity or protocol mismatch"
                    );
                    Message::Hello {
                        protocol: PROTOCOL_VERSION,
                        device_id: self.identity.device(),
                    }
                }
                Message::Poll => {
                    let mut engine = self.engine.lock().unwrap();
                    engine.expire();
                    if engine.paused {
                        Message::Idle
                    } else {
                        engine
                            .current
                            .clone()
                            .filter(|o| o.source_device == engine.device_id)
                            .map(|offer| Message::Offer { offer })
                            .unwrap_or(Message::Idle)
                    }
                }
                Message::Offer { offer } => {
                    if self.accept(offer, peer).is_ok() {
                        Message::Ack
                    } else {
                        Message::Error {
                            request_id: None,
                            code: ErrorCode::InvalidRequest,
                        }
                    }
                }
                Message::Read {
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
                                && length as usize <= MAX_CHUNK
                                && offset <= source.len() =>
                        {
                            let mut bytes = vec![0; length as usize];
                            match source.read_at(offset, &mut bytes) {
                                Ok(n) => {
                                    bytes.truncate(n);
                                    Message::Data {
                                        request_id,
                                        offset,
                                        eof: offset + n as u64 >= source.len(),
                                        bytes,
                                    }
                                }
                                Err(_) => Message::Error {
                                    request_id: Some(request_id),
                                    code: ErrorCode::VersionChanged,
                                },
                            }
                        }
                        _ => Message::Error {
                            request_id: Some(request_id),
                            code: ErrorCode::InvalidRequest,
                        },
                    }
                }
                // One bounded request per connection at a time. Cancel retires it between chunks.
                Message::Cancel { .. } => Message::Ack,
                _ => Message::Error {
                    request_id: None,
                    code: ErrorCode::InvalidRequest,
                },
            };
            write_frame(&mut stream, &response)?;
        }
        Ok(())
    }
    pub fn sync_once(&self, peer: &Peer) -> Result<()> {
        ensure!(self.trusted(&peer.fingerprint), "unpaired");
        let mut stream = connect(&self.identity, peer)?;
        let offer = {
            let mut e = self.engine.lock().unwrap();
            e.expire();
            if e.paused {
                return Ok(());
            }
            e.current.clone().filter(|o| o.source_device == e.device_id)
        };
        if let Some(offer) = offer {
            write_frame(&mut stream, &Message::Offer { offer })?;
            let _: Message = read_frame(&mut stream)?;
        }
        write_frame(&mut stream, &Message::Poll)?;
        if let Message::Offer { offer } = read_frame(&mut stream)? {
            self.accept(offer, peer.clone())?;
        }
        Ok(())
    }
    pub fn start_sync(self: &Arc<Self>) {
        let node = self.clone();
        std::thread::spawn(move || {
            while !node.stop.load(Ordering::Acquire) {
                let peers = node.peers.lock().unwrap().clone();
                for peer in peers {
                    if node.stop.load(Ordering::Acquire) {
                        break;
                    }
                    let _ = node.sync_once(&peer);
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
    }
}

pub struct RemoteSource {
    pub node: Arc<Node>,
    pub peer: Peer,
    pub offer: Offer,
    pub file: FileEntry,
    connection: Mutex<Option<Client>>,
}
impl RemoteSource {
    pub fn new(node: Arc<Node>, peer: Peer, offer: Offer, file: FileEntry) -> Self {
        Self {
            node,
            peer,
            offer,
            file,
            connection: Mutex::new(None),
        }
    }
}
impl ReadAt for RemoteSource {
    fn len(&self) -> u64 {
        self.file.size.unwrap_or(0)
    }
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        ensure!(
            !self.node.stop.load(Ordering::Acquire) && !self.node.engine.lock().unwrap().paused,
            "cancelled"
        );
        ensure!(
            self.node.trusted(&self.peer.fingerprint) && self.offer.expires_unix_ms > now_ms(),
            "revoked or expired"
        );
        ensure!(
            offset <= self.len() && buffer.len() <= MAX_CHUNK,
            "invalid range"
        );
        if buffer.is_empty() || offset == self.len() {
            return Ok(0);
        }
        let mut connection = self.connection.lock().unwrap();
        if connection.is_none() {
            *connection = Some(connect(&self.node.identity, &self.peer)?);
        }
        let result = (|| {
            let stream = connection.as_mut().unwrap();
            write_frame(
                stream,
                &Message::Read {
                    request_id: 1,
                    source_device: self.offer.source_device.clone(),
                    offer_id: self.offer.offer_id.clone(),
                    file_id: self.file.file_id.clone(),
                    version: self.file.version.clone(),
                    offset,
                    length: buffer.len() as u32,
                },
            )?;
            match read_frame(stream)? {
                Message::Data {
                    request_id: 1,
                    offset: returned,
                    bytes,
                    eof,
                } if returned == offset
                    && bytes.len() <= buffer.len()
                    && offset + bytes.len() as u64 <= self.len() =>
                {
                    ensure!(!bytes.is_empty() || eof, "empty non-EOF response");
                    ensure!(
                        eof == (offset + bytes.len() as u64 == self.len()),
                        "inconsistent EOF"
                    );
                    buffer[..bytes.len()].copy_from_slice(&bytes);
                    Ok(bytes.len())
                }
                _ => anyhow::bail!("remote read rejected"),
            }
        })();
        if result.is_err() {
            *connection = None;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mutual_tls_offer_lazy_read_and_untrusted_rejection() {
        let a = Node::new(Identity::generate().unwrap(), vec![]);
        let b = Node::new(Identity::generate().unwrap(), vec![]);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let peer = Peer {
            address: listener.local_addr().unwrap(),
            fingerprint: a.identity.pin(),
        };
        a.peers.lock().unwrap().push(Peer {
            address: "127.0.0.1:1".parse().unwrap(),
            fingerprint: b.identity.pin(),
        });
        b.peers.lock().unwrap().push(peer.clone());
        let source = Arc::new(PatternSource::new(4096));
        let id = engine::new_id();
        let file = FileEntry {
            file_id: id.clone(),
            relative_path: "sample.bin".into(),
            size: Some(4096),
            version: "pattern-v1".into(),
            media_type: "application/octet-stream".into(),
        };
        let offer = a
            .engine
            .lock()
            .unwrap()
            .local(
                Content::Files {
                    files: vec![file.clone()],
                },
                std::collections::HashMap::from([(id, source.clone() as Arc<dyn ReadAt>)]),
            )
            .unwrap();
        let server = a.serve(listener).unwrap();
        b.sync_once(&peer).unwrap();
        assert_eq!(source.bytes_read.load(Ordering::Relaxed), 0);
        let remote = RemoteSource::new(b.clone(), peer.clone(), offer, file);
        let mut bytes = [0; 100];
        assert_eq!(remote.read_at(250, &mut bytes).unwrap(), 100);
        assert_eq!(&bytes[..3], &[250, 0, 1]);
        let stranger = Identity::generate().unwrap();
        let outcome = connect(&stranger, &peer).and_then(|mut unknown| {
            write_frame(&mut unknown, &Message::Poll)?;
            read_frame::<_, Message>(&mut unknown)
        });
        assert!(outcome.is_err());
        a.peers.lock().unwrap().clear();
        assert!(remote.read_at(0, &mut bytes).is_err());
        a.stop.store(true, Ordering::Release);
        server.join().unwrap();
    }
}
