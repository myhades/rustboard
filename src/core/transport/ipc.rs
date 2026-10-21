use super::*;
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

impl Node {
    /// OS adapters authenticate the caller before dispatching portable IPC operations.
    pub fn local_ipc(self: &Arc<Self>, operation: IpcOperation) -> Result<serde_json::Value> {
        Ok(serde_json::to_value(self.local_ipc_reply(operation)?)?)
    }
    /// Native transports serialize this reply directly; validation is shared with local_ipc.
    pub fn local_ipc_reply(self: &Arc<Self>, operation: IpcOperation) -> Result<IpcReply> {
        self.local_ipc_session_reply(operation, None)
    }
    pub(crate) fn local_ipc_session_reply(
        self: &Arc<Self>,
        operation: IpcOperation,
        retained: Option<SharedBatchRead>,
    ) -> Result<IpcReply> {
        match operation {
            IpcOperation::GetOffer => {
                let mut engine = self.engine.lock().unwrap();
                engine.expire();
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
                let offers: Vec<_> = self
                    .engine
                    .lock()
                    .unwrap()
                    .retained_offers()
                    .into_iter()
                    .filter(|o| matches!(o.content, Content::Files { .. }))
                    .collect();
                Ok(IpcReply::Other(
                    serde_json::json!({"ok":true,"protocol":1,"offers":offers}),
                ))
            }
            IpcOperation::ReadBinary {
                background,
                request_id,
                offer_id,
                file_id,
                version,
                offset,
                length,
            } => {
                ensure!(length > 0 && length as usize <= MAX_CHUNK, "invalid range");
                let (source_device, file) = self
                    .engine
                    .lock()
                    .unwrap()
                    .file_entry(&offer_id, &file_id, &version)?;
                let source: Arc<dyn ReadAt> = if source_device == self.identity.device() {
                    self.engine.lock().unwrap().source(
                        &source_device,
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
                        .find(|p| p.device() == source_device)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("unauthorized"))?;
                    ensure!(file.size.is_some(), "unknown size unsupported");
                    let key = (
                        source_device.clone(),
                        offer_id.clone(),
                        file.file_id.clone(),
                        file.version.clone(),
                        request_id,
                        background,
                    );
                    let connection = {
                        let mut cache = self.ipc_connections.lock().unwrap();

                        if let Some((_, connection)) = cache.iter().find(|(k, _)| *k == key) {
                            connection.clone()
                        } else {
                            ensure!(cache.len() < 8, "active read capacity reached");
                            let connection = Arc::new(Mutex::new(ReadConnection::default()));
                            cache.push((key, connection.clone()));
                            connection
                        }
                    };
                    let mut remote = RemoteSource::new(self.clone(), peer, offer_id.clone(), file);
                    remote.batch = retained
                        .clone()
                        .unwrap_or_else(|| connection.lock().unwrap().batch.clone());
                    remote.connection = connection;
                    remote.background = background;
                    Arc::new(remote)
                };
                let source = source.open_reader()?.unwrap_or(source);
                ensure!(offset <= source.len(), "invalid range");
                let mut bytes = vec![0; length as usize];
                let n = if background {
                    source.read_background(offset, &mut bytes)?
                } else {
                    source.read_at(offset, &mut bytes)?
                };
                bytes.truncate(n);
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
            IpcOperation::Retain { .. } => anyhow::bail!("retain requires a native session"),
            IpcOperation::Control { .. } => {
                anyhow::bail!("control operation belongs to native adapter")
            }
        }
    }
}
