use super::Captured;
use anyhow::{Context, Result, ensure};
use dbus::{
    arg::{OwnedFd, PropMap, Variant},
    blocking::Connection,
    message::MatchRule,
};
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

const BUS: &str = "org.gnome.Mutter.RemoteDesktop";
const IFACE: &str = "org.gnome.Mutter.RemoteDesktop.Session";
const TIMEOUT: Duration = Duration::from_secs(5);
const MIMES: [&str; 7] = [
    "text/uri-list",
    "x-special/gnome-copied-files",
    "image/png",
    "image/jpeg",
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "text/plain",
];

struct Receiving {
    file: File,
    mime: &'static str,
    bytes: Vec<u8>,
    touched: Instant,
}
struct Sending {
    file: File,
    bytes: Arc<Vec<u8>>,
    offset: usize,
    serial: u32,
    touched: Instant,
}

pub struct Clipboard {
    conn: Connection,
    path: dbus::Path<'static>,
    owner: String,
    formats: HashMap<String, Arc<Vec<u8>>>,
    receiving: Option<Receiving>,
    sending: Vec<Sending>,
}
impl Clipboard {
    pub fn new() -> Result<Self> {
        let conn = Connection::new_session()?;
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
        Ok(Self {
            conn,
            path,
            owner,
            formats: HashMap::new(),
            receiving: None,
            sending: Vec::new(),
        })
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
                            .and_then(|(fd,)| nonblocking(fd))
                        {
                            Ok(file) => self.sending.push(Sending {
                                file,
                                bytes,
                                offset: 0,
                                serial,
                                touched: Instant::now(),
                            }),
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
            self.receiving = None;
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
                if let Some(mime) = MIMES.into_iter().find(|m| types.contains(m)) {
                    let (fd,): (OwnedFd,) = self.call("SelectionRead", (mime,))?;
                    self.receiving = Some(Receiving {
                        file: nonblocking(fd)?,
                        mime,
                        bytes: Vec::new(),
                        touched: Instant::now(),
                    });
                } else {
                    captured = Some(Captured::Unsupported);
                }
            }
        }
        if let Some(mut read) = self.receiving.take() {
            ensure!(read.touched.elapsed() < TIMEOUT, "clipboard read timed out");
            let mut buffer = [0u8; 65536];
            let mut complete = false;
            for _ in 0..16 {
                match read.file.read(&mut buffer) {
                    Ok(0) => {
                        complete = true;
                        break;
                    }
                    Ok(n) => {
                        ensure!(
                            read.bytes.len() + n <= crate::app::image_file::MAX_IMAGE_BYTES,
                            "clipboard data too large"
                        );
                        read.bytes.extend_from_slice(&buffer[..n]);
                        read.touched = Instant::now();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e.into()),
                }
            }
            if complete {
                captured = Some(super::capture(read.mime, read.bytes)?);
            } else {
                self.receiving = Some(read);
            }
        }
        let mut index = 0;
        while index < self.sending.len() {
            let send = &mut self.sending[index];
            let mut failed = send.touched.elapsed() >= TIMEOUT;
            if !failed {
                match send.file.write(&send.bytes[send.offset..]) {
                    Ok(n) => {
                        send.offset += n;
                        send.touched = Instant::now();
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => failed = true,
                }
            }
            if failed || send.offset == send.bytes.len() {
                let serial = send.serial;
                self.sending.swap_remove(index); // Close the pipe before acknowledging completion.
                self.done(serial, !failed);
            } else {
                index += 1;
            }
        }
        Ok(captured)
    }
    fn publish(&mut self, formats: HashMap<String, Arc<Vec<u8>>>) -> Result<()> {
        self.receiving = None;
        let mut options = PropMap::new();
        options.insert(
            "mime-types".into(),
            Variant(Box::new(formats.keys().cloned().collect::<Vec<_>>())),
        );
        self.call::<_, ()>("SetSelection", (options,))?;
        self.formats = formats;
        Ok(())
    }
    pub fn publish_text(&mut self, text: &str) -> Result<()> {
        let bytes = Arc::new(text.as_bytes().to_vec());
        self.publish(
            ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"]
                .into_iter()
                .map(|m| (m.into(), bytes.clone()))
                .collect(),
        )
    }
    pub fn publish_files(&mut self, paths: &[PathBuf]) -> Result<()> {
        let uris: Vec<_> = paths.iter().map(|p| super::uri(p)).collect();
        self.publish(HashMap::from([
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
fn nonblocking(fd: OwnedFd) -> Result<File> {
    let file = unsafe { File::from_raw_fd(fd.into_fd()) };
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    ensure!(
        flags >= 0
            && unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                == 0,
        "cannot configure clipboard pipe"
    );
    Ok(file)
}
