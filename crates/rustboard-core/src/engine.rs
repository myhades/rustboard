use crate::*;
use anyhow::{Result, ensure};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

// A short handoff grace bridges native descriptor acquisition and the first read.
// Reads renew the whole batch so sequential files survive clipboard replacement.
const HANDOFF_GRACE: Duration = Duration::from_secs(30);
const TRANSFER_IDLE: Duration = Duration::from_secs(120);
const MAX_BATCHES: usize = 8;
struct FileBatch {
    offer: Offer,
    sources: HashMap<String, Arc<dyn ReadAt>>,
    retired: Option<Instant>,
    active_until: Option<Instant>,
}
pub struct Engine {
    pub device_id: String,
    pub paused: bool,
    pub text_only: bool,
    clock: u64,
    clock_path: Option<PathBuf>,
    watermark: Option<(u64, String, String)>,
    pub current: Option<Offer>,
    batches: HashMap<String, FileBatch>,
    pub accepted: u64,
}
impl Engine {
    pub fn new(device_id: String) -> Self {
        Self {
            device_id,
            paused: false,
            text_only: false,
            clock: 0,
            clock_path: None,
            watermark: None,
            current: None,
            batches: HashMap::new(),
            accepted: 0,
        }
    }
    pub fn persist_clock(&mut self, path: PathBuf) -> Result<()> {
        if path.exists() {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Clock {
                clock: u64,
                watermark: Option<(u64, String, String)>,
            }
            let value: Clock = serde_json::from_slice(&std::fs::read(&path)?)?;
            self.clock = value.clock;
            self.watermark = value.watermark;
        }
        self.clock_path = Some(path);
        Ok(())
    }
    fn save_clock(&self) -> Result<()> {
        if let Some(path) = &self.clock_path {
            let temporary = path.with_extension("new");
            let mut file = std::fs::File::create(&temporary)?;
            use std::io::Write;
            file.write_all(&serde_json::to_vec(
                &serde_json::json!({"clock":self.clock,"watermark":self.watermark}),
            )?)?;
            file.sync_all()?;
            std::fs::rename(temporary, path)?;
        }
        Ok(())
    }
    pub fn withdraw(&mut self) {
        self.current = None;
        self.batches.clear();
    }
    /// Clipboard replacement is not explicit transfer revocation.
    pub fn clear_clipboard(&mut self) {
        self.retire_current();
        self.current = None;
    }
    fn retire_current(&mut self) {
        if let Some(batch) = self
            .current
            .as_ref()
            .and_then(|o| self.batches.get_mut(&o.offer_id))
        {
            batch.retired.get_or_insert(Instant::now());
        }
    }
    fn register_batch(
        &mut self,
        offer: &Offer,
        sources: HashMap<String, Arc<dyn ReadAt>>,
    ) -> Result<()> {
        self.expire();
        if matches!(offer.content, Content::Files { .. }) {
            ensure!(
                self.batches.len() < MAX_BATCHES,
                "file transfer capacity reached"
            );
            // Bound retained metadata as well as the number of stable-source handles.
            let size = serde_json::to_vec(offer)?.len();
            let total: usize = self
                .batches
                .values()
                .map(|b| serde_json::to_vec(&b.offer).map(|v| v.len()))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .sum();
            ensure!(
                size + total < MAX_FRAME / 2,
                "file metadata capacity reached"
            );
            self.batches.insert(
                offer.offer_id.clone(),
                FileBatch {
                    offer: offer.clone(),
                    sources,
                    retired: None,
                    active_until: None,
                },
            );
        }
        self.retire_current();
        Ok(())
    }
    pub fn retained_offers(&mut self) -> Vec<Offer> {
        self.expire();
        self.batches.values().map(|b| b.offer.clone()).collect()
    }
    pub fn file_offer(&mut self, id: &str) -> Result<Offer> {
        self.expire();
        ensure!(!self.paused, "paused");
        Ok(self
            .batches
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("file offer expired"))?
            .offer
            .clone())
    }
    pub fn touch_file(
        &mut self,
        device: &str,
        offer: &str,
        file: &str,
        version: &str,
    ) -> Result<()> {
        self.expire();
        ensure!(!self.paused, "paused");
        let batch = self
            .batches
            .get_mut(offer)
            .ok_or_else(|| anyhow::anyhow!("file offer expired"))?;
        ensure!(batch.offer.source_device == device, "source mismatch");
        let Content::Files { files } = &batch.offer.content else {
            anyhow::bail!("not files")
        };
        ensure!(
            files
                .iter()
                .any(|f| f.file_id == file && f.version == version),
            "file/version mismatch"
        );
        batch.active_until = Some(Instant::now() + TRANSFER_IDLE);
        Ok(())
    }
    pub fn local(
        &mut self,
        content: Content,
        sources: HashMap<String, Arc<dyn ReadAt>>,
    ) -> Result<Offer> {
        ensure!(!self.paused, "paused");
        ensure!(
            !self.text_only || matches!(content, Content::Text { .. }),
            "text-only mode"
        );
        self.clock = self
            .clock
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("clock exhausted"))?;
        let offer = Offer {
            protocol: PROTOCOL_VERSION,
            source_device: self.device_id.clone(),
            offer_id: new_id(),
            event_sequence: self.clock,
            expires_unix_ms: now_ms() + 600_000,
            content,
        };
        offer.validate(now_ms())?;
        self.register_batch(&offer, sources)?;
        self.watermark = Some((
            offer.event_sequence,
            offer.source_device.clone(),
            offer.offer_id.clone(),
        ));
        self.save_clock()?;
        self.current = Some(offer.clone());
        Ok(offer)
    }
    pub fn files(&mut self, paths: Vec<PathBuf>) -> Result<Offer> {
        ensure!(paths.len() <= MAX_FILES, "too many files");
        let mut files = Vec::new();
        let mut sources: HashMap<String, Arc<dyn ReadAt>> = HashMap::new();
        for path in paths {
            let source = Arc::new(LocalFile::open(&path)?);
            let id = new_id();
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| anyhow::anyhow!("invalid filename"))?
                .to_string();
            files.push(FileEntry {
                file_id: id.clone(),
                relative_path: name,
                size: Some(source.len()),
                version: source.version(),
                media_type: "application/octet-stream".into(),
            });
            sources.insert(id, source);
        }
        self.local(Content::Files { files }, sources)
    }
    pub fn accept(&mut self, offer: Offer, authenticated_device: &str) -> Result<bool> {
        ensure!(!self.paused, "paused");
        offer.validate(now_ms())?;
        ensure!(
            offer.source_device == authenticated_device && offer.source_device != self.device_id,
            "origin mismatch"
        );
        ensure!(
            !self.text_only || matches!(offer.content, Content::Text { .. }),
            "text-only mode"
        );
        ensure!(offer.event_sequence < u64::MAX - 1, "clock exhausted");
        self.clock = self.clock.max(offer.event_sequence);
        if self.watermark.as_ref().is_some_and(|(seq, device, id)| {
            (*seq, device.as_str(), id.as_str()) >= offer.order_key()
        }) {
            return Ok(false);
        }
        self.register_batch(&offer, HashMap::new())?;
        self.watermark = Some((
            offer.event_sequence,
            offer.source_device.clone(),
            offer.offer_id.clone(),
        ));
        self.save_clock()?;
        self.current = Some(offer);
        self.accepted += 1;
        Ok(true)
    }
    pub fn expire(&mut self) {
        if self
            .current
            .as_ref()
            .is_some_and(|o| o.expires_unix_ms <= now_ms())
        {
            self.clear_clipboard();
        }
        let now = Instant::now();
        self.batches.retain(|_, b| {
            b.active_until.is_some_and(|until| until > now)
                || b.retired.map_or(b.offer.expires_unix_ms > now_ms(), |at| {
                    now.duration_since(at) < HANDOFF_GRACE
                })
        });
    }
    pub fn source(
        &mut self,
        device: &str,
        offer: &str,
        file: &str,
        version: &str,
    ) -> Result<Arc<dyn ReadAt>> {
        ensure!(device == self.device_id, "source is not local");
        self.touch_file(device, offer, file, version)?;
        Ok(self
            .batches
            .get(offer)
            .unwrap()
            .sources
            .get(file)
            .ok_or_else(|| anyhow::anyhow!("not found"))?
            .clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn batch(engine: &mut Engine) -> Offer {
        let files: Vec<_> = (0..2)
            .map(|_| FileEntry {
                file_id: new_id(),
                relative_path: new_id(),
                size: Some(4),
                version: "v".into(),
                media_type: "application/octet-stream".into(),
            })
            .collect();
        let sources = files
            .iter()
            .map(|f| {
                (
                    f.file_id.clone(),
                    Arc::new(PatternSource::new(4)) as Arc<dyn ReadAt>,
                )
            })
            .collect();
        engine.local(Content::Files { files }, sources).unwrap()
    }
    #[test]
    fn batch_survives_clipboard_changes_and_expires_after_idle() {
        let mut e = Engine::new(new_id());
        let first = batch(&mut e);
        let Content::Files { files } = &first.content else {
            panic!()
        };
        e.source(
            &first.source_device,
            &first.offer_id,
            &files[0].file_id,
            "v",
        )
        .unwrap();
        let second = batch(&mut e);
        e.local(
            Content::Text {
                text: "new clipboard".into(),
            },
            HashMap::new(),
        )
        .unwrap();
        // Old offer TTL and descriptor grace no longer cancel an active batch.
        let b = e.batches.get_mut(&first.offer_id).unwrap();
        b.retired = Some(Instant::now() - HANDOFF_GRACE - Duration::from_secs(1));
        b.offer.expires_unix_ms = 0;
        let source = e
            .source(
                &first.source_device,
                &first.offer_id,
                &files[1].file_id,
                "v",
            )
            .unwrap();
        let mut bytes = [0; 4];
        assert_eq!(source.read_at(0, &mut bytes).unwrap(), 4);
        assert_eq!(bytes, [0, 1, 2, 3]);
        assert!(e.file_offer(&second.offer_id).is_ok());
        e.batches.get_mut(&first.offer_id).unwrap().active_until =
            Some(Instant::now() - Duration::from_secs(1));
        assert!(e.file_offer(&first.offer_id).is_err());
        assert!(
            matches!(&e.current.as_ref().unwrap().content, Content::Text { text } if text == "new clipboard")
        );
    }
    #[test]
    fn capacity_never_evicts_active_batches_and_withdraw_revokes() {
        let mut e = Engine::new(new_id());
        for _ in 0..MAX_BATCHES {
            batch(&mut e);
        }
        let current = e.current.clone();
        let result = e.local(
            Content::Files {
                files: vec![FileEntry {
                    file_id: new_id(),
                    relative_path: "full".into(),
                    size: Some(0),
                    version: "v".into(),
                    media_type: "application/octet-stream".into(),
                }],
            },
            HashMap::new(),
        );
        assert!(result.is_err());
        assert_eq!(e.current, current);
        assert_eq!(e.batches.len(), MAX_BATCHES);
        e.local(
            Content::Text {
                text: "still usable".into(),
            },
            HashMap::new(),
        )
        .unwrap();
        e.withdraw();
        assert!(e.retained_offers().is_empty());
    }
    #[test]
    fn old_integer_clock_is_rejected() {
        let path = std::env::temp_dir().join(new_id());
        std::fs::write(&path, b"123").unwrap();
        assert!(Engine::new(new_id()).persist_clock(path.clone()).is_err());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn restart_preserves_sequence_and_replay_watermark() {
        let path = std::env::temp_dir().join(format!("rustboard-clock-{}.json", new_id()));
        let id = new_id();
        let mut a = Engine::new(id.clone());
        a.persist_clock(path.clone()).unwrap();
        let first = a
            .local(
                Content::Text {
                    text: "before".into(),
                },
                HashMap::new(),
            )
            .unwrap();
        drop(a);
        let mut a = Engine::new(id);
        a.persist_clock(path.clone()).unwrap();
        let second = a
            .local(
                Content::Text {
                    text: "after".into(),
                },
                HashMap::new(),
            )
            .unwrap();
        assert!(second.event_sequence > first.event_sequence);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn deterministic_conflict_and_expiration() {
        let mut a = Engine::new(new_id());
        let mut b = Engine::new(new_id());
        let oa = a
            .local(Content::Text { text: "a".into() }, HashMap::new())
            .unwrap();
        let ob = b
            .local(Content::Text { text: "b".into() }, HashMap::new())
            .unwrap();
        a.accept(ob.clone(), &b.device_id).unwrap();
        b.accept(oa.clone(), &a.device_id).unwrap();
        assert_eq!(a.current, b.current);
        assert!(!a.accept(ob, &b.device_id).unwrap());
        a.current.as_mut().unwrap().expires_unix_ms = 0;
        a.expire();
        assert!(a.current.is_none());
    }
}
