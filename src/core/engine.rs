use crate::core::*;
#[cfg(test)]
use crate::test_pattern::PatternSource;
use anyhow::{Result, ensure};
use std::{
    collections::{HashMap, HashSet},
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
    entries: HashMap<String, usize>,
    readers: Weak<()>,
    roots: Vec<PathBuf>,
    sent: HashMap<String, HashMap<usize, u64>>,
    received: HashSet<String>,
    moved: HashSet<String>,
    move_finished: bool,
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
    pub fn clock(&self) -> u64 {
        self.clock
    }
    pub fn observe_clock(&mut self, clock: u64) -> Result<()> {
        ensure!(clock < u64::MAX - 1, "clock exhausted");
        if clock > self.clock {
            self.clock = clock;
            self.save_clock()?;
        }
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
        if !offer.content.entries().is_empty() {
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
                size + total <= MAX_FRAME - MAX_MANIFEST,
                "file metadata capacity reached"
            );
            self.batches.insert(
                offer.offer_id.clone(),
                FileBatch {
                    offer: offer.clone(),
                    sources,
                    entries: offer
                        .content
                        .entries()
                        .iter()
                        .enumerate()
                        .map(|(index, f)| (f.file_id.clone(), index))
                        .collect(),
                    readers: Weak::new(),
                    roots: Vec::new(),
                    sent: HashMap::new(),
                    received: HashSet::new(),
                    moved: HashSet::new(),
                    move_finished: false,
                },
            );
        }
        Ok(())
    }
    pub fn retained_ids(&mut self) -> Vec<String> {
        self.expire();
        self.batches.keys().cloned().collect()
    }
    pub fn batch_device(&mut self, id: &str) -> Result<String> {
        self.expire();
        Ok(self
            .batches
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("file offer expired"))?
            .offer
            .source_device
            .clone())
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
        ensure!(
            batch.entries.get(file).is_some_and(|i| {
                let f = &batch.offer.content.entries()[*i];
                !f.directory && f.version == version
            }),
            "file/version mismatch"
        );
        Ok(())
    }
    pub fn file_entry(
        &mut self,
        offer: &str,
        file: &str,
        version: &str,
    ) -> Result<(String, FileEntry)> {
        self.expire();
        let batch = self
            .batches
            .get(offer)
            .ok_or_else(|| anyhow::anyhow!("file offer expired"))?;
        let entry = batch
            .entries
            .get(file)
            .map(|i| &batch.offer.content.entries()[*i])
            .filter(|f| !f.directory && f.version == version)
            .ok_or_else(|| anyhow::anyhow!("version_changed"))?;
        Ok((batch.offer.source_device.clone(), entry.clone()))
    }
    pub fn local(
        &mut self,
        content: Content,
        sources: HashMap<String, Arc<dyn ReadAt>>,
    ) -> Result<Offer> {
        self.local_content(content, sources, Vec::new(), false)
    }
    pub fn local_files(
        &mut self,
        files: Vec<FileEntry>,
        sources: HashMap<String, Arc<dyn ReadAt>>,
        roots: Vec<PathBuf>,
        cut: bool,
    ) -> Result<Offer> {
        self.local_content(Content::Files { files }, sources, roots, cut)
    }
    fn local_content(
        &mut self,
        mut content: Content,
        sources: HashMap<String, Arc<dyn ReadAt>>,
        roots: Vec<PathBuf>,
        cut: bool,
    ) -> Result<Offer> {
        ensure!(
            !self.text_only || matches!(content, Content::Text { .. }),
            "text-only mode"
        );
        if self.text_only
            && let Content::Text { html, rtf, .. } = &mut content
        {
            *html = None;
            *rtf = None;
        }
        self.clock = self
            .clock
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("clock exhausted"))?;
        let offer = Offer {
            cut,
            protocol: PROTOCOL_VERSION,
            source_device: self.device_id.clone(),
            offer_id: new_id(),
            event_sequence: self.clock,
            expires_unix_ms: now_ms() + 600_000,
            content,
        };
        offer.validate(now_ms())?;
        self.register_batch(&offer, sources)?;
        if let Some(batch) = self.batches.get_mut(&offer.offer_id) {
            batch.roots = roots;
        }
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
    // Track delivered ranges per peer; reads by different peers cannot complete a move together.
    pub fn sent(&mut self, offer: &str, peer: &str, file: &str, offset: u64, length: u64) {
        let Some(batch) = self.batches.get_mut(offer) else {
            return;
        };
        if batch.roots.is_empty() {
            return;
        }
        let Some(&index) = batch.entries.get(file) else {
            return;
        };
        if batch.sent.len() >= 16 && !batch.sent.contains_key(peer) {
            return;
        }
        let prefix = batch
            .sent
            .entry(peer.into())
            .or_default()
            .entry(index)
            .or_default();
        if offset <= *prefix {
            *prefix = (*prefix).max(offset.saturating_add(length));
        }
    }
    pub fn take_recycle_roots(&mut self, offer: &str, peer: &str) -> Result<Vec<PathBuf>> {
        self.expire();
        let batch = self
            .batches
            .get_mut(offer)
            .ok_or_else(|| anyhow::anyhow!("file offer expired"))?;
        ensure!(
            batch.offer.source_device == self.device_id,
            "source is not local"
        );
        let Content::Files { files } = &batch.offer.content else {
            anyhow::bail!("not files")
        };
        ensure!(
            files.iter().enumerate().all(|(i, f)| f.directory
                || f.size.is_some_and(|size| {
                    batch
                        .sent
                        .get(peer)
                        .and_then(|sent| sent.get(&i))
                        .copied()
                        .unwrap_or(0)
                        >= size
                })),
            "batch transfer incomplete"
        );
        Ok(std::mem::take(&mut batch.roots))
    }
    // A download receipt is only enough for source-initiated cuts. A native move
    // must acknowledge every selected root, including empty directories.
    pub fn received(&mut self, offer: &str, file: &str, moving: bool) -> Result<bool> {
        let batch = self
            .batches
            .get_mut(offer)
            .ok_or_else(|| anyhow::anyhow!("file offer expired"))?;
        let Content::Files { files } = &batch.offer.content else {
            anyhow::bail!("not files")
        };
        let index = *batch
            .entries
            .get(file)
            .ok_or_else(|| anyhow::anyhow!("file not found"))?;
        if moving {
            batch.moved.insert(file.into());
        } else if !files[index].directory {
            batch.received.insert(file.into());
        }
        let moved = files
            .iter()
            .filter(|f| !f.relative_path.contains('/'))
            .all(|f| batch.moved.contains(&f.file_id));
        let downloaded = batch.offer.cut
            && files.iter().any(|f| !f.directory)
            && files
                .iter()
                .filter(|f| !f.directory)
                .all(|f| batch.received.contains(&f.file_id));
        if batch.move_finished || !(moved || downloaded) {
            return Ok(false);
        }
        batch.move_finished = true;
        Ok(true)
    }
    pub fn accept(&mut self, mut offer: Offer, authenticated_device: &str) -> Result<bool> {
        offer.validate(now_ms())?;
        ensure!(
            offer.source_device == authenticated_device && offer.source_device != self.device_id,
            "origin mismatch"
        );
        ensure!(
            !self.text_only || matches!(offer.content, Content::Text { .. }),
            "text-only mode"
        );
        if self.text_only
            && let Content::Text { html, rtf, .. } = &mut offer.content
        {
            *html = None;
            *rtf = None;
        }
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
    #[test]
    fn recycling_requires_one_peer_to_receive_the_entire_batch_once() {
        let mut engine = Engine::new(new_id());
        let mut offer = batch(&mut engine);
        let Content::Files { files } = &mut offer.content else {
            unreachable!()
        };
        let first = files[0].file_id.clone();
        let second = files[1].file_id.clone();
        let roots = vec![PathBuf::from("first"), PathBuf::from("second")];
        engine.batches.get_mut(&offer.offer_id).unwrap().roots = roots.clone();
        assert!(engine.take_recycle_roots(&offer.offer_id, "a").is_err());
        engine.sent(&offer.offer_id, "a", &first, 0, 4);
        engine.sent(&offer.offer_id, "b", &second, 0, 4);
        assert!(engine.take_recycle_roots(&offer.offer_id, "a").is_err());
        engine.sent(&offer.offer_id, "a", &second, 2, 2);
        assert!(engine.take_recycle_roots(&offer.offer_id, "a").is_err());
        engine.sent(&offer.offer_id, "a", &second, 0, 4);
        assert_eq!(
            engine.take_recycle_roots(&offer.offer_id, "a").unwrap(),
            roots
        );
        assert!(
            engine
                .take_recycle_roots(&offer.offer_id, "a")
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn move_receipts_wait_for_every_root_and_cut_downloads_for_every_file() {
        let mut engine = Engine::new(new_id());
        let offer = batch(&mut engine);
        let files = offer.content.entries();
        assert!(
            !engine
                .received(&offer.offer_id, &files[0].file_id, false)
                .unwrap()
        );
        assert!(
            !engine
                .received(&offer.offer_id, &files[1].file_id, false)
                .unwrap()
        );
        assert!(
            !engine
                .received(&offer.offer_id, &files[0].file_id, true)
                .unwrap()
        );
        assert!(
            engine
                .received(&offer.offer_id, &files[1].file_id, true)
                .unwrap()
        );
        assert!(
            !engine
                .received(&offer.offer_id, &files[1].file_id, true)
                .unwrap()
        );
        let offer = batch(&mut engine);
        let files = offer.content.entries();
        engine.batches.get_mut(&offer.offer_id).unwrap().offer.cut = true;
        assert!(
            !engine
                .received(&offer.offer_id, &files[0].file_id, false)
                .unwrap()
        );
        assert!(engine.received(&offer.offer_id, "unknown", false).is_err());
        assert!(
            engine
                .received(&offer.offer_id, &files[1].file_id, false)
                .unwrap()
        );
    }
    fn batch(engine: &mut Engine) -> Offer {
        let files: Vec<_> = (0..2)
            .map(|_| FileEntry {
                directory: false,
                modified_ms: None,
                executable: None,
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
    fn text_mode_keeps_only_the_plain_text_representation() {
        let mut a = Engine::new(new_id());
        let mut b = Engine::new(new_id());
        b.text_only = true;
        let offer = a
            .local(
                Content::Text {
                    text: "hello".into(),
                    html: Some("<b>hello</b>".into()),
                    rtf: None,
                },
                HashMap::new(),
            )
            .unwrap();
        b.accept(offer.clone(), &a.device_id).unwrap();
        assert_eq!(b.current.unwrap().content, Content::text("hello"));
        a.text_only = true;
        assert_eq!(
            a.local(offer.content, HashMap::new()).unwrap().content,
            Content::text("hello")
        );
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
    fn pairing_clock_survives_restart_without_replacing_clipboard_or_watermark() {
        let path = std::env::temp_dir().join(new_id());
        let id = new_id();
        let mut fresh = Engine::new(id.clone());
        fresh.persist_clock(path.clone()).unwrap();
        let mut established = Engine::new(new_id());
        established.observe_clock(156).unwrap();
        let previous = established
            .local(
                Content::Text {
                    text: "previous".into(),
                    html: None,
                    rtf: None,
                },
                HashMap::new(),
            )
            .unwrap();
        assert!(
            fresh
                .accept(previous.clone(), &established.device_id)
                .unwrap()
        );
        established.observe_clock(200).unwrap();
        let watermark = fresh.watermark.clone();
        fresh.observe_clock(established.clock()).unwrap();
        assert_eq!(fresh.current.as_ref(), Some(&previous));
        assert_eq!(fresh.watermark, watermark);
        assert!(fresh.observe_clock(u64::MAX - 1).is_err());
        drop(fresh);
        let mut fresh = Engine::new(id);
        fresh.persist_clock(path.clone()).unwrap();
        fresh.observe_clock(3).unwrap();
        assert_eq!(fresh.clock(), 200);
        assert!(!fresh.accept(previous, &established.device_id).unwrap());
        established.clear_clipboard();
        let offer = fresh
            .local(
                Content::Text {
                    text: "first copy after pairing".into(),
                    html: None,
                    rtf: None,
                },
                HashMap::new(),
            )
            .unwrap();
        assert_eq!(offer.event_sequence, 201);
        assert!(established.accept(offer, &fresh.device_id).unwrap());
        std::fs::remove_file(path).unwrap();
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
                    html: None,
                    rtf: None,
                },
                HashMap::new(),
            )
            .unwrap();
        let mut remote = Engine::new(new_id());
        let mut incoming = remote
            .local(
                Content::Text {
                    text: "remote".into(),
                    html: None,
                    rtf: None,
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
                    html: None,
                    rtf: None,
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
            .local(
                Content::Text {
                    text: "a".into(),
                    html: None,
                    rtf: None,
                },
                HashMap::new(),
            )
            .unwrap();
        let ob = b
            .local(
                Content::Text {
                    text: "b".into(),
                    html: None,
                    rtf: None,
                },
                HashMap::new(),
            )
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
