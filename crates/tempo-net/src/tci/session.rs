//! One TCI connection to an SDR program, from the WebSocket handshake to the close.
//!
//! [`Session`] is the protocol core, with no I/O of its own: messages in ([`Session::on_text`],
//! [`Session::on_binary`]), messages out through the [`Outbox`] each call is given, time passed in
//! as milliseconds on a monotonic clock. [`Connection`] runs it on a std thread over a synchronous
//! tungstenite WebSocket to the SDR program's TCI server (the simulator in tests).
//!
//! **What the session does.**
//! - *Connect* ([`Connection::connect`]): TCP within [`CONNECT_TIMEOUT`], then the WebSocket
//!   handshake within [`HANDSHAKE_TIMEOUT`]. The WebSocket layer refuses a message over
//!   [`MAX_WS_MESSAGE`], and the codecs refuse much less ([`super::wire::MAX_MESSAGE`],
//!   [`super::frame::MAX_FRAME`]), so nothing a server sends is held without a bound, and nothing
//!   is waited for without one either: the handshake ends at its deadline, each read of the
//!   driver within [`READ_POLL`], however the server's bytes trickle in, and each message the
//!   session writes within [`WRITE_TIMEOUT`], however slowly the server reads it. A message that
//!   runs out of time ends the session as a lost link ([`End::Lost`]), its key carried to the next.
//! - *The handshake to `ready`* (TCI 2.0 §4.1): the init burst is folded into the model
//!   ([`super::model`]) and the dialect learned from it ([`super::dialect`]). At `ready;` the
//!   dialect is pinned and the session's own setup goes out, in this order: the stops an earlier
//!   session left owing ([`Config::carried`]); `audio_start:<trx>;` when Nexus's audio is on; the
//!   meters (pushed where the dialect pushes, otherwise `rx_smeter` polled every
//!   [`METER_POLL_MS`]). No `ready;` within [`Config::ready_timeout_ms`] ends the session
//!   ([`End::NoReady`]). Each text message is read on its own: piHPSDR sends two unterminated
//!   burst lines, and buffering across messages would swallow its `ready;`.
//! - *The model* is the server's word only. A readback the codec refuses leaves its parameter
//!   unknown, never the value before ([`super::model::Reading::Unreadable`]); the session asks
//!   again where a read exists, at most once a [`REREAD_MS`].
//! - *Keepalive*: a WebSocket ping every [`Config::ping_interval_ms`], and anything the server
//!   sends (a pong, a notice, a frame) counts as heard. Silence for [`Config::keyed_silence_ms`]
//!   while an over of ours is open sends its stops at once ([`Halt::Silent`]); silence for
//!   [`Config::stale_ms`] ends the session ([`End::Stale`]). A half-open socket is never trusted.
//! - *Receive audio and IQ* for this session's transceiver go to the [`Streams`] the owner gives
//!   it, on this thread. A receive block's arrival time is what admission's freshness term reads.
//!   IQ also goes to the panadapter's [`IqTap`] while one is set ([`Connection::set_iq_tap`]),
//!   with the receiver's centre frequency (`dds`) as the server last reported it before the
//!   block: the panadapter is not the owner of the session's audio, so it never stands between
//!   the session and those streams.
//! - *IQ*: `iq_start:<trx>;` and `iq_stop:<trx>;` go out through [`Session::send`] like any other
//!   stream command. A session that started its transceiver's IQ stops it in its close, after
//!   the stops ([`Session::close`]).
//!
//! **Transmit.** [`Session::key`] runs admission ([`super::admission`]); only an admitted key is
//! rendered, and the session's keyed belief is set before the write, so a key whose write fails
//! partway still needs its stop. The belief clears only on the server's own report that the
//! transmitter is off (`trx:<trx>,false` for an over, `tune:<trx>,false` for a tune), and only a
//! report that cannot predate the key: one after its echo, after our stop, or on a new connection.
//! A lost link never clears it; the belief survives the session
//! ([`Snapshot::keyed`], [`Event::Closed`]) for the next one. While a key of ours is open:
//! - *TX_CHRONO* for our transceiver is answered at once with exactly the frames it asks for, read
//!   with the dialect's `length` meaning: Nexus's audio from [`Streams::transmit`] while the over is
//!   open, silence after our stop (until the server reports the unkey), and nothing at all when no
//!   key of ours is on the air: another program's over gets none of our samples. A chrono
//!   whose answer would not fit one frame is dropped and counted before anything is sized by it.
//! - *Nexus's audio stream* stays up for the whole over: a server report that it stopped
//!   ends the over at once ([`Halt::AudioStopped`]), and [`Session::send`] will not stop or reshape
//!   it while a key of ours that takes it may be up ([`SendError::AudioInUse`]).
//! - *The chrono window*: a key that takes Nexus's audio with no TX_CHRONO within
//!   [`Config::chrono_window_ms`] of its write is ended at once ([`Halt::NoChrono`]): the server is
//!   not taking Nexus's audio, and may be transmitting its microphone.
//! - *The echo window*: a key the server has not echoed within [`Config::key_echo_ms`] is ended
//!   ([`Halt::NoEcho`]). An echo that names a source other than Nexus's audio for a key that takes
//!   it ends the over at once ([`Halt::WrongSource`]).
//! - *The server's own say*: `stop;` (the SDR program stops) ends an open over and says so
//!   ([`Halt::SdrStopped`]); a `trx:<trx>,false` after the echo and before our stop is the
//!   server ending our over itself ([`Halt::ServerEnded`]).
//! - *Escalation*: an unkey the server has not confirmed [`Config::unkey_deadline_ms`] after our
//!   first stop, while it is still heard from, sends the stops again, reports
//!   [`Event::UnkeyUnconfirmed`] (the operator must be told the radio may still be transmitting),
//!   and closes the session ([`End::UnkeyUnconfirmed`]). A server silent since our stop is a dead
//!   link instead: the stale close ends it, with the same belief handed on.
//!
//! **Stops are never gated**. [`Session::stop`] sends any [`Stop`] and
//! [`Session::end_ours`], rigctld's `T 0`, sends `trx:<trx>,false;` (with `tune:<trx>,false;`
//! first when a tune of ours may be up), in every phase but the closed one: before `ready;`, in
//! the middle of a key, while closing. On a server that has no notion of an owner (ExpertSDR3 by
//! the spec) `trx:<trx>,false;` also ends another program's over: that is the safe direction.
//!
//! **Every close sends the stops first**: a close on purpose, the escalation, a stale
//! link, a server that never said `ready;`, a message the WebSocket layer refused. A close on
//! purpose with a key of ours on the air waits up to [`Config::close_wait_ms`] for the server to
//! report the unkey before the close frame. After the close frame the driver reads on, dropping
//! what comes, until the server's own close frame or [`CLOSE_DRAIN`]: a socket let go with the
//! server's frames unread ends in a reset rather than a FIN, and a server that writes before it
//! reads, or whose system drops unread data on a reset, would never read the stops and the close
//! frame sent just before. A link that has already dropped carries nothing more; its belief is
//! handed on instead.
//!
//! The session itself never starts a transmission: none of its reactions to text, frames, time or
//! teardown is a key.
//!
//! Nexus's own design, not a port; the shape is the native Flex client's (`crate::flex::session`).

use std::collections::{HashMap, VecDeque};
use std::io::{self, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tungstenite::error::ProtocolError;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Bytes, Message, WebSocket};

use super::admission::{self, Facts, Refusal, TxStart, RX_FRESH_MS};
use super::dialect::{Evidence, Preference};
use super::frame::{Block, BlockKind, Chrono, Frame, Tally, MAX_PAYLOAD};
use super::model::Model;
use super::wire::{
    self, Call, ClientText, Command, Hz, Notice, Query, Setting, Source, Stop, StreamSetting,
    WireError,
};
use super::{Channel, Dialect, SMeter, Trx, EXPERT_SDR3};

/// How long a server has for its init burst, up to `ready;`.
pub const READY_TIMEOUT_MS: u64 = 5_000;

/// How often the session pings.
pub const PING_INTERVAL_MS: u64 = 1_000;

/// Silence this long while an over of ours is open ends it.
pub const KEYED_SILENCE_MS: u64 = 1_500;

/// Silence this long ends the session.
pub const STALE_MS: u64 = 5_000;

/// How long after a key's write the server has to echo it. WSJT-X waits 1 s
/// (`TCITransceiver.cpp:1092`); AetherSDR echoes only once the Flex transmits and refuses after
/// 1.25 s, so this allows a little more.
pub const KEY_ECHO_MS: u64 = 1_500;

/// How long after a key that takes Nexus's audio the first TX_CHRONO may take (it
/// starts at 300 ms; the bench sets it).
pub const CHRONO_WINDOW_MS: u64 = 300;

/// How long after our first stop the server has to report the unkey before the escalation.
pub const UNKEY_DEADLINE_MS: u64 = 2_000;

/// How long a close on purpose waits for the unkey's report while a key of ours is on the air.
pub const CLOSE_WAIT_MS: u64 = 1_000;

