//! What the TCI server asks of the app: [`TciBackend`], one call for each thing a program may
//! change, each answered by the app (the engine's TCI side in the app; a mock in this module's
//! tests). The server never touches the radio itself.
//!
//! # The transmit rule
//!
//! Every call that can put the transmitter on the air is a verdict: [`TciBackend::key`],
//! [`TciBackend::tune_on`] and [`TciBackend::cw`] run the station's gates (every
//! Nexus gate first, then TCI's own) and answer `Ok` or the first [`Refusal`]. The server never
//! decides that a program may transmit; it only refuses more (a receiver other than 0, a source it
//! does not know, the forms its command table refuses outright). A program's refused key is
//! answered on the wire as the spec answers any key that did not take, `trx:0,false;`.
//!
//! Every call that ends a transmission is required, with no default, so an implementation cannot
//! leave one out: [`TciBackend::release`], [`TciBackend::tune_off`], [`TciBackend::end_own`],
//! [`TciBackend::cw_stop`] and [`TciBackend::disconnected`]. None of them is gated, and the server
//! still passes a program's stops on while it drops the rest of a flood (`conn.rs`).
//!
//! # Threads and locks
//!
//! Calls come from each program's own reader thread: one at a time for a program, several
//! programs at once. An implementation is `Send + Sync` and takes its own lock briefly, as the CAT
//! broker's backend does. The server holds none of its own locks while it calls the backend, so a
//! backend may call the server's [`Handle`](super::Handle) from inside a call (to publish a
//! snapshot after a change, say). The reverse holds too: [`Handle::publish`](super::Handle::publish)
//! never calls the backend, so the app may publish while it holds its engine lock. It must: a
//! snapshot built and published under the lock that [`TciBackend::key`] and the setters take is
//! never older than a command's answer, so a stale snapshot cannot undo an echo the server has
//! just sent (`Snapshot` has the detail).
//!
//! A program's [`TciBackend::disconnected`] waits for that program's calls in flight that can
//! transmit ([`TciBackend::key`], [`TciBackend::tune_on`], [`TciBackend::cw`],
//! [`TciBackend::tx_audio`]), and for a [`TciBackend::spot`] or [`TciBackend::set_mode`] in flight,
//! and none of them is made for it afterwards: what such a call admitted just before the end is
//! ended by it, nothing the program sends transmits after it, and no spot or mode ask of its
//! outlives it. So the
//! app must never call [`Handle::disconnect`](super::Handle::disconnect) or
//! [`Server::stop`](super::Server::stop) while holding the lock its backend takes, nor from inside
//! a backend call.

use std::fmt;
use std::net::SocketAddr;

use crate::tci::frame::{AudioRate, Channels, SampleFormat};
use crate::tci::wire::{Call, Hz, Text, Token, Word};

/// One program's connection: numbered from 1 in the order programs connect, never reused while
/// the server runs. The engine records a program's ownership of the transmitter by this number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientId(u64);

impl ClientId {
    pub(super) fn new(n: u64) -> ClientId {
        ClientId(n)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "program {}", self.0)
    }
}

/// Where a program's key takes its audio (`trx:0,true[,<source>];`, §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyKind {
    /// `trx:0,true,tci;`: an over carrying the program's own TCI audio. Admitted only
    /// while that program's audio stream is started (the verdict's item 9); it never falls back
    /// to the microphone.
    Tci,
    /// The radio's microphone: `trx:0,true;` with no source, or a microphone source. Phone only.
    Mic(Mic),
}

/// A microphone a key may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mic {
    /// No source given: the SDR program's selected microphone.
    Selected,
    Mic1,
    Mic2,
    MicPc,
    /// The E-Coder2 panel's.
    Ecoder2,
}

