//! TCI, Expert Electronics' Transceiver Control Interface: the protocol core. One WebSocket
//! carries text commands (CAT, CW, spots, meters, stream control) and binary frames (receive
//! audio, transmit audio, IQ). The SDR program is the server; Nexus is a client, and later a
//! server too.
//!
//! **The client core.** The codecs, and the client session built on them: the status model, the
//! dialect learned from a server, the admission a key must pass and the reconnect ladder. Nothing
//! in the app uses it yet: the daemon that serves it to the radio loop is a later change.
//!
//! # The specification
//!
//! Written from the specification alone: *TCI Protocol*, version 2.0, 12 January 2024, by Expert
//! Electronics ("Expert Group" LLC, Taganrog), under the MIT licence printed on its first page and
//! in its repository. The document is `TCI Protocol.pdf` in <https://github.com/ExpertSDR3/TCI> at
//! commit `b081213ff97150fd29f669c633f060f93c81a286` (41 pages, SHA-256
//! `2abd8d8df553403b28b8c7f0d910c8c777147821737d24cefb28ef08e67d8f09`). Section numbers here
//! (§3.1, §3.4, §4.2, …) are that document's. No code or text is taken from any implementation.
//! Where real servers depart from the document, the evidence is cited where the departure is
//! handled; those sources were read for their behaviour only.
//!
//! # Layers
//!
//! | Module | What it is |
//! |---|---|
//! | [`wire`] | text commands: a message split into commands, each read into a typed value and rendered back |
//! | [`frame`] | binary frames: the 64-byte header, the sample formats, the two readings of `length`, TX_CHRONO |
//! | this file | receiver and channel numbers, and the [`Dialect`] table |
//! | [`model`] | the radio as the server reports it, folded from its notices; a refused readback is unknown |
//! | [`dialect`] | which [`Dialect`] a server speaks, learned from its init burst and pinned at `ready;` |
//! | [`admission`] | the server-side checks a key must pass, and the only value a key renders from |
//! | [`session`] | one connection: handshake to `ready;`, the model, keys and their readback, stops, keepalive, the close |
//! | [`probe`] | "Find TCI server": one connection that reads a server's handshake to `ready;` and closes, writing nothing |
//! | [`reconnect`] | what to do when a connection fails or a session ends |
//!
//! # The transmit rule
//!
//! The codec keeps what can key apart by type, so the admission the session adds can sit on a
//! typed value and never on text ([`wire`] has the detail):
//!
//! - a [`wire::Start`] (`trx:<trx>,true`, `tune:<trx>,true`, a CW macro or message, a key-down,
//!   CW terminal mode) is rendered only inside this module;
//! - a [`wire::Stop`] is rendered by anyone, always: a stop is never gated;
//! - everything else a client sends is a [`wire::Command`], and no command renders text that a
//!   server reads as a start (a property test reads every rendered command back as a server would).
//!
//! # Outside data
//!
//! Everything a server sends is outside data. Neither decoder panics on any input, every length is
//! bounded before anything is allocated for it, and a malformed command or frame is refused with a
//! typed error ([`wire::WireError`], [`frame::FrameError`]) that the session counts and drops.
//! Receiver and channel numbers are bounded here ([`MAX_TRX`], [`MAX_CHANNELS`]), so a model
//! indexed by them stays small.

pub mod admission;
pub mod dialect;
pub mod frame;
pub mod model;
pub mod probe;
pub mod reconnect;
pub mod server;
pub mod session;
pub mod wire;

use std::fmt;

use frame::LengthMeaning;

/// Receiver numbers are below this. ExpertSDR announces `trx_count:2`, piHPSDR 1, AetherSDR one per
/// owned Flex slice (8 at most); no server announces more.
pub const MAX_TRX: u32 = 16;

/// Channel numbers within a receiver are below this. The spec names A, B and C (§4.1
/// `CHANNEL_COUNT`); servers announce 2.
pub const MAX_CHANNELS: u32 = 4;

/// A receiver (transceiver) number: `arg1` of most commands, and a frame's `receiver`. Below
/// [`MAX_TRX`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Trx(u8);