/// The shortest time between two reads of the same unreadable parameter.
pub const REREAD_MS: u64 = 1_000;

/// The interval asked of a server that pushes its meters.
pub const METER_INTERVAL_MS: u32 = 200;

/// How often a server whose S-meter is polled is asked.
pub const METER_POLL_MS: u64 = 500;

/// The driver's thread. It catches its own panic and sends the stops after it
/// ([`crate::on_a_transmit_driver`]).
pub const DRIVER_THREAD: &str = "tci-session";

/// The longest one read of the driver lasts before it looks at requests and time again, however
/// the server's bytes trickle in: a frame left partway is read on the next time. A stop waits at
/// most this long behind a read, and so do the windows and deadlines of [`Session::poll`].
pub const READ_POLL: Duration = Duration::from_millis(20);

/// How long the TCP connect may take. Bounded so that nothing waiting on it (ultimately the radio
/// loop, which unkeys) is held by an unreachable address.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// How long the WebSocket handshake may take, all of it: a server that trickles its answer is cut
/// off here, not at a timeout on each read.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);

/// How long one message may take to write, all of it, before the connection counts as gone: a
/// server that reads it a few bytes at a time is cut off here, not at a timeout on each write of
/// the socket.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a close reads on after its close frame, waiting for the server's own, before it lets
/// the socket go.
pub const CLOSE_DRAIN: Duration = Duration::from_millis(200);

/// The largest WebSocket message read. The codecs refuse anything over 64 KiB, counted, and the
/// session goes on; a message over this ends the session ([`End::ProtocolError`]).
pub const MAX_WS_MESSAGE: usize = 1 << 20;

/// How long a spot click in the legacy spelling (`clicked_on_spot`, which names no transceiver)
/// waits for the same click in the receiver's spelling (`rx_clicked_on_spot`, which names the
/// transceiver and the channel). A server that sends both sends them one after the other for one
/// click (AetherSDR's `TciServer::broadcastSpotClicked`), and the receiver's spelling says whose
/// panorama it was. A legacy click with no such twin by then is reported on its own.
pub const CLICK_PAIR_MS: u64 = 250;

/// The most clicks held for their twin at once, in each spelling: a burst of clicks from a server
/// is not kept without bound.
const CLICKS_HELD: usize = 8;

/// How the session is set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// The server's transceiver this radio is (the profile's `tci_trx`).
    pub trx: Trx,
    /// The operator's dialect setting (the profile's `tci_dialect`).
    pub preference: Preference,
    /// Nexus's audio over TCI, both ways (the profile's `tci_audio`): `audio_start:<trx>;` at
    /// `ready;`, and keys that take Nexus's audio. Off, only microphone keys and Tune are offered.
    pub audio: bool,
    /// A key an earlier session of ours left possibly on the air: this session sends its stops
    /// first after `ready;` and holds the belief until the server reports the unkey.
    pub carried: Option<OurKey>,
    /// [`READY_TIMEOUT_MS`].
    pub ready_timeout_ms: u64,
    /// [`PING_INTERVAL_MS`].
    pub ping_interval_ms: u64,
    /// [`KEYED_SILENCE_MS`].
    pub keyed_silence_ms: u64,
    /// [`STALE_MS`].
    pub stale_ms: u64,
    /// [`KEY_ECHO_MS`].
    pub key_echo_ms: u64,
    /// [`CHRONO_WINDOW_MS`].
    pub chrono_window_ms: u64,
    /// [`UNKEY_DEADLINE_MS`].
    pub unkey_deadline_ms: u64,
    /// [`CLOSE_WAIT_MS`].
    pub close_wait_ms: u64,
}

impl Config {
    /// The production settings for transceiver `trx`, receive audio on.
    pub fn new(trx: Trx) -> Config {
        Config {
            trx,
            preference: Preference::Auto,
            audio: true,
            carried: None,
            ready_timeout_ms: READY_TIMEOUT_MS,
            ping_interval_ms: PING_INTERVAL_MS,
            keyed_silence_ms: KEYED_SILENCE_MS,
            stale_ms: STALE_MS,
            key_echo_ms: KEY_ECHO_MS,
            chrono_window_ms: CHRONO_WINDOW_MS,
            unkey_deadline_ms: UNKEY_DEADLINE_MS,
            close_wait_ms: CLOSE_WAIT_MS,
        }
    }
}

/// A key of ours: the transceiver it keys and what it started. A session that ends with one
/// possibly on the air hands it to the next ([`Config::carried`],
/// [`super::reconnect::Step::StillKeyed`]), which stops it first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OurKey {
    pub trx: Trx,
    pub kind: TxStart,
}

/// Where the session writes. Text goes out only as [`ClientText`], which only the codec makes, so
/// no raw string and no server notice can reach the server from here.
pub trait Outbox {
    /// One text message. `false`: the transport failed.
    fn text(&mut self, text: &ClientText) -> bool;
    /// One binary message.
    fn binary(&mut self, frame: Vec<u8>) -> bool;
    /// A WebSocket ping.
    fn ping(&mut self) -> bool;
    /// The close frame.
    fn close(&mut self);
}

/// The audio and IQ a session carries, for whoever owns them (the receive path and the transmit
/// tee; the panadapter). Called on the session's own thread, between reads: each call
/// must be short.
pub trait Streams: Send {
    /// A receive audio block for this session's transceiver.
    fn receive(&mut self, block: &Block<'_>) {
        let _ = block;
    }

    /// An IQ block for this session's transceiver.
    fn iq(&mut self, block: &Block<'_>) {
        let _ = block;
    }

    /// Transmit audio for an over of ours, once per TX_CHRONO: write up to `out.len()` mono samples
    /// at `rate` Hz (full scale ±1.0) and return how many were written. The rest goes out as
    /// silence, on every channel the server asked for. Called only while an over of ours that
    /// takes Nexus's audio is open; never for another program's over.
    fn transmit(&mut self, rate: u32, out: &mut [f32]) -> usize {
        let _ = (rate, out);
        0
    }
}

/// [`Streams`] that take nothing and give silence.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoStreams;

impl Streams for NoStreams {}

/// Where an RF panadapter takes a session's IQ ([`Connection::set_iq_tap`]), beside the owner's
/// [`Streams`]. Called on the session's own thread, between reads, for each IQ block of this
/// session's transceiver, with the receiver's centre frequency (`dds`) as the server last reported
/// it before the block, if it has: each call must be short.
pub trait IqTap: Send + Sync {
    fn iq(&self, block: &Block<'_>, centre: Option<Hz>);
}

/// Where the session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Connected; the init burst is arriving.
    AwaitReady,
    /// `ready;` came: CAT, streams and keys.
    Ready,
    /// The stops are out; waiting for the unkey's report before the close frame.
    Closing,
    Closed,
}

/// How a session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// Closed on purpose.
    Closed,
    /// No `ready;` within the ready timeout.
    NoReady,
    /// The connection ended: the server closed it, it dropped, or a write failed.
    Lost,
    /// Nothing heard within the stale time: a half-open socket.
    Stale,
    /// The WebSocket layer refused what the server sent (a message over [`MAX_WS_MESSAGE`], a
    /// broken frame, text that is not UTF-8).
    ProtocolError(String),
    /// The server did not report our unkey in time; the stops went out again before the close.
    UnkeyUnconfirmed,
}

/// Why the session ended an over of ours by itself, before anyone asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Halt {
    /// No TX_CHRONO within the window after a key that takes Nexus's audio: the server is not
    /// taking it.
    NoChrono,
    /// The server did not echo the key within the window.
    NoEcho,
    /// The SDR program stopped (`stop;`).
    SdrStopped,
    /// Nothing heard from the server while the over was open.
    Silent,
    /// The server reported the transmitter off before our stop: it ended the over itself (the
    /// SDR's own operator, its own timeout).
    ServerEnded,
    /// The server says the over takes a source other than Nexus's audio.
    WrongSource(Source),
    /// Nexus's audio stream stopped under the over (`audio_stop`, or a report of it that could not
    /// be read): the SDR would take its microphone.
    AudioStopped,
}

/// What the session reports, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// `ready;` came: the dialect is pinned. The protocol line and device as sent, if any.
    Ready {
        dialect: Dialect,
        program: Option<String>,
        version: Option<String>,
        device: Option<String>,
    },
    /// The server echoed our key: it transmits for us.
    KeyConfirmed(TxStart),
    /// The session ended an over of ours before anyone asked, and sent its stops (none for
    /// [`Halt::ServerEnded`]). The engine halts its transmission and says why.
    Halted(Halt),
    /// The server reported our unkey: nothing of ours is on the air.
    Unkeyed,
    /// The server did not report our unkey in time: the stops went out again and the session is
    /// closing. The operator must be told the radio may still be transmitting.
    UnkeyUnconfirmed,
    /// Another program, or the SDR's own operator, keyed the transmitter (`on`) or let it go.
    ForeignKey { on: bool },
    /// The SDR program started (`start;`) or stopped (`stop;`).
    Running(bool),
    /// The SDR's operator clicked a spot on its panorama: select or tune only, never transmit.
    /// Reported once per click on this session's transceiver, whichever spellings the server sends
    /// it in ([`CLICK_PAIR_MS`]); never for a click the server says was on another transceiver's
    /// panorama. `channel`: the receive channel the click names (A or B), or `None` from a server
    /// that named none (the legacy spelling alone).
    SpotClicked {
        call: Call,
        hz: Hz,
        channel: Option<Channel>,
    },
    /// The session ended. `carried`: a key of ours that may still be on the air. Nothing follows.
    Closed { end: End, carried: Option<OurKey> },
}