/// Why the backend refused a program's command: the first gate that failed, in the verdict's
/// order, or the dial or mode rule that applies. The server names it in the connection log; on the
/// wire a refusal looks like the command's own answer (`trx:0,false;`, the value in force).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// "Programs on TCI may transmit" is off (item 2).
    Switch,
    /// Transmit is not enabled in Nexus (item 3).
    Disarmed,
    /// The operator's Stop holds until the operator's own arm (item 3).
    StopHold,
    /// A standing watchdog trip (item 3).
    WatchdogTrip,
    /// A band or radio change holds programs' keys until the operator's arm (item 3).
    ContextHold,
    /// The radio's connection cannot transmit (item 4).
    Connection,
    /// A `tci` key while the radio takes its transmit audio from its own microphone (item 4): the
    /// program's audio would not reach the air, and the microphone would carry the over.
    RadioMic,
    /// Outside the licence: for an audio over, the whole transmit passband (item 5).
    Licence,
    /// A key where the licence allows only CW and single-sideband voice (a Novice or a
    /// Technician from 28.3 to 28.5 MHz), whatever the radio's mode (item 5): a program's audio
    /// may be a picture or data, which Nexus cannot tell from voice.
    VoiceOnly,
    /// Something else owns the transmitter (item 6).
    Owner,
    /// The clock repair holds transmit (item 7).
    ClockRepair,
    /// A device signed in moments ago holds transmit until it is approved at the station (item 7,
    /// the 2026-10-23 sign-in decision).
    OnHold,
    /// Not a section this command may use (item 8).
    Section,
    /// A `tci` key in the Digital section (item 8): FT8, FT4 and the other slot-timed modes go out
    /// through the server only once the loop bench has measured Nexus's added delay at or under
    /// 0.2 s and the recorded overs decode (the operator's ruling of 2026-10-19). Until then the
    /// section refuses them. Also a `tci` key in any section from a program whose last
    /// `modulation:` asked a data mode: a data-mode over from a program is held like FT8 (the
    /// operator's ruling of 2026-10-20).
    SlotTimed,
    /// A `tci` key from a program whose last `modulation:` asked no voice mode, or that has asked
    /// none (item 8): a program's own audio goes out only after it asks USB, LSB, AM or FM (the
    /// operator's ruling of 2026-10-20). WSJT-X, JTDX and MSHV ask none in their default setup.
    VoiceMode,
    /// A `tci` key with no audio stream started (item 9).
    AudioStream,
    /// CW text Nexus will not key (`cw_macros`): a character its keyer cannot send, a `;`,
    /// over 256 characters, or more than Nexus queues for programs at once. Refused whole.
    CwText,
    /// A dial or mode change while anything is on the air.
    OnAir,
    /// Within 200 ms of the operator's own change.
    OperatorChange,
    /// Outside the radio's range.
    Limits,
    /// Power raised: a program may lower it, never raise it.
    Raise,
    /// A mode outside the section's family.
    Family,
    /// The tier is off or not offered (CW, spots).
    Tier,
    /// A spot whose callsign is not one: 1 to 16 letters, digits and `/`, shaped like a callsign.
    SpotCall,
    /// No receive source for programs' audio (`audio_start`).
    NoSource,
}

impl Refusal {
    /// The gate, in the connection log's words.
    pub fn text(self) -> &'static str {
        match self {
            Refusal::Switch => "programs on TCI may not transmit",
            Refusal::Disarmed => "transmit is not enabled",
            Refusal::StopHold => "Stop TX holds until the operator arms",
            Refusal::WatchdogTrip => "the TX watchdog has tripped",
            Refusal::ContextHold => "a band or radio change holds programs' keys",
            Refusal::Connection => "this radio's connection cannot transmit",
            Refusal::RadioMic => "the radio takes its transmit audio from its own microphone",
            Refusal::Licence => "outside the licence",
            Refusal::VoiceOnly => {
                "on 28.3–28.5 MHz a Novice or Technician license allows only CW and \
                 single-sideband voice, so Nexus does not let other programs key the radio there"
            }
            Refusal::Owner => "something else is transmitting",
            Refusal::ClockRepair => "the clock is being repaired",
            Refusal::OnHold => "a device just signed in and holds transmit",
            Refusal::Section => "not in this section",
            Refusal::SlotTimed => {
                "FT8, FT4 and other slot-timed modes may not transmit through TCI yet, and a \
                 data-mode over from a program is held like FT8"
            }
            Refusal::VoiceMode => {
                "a program may transmit its own audio only after it asks for a voice mode (USB, \
                 LSB, AM or FM): set the program's Radio mode, or use Nexus for FT8; some \
                 programs, WSJT-X and JTDX among them, ask for their mode only when it differs \
                 from Nexus's"
            }
            Refusal::AudioStream => "no TCI audio stream",
            Refusal::CwText => {
                "CW Nexus will not send: a character with no Morse, a semicolon, over 256 \
                 characters, or too much already waiting"
            }
            Refusal::OnAir => "not while transmitting",
            Refusal::OperatorChange => "the operator just changed it",
            Refusal::Limits => "outside the radio's range",
            Refusal::Raise => "programs may lower power, not raise it",
            Refusal::Family => "not this section's mode",
            Refusal::Tier => "not offered",
            Refusal::SpotCall => "a spot whose callsign is not one",
            Refusal::NoSource => "no receive audio to send",
        }
    }
}

