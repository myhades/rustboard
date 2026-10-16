use crate::*;
use anyhow::{Result, ensure};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
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

pub struct Engine {
    pub device_id: String,
    pub paused: bool,
    pub text_only: bool,
    clock: u64,
    clock_path: Option<PathBuf>,
    watermark: Option<(u64, String, String)>,
    pub current: Option<Offer>,
    sources: HashMap<String, Arc<dyn ReadAt>>,
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
            sources: HashMap::new(),
            accepted: 0,
        }
    }
    pub fn persist_clock(&mut self, path: PathBuf) -> Result<()> {
        if path.exists() {
            let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
            if let Some(clock) = value.as_u64() {
                self.clock = clock;
            } else {
                self.clock = serde_json::from_value(value["clock"].clone())?;
                self.watermark = serde_json::from_value(value["watermark"].clone())?;
            }
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
        self.sources.clear();
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
        self.watermark = Some((
            offer.event_sequence,
            offer.source_device.clone(),
            offer.offer_id.clone(),
        ));
        self.save_clock()?;
        self.current = Some(offer.clone());
        self.sources = sources;
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
        self.watermark = Some((
            offer.event_sequence,
            offer.source_device.clone(),
            offer.offer_id.clone(),
        ));
        self.save_clock()?;
        self.sources.clear();
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
            self.current = None;
            self.sources.clear();
        }
    }
    pub fn source(
        &mut self,
        device: &str,
        offer: &str,
        file: &str,
        version: &str,
    ) -> Result<Arc<dyn ReadAt>> {
        self.expire();
        ensure!(!self.paused, "paused");
        let current = self
            .current
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("expired"))?;
        ensure!(
            current.source_device == self.device_id
                && device == self.device_id
                && current.offer_id == offer,
            "unknown offer"
        );
        let Content::Files { files } = &current.content else {
            anyhow::bail!("not file offer");
        };
        ensure!(
            files
                .iter()
                .any(|f| f.file_id == file && f.version == version),
            "version or file mismatch"
        );
        Ok(self
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
