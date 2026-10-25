use super::*;
use std::{
    io,
    net::Shutdown,
    sync::{Condvar, mpsc},
};

pub(super) struct PeerSync {
    connection: Option<Arc<Control>>,
    wake: mpsc::SyncSender<()>,
    retry_at: Option<Instant>,
    retry_delay: u64,
    connecting: bool,
    error: Option<&'static str>,
}

pub(super) struct Control {
    tls: Mutex<rustls::Connection>,
    socket: TcpStream,
    outbound: bool,
    wake: mpsc::SyncSender<()>,
    messages: Mutex<Vec<Message>>,
    busy: Mutex<Option<String>>,
    probe: Mutex<(u64, u64)>, // last requested and last answered
    probe_ready: Condvar,
}
impl Control {
    fn close(&self) {
        let _ = self.socket.shutdown(Shutdown::Both);
    }
    fn flush(&self) -> io::Result<()> {
        let mut encrypted = Vec::new();
        let mut tls = self.tls.lock().unwrap();
        while tls.wants_write() {
            tls.write_tls(&mut encrypted)?;
        }
        drop(tls);
        (&self.socket).write_all(&encrypted)
    }
    fn send(&self, message: &Message) -> Result<()> {
        let mut frame = Vec::new();
        write_frame(&mut frame, message)?;
        let mut remaining = frame.as_slice();
        while !remaining.is_empty() {
            let n = self.tls.lock().unwrap().writer().write(remaining)?;
            ensure!(n != 0, "control write stopped");
            remaining = &remaining[n..];
            self.flush()?;
        }
        Ok(())
    }
    fn queue(&self, message: Message) -> Result<()> {
        let mut messages = self.messages.lock().unwrap();
        ensure!(messages.len() < 8, "control response capacity reached");
        messages.push(message);
        let _ = self.wake.try_send(());
        Ok(())
    }
    fn responsive(&self) -> Result<bool> {
        let mut probe = self.probe.lock().unwrap();
        probe.0 += 1;
        let request_id = probe.0;
        self.queue(Message::Ping { request_id })?;
        let (probe, _) = self
            .probe_ready
            .wait_timeout_while(probe, Duration::from_secs(1), |probe| probe.1 < request_id)
            .unwrap();
        Ok(probe.1 >= request_id)
    }
}

// Block on the socket without holding TLS state needed by the sending thread.
struct ControlReader<'a>(&'a Control);
impl Read for ControlReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.0.tls.lock().unwrap().reader().read(bytes) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                result => return result,
            }
            let mut encrypted = [0; 16 * 1024];
            let n = (&self.0.socket).read(&mut encrypted)?;
            if n == 0 {
                return Ok(0);
            }
            let mut input = &encrypted[..n];
            let mut tls = self.0.tls.lock().unwrap();
            while !input.is_empty() {
                tls.read_tls(&mut input)?;
                tls.process_new_packets().map_err(io::Error::other)?;
            }
            if tls.wants_write() {
                let _ = self.0.wake.try_send(());
            }
        }
    }
}

impl Node {
    pub fn start_sync(self: &Arc<Self>) {
        let peers = self.peers.lock().unwrap().clone();
        for peer in peers {
            self.sync_peer(&peer);
        }
    }