/// A program's audio stream as it set it (§4.3), the spec's defaults until it does: the shape of
/// the receive audio it is sent, and how its transmit frames are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AudioFormat {
    /// `audio_samplerate`.
    pub rate: AudioRate,
    /// `audio_stream_sample_type`.
    pub sample_type: SampleFormat,
    /// `audio_stream_channels`.
    pub channels: Channels,
    /// `audio_stream_samples`: the `length` of each receive frame, within
    /// [`AudioFormat::SAMPLES`].
    pub samples: u32,
}

impl AudioFormat {
    /// The range the spec allows `audio_stream_samples` (§4.3).
    pub const SAMPLES: std::ops::RangeInclusive<u32> = 100..=2048;
}

impl Default for AudioFormat {
    /// 48 kHz float32 stereo, 2048 values a frame: the spec's defaults (§4.3), and what the init
    /// burst tells a program.
    fn default() -> Self {
        AudioFormat {
            rate: AudioRate::new(48_000).unwrap_or_else(|| unreachable!("48 kHz is a TCI rate")),
            sample_type: SampleFormat::Float32,
            channels: Channels::Two,
            samples: 2048,
        }
    }
}

/// A CW request (§3.2): the text as the program sent it, unescaped. Filtering it to what Nexus
/// can time is the backend's.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Cw {
    /// `cw_macros:0,<text>;`
    Macro(Text),
    /// `cw_msg:0,<prefix>,<call>,<suffix>;`, sent as one text.
    Message {
        prefix: Text,
        call: Text,
        suffix: Text,
    },
}

/// A CW speed change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CwSpeed {
    /// `cw_macros_speed:<wpm>;`
    Set(u32),
    /// `cw_macros_speed_up:<n>;`
    Up(u32),
    /// `cw_macros_speed_down:<n>;`
    Down(u32),
}

/// A spot from a program (`spot:<call>,<mode>,<hz>,<argb>,<text>;`, §4.3), as the codec read it.
/// The store's own rules are the backend's.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Spot {
    pub call: Call,
    pub mode: Token,
    pub hz: Hz,
    pub argb: u32,
    pub text: Text,
}

/// Why a connection was refused before it became a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeRefusal {
    /// It carried an `Origin` header: a web page. The origin, cut to printable ASCII
    /// and 100 characters.
    Origin(String),
    /// Its `Host` header was missing, repeated or not this computer's loopback name and port: DNS
    /// rebinding. What it said, cut the same way.
    Host(String),
    /// Not WebSocket version 13 (answered 426).
    Version,
    /// Not a WebSocket handshake at all (answered 400).
    Malformed,
    /// The server already has its most programs (answered 503).
    Full,
    /// Too many handshakes at once: closed unanswered.
    Busy,
    /// No complete handshake inside the deadline: closed unanswered.
    Timeout,
    /// The connection ended before its handshake did.
    Incomplete,
    /// Not from this computer: never, while the server binds only loopback, and refused anyway.
    NotLocal,
}

/// Why a program's connection ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Closed {
    /// The program closed it.
    Program,
    /// The connection failed.
    Lost,
    /// No answer to the server's pings in time.
    Silent,
    /// Over the command rate for too long.
    Flood,
    /// A message over the size cap (closed 1009).
    TooBig,
    /// Messages in a row that were not TCI at all.
    Garbage,
    /// A WebSocket protocol error.
    Protocol,
    /// It stopped reading what the server sent (the outbox).
    Backlog,
    /// The server stopped.
    Stopped,
    /// The operator disconnected it.
    Disconnected,
}

