//! [`LanCivIo`]: the CI-V engine's byte transport over the network session.
//!
//! The engine reads and writes a byte stream, as it does a serial port; the session carries CI-V
//! one frame per datagram. So `Read` hands out the frames the session delivered, one after the
//! other, and `Write` collects bytes up to each frame's `FD` and hands every whole frame to the
//! session thread, which says at once whether it went out.
//!
//! The engine's rules for a transport, and how each maps here:
//! - a read with nothing to give TIMES OUT after [`READ_TIMEOUT`], as the serial port does;
//! - a read once the session is lost or closed is an ERROR, so the engine stops and its daemon is
//!   no longer alive (the radio loop then rebuilds it, on the retry ladder);
//! - a write the session refuses because its CI-V socket is reporting errors is a TIMEOUT: the
//!   engine fails that one request at once, rather than waiting out its deadline, and lives on;
//! - a write once the session is no longer connected is an ERROR, which stops the engine.
//!
//! Only CI-V payloads pass through here, so the CI-V diagnostic log, which the engine feeds, never
//! sees the login.

use std::io::{self, Read, Write};
use std::sync::mpsc;

use tempo_net::icom::session::CivError;

use super::{Outgoing, SEND_WAIT};
pub use crate::civ::engine::READ_TIMEOUT;

/// The CI-V engine's byte transport over the network session. See the module notes.
pub struct LanCivIo {
    /// CI-V frames the session delivered.
    inbox: mpsc::Receiver<Vec<u8>>,
    /// The rest of a delivered frame a short read did not take.
    unread: Vec<u8>,
    /// Frames for the radio, each with a channel for whether it went.
    outbox: mpsc::Sender<Outgoing>,
    /// Bytes written since the last `FD`.
    partial: Vec<u8>,
}

impl LanCivIo {
    pub(super) fn new(inbox: mpsc::Receiver<Vec<u8>>, outbox: mpsc::Sender<Outgoing>) -> LanCivIo {
        LanCivIo {
            inbox,
            unread: Vec::new(),
            outbox,
            partial: Vec::new(),
        }
    }

    /// Hands one whole frame to the session thread and waits for its answer.
    fn send_frame(&self, frame: Vec<u8>) -> io::Result<()> {
        let (reply, answer) = mpsc::sync_channel(1);
        if self.outbox.send((frame, reply)).is_err() {
            return Err(gone());
        }
        match answer.recv_timeout(SEND_WAIT) {
            Ok(Ok(())) => Ok(()),
            // The CI-V socket is failing: this request cannot be answered, the session may yet be.
            Ok(Err(CivError::SocketFailing)) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the radio's CI-V socket is reporting errors",
            )),
            // Longer than a CI-V packet can carry; no command the engine sends is.
            Ok(Err(CivError::TooLong { .. } | CivError::Frame(_))) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "a CI-V frame the network packet cannot carry",
            )),
            Ok(Err(CivError::NotConnected)) => Err(gone()),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the network session did not take the frame in time",
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(gone()),
        }
    }
}

fn gone() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "the radio's network session has ended",
    )
}

impl Read for LanCivIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.unread.is_empty() {
            match self.inbox.recv_timeout(READ_TIMEOUT) {
                Ok(frame) => self.unread = frame,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "no data"))
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "the radio's network session has ended",
                    ))
                }
            }
        }
        let n = buf.len().min(self.unread.len());
        buf[..n].copy_from_slice(&self.unread[..n]);
        self.unread.drain(..n);
        Ok(n)
    }
}

impl Write for LanCivIo {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for &b in buf {
            self.partial.push(b);
            if b == crate::civ::frame::END {
                let frame = std::mem::take(&mut self.partial);
                self.send_frame(frame)?;
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