/// Why a command was not sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError {
    /// Not ready yet, or closing.
    NotReady,
    /// The connection is closed.
    Closed,
    /// It would stop or reshape Nexus's audio stream while a key of ours that takes that audio
    /// may be up: the SDR would take its microphone.
    AudioInUse,
}

/// What a stop did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopOutcome {
    /// These stops went out, in order.
    Sent(Vec<Stop>),
    /// The connection is closed; nothing can be written.
    NotConnected,
}

/// How long the server took, for the bench.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Timing {
    /// From our last key's write to its echo.
    pub echo_ms: Option<u64>,
    /// From our last key's write to its first TX_CHRONO.
    pub first_chrono_ms: Option<u64>,
}

/// A copy of the session's state, for whoever drives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub phase: Phase,
    pub trx: Trx,
    /// [`EXPERT_SDR3`], the spec, until `ready;` pins the server's.
    pub dialect: Dialect,
    pub model: Model,
    pub evidence: Evidence,
    /// A key of ours, from its write until the server reports it ended. Kept when the session
    /// closes: a link lost while keyed is still keyed.
    pub keyed: Option<OurKey>,
    /// The server has echoed it.
    pub key_confirmed: bool,
    /// It is open: no stop of ours has gone out for it yet.
    pub over_open: bool,
    /// The server reports a transmission that is not ours.
    pub foreign: bool,
    /// A receive block for our transceiver arrived within [`RX_FRESH_MS`].
    pub rx_audio_fresh: bool,
    pub drive_ceiling: Option<u32>,
    pub tune_drive_ceiling: Option<u32>,
    /// Frames read and refused.
    pub frames: Tally,
    /// Text commands refused, and the last refusal.
    pub text_refused: u64,
    pub last_text_error: Option<WireError>,
    /// TX_CHRONO that arrived with no over of ours to answer: answered with nothing.
    pub chronos_not_ours: u64,
    /// TX_CHRONO of ours asking for more than one frame can carry: dropped, unanswered.
    pub chronos_refused: u64,
    pub timing: Timing,
}

impl Snapshot {
    /// Whether a key of ours may be on the air.
    pub fn ours_on_air(&self) -> bool {
        self.keyed.is_some()
    }
}

/// A key of ours, from its write until the server reports it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Keyed {
    trx: Trx,
    kind: TxStart,
    /// When it was written; `None` for a belief carried from an earlier session.
    sent_ms: Option<u64>,
    echoed: bool,
    chrono_seen: bool,
    /// When our first stop for it went out.
    stop_ms: Option<u64>,
}

/// The SDR operator's spot clicks, each reported once. A server that sends a click in both
/// spellings sends them back to back, in either order; the receiver's spelling decides whose
/// panorama it was, so its legacy twin is never reported for it. A legacy click is held
/// [`CLICK_PAIR_MS`] for its twin, then reported on its own ([`Clicks::due`]).
#[derive(Debug, Default)]
struct Clicks {
    /// Legacy clicks waiting for their twin: the call, the frequency, when.
    held: VecDeque<(Call, Hz, u64)>,
    /// Receiver-spelled clicks a legacy twin may still follow: the call, the frequency, when.
    answered: VecDeque<(Call, Hz, u64)>,
}

impl Clicks {
    /// A click in the legacy spelling: dropped if its receiver-spelled twin came first, held for
    /// that twin otherwise.
    fn legacy(&mut self, call: Call, hz: Hz, now: u64) {
        self.answered
            .retain(|(_, _, at)| now.saturating_sub(*at) < CLICK_PAIR_MS);
        if let Some(i) = self
            .answered
            .iter()
            .position(|(c, h, _)| *c == call && *h == hz)
        {
            self.answered.remove(i);
            return;
        }
        if self.held.len() >= CLICKS_HELD {
            self.held.pop_front();
        }
        self.held.push_back((call, hz, now));
    }

    /// A click in the receiver's spelling: it settles its legacy twin, held or still to come, and
    /// is reported when it is on `ours`.
    fn receiver(
        &mut self,
        trx: Trx,
        channel: Channel,
        call: Call,
        hz: Hz,
        ours: Trx,
        now: u64,
    ) -> Option<Event> {
        match self
            .held
            .iter()
            .position(|(c, h, _)| *c == call && *h == hz)
        {
            Some(i) => {
                self.held.remove(i);
            }
            None => {
                if self.answered.len() >= CLICKS_HELD {
                    self.answered.pop_front();
                }
                self.answered.push_back((call.clone(), hz, now));
            }
        }
        (trx == ours).then_some(Event::SpotClicked {
            call,
            hz,
            channel: Some(channel),
        })
    }

    /// The legacy clicks whose twin has not come within [`CLICK_PAIR_MS`], oldest first.
    fn due(&mut self, now: u64) -> Vec<Event> {
        let mut due = Vec::new();
        while self
            .held
            .front()
            .is_some_and(|(_, _, at)| now.saturating_sub(*at) >= CLICK_PAIR_MS)
        {
            if let Some((call, hz, _)) = self.held.pop_front() {
                due.push(Event::SpotClicked {
                    call,
                    hz,
                    channel: None,
                });
            }
        }
        due
    }
}

/// The protocol core of one connection.
pub struct Session {
    config: Config,
    phase: Phase,
    model: Model,
    evidence: Evidence,
    dialect: Dialect,
    keyed: Option<Keyed>,
    foreign: bool,
    drive_ceiling: Option<u32>,
    tune_drive_ceiling: Option<u32>,
    ready_deadline_ms: u64,
    last_heard_ms: u64,
    last_ping_ms: u64,
    last_meter_poll_ms: Option<u64>,
    rx_audio_ms: Option<u64>,
    reread_ms: HashMap<Query, u64>,
    frames: Tally,
    text_refused: u64,
    last_text_error: Option<WireError>,
    chronos_not_ours: u64,
    chronos_refused: u64,
    timing: Timing,
    /// The SDR operator's spot clicks, each reported once.
    clicks: Clicks,
    /// Why the session is closing, and when the close frame goes regardless.
    closing: Option<(End, u64)>,
    events: VecDeque<Event>,
    now_ms: u64,
    /// The panadapter's tap on this transceiver's IQ ([`IqTap`]).
    iq_tap: Option<Arc<dyn IqTap>>,
    /// `iq_start:<trx>;` for this transceiver went out and no `iq_stop` after it: the close stops it.
    iq_started: bool,
}

impl Session {
    /// A session for a WebSocket that has just opened.
    pub fn new(config: Config, now_ms: u64) -> Session {
        Session {
            config,
            phase: Phase::AwaitReady,
            model: Model::new(),
            evidence: Evidence::default(),
            dialect: EXPERT_SDR3,
            keyed: config.carried.map(|c| Keyed {
                trx: c.trx,
                kind: c.kind,
                sent_ms: None,
                echoed: false,
                chrono_seen: false,
                stop_ms: None,
            }),
            foreign: false,
            drive_ceiling: None,
            tune_drive_ceiling: None,
            ready_deadline_ms: now_ms.saturating_add(config.ready_timeout_ms),
            last_heard_ms: now_ms,
            last_ping_ms: now_ms,
            last_meter_poll_ms: None,
            rx_audio_ms: None,
            reread_ms: HashMap::new(),
            frames: Tally::default(),
            text_refused: 0,
            last_text_error: None,
            chronos_not_ours: 0,
            chronos_refused: 0,
            timing: Timing::default(),
            clicks: Clicks::default(),
            closing: None,
            events: VecDeque::new(),
            now_ms,
            iq_tap: None,
            iq_started: false,
        }
    }