/// A command the server refuses outright, named in the connection log once per
/// program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Refused {
    /// A receiver other than 0: Nexus serves one (`trx_count:1`).
    Receiver,
    /// A channel other than A and B.
    Channel,
    /// `dds` or `if`: Nexus has no panorama to move.
    Panorama,
    /// `vfo:0,1` away from the dial: no split.
    Split,
    /// A per-receiver switch Nexus does not offer through TCI (`rit_enable`, the receiver's DSP,
    /// `lock`, …), or `split_enable:…,true`.
    Switch,
    /// `rit_offset`, `xit_offset`.
    Offset,
    /// `rx_channel_enable`.
    Channels,
    /// `rx_filter_band`.
    Filter,
    /// Volumes, mute, the monitor.
    Audio,
    /// AGC, the noise blanker's parameters, squelch.
    Dsp,
    /// `digl_offset`, `digu_offset`.
    DigitalOffset,
    /// `cw_macros_delay`, `cw_keyer_speed`.
    CwSetting,
    /// IQ: none from a sound-card radio.
    Iq,
    /// `line_out_start`, `line_out_stop`.
    LineOut,
    /// `line_out_recorder_*`: it would write a file at a path the program names.
    Recorder,
    /// `set_in_focus`: a program does not take Nexus's window.
    Focus,
    /// `keyer`: straight-key streams.
    Keyer,
    /// `cw_terminal:true`: Nexus never holds the transmitter after a macro.
    CwTerminal,
    /// `cw_msg:<call>`: no edits to a message in flight.
    CwCall,
    /// `rx_enable:…,false`: the receiver stays on.
    RxEnable,
    /// `tx_enable` from a program: a notice, never an arm.
    TxEnable,
    /// A key naming a source Nexus does not know.
    Source,
    /// A mode Nexus does not list (`modulations_list`).
    Mode,
    /// A `drive` with no receiver (TCI 1.x).
    OldDrive,
    /// An `audio_stream_samples` outside 100–2048.
    Samples,
}

impl Refused {
    /// The command, in the connection log's words.
    pub fn text(self) -> &'static str {
        match self {
            Refused::Receiver => "a receiver other than 0",
            Refused::Channel => "a channel other than A or B",
            Refused::Panorama => "panorama (dds, if)",
            Refused::Split => "split (vfo B away from the dial)",
            Refused::Switch => "a receiver switch Nexus does not offer through TCI",
            Refused::Offset => "RIT or XIT offset",
            Refused::Channels => "rx_channel_enable",
            Refused::Filter => "rx_filter_band",
            Refused::Audio => "volume, mute or monitor",
            Refused::Dsp => "AGC, noise blanker or squelch settings",
            Refused::DigitalOffset => "digl_offset or digu_offset",
            Refused::CwSetting => "cw_macros_delay or cw_keyer_speed",
            Refused::Iq => "IQ",
            Refused::LineOut => "line out",
            Refused::Recorder => "the line-out recorder (writes a file)",
            Refused::Focus => "set_in_focus",
            Refused::Keyer => "keyer (straight key)",
            Refused::CwTerminal => "cw_terminal:true",
            Refused::CwCall => "cw_msg callsign edit",
            Refused::RxEnable => "rx_enable:false",
            Refused::TxEnable => "tx_enable from a program",
            Refused::Source => "a key with an unknown source",
            Refused::Mode => "a mode Nexus does not list",
            Refused::OldDrive => "drive without a receiver",
            Refused::Samples => "audio_stream_samples outside 100-2048",
        }
    }
}

/// What the server reports for the connection log. Each is reported once: a program's refusals of
/// one kind are reported again only when the answer changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A program connected.
    Connected { client: ClientId, peer: SocketAddr },
    /// A program's connection ended.
    Closed { client: ClientId, why: Closed },
    /// A connection was refused before it became a program.
    Refused {
        peer: SocketAddr,
        why: HandshakeRefusal,
    },
    /// A program's key, Tune, CW or change was refused by the backend's gate.
    Refusal { client: ClientId, refusal: Refusal },
    /// A program's command was refused by the server.
    CommandRefused { client: ClientId, what: Refused },
}

/// The app's side of the server. See the module header for the transmit rule and the lock rule.
pub trait TciBackend: Send + Sync {
    /// A program connected; its handshake passed every check.
    fn connected(&self, _client: ClientId, _peer: SocketAddr) {}

    /// The program is gone: it closed, the connection failed or went silent, or the server or the
    /// operator ended it. Anything it keyed, tuned or sent as CW ends hard now (a lost
    /// socket unkeys within one radio-loop tick). Called once a program, before its slot is freed,
    /// after its transmit calls, its spot and its mode ask in flight have returned, and never
    /// followed by another of them. No lock of the server's is held but that program's transmit
    /// gate, which only its transmit calls, its spots and its mode asks take.
    fn disconnected(&self, client: ClientId);

    /// A line for the connection log.
    fn event(&self, _event: Event) {}

