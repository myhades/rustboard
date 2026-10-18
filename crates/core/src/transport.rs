//! TLS 1.3 with explicit SHA-256 certificate pins and proof-of-possession signatures.
//! Pairing is bilateral and out-of-band: no unauthenticated clipboard endpoint exists.
#[cfg(test)]
use crate::test_pattern::PatternSource;
use crate::{engine::Engine, *};
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
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
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
    /// Platform credential stores supply PKCS#8 material; validate its certificate binding.
    pub fn from_parts(cert: Vec<u8>, key: Vec<u8>) -> Result<Self> {
        let identity = Self { cert, key };
        rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(identity.cert.clone())],
            identity.key(),
        )?;
        Ok(identity)
    }
    pub fn private_key_der(&self) -> &[u8] {
        &self.key
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
    pairing: bool,
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
        if !self.pairing {
            verify_pin(cert, &self.pins)?;
        }
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
        if !self.pairing {
            verify_pin(cert, &self.pins)?;
        }
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
const ALPN_V2: &[u8] = b"rustboard/2";
// Active sequential reads only. Independent of file size; no work at publication.
const READ_WINDOW: usize = 4 * 1024 * 1024;

fn write_v2_response<W: Write>(writer: &mut W, message: &Message) -> Result<()> {
    if let Message::Data {
        request_id,
        offset,
        eof,
        bytes,
    } = message
    {
        write_binary_data(writer, *request_id, *offset, *eof, bytes)
    } else {
        write_frame(writer, message)
    }
}
fn write_binary_data<W: Write>(
    writer: &mut W,
    request_id: u64,
    offset: u64,
    eof: bool,
    bytes: &[u8],
) -> Result<()> {
    ensure!(bytes.len() <= READ_WINDOW, "data too large");
    let mut header = [0u8; 21];
    header[..4].copy_from_slice(&((bytes.len() as u32 + 17) | 0x8000_0000).to_be_bytes());
    header[4..12].copy_from_slice(&request_id.to_be_bytes());
    header[12..20].copy_from_slice(&offset.to_be_bytes());
    header[20] = u8::from(eof);
    let mut buffered = std::io::BufWriter::with_capacity(MAX_CHUNK, writer);
    buffered.write_all(&header)?;
    buffered.write_all(bytes)?;
    buffered.flush()?;
    Ok(())
}

fn read_v2_data<R: Read>(reader: &mut R, limit: usize) -> Result<DataReply> {
    let mut header = [0u8; 21];
    reader.read_exact(&mut header[..4])?;
    let size = u32::from_be_bytes(header[..4].try_into().unwrap());
    ensure!(size & 0x8000_0000 != 0, "remote read rejected");
    let size = (size & 0x7fff_ffff) as usize;
    ensure!(
        size >= 17 && size - 17 <= limit && size - 17 <= READ_WINDOW,
        "invalid data length"
    );
    reader.read_exact(&mut header[4..])?;
    ensure!(header[20] <= 1, "invalid EOF");
    let mut bytes = vec![0; size - 17];
    reader.read_exact(&mut bytes)?;
    Ok(DataReply {
        request_id: u64::from_be_bytes(header[4..12].try_into().unwrap()),
        offset: u64::from_be_bytes(header[12..20].try_into().unwrap()),
        eof: header[20] == 1,
        bytes,
    })
}

fn read_response(stream: &mut Client) -> Result<Message> {
    read_frame(stream)
}

pub fn connect(identity: &Identity, peer: &Peer) -> Result<Client> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pins {
            pairing: false,
            pins: vec![peer.fingerprint.clone()],
        }))
        .with_client_auth_cert(
            vec![CertificateDer::from(identity.cert.clone())],
            identity.key(),
        )?;
    config.alpn_protocols = vec![ALPN_V2.to_vec()];
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
        matches!(stream.conn.alpn_protocol(), Some(ALPN_V2)),
        "ALPN mismatch"
    );
    Ok(stream)
}

