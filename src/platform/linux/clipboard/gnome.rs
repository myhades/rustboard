use super::{
    Captured,
    pipe::{Receiving, Sending},
};
use anyhow::{Context, Result};
use dbus::{
    arg::{OwnedFd, PropMap, Variant},
    blocking::Connection,
    message::MatchRule,
};
use std::{
    collections::HashMap,
    fs::File,
    os::fd::FromRawFd,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

const BUS: &str = "org.gnome.Mutter.RemoteDesktop";
const IFACE: &str = "org.gnome.Mutter.RemoteDesktop.Session";
const TIMEOUT: Duration = Duration::from_secs(5);
pub struct Clipboard {
    conn: Connection,
    path: dbus::Path<'static>,
    owner: String,
    formats: HashMap<String, Arc<Vec<u8>>>,
    receiving: Vec<Receiving>,
    captured: Option<Captured>,
    sending: Vec<(u32, Sending)>,
}
impl Clipboard {
    pub fn connect() -> Result<Option<Self>> {
        let conn = Connection::new_session()?;
        let (available,): (bool,) = conn
            .with_proxy("org.freedesktop.DBus", "/org/freedesktop/DBus", TIMEOUT)
            .method_call("org.freedesktop.DBus", "NameHasOwner", (BUS,))?;
        if !available {
            return Ok(None);
        }
        let (owner,): (String,) = conn
            .with_proxy("org.freedesktop.DBus", "/org/freedesktop/DBus", TIMEOUT)
            .method_call("org.freedesktop.DBus", "GetNameOwner", (BUS,))
            .context("GNOME Wayland clipboard unavailable: Mutter RemoteDesktop is required")?;
        let (path,): (dbus::Path<'static>,) = conn
            .with_proxy(BUS, "/org/gnome/Mutter/RemoteDesktop", TIMEOUT)
            .method_call(BUS, "CreateSession", ())?;
        let rule = MatchRule::new_signal(IFACE, "SelectionOwnerChanged")
            .with_path(path.clone())
            .with_sender(owner.clone());
        conn.add_match_no_cb(&rule.match_str())?;
        for member in ["SelectionTransfer", "Closed"] {
            let rule = MatchRule::new_signal(IFACE, member)
                .with_path(path.clone())
                .with_sender(owner.clone());
            conn.add_match_no_cb(&rule.match_str())?;
        }
        let rule = MatchRule::new_signal("org.freedesktop.DBus", "NameOwnerChanged")
            .with_sender("org.freedesktop.DBus")
            .with_path("/org/freedesktop/DBus");
        conn.add_match_no_cb(&rule.match_str())?;
        // Clipboard access is independent of Start: no screen or input devices are requested.
        let _: () = conn.with_proxy(BUS, path.clone(), TIMEOUT).method_call(
            IFACE,
            "EnableClipboard",
            (PropMap::new(),),
        )?;
        // EnableClipboard reports the existing selection. As on X11, capture only subsequent changes.
        while conn.channel().pop_message().is_some() {}
        Ok(Some(Self {
            conn,
            path,
            owner,
            formats: HashMap::new(),
            receiving: Vec::new(),
            captured: None,
            sending: Vec::new(),
        }))
    }
    fn call<A: dbus::arg::AppendAll, R: dbus::arg::ReadAll>(
        &self,
        method: &str,
        args: A,
    ) -> Result<R> {
        Ok(self
            .conn
            .with_proxy(BUS, self.path.clone(), TIMEOUT)
            .method_call(IFACE, method, args)?)
    }
    fn done(&self, serial: u32, success: bool) {
        if let Err(error) = self.call::<_, ()>("SelectionWriteDone", (serial, success)) {
            log::debug!(target: "clipboard", "selection completion failed: {error}");
        }
    }
    pub fn poll(&mut self) -> Result<Option<Captured>> {
        self.conn
            .channel()
            .read_write(Some(Duration::ZERO))
            .map_err(|_| {
                super::super::STOP.store(true, Ordering::Relaxed);
                anyhow::anyhow!("desktop session disconnected")
            })?;
        let mut captured = None;
        let mut owner_change = None;
        for _ in 0..64 {
            let Some(message) = self.conn.channel().pop_message() else {
                break;
            };
            if message.sender().as_deref() == Some("org.freedesktop.DBus")
                && message.interface().as_deref() == Some("org.freedesktop.DBus")
                && message.member().as_deref() == Some("NameOwnerChanged")
            {
                let (name, old, new): (String, String, String) = message.read3()?;
                if name == BUS && old == self.owner && new != self.owner {
                    super::super::STOP.store(true, Ordering::Relaxed);
                    anyhow::bail!("GNOME desktop session ended");
                }
                continue;
            }
            if message.sender().as_deref() != Some(self.owner.as_str())
                || message.path().as_ref() != Some(&self.path)
                || message.interface().as_deref() != Some(IFACE)
            {
                continue;
            }
            match message.member().as_deref() {
                Some("Closed") => {
                    super::super::STOP.store(true, Ordering::Relaxed);
                    anyhow::bail!("GNOME clipboard session closed");
                }
                Some("SelectionOwnerChanged") => {
                    owner_change = Some(message.read1::<PropMap>()?);
                }
                Some("SelectionTransfer") => {
                    let (mime, serial): (String, u32) = message.read2()?;
                    if let Some(bytes) = self
                        .formats
                        .get(&mime)
                        .cloned()
                        .filter(|_| self.sending.len() < 8)
                    {
                        match self
                            .call::<_, (OwnedFd,)>("SelectionWrite", (serial,))
                            .and_then(|(fd,)| {
                                Sending::new(unsafe { File::from_raw_fd(fd.into_fd()) }, bytes)
                            }) {
                            Ok(send) => self.sending.push((serial, send)),
                            Err(_) => self.done(serial, false),
                        }
                    } else {
                        self.done(serial, false);
                    }
                }
                _ => {}
            }
        }
        // SetSelection can queue an unset followed by a new owner in the same round trip.
        if let Some(options) = owner_change {
            self.receiving.clear();
            self.captured = None;
            captured = None;
            if options.get("session-is-owner").and_then(|v| v.0.as_u64()) != Some(1) {
                self.formats.clear();
                // Mutter RemoteDesktop wraps its MIME array in a one-field D-Bus struct.
                let types: Vec<_> = options
                    .get("mime-types")
                    .and_then(|v| v.0.as_iter()?.next()?.as_iter())
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str())
                    .collect();
                for mime in super::capture_types(&types) {
                    if let Ok((fd,)) = self.call::<_, (OwnedFd,)>("SelectionRead", (mime,)) {
                        self.receiving.push(Receiving::new(
                            unsafe { File::from_raw_fd(fd.into_fd()) },
                            mime,
                        )?);
                    }
                }
                if self.receiving.is_empty() {
                    captured = Some(Captured::Unsupported);
                }
            }
        }
        if !self.receiving.is_empty() {
            let mut index = 0;
            while index < self.receiving.len() {
                match self.receiving[index].poll() {
                    Ok(Some(value)) => {
                        super::merge(&mut self.captured, value);
                        self.receiving.swap_remove(index);
                    }
                    Ok(None) => index += 1,
                    Err(_) => {
                        self.receiving.swap_remove(index);
                    }
                }
            }
            if self.receiving.is_empty() {
                captured = self.captured.take().or(Some(Captured::Unsupported));
            }
        }
        let mut index = 0;
        while index < self.sending.len() {
            match self.sending[index].1.poll() {
                Ok(false) => index += 1,
                result => {
                    let (serial, send) = self.sending.swap_remove(index);
                    drop(send);
                    self.done(serial, result.is_ok());
                }
            }
        }
        Ok(captured)
    }
    pub fn publish_formats(&mut self, formats: HashMap<String, Arc<Vec<u8>>>) -> Result<()> {
        self.receiving.clear();
        self.captured = None;
        let mut options = PropMap::new();
        options.insert(
            "mime-types".into(),
            Variant(Box::new(formats.keys().cloned().collect::<Vec<_>>())),
        );
        self.call::<_, ()>("SetSelection", (options,))?;
        self.formats = formats;
        Ok(())
    }
    pub fn publish_files(&mut self, paths: &[PathBuf]) -> Result<()> {
        let uris: Vec<_> = paths.iter().map(|p| super::uri(p)).collect();
        self.publish_formats(HashMap::from([
            (
                "text/uri-list".into(),
                Arc::new(format!("{}\r\n", uris.join("\r\n")).into_bytes()),
            ),
            (
                "x-special/gnome-copied-files".into(),
                Arc::new(format!("copy\n{}", uris.join("\n")).into_bytes()),
            ),
        ]))
    }
    pub fn clear_owned(&mut self) -> Result<()> {
        // An empty options dictionary unsets only this session's source, never another application's.
        self.call::<_, ()>("SetSelection", (PropMap::new(),))?;
        self.formats.clear();
        Ok(())
    }
}
impl Drop for Clipboard {
    fn drop(&mut self) {
        let _ = self.call::<_, ()>("DisableClipboard", ());
        // Closing the D-Bus connection destroys its unstarted session.
    }
}