    /// `vfo:0,0,<hz>;`: retune. `Ok` with the dial now in force; the server has already refused a
    /// frequency outside the snapshot's `vfo_limits`. Refuse on air, within 200 ms of the
    /// operator's own change, and as the CAT broker's dial rule does.
    fn set_dial(&self, client: ClientId, hz: Hz) -> Result<Hz, Refusal>;

    /// `modulation:0,<mode>;`, a word from [`super::MODULATIONS`]: `Ok` with the mode now in force.
    /// Only the side inside the section's family. Asked through the program's gate,
    /// as a spot is: never once its end has begun, and its end waits for one in flight, so a mode
    /// kept as the program's ask never outlives it.
    fn set_mode(&self, client: ClientId, mode: &Word) -> Result<Word, Refusal>;

    /// `drive:0,<level>;`: `Ok` with the level now in force. Lower only, under the
    /// operator's ceiling.
    fn set_drive(&self, client: ClientId, level: u32) -> Result<u32, Refusal>;

    /// `tune_drive:0,<level>;`, as [`TciBackend::set_drive`], against the Tune power.
    fn set_tune_drive(&self, client: ClientId, level: u32) -> Result<u32, Refusal>;

    /// `trx:0,true[,<source>];`: the verdict, every Nexus gate first. `Ok` arms the
    /// program's over (a [`KeyKind::Tci`] over keys on its first audio). A program
    /// re-asserting its own key is answered as before. `audio_started`: whether this program's
    /// `audio_start` is in force (the verdict's item 9).
    fn key(&self, client: ClientId, kind: KeyKind, audio_started: bool) -> Result<(), Refusal>;

    /// `trx:0,false…;`. From the program that keyed: end its over gracefully (the queued audio
    /// plays out, then unkey). From any other program: halt whatever is on the air,
    /// as WSJT-X's UDP HaltTx does (the operator's choice); with nothing on the air,
    /// nothing. Never gated.
    fn release(&self, client: ClientId);

    /// `tune:0,true;`: the verdict, with the licence judged at the carrier and Nexus's tune timeout.
    fn tune_on(&self, client: ClientId) -> Result<(), Refusal>;

    /// `tune:0,false;`: end Tune, whoever started it. Never gated.
    fn tune_off(&self, client: ClientId);

    /// `stop;`: end this program's own key, Tune and CW, and nothing else; the radio keeps
    /// running. Never gated.
    fn end_own(&self, client: ClientId);

    /// One transmit-audio frame (type 2, receiver 0) from the program that holds an admitted
    /// [`KeyKind::Tci`] key, to be read with its `format` (not the header's `channels`, which
    /// WSJT-X leaves unset). The frame is cut to the `length` values it claims: the server refuses
    /// a frame whose claim and payload disagree, drops every other program's frames, and bounds the
    /// owner's to twice real time. The backend checks the owner again, since an over can end
    /// between the two.
    fn tx_audio(&self, client: ClientId, format: AudioFormat, frame: &[u8]);

    /// `audio_start:0;`: `Ok` when a receive source exists, and the program is then sent receive
    /// audio in `format` (through [`Handle::send_rx_audio`](super::Handle::send_rx_audio)).
    fn audio_start(&self, client: ClientId, format: AudioFormat) -> Result<(), Refusal>;

    /// `audio_stop:0;`.
    fn audio_stop(&self, client: ClientId);

    /// `cw_macros` or `cw_msg`: the verdict, CW section only. Not offered unless implemented.
    fn cw(&self, _client: ClientId, _cw: Cw) -> Result<(), Refusal> {
        Err(Refusal::Tier)
    }

    /// `cw_macros_stop;`: stop CW from any origin. Never gated.
    fn cw_stop(&self, client: ClientId);

    /// A CW speed change: `Ok` with the speed now in force. Not offered unless implemented.
    fn cw_speed(&self, _client: ClientId, _speed: CwSpeed) -> Result<u32, Refusal> {
        Err(Refusal::Tier)
    }

    /// A spot from a program. It never tunes and never keys. Not offered unless
    /// implemented. Asked through the program's gate, as a transmit call is: never once its end has
    /// begun, and its end waits for one in flight, so the spots it drops at
    /// [`TciBackend::disconnected`] never come back.
    fn spot(&self, _client: ClientId, _spot: Spot) -> Result<(), Refusal> {
        Err(Refusal::Tier)
    }

    /// `spot_delete:<call>;`: this program's own spot only.
    fn spot_delete(&self, _client: ClientId, _call: &Call) {}

    /// `spot_clear;`: this program's own spots only.
    fn spot_clear(&self, _client: ClientId) {}
}