type IpcSourceKey = (String, String, String, String, u64);
#[derive(Default)]
struct ReadConnection {
    stream: Option<Client>,
    last_request: Option<Instant>,
    bytes: Vec<u8>,
    offset: u64,
    next: Option<u64>,
}
type SharedConnection = Arc<Mutex<ReadConnection>>;

/// Wire-equivalent IPC reply without a serde_json::Value allocation per data byte.
#[derive(Serialize)]
#[serde(untagged)]
pub enum IpcReply {
    Data {
        ok: bool,
        protocol: u16,
        response: IpcData,
    },
    Other(serde_json::Value),
}

// In-memory data reply; native adapters encode the payload as binary.
#[derive(Serialize)]
pub struct IpcData {
    bytes: Vec<u8>,
    eof: bool,
    offset: u64,
    request_id: u64,
    #[serde(rename = "type")]
    kind: &'static str,
}

/// File data has one binary encoding; control and error messages are JSON.
pub fn write_ipc_reply<W: Write>(writer: &mut W, reply: &IpcReply) -> Result<()> {
    if let IpcReply::Data { response, .. } = reply {
        write_binary_data(
            writer,
            response.request_id,
            response.offset,
            response.eof,
            &response.bytes,
        )
    } else {
        write_frame(writer, reply)
    }
}

