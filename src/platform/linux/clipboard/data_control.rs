use super::{
    Captured, MIMES,
    pipe::{Receiving, Sending},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::HashMap,
    fs::File,
    os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    protocol::{wl_registry, wl_seat},
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1 as device, ext_data_control_manager_v1 as manager,
    ext_data_control_offer_v1 as offer, ext_data_control_source_v1 as source,
};

type Formats = HashMap<String, Arc<Vec<u8>>>;

#[derive(Default)]
struct State {
    manager: Option<manager::ExtDataControlManagerV1>,
    seat: Option<wl_seat::WlSeat>,
    offers: HashMap<offer::ExtDataControlOfferV1, Vec<String>>,
    selection: Option<offer::ExtDataControlOfferV1>,
    source: Option<source::ExtDataControlSourceV1>,
    marker: String,
    changed: bool,
    finished: bool,
    sending: Vec<Sending>,
}
pub struct Clipboard {
    queue: EventQueue<State>,
    state: State,
    device: device::ExtDataControlDeviceV1,
    receiving: Option<Receiving>,
}
impl Clipboard {
    /// None means the compositor does not advertise this protocol, not that initialization failed.
    pub fn connect() -> Result<Option<Self>> {
        let connection =
            Connection::connect_to_env().context("cannot connect to Wayland desktop")?;
        let mut queue = connection.new_event_queue();
        let mut state = State {
            marker: format!("application/x-rustboard-{}", crate::core::engine::new_id()),
            ..State::default()
        };
        connection.display().get_registry(&queue.handle(), ());
        queue.roundtrip(&mut state)?;
        let Some(manager) = &state.manager else {
            return Ok(None);
        };
        let seat = state.seat.as_ref().context("Wayland desktop has no seat")?;
        let device = manager.get_data_device(seat, &queue.handle(), ());
        queue.roundtrip(&mut state)?;
        ensure!(!state.finished, "Wayland clipboard access denied");
        // Binding reports the existing selection. Capture future changes only.
        state.changed = false;
        Ok(Some(Self {
            queue,
            state,
            device,
            receiving: None,
        }))
    }
    fn dispatch(&mut self) -> Result<()> {
        self.queue.dispatch_pending(&mut self.state)?;
        if let Some(guard) = self.queue.prepare_read() {
            let mut fd = libc::pollfd {
                fd: self.queue.as_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut fd, 1, 0) };
            if ready > 0 {
                guard.read()?;
            } else {
                drop(guard);
                if ready < 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
            }
        }
        self.queue.dispatch_pending(&mut self.state)?;
        ensure!(!self.state.finished, "Wayland clipboard session ended");
        self.flush()
    }
    fn flush(&self) -> Result<()> {
        match self.queue.flush() {
            Ok(()) => Ok(()),
            Err(wayland_client::backend::WaylandError::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }
    pub fn poll(&mut self) -> Result<Option<Captured>> {
        if let Err(error) = self.dispatch() {
            super::super::STOP.store(true, Ordering::Relaxed);
            return Err(error);
        }
        // Metadata pipes remain bounded and never read the files named in a URI list.
        self.state
            .sending
            .retain_mut(|send| matches!(send.poll(), Ok(false)));
        if std::mem::take(&mut self.state.changed) {
            self.receiving = None;
            let Some(offer) = &self.state.selection else {
                return Ok(Some(Captured::Unsupported));
            };
            let types = &self.state.offers[offer];
            if types.contains(&self.state.marker) {
                return Ok(None);
            }
            let Some(mime) = MIMES.into_iter().find(|m| types.iter().any(|t| t == m)) else {
                return Ok(Some(Captured::Unsupported));
            };
            let mut fds = [-1; 2];
            ensure!(
                unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == 0,
                "cannot open clipboard pipe"
            );
            let reader = unsafe { File::from_raw_fd(fds[0]) };
            let writer = unsafe { OwnedFd::from_raw_fd(fds[1]) };
            let receiving = Receiving::new(reader, mime)?;
            offer.receive(mime.into(), writer.as_fd());
            self.receiving = Some(receiving);
            self.flush()?;
        }
        if let Some(mut read) = self.receiving.take() {
            if let Some(value) = read.poll()? {
                return Ok(Some(value));
            }
            self.receiving = Some(read);
        }
        Ok(None)
    }
    fn publish(&mut self, formats: Formats) -> Result<()> {
        self.receiving = None;
        let source = self
            .state
            .manager
            .as_ref()
            .unwrap()
            .create_data_source(&self.queue.handle(), formats.clone());
        for mime in formats.keys() {
            source.offer(mime.clone());
        }
        source.offer(self.state.marker.clone());
        self.device.set_selection(Some(&source));
        if let Some(old) = self.state.source.replace(source) {
            old.destroy();
        }
        self.flush()
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
        // Destroying our source cannot clear another client's newer clipboard selection.
        if let Some(source) = self.state.source.take() {
            source.destroy();
        }
        self.flush()
    }
}
impl Drop for Clipboard {
    fn drop(&mut self) {
        let _ = self.clear_owned();
        self.device.destroy();
        let _ = self.flush();
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "ext_data_control_manager_v1" if state.manager.is_none() => {
                    state.manager = Some(registry.bind(name, version.min(1), qh, ()))
                }
                "wl_seat" if state.seat.is_none() => {
                    state.seat = Some(registry.bind(name, version.min(1), qh, ()))
                }
                _ => {}
            }
        }
    }
}
wayland_client::delegate_noop!(State: ignore wl_seat::WlSeat);
wayland_client::delegate_noop!(State: ignore manager::ExtDataControlManagerV1);

impl Dispatch<device::ExtDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &device::ExtDataControlDeviceV1,
        event: device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            device::Event::DataOffer { id } => {
                state.offers.insert(id, Vec::new());
            }
            device::Event::Selection { id } => {
                if let Some(old) = std::mem::replace(&mut state.selection, id) {
                    state.offers.remove(&old);
                    old.destroy();
                }
                state.changed = true;
            }
            device::Event::PrimarySelection { id: Some(offer) } => {
                state.offers.remove(&offer);
                offer.destroy();
            }
            device::Event::Finished => state.finished = true,
            _ => {}
        }
    }
    wayland_client::event_created_child!(State, device::ExtDataControlDeviceV1, [0 => (offer::ExtDataControlOfferV1, ())]);
}
impl Dispatch<offer::ExtDataControlOfferV1, ()> for State {
    fn event(
        state: &mut Self,
        offer: &offer::ExtDataControlOfferV1,
        event: offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let offer::Event::Offer { mime_type } = event
            && let Some(types) = state.offers.get_mut(offer)
            && types.len() < 64
        {
            types.push(mime_type);
        }
    }
}
impl Dispatch<source::ExtDataControlSourceV1, Formats> for State {
    fn event(
        state: &mut Self,
        source: &source::ExtDataControlSourceV1,
        event: source::Event,
        formats: &Formats,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            source::Event::Send { mime_type, fd } => {
                if state.sending.len() < 8
                    && let Some(bytes) = formats.get(&mime_type)
                    && let Ok(send) = Sending::new(File::from(fd), bytes.clone())
                {
                    state.sending.push(send);
                }
            }
            source::Event::Cancelled => {
                if state.source.as_ref() == Some(source) {
                    state.source = None;
                }
                if source.is_alive() {
                    source.destroy();
                }
            }
            _ => {}
        }
    }
}