    /// Hand this transceiver's IQ blocks to `tap` from the next block on, or stop handing them.
    pub fn set_iq_tap(&mut self, tap: Option<Arc<dyn IqTap>>) {
        self.iq_tap = tap;
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn is_closed(&self) -> bool {
        self.phase == Phase::Closed
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    /// Whether a key of ours may be on the air.
    pub fn ours_on_air(&self) -> bool {
        self.keyed.is_some()
    }

    /// Whether an over of ours that takes Nexus's audio is open (keyed, no stop sent).
    pub fn audio_over_open(&self) -> bool {
        self.keyed
            .is_some_and(|k| k.kind == TxStart::Audio && k.stop_ms.is_none())
    }

    fn over_open(&self) -> bool {
        self.keyed.is_some_and(|k| k.stop_ms.is_none())
    }

    fn rx_audio_fresh(&self, now: u64) -> bool {
        self.rx_audio_ms
            .is_some_and(|at| now.saturating_sub(at) <= RX_FRESH_MS)
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot_with(self.model.clone())
    }

    /// [`Self::snapshot`] around a copy of the model the caller already has: the driver's publish
    /// hands back the one it published while the model has not changed.
    fn snapshot_with(&self, model: Model) -> Snapshot {
        Snapshot {
            phase: self.phase,
            trx: self.config.trx,
            dialect: self.dialect,
            model,
            evidence: self.evidence.clone(),
            keyed: self.keyed.map(|k| OurKey {
                trx: k.trx,
                kind: k.kind,
            }),
            key_confirmed: self.keyed.is_some_and(|k| k.echoed),
            over_open: self.over_open(),
            foreign: self.foreign,
            rx_audio_fresh: self.rx_audio_fresh(self.now_ms),
            drive_ceiling: self.drive_ceiling,
            tune_drive_ceiling: self.tune_drive_ceiling,
            frames: self.frames,
            text_refused: self.text_refused,
            last_text_error: self.last_text_error.clone(),
            chronos_not_ours: self.chronos_not_ours,
            chronos_refused: self.chronos_refused,
            timing: self.timing,
        }
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        self.events.drain(..).collect()
    }

    fn facts(&self, now: u64) -> Facts {
        Facts {
            trx: self.config.trx,
            dialect: self.dialect,
            ready: self.phase == Phase::Ready,
            keyed: self.keyed.is_some(),
            audio: self.config.audio,
            rx_audio_fresh: self.rx_audio_fresh(now),
            drive_ceiling: self.drive_ceiling,
            tune_drive_ceiling: self.tune_drive_ceiling,
        }
    }

    /// The admission `start` would get now, without keying: for a caller that explains why
    /// transmit is not offered.
    pub fn admits(&self, start: TxStart, now: u64) -> Result<(), Refusal> {
        admission::admit(&self.model, &self.facts(now), start).map(|_| ())
    }

    // ── Writing ──────────────────────────────────────────────────────────────────────────────

    /// Write one command; a failed write ends the session ([`End::Lost`]).
    fn write(&mut self, out: &mut dyn Outbox, text: &ClientText, now: u64) -> bool {
        if out.text(text) {
            true
        } else {
            self.transport_closed(now);
            false
        }
    }

    /// Send a command that cannot key the transmitter: CAT writes and reads, stream control,
    /// spots, meters ([`Self::may_send`] says which, and when). A drive or tune drive written for
    /// our transceiver becomes the ceiling a key's readback is held to ([`super::admission`]).
    pub fn send(
        &mut self,
        out: &mut dyn Outbox,
        command: &Command,
        now: u64,
    ) -> Result<(), SendError> {
        match self.phase {
            Phase::Ready => {}
            Phase::AwaitReady | Phase::Closing => return Err(SendError::NotReady),
            Phase::Closed => return Err(SendError::Closed),
        }
        self.may_send(command)?;
        match command {
            Command::Set(Setting::Drive { trx, level })
                if trx.is_none_or(|t| t == self.config.trx) =>
            {
                self.drive_ceiling = Some(*level);
            }
            Command::Set(Setting::TuneDrive { trx, level }) if *trx == self.config.trx => {
                self.tune_drive_ceiling = Some(*level);
            }
            _ => {}
        }
        if !self.write(out, &command.render(), now) {
            return Err(SendError::Closed);
        }
        match command {
            Command::Stream(StreamSetting::IqStart(trx)) if *trx == self.config.trx => {
                self.iq_started = true;
            }
            Command::Stream(StreamSetting::IqStop(trx)) if *trx == self.config.trx => {
                self.iq_started = false;
            }
            _ => {}
        }
        Ok(())
    }

    /// What [`Self::send`] lets out, named variant by variant, so a command added to [`Command`]
    /// does not compile until it is placed here. Nothing a [`Command`] renders keys the
    /// transmitter: a key, a tune, a CW macro or message, CW terminal mode and a key-down are
    /// [`super::wire::Start`]s, rendered only through admission ([`Self::key`]). What is refused is
    /// the one way a command changes what an over carries: stopping or reshaping Nexus's audio
    /// stream while a key of ours that takes it may be up, which would give the SDR its microphone
    /// (TCI 2.0 §4.2, TRX).
    fn may_send(&self, command: &Command) -> Result<(), SendError> {
        let audio_key = self.keyed.filter(|k| k.kind == TxStart::Audio);
        match command {
            Command::RadioStart
            | Command::RadioStop
            | Command::Set(_)
            | Command::Read(_)
            | Command::Spot { .. }
            | Command::SpotDelete { .. }
            | Command::SpotClear
            | Command::RxSensorsEnable { .. }
            | Command::TxSensorsEnable { .. }
            | Command::CwMacrosSpeedUp(_)
            | Command::CwMacrosSpeedDown(_)
            | Command::SetInFocus => Ok(()),
            Command::Stream(stream) => match stream {
                StreamSetting::AudioStart(_)
                | StreamSetting::IqStart(_)
                | StreamSetting::IqStop(_)
                | StreamSetting::LineOutStart(_)
                | StreamSetting::LineOutStop(_)
                | StreamSetting::IqSampleRate(_) => Ok(()),
                StreamSetting::AudioStop(trx) => match audio_key {
                    Some(k) if k.trx == *trx => Err(SendError::AudioInUse),
                    _ => Ok(()),
                },
                StreamSetting::AudioSampleRate(_)
                | StreamSetting::AudioSampleType(_)
                | StreamSetting::AudioChannels(_)
                | StreamSetting::AudioSamples(_)
                | StreamSetting::TxAudioBuffering(_) => match audio_key {
                    Some(_) => Err(SendError::AudioInUse),
                    None => Ok(()),
                },
            },
        }
    }

    /// Key the transmitter. Admission decides; the keyed belief is set before the write.
    pub fn key(&mut self, out: &mut dyn Outbox, start: TxStart, now: u64) -> Result<(), Refusal> {
        self.now_ms = now;
        let admitted = admission::admit(&self.model, &self.facts(now), start)?;
        let text = admitted
            .render(&self.dialect)
            .map_err(|_| Refusal::NoSourceArgument {
                dialect: self.dialect.name,
            })?;
        self.keyed = Some(Keyed {
            trx: self.config.trx,
            kind: start,
            sent_ms: Some(now),
            echoed: false,
            chrono_seen: false,
            stop_ms: None,
        });
        self.timing = Timing::default();
        self.write(out, &text, now);
        Ok(())
    }

    /// The stops that end everything of ours, in order: for a key of ours, a tune's own stop
    /// first, then `trx:<its trx>,false;`; and `trx:<trx>,false;` for this session's transceiver
    /// always.
    fn our_stops(&self) -> Vec<Stop> {
        let mut stops = Vec::new();
        if let Some(k) = self.keyed {
            if k.kind == TxStart::Tune {
                stops.push(Stop::Tune(k.trx));
            }
            stops.push(Stop::Trx(k.trx));
        }
        if !stops.contains(&Stop::Trx(self.config.trx)) {
            stops.push(Stop::Trx(self.config.trx));
        }
        stops
    }

    /// Send a stop. Never gated: it goes out in every phase but the closed one, whatever the
    /// session believes. A stop that ends our key starts its unkey deadline.
    pub fn stop(&mut self, out: &mut dyn Outbox, stop: Stop, now: u64) -> StopOutcome {
        if self.phase == Phase::Closed {
            return StopOutcome::NotConnected;
        }
        let ends_ours = self.keyed.is_some_and(|k| match k.kind {
            TxStart::Audio | TxStart::Microphone => stop == Stop::Trx(k.trx),
            TxStart::Tune => stop == Stop::Tune(k.trx) || stop == Stop::Trx(k.trx),
        });
        if ends_ours {
            if let Some(k) = self.keyed.as_mut() {
                k.stop_ms.get_or_insert(now);
            }
        }
        if self.write(out, &stop.render(), now) {
            StopOutcome::Sent(vec![stop])
        } else {
            StopOutcome::NotConnected
        }
    }

    /// End everything of ours: rigctld's `T 0`. Never gated: `trx:<trx>,false;` goes out whatever
    /// the session believes, with the tune's stop first when a tune of ours may be up.
    pub fn end_ours(&mut self, out: &mut dyn Outbox, now: u64) -> StopOutcome {
        let mut sent = Vec::new();
        for stop in self.our_stops() {
            match self.stop(out, stop, now) {
                StopOutcome::Sent(mut s) => sent.append(&mut s),
                StopOutcome::NotConnected => return StopOutcome::NotConnected,
            }
        }
        StopOutcome::Sent(sent)
    }

    /// End an open over of ours by the session's own decision, and say why.
    fn halt(&mut self, out: &mut dyn Outbox, why: Halt, now: u64) {
        if !self.over_open() {
            return;
        }
        self.events.push_back(Event::Halted(why));
        let _ = self.end_ours(out, now);
    }

    // ── Reading ──────────────────────────────────────────────────────────────────────────────

    /// Something arrived that is not text or a frame (a pong, a ping).
    pub fn heard(&mut self, now: u64) {
        self.now_ms = now;
        self.last_heard_ms = now;
    }

    /// One text message from the server: every command in it, in order.
    pub fn on_text(&mut self, out: &mut dyn Outbox, text: &str, now: u64) {
        self.heard(now);
        if self.phase == Phase::Closed {
            return;
        }
        for raw in wire::split(text) {
            match raw {
                Ok(raw) => match Notice::parse(&raw) {
                    Ok(notice) => self.on_notice(out, notice, now),
                    Err(e) => self.refused(e, Some(raw.args()), text),
                },
                Err(e) => self.refused(e, None, text),
            }
            if self.phase == Phase::Closed {
                return;
            }
            self.check_audio(out, now);
        }
    }

    /// A command from the server that could not be read, with its arguments when the codec split
    /// them: counted, and what it names is unknown ([`Model::refused`]). A command cut off at the
    /// message's end is such a report as much as a malformed one.
    fn refused(&mut self, e: WireError, args: Option<&[&str]>, text: &str) {
        self.text_refused = self.text_refused.saturating_add(1);
        let name = match &e {
            WireError::Unterminated => cut_name(text),
            _ => e.command(),
        };
        if let Some(name) = name {
            self.model.refused(&name.to_ascii_lowercase(), args);
        }
        self.last_text_error = Some(e);
    }

    /// Our own open over of Nexus's audio, keyed on this connection, whose stream the server no
    /// longer reports running (`audio_stop`, or a report of it that could not be read): ended at
    /// once, since the SDR would take its microphone (this holds for the whole over).
    fn check_audio(&mut self, out: &mut dyn Outbox, now: u64) {
        let lost = self.phase == Phase::Ready
            && self.keyed.is_some_and(|k| {
                k.kind == TxStart::Audio
                    && k.sent_ms.is_some()
                    && k.stop_ms.is_none()
                    && !self.model.trx(k.trx).audio.is_true()
            });
        if lost {
            self.halt(out, Halt::AudioStopped, now);
        }
    }

    fn on_notice(&mut self, out: &mut dyn Outbox, notice: Notice, now: u64) {
        self.evidence.notice(&notice);
        self.model.apply(&notice);
        let ours = self.config.trx;
        match notice {
            Notice::Ready => {
                if self.phase == Phase::AwaitReady {
                    self.on_ready(out, now);
                }
            }
            Notice::Trx { trx, on, source } => self.on_trx(out, trx, on, source, now),
            Notice::Tune { trx, on } => self.on_tune(out, trx, on, now),
            // The run state in the burst is the model's; a change after `ready;` is news.
            Notice::RadioStart if self.phase == Phase::Ready => {
                self.events.push_back(Event::Running(true));
            }
            Notice::RadioStop if self.phase == Phase::Ready => {
                self.events.push_back(Event::Running(false));
                self.halt(out, Halt::SdrStopped, now);
            }
            Notice::ClickedOnSpot { call, hz } => self.clicks.legacy(call, hz, now),
            Notice::RxClickedOnSpot {
                trx,
                channel,
                call,
                hz,
            } => {
                let click = self.clicks.receiver(trx, channel, call, hz, ours, now);
                self.events.extend(click);
            }
            _ => {}
        }
    }

    fn on_ready(&mut self, out: &mut dyn Outbox, now: u64) {
        self.phase = Phase::Ready;
        self.dialect = self.evidence.pin(self.config.preference);
        // The stops an earlier session left owing, before anything else, whatever the burst
        // said: a stop costs nothing on an idle transmitter.
        if let Some(owed) = self.config.carried {
            let mut stops = Vec::new();
            if owed.kind == TxStart::Tune {
                stops.push(Stop::Tune(owed.trx));
            }
            stops.push(Stop::Trx(owed.trx));
            for stop in stops {
                if let StopOutcome::NotConnected = self.stop(out, stop, now) {
                    return;
                }
            }
        }
        let trx = self.config.trx;
        let mut setup = Vec::new();
        if self.config.audio {
            setup.push(Command::Stream(StreamSetting::AudioStart(trx)));
        }
        if matches!(self.dialect.smeter, SMeter::Channel | SMeter::Receiver) {
            setup.push(Command::RxSensorsEnable {
                on: true,
                interval_ms: Some(METER_INTERVAL_MS),
            });
            setup.push(Command::TxSensorsEnable {
                on: true,
                interval_ms: Some(METER_INTERVAL_MS),
            });
        }
        for command in setup {
            if !self.write(out, &command.render(), now) {
                return;
            }
        }
        self.events.push_back(Event::Ready {
            dialect: self.dialect,
            program: self.evidence.program.clone(),
            version: self.evidence.version.clone(),
            device: self.model.device.known().map(|d| d.as_str().to_string()),
        });
    }

    /// The server's `trx`.
    fn on_trx(
        &mut self,
        out: &mut dyn Outbox,
        trx: Trx,
        on: bool,
        source: Option<Source>,
        now: u64,
    ) {
        let Some(k) = self
            .keyed
            .filter(|k| k.kind != TxStart::Tune && k.trx == trx)
        else {
            if trx == self.config.trx {
                self.foreign_report(on);
            }
            return;
        };
        if on {
            if k.sent_ms.is_some() && !k.echoed {
                if let Some(key) = self.keyed.as_mut() {
                    key.echoed = true;
                }
                self.timing.echo_ms = k.sent_ms.map(|sent| now.saturating_sub(sent));
                self.events.push_back(Event::KeyConfirmed(k.kind));
            }
            // Our own key, written on this connection, for Nexus's audio: the server says it took
            // something else.
            if k.kind == TxStart::Audio && k.sent_ms.is_some() {
                if let Some(source) = source.filter(|s| *s != Source::Tci) {
                    self.halt(out, Halt::WrongSource(source), now);
                }
            }
            return;
        }
        // Off: proof only when it cannot predate the key.
        let proof = k.echoed || k.stop_ms.is_some() || k.sent_ms.is_none();
        if proof {
            self.unkeyed(out, k.stop_ms.is_none() && k.sent_ms.is_some());
        }
    }

    /// The server's `tune`.
    fn on_tune(&mut self, out: &mut dyn Outbox, trx: Trx, on: bool, now: u64) {
        let Some(k) = self
            .keyed
            .filter(|k| k.kind == TxStart::Tune && k.trx == trx)
        else {
            return;
        };
        if on {
            if k.sent_ms.is_some() && !k.echoed {
                if let Some(key) = self.keyed.as_mut() {
                    key.echoed = true;
                }
                self.timing.echo_ms = k.sent_ms.map(|sent| now.saturating_sub(sent));
                self.events.push_back(Event::KeyConfirmed(TxStart::Tune));
            }
        } else if k.echoed || k.stop_ms.is_some() || k.sent_ms.is_none() {
            self.unkeyed(out, k.stop_ms.is_none() && k.sent_ms.is_some());
        }
    }

    /// The server proved our key ended.
    fn unkeyed(&mut self, out: &mut dyn Outbox, by_server: bool) {
        if by_server {
            self.events.push_back(Event::Halted(Halt::ServerEnded));
        }
        self.keyed = None;
        self.events.push_back(Event::Unkeyed);
        if self.phase == Phase::Closing {
            self.finish_close(out);
        }
    }

    /// A `trx` report while no over of ours is keyed: someone else's transmission, or its end.
    fn foreign_report(&mut self, on: bool) {
        if on != self.foreign {
            self.foreign = on;
            self.events.push_back(Event::ForeignKey { on });
        }
    }

    /// One binary message from the server.
    pub fn on_binary(
        &mut self,
        out: &mut dyn Outbox,
        streams: &mut dyn Streams,
        bytes: &[u8],
        now: u64,
    ) {
        self.heard(now);
        if self.phase == Phase::Closed {
            return;
        }
        let ours = self.config.trx;
        match self.frames.decode(bytes) {
            Some(Frame::Block(block)) if block.header.trx == ours => match block.header.kind {
                BlockKind::RxAudio => {
                    self.rx_audio_ms = Some(now);
                    self.evidence.block(&block);
                    streams.receive(&block);
                }
                BlockKind::Iq => {
                    if let Some(tap) = &self.iq_tap {
                        tap.iq(&block, self.model.trx(ours).dds.known().copied());
                    }
                    streams.iq(&block);
                }
                BlockKind::TxAudio | BlockKind::LineOut => {}
            },
            Some(Frame::Chrono(chrono)) => self.on_chrono(out, streams, chrono, now),
            Some(Frame::Block(_)) | None => {}
        }
    }

    /// A TX_CHRONO: answered only for a key of ours that takes Nexus's audio, with exactly the
    /// frames it asks for.
    fn on_chrono(
        &mut self,
        out: &mut dyn Outbox,
        streams: &mut dyn Streams,
        chrono: Chrono,
        now: u64,
    ) {
        let Some(k) = self
            .keyed
            .filter(|k| k.kind == TxStart::Audio && k.sent_ms.is_some() && k.trx == chrono.trx)
        else {
            self.chronos_not_ours = self.chronos_not_ours.saturating_add(1);
            return;
        };
        let meaning = self.dialect.chrono_length;
        let frames = chrono.frames(meaning);
        let channels = chrono.channels.count();
        // The decoder refuses a `length` past MAX_CHRONO_LENGTH; whatever that bound is, nothing is
        // sized by the server's number until its answer is known to fit one frame. An answer that
        // cannot be sent is dropped and counted, never answered in part.
        let fits = frames
            .checked_mul(channels)
            .and_then(|values| values.checked_mul(chrono.format.bytes()))
            .is_some_and(|bytes| bytes <= MAX_PAYLOAD);
        if !fits {
            self.chronos_refused = self.chronos_refused.saturating_add(1);
            return;
        }
        if !k.chrono_seen {
            if let Some(key) = self.keyed.as_mut() {
                key.chrono_seen = true;
            }
            self.timing.first_chrono_ms = k.sent_ms.map(|sent| now.saturating_sub(sent));
        }
        let mut mono = vec![0.0f32; frames];
        if k.stop_ms.is_none() {
            streams.transmit(chrono.sample_rate, &mut mono);
        }
        let mut samples = Vec::with_capacity(frames * channels);
        for s in mono {
            samples.extend(std::iter::repeat_n(s, channels));
        }
        if let Ok(answer) = chrono.answer(meaning, &samples) {
            if !out.binary(answer) {
                self.transport_closed(now);
            }
        }
    }

    // ── Time ─────────────────────────────────────────────────────────────────────────────────

    /// Timers: the ready deadline, keepalive and silence, our key's windows and its unkey
    /// deadline, the meter poll, the reads of what is unreadable, the close.
    pub fn poll(&mut self, out: &mut dyn Outbox, now: u64) {
        self.now_ms = now;
        match self.phase {
            Phase::Closed => return,
            Phase::Closing => {
                if self.closing.as_ref().is_some_and(|(_, at)| now >= *at) {
                    self.finish_close(out);
                }
                return;
            }
            Phase::AwaitReady if now >= self.ready_deadline_ms => {
                self.close_with(out, End::NoReady, now);
                return;
            }
            Phase::AwaitReady | Phase::Ready => {}
        }
        // A legacy spot click whose receiver-spelled twin did not come is reported on its own.
        let clicks = self.clicks.due(now);
        self.events.extend(clicks);
        if now
            >= self
                .last_ping_ms
                .saturating_add(self.config.ping_interval_ms)
        {
            self.last_ping_ms = now;
            if !out.ping() {
                self.transport_closed(now);
                return;
            }
        }
        let silent_ms = now.saturating_sub(self.last_heard_ms);
        if silent_ms >= self.config.stale_ms {
            self.close_with(out, End::Stale, now);
            return;
        }
        if self.phase != Phase::Ready {
            return;
        }
        if silent_ms >= self.config.keyed_silence_ms {
            self.halt(out, Halt::Silent, now);
        }
        self.key_windows(out, now);
        if self.phase != Phase::Ready {
            return;
        }
        self.poll_meter(out, now);
        self.reread(out, now);
    }

    /// Our key's chrono and echo windows while it is open, and its unkey deadline once stopped.
    fn key_windows(&mut self, out: &mut dyn Outbox, now: u64) {
        let Some(k) = self.keyed else {
            return;
        };
        match (k.sent_ms, k.stop_ms) {
            (Some(sent), None) => {
                let since = now.saturating_sub(sent);
                if k.kind == TxStart::Audio
                    && !k.chrono_seen
                    && since >= self.config.chrono_window_ms
                {
                    self.halt(out, Halt::NoChrono, now);
                } else if !k.echoed && since >= self.config.key_echo_ms {
                    self.halt(out, Halt::NoEcho, now);
                }
            }
            // Only a server that is heard from after our stop has failed to confirm it; a silent
            // one is a dead link, which the stale close ends with the same belief handed on.
            (_, Some(stop))
                if now.saturating_sub(stop) >= self.config.unkey_deadline_ms
                    && self.last_heard_ms > stop =>
            {
                self.events.push_back(Event::UnkeyUnconfirmed);
                self.close_with(out, End::UnkeyUnconfirmed, now);
            }
            _ => {}
        }
    }

    fn poll_meter(&mut self, out: &mut dyn Outbox, now: u64) {
        if self.dialect.smeter != SMeter::Poll {
            return;
        }
        let due = self
            .last_meter_poll_ms
            .is_none_or(|at| now.saturating_sub(at) >= METER_POLL_MS);
        if due {
            self.last_meter_poll_ms = Some(now);
            let poll = Command::Read(Query::RxSmeter {
                trx: self.config.trx,
                channel: Channel::A,
            });
            self.write(out, &poll.render(), now);
        }
    }

    /// Ask again for what the server reported in a form that could not be read.
    fn reread(&mut self, out: &mut dyn Outbox, now: u64) {
        for query in self.model.rereads(self.config.trx, &self.dialect) {
            let due = self
                .reread_ms
                .get(&query)
                .is_none_or(|at| now.saturating_sub(*at) >= REREAD_MS);
            if due {
                self.reread_ms.insert(query.clone(), now);
                if !self.write(out, &Command::Read(query).render(), now) {
                    return;
                }
            }
        }
    }

    // ── Ending ───────────────────────────────────────────────────────────────────────────────

    /// Close on purpose: the stops, then `iq_stop` when this session started its IQ, then (with a
    /// key of ours on the air) a bounded wait for the server's report of the unkey, then the close
    /// frame and [`Event::Closed`].
    pub fn close(&mut self, out: &mut dyn Outbox, now: u64) {
        self.close_with(out, End::Closed, now);
    }

    /// The WebSocket layer refused what the server sent: the stops, then the close.
    pub fn protocol_error(&mut self, out: &mut dyn Outbox, why: String, now: u64) {
        self.close_with(out, End::ProtocolError(why), now);
    }

    fn close_with(&mut self, out: &mut dyn Outbox, end: End, now: u64) {
        if matches!(self.phase, Phase::Closing | Phase::Closed) {
            return;
        }
        let wait = end == End::Closed && self.keyed.is_some();
        self.phase = Phase::Closing;
        let at = if wait {
            now.saturating_add(self.config.close_wait_ms)
        } else {
            now
        };
        self.closing = Some((end, at));
        // The stops before every close; a failed write closes the session here.
        let _ = self.end_ours(out, now);
        // Then the IQ stream this session started, which keys nothing, so it goes after them.
        if self.phase == Phase::Closing && self.iq_started {
            let stop = Command::Stream(StreamSetting::IqStop(self.config.trx));
            if self.write(out, &stop.render(), now) {
                self.iq_started = false;
            }
        }
        if self.phase == Phase::Closing && !wait {
            self.finish_close(out);
        }
    }

    /// The close frame, then closed.
    fn finish_close(&mut self, out: &mut dyn Outbox) {
        if self.phase == Phase::Closed {
            return;
        }
        out.close();
        let end = self.closing.take().map_or(End::Closed, |(end, _)| end);
        self.finished(end);
    }

    /// The connection is gone: the server closed it, it dropped, or a write failed. Nothing more
    /// can be written. A close already under way keeps its reason. The keyed belief stays.
    pub fn transport_closed(&mut self, _now: u64) {
        if self.phase == Phase::Closed {
            return;
        }
        let end = self.closing.take().map_or(End::Lost, |(end, _)| end);
        self.finished(end);
    }

    fn finished(&mut self, end: End) {
        self.phase = Phase::Closed;
        let carried = self.keyed.map(|k| OurKey {
            trx: k.trx,
            kind: k.kind,
        });
        self.events.push_back(Event::Closed { end, carried });
    }
}

/// The name of the command a message ends in when it ends with no `;`: the text after the last
/// `;`, up to its `:`. `None` when that is not a command name.
fn cut_name(message: &str) -> Option<&str> {
    let tail = message.rsplit_once(';').map_or(message, |(_, tail)| tail);
    let name = tail.trim_start().split(':').next()?.trim_end();
    let is_name = !name.is_empty()
        && name.len() <= wire::MAX_NAME
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    is_name.then_some(name)
}

// ── The thread driver ────────────────────────────────────────────────────────────────────────

/// Why a connection could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// No TCP connection: refused, timed out, unreachable.
    Unreachable(String),
    /// The address answered, but the WebSocket handshake failed: not a TCI server, or it refused.
    Handshake(String),
    /// The session's thread could not be started.
    Spawn(String),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::Unreachable(why) => write!(f, "nothing answered: {why}"),
            ConnectError::Handshake(why) => write!(f, "not a TCI server: {why}"),
            ConnectError::Spawn(why) => write!(f, "could not start: {why}"),
        }
    }
}

