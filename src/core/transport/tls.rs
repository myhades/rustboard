use super::*;
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
    pub(super) fn key(&self) -> rustls::pki_types::PrivateKeyDer<'static> {
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
pub(super) struct Pins {
    pub(super) pairing: bool,
    pub(super) pins: Vec<String>,
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

// complete_io may perform many reads internally; enforce the total budget on each I/O.
pub(super) struct HandshakeIo<'a> {
    socket: &'a mut TcpStream,
    deadline: Instant,
    active: Option<&'a dyn Fn() -> bool>,
}
impl<'a> HandshakeIo<'a> {
    pub(super) fn new(socket: &'a mut TcpStream) -> Self {
        Self {
            socket,
            deadline: Instant::now() + Duration::from_secs(5),
            active: None,
        }
    }
    pub(super) fn while_active(mut self, active: &'a dyn Fn() -> bool) -> Self {
        self.active = Some(active);
        self
    }
    fn remaining(&self) -> std::io::Result<Duration> {
        if self.active.is_some_and(|active| !active()) {
            return Err(std::io::ErrorKind::ConnectionAborted.into());
        }
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| std::io::ErrorKind::TimedOut.into())
    }
}
impl Read for HandshakeIo<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        loop {
            self.socket
                .set_read_timeout(Some(self.remaining()?.min(Duration::from_millis(200))))?;
            match self.socket.read(bytes) {
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue;
                }
                result => return result,
            }
        }
    }
}
impl Write for HandshakeIo<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.socket.set_write_timeout(Some(self.remaining()?))?;
        self.socket.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.socket.flush()
    }
}
impl Drop for HandshakeIo<'_> {
    fn drop(&mut self) {
        let _ = self.socket.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = self.socket.set_write_timeout(Some(Duration::from_secs(5)));
    }
}

pub type Client = StreamOwned<ClientConnection, TcpStream>;
pub(super) const ALPN_V2: &[u8] = b"rustboard/2";
pub(super) fn crypto_provider() -> rustls::crypto::CryptoProvider {
    let mut provider = rustls::crypto::ring::default_provider();
    let prefer_chacha = {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            !(std::is_x86_feature_detected!("aes") && std::is_x86_feature_detected!("pclmulqdq"))
        }
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        {
            false
        }
    };
    if prefer_chacha {
        provider
            .cipher_suites
            .sort_by_key(|suite| suite.suite() != CipherSuite::TLS13_CHACHA20_POLY1305_SHA256);
    }
    provider
}
pub fn connect(identity: &Identity, peer: &Peer) -> Result<Client> {
    let provider = Arc::new(crypto_provider());
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
    {
        let mut io = HandshakeIo::new(&mut stream.sock);
        while stream.conn.is_handshaking() {
            stream.conn.complete_io(&mut io)?;
        }
    }
    ensure!(
        matches!(stream.conn.alpn_protocol(), Some(ALPN_V2)),
        "ALPN mismatch"
    );
    Ok(stream)
}
