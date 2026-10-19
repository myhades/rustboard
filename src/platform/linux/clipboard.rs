use anyhow::{Result, ensure};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use x11rb::{
    CURRENT_TIME, NONE,
    connection::Connection,
    protocol::{
        Event,
        xfixes::{ConnectionExt as _, SelectionEventMask},
        xproto::*,
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        CLIPBOARD, TARGETS, UTF8_STRING, INCR, RUSTBOARD_DATA,
        URI: b"text/uri-list", COPIED: b"x-special/gnome-copied-files",
        TEXT: b"text/plain;charset=utf-8", PLAIN: b"text/plain",
        PNG: b"image/png", JPEG: b"image/jpeg",
    }
}
pub enum Captured {
    Text(String),
    Files(Vec<PathBuf>),
    Image(Vec<u8>, &'static str),
    Unsupported,
}
struct Pending {
    owner: Window,
    time: Timestamp,
    target: Atom,
    incremental: bool,
    bytes: Vec<u8>,
    touched: Instant,
}
struct Sending {
    target: Atom,
    bytes: Arc<Vec<u8>>,
    offset: usize,
    touched: Instant,
}
pub struct Clipboard {
    conn: RustConnection,
    window: Window,
    atoms: Atoms,
    pending: Option<Pending>,
    formats: Vec<(Atom, Arc<Vec<u8>>)>,
    sends: HashMap<(Window, Atom), Sending>,
}
impl Clipboard {
    pub fn new() -> Result<Self> {
        ensure!(
            std::env::var("XDG_SESSION_TYPE").as_deref() != Ok("wayland"),
            "Wayland is not supported"
        );
        let (conn, screen) = x11rb::connect(None)?;
        conn.xfixes_query_version(5, 0)?.reply()?;
        let window = conn.generate_id()?;
        conn.create_window(
            0,
            window,
            conn.setup().roots[screen].root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?
        .check()?;
        let atoms = Atoms::new(&conn)?.reply()?;
        conn.xfixes_select_selection_input(
            window,
            atoms.CLIPBOARD,
            SelectionEventMask::SET_SELECTION_OWNER
                | SelectionEventMask::SELECTION_WINDOW_DESTROY
                | SelectionEventMask::SELECTION_CLIENT_CLOSE,
        )?
        .check()?;
        conn.flush()?;
        // Capture future changes only; restarting an Agent must not publish stale clipboard state.
        Ok(Self {
            conn,
            window,
            atoms,
            pending: None,
            formats: vec![],
            sends: HashMap::new(),
        })
    }
    fn convert(&mut self, owner: Window, time: Timestamp, target: Atom) -> Result<()> {
        self.pending = Some(Pending {
            owner,
            time,
            target,
            incremental: false,
            bytes: Vec::new(),
            touched: Instant::now(),
        });
        self.conn.convert_selection(
            self.window,
            self.atoms.CLIPBOARD,
            target,
            self.atoms.RUSTBOARD_DATA,
            time,
        )?;
        self.conn.flush()?;
        Ok(())
    }
    fn capture(&self, target: Atom, bytes: Vec<u8>) -> Result<Captured> {
        if target == self.atoms.PNG {
            return Ok(Captured::Image(bytes, "png"));
        }
        if target == self.atoms.JPEG {
            return Ok(Captured::Image(bytes, "jpg"));
        }
        let text = String::from_utf8(bytes)?;
        if target == self.atoms.URI || target == self.atoms.COPIED {
            let mut paths = Vec::new();
            for line in text
                .lines()
                .filter(|s| !s.is_empty() && !s.starts_with('#') && *s != "copy" && *s != "cut")
            {
                paths.push(parse_uri(line)?);
            }
            ensure!(paths.len() <= 256, "too many files");
            return Ok(Captured::Files(paths));
        }
        ensure!(
            text.len() <= 262144 && !text.contains('\0'),
            "invalid clipboard text"
        );
        Ok(Captured::Text(text))
    }
    fn received(&mut self, event: SelectionNotifyEvent) -> Result<Option<Captured>> {
        let Some(p) = &self.pending else {
            return Ok(None);
        };
        if event.target != p.target || event.time != p.time {
            return Ok(None);
        }
        if event.property == NONE {
            self.pending = None;
            return Ok(Some(Captured::Unsupported));
        }
        if self
            .conn
            .get_selection_owner(self.atoms.CLIPBOARD)?
            .reply()?
            .owner
            != p.owner
        {
            self.pending = None;
            return Ok(None);
        }
        let value = self
            .conn
            .get_property(
                true,
                self.window,
                self.atoms.RUSTBOARD_DATA,
                AtomEnum::ANY,
                0,
                (crate::app::image_file::MAX_IMAGE_BYTES / 4 + 1) as u32,
            )?
            .reply()?;
        ensure!(value.bytes_after == 0, "clipboard data too large");
        if p.target == self.atoms.TARGETS {
            let targets: Vec<_> = value.value32().into_iter().flatten().collect();
            let preferred = [
                self.atoms.URI,
                self.atoms.COPIED,
                self.atoms.PNG,
                self.atoms.JPEG,
                self.atoms.UTF8_STRING,
                self.atoms.TEXT,
                self.atoms.PLAIN,
            ];
            if let Some(target) = preferred.into_iter().find(|a| targets.contains(a)) {
                self.convert(p.owner, p.time, target)?;
            } else {
                self.pending = None;
                return Ok(Some(Captured::Unsupported));
            }
        } else if value.type_ == self.atoms.INCR {
            self.pending.as_mut().unwrap().incremental = true;
        } else {
            let target = p.target;
            self.pending = None;
            return Ok(Some(self.capture(target, value.value)?));
        }
        Ok(None)
    }
    fn request(&mut self, e: SelectionRequestEvent) -> Result<()> {
        let property = if e.property == NONE {
            e.target
        } else {
            e.property
        };
        let mut accepted = false;
        if e.selection == self.atoms.CLIPBOARD && !self.formats.is_empty() {
            if e.target == self.atoms.TARGETS {
                let mut targets = vec![self.atoms.TARGETS];
                targets.extend(self.formats.iter().map(|(a, _)| *a));
                self.conn.change_property32(
                    PropMode::REPLACE,
                    e.requestor,
                    property,
                    AtomEnum::ATOM,
                    &targets,
                )?;
                accepted = true;
            } else if let Some((_, bytes)) = self.formats.iter().find(|(a, _)| *a == e.target) {
                if bytes.len() <= 60000 {
                    self.conn.change_property8(
                        PropMode::REPLACE,
                        e.requestor,
                        property,
                        e.target,
                        bytes,
                    )?;
                    accepted = true;
                } else if self.sends.len() < 8 {
                    self.conn.change_window_attributes(
                        e.requestor,
                        &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
                    )?;
                    self.conn.change_property32(
                        PropMode::REPLACE,
                        e.requestor,
                        property,
                        self.atoms.INCR,
                        &[bytes.len() as u32],
                    )?;
                    self.sends.insert(
                        (e.requestor, property),
                        Sending {
                            target: e.target,
                            bytes: bytes.clone(),
                            offset: 0,
                            touched: Instant::now(),
                        },
                    );
                    accepted = true;
                }
            }
        }
        self.conn.send_event(
            false,
            e.requestor,
            EventMask::NO_EVENT,
            SelectionNotifyEvent {
                response_type: SELECTION_NOTIFY_EVENT,
                sequence: 0,
                time: e.time,
                requestor: e.requestor,
                selection: e.selection,
                target: e.target,
                property: if accepted { property } else { NONE },
            },
        )?;
        Ok(())
    }
    pub fn poll(&mut self) -> Result<Option<Captured>> {
        self.sends
            .retain(|_, s| s.touched.elapsed() < Duration::from_secs(5));
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.touched.elapsed() > Duration::from_secs(5))
        {
            self.pending = None;
        }
        let mut captured = None;
        while let Some(event) = self.conn.poll_for_event()? {
            match event {
                Event::XfixesSelectionNotify(e) if e.selection == self.atoms.CLIPBOARD => {
                    if e.owner != self.window
                        && self
                            .conn
                            .get_selection_owner(self.atoms.CLIPBOARD)?
                            .reply()?
                            .owner
                            == e.owner
                    {
                        self.formats.clear();
                        self.pending = None;
                        captured = None;
                        if e.owner != NONE {
                            self.convert(e.owner, e.timestamp, self.atoms.TARGETS)?;
                        } else {
                            captured = Some(Captured::Unsupported);
                        }
                    }
                }
                Event::SelectionNotify(e) => {
                    if let Some(value) = self.received(e)? {
                        captured = Some(value);
                    }
                }
                Event::SelectionRequest(e) => self.request(e)?,
                Event::PropertyNotify(e)
                    if e.window == self.window
                        && e.atom == self.atoms.RUSTBOARD_DATA
                        && e.state == Property::NEW_VALUE
                        && self.pending.as_ref().is_some_and(|p| p.incremental) =>
                {
                    let value = self
                        .conn
                        .get_property(
                            true,
                            self.window,
                            e.atom,
                            AtomEnum::ANY,
                            0,
                            (crate::app::image_file::MAX_IMAGE_BYTES / 4 + 1) as u32,
                        )?
                        .reply()?;
                    let p = self.pending.as_mut().unwrap();
                    ensure!(
                        value.bytes_after == 0
                            && p.bytes.len() + value.value.len()
                                <= crate::app::image_file::MAX_IMAGE_BYTES,
                        "clipboard data too large"
                    );
                    p.touched = Instant::now();
                    if value.value.is_empty() {
                        let p = self.pending.take().unwrap();
                        if self
                            .conn
                            .get_selection_owner(self.atoms.CLIPBOARD)?
                            .reply()?
                            .owner
                            == p.owner
                        {
                            captured = Some(self.capture(p.target, p.bytes)?);
                        }
                    } else {
                        p.bytes.extend(value.value);
                    }
                }
                Event::PropertyNotify(e) if e.state == Property::DELETE => {
                    let key = (e.window, e.atom);
                    if let Some(s) = self.sends.get_mut(&key) {
                        let end = (s.offset + 60000).min(s.bytes.len());
                        self.conn.change_property8(
                            PropMode::REPLACE,
                            e.window,
                            e.atom,
                            s.target,
                            &s.bytes[s.offset..end],
                        )?;
                        if s.offset == end {
                            self.sends.remove(&key);
                        } else {
                            s.offset = end;
                            s.touched = Instant::now();
                        }
                    }
                }
                _ => {}
            }
        }
        self.conn.flush()?;
        Ok(captured)
    }
    pub fn publish_text(&mut self, text: &str) -> Result<()> {
        let bytes = Arc::new(text.as_bytes().to_vec());
        self.formats = vec![
            (self.atoms.UTF8_STRING, bytes.clone()),
            (self.atoms.TEXT, bytes.clone()),
            (self.atoms.PLAIN, bytes),
        ];
        self.claim()
    }
    pub fn publish_files(&mut self, paths: &[PathBuf]) -> Result<()> {
        let uris: Vec<_> = paths.iter().map(|p| uri(p)).collect();
        self.formats = vec![
            (
                self.atoms.URI,
                Arc::new(format!("{}\r\n", uris.join("\r\n")).into_bytes()),
            ),
            (
                self.atoms.COPIED,
                Arc::new(format!("copy\n{}", uris.join("\n")).into_bytes()),
            ),
        ];
        self.claim()
    }
    fn claim(&mut self) -> Result<()> {
        self.pending = None;
        self.conn
            .set_selection_owner(self.window, self.atoms.CLIPBOARD, CURRENT_TIME)?
            .check()?;
        self.conn.flush()?;
        Ok(())
    }
    pub fn clear_owned(&mut self) -> Result<()> {
        if self
            .conn
            .get_selection_owner(self.atoms.CLIPBOARD)?
            .reply()?
            .owner
            == self.window
        {
            self.conn
                .set_selection_owner(NONE, self.atoms.CLIPBOARD, CURRENT_TIME)?;
            self.conn.flush()?;
        }
        self.formats.clear();
        Ok(())
    }
}
pub fn uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            out.push(b as char);
        } else {
            use std::fmt::Write;
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}
fn parse_uri(value: &str) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let value = value
        .strip_prefix("file://")
        .ok_or_else(|| anyhow::anyhow!("local file URI required"))?;
    let value = value
        .strip_prefix("localhost/")
        .map(|v| format!("/{v}"))
        .unwrap_or_else(|| value.into());
    ensure!(value.starts_with('/'), "remote URI unsupported");
    let mut bytes = Vec::new();
    let mut i = 0;
    while i < value.len() {
        if value.as_bytes()[i] == b'%' {
            let hex = value
                .get(i + 1..i + 3)
                .ok_or_else(|| anyhow::anyhow!("invalid URI escape"))?;
            bytes.push(u8::from_str_radix(hex, 16)?);
            i += 3;
        } else {
            bytes.push(value.as_bytes()[i]);
            i += 1;
        }
    }
    ensure!(!bytes.contains(&0), "invalid URI");
    Ok(std::ffi::OsString::from_vec(bytes).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_uri_roundtrip_and_rejection() {
        let path = Path::new("/tmp/中文 # % image.png");
        assert_eq!(parse_uri(&uri(path)).unwrap(), path);
        for bad in [
            "https://host/a",
            "file://host/a",
            "file:///a%",
            "file:///a%00",
        ] {
            assert!(parse_uri(bad).is_err());
        }
    }
}
