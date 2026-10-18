use crate::core::*;
#[cfg(test)]
use crate::test_pattern::PatternSource;
use anyhow::{Result, ensure};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Weak},
    time::{SystemTime, UNIX_EPOCH},
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

// Bound concurrent active transfers, not idle clipboard history.
const MAX_ACTIVE_BATCHES: usize = 8;
struct FileBatch {
    offer: Offer,
    sources: HashMap<String, Arc<dyn ReadAt>>,
    readers: Weak<()>,
}
pub struct Engine {
    pub device_id: String,
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
        self.current = None;
        self.expire();
    }
    /// The caller owns this token for the native transfer lifetime, including gaps
    /// between bounded reads. The engine keeps no strong reference to it.
    pub fn retain_batch(&mut self, id: &str) -> Result<Arc<()>> {
        self.expire();
        let batch = self
            .batches
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("file offer expired"))?;
        if let Some(readers) = batch.readers.upgrade() {
            return Ok(readers);
        }
        let readers = Arc::new(());
        batch.readers = Arc::downgrade(&readers);
        Ok(readers)
    }
    fn register_batch(
        &mut self,
        offer: &Offer,
        sources: HashMap<String, Arc<dyn ReadAt>>,
    ) -> Result<()> {
        self.expire();
        if matches!(offer.content, Content::Files { .. }) {
            ensure!(
                self.batches
                    .values()
                    .filter(|b| b.readers.strong_count() > 0)
                    .count()
                    < MAX_ACTIVE_BATCHES,
                "file transfer capacity reached"
            );
            // Bound retained metadata as well as the number of stable-source handles.
            let size = serde_json::to_vec(offer)?.len();
            let total: usize = self
                .batches
                .values()
                .filter(|b| b.readers.strong_count() > 0)
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
                    readers: Weak::new(),
                },
            );
        }
        Ok(())
    }
    pub fn retained_offers(&mut self) -> Vec<Offer> {
        self.expire();
        self.batches.values().map(|b| b.offer.clone()).collect()
    }
    pub fn file_offer(&mut self, id: &str) -> Result<Offer> {
        self.expire();
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
        Ok(())
    }
    pub fn local(
        &mut self,
        content: Content,
        sources: HashMap<String, Arc<dyn ReadAt>>,
    ) -> Result<Offer> {
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
        self.expire();
        Ok(offer)
    }
    pub fn accept(&mut self, offer: Offer, authenticated_device: &str) -> Result<bool> {
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
        self.expire();
        self.accepted += 1;
        Ok(true)
    }
    pub fn expire(&mut self) {
        if self
            .current
            .as_ref()
            .is_some_and(|o| o.expires_unix_ms <= now_ms())
        {
            self.current = None;
        }
        let current = self.current.as_ref().map(|o| o.offer_id.as_str());
        self.batches
            .retain(|id, b| Some(id.as_str()) == current || b.readers.strong_count() > 0);
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
    fn only_current_and_owned_transfers_survive_replacement() {
        let mut e = Engine::new(new_id());
        let first = batch(&mut e);
        let transfer = e.retain_batch(&first.offer_id).unwrap();
        let second = batch(&mut e);
        let third = batch(&mut e);
        assert!(e.file_offer(&second.offer_id).is_err());
        assert!(e.file_offer(&first.offer_id).is_ok());
        assert!(e.file_offer(&third.offer_id).is_ok());
        drop(transfer);
        assert!(e.file_offer(&first.offer_id).is_err());
        for _ in 0..32 {
            let current = batch(&mut e);
            assert_eq!(e.retained_offers().len(), 1);
            assert_eq!(e.retained_offers()[0].offer_id, current.offer_id);
        }
        e.clear_clipboard();
        assert!(e.retained_offers().is_empty());
    }
    #[test]
    fn capacity_protects_active_transfers_and_withdraw_revokes() {
        let mut e = Engine::new(new_id());
        let mut transfers = Vec::new();
        for _ in 0..MAX_ACTIVE_BATCHES {
            let offer = batch(&mut e);
            transfers.push(e.retain_batch(&offer.offer_id).unwrap());
        }
        let current = e.current.clone();
        assert!(
            e.local(current.as_ref().unwrap().content.clone(), HashMap::new())
                .is_err()
        );
        assert_eq!(e.current, current);
        e.withdraw();
        assert!(e.retained_offers().is_empty());
    }
    #[test]
    fn malformed_clock_is_rejected_without_overwrite() {
        let path = std::env::temp_dir().join(new_id());
        for bytes in [
            b"123".as_slice(),
            b"{",
            br#"{"clock":1,"watermark":null,"unexpected":true}"#,
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(Engine::new(new_id()).persist_clock(path.clone()).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
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
        let mut remote = Engine::new(new_id());
        let mut incoming = remote
            .local(
                Content::Text {
                    text: "remote".into(),
                },
                HashMap::new(),
            )
            .unwrap();
        incoming.event_sequence = first.event_sequence + 1;
        assert!(a.accept(incoming.clone(), &remote.device_id).unwrap());
        drop(a);
        let mut a = Engine::new(id);
        a.persist_clock(path.clone()).unwrap();
        assert!(!a.accept(incoming.clone(), &remote.device_id).unwrap());
        assert!(a.current.is_none());
        let second = a
            .local(
                Content::Text {
                    text: "after".into(),
                },
                HashMap::new(),
            )
            .unwrap();
        assert!(second.event_sequence > incoming.event_sequence);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn invalid_file_tuple_cannot_read_or_renew_batch() {
        let mut e = Engine::new(new_id());
        let offer = batch(&mut e);
        let Content::Files { files } = &offer.content else {
            panic!()
        };
        for tuple in [
            (
                new_id(),
                offer.offer_id.clone(),
                files[0].file_id.clone(),
                "v".to_owned(),
            ),
            (
                e.device_id.clone(),
                new_id(),
                files[0].file_id.clone(),
                "v".to_owned(),
            ),
            (
                e.device_id.clone(),
                offer.offer_id.clone(),
                new_id(),
                "v".to_owned(),
            ),
            (
                e.device_id.clone(),
                offer.offer_id.clone(),
                files[0].file_id.clone(),
                "wrong".to_owned(),
            ),
        ] {
            assert!(e.source(&tuple.0, &tuple.1, &tuple.2, &tuple.3).is_err());
            assert_eq!(e.batches[&offer.offer_id].readers.strong_count(), 0);
        }
        assert!(
            e.source(
                &e.device_id.clone(),
                &offer.offer_id,
                &files[0].file_id,
                "v"
            )
            .is_ok()
        );
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