    fn sync_peer(self: &Arc<Self>, peer: &Peer) {
        let mut states = self.peer_connections.lock().unwrap();
        if states.contains_key(&peer.address) || self.stop.load(Ordering::Acquire) {
            return;
        }
        // Only a wake-up is queued. The worker reads the latest offer when ready.
        let (wake, updates) = mpsc::sync_channel(1);
        self.engine.lock().unwrap().changes.push(wake.clone());
        states.insert(
            peer.address,
            PeerSync {
                connection: None,
                wake,
                retry_at: None,
                retry_delay: 1,
                connecting: false,
                error: None,
            },
        );
        let node = self.clone();
        let peer = peer.clone();
        std::thread::spawn(move || node.sync_worker(peer, updates));
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Release);
        for state in self.peer_connections.lock().unwrap().values() {
            if let Some(connection) = &state.connection {
                connection.close();
            }
            let _ = state.wake.try_send(());
        }
    }

    pub fn peer_status(&self, address: SocketAddr) -> serde_json::Value {
        let states = self.peer_connections.lock().unwrap();
        let state = states.get(&address);
        if state.is_some_and(|s| s.connection.is_some()) {
            return serde_json::json!({"status":"connected"});
        }
        if let Some(error) = state.and_then(|s| s.error) {
            return serde_json::json!({"status":"error", "error":error});
        }
        let retry = state
            .and_then(|s| s.retry_at)
            .and_then(|at| at.checked_duration_since(Instant::now()))
            .map(|delay| delay.as_secs() + u64::from(delay.subsec_nanos() != 0));
        serde_json::json!({"status":"disconnected", "retry_in_seconds":retry})
    }

    pub(super) fn control(
        self: &Arc<Self>,
        peer: &Peer,
        tls: rustls::Connection,
        socket: TcpStream,
        outbound: bool,
    ) -> Result<Option<Arc<Control>>> {
        ensure!(
            !self.stop.load(Ordering::Acquire) && self.trusted(&peer.fingerprint),
            "peer revoked"
        );
        tcp_keepalive(&socket)?;
        socket.set_read_timeout(None)?;
        self.sync_peer(peer);
        let mut states = self.peer_connections.lock().unwrap();
        let state = states
            .get(&peer.address)
            .ok_or_else(|| anyhow::anyhow!("sync stopped"))?;
        let connection = Arc::new(Control {
            tls: Mutex::new(tls),
            socket,
            outbound,
            wake: state.wake.clone(),
            messages: Mutex::new(Vec::new()),
            busy: Mutex::new(None),
            probe: Mutex::new((0, 0)),
            probe_ready: Condvar::new(),
        });
        // Both peers choose the connection initiated by the same device.
        let preferred = self.identity.pin() < peer.fingerprint;
        loop {
            let old = states[&peer.address]
                .connection
                .as_ref()
                .filter(|old| old.outbound != outbound && old.outbound == preferred)
                .cloned();
            let Some(old) = old else { break };
            // A replacement must not be rejected solely because a half-open socket exists.
            drop(states);
            let responsive = old.responsive().unwrap_or(false);
            states = self.peer_connections.lock().unwrap();
            if !states[&peer.address]
                .connection
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, &old))
            {
                continue;
            }
            if responsive {
                connection.close();
                return Ok(None);
            }
            break;
        }
        ensure!(!self.stop.load(Ordering::Acquire), "sync stopped");
        let state = states.get_mut(&peer.address).unwrap();
        if let Some(old) = state.connection.replace(connection.clone()) {
            old.close();
        }
        state.retry_at = None;
        state.retry_delay = 1;
        state.error = None;
        let _ = state.wake.try_send(());
        log::info!(target: "network", "peer {} connected", peer.address);
        Ok(Some(connection))
    }

    pub(super) fn receive_control(&self, peer: &Peer, connection: &Arc<Control>) {
        let result = (|| -> Result<()> {
            let mut reader = ControlReader(connection);
            while !self.stop.load(Ordering::Acquire) {
                let message = read_frame(&mut reader)?;
                ensure!(self.trusted(&peer.fingerprint), "peer revoked");
                match message {
                    Message::Offer { offer } => {
                        if self.engine.lock().unwrap().text_only
                            && !matches!(offer.content, Content::Text { .. })
                        {
                            continue;
                        }
                        let offer_id = offer.offer_id.clone();
                        if let Err(error) = self.accept(offer, peer.clone()) {
                            if !error.is::<engine::CapacityExceeded>() {
                                return Err(error);
                            }
                            connection.queue(Message::OfferRejected {
                                offer_id,
                                code: ErrorCode::Busy,
                            })?;
                        }
                    }
                    Message::OfferRejected {
                        offer_id,
                        code: ErrorCode::Busy,
                    } => {
                        uuid::Uuid::parse_str(&offer_id)?;
                        *connection.busy.lock().unwrap() = Some(offer_id);
                        let _ = connection.wake.try_send(());
                    }
                    Message::Ping { request_id } => {
                        connection.queue(Message::Pong { request_id })?
                    }
                    Message::Pong { request_id } => {
                        let mut probe = connection.probe.lock().unwrap();
                        if request_id <= probe.0 {
                            probe.1 = probe.1.max(request_id);
                            connection.probe_ready.notify_all();
                        }
                    }
                    _ => anyhow::bail!("invalid control message"),
                }
            }
            Ok(())
        })();
        self.control_closed(peer, connection, result.err().as_ref());
    }

    fn control_closed(
        &self,
        peer: &Peer,
        connection: &Arc<Control>,
        error: Option<&anyhow::Error>,
    ) {
        connection.close();
        let mut states = self.peer_connections.lock().unwrap();
        let Some(state) = states.get_mut(&peer.address) else {
            return;
        };
        if !state
            .connection
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, connection))
        {
            return;
        }
        state.connection = None;
        state.retry_at = Some(Instant::now() + Duration::from_secs(1));
        state.error = error.and_then(sync_error);
        let _ = state.wake.try_send(());
        if !self.stop.load(Ordering::Acquire) {
            log::info!(target: "network", "peer {} disconnected", peer.address);
            if let Some(error) = error {
                if state.error.is_some() {
                    log::warn!(target: "network", "peer {}: {}", peer.address, network_error(error));
                } else {
                    log::debug!(target: "network", "peer {}: {}", peer.address, network_error(error));
                }
            }
        }
    }

    fn sync_worker(self: Arc<Self>, peer: Peer, updates: mpsc::Receiver<()>) {
        let mut previous: Option<Arc<Control>> = None;
        let mut sent = None;
        let mut retry_offer: Option<(String, Instant)> = None;
        while !self.stop.load(Ordering::Acquire) && self.trusted(&peer.fingerprint) {
            let (connection, retry_at, connecting) = {
                let states = self.peer_connections.lock().unwrap();
                let state = &states[&peer.address];
                (state.connection.clone(), state.retry_at, state.connecting)
            };
            if let Some(connection) = connection {
                // All writes, including receiver responses and probes, use this one worker.
                let responses = std::mem::take(&mut *connection.messages.lock().unwrap());
                let result = (|| -> Result<()> {
                    connection.flush()?;
                    for message in responses {
                        connection.send(&message)?;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    self.control_closed(&peer, &connection, Some(&error));
                    continue;
                }
                if previous
                    .as_ref()
                    .is_none_or(|old| !Arc::ptr_eq(old, &connection))
                {
                    sent = None;
                    retry_offer = None;
                    previous = Some(connection.clone());
                }
                if let Some(id) = connection.busy.lock().unwrap().take()
                    && sent.as_ref() == Some(&id)
                {
                    retry_offer = Some((id, Instant::now() + Duration::from_secs(1)));
                }
                let offer = {
                    let mut engine = self.engine.lock().unwrap();
                    engine.expire();
                    let current = engine
                        .current
                        .as_ref()
                        .filter(|offer| offer.source_device == engine.device_id);
                    if retry_offer
                        .as_ref()
                        .is_some_and(|(id, _)| current.is_none_or(|offer| &offer.offer_id != id))
                    {
                        retry_offer = None;
                    }
                    current
                        .filter(|offer| {
                            sent.as_ref() != Some(&offer.offer_id)
                                || retry_offer
                                    .as_ref()
                                    .is_some_and(|(_, at)| *at <= Instant::now())
                        })
                        .cloned()
                };
                if let Some(offer) = offer {
                    let id = offer.offer_id.clone();
                    if let Err(error) = connection.send(&Message::Offer { offer }) {
                        self.control_closed(&peer, &connection, Some(&error));
                        continue;
                    }
                    sent = Some(id);
                    retry_offer = None;
                }
                if let Some((_, at)) = retry_offer {
                    let _ = updates.recv_timeout(at.saturating_duration_since(Instant::now()));
                } else if updates.recv().is_err() {
                    break;
                }
                continue;
            }
            previous = None;
            if connecting {
                if updates.recv().is_err() {
                    break;
                }
                continue;
            }
            if let Some(delay) = retry_at.and_then(|at| at.checked_duration_since(Instant::now())) {
                let _ = updates.recv_timeout(delay);
                continue;
            }
            let mut states = self.peer_connections.lock().unwrap();
            let state = states.get_mut(&peer.address).unwrap();
            if state.connection.is_some() || state.connecting {
                continue;
            }
            state.connecting = true;
            drop(states);
            let node = self.clone();
            let peer = peer.clone();
            std::thread::spawn(move || node.connect_peer(peer));
        }
        if let Some(state) = self.peer_connections.lock().unwrap().get_mut(&peer.address)
            && let Some(connection) = state.connection.take()
        {
            connection.close();
        }
    }

    fn connect_peer(self: Arc<Self>, peer: Peer) {
        let result = (|| -> Result<Option<Arc<Control>>> {
            let mut stream = connect(&self.identity, &peer)?;
            write_frame(
                &mut stream,
                &Message::Hello {
                    protocol: PROTOCOL_VERSION,
                    device_id: self.identity.device(),
                },
            )?;
            ensure!(
                matches!(read_frame(&mut stream)?, Message::Hello { protocol, device_id }
                    if protocol == PROTOCOL_VERSION && device_id == peer.device()),
                "identity or protocol mismatch"
            );
            self.control(
                &peer,
                rustls::Connection::Client(stream.conn),
                stream.sock,
                true,
            )
        })();
        {
            let mut states = self.peer_connections.lock().unwrap();
            let state = states.get_mut(&peer.address).unwrap();
            state.connecting = false;
            // An incoming connection can succeed while the outgoing attempt fails.
            if let Err(error) = &result
                && state.connection.is_none()
                && !self.stop.load(Ordering::Acquire)
            {
                state.retry_at = Some(Instant::now() + Duration::from_secs(state.retry_delay));
                let reason = sync_error(error);
                if reason.is_some() && state.error != reason {
                    log::warn!(target: "network", "peer {} connection failed: {}", peer.address, network_error(error));
                } else {
                    log::debug!(target: "network", "peer {} connection failed: {}", peer.address, network_error(error));
                }
                state.error = reason;
                state.retry_delay = (state.retry_delay * 2).min(30);
            }
            let _ = state.wake.try_send(());
        }
        if let Ok(Some(connection)) = result {
            self.receive_control(&peer, &connection);
        }
    }
}

fn sync_error(error: &anyhow::Error) -> Option<&'static str> {
    for cause in error.chain() {
        if let Some(error) = cause.downcast_ref::<rustls::Error>().or_else(|| {
            cause
                .downcast_ref::<io::Error>()?
                .get_ref()?
                .downcast_ref::<rustls::Error>()
        }) {
            return Some(match error {
                rustls::Error::General(message) if message == "unpaired certificate" => {
                    "certificate mismatch"
                }
                rustls::Error::NoApplicationProtocol => "protocol mismatch",
                _ => "tls handshake failed",
            });
        }
    }
    if error.chain().any(|cause| cause.is::<io::Error>()) {
        return None;
    }
    Some(match error.to_string().as_str() {
        "ALPN mismatch" | "identity or protocol mismatch" => "protocol mismatch",
        "peer revoked" => "peer revoked",
        _ => "invalid message",
    })
}

#[cfg(test)]
mod tests;
