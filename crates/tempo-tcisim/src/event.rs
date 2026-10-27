//! The simulator's log: everything it received, decided and sent, in time order.
//!
//! Times are measured from the simulator's start on the monotonic clock. A message or frame the
//! simulator sends is stamped just before its bytes are written, on the thread that writes them,
//! and every entry is placed in the log by its time; so anything a client sends in answer to a
//! message is logged after that message (tempo-flexsim's lesson).

use std::time::Duration;

use crate::frame::{Fit, Header};
use crate::wire::Flaw;

/// Who keyed, or asked to unkey, the transmitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyer {
    /// A client, by connection number.
    Client(usize),
    /// The SDR's own operator, or another program the simulator stands in for
    /// ([`crate::Simulator::operator`], [`crate::Fault::ForeignKey`]).
    Operator,
}

/// A microphone input (spec §4.2, TRX).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mic {
    /// The microphone selected in the SDR program: a key that names no source.
    Selected,
    Mic1,
    Mic2,
    MicPc,
    Ecoder2,
}

/// What the transmitter sends while keyed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The keying client's TCI audio, pulled with TX_CHRONO.
    Tci,
    /// A microphone.
    Mic(Mic),
    /// The tune carrier.
    Tune,
    /// The SDR's CW keyer sending a macro or message.
    Cw,
    /// The `keyer` command's straight key.
    Key,
}

/// Why a key ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// `trx:<n>,false`, `tune:<n>,false`, `keyer:<n>,false` or `stop;` from this keyer.
    Asked(Keyer),
    /// The keying client's connection ended and the dialect unkeys on that.
    Left(usize),
    /// [`crate::Fault::ServerStop`].
    ServerStop,
    /// The CW text was done, or `cw_macros_stop` ended it.
    CwDone,
    /// Another key took the transmitter.
    Replaced,
}

/// Who ended a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closer {
    /// The client closed the WebSocket cleanly (a close frame).
    Client,
    /// The TCP connection ended with no close frame, or failed.
    Dropped,
    /// A fault, or the test, dropped it ([`crate::Fault::DisconnectMidOver`],
    /// [`crate::Simulator::drop_connection`]).
    Simulator,
    /// The simulator was dropped.
    Stopped,
}

/// Why a client command was not acted on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// Not a well-formed command.
    Malformed(Flaw),
    /// A name the simulator does not know.
    Unknown,
    /// The wrong number of arguments.
    Arity,
    /// A value that does not parse or is out of range (a transceiver that does not exist, a
    /// frequency outside `vfo_limits`, a mode not in `modulations_list` …).
    Value,
    /// A notice only a server sends (`tx_enable`, `rx_channel_sensors` …).
    ServerOnly,
    /// Known, but not modelled for this dialect (`rx_smeter` on a server that pushes, the line-out
    /// recorder …).
    Unsupported,
    /// A fault swallowed it ([`crate::Fault::NoAudioAck`]).
    Fault(&'static str),
}

/// Why the radio refused a key, a CW macro or a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// `receive_only:true`.
    ReceiveOnly,
    /// `tx_enable:<n>,false`.
    TxDisabled,
    /// The SDR is stopped.
    Stopped,
    /// Another client owns the transmitter (the dialect's [`crate::dialect::Ownership`]).
    NotOwner,
    /// The transmitter is keyed by something a CW macro cannot join.
    Busy,
    /// [`crate::Fault::StuckTrx`] ignored the unkey.
    Stuck,
    /// `audio_start` at a rate or sample type the dialect does not stream.
    Format,
    /// A `cw_msg` callsign correction after the callsign started.
    TooLate,
}

/// One entry in the log. `conn` numbers connections from 0 in the order their handshakes finished.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A client finished its WebSocket handshake.
    Connected { conn: usize },
    /// A TCP connection whose WebSocket handshake failed.
    Rejected { why: String },
    /// A text message from a client, whole.
    TextIn { conn: usize, text: String },
    /// A well-formed command from that message: as it arrived (without `;`), its name lower-cased,
    /// its arguments.
    Command {
        conn: usize,
        text: String,
        name: String,
        args: Vec<String>,
    },
    /// A command applied as the SDR's own operator ([`crate::Simulator::operator`]).
    Operator { text: String },
    /// A client command the radio did not act on.
    Ignored { conn: usize, text: String, why: Why },
    /// A binary frame from a client. `header` is `None` for one too short to have one. `fit` is how
    /// its payload fits its own header; `asked` how it fits the last TX_CHRONO sent to this client
    /// (`None` before the first). `ours` is whether this client held a `tci` key when it arrived.
    FrameIn {
        conn: usize,
        header: Option<Header>,
        bytes: usize,
        fit: Fit,
        asked: Option<Fit>,
        ours: bool,
    },
    /// A text message the simulator sent, whole.
    TextOut { conn: usize, text: String },
    /// A binary frame the simulator sent. `header` is `None` for one too short to have one (only
    /// [`crate::Fault::Garbage`] sends those).
    FrameOut {
        conn: usize,
        header: Option<Header>,
        bytes: usize,
    },
    /// The transmitter keyed.
    Keyed { trx: u32, by: Keyer, source: Source },
    /// The transmitter unkeyed. `by` held the key.
    Unkeyed { trx: u32, by: Keyer, cause: Cause },
    /// The keying client left and the radio stays keyed: the dialect does not unkey a lost client
    /// or a fault kept it.
    Orphaned { trx: u32, conn: usize },
    /// A `tci` key took the microphone because the client's audio stream was not running (spec
    /// §4.2): the hazard the client's audio rule guards against.
    MicFallback { conn: usize, trx: u32 },
    /// A key, CW macro, stream or unkey the radio refused. `conn` is `None` for the operator.
    Refused {
        conn: Option<usize>,
        text: String,
        why: Refusal,
    },
    /// [`crate::Fault::Hold200ms`] held this write back.
    Held { conn: usize, text: String },
    /// A `vfo` write moved a transceiver to another band; `restored` when
    /// [`crate::Fault::BandRestore`] then sent the band's settings.
    BandChanged {
        trx: u32,
        band: &'static str,
        restored: bool,
    },
    /// The CW keyer started text: its duration at the speed it started at, in ms.
    CwStarted { trx: u32, text: String, ms: u64 },
    /// The CW keyer finished its text, or was stopped.
    CwEnded { trx: u32, stopped: bool },
    /// The radio fell more than [`crate::radio::BEHIND`] behind a stream (its process starved of
    /// CPU) and gave up `frames` frames of `kind` ([`crate::frame::kind`]: receive audio, IQ or
    /// TX_CHRONO) for `conn` on transceiver `trx`, as an SDR program does; the stream went on from
    /// the next frame to come.
    Lost {
        conn: usize,
        trx: u32,
        kind: u32,
        frames: u64,
    },
    /// A fault acted.
    Fault {
        name: &'static str,
        conn: Option<usize>,
        detail: String,
    },
    /// A connection ended.
    Closed { conn: usize, by: Closer },
}

/// A logged event and when it happened, from the simulator's start.
#[derive(Debug, Clone, PartialEq)]
pub struct Logged {
    pub at: Duration,
    pub event: Event,
}