pub struct Node {
    pub identity: Identity,
    pub engine: Mutex<Engine>,
    pub peers: Mutex<Vec<Peer>>,
    pub stop: AtomicBool,
    /// Only the latest accepted offer is queued: bounded even if clipboard STA is busy.
    pub incoming: Mutex<Option<(Offer, Peer)>>,
    active: AtomicUsize,
    // Bounded transport cache only: no file bytes, and every IPC read reauthorizes.
    ipc_connections: Mutex<Vec<(IpcSourceKey, SharedConnection)>>,
}
impl Node {
    /// Portable File Provider/FUSE IPC operations; OS adapter authenticates the local caller.
    pub fn local_ipc(self: &Arc<Self>, operation: IpcOperation) -> Result<serde_json::Value> {
        Ok(serde_json::to_value(self.local_ipc_reply(operation)?)?)
    }
    /// Native transports serialize this reply directly; validation is shared with local_ipc.
    pub fn local_ipc_reply(self: &Arc<Self>, operation: IpcOperation) -> Result<IpcReply> {
        match operation {
            IpcOperation::GetOffer => {
                let mut engine = self.engine.lock().unwrap();
                engine.expire();
                ensure!(!engine.paused, "paused");
                Ok(IpcReply::Other(
                    serde_json::json!({"ok":true,"protocol":1,"offer":engine.current}),
                ))
            }
            IpcOperation::GetFileOffer { offer_id } => {
                let offer = self.engine.lock().unwrap().file_offer(&offer_id)?;
                Ok(IpcReply::Other(
                    serde_json::json!({"ok":true,"protocol":1,"offer":offer}),
                ))
            }
            IpcOperation::GetFileOffers => {
                let offers = self.engine.lock().unwrap().retained_offers();
                Ok(IpcReply::Other(
                    serde_json::json!({"ok":true,"protocol":1,"offers":offers}),
                ))
            }
            IpcOperation::ReadBinary {
                request_id,
                offer_id,
                file_id,
                version,
                offset,
                length,
            } => {
                ensure!(length > 0 && length as usize <= MAX_CHUNK, "invalid range");
                let offer = self.engine.lock().unwrap().file_offer(&offer_id)?;
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
                    let key = (
                        offer.source_device.clone(),
                        offer.offer_id.clone(),
                        file.file_id.clone(),
                        file.version.clone(),
                        request_id,
                    );
                    let connection = {
                        let mut cache = self.ipc_connections.lock().unwrap();

                        if let Some((_, connection)) = cache.iter().find(|(k, _)| *k == key) {
                            connection.clone()
                        } else {
                            if cache.len() >= 2 {
                                cache.remove(0);
                            }
                            let connection = Arc::new(Mutex::new(ReadConnection::default()));
                            cache.push((key, connection.clone()));
                            connection
                        }
                    };
                    let mut remote = RemoteSource::new(self.clone(), peer, offer, file);
                    remote.connection = connection;
                    Arc::new(remote)
                };
                ensure!(offset <= source.len(), "invalid range");
                let mut bytes = vec![0; length as usize];
                let n = source.read_at(offset, &mut bytes)?;
                bytes.truncate(n);
                if offset + n as u64 == source.len() {
                    self.ipc_connections.lock().unwrap().retain(|(key, _)| {
                        key.1 != offer_id
                            || key.2 != file_id
                            || key.3 != version
                            || key.4 != request_id
                    });
                }
                Ok(IpcReply::Data {
                    ok: true,
                    protocol: 1,
                    response: IpcData {
                        bytes,
                        eof: offset + n as u64 == source.len(),
                        offset,
                        request_id,
                        kind: "data",
                    },
                })
            }
            IpcOperation::Cancel { request_id } => {
                self.ipc_connections
                    .lock()
                    .unwrap()
                    .retain(|(key, _)| key.4 != request_id);
                Ok(IpcReply::Other(
                    serde_json::json!({"ok":true,"protocol":1,"request_id":request_id}),
                ))
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
                .with_client_cert_verifier(Arc::new(Pins {
                    pins,
                    pairing: false,
                }))
                .with_single_cert(
                    vec![CertificateDer::from(self.identity.cert.clone())],
                    self.identity.key(),
                )?;
        config.alpn_protocols = vec![ALPN_V2.to_vec()];
        let mut stream = StreamOwned::new(ServerConnection::new(Arc::new(config))?, socket);
        while stream.conn.is_handshaking() {
            stream.conn.complete_io(&mut stream.sock)?;
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
                                && length as usize <= READ_WINDOW
                                && offset <= source.len() =>
                        {
                            let mut bytes = vec![0; length as usize];
                            let read = (|| -> Result<usize> {
                                let mut total = 0;
                                while total < bytes.len() && offset + (total as u64) < source.len()
                                {
                                    ensure!(
                                        self.trusted(&pin) && !self.stop.load(Ordering::Acquire),
                                        "revoked"
                                    );
                                    // Revalidate authority and source version between bounded reads.
                                    self.engine.lock().unwrap().source(
                                        &source_device,
                                        &offer_id,
                                        &file_id,
                                        &version,
                                    )?;
                                    let end = (total + MAX_CHUNK).min(bytes.len());
                                    let n = source
                                        .read_at(offset + total as u64, &mut bytes[total..end])?;
                                    ensure!(n > 0, "unexpected EOF");
                                    total += n;
                                }
                                Ok(total)
                            })();
                            match read {
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
            write_v2_response(&mut stream, &response)?;
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
            let _: Message = read_response(&mut stream)?;
        }
        write_frame(&mut stream, &Message::Poll)?;
        if let Message::Offer { offer } = read_response(&mut stream)? {
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
    connection: SharedConnection,
}
impl RemoteSource {
    pub fn new(node: Arc<Node>, peer: Peer, offer: Offer, file: FileEntry) -> Self {
        Self {
            node,
            peer,
            offer,
            file,
            connection: Arc::new(Mutex::new(ReadConnection::default())),
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
            self.node.trusted(&self.peer.fingerprint),
            "revoked or expired"
        );
        ensure!(
            offset <= self.len() && buffer.len() <= MAX_CHUNK,
            "invalid range"
        );
        if buffer.is_empty() {
            return Ok(0);
        }
        let mut connection = self.connection.lock().unwrap();
        let result = (|| {
            self.node.engine.lock().unwrap().touch_file(
                &self.offer.source_device,
                &self.offer.offer_id,
                &self.file.file_id,
                &self.file.version,
            )?;
            let available = offset
                .checked_sub(connection.offset)
                .and_then(|n| usize::try_from(n).ok());
            if let Some(start) = available.filter(|start| *start < connection.bytes.len()) {
                let n = buffer.len().min(connection.bytes.len() - start);
                buffer[..n].copy_from_slice(&connection.bytes[start..start + n]);
                connection.next = Some(offset + n as u64);
                return Ok(n);
            }
            // The server closes idle sockets after five seconds. Cached bytes do not
            // keep that socket alive; reconnect before issuing a later request.
            if connection
                .last_request
                .is_some_and(|at| at.elapsed() >= Duration::from_secs(4))
            {
                connection.stream = None;
            }
            if connection.stream.is_none() {
                connection.stream = Some(connect(&self.node.identity, &self.peer)?);
            }
            let length = if connection.next == Some(offset) && buffer.len() == MAX_CHUNK {
                (self.len() - offset).min(READ_WINDOW as u64) as usize
            } else {
                buffer.len()
            };
            connection.bytes.clear();
            connection.last_request = Some(Instant::now());
            let stream = connection.stream.as_mut().unwrap();
            write_frame(
                stream,
                &Message::Read {
                    request_id: 1,
                    source_device: self.offer.source_device.clone(),
                    offer_id: self.offer.offer_id.clone(),
                    file_id: self.file.file_id.clone(),
                    version: self.file.version.clone(),
                    offset,
                    length: length as u32,
                },
            )?;
            let reply = read_v2_data(stream, length)?;
            match reply {
                DataReply {
                    request_id: 1,
                    offset: returned,
                    bytes,
                    eof,
                    ..
                } if returned == offset
                    && bytes.len() <= length
                    && offset + bytes.len() as u64 <= self.len() =>
                {
                    ensure!(!bytes.is_empty() || eof, "empty non-EOF response");
                    ensure!(
                        eof == (offset + bytes.len() as u64 == self.len()),
                        "inconsistent EOF"
                    );
                    let n = bytes.len().min(buffer.len());
                    buffer[..n].copy_from_slice(&bytes[..n]);
                    connection.offset = offset;
                    connection.bytes = if bytes.len() > n { bytes } else { Vec::new() };
                    connection.next = Some(offset + n as u64);
                    Ok(n)
                }
                _ => anyhow::bail!("remote read rejected"),
            }
        })();
        if result.is_err() {
            *connection = ReadConnection::default();
        }
        result
    }
}

struct DataReply {
    request_id: u64,
    offset: u64,
    bytes: Vec<u8>,
    eof: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_frame_bounds_and_truncation() {
        let message = Message::Data {
            request_id: u64::MAX,
            offset: 9007199254740993,
            eof: true,
            bytes: vec![255; MAX_CHUNK],
        };
        let mut encoded = Vec::new();
        write_v2_response(&mut encoded, &message).unwrap();
        let reply = read_v2_data(&mut &encoded[..], MAX_CHUNK).unwrap();
        assert_eq!(reply.request_id, u64::MAX);
        assert_eq!(reply.offset, 9007199254740993);
        assert!(reply.eof);
        assert_eq!(reply.bytes, vec![255; MAX_CHUNK]);
        assert!(read_v2_data(&mut &encoded[..], MAX_CHUNK - 1).is_err());
        for n in [0, 3, 4, 20, encoded.len() - 1] {
            assert!(read_v2_data(&mut &encoded[..n], MAX_CHUNK).is_err());
        }
        encoded[20] = 2;
        assert!(read_v2_data(&mut &encoded[..], MAX_CHUNK).is_err());
        assert!(read_v2_data(&mut &u32::MAX.to_be_bytes()[..], READ_WINDOW).is_err());
    }

    #[test]
    fn negotiated_binary_window_is_lazy_bounded_and_revocable() {
        let a = Node::new(Identity::generate().unwrap(), vec![]);
        let b = Node::new(Identity::generate().unwrap(), vec![]);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let peer = Peer {
            address: listener.local_addr().unwrap(),
            fingerprint: a.identity.pin(),
        };
        a.peers.lock().unwrap().push(Peer {
            address: peer.address,
            fingerprint: b.identity.pin(),
        });
        b.peers.lock().unwrap().push(peer.clone());
        let source = Arc::new(PatternSource::new(READ_WINDOW as u64 * 3));
        let id = engine::new_id();
        let file = FileEntry {
            file_id: id.clone(),
            relative_path: "test.bin".into(),
            size: Some(source.len()),
            version: "v1".into(),
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
        let remote = RemoteSource::new(b.clone(), peer, offer, file);
        remote.read_at(0, &mut [0; 4096]).unwrap();
        assert_eq!(source.bytes_read.load(Ordering::Relaxed), 4096);
        a.engine
            .lock()
            .unwrap()
            .local(
                Content::Text {
                    text: "source changed".into(),
                },
                Default::default(),
            )
            .unwrap();
        b.sync_once(&remote.peer).unwrap();
        b.engine
            .lock()
            .unwrap()
            .local(
                Content::Text {
                    text: "receiver changed".into(),
                },
                Default::default(),
            )
            .unwrap();
        // Exercise a real server idle disconnect after clipboard replacement.
        std::thread::sleep(Duration::from_secs(6));
        let mut bytes = vec![0; MAX_CHUNK];
        remote.read_at(4096, &mut bytes).unwrap();
        assert_eq!(
            source.bytes_read.load(Ordering::Relaxed),
            4096 + READ_WINDOW as u64
        );
        remote.read_at(4096 + MAX_CHUNK as u64, &mut bytes).unwrap();
        assert_eq!(bytes[0], ((4096 + MAX_CHUNK) % 251) as u8);
        assert_eq!(
            source.bytes_read.load(Ordering::Relaxed),
            4096 + READ_WINDOW as u64
        );
        assert!(remote.connection.lock().unwrap().bytes.len() <= READ_WINDOW);
        b.engine.lock().unwrap().withdraw();
        assert!(
            remote
                .read_at(4096 + 2 * MAX_CHUNK as u64, &mut bytes)
                .is_err()
        );
        assert!(remote.connection.lock().unwrap().bytes.is_empty());
        a.stop.store(true, Ordering::Release);
        server.join().unwrap();
    }
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
        let ipc_read = || IpcOperation::ReadBinary {
            request_id: 7,
            offer_id: offer.offer_id.clone(),
            file_id: file.file_id.clone(),
            version: file.version.clone(),
            offset: 250,
            length: 100,
        };
        let remote = RemoteSource::new(b.clone(), peer.clone(), offer.clone(), file.clone());
        let mut bytes = [0; 100];
        assert_eq!(remote.read_at(250, &mut bytes).unwrap(), 100);
        assert_eq!(&bytes[..3], &[250, 0, 1]);
        for _ in 0..2 {
            let reply = b.local_ipc(ipc_read()).unwrap();
            assert_eq!(reply["response"]["bytes"][0], 250);
        }
        assert_eq!(b.ipc_connections.lock().unwrap().len(), 1);
        assert_eq!(source.bytes_read.load(Ordering::Relaxed), 300);
        // Existing cached TLS sessions must still fail after receiver-side unpair.
        b.peers.lock().unwrap().clear();
        assert!(b.local_ipc(ipc_read()).is_err());
        assert_eq!(source.bytes_read.load(Ordering::Relaxed), 300);
        b.peers.lock().unwrap().push(peer.clone());
        b.local_ipc(IpcOperation::Cancel { request_id: 7 }).unwrap();
        assert!(b.ipc_connections.lock().unwrap().is_empty());
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

pub mod pairing;