impl std::error::Error for ConnectError {}

enum Request {
    Send(Command, Sender<Result<(), SendError>>),
    Key(TxStart, Sender<Result<(), Refusal>>),
    Stop(Stop, Sender<StopOutcome>),
    EndOurs(Sender<StopOutcome>),
    IqTap(Option<Arc<dyn IqTap>>, Sender<()>),
    Close,
}

struct Shared {
    snapshot: Mutex<Snapshot>,
    changed: Condvar,
    /// The snapshot's `ours_on_air`, readable without copying it: kept once the session closes.
    on_air: AtomicBool,
    /// An over of ours that takes Nexus's audio is open, and the session is not closed.
    audio_over: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The session's writes through the WebSocket: each message whole within [`WRITE_TIMEOUT`], or a
/// failed write.
struct WsOut<'a>(&'a mut WebSocket<Socket>);

impl WsOut<'_> {
    fn send(&mut self, message: Message) -> bool {
        self.0.get_mut().deadline = Instant::now() + WRITE_TIMEOUT;
        self.0.send(message).is_ok()
    }
}

impl Outbox for WsOut<'_> {
    fn text(&mut self, text: &ClientText) -> bool {
        let sent = self.send(Message::text(text.as_str()));
        #[cfg(test)]
        self.0.get_mut().spring(text.as_str());
        sent
    }

    fn binary(&mut self, frame: Vec<u8>) -> bool {
        self.send(Message::binary(frame))
    }

    fn ping(&mut self) -> bool {
        self.send(Message::Ping(Bytes::new()))
    }

    fn close(&mut self) {
        self.0.get_mut().deadline = Instant::now() + WRITE_TIMEOUT;
        let _ = self.0.close(None);
        let _ = self.0.flush();
    }
}

