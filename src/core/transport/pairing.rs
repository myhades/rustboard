//! Temporary, TLS-only identity enrollment. Never handles clipboard messages.
use super::*;
const ALPN: &[u8] = b"rustboard-pair/1";
pub struct Channel {
    stream: Box<dyn PairIo>,
    pub pin: String,
    pub socket: TcpStream,
}
trait PairIo: Read + Write + Send {}
impl<T: Read + Write + Send> PairIo for T {}
impl Channel {
    pub fn confirm(&mut self, mut approved: impl FnMut(bool) -> Result<bool>) -> Result<()> {
        let mut sent = false;
        let mut bytes = [0; 8];
        let mut offset = 0;
        loop {
            let received = offset == bytes.len();
            if approved(received)? && !sent {
                self.send(1)?;
                sent = true;
            }
            if received {
                if sent {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            match self.stream.read(&mut bytes[offset..]) {
                Ok(0) => anyhow::bail!("peer declined pairing"),
                Ok(n) => {
                    offset += n;
                    if offset == bytes.len() {
                        ensure!(u64::from_be_bytes(bytes) == 1, "peer declined pairing");
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    pub fn send(&mut self, value: u64) -> Result<()> {
        self.stream.write_all(&value.to_be_bytes())?;
        self.stream.flush()?;
        Ok(())
    }
    pub fn receive(&mut self, active: impl Fn() -> bool) -> Result<u64> {
        let mut b = [0; 8];
        let mut offset = 0;
        while offset < b.len() {
            ensure!(active(), "pairing cancelled or expired");
            match self.stream.read(&mut b[offset..]) {
                Ok(0) => anyhow::bail!("peer disconnected"),
                Ok(n) => offset += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(u64::from_be_bytes(b))
    }
}
fn socket(socket: &TcpStream) -> Result<()> {
    socket.set_nonblocking(false)?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    socket.set_write_timeout(Some(Duration::from_secs(5)))?;
    socket.set_nodelay(true)?;
    Ok(())
}
pub fn connect(identity: &Identity, address: SocketAddr) -> Result<Channel> {
    let mut config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&version::TLS13])?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Pins {
                pins: vec![],
                pairing: true,
            }))
            .with_client_auth_cert(
                vec![CertificateDer::from(identity.cert.clone())],
                identity.key(),
            )?;
    config.alpn_protocols = vec![ALPN.to_vec()];
    let sock = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
    socket(&sock)?;
    let conn = ClientConnection::new(Arc::new(config), ServerName::try_from("rustboard.local")?)?;
    let mut s = StreamOwned::new(conn, sock);
    while s.conn.is_handshaking() {
        s.conn.complete_io(&mut s.sock)?;
    }
    ensure!(
        s.conn.alpn_protocol() == Some(ALPN),
        "pairing ALPN mismatch"
    );
    let pin = fingerprint(
        s.conn
            .peer_certificates()
            .ok_or_else(|| anyhow::anyhow!("missing peer certificate"))?[0]
            .as_ref(),
    );
    // Human confirmation is bounded by the session deadline in the platform layer.
    s.sock.set_read_timeout(Some(Duration::from_millis(200)))?;
    let socket = s.sock.try_clone()?;
    Ok(Channel {
        stream: Box::new(s),
        pin,
        socket,
    })
}
pub fn accept(identity: &Identity, sock: TcpStream) -> Result<Channel> {
    socket(&sock)?;
    let mut config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&version::TLS13])?
            .with_client_cert_verifier(Arc::new(Pins {
                pins: vec![],
                pairing: true,
            }))
            .with_single_cert(
                vec![CertificateDer::from(identity.cert.clone())],
                identity.key(),
            )?;
    config.alpn_protocols = vec![ALPN.to_vec()];
    let mut s = StreamOwned::new(ServerConnection::new(Arc::new(config))?, sock);
    while s.conn.is_handshaking() {
        s.conn.complete_io(&mut s.sock)?;
    }
    ensure!(
        s.conn.alpn_protocol() == Some(ALPN),
        "pairing ALPN mismatch"
    );
    let pin = fingerprint(
        s.conn
            .peer_certificates()
            .ok_or_else(|| anyhow::anyhow!("missing peer certificate"))?[0]
            .as_ref(),
    );
    s.sock.set_read_timeout(Some(Duration::from_millis(200)))?;
    let socket = s.sock.try_clone()?;
    Ok(Channel {
        stream: Box::new(s),
        pin,
        socket,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enrollment_proves_both_keys_and_rejects_sync_alpn() {
        let a = Identity::generate().unwrap();
        let b = Identity::generate().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server_identity = a.clone();
        let expected = b.pin();
        let t = std::thread::spawn(move || {
            let mut c = accept(&server_identity, listener.accept().unwrap().0).unwrap();
            assert_eq!(c.pin, expected);
            assert_eq!(c.receive(|| true).unwrap(), 45452);
            std::thread::sleep(Duration::from_millis(350));
            c.send(45451).unwrap();
            // The second confirmer can observe approval before making its own choice.
            c.confirm(Ok).unwrap();
            c.send(156).unwrap();
            assert_eq!(c.receive(|| true).unwrap(), 3);
            let started = std::time::Instant::now();
            let error = c
                .receive(|| started.elapsed() < Duration::from_millis(100))
                .unwrap_err();
            assert!(error.to_string().contains("pairing cancelled"));
            assert!(started.elapsed() < Duration::from_secs(2));
        });
        let mut c = connect(&b, address).unwrap();
        assert_eq!(c.pin, a.pin());
        c.send(45452).unwrap();
        assert_eq!(c.receive(|| true).unwrap(), 45451);
        c.confirm(|_| Ok(true)).unwrap();
        c.send(3).unwrap();
        assert_eq!(c.receive(|| true).unwrap(), 156);
        t.join().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let pin = a.pin();
        let t = std::thread::spawn(move || {
            assert!(accept(&a, listener.accept().unwrap().0).is_err());
        });
        assert!(
            super::super::connect(
                &b,
                &Peer {
                    address,
                    fingerprint: pin
                }
            )
            .is_err()
        );
        t.join().unwrap();
    }
}
