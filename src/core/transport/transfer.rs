use super::*;
// A separate control connection owns the batch across read-connection idle
// reconnects. Closing it releases the source; no history grace or renewal timer.
pub(crate) struct BatchRead {
    pub(super) _local: Arc<()>,
    pub(super) _remote: Client,
}
pub(crate) type SharedBatchRead = Arc<Mutex<Option<BatchRead>>>;
#[derive(Default)]
pub(super) struct ReadConnection {
    pub(super) batch: SharedBatchRead,
    pub(super) stream: Option<Client>,
    pub(super) last_request: Option<Instant>,
    pub(super) bytes: Vec<u8>,
    pub(super) offset: u64,
    pub(super) next: Option<u64>,
}
pub(super) type SharedConnection = Arc<Mutex<ReadConnection>>;

impl Node {
    pub(super) fn begin_batch(&self, id: &str) -> Result<BatchRead> {
        let (offer, token) = {
            let mut engine = self.engine.lock().unwrap();
            (engine.file_offer(id)?, engine.retain_batch(id)?)
        };
        let peer = self
            .peers
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.device() == offer.source_device)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("source peer unavailable"))?;
        let mut stream = connect(&self.identity, &peer)?;
        batch_keepalive(&stream.sock)?;
        write_frame(
            &mut stream,
            &Message::Retain {
                offer_id: id.into(),
            },
        )?;
        ensure!(
            matches!(read_frame::<_, Message>(&mut stream)?, Message::Ack),
            "batch unavailable"
        );
        Ok(BatchRead {
            _local: token,
            _remote: stream,
        })
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn retain_session(&self, id: &str) -> Result<SharedBatchRead> {
        Ok(Arc::new(Mutex::new(Some(self.begin_batch(id)?))))
    }
}
pub struct RemoteSource {
    pub node: Arc<Node>,
    pub peer: Peer,
    pub offer: Offer,
    pub file: FileEntry,
    pub(super) connection: SharedConnection,
    pub(super) background: bool,
    pub(super) batch: SharedBatchRead,
}
impl RemoteSource {
    pub fn batch(node: Arc<Node>, peer: Peer, offer: Offer) -> Vec<Arc<Self>> {
        let batch = Arc::default();
        let Content::Files { files } = &offer.content else {
            return vec![];
        };
        files
            .iter()
            .map(|file| {
                let mut source = Self::new(node.clone(), peer.clone(), offer.clone(), file.clone());
                source.batch = Arc::clone(&batch);
                Arc::new(source)
            })
            .collect()
    }

    pub fn new(node: Arc<Node>, peer: Peer, offer: Offer, file: FileEntry) -> Self {
        Self {
            node,
            peer,
            offer,
            file,
            connection: Arc::new(Mutex::new(ReadConnection::default())),
            background: false,
            batch: Arc::default(),
        }
    }
}
impl ReadAt for RemoteSource {
    fn len(&self) -> u64 {
        self.file.size.unwrap_or(0)
    }
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize> {
        ensure!(!self.node.stop.load(Ordering::Acquire), "cancelled");
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
        {
            let mut batch = self.batch.lock().unwrap();
            if batch.is_none() {
                *batch = Some(self.node.begin_batch(&self.offer.offer_id)?);
            }
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
            let bytes = std::mem::take(&mut connection.bytes);
            connection.last_request = Some(Instant::now());
            let stream = connection.stream.as_mut().unwrap();
            write_frame(
                stream,
                &Message::Read {
                    background: self.background,
                    request_id: 1,
                    source_device: self.offer.source_device.clone(),
                    offer_id: self.offer.offer_id.clone(),
                    file_id: self.file.file_id.clone(),
                    version: self.file.version.clone(),
                    offset,
                    length: length as u32,
                },
            )?;
            let reply = read_v2_data(stream, length, bytes)?;
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
                    connection.bytes = bytes;
                    if connection.bytes.len() <= n {
                        connection.bytes.clear();
                    }
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