/// The socket under the session's WebSocket. Each read and each write gets only the time left
/// before `deadline`, and none starts once that has run out. A timeout on each would bound one
/// read or write and no more: the WebSocket layer reads on until a handshake answer or a frame is
/// whole, and writes on until a message is out, so a server sending a byte just inside each
/// read's timeout could hold the handshake for minutes, and one read of the driver for hours (a
/// frame may be up to [`MAX_WS_MESSAGE`]), and a server reading a few bytes inside each write's
/// timeout could hold one message as long, while stops, windows and deadlines waited. `deadline`
/// is the handshake's, then the end of each read of the driver ([`READ_POLL`]; an answer the
/// WebSocket layer writes inside a read, such as a pong, goes in the read's time), of each message
/// the session writes ([`WRITE_TIMEOUT`]), and of the close's drain ([`CLOSE_DRAIN`]). A frame a
/// read leaves partway is kept by the WebSocket layer, and the next read goes on with it. The
/// probe ([`super::probe`]) and the TCI server's handshake read the same way.
struct Socket {
    tcp: TcpStream,
    deadline: Instant,
    #[cfg(test)]
    trap: Option<Trap>,
}

/// Under test, a fault the driver's own writes trip: the text that starts with `on` goes out whole,
/// then the driver panics on its way back, before the step publishes what it did. After that every
/// write fails if `later_fail` (the link went with the driver).
#[cfg(test)]
#[derive(Clone, Copy)]
struct Trap {
    on: &'static str,
    later_fail: bool,
    fired: bool,
}