impl Trx {
    pub fn new(n: u32) -> Option<Trx> {
        if n < MAX_TRX {
            u8::try_from(n).ok().map(Trx)
        } else {
            None
        }
    }

    pub fn get(self) -> u32 {
        u32::from(self.0)
    }

    /// The number as an index, always below [`MAX_TRX`].
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

impl fmt::Display for Trx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A receive channel (VFO) within a receiver: 0 is A, 1 is B. Below [`MAX_CHANNELS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Channel(u8);

impl Channel {
    pub const A: Channel = Channel(0);
    pub const B: Channel = Channel(1);

    pub fn new(n: u32) -> Option<Channel> {
        if n < MAX_CHANNELS {
            u8::try_from(n).ok().map(Channel)
        } else {
            None
        }
    }

    pub fn get(self) -> u32 {
        u32::from(self.0)
    }

    /// The number as an index, always below [`MAX_CHANNELS`].
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// How a server spells its drive (output power) command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveForm {
    /// `drive:<trx>,<0–100>;`, read with `drive:<trx>;` (TCI 2.0, §4.2).
    PerTrx,
    /// `drive:<0–100>;`, read with `drive;`: TCI 1.x, as WSJT-X speaks it to ExpertSDR2
    /// (WSJT-X 3.0.2 `Transceiver/TCITransceiver.cpp:737-743`).
    Global,
}

/// How a server reports the receive level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SMeter {
    /// `rx_channel_sensors:<trx>,<ch>,<dBm>;`, pushed once the client sends
    /// `rx_sensors_enable:true;` (TCI 2.0, §4.5).
    Channel,
    /// `rx_sensors:<trx>,<dBm>;`, pushed the same way (TCI 1.9, §4.4; deprecated by 2.0).
    Receiver,
    /// `rx_smeter:<trx>,<ch>;` asked and `rx_smeter:<trx>,<ch>,<dBm>;` answered. Not in the
    /// spec; WSJT-X asks it of a server that does not announce ExpertSDR3
    /// (`TCITransceiver.cpp:469`, `:1278`) and reads the answer at `:647-648`.
    Poll,
}

/// What one server family does differently on the wire, as data. The encoders and the
/// session read these fields; nothing branches on a server's name. A server is never identified
/// by its `protocol:` line alone (piHPSDR announces `ExpertSDR3,2.0` and AetherSDR
/// `ExpertSDR3,1.5`), so the session learns the dialect from what the server does and starts from
/// [`EXPERT_SDR3`], the spec.
///
/// Only the differences that change bytes on the wire are here. The rest of what differs (when
/// `trx:true` is echoed, when the first TX_CHRONO comes, whether a dropped client is unkeyed, who
/// owns the transmitter, transmit full scale) is the session's and the bench's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dialect {
    pub name: &'static str,
    pub drive: DriveForm,
    /// Whether a key may name its signal source (`trx:<trx>,true,tci;`, TCI 2.0 §4.2). Where it
    /// cannot, a key that names one is not rendered ([`wire::Unrenderable`]): dropping the source
    /// would key the SDR's microphone instead of Nexus's audio.
    pub trx_source: bool,
    pub smeter: SMeter,
    /// What a TX_CHRONO's `length` counts ([`frame::Chrono::frames`]).
    pub chrono_length: LengthMeaning,
}

/// ExpertSDR3, the spec's own server (TCI 2.0). TX_CHRONO `length` counts values: WSJT-X answers
/// each request with that many values (`TCITransceiver.cpp:869-891` with `readAudioData` at
/// `:1529-1544`, two per stereo frame) and its transmit audio works on ExpertSDR3. Not measured
/// here.
pub const EXPERT_SDR3: Dialect = Dialect {
    name: "ExpertSDR3 (TCI 2.0)",
    drive: DriveForm::PerTrx,
    trx_source: true,
    smeter: SMeter::Channel,
    chrono_length: LengthMeaning::Values,
};

/// ExpertSDR2 (TCI 1.x), as WSJT-X speaks to it: drive without a receiver, no source on a key,
/// the S-meter polled. Its own behaviour is not measured.
pub const EXPERT_SDR2: Dialect = Dialect {
    name: "ExpertSDR2 (TCI 1.x)",
    drive: DriveForm::Global,
    trx_source: false,
    smeter: SMeter::Poll,
    chrono_length: LengthMeaning::Values,
};

/// Thetis announcing itself (`protocol:Thetis,…`), as WSJT-X treats it (`TCITransceiver.cpp:822`):
/// drive per receiver (`:740`), the S-meter polled (`:469`, `:1278`), and keys sent without a
/// source (`:1085-1090`). No source is assumed here: Thetis's v2.10.3.14 notes say its ExpertSDR3
/// emulation, the default since that release, is what lets WSJT-X and JTDX transmit TCI audio.
/// In that emulation it announces ExpertSDR3 and is read as [`EXPERT_SDR3`].
pub const THETIS_NATIVE: Dialect = Dialect {
    name: "Thetis (its own protocol line)",
    drive: DriveForm::PerTrx,
    trx_source: false,
    smeter: SMeter::Poll,
    chrono_length: LengthMeaning::Values,
};

/// piHPSDR (`src/tci.c` at `3b63530267`): `drive:0,<v>;`, `trx` with a `tci` source,
/// `rx_channel_sensors` pushed (and `rx_sensors` beside it).
pub const PIHPSDR: Dialect = Dialect {
    name: "piHPSDR",
    drive: DriveForm::PerTrx,
    trx_source: true,
    smeter: SMeter::Channel,
    chrono_length: LengthMeaning::Values,
};

/// AetherSDR's TCI server in front of a Flex (`src/core/TciServer.cpp` at `05c5b805`): a
/// TX_CHRONO of `length` 2048 with 2 channels asks for 1024 stereo frames.
pub const AETHERSDR: Dialect = Dialect {
    name: "AetherSDR",
    drive: DriveForm::PerTrx,
    trx_source: true,
    smeter: SMeter::Channel,
    chrono_length: LengthMeaning::Values,
};

/// Every dialect, the spec's first.
pub const DIALECTS: [Dialect; 5] = [EXPERT_SDR3, EXPERT_SDR2, THETIS_NATIVE, PIHPSDR, AETHERSDR];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receiver_and_channel_numbers_are_bounded() {
        assert_eq!(Trx::new(0).map(Trx::get), Some(0));
        assert_eq!(Trx::new(MAX_TRX - 1).map(Trx::index), Some(15));
        // The bound itself, and the first number a byte could still hold past it.
        assert_eq!(Trx::new(MAX_TRX), None);
        assert_eq!(Trx::new(20), None);
        assert_eq!(Trx::new(300), None);
        assert_eq!(Trx::new(u32::MAX), None);
        assert_eq!(Channel::new(1), Some(Channel::B));
        assert_eq!(Channel::new(MAX_CHANNELS - 1).map(Channel::get), Some(3));
        assert_eq!(Channel::new(MAX_CHANNELS), None);
        assert_eq!(Channel::new(9), None);
        assert_eq!(Channel::new(u32::MAX), None);
    }

    #[test]
    fn the_dialect_table_holds_the_spellings_plan_2_5_records() {
        let names: Vec<_> = DIALECTS.iter().map(|d| d.name).collect();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "{names:?}");
        assert_eq!(DIALECTS[0], EXPERT_SDR3, "the spec's server comes first");
        // TCI 1.x is the only dialect with a global drive; it and Thetis announcing itself are the
        // two whose keys carry no source, and the two whose S-meter is polled.
        for d in DIALECTS {
            assert_eq!(d.drive == DriveForm::Global, d == EXPERT_SDR2, "{}", d.name);
            let as_wsjtx_keys_it = d == EXPERT_SDR2 || d == THETIS_NATIVE;
            assert_eq!(!d.trx_source, as_wsjtx_keys_it, "{}", d.name);
            assert_eq!(d.smeter == SMeter::Poll, as_wsjtx_keys_it, "{}", d.name);
        }
        assert_eq!(EXPERT_SDR3.smeter, SMeter::Channel);
        // Every server read counts a chrono's length in values.
        assert!(DIALECTS
            .iter()
            .all(|d| d.chrono_length == LengthMeaning::Values));
    }
}
