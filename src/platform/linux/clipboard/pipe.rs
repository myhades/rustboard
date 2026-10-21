use super::Captured;
use anyhow::{Result, ensure};
use std::{
    fs::File,
    io::{Read, Write},
    os::fd::AsRawFd,
    sync::Arc,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(5);

fn nonblocking(file: &File) -> Result<()> {
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    ensure!(
        flags >= 0
            && unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                == 0,
        "cannot configure clipboard pipe"
    );
    Ok(())
}

pub(super) struct Receiving {
    file: File,
    mime: &'static str,
    bytes: Vec<u8>,
    touched: Instant,
}
impl Receiving {
    pub fn new(file: File, mime: &'static str) -> Result<Self> {
        nonblocking(&file)?;
        Ok(Self {
            file,
            mime,
            bytes: Vec::new(),
            touched: Instant::now(),
        })
    }
    pub fn poll(&mut self) -> Result<Option<Captured>> {
        ensure!(self.touched.elapsed() < TIMEOUT, "clipboard read timed out");
        let mut buffer = [0u8; 65536];
        for _ in 0..16 {
            match self.file.read(&mut buffer) {
                Ok(0) => {
                    return super::capture(self.mime, std::mem::take(&mut self.bytes)).map(Some);
                }
                Ok(n) => {
                    ensure!(
                        self.bytes.len() + n
                            <= if self.mime.starts_with("image/") {
                                crate::app::image_file::MAX_IMAGE_BYTES
                            } else {
                                crate::core::MAX_TEXT
                            },
                        "clipboard data too large"
                    );
                    self.bytes.extend_from_slice(&buffer[..n]);
                    self.touched = Instant::now();
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(None)
    }
}

pub(super) struct Sending {
    file: File,
    bytes: Arc<Vec<u8>>,
    offset: usize,
    touched: Instant,
}
impl Sending {
    pub fn new(file: File, bytes: Arc<Vec<u8>>) -> Result<Self> {
        nonblocking(&file)?;
        Ok(Self {
            file,
            bytes,
            offset: 0,
            touched: Instant::now(),
        })
    }
    pub fn poll(&mut self) -> Result<bool> {
        ensure!(
            self.touched.elapsed() < TIMEOUT,
            "clipboard write timed out"
        );
        match self.file.write(&self.bytes[self.offset..]) {
            Ok(n) => {
                self.offset += n;
                self.touched = Instant::now();
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(e.into()),
        }
        Ok(self.offset == self.bytes.len())
    }
}