#[cfg(test)]
thread_local! {
    /// The trap the next [`Connection::connect`] on this thread sets in its socket.
    static NEXT_TRAP: std::cell::Cell<Option<Trap>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
impl Socket {
    fn spring(&mut self, text: &str) {
        if let Some(trap) = self
            .trap
            .as_mut()
            .filter(|t| !t.fired && text.starts_with(t.on))
        {
            trap.fired = true;
            panic!("the session's driver dies right after writing {text:?}");
        }
    }

    fn link_gone(&self) -> io::Result<()> {
        if self.trap.is_some_and(|t| t.fired && t.later_fail) {
            return Err(io::Error::new(
                ErrorKind::BrokenPipe,
                "the link went with the driver",
            ));
        }
        Ok(())
    }
}

impl Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(ErrorKind::TimedOut, "the read's time is up"));
        }
        self.tcp.set_read_timeout(Some(left))?;
        self.tcp.read(buf)
    }
}

impl Write for Socket {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        #[cfg(test)]
        self.link_gone()?;
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(
                ErrorKind::TimedOut,
                "the write's time is up",
            ));
        }
        self.tcp.set_write_timeout(Some(left))?;
        self.tcp.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        #[cfg(test)]
        self.link_gone()?;
        self.tcp.flush()
    }
}

