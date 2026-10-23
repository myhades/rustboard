use super::Captured;
use anyhow::{Result, ensure};
use std::{
    collections::HashMap,
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
        CUT: b"application/x-kde-cutselection", URI: b"text/uri-list", COPIED: b"x-special/gnome-copied-files",
        TEXT: b"text/plain;charset=utf-8", PLAIN: b"text/plain",
        PNG: b"image/png", JPEG: b"image/jpeg",
        HTML: b"text/html", RTF: b"text/rtf", APP_RTF: b"application/rtf",
    }
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
    remaining: Vec<Atom>,
    captured: Option<Captured>,
    formats: Vec<(Atom, Arc<Vec<u8>>)>,
    sends: HashMap<(Window, Atom), Sending>,
}
impl Clipboard {
    pub fn new() -> Result<Self> {
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
            remaining: Vec::new(),
            captured: None,
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
        let mime = if target == self.atoms.CUT {
            "application/x-kde-cutselection"
        } else if target == self.atoms.PNG {
            "image/png"
        } else if target == self.atoms.JPEG {
            "image/jpeg"
        } else if target == self.atoms.HTML {
            "text/html"
        } else if target == self.atoms.RTF || target == self.atoms.APP_RTF {
            "text/rtf"
        } else if target == self.atoms.URI {
            "text/uri-list"
        } else if target == self.atoms.COPIED {
            "x-special/gnome-copied-files"
        } else {
            "text/plain"
        };
        super::capture(mime, bytes)
    }
    fn finish(
        &mut self,
        owner: Window,
        time: Timestamp,
        target: Atom,
        bytes: Option<Vec<u8>>,
    ) -> Result<Option<Captured>> {
        if let Some(bytes) = bytes
            && let Ok(value) = self.capture(target, bytes)
        {
            super::merge(&mut self.captured, value);
        }
        if !self.remaining.is_empty() {
            let next = self.remaining.remove(0);
            self.convert(owner, time, next)?;
            Ok(None)
        } else {
            self.pending = None;
            Ok(self.captured.take().or(Some(Captured::Unsupported)))
        }
    }
    fn received(&mut self, event: SelectionNotifyEvent) -> Result<Option<Captured>> {
        let Some(p) = &self.pending else {
            return Ok(None);
        };
        if event.target != p.target || event.time != p.time {
            return Ok(None);
        }
        if event.property == NONE {
            return self.finish(p.owner, p.time, p.target, None);
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
            let mapping = [
                (self.atoms.CUT, "application/x-kde-cutselection"),
                (self.atoms.URI, "text/uri-list"),
                (self.atoms.COPIED, "x-special/gnome-copied-files"),
                (self.atoms.PNG, "image/png"),
                (self.atoms.JPEG, "image/jpeg"),
                (self.atoms.HTML, "text/html"),
                (self.atoms.RTF, "text/rtf"),
                (self.atoms.APP_RTF, "application/rtf"),
                (self.atoms.UTF8_STRING, "UTF8_STRING"),
                (self.atoms.TEXT, "text/plain;charset=utf-8"),
                (self.atoms.PLAIN, "text/plain"),
            ];
            let types: Vec<_> = mapping
                .iter()
                .filter(|(a, _)| targets.contains(a))
                .map(|(_, m)| *m)
                .collect();
            self.remaining = super::capture_types(&types)
                .into_iter()
                .map(|m| mapping.iter().find(|(_, mime)| *mime == m).unwrap().0)
                .collect();
            self.captured = None;
            if !self.remaining.is_empty() {
                let target = self.remaining.remove(0);
                self.convert(p.owner, p.time, target)?;
            } else {
                self.pending = None;
                return Ok(Some(Captured::Unsupported));
            }
        } else if value.type_ == self.atoms.INCR {
            self.pending.as_mut().unwrap().incremental = true;
        } else {
            return self.finish(p.owner, p.time, p.target, Some(value.value));
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
                        self.remaining.clear();
                        self.captured = None;
                        captured = None;
                        if e.owner != NONE {
                            self.convert(e.owner, e.timestamp, self.atoms.TARGETS)?;
                        } else {
                            captured = Some(Captured::Cleared);
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
                            captured = self.finish(p.owner, p.time, p.target, Some(p.bytes))?;
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
    pub fn publish_formats(&mut self, formats: HashMap<String, Arc<Vec<u8>>>) -> Result<()> {
        self.formats = formats
            .into_iter()
            .map(|(mime, bytes)| {
                Ok((
                    self.conn.intern_atom(false, mime.as_bytes())?.reply()?.atom,
                    bytes,
                ))
            })
            .collect::<Result<_>>()?;
        self.claim()
    }
    fn claim(&mut self) -> Result<()> {
        self.pending = None;
        self.remaining.clear();
        self.captured = None;
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