/// A [`Session`] running on its own thread over one WebSocket. `Sync`, so one connection serves
/// several callers at once: each request waits on its own answer, and a stop waits only for the
/// read under way ([`READ_POLL`]) and for what the driver writes before it, each message within
/// [`WRITE_TIMEOUT`] or the session ends.
pub struct Connection {
    requests: Sender<Request>,
    /// Behind a lock only so the connection is `Sync`; one reader drains it.
    events: Mutex<Receiver<Event>>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Connection {
    /// Open TCP to the server at `addr`, make the WebSocket handshake, and start the session.
    pub fn connect(
        addr: SocketAddr,
        config: Config,
        streams: Box<dyn Streams>,
    ) -> Result<Connection, ConnectError> {
        let unreachable = |e: io::Error| ConnectError::Unreachable(e.to_string());
        let tcp = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(unreachable)?;
        tcp.set_nodelay(true).map_err(unreachable)?;
        let limits = WebSocketConfig::default()
            .max_message_size(Some(MAX_WS_MESSAGE))
            .max_frame_size(Some(MAX_WS_MESSAGE));
        let url = format!("ws://{addr}/");
        // The whole handshake inside its deadline, our request as well as the server's answer,
        // however that answer trickles in.
        let socket = Socket {
            tcp,
            deadline: Instant::now() + HANDSHAKE_TIMEOUT,
            #[cfg(test)]
            trap: NEXT_TRAP.take(),
        };
        let (ws, _) = tungstenite::client::client_with_config(url.as_str(), socket, Some(limits))
            .map_err(|e| ConnectError::Handshake(e.to_string()))?;
        Connection::spawn(ws, config, streams).map_err(|e| ConnectError::Spawn(e.to_string()))
    }

    /// Start the session over an open WebSocket: the driver sets each read's deadline.
    fn spawn(
        ws: WebSocket<Socket>,
        config: Config,
        streams: Box<dyn Streams>,
    ) -> io::Result<Connection> {
        let (requests, request_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        let epoch = Instant::now();
        let session = Session::new(config, 0);
        let shared = Arc::new(Shared {
            snapshot: Mutex::new(session.snapshot()),
            changed: Condvar::new(),
            on_air: AtomicBool::new(session.ours_on_air()),
            audio_over: AtomicBool::new(false),
        });
        let thread = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name(DRIVER_THREAD.into())
                .spawn(move || {
                    let (mut ws, mut session) = (ws, session);
                    let driven = std::panic::catch_unwind(AssertUnwindSafe(|| {
                        drive(
                            &mut ws,
                            &mut session,
                            streams,
                            epoch,
                            request_rx,
                            &event_tx,
                            &shared,
                        );
                    }));
                    if driven.is_err() {
                        died(&mut ws, &mut session, epoch, &shared);
                    }
                })?
        };
        Ok(Connection {
            requests,
            events: Mutex::new(events),
            shared,
            thread: Some(thread),
        })
    }

    /// Send a command that cannot key ([`Session::send`]).
    pub fn send(&self, command: Command) -> Result<(), SendError> {
        let (tx, rx) = mpsc::channel();
        self.requests
            .send(Request::Send(command, tx))
            .map_err(|_| SendError::Closed)?;
        rx.recv().unwrap_or(Err(SendError::Closed))
    }

    /// Key the transmitter (admission decides). `Err(None)` when the session is gone.
    pub fn key(&self, start: TxStart) -> Result<(), Option<Refusal>> {
        let (tx, rx) = mpsc::channel();
        self.requests
            .send(Request::Key(start, tx))
            .map_err(|_| None)?;
        rx.recv().map_err(|_| None)?.map_err(Some)
    }

    /// Send a stop. Never gated.
    pub fn stop(&self, stop: Stop) -> StopOutcome {
        let (tx, rx) = mpsc::channel();
        if self.requests.send(Request::Stop(stop, tx)).is_err() {
            return StopOutcome::NotConnected;
        }
        rx.recv().unwrap_or(StopOutcome::NotConnected)
    }

    /// End everything of ours ([`Session::end_ours`]): rigctld's `T 0`. Never gated.
    pub fn end_ours(&self) -> StopOutcome {
        let (tx, rx) = mpsc::channel();
        if self.requests.send(Request::EndOurs(tx)).is_err() {
            return StopOutcome::NotConnected;
        }
        rx.recv().unwrap_or(StopOutcome::NotConnected)
    }

    /// Hand this transceiver's IQ blocks to `tap` from the next block on, or stop handing them
    /// ([`Session::set_iq_tap`]). `false` when the session has ended.
    pub fn set_iq_tap(&self, tap: Option<Arc<dyn IqTap>>) -> bool {
        let (tx, rx) = mpsc::channel();
        self.requests.send(Request::IqTap(tap, tx)).is_ok() && rx.recv().is_ok()
    }

    /// Whether a key of ours may be on the air, as the last published state says, without copying
    /// it. Kept once the session has closed: a session that ended keyed still says so.
    pub fn ours_on_air(&self) -> bool {
        self.shared.on_air.load(Ordering::Acquire)
    }

    /// Whether an over of ours that takes Nexus's audio is open on a live session: the transmit
    /// path asks before it queues audio.
    pub fn audio_over_open(&self) -> bool {
        self.shared.audio_over.load(Ordering::Acquire)
    }

    /// The next event, waiting up to `timeout`.
    pub fn next_event(&self, timeout: Duration) -> Option<Event> {
        self.recv_event(timeout).ok()
    }

    /// The next event, waiting up to `timeout`, or `Disconnected` once the driver has stopped and
    /// every event it sent has been taken: from then on the channel answers at once, so a reader
    /// that waits on it in a loop must stop there, whatever the session's phase reads.
    pub fn recv_event(&self, timeout: Duration) -> Result<Event, mpsc::RecvTimeoutError> {
        lock(&self.events).recv_timeout(timeout)
    }

    /// The session's state as of its last step. Each step's state is published before its answers
    /// and events go out, so whoever acts on an answer or an event reads the state it reports, or
    /// a later one.
    pub fn snapshot(&self) -> Snapshot {
        lock(&self.shared.snapshot).clone()
    }

    /// Read the state in place, without the copy [`Self::snapshot`] makes.
    pub fn read<T>(&self, f: impl FnOnce(&Snapshot) -> T) -> T {
        f(&lock(&self.shared.snapshot))
    }

    /// Wait until `done` holds for the state, or `timeout` passes. Returns whether it held.
    pub fn wait_until(&self, timeout: Duration, done: impl Fn(&Snapshot) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        let mut snapshot = lock(&self.shared.snapshot);
        loop {
            if done(&snapshot) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            snapshot = self
                .shared
                .changed
                .wait_timeout(snapshot, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Close on purpose, waiting for the teardown (stops first) to finish, the server's answer to
    /// the close frame included (at most [`CLOSE_DRAIN`]).
    pub fn close(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let _ = self.requests.send(Request::Close);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn retryable(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
    )
}

fn drive(
    ws: &mut WebSocket<Socket>,
    session: &mut Session,
    mut streams: Box<dyn Streams>,
    epoch: Instant,
    requests: Receiver<Request>,
    events: &Sender<Event>,
    shared: &Shared,
) {
    let now = || u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
    loop {
        // Requests first, so a stop never waits behind more than one read.
        loop {
            let mut out = WsOut(ws);
            match requests.try_recv() {
                Ok(Request::Send(command, reply)) => {
                    let sent = session.send(&mut out, &command, now());
                    answer(session, shared, reply, sent);
                }
                Ok(Request::Key(start, reply)) => {
                    let keyed = session.key(&mut out, start, now());
                    answer(session, shared, reply, keyed);
                }
                Ok(Request::Stop(stop, reply)) => {
                    let stopped = session.stop(&mut out, stop, now());
                    answer(session, shared, reply, stopped);
                }
                Ok(Request::EndOurs(reply)) => {
                    let stopped = session.end_ours(&mut out, now());
                    answer(session, shared, reply, stopped);
                }
                Ok(Request::IqTap(tap, reply)) => {
                    session.set_iq_tap(tap);
                    let _ = reply.send(());
                }
                Ok(Request::Close) | Err(TryRecvError::Disconnected) => {
                    session.close(&mut out, now());
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        if !session.is_closed() {
            // One read, however the server's bytes trickle in: a frame it leaves partway is read
            // on the next time round, after requests and time.
            ws.get_mut().deadline = Instant::now() + READ_POLL;
            let read = ws.read();
            let mut out = WsOut(ws);
            match read {
                Ok(Message::Text(text)) => session.on_text(&mut out, text.as_str(), now()),
                Ok(Message::Binary(bytes)) => {
                    session.on_binary(&mut out, streams.as_mut(), &bytes, now());
                }
                Ok(Message::Close(_)) => session.transport_closed(now()),
                Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {
                    session.heard(now());
                }
                Err(tungstenite::Error::Io(e)) if retryable(&e) => {}
                // The TCP connection ended with no close frame: a lost link, not a broken server.
                Err(tungstenite::Error::Protocol(ProtocolError::ResetWithoutClosingHandshake)) => {
                    session.transport_closed(now());
                }
                Err(
                    e @ (tungstenite::Error::Capacity(_)
                    | tungstenite::Error::Protocol(_)
                    | tungstenite::Error::Utf8(_)),
                ) => session.protocol_error(&mut out, e.to_string(), now()),
                Err(_) => session.transport_closed(now()),
            }
            session.poll(&mut out, now());
        }
        publish(session, shared);
        for event in session.take_events() {
            let _ = events.send(event);
        }
        if session.is_closed() {
            break;
        }
    }
    // Closed: a request from here is answered at once as closed, not after the drain.
    drop(requests);
    drain(ws);
}

/// After the close frame: read on, dropping what comes, until the server's own close frame (it
/// sends nothing after it, RFC 6455 §5.5.1), the end of the connection, or [`CLOSE_DRAIN`]. A
/// socket let go with the server's frames still unread ends in a reset rather than a FIN, and a
/// server that writes before it reads, or whose system drops unread data on a reset, then never
/// reads what went out just before: the stops, `iq_stop`, the close frame. A link already gone
/// ends it at the first read. What is still to write (a message cut off at its deadline, the
/// answer to the server's own close frame) goes first, in the same time.
fn drain(ws: &mut WebSocket<Socket>) {
    let until = Instant::now() + CLOSE_DRAIN;
    ws.get_mut().deadline = until;
    let _ = ws.flush();
    loop {
        match ws.read() {
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if retryable(&e) && Instant::now() < until => {}
            Err(_) => return,
        }
    }
}

/// The driver panicked: a bug in the session, never a state it is meant to reach. TX SAFETY: the
/// stops a close sends first go out first here too ([`Session::end_ours`], best effort: the session
/// may be part way through a step), then the session is published closed, so every reader sees it
/// gone (the radio loop's liveness check, a wait on the state, the transmit path) rather than open
/// forever, and the event channel, dropped with the thread, ends the watcher. Then the close frame
/// and the drain a close ends with ([`drain`]), so the server reads the stops before the socket
/// goes: one let go with the server's frames still unread ends in a reset, which may drop them.
/// Nothing else of the close: it waits on a server a dead driver cannot read.
///
/// The key published is the last published one, or else the session's own, never less: a step
/// publishes only once it is done, so a driver that died inside the step that wrote a key left the
/// state from before that key published, and the watcher's alarm, the radio loop (`keyed`) and the
/// daemon's teardown, which hands the key on to the next session to stop first, read from it.
/// Nothing is published as off the air: no report from the server can prove the unkey now, and the
/// stops below may never have reached it.
fn died(ws: &mut WebSocket<Socket>, session: &mut Session, epoch: Instant, shared: &Shared) {
    let now = u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
    let keyed = session.keyed.map(|k| OurKey {
        trx: k.trx,
        kind: k.kind,
    });
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let mut out = WsOut(ws);
        let _ = session.end_ours(&mut out, now);
        out.close();
    }));
    let mut published = lock(&shared.snapshot);
    published.phase = Phase::Closed;
    published.keyed = published.keyed.or(keyed);
    if published.ours_on_air() {
        shared.on_air.store(true, Ordering::Release);
    }
    drop(published);
    shared.audio_over.store(false, Ordering::Release);
    shared.changed.notify_all();
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| drain(ws)));
}

/// Store the session's state for [`Connection::snapshot`] and wake [`Connection::wait_until`].
/// The driver does this before it announces anything a step did (an answer, an event), so a reader
/// woken by an announcement reads the state it reports, or a later one.
fn publish(session: &Session, shared: &Shared) {
    let mut published = lock(&shared.snapshot);
    // Built before anything published is touched: a panic part way leaves the last state whole.
    let mut snapshot = session.snapshot_with(Model::default());
    // The model is everything the server has reported, and the driver publishes every turn, fifty
    // a second on a quiet link: the copy already published stands while it is equal, and only a
    // changed model is copied again.
    snapshot.model = if published.model == session.model {
        std::mem::take(&mut published.model)
    } else {
        session.model.clone()
    };
    shared
        .on_air
        .store(snapshot.ours_on_air(), Ordering::Release);
    shared.audio_over.store(
        session.audio_over_open() && !session.is_closed(),
        Ordering::Release,
    );
    *published = snapshot;
    drop(published);
    shared.changed.notify_all();
}

/// Answer a request, once the state it left is published.
fn answer<T>(session: &Session, shared: &Shared, reply: Sender<T>, value: T) {
    publish(session, shared);
    let _ = reply.send(value);
}

#[cfg(test)]
mod sim_tests;
#[cfg(test)]
mod socket_tests;
#[cfg(test)]
mod tests;
