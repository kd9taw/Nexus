//! The simulated radio: TCI's state and rules, with no sockets.
//!
//! [`Radio`] takes what clients send, and its own timers, and answers with effects: text and
//! frames for connections, timers to set, log entries, connections to drop or silence. The server
//! runs it under one lock and carries the effects out, so the model can be read, and tested, on
//! its own.
//!
//! **The server is a synchronizer** (spec §3.1, §3.5). A change any client makes, or the SDR's own
//! operator makes, is broadcast to every client, the sender included; a read is answered to the
//! asker alone. Per-client settings (the audio stream's format, streams, sensor pushes) are
//! answered to that client only. A value the radio does not accept is ignored and logged, never
//! echoed: an invalid command is ignored (spec §3.1).
//!
//! **What keys the transmitter**: `trx` (with its source, spec §4.2), `tune`, a CW macro or
//! message, and `keyer`. Who may key and unkey is the dialect's [`Ownership`]; where a key's audio
//! comes from is its [`KeySource`] and, for a `tci` key, whether the client's audio stream runs.
//! A `tci` key that takes TCI audio is pulled with TX_CHRONO, one header at a time at the rate
//! the audio is consumed (§3.4), from the moment the key is echoed.
//!
//! **A radio that runs late** (its process starved of CPU) keeps to its own clock, as an SDR
//! program does: a receive, IQ or TX_CHRONO frame more than [`BEHIND`] late is given up with every
//! frame due since, and the stream goes on from the next one to come, its samples still counted
//! from its start ([`Event::Lost`]). It never sends the whole backlog at once.
//!
//! **What is not modelled**: the line-out stream and recorder, `keyer`'s element timing, and the
//! 200 ms hold for PTT (the hold applies to parameters; a key follows ownership instead).

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::time::{Duration, Instant};

use crate::cw;
use crate::dialect::{
    AudioFormat, ChronoLength, ClickForm, Dialect, DriveForm, Ident, KeySource, Ownership,
    SensorForm, Spelling, SpotStore, TrxInit,
};
use crate::event::{Cause, Closer, Event, Keyer, Mic, Refusal, Source, Why};
use crate::fault::{self, Fault};
use crate::frame::{self, kind, Fit, Header, SampleType};
use crate::synth::{self, Receive, Timeline, Tuning};
use crate::wire::{self, flag, notice, Command};

/// A key the transmitter holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub by: Keyer,
    pub source: Source,
    /// Counts every key the radio took, from 1.
    pub id: u64,
    /// The keying client left and the radio kept the key.
    pub orphaned: bool,
}

/// One transceiver's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrxState {
    /// VFO A and B, Hz.
    pub vfo: [u64; 2],
    pub dds: u64,
    pub if_hz: [i64; 2],
    pub modulation: String,
    pub filter: (i64, i64),
    pub drive: u32,
    pub tune_drive: u32,
    pub split: bool,
    pub rit: bool,
    pub rit_offset: i64,
    pub xit: bool,
    pub xit_offset: i64,
    pub tx_enable: bool,
    /// What keys the transmitter, if anything.
    pub key: Option<Key>,
    /// The key has been echoed (a dialect may echo only once the radio transmits).
    pub echoed: bool,
}

impl TrxState {
    fn new(init: &TrxInit) -> TrxState {
        TrxState {
            vfo: init.vfo,
            dds: init.dds,
            if_hz: init.if_hz,
            modulation: init.modulation.clone(),
            filter: sideband_filter(init.filter, lower_sideband(&init.modulation)),
            drive: init.drive,
            tune_drive: init.tune_drive,
            split: init.split,
            rit: init.rit,
            rit_offset: init.rit_offset,
            xit: init.xit,
            xit_offset: init.xit_offset,
            tx_enable: init.tx_enable,
            key: None,
            echoed: false,
        }
    }

    /// Transmitting, other than the tune carrier: what `trx` reports.
    pub fn transmitting(&self) -> bool {
        self.key.is_some_and(|k| k.source != Source::Tune)
    }

    /// The tune carrier is up: what `tune` reports.
    pub fn tuning(&self) -> bool {
        self.key.is_some_and(|k| k.source == Source::Tune)
    }
}

/// A spot a client sent for the panorama.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spot {
    pub call: String,
    pub mode: String,
    pub hz: u64,
    pub argb: u32,
    pub text: String,
    /// The connection that sent it.
    pub by: usize,
    /// The colour the panorama draws it in, ARGB; `None`: the server's own default colour, which
    /// is what AetherSDR draws a colour that does not start with `#` in ([`SpotStore`]).
    pub drawn: Option<u32>,
}

/// A colour as AetherSDR's panorama draws one: `#AARRGGBB`, or `#RRGGBB` taken as opaque.
fn hash_colour(text: &str) -> Option<u32> {
    let hex = text.strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        8 => u32::from_str_radix(hex, 16).ok(),
        6 => u32::from_str_radix(hex, 16)
            .ok()
            .map(|rgb| 0xFF00_0000 | rgb),
        _ => None,
    }
}

/// What the radio's meters read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meters {
    /// The receive level, dBm.
    pub smeter_dbm: f64,
    /// The microphone level `tx_sensors` reports, dBm.
    pub mic_dbm: f64,
    /// Output at drive 100, W. The output at a lower drive is in proportion.
    pub max_power_w: f64,
    /// The SWR while transmitting.
    pub swr: f64,
}

impl Default for Meters {
    fn default() -> Self {
        Meters {
            smeter_dbm: -73.0,
            mic_dbm: -30.0,
            max_power_w: 100.0,
            swr: 1.2,
        }
    }
}

/// A running receive or IQ stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stream {
    id: u64,
    started: Instant,
    /// Frames (samples per channel) generated so far.
    frames: u64,
    stall_logged: bool,
}

impl Stream {
    fn new(id: u64, started: Instant) -> Stream {
        Stream {
            id,
            started,
            frames: 0,
            stall_logged: false,
        }
    }
}

/// What a TX_CHRONO asked a client for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ask {
    length: u32,
    channels: u32,
}

/// What one connection has asked of the radio.
#[derive(Debug, Clone, PartialEq)]
struct Client {
    audio: AudioFormat,
    iq_rate: u32,
    tx_buffering_ms: u32,
    audio_streams: BTreeMap<u32, Stream>,
    iq_streams: BTreeMap<u32, Stream>,
    rx_sensors: Option<Duration>,
    tx_sensors: Option<Duration>,
    rx_gen: u64,
    tx_gen: u64,
    /// Transmit audio the server enabled itself (piHPSDR's `tci` key).
    tx_audio: bool,
    asked: Option<Ask>,
}

/// A client's settings and streams, as [`Snapshot`] shows them.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientView {
    pub audio: AudioFormat,
    pub iq_rate: u32,
    pub tx_buffering_ms: u32,
    /// The transceivers whose receive audio this client is sent.
    pub audio_running: Vec<u32>,
    /// The transceivers whose IQ this client is sent.
    pub iq_running: Vec<u32>,
    pub rx_sensors: Option<Duration>,
    pub tx_sensors: Option<Duration>,
}

/// The radio's state at one moment ([`crate::Simulator::radio`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub trx: Vec<TrxState>,
    /// The SDR runs (`start;`) rather than stopped.
    pub running: bool,
    /// The client that owns the transmitter, under [`Ownership::FirstKeyer`].
    pub owner: Option<usize>,
    pub spots: Vec<Spot>,
    pub cw_speed: u32,
    pub cw_delay: u32,
    pub cw_terminal: bool,
    /// The CW keyer is sending text.
    pub cw_sending: bool,
    pub clients: BTreeMap<usize, ClientView>,
}

/// Where a message goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum To {
    All,
    One(usize),
}

/// Something the radio does later.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Action {
    RxAudio {
        conn: usize,
        trx: u32,
        id: u64,
    },
    Iq {
        conn: usize,
        trx: u32,
        id: u64,
    },
    Chrono {
        conn: usize,
        trx: u32,
        key: u64,
        n: u64,
        epoch: Instant,
    },
    RxSensors {
        conn: usize,
        gen: u64,
    },
    TxSensors {
        conn: usize,
        gen: u64,
    },
    /// [`Fault::TxSensorsLate`]: the transmit meters are pushed from now on.
    TxSensorsFrom,
    Echo {
        trx: u32,
        key: u64,
    },
    TailEnd {
        trx: u32,
        key: u64,
    },
    CwEmpty {
        job: u64,
    },
    CwCallsign {
        job: u64,
    },
    CwEnd {
        job: u64,
    },
    ForeignKey {
        on: bool,
    },
    ServerStop,
    Drop {
        conn: usize,
    },
    Silence {
        conn: usize,
    },
    Ping {
        conn: usize,
    },
}

/// What the radio asks the server to do.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Effect {
    Text {
        to: To,
        text: String,
    },
    Frame {
        to: usize,
        bytes: Vec<u8>,
    },
    Timer {
        at: Instant,
        action: Action,
    },
    Log(Event),
    /// Close this connection's TCP with no close frame.
    Drop(usize),
    /// Stop answering this connection, and stop reading it as commands.
    Silence(usize),
    /// A WebSocket ping.
    Ping(usize),
}

/// What a key request asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wanted {
    /// A TCI 2.0 key naming its source, or none (the selected microphone).
    Mic(Mic),
    /// `trx:<n>,true,tci`.
    Tci,
    /// A 1.x key: TCI audio when the client's stream runs. `asked_tci`: the client wrote `tci`.
    Running {
        asked_tci: bool,
    },
    Tune,
    Key,
}

/// Which stop a client sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Trx,
    Tune,
    Keyer,
}

/// A CW message (spec §3.2.2): the callsign is sent `times` times between prefix and suffix.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Message {
    prefix: String,
    call: String,
    times: usize,
    suffix: String,
}

impl Message {
    /// The callsign as keyed: repeated, space between.
    fn calls(&self) -> String {
        vec![self.call.as_str(); self.times.max(1)].join(" ")
    }
}

/// A CW transmission in progress.
#[derive(Debug, Clone, PartialEq)]
struct CwJob {
    id: u64,
    trx: u32,
    by: Keyer,
    /// A message, or `None` for a macro.
    message: Option<Message>,
    /// When its first element is on the air.
    start: Instant,
    schedule: cw::Schedule,
    /// Where the message's callsign starts and ends, ms from `start`.
    call_span: Option<(f64, f64)>,
}

/// The CW keyer.
#[derive(Debug, Clone, PartialEq, Default)]
struct CwKeyer {
    speed: u32,
    delay: u32,
    terminal: bool,
    job: Option<CwJob>,
    queue: VecDeque<(u32, Keyer, String)>,
}

/// The parameters the radio keeps without modelling them: their key arguments' count. A set has
/// more arguments than that, a read exactly that many.
const GENERIC: &[(&str, usize)] = &[
    ("volume", 0),
    ("mute", 0),
    ("mon_volume", 0),
    ("mon_enable", 0),
    ("rx_mute", 1),
    ("rx_volume", 2),
    ("rx_balance", 2),
    ("agc_mode", 1),
    ("agc_gain", 1),
    ("rx_nb_enable", 1),
    ("rx_nb_param", 1),
    ("rx_bin_enable", 1),
    ("rx_nr_enable", 1),
    ("rx_anc_enable", 1),
    ("rx_anf_enable", 1),
    ("rx_apf_enable", 1),
    ("rx_dse_enable", 1),
    ("rx_nf_enable", 1),
    ("lock", 1),
    ("sql_enable", 1),
    ("sql_level", 1),
    ("digl_offset", 0),
    ("digu_offset", 0),
    ("rx_channel_enable", 2),
    ("rx_enable", 1),
    ("cw_keyer_speed", 0),
];

/// Notices only a server sends: a client sending one is ignored.
const SERVER_ONLY: &[&str] = &[
    "tx_enable",
    "rx_channel_sensors",
    "rx_sensors",
    "tx_sensors",
    "tx_frequency",
    "clicked_on_spot",
    "rx_clicked_on_spot",
    "callsign_send",
    "cw_macros_empty",
    "app_focus",
    "tx_footswitch",
    "ready",
];

/// The amateur bands, for a band change (spec §3.5): name, lowest and highest Hz.
const BANDS: &[(&str, u64, u64)] = &[
    ("160m", 1_800_000, 2_000_000),
    ("80m", 3_500_000, 4_000_000),
    ("60m", 5_250_000, 5_450_000),
    ("40m", 7_000_000, 7_300_000),
    ("30m", 10_100_000, 10_150_000),
    ("20m", 14_000_000, 14_350_000),
    ("17m", 18_068_000, 18_168_000),
    ("15m", 21_000_000, 21_450_000),
    ("12m", 24_890_000, 24_990_000),
    ("10m", 28_000_000, 29_700_000),
    ("6m", 50_000_000, 54_000_000),
    ("2m", 144_000_000, 148_000_000),
];

/// The band `hz` is in, if any.
pub fn band(hz: u64) -> Option<&'static str> {
    BANDS
        .iter()
        .find(|(_, lo, hi)| (*lo..=*hi).contains(&hz))
        .map(|(name, _, _)| *name)
}

/// The modes whose audio is the lower sideband: their filter edges are negative.
fn lower_sideband(mode: &str) -> bool {
    ["lsb", "digl", "cwl"]
        .iter()
        .any(|m| m.eq_ignore_ascii_case(mode))
}

/// A filter mirrored to the side of the dial `lower` says.
fn sideband_filter(filter: (i64, i64), lower: bool) -> (i64, i64) {
    let (lo, hi) = filter;
    let below = hi <= 0;
    if below == lower {
        filter
    } else {
        (-hi, -lo)
    }
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn secs(frames: u64, rate: u32) -> Duration {
    Duration::from_secs_f64(frames as f64 / f64::from(rate.max(1)))
}

/// How late a stream's frame may go out before the radio gives it up. An SDR program that falls
/// behind sends what it holds late, all at once, and loses what is older than its buffer. The
/// length is this simulator's choice (UNVERIFIED for any real server): long enough that no
/// scheduling hiccup on a loaded machine loses a frame.
pub const BEHIND: Duration = Duration::from_millis(200);

/// The radio. See the module header.
pub(crate) struct Radio {
    dialect: Dialect,
    faults: Vec<Fault>,
    receive: Receive,
    meters: Meters,
    started: Instant,
    trx: Vec<TrxState>,
    running: bool,
    clients: BTreeMap<usize, Client>,
    owner: Option<usize>,
    spots: Vec<Spot>,
    /// Unmodelled state, in the order first seen: a parameter's key (`name:key-args`) and the
    /// notice that sets it, or no key for a line replayed as written (a recorded burst's line of a
    /// kind the simulator does not know).
    params: Vec<(Option<String>, String)>,
    holds: HashMap<String, (Keyer, Instant)>,
    keyer: CwKeyer,
    loopback: Timeline,
    tx_audio: BTreeMap<usize, VecDeque<f32>>,
    tx_audio_cap: usize,
    last_tx_frequency: Option<u64>,
    next_id: u64,
    /// One-shot faults that have fired, and per-key fault notes already logged.
    fired: BTreeSet<String>,
    now: Instant,
    fx: Vec<Effect>,
}

/// Spot list bound, so a client that never clears cannot grow the simulator without limit.
const MAX_SPOTS: usize = 10_000;

impl Radio {
    pub(crate) fn new(
        dialect: Dialect,
        faults: Vec<Fault>,
        receive: Receive,
        meters: Meters,
        tx_audio_cap: usize,
        started: Instant,
    ) -> Radio {
        let count = dialect.trx_count.max(1) as usize;
        let mut trx: Vec<TrxState> = (0..count)
            .map(|t| TrxState::new(&dialect.initial_trx(t)))
            .collect();
        if fault::has(&faults, |f| matches!(f, Fault::TxEnableFalse)) {
            for t in &mut trx {
                t.tx_enable = false;
            }
        }
        let keyer = CwKeyer {
            speed: dialect.initial.cw_speed,
            delay: dialect.initial.cw_delay,
            ..CwKeyer::default()
        };
        let mut radio = Radio {
            running: dialect.initial.running,
            loopback: Timeline::new(started, 48_000, 30),
            dialect,
            faults,
            receive,
            meters,
            started,
            trx,
            clients: BTreeMap::new(),
            owner: None,
            spots: Vec::new(),
            params: Vec::new(),
            holds: HashMap::new(),
            keyer,
            tx_audio: BTreeMap::new(),
            tx_audio_cap,
            last_tx_frequency: None,
            next_id: 0,
            fired: BTreeSet::new(),
            now: started,
            fx: Vec::new(),
        };
        // The unmodelled state lines a profile starts with: parameters it keeps and answers, or
        // lines it replays as written.
        for line in radio.dialect.initial.extra.clone() {
            let commands = wire::parse(&line).commands;
            match commands.first() {
                Some(c) if generic_keys(&c.name).is_some_and(|keys| c.args.len() > keys) => {
                    let keys = generic_keys(&c.name).unwrap_or(0);
                    radio.store_param(c, keys);
                }
                _ => radio.params.push((None, line)),
            }
        }
        radio.last_tx_frequency = Some(radio.tx_frequency_of(0));
        radio
    }

    /// The effects collected since the last call.
    pub(crate) fn take(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.fx)
    }

    // ----- What the server calls -----

    /// A client finished its handshake: its init burst, and the faults that act after it.
    pub(crate) fn connect(&mut self, conn: usize, now: Instant) {
        self.now = now;
        self.clients.insert(
            conn,
            Client {
                audio: self.dialect.audio,
                iq_rate: self.dialect.iq_rate,
                tx_buffering_ms: 50,
                audio_streams: BTreeMap::new(),
                iq_streams: BTreeMap::new(),
                rx_sensors: None,
                tx_sensors: None,
                rx_gen: 0,
                tx_gen: 0,
                tx_audio: false,
                asked: None,
            },
        );
        let lines = self.burst();
        if self.dialect.burst_in_one_message {
            self.send(To::One(conn), lines.concat());
        } else {
            for line in lines {
                self.send(To::One(conn), line);
            }
        }
        if self.has(|f| matches!(f, Fault::Garbage)) {
            let text = fault::garbage_text();
            let frames = fault::garbage_frames();
            self.log(Event::Fault {
                name: "Garbage",
                conn: Some(conn),
                detail: format!("{} text messages, {} frames", text.len(), frames.len()),
            });
            for t in text {
                self.send(To::One(conn), t);
            }
            for f in frames {
                self.fx.push(Effect::Frame { to: conn, bytes: f });
            }
        }
        if let Some((bytes, binary)) = self.find(|f| match f {
            Fault::Oversized { bytes, binary } => Some((*bytes, *binary)),
            _ => None,
        }) {
            self.log(Event::Fault {
                name: "Oversized",
                conn: Some(conn),
                detail: format!("{bytes} bytes, binary {binary}"),
            });
            if binary {
                let header = Header {
                    sample_rate: 48_000,
                    format: SampleType::Float32.code(),
                    length: u32::try_from(bytes / 4).unwrap_or(u32::MAX),
                    kind: kind::RX_AUDIO,
                    channels: 1,
                    ..Header::default()
                };
                let payload = vec![0u8; bytes / 4 * 4];
                self.fx.push(Effect::Frame {
                    to: conn,
                    bytes: header.frame(&payload),
                });
            } else {
                let head = "x_oversized:";
                let body = "X".repeat(bytes.saturating_sub(head.len() + 1));
                self.send(To::One(conn), format!("{head}{body};"));
            }
        }
        if let Some(after) = self.find(|f| match f {
            Fault::HalfOpen { after } => Some(*after),
            _ => None,
        }) {
            if self.once("HalfOpen") {
                self.timer(now + after, Action::Silence { conn });
            }
        }
        if let Some(after) = self.find(|f| match f {
            Fault::ForeignKey { after, .. } => Some(*after),
            _ => None,
        }) {
            if self.once("ForeignKey") {
                self.timer(now + after, Action::ForeignKey { on: true });
            }
        }
        if let Some(every) = self.dialect.ping_every {
            self.timer(now + every, Action::Ping { conn });
        }
    }

    /// A connection ended.
    pub(crate) fn disconnect(&mut self, conn: usize, by: Closer, now: Instant) {
        self.now = now;
        self.clients.remove(&conn);
        let kept_by_fault = self.has(|f| matches!(f, Fault::StuckTrx))
            || (by == Closer::Simulator
                && self.has(|f| {
                    matches!(
                        f,
                        Fault::DisconnectMidOver {
                            radio_stays_keyed: true,
                            ..
                        }
                    )
                }));
        let dialect_unkeys = match by {
            Closer::Client => self.dialect.unkey_on_close,
            Closer::Dropped | Closer::Simulator => self.dialect.unkey_on_drop,
            Closer::Stopped => false,
        };
        for t in 0..self.trx.len() {
            let Some(k) = self.trx[t].key else { continue };
            if k.by != Keyer::Client(conn) {
                continue;
            }
            if dialect_unkeys && !kept_by_fault {
                self.unkey_now(t, Cause::Left(conn), true);
            } else if let Some(key) = self.trx[t].key.as_mut() {
                key.orphaned = true;
                self.log(Event::Orphaned {
                    trx: t as u32,
                    conn,
                });
            }
        }
        if self.owner == Some(conn) {
            self.owner = None;
        }
    }

    /// A text message from a client.
    pub(crate) fn on_text(&mut self, conn: usize, text: &str, now: Instant) {
        self.now = now;
        let parsed = wire::parse(text);
        for flaw in parsed.flaws {
            let text = match &flaw {
                wire::Flaw::Unterminated(t)
                | wire::Flaw::BadName(t)
                | wire::Flaw::TooManyArgs(t)
                | wire::Flaw::ArgTooLong(t) => t.clone(),
                wire::Flaw::MessageTooLong(_) | wire::Flaw::TooManyCommands => String::new(),
            };
            self.log(Event::Ignored {
                conn,
                text,
                why: Why::Malformed(flaw),
            });
        }
        for cmd in parsed.commands {
            self.log(Event::Command {
                conn,
                text: cmd.text.clone(),
                name: cmd.name.clone(),
                args: cmd.args.clone(),
            });
            self.apply(Keyer::Client(conn), &cmd);
        }
    }

    /// Commands applied as the SDR's own operator.
    pub(crate) fn operator(&mut self, text: &str, now: Instant) {
        self.now = now;
        self.log(Event::Operator {
            text: text.to_string(),
        });
        for cmd in wire::parse(text).commands {
            self.apply(Keyer::Operator, &cmd);
        }
    }

    /// The SDR's operator clicks a spot on the panorama.
    pub(crate) fn click(&mut self, call: &str, hz: u64, now: Instant) {
        self.click_on(0, 0, call, hz, now);
    }

    /// The SDR's operator clicks a spot on transceiver `trx`'s panorama, in its receive channel
    /// `channel` (0 = A, 1 = B). The receiver-qualified spelling names both; the legacy one neither.
    pub(crate) fn click_on(&mut self, trx: u32, channel: u32, call: &str, hz: u64, now: Instant) {
        self.now = now;
        for form in self.dialect.spot_click.clone() {
            let line = match form {
                ClickForm::Legacy => notice("clicked_on_spot", [call.to_string(), hz.to_string()]),
                ClickForm::Receiver => notice(
                    "rx_clicked_on_spot",
                    [
                        trx.to_string(),
                        channel.to_string(),
                        call.to_string(),
                        hz.to_string(),
                    ],
                ),
            };
            self.broadcast(line);
        }
    }

    /// A binary frame from a client.
    pub(crate) fn on_binary(&mut self, conn: usize, bytes: &[u8], now: Instant) {
        self.now = now;
        let ours = self.trx.iter().any(|t| {
            t.key
                .is_some_and(|k| k.by == Keyer::Client(conn) && k.source == Source::Tci)
        });
        let asked = self.clients.get(&conn).and_then(|c| c.asked);
        let Some(h) = Header::decode(bytes) else {
            self.log(Event::FrameIn {
                conn,
                header: None,
                bytes: bytes.len(),
                fit: Fit::Neither,
                asked: asked.map(|_| Fit::Neither),
                ours,
            });
            return;
        };
        let payload = &bytes[frame::HEADER_LEN..];
        let format = SampleType::from_code(h.format);
        let values = format
            .filter(|f| payload.len().is_multiple_of(f.bytes()))
            .map(|f| payload.len() / f.bytes());
        let fit = values.map_or(Fit::Neither, |v| frame::fit(h.length, h.channels, v));
        let asked_fit =
            asked.map(|a| values.map_or(Fit::Neither, |v| frame::fit(a.length, a.channels, v)));
        self.log(Event::FrameIn {
            conn,
            header: Some(h),
            bytes: bytes.len(),
            fit,
            asked: asked_fit,
            ours,
        });
        if h.kind != kind::TX_AUDIO {
            return;
        }
        let Some(samples) = format.and_then(|f| frame::decode_samples(f, payload)) else {
            return;
        };
        let channels = match h.channels {
            1 | 2 => h.channels,
            _ => asked.map_or(2, |a| a.channels.max(1)),
        } as usize;
        let first: Vec<f32> = samples.iter().step_by(channels).copied().collect();
        let held = self.tx_audio.entry(conn).or_default();
        held.extend(first.iter().copied());
        while held.len() > self.tx_audio_cap {
            held.pop_front();
        }
        // Only audio that would be on the air comes back: a client's own `tci` key.
        if self.receive.loopback && ours {
            self.loopback.write(now, &first);
        }
    }

    /// A timer came due.
    pub(crate) fn on_action(&mut self, action: Action, now: Instant) {
        self.now = now;
        match action {
            Action::RxAudio { conn, trx, id } => self.rx_audio_tick(conn, trx, id),
            Action::Iq { conn, trx, id } => self.iq_tick(conn, trx, id),
            Action::Chrono {
                conn,
                trx,
                key,
                n,
                epoch,
            } => self.chrono_tick(conn, trx, key, n, epoch),
            Action::RxSensors { conn, gen } => self.rx_sensors_tick(conn, gen),
            Action::TxSensors { conn, gen } => self.tx_sensors_tick(conn, gen),
            Action::TxSensorsFrom => {
                self.fired.insert("TxSensorsLate:pushing".into());
            }
            Action::Echo { trx, key } => self.echo(trx as usize, key),
            Action::TailEnd { trx, key } => {
                if self.trx[trx as usize].key.is_some_and(|k| k.id == key) {
                    let by = self.trx[trx as usize].key.map_or(Keyer::Operator, |k| k.by);
                    self.unkey_now(trx as usize, Cause::Asked(by), true);
                }
            }
            Action::CwEmpty { job } => {
                if let Some(j) = self.keyer.job.as_ref().filter(|j| j.id == job) {
                    let to = match j.by {
                        Keyer::Client(c) => To::One(c),
                        Keyer::Operator => To::All,
                    };
                    self.send(to, "cw_macros_empty;".to_string());
                }
            }
            Action::CwCallsign { job } => {
                if let Some(j) = self.keyer.job.as_ref().filter(|j| j.id == job) {
                    if let Some(message) = &j.message {
                        let line = notice("callsign_send", [message.call.clone()]);
                        self.broadcast(line);
                    }
                }
            }
            Action::CwEnd { job } => self.cw_end(job),
            Action::ForeignKey { on } => self.foreign_key(on),
            Action::ServerStop => self.server_stop(),
            Action::Drop { conn } => {
                if self.clients.contains_key(&conn) {
                    self.log(Event::Fault {
                        name: "DisconnectMidOver",
                        conn: Some(conn),
                        detail: "the TCP connection is dropped while keyed".into(),
                    });
                    self.fx.push(Effect::Drop(conn));
                }
            }
            Action::Silence { conn } => {
                if self.clients.contains_key(&conn) {
                    self.log(Event::Fault {
                        name: "HalfOpen",
                        conn: Some(conn),
                        detail: "the server goes silent; the socket stays open".into(),
                    });
                    self.fx.push(Effect::Silence(conn));
                }
            }
            Action::Ping { conn } => {
                if self.clients.contains_key(&conn) {
                    self.fx.push(Effect::Ping(conn));
                    if let Some(every) = self.dialect.ping_every {
                        self.timer(now + every, Action::Ping { conn });
                    }
                }
            }
        }
    }

    /// The radio's state now.
    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot {
            trx: self.trx.clone(),
            running: self.running,
            owner: self.owner,
            spots: self.spots.clone(),
            cw_speed: self.keyer.speed,
            cw_delay: self.keyer.delay,
            cw_terminal: self.keyer.terminal,
            cw_sending: self.keyer.job.is_some(),
            clients: self
                .clients
                .iter()
                .map(|(id, c)| {
                    (
                        *id,
                        ClientView {
                            audio: c.audio,
                            iq_rate: c.iq_rate,
                            tx_buffering_ms: c.tx_buffering_ms,
                            audio_running: c.audio_streams.keys().copied().collect(),
                            iq_running: c.iq_streams.keys().copied().collect(),
                            rx_sensors: c.rx_sensors,
                            tx_sensors: c.tx_sensors,
                        },
                    )
                })
                .collect(),
        }
    }

    /// The transmit audio (channel 0) a connection has sent, the latest `tx_audio_cap` samples.
    pub(crate) fn tx_audio(&self, conn: usize) -> Vec<f32> {
        self.tx_audio
            .get(&conn)
            .map(|d| d.iter().copied().collect())
            .unwrap_or_default()
    }

    pub(crate) fn set_meters(&mut self, meters: Meters) {
        self.meters = meters;
    }

    // ----- Effects -----

    fn send(&mut self, to: To, text: String) {
        self.fx.push(Effect::Text { to, text });
    }

    fn broadcast(&mut self, text: String) {
        self.send(To::All, text);
    }

    /// Answer a read: to the asking client; the operator asks nothing.
    fn reply(&mut self, who: Keyer, text: String) {
        if let Keyer::Client(c) = who {
            self.send(To::One(c), text);
        }
    }

    fn timer(&mut self, at: Instant, action: Action) {
        self.fx.push(Effect::Timer { at, action });
    }

    fn log(&mut self, event: Event) {
        self.fx.push(Effect::Log(event));
    }

    fn ignore(&mut self, who: Keyer, cmd: &Command, why: Why) {
        if let Keyer::Client(conn) = who {
            self.log(Event::Ignored {
                conn,
                text: cmd.text.clone(),
                why,
            });
        }
    }

    fn refuse(&mut self, who: Keyer, text: &str, why: Refusal) {
        self.log(Event::Refused {
            conn: match who {
                Keyer::Client(c) => Some(c),
                Keyer::Operator => None,
            },
            text: text.to_string(),
            why,
        });
    }

    fn has(&self, pick: impl Fn(&Fault) -> bool) -> bool {
        fault::has(&self.faults, pick)
    }

    fn find<T>(&self, pick: impl Fn(&Fault) -> Option<T>) -> Option<T> {
        fault::find(&self.faults, pick)
    }

    /// `true` the first time `name` is asked for in this simulator's life.
    fn once(&mut self, name: &str) -> bool {
        self.fired.insert(name.to_string())
    }

    fn next(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn no_echo(&self) -> bool {
        self.has(|f| matches!(f, Fault::NoEcho))
    }

    // ----- The init burst -----

    /// The line (or lines, for a channel count spelled both ways) of one identity item.
    fn identity_lines(&self, ident: Ident) -> Vec<String> {
        let d = &self.dialect;
        match ident {
            Ident::VfoLimits => vec![notice("vfo_limits", [d.vfo_limits.0, d.vfo_limits.1])],
            Ident::IfLimits => vec![notice("if_limits", [d.if_limits.0, d.if_limits.1])],
            Ident::TrxCount => vec![notice("trx_count", [d.trx_count])],
            Ident::ChannelCount => {
                let mut lines = Vec::new();
                if matches!(d.channel_count, Spelling::Singular | Spelling::Both) {
                    lines.push(notice("channel_count", [d.channels]));
                }
                if matches!(d.channel_count, Spelling::Plural | Spelling::Both) {
                    lines.push(notice("channels_count", [d.channels]));
                }
                lines
            }
            Ident::Device => vec![notice("device", [d.device.clone()])],
            Ident::ReceiveOnly => {
                let receive_only = d.receive_only || self.has(|f| matches!(f, Fault::ReceiveOnly));
                vec![notice("receive_only", [flag(receive_only)])]
            }
            Ident::Modulations => vec![notice("modulations_list", d.modulations.clone())],
            Ident::Protocol => {
                let (program, version) = self
                    .find(|f| match f {
                        Fault::UnknownVersion { program, version } => {
                            Some((program.clone(), version.clone()))
                        }
                        _ => None,
                    })
                    .unwrap_or((d.program.clone(), d.version.clone()));
                vec![notice("protocol", [program, version])]
            }
        }
    }

    fn burst(&mut self) -> Vec<String> {
        let d = self.dialect.clone();
        let mut lines = Vec::new();
        for ident in &d.identity_order {
            lines.extend(self.identity_lines(*ident));
        }
        for t in 0..self.trx.len() {
            let s = self.trx[t].clone();
            lines.push(notice("dds", [t.to_string(), s.dds.to_string()]));
            for ch in 0..2 {
                lines.push(notice(
                    "if",
                    [t.to_string(), ch.to_string(), s.if_hz[ch].to_string()],
                ));
            }
            for ch in 0..2 {
                lines.push(notice(
                    "vfo",
                    [t.to_string(), ch.to_string(), s.vfo[ch].to_string()],
                ));
            }
            lines.push(notice("modulation", [t.to_string(), s.modulation.clone()]));
            lines.push(notice(
                "rx_filter_band",
                [
                    t.to_string(),
                    s.filter.0.to_string(),
                    s.filter.1.to_string(),
                ],
            ));
            lines.push(notice(
                "rit_enable",
                [t.to_string(), flag(s.rit).to_string()],
            ));
            lines.push(notice(
                "rit_offset",
                [t.to_string(), s.rit_offset.to_string()],
            ));
            lines.push(notice(
                "xit_enable",
                [t.to_string(), flag(s.xit).to_string()],
            ));
            lines.push(notice(
                "xit_offset",
                [t.to_string(), s.xit_offset.to_string()],
            ));
            lines.push(notice(
                "split_enable",
                [t.to_string(), flag(s.split).to_string()],
            ));
            if d.drive == DriveForm::PerTrx {
                lines.push(notice("drive", [t.to_string(), s.drive.to_string()]));
            }
            lines.push(notice(
                "tune_drive",
                [t.to_string(), s.tune_drive.to_string()],
            ));
            lines.push(notice(
                "tx_enable",
                [t.to_string(), flag(s.tx_enable).to_string()],
            ));
            lines.push(notice(
                "trx",
                [t.to_string(), flag(s.transmitting()).to_string()],
            ));
            lines.push(notice(
                "tune",
                [t.to_string(), flag(s.tuning()).to_string()],
            ));
        }
        if d.drive == DriveForm::Bare {
            lines.push(notice("drive", [self.trx[0].drive]));
        }
        lines.push(notice("cw_macros_speed", [self.keyer.speed]));
        lines.push(notice("cw_macros_delay", [self.keyer.delay]));
        lines.extend(self.params.iter().map(|(_, line)| line.clone()));
        if d.burst_stream_settings {
            let a = d.audio;
            lines.push(notice("audio_samplerate", [a.rate]));
            lines.push(notice("audio_stream_sample_type", [a.sample_type.name()]));
            lines.push(notice("audio_stream_channels", [a.channels]));
            lines.push(notice("audio_stream_samples", [a.samples()]));
            lines.push(notice("tx_stream_audio_buffering", [50]));
            lines.push(notice("iq_samplerate", [d.iq_rate]));
        }
        let run_state = if self.running { "start;" } else { "stop;" }.to_string();
        if !d.run_state_after_ready {
            lines.push(run_state.clone());
        }
        lines.extend(d.quirks.iter().cloned());
        if !self.has(|f| matches!(f, Fault::NoReady)) {
            lines.push("ready;".to_string());
        }
        if d.run_state_after_ready {
            lines.push(run_state);
        }
        lines
    }

    // ----- Commands -----

    fn apply(&mut self, who: Keyer, cmd: &Command) {
        let n = cmd.args.len();
        // A server acts only on the commands it knows; its own operator is not a client.
        if let (Keyer::Client(_), Some(known)) = (who, &self.dialect.commands) {
            if !known.contains(&cmd.name) {
                return self.ignore(who, cmd, Why::Unsupported);
            }
        }
        match cmd.name.as_str() {
            "start" | "stop" if n == 0 => self.run(cmd.name == "start", who),
            "start" | "stop" => self.ignore(who, cmd, Why::Arity),
            "vfo" => self.cmd_vfo(who, cmd),
            "dds" => self.cmd_dds(who, cmd),
            "if" => self.cmd_if(who, cmd),
            "modulation" => self.cmd_modulation(who, cmd),
            "rx_filter_band" => self.cmd_filter(who, cmd),
            "split_enable" | "rit_enable" | "xit_enable" => self.cmd_switch(who, cmd),
            "rit_offset" | "xit_offset" => self.cmd_offset(who, cmd),
            "drive" | "tune_drive" => self.cmd_drive(who, cmd),
            "trx" => self.cmd_trx(who, cmd),
            "tune" => self.cmd_tune(who, cmd),
            "keyer" => self.cmd_keyer(who, cmd),
            "audio_samplerate"
            | "audio_stream_sample_type"
            | "audio_stream_channels"
            | "audio_stream_samples"
            | "tx_stream_audio_buffering"
            | "iq_samplerate" => self.cmd_stream_setting(who, cmd),
            "audio_start" | "audio_stop" | "iq_start" | "iq_stop" => self.cmd_stream(who, cmd),
            "rx_sensors_enable" | "tx_sensors_enable" => self.cmd_sensors(who, cmd),
            "rx_smeter" => self.cmd_smeter(who, cmd),
            "cw_macros_speed" | "cw_macros_delay" => self.cmd_cw_setting(who, cmd),
            "cw_macros_speed_up" | "cw_macros_speed_down" => self.cmd_cw_step(who, cmd),
            "cw_terminal" => self.cmd_cw_terminal(who, cmd),
            "cw_macros" => self.cmd_cw_macros(who, cmd),
            "cw_msg" => self.cmd_cw_msg(who, cmd),
            "cw_macros_stop" if n == 0 => self.cw_stop(who),
            "cw_macros_stop" => self.ignore(who, cmd, Why::Arity),
            "spot" | "spot_delete" | "spot_clear" => self.cmd_spot(who, cmd),
            "set_in_focus" => {}
            "vfo_lock" => self.cmd_vfo_lock(who, cmd),
            "vfo_limits" | "if_limits" | "trx_count" | "channel_count" | "channels_count"
            | "device" | "receive_only" | "modulations_list" | "protocol" => {
                self.cmd_identity(who, cmd)
            }
            "line_out_start"
            | "line_out_stop"
            | "line_out_recorder_start"
            | "line_out_recorder_save"
            | "line_out_recorder_break" => self.ignore(who, cmd, Why::Unsupported),
            name if SERVER_ONLY.contains(&name) => self.ignore(who, cmd, Why::ServerOnly),
            name => match generic_keys(name) {
                Some(keys) => self.cmd_generic(who, cmd, keys),
                None => self.ignore(who, cmd, Why::Unknown),
            },
        }
    }

    /// The transceiver an argument names.
    fn trx_arg(&self, arg: &str) -> Option<usize> {
        arg.parse::<usize>().ok().filter(|t| *t < self.trx.len())
    }

    /// The channel an argument names.
    fn channel_arg(&self, arg: &str) -> Option<usize> {
        arg.parse::<usize>()
            .ok()
            .filter(|c| *c < (self.dialect.channels.clamp(1, 2) as usize))
    }

    /// Whether `who` may write the parameter `key` now ([`Fault::Hold200ms`]).
    fn may_set(&mut self, who: Keyer, key: &str, cmd: &Command) -> bool {
        if who == Keyer::Operator {
            return true; // the SDR's own operator always wins (spec §3.5)
        }
        if let Some(&(holder, until)) = self.holds.get(key) {
            if self.now < until && holder != who {
                if let Keyer::Client(conn) = who {
                    self.log(Event::Held {
                        conn,
                        text: cmd.text.clone(),
                    });
                }
                return false;
            }
        }
        true
    }

    /// `who` changed the parameter `key`: under [`Fault::Hold200ms`], it is held for the window.
    fn hold(&mut self, who: Keyer, key: String) {
        if let Some(window) = self.find(|f| match f {
            Fault::Hold200ms { window } => Some(*window),
            _ => None,
        }) {
            let until = self.now + window;
            self.holds.insert(key, (who, until));
        }
    }

    fn run(&mut self, on: bool, who: Keyer) {
        self.running = on;
        self.broadcast(if on { "start;" } else { "stop;" }.to_string());
        if !on {
            for t in 0..self.trx.len() {
                if self.trx[t].key.is_some() {
                    self.unkey_now(t, Cause::Asked(who), true);
                }
            }
            self.keyer.job = None;
            self.keyer.queue.clear();
        }
    }

    fn cmd_vfo(&mut self, who: Keyer, cmd: &Command) {
        if !matches!(cmd.args.len(), 2 | 3) {
            return self.ignore(who, cmd, Why::Arity);
        }
        let (Some(t), Some(ch)) = (self.trx_arg(cmd.arg(0)), self.channel_arg(cmd.arg(1))) else {
            return self.ignore(who, cmd, Why::Value);
        };
        if cmd.args.len() == 2 {
            let hz = self.trx[t].vfo[ch];
            return self.reply(who, notice("vfo", [t as u64, ch as u64, hz]));
        }
        let (lo, hi) = self.dialect.vfo_limits;
        let Some(hz) = cmd
            .arg(2)
            .parse::<u64>()
            .ok()
            .filter(|hz| (lo..=hi).contains(hz))
        else {
            return self.ignore(who, cmd, Why::Value);
        };
        let key = format!("vfo:{t},{ch}");
        if !self.may_set(who, &key, cmd) {
            return;
        }
        let before = band(self.trx[t].vfo[ch]);
        self.trx[t].vfo[ch] = hz;
        self.hold(who, key);
        self.broadcast(notice("vfo", [t as u64, ch as u64, hz]));
        if let Some(name) = band(hz).filter(|b| Some(*b) != before) {
            let restore = self.find(|f| match f {
                Fault::BandRestore {
                    drive,
                    tune_drive,
                    modulation,
                } => Some((*drive, *tune_drive, modulation.clone())),
                _ => None,
            });
            self.log(Event::BandChanged {
                trx: t as u32,
                band: name,
                restored: restore.is_some(),
            });
            if let Some((drive, tune_drive, modulation)) = restore {
                if let Some(mode) =
                    modulation.and_then(|m| self.dialect.modulation(&m).map(str::to_string))
                {
                    self.set_modulation(t, mode);
                }
                self.trx[t].drive = drive.min(100);
                self.trx[t].tune_drive = tune_drive.min(100);
                let line = self.drive_line(t, false);
                self.broadcast(line);
                let line = self.drive_line(t, true);
                self.broadcast(line);
            }
        }
        self.tx_frequency_changed(t);
    }

    fn cmd_dds(&mut self, who: Keyer, cmd: &Command) {
        if !matches!(cmd.args.len(), 1 | 2) {
            return self.ignore(who, cmd, Why::Arity);
        }
        let Some(t) = self.trx_arg(cmd.arg(0)) else {
            return self.ignore(who, cmd, Why::Value);
        };
        if cmd.args.len() == 1 {
            let hz = self.trx[t].dds;
            return self.reply(who, notice("dds", [t as u64, hz]));
        }
        let (lo, hi) = self.dialect.vfo_limits;
        let Some(hz) = cmd
            .arg(1)
            .parse::<u64>()
            .ok()
            .filter(|hz| (lo..=hi).contains(hz))
        else {
            return self.ignore(who, cmd, Why::Value);
        };
        let key = format!("dds:{t}");
        if !self.may_set(who, &key, cmd) {
            return;
        }
        self.trx[t].dds = hz;
        self.hold(who, key);
        self.broadcast(notice("dds", [t as u64, hz]));
    }

    fn cmd_if(&mut self, who: Keyer, cmd: &Command) {
        if !matches!(cmd.args.len(), 2 | 3) {
            return self.ignore(who, cmd, Why::Arity);
        }
        let (Some(t), Some(ch)) = (self.trx_arg(cmd.arg(0)), self.channel_arg(cmd.arg(1))) else {
            return self.ignore(who, cmd, Why::Value);
        };
        if cmd.args.len() == 2 {
            let hz = self.trx[t].if_hz[ch];
            return self.reply(who, notice("if", [t as i64, ch as i64, hz]));
        }
        let (lo, hi) = self.dialect.if_limits;
        let Some(hz) = cmd
            .arg(2)
            .parse::<i64>()
            .ok()
            .filter(|hz| (lo..=hi).contains(hz))
        else {
            return self.ignore(who, cmd, Why::Value);
        };
        let key = format!("if:{t},{ch}");
        if !self.may_set(who, &key, cmd) {
            return;
        }
        self.trx[t].if_hz[ch] = hz;
        self.hold(who, key);
        self.broadcast(notice("if", [t as i64, ch as i64, hz]));
    }

    fn cmd_modulation(&mut self, who: Keyer, cmd: &Command) {
        if !matches!(cmd.args.len(), 1 | 2) {
            return self.ignore(who, cmd, Why::Arity);
        }
        let Some(t) = self.trx_arg(cmd.arg(0)) else {
            return self.ignore(who, cmd, Why::Value);
        };
        if cmd.args.len() == 1 {
            let mode = self.trx[t].modulation.clone();
            return self.reply(who, notice("modulation", [t.to_string(), mode]));
        }
        let Some(mode) = self.dialect.modulation(cmd.arg(1)).map(str::to_string) else {
            return self.ignore(who, cmd, Why::Value);
        };
        let key = format!("modulation:{t}");
        if !self.may_set(who, &key, cmd) {
            return;
        }
        self.hold(who, key);
        self.set_modulation(t, mode);
    }

    /// Set and broadcast a mode; a change of sideband mirrors the filter, which is broadcast too.
    fn set_modulation(&mut self, t: usize, mode: String) {
        let lower = lower_sideband(&mode);
        let was = self.trx[t].filter;
        self.trx[t].modulation = mode.clone();
        self.trx[t].filter = sideband_filter(was, lower);
        self.broadcast(notice("modulation", [t.to_string(), mode]));
        if self.trx[t].filter != was {
            let (lo, hi) = self.trx[t].filter;
            self.broadcast(notice("rx_filter_band", [t as i64, lo, hi]));
        }
    }

    fn cmd_filter(&mut self, who: Keyer, cmd: &Command) {
        if !matches!(cmd.args.len(), 1 | 3) {
            return self.ignore(who, cmd, Why::Arity);
        }
        let Some(t) = self.trx_arg(cmd.arg(0)) else {
            return self.ignore(who, cmd, Why::Value);
        };
        if cmd.args.len() == 1 {
            let (lo, hi) = self.trx[t].filter;
            return self.reply(who, notice("rx_filter_band", [t as i64, lo, hi]));
        }
        let (Ok(lo), Ok(hi)) = (cmd.arg(1).parse::<i64>(), cmd.arg(2).parse::<i64>()) else {
            return self.ignore(who, cmd, Why::Value);
        };
        if lo >= hi {
            return self.ignore(who, cmd, Why::Value);
        }
        let key = format!("rx_filter_band:{t}");
        if !self.may_set(who, &key, cmd) {
            return;
        }
        self.trx[t].filter = (lo, hi);
        self.hold(who, key);
        self.broadcast(notice("rx_filter_band", [t as i64, lo, hi]));
    }

    fn cmd_switch(&mut self, who: Keyer, cmd: &Command) {
        if !matches!(cmd.args.len(), 1 | 2) {
            return self.ignore(who, cmd, Why::Arity);
        }
        let Some(t) = self.trx_arg(cmd.arg(0)) else {
            return self.ignore(who, cmd, Why::Value);
        };
        let name = cmd.name.as_str();
        let current = |s: &TrxState| match name {
            "split_enable" => s.split,
            "rit_enable" => s.rit,
            _ => s.xit,
        };
        if cmd.args.len() == 1 {
            let on = current(&self.trx[t]);
            return self.reply(who, notice(name, [t.to_string(), flag(on).to_string()]));
        }
        let Some(on) = wire::boolean(cmd.arg(1)) else {
            return self.ignore(who, cmd, Why::Value);
        };
        let key = format!("{name}:{t}");
        if !self.may_set(who, &key, cmd) {
            return;
        }
        match name {
            "split_enable" => self.trx[t].split = on,
            "rit_enable" => self.trx[t].rit = on,
            _ => self.trx[t].xit = on,
        }
        self.hold(who, key);
        self.broadcast(notice(name, [t.to_string(), flag(on).to_string()]));
        self.tx_frequency_changed(t);
    }

    fn cmd_offset(&mut self, who: Keyer, cmd: &Command) {
        if !matches!(cmd.args.len(), 1 | 2) {
            return self.ignore(who, cmd, Why::Arity);
        }
        let Some(t) = self.trx_arg(cmd.arg(0)) else {
            return self.ignore(who, cmd, Why::Value);
        };
        let rit = cmd.name == "rit_offset";
        if cmd.args.len() == 1 {
            let hz = if rit {
                self.trx[t].rit_offset
            } else {
                self.trx[t].xit_offset
            };
            return self.reply(who, notice(&cmd.name, [t as i64, hz]));
        }
        let Some(hz) = cmd
            .arg(1)
            .parse::<i64>()
            .ok()
            .filter(|hz| hz.abs() <= 100_000)
        else {
            return self.ignore(who, cmd, Why::Value);
        };
        let key = format!("{}:{t}", cmd.name);
        if !self.may_set(who, &key, cmd) {
            return;
        }
        if rit {
            self.trx[t].rit_offset = hz;
        } else {
            self.trx[t].xit_offset = hz;
        }
        self.hold(who, key);
        self.broadcast(notice(&cmd.name, [t as i64, hz]));
        self.tx_frequency_changed(t);
    }

    /// The `drive` (or `tune_drive`) line for transceiver `t`, in the dialect's form.
    fn drive_line(&self, t: usize, tune: bool) -> String {
        if tune {
            notice("tune_drive", [t as u32, self.trx[t].tune_drive])
        } else if self.dialect.drive == DriveForm::Bare {
            notice("drive", [self.trx[t].drive])
        } else {
            notice("drive", [t as u32, self.trx[t].drive])
        }
    }

    fn cmd_drive(&mut self, who: Keyer, cmd: &Command) {
        let tune = cmd.name == "tune_drive";
        let bare = !tune && self.dialect.drive == DriveForm::Bare;
        // The transceiver and the value's place: `drive:<v>` in the 1.x form has no transceiver.
        let (t, value) = if bare {
            match cmd.args.len() {
                0 => (0, None),
                1 => (0, Some(cmd.arg(0))),
                _ => return self.ignore(who, cmd, Why::Arity),
            }
        } else {
            let Some(t) = (match cmd.args.len() {
                1 | 2 => self.trx_arg(cmd.arg(0)),
                _ => return self.ignore(who, cmd, Why::Arity),
            }) else {
                return self.ignore(who, cmd, Why::Value);
            };
            (t, (cmd.args.len() == 2).then(|| cmd.arg(1)))
        };
        let Some(value) = value else {
            let line = self.drive_line(t, tune);
            return self.reply(who, line);
        };
        let Some(v) = value.parse::<u32>().ok().filter(|v| *v <= 100) else {
            return self.ignore(who, cmd, Why::Value);
        };
        let key = format!("{}:{t}", cmd.name);
        if !self.may_set(who, &key, cmd) {
            return;
        }
        if tune {
            self.trx[t].tune_drive = v;
        } else if bare {
            for s in &mut self.trx {
                s.drive = v;
            }
        } else {
            self.trx[t].drive = v;
        }
        self.hold(who, key);
        let line = self.drive_line(t, tune);
        self.broadcast(line);
    }

    fn cmd_trx(&mut self, who: Keyer, cmd: &Command) {
        if !matches!(cmd.args.len(), 1..=3) {
            return self.ignore(who, cmd, Why::Arity);
        }
        let Some(t) = self.trx_arg(cmd.arg(0)) else {
            return self.ignore(who, cmd, Why::Value);
        };
        if cmd.args.len() == 1 {
            if !self.no_echo() {
                let on = self.trx[t].transmitting();
                self.reply(who, notice("trx", [t.to_string(), flag(on).to_string()]));
            }
            return;
        }
        let Some(on) = wire::boolean(cmd.arg(1)) else {
            return self.ignore(who, cmd, Why::Value);
        };
        if !on {
            return self.stop_request(t, who, &cmd.text, Stop::Trx);
        }
        let source = cmd.args.get(2).map(|s| s.to_ascii_lowercase());
        let wanted = match self.dialect.key_source {
            KeySource::RunningAudio => Wanted::Running {
                asked_tci: source.as_deref() == Some("tci"),
            },
            KeySource::Argument => match source.as_deref() {
                None => Wanted::Mic(Mic::Selected),
                Some("tci") => Wanted::Tci,
                Some("mic1") => Wanted::Mic(Mic::Mic1),
                Some("mic2") => Wanted::Mic(Mic::Mic2),
                Some("micpc") => Wanted::Mic(Mic::MicPc),
                Some("ecoder2") => Wanted::Mic(Mic::Ecoder2),
                Some(_) => return self.ignore(who, cmd, Why::Value),
            },
        };
        self.key(t, who, wanted, &cmd.text);
    }

    fn cmd_tune(&mut self, who: Keyer, cmd: &Command) {
        if !matches!(cmd.args.len(), 1 | 2) {
            return self.ignore(who, cmd, Why::Arity);
        }
        let Some(t) = self.trx_arg(cmd.arg(0)) else {
            return self.ignore(who, cmd, Why::Value);
        };
        if cmd.args.len() == 1 {
            let on = self.trx[t].tuning();
            return self.reply(who, notice("tune", [t.to_string(), flag(on).to_string()]));
        }
        match wire::boolean(cmd.arg(1)) {
            Some(true) => self.key(t, who, Wanted::Tune, &cmd.text),
            Some(false) => self.stop_request(t, who, &cmd.text, Stop::Tune),
            None => self.ignore(who, cmd, Why::Value),
        }
    }

    fn cmd_keyer(&mut self, who: Keyer, cmd: &Command) {
        if !matches!(cmd.args.len(), 2 | 3) {
            return self.ignore(who, cmd, Why::Arity);
        }
        let Some(t) = self.trx_arg(cmd.arg(0)) else {
            return self.ignore(who, cmd, Why::Value);
        };
        match wire::boolean(cmd.arg(1)) {
            Some(true) => self.key(t, who, Wanted::Key, &cmd.text),
            Some(false) => self.stop_request(t, who, &cmd.text, Stop::Keyer),
            None => self.ignore(who, cmd, Why::Value),
        }
    }

    /// The radio's own checks before any key: receive-only, transmit permission, running,
    /// ownership. A refusal for ownership is answered with the actual state, as the servers read
    /// do; the others are not answered.
    fn admit(&mut self, t: usize, who: Keyer, text: &str, tune: bool) -> bool {
        let receive_only =
            self.dialect.receive_only || self.has(|f| matches!(f, Fault::ReceiveOnly));
        let refusal = if receive_only {
            Some(Refusal::ReceiveOnly)
        } else if !self.trx[t].tx_enable {
            Some(Refusal::TxDisabled)
        } else if !self.running {
            Some(Refusal::Stopped)
        } else {
            None
        };
        if let Some(why) = refusal {
            self.refuse(who, text, why);
            return false;
        }
        let Keyer::Client(c) = who else {
            return true; // the SDR's own operator always wins
        };
        let mine = match self.dialect.ownership {
            Ownership::None => true,
            Ownership::FirstKeyer => match self.owner {
                Some(o) => o == c,
                None => {
                    self.owner = Some(c);
                    true
                }
            },
            Ownership::Initiator => self.trx[t].key.is_none_or(|k| k.orphaned || k.by == who),
        };
        if !mine {
            self.refuse(who, text, Refusal::NotOwner);
            self.answer_state(t, who, tune);
        }
        mine
    }

    /// The actual `trx` (or `tune`) state, to the asker.
    fn answer_state(&mut self, t: usize, who: Keyer, tune: bool) {
        if tune {
            let on = self.trx[t].tuning();
            self.reply(who, notice("tune", [t.to_string(), flag(on).to_string()]));
        } else if !self.no_echo() {
            let on = self.trx[t].transmitting();
            self.reply(who, notice("trx", [t.to_string(), flag(on).to_string()]));
        }
    }

    /// Whether `conn`'s audio stream for transceiver `t` runs (or the server started its transmit
    /// audio itself).
    fn audio_running(&self, conn: usize, t: usize) -> bool {
        self.clients
            .get(&conn)
            .is_some_and(|c| c.tx_audio || c.audio_streams.contains_key(&(t as u32)))
    }

    fn key(&mut self, t: usize, who: Keyer, wanted: Wanted, text: &str) {
        if !self.admit(t, who, text, wanted == Wanted::Tune) {
            return;
        }
        let running = match who {
            Keyer::Client(c) => self.audio_running(c, t),
            Keyer::Operator => true,
        };
        let source = match wanted {
            Wanted::Mic(m) => Source::Mic(m),
            Wanted::Tune => Source::Tune,
            Wanted::Key => Source::Key,
            Wanted::Tci
                if running || !self.dialect.mic_fallback || self.dialect.tci_key_starts_audio =>
            {
                Source::Tci
            }
            Wanted::Running { .. } if running => Source::Tci,
            Wanted::Tci | Wanted::Running { asked_tci: true } => {
                if let Keyer::Client(conn) = who {
                    self.log(Event::MicFallback {
                        conn,
                        trx: t as u32,
                    });
                }
                Source::Mic(Mic::Selected)
            }
            Wanted::Running { asked_tci: false } => Source::Mic(Mic::Selected),
        };
        if let Some(k) = self.trx[t].key {
            if k.by == who && k.source == source {
                // The same key again: nothing changes; the state is reported again.
                if self.trx[t].echoed {
                    self.answer_state(t, who, source == Source::Tune);
                }
                return;
            }
            self.unkey_now(t, Cause::Replaced, false);
        }
        self.take_key(t, who, source);
    }

    /// Key transceiver `t` (admission done), and echo it now or when the dialect does.
    fn take_key(&mut self, t: usize, who: Keyer, source: Source) {
        let id = self.next();
        self.trx[t].key = Some(Key {
            by: who,
            source,
            id,
            orphaned: false,
        });
        self.trx[t].echoed = false;
        self.log(Event::Keyed {
            trx: t as u32,
            by: who,
            source,
        });
        if let (Source::Tci, Keyer::Client(c), true) =
            (source, who, self.dialect.tci_key_starts_audio)
        {
            if let Some(client) = self.clients.get_mut(&c) {
                client.tx_audio = true;
            }
            // piHPSDR tells the client which stream it expects, then starts it.
            let a = self.dialect.audio;
            for line in [
                notice("audio_samplerate", [a.rate]),
                notice("audio_stream_sample_type", [a.sample_type.name()]),
                notice("audio_stream_channels", [a.channels]),
                notice("audio_stream_samples", [a.samples()]),
                notice("tx_stream_audio_buffering", [50]),
                notice("audio_start", [t]),
            ] {
                self.send(To::One(c), line);
            }
        }
        let delay = self.dialect.trx_echo_delay;
        if delay.is_zero() {
            self.echo(t, id);
        } else {
            self.timer(
                self.now + delay,
                Action::Echo {
                    trx: t as u32,
                    key: id,
                },
            );
        }
    }

    /// The radio transmits: echo the key, start TX_CHRONO for TCI audio, and arm the faults that
    /// fire on a key.
    fn echo(&mut self, t: usize, id: u64) {
        let Some(k) = self.trx[t].key.filter(|k| k.id == id) else {
            return;
        };
        self.trx[t].echoed = true;
        match k.source {
            Source::Tune => self.broadcast(notice("tune", [t.to_string(), "true".to_string()])),
            _ if self.no_echo() => {}
            _ => self.broadcast(notice("trx", [t.to_string(), "true".to_string()])),
        }
        let now = self.now;
        if k.source == Source::Tci {
            let targets: Vec<usize> = match k.by {
                Keyer::Client(c) => vec![c],
                Keyer::Operator if self.chrono_to_all() => self
                    .clients
                    .iter()
                    .filter(|(_, c)| !c.audio_streams.is_empty())
                    .map(|(id, _)| *id)
                    .collect(),
                Keyer::Operator => Vec::new(),
            };
            if self.has(|f| matches!(f, Fault::NoChrono)) {
                if !targets.is_empty() {
                    self.log(Event::Fault {
                        name: "NoChrono",
                        conn: None,
                        detail: format!("key {id}: no TX_CHRONO will be sent"),
                    });
                }
            } else {
                let late = self
                    .find(|f| match f {
                        Fault::ChronoLate { delay } => Some(*delay),
                        _ => None,
                    })
                    .unwrap_or(Duration::ZERO);
                let epoch = now + self.dialect.first_chrono + late;
                for conn in targets {
                    self.timer(
                        epoch,
                        Action::Chrono {
                            conn,
                            trx: t as u32,
                            key: id,
                            n: 0,
                            epoch,
                        },
                    );
                }
            }
        }
        if let (Keyer::Client(c), Some(after)) = (
            k.by,
            self.find(|f| match f {
                Fault::DisconnectMidOver { after, .. } => Some(*after),
                _ => None,
            }),
        ) {
            if self.once("DisconnectMidOver") {
                self.timer(now + after, Action::Drop { conn: c });
            }
        }
        if let Some(after) = self.find(|f| match f {
            Fault::TxSensorsLate { after } => Some(*after),
            _ => None,
        }) {
            if self.once("TxSensorsLate") {
                self.timer(now + after, Action::TxSensorsFrom);
            }
        }
        if let Some(after) = self.find(|f| match f {
            Fault::ServerStop { after } => Some(*after),
            _ => None,
        }) {
            if self.once("ServerStop") {
                self.timer(now + after, Action::ServerStop);
            }
        }
    }

    fn chrono_to_all(&self) -> bool {
        self.has(|f| {
            matches!(
                f,
                Fault::ForeignKey {
                    chrono_to_all: true,
                    ..
                }
            )
        })
    }

    /// A client's (or the operator's) stop: `trx:<n>,false`, `tune:<n>,false` or
    /// `keyer:<n>,false`. Stops are never refused for the radio's state; only ownership and a
    /// fault can make the radio ignore one, and then it answers with the state it keeps.
    fn stop_request(&mut self, t: usize, who: Keyer, text: &str, stop: Stop) {
        let Some(k) = self.trx[t].key else {
            return self.answer_state(t, who, stop == Stop::Tune);
        };
        if stop == Stop::Tune && k.source != Source::Tune {
            return self.answer_state(t, who, true);
        }
        if self.has(|f| matches!(f, Fault::StuckTrx)) && stop == Stop::Trx {
            self.refuse(who, text, Refusal::Stuck);
            return self.answer_state(t, who, false);
        }
        if let Keyer::Client(c) = who {
            let allowed = match self.dialect.ownership {
                Ownership::None => true,
                Ownership::FirstKeyer => self.owner == Some(c),
                Ownership::Initiator => k.orphaned || k.by == who,
            };
            if !allowed {
                self.refuse(who, text, Refusal::NotOwner);
                return self.answer_state(t, who, stop == Stop::Tune);
            }
        }
        let tail = self.dialect.ptt_tail;
        if k.source == Source::Tci && !tail.is_zero() {
            self.timer(
                self.now + tail,
                Action::TailEnd {
                    trx: t as u32,
                    key: k.id,
                },
            );
            return;
        }
        self.unkey_now(t, Cause::Asked(who), true);
    }

    /// Unkey transceiver `t` at once; with `echo`, announce it.
    fn unkey_now(&mut self, t: usize, cause: Cause, echo: bool) {
        let Some(k) = self.trx[t].key.take() else {
            return;
        };
        self.trx[t].echoed = false;
        self.log(Event::Unkeyed {
            trx: t as u32,
            by: k.by,
            cause,
        });
        if echo {
            match k.source {
                Source::Tune => {
                    self.broadcast(notice("tune", [t.to_string(), "false".to_string()]))
                }
                _ if self.no_echo() => {}
                _ => self.broadcast(notice("trx", [t.to_string(), "false".to_string()])),
            }
        }
        if k.source == Source::Cw && cause != Cause::CwDone {
            self.cw_cancel();
        }
    }

    fn foreign_key(&mut self, on: bool) {
        let Some((hold, all)) = self.find(|f| match f {
            Fault::ForeignKey {
                hold,
                chrono_to_all,
                ..
            } => Some((*hold, *chrono_to_all)),
            _ => None,
        }) else {
            return;
        };
        if on {
            self.log(Event::Fault {
                name: "ForeignKey",
                conn: None,
                detail: format!("another program keys for {} ms", millis(hold)),
            });
            let wanted = if all {
                Wanted::Tci
            } else {
                Wanted::Mic(Mic::Selected)
            };
            self.key(0, Keyer::Operator, wanted, "trx:0,true");
            self.timer(self.now + hold, Action::ForeignKey { on: false });
        } else if self.trx[0].key.is_some_and(|k| k.by == Keyer::Operator) {
            self.unkey_now(0, Cause::Asked(Keyer::Operator), true);
        }
    }

    fn server_stop(&mut self) {
        self.log(Event::Fault {
            name: "ServerStop",
            conn: None,
            detail: "the SDR stops; no trx:false echo".into(),
        });
        self.running = false;
        self.broadcast("stop;".to_string());
        for t in 0..self.trx.len() {
            self.unkey_now(t, Cause::ServerStop, false);
        }
        self.keyer.job = None;
        self.keyer.queue.clear();
    }

    // ----- Streams -----

    /// Frames per packet and the header's `length`, for `length` written as values (or frames,
    /// under [`Fault::LengthAsFrames`]).
    fn shape(&self, length: u32, channels: u32) -> (u64, u32) {
        let channels = channels.max(1);
        if self.has(|f| matches!(f, Fault::LengthAsFrames)) {
            (u64::from(length.max(1)), length)
        } else {
            (u64::from((length / channels).max(1)), length)
        }
    }

    /// A header's `format` for float32 samples, or 4 under [`Fault::FormatCode4`].
    fn format_code(&self, t: SampleType) -> u32 {
        if t == SampleType::Float32 && self.has(|f| matches!(f, Fault::FormatCode4)) {
            4
        } else {
            t.code()
        }
    }

    fn cmd_stream_setting(&mut self, who: Keyer, cmd: &Command) {
        let Keyer::Client(c) = who else {
            return; // a per-client setting: the operator has none
        };
        let Some(client) = self.clients.get(&c).cloned() else {
            return;
        };
        let name = cmd.name.as_str();
        if cmd.args.is_empty() {
            let line = match name {
                "audio_samplerate" => notice(name, [client.audio.rate]),
                "audio_stream_sample_type" => notice(name, [client.audio.sample_type.name()]),
                "audio_stream_channels" => notice(name, [client.audio.channels]),
                "audio_stream_samples" => notice(name, [client.audio.samples()]),
                "tx_stream_audio_buffering" => notice(name, [client.tx_buffering_ms]),
                _ => notice(name, [client.iq_rate]),
            };
            return self.reply(who, line);
        }
        if cmd.args.len() != 1 {
            return self.ignore(who, cmd, Why::Arity);
        }
        let arg = cmd.arg(0);
        let number = arg.parse::<u32>().ok();
        let mut next = client.clone();
        let ok = match name {
            "audio_samplerate" => number
                .filter(|r| [8_000, 12_000, 24_000, 48_000].contains(r))
                .map(|r| next.audio.rate = r)
                .is_some(),
            "audio_stream_sample_type" => SampleType::from_name(arg)
                .map(|t| next.audio.sample_type = t)
                .is_some(),
            "audio_stream_channels" => number
                .filter(|n| matches!(n, 1 | 2))
                .map(|n| next.audio.channels = n)
                .is_some(),
            "audio_stream_samples" => number
                .filter(|n| (100..=2048).contains(n))
                .map(|n| next.audio.samples = Some(n))
                .is_some(),
            "tx_stream_audio_buffering" => number
                .filter(|n| (50..=500).contains(n))
                .map(|n| next.tx_buffering_ms = n)
                .is_some(),
            _ => number
                .filter(|r| [48_000, 96_000, 192_000, 384_000].contains(r))
                .map(|r| next.iq_rate = r)
                .is_some(),
        };
        if !ok {
            return self.ignore(who, cmd, Why::Value);
        }
        // A running stream restarts in its new shape: a new id (so the old one's timer lapses),
        // from sample 0.
        let now = self.now;
        let audio: Vec<u32> = if next.audio != client.audio {
            next.audio_streams.keys().copied().collect()
        } else {
            Vec::new()
        };
        let iqs: Vec<u32> = if next.iq_rate != client.iq_rate {
            next.iq_streams.keys().copied().collect()
        } else {
            Vec::new()
        };
        let (frames, _) = self.shape(next.audio.samples(), next.audio.channels);
        let (pairs, _) = self.shape(self.dialect.iq_values, 2);
        let mut timers = Vec::new();
        for t in audio {
            let id = self.next();
            next.audio_streams.insert(t, Stream::new(id, now));
            timers.push((
                now + secs(frames, next.audio.rate),
                Action::RxAudio {
                    conn: c,
                    trx: t,
                    id,
                },
            ));
        }
        for t in iqs {
            let id = self.next();
            next.iq_streams.insert(t, Stream::new(id, now));
            timers.push((
                now + secs(pairs, next.iq_rate),
                Action::Iq {
                    conn: c,
                    trx: t,
                    id,
                },
            ));
        }
        self.clients.insert(c, next);
        for (at, action) in timers {
            self.timer(at, action);
        }
        if self.dialect.echo_stream_settings {
            self.reply(who, notice(name, [arg]));
        }
    }

    fn cmd_stream(&mut self, who: Keyer, cmd: &Command) {
        let Keyer::Client(c) = who else {
            return;
        };
        if cmd.args.len() != 1 {
            return self.ignore(who, cmd, Why::Arity);
        }
        let Some(t) = self.trx_arg(cmd.arg(0)) else {
            return self.ignore(who, cmd, Why::Value);
        };
        let Some(client) = self.clients.get(&c).cloned() else {
            return;
        };
        let iq = cmd.name.starts_with("iq");
        let start = cmd.name.ends_with("start");
        let t32 = t as u32;
        if !start {
            if let Some(client) = self.clients.get_mut(&c) {
                if iq {
                    client.iq_streams.remove(&t32);
                } else {
                    client.audio_streams.remove(&t32);
                }
            }
            let ack = if iq {
                self.dialect.iq_ack
            } else {
                self.dialect.audio_ack
            };
            if ack {
                self.reply(who, notice(&cmd.name, [t]));
            }
            return;
        }
        if !iq && self.has(|f| matches!(f, Fault::NoAudioAck)) {
            return self.ignore(who, cmd, Why::Fault("NoAudioAck"));
        }
        if !iq
            && self.dialect.audio_needs_default_format
            && (client.audio.rate != 48_000 || client.audio.sample_type != SampleType::Float32)
        {
            return self.refuse(who, &cmd.text, Refusal::Format);
        }
        let running = if iq {
            client.iq_streams.contains_key(&t32)
        } else {
            client.audio_streams.contains_key(&t32)
        };
        if !running {
            let id = self.next();
            let now = self.now;
            let stream = Stream::new(id, now);
            let (first_due, action) = if iq {
                let (pairs, _) = self.shape(self.dialect.iq_values, 2);
                (
                    now + secs(pairs, client.iq_rate),
                    Action::Iq {
                        conn: c,
                        trx: t32,
                        id,
                    },
                )
            } else {
                let (frames, _) = self.shape(client.audio.samples(), client.audio.channels);
                (
                    now + secs(frames, client.audio.rate),
                    Action::RxAudio {
                        conn: c,
                        trx: t32,
                        id,
                    },
                )
            };
            if let Some(client) = self.clients.get_mut(&c) {
                if iq {
                    client.iq_streams.insert(t32, stream);
                } else {
                    client.audio_streams.insert(t32, stream);
                }
            }
            self.timer(first_due, action);
        }
        let ack = if iq {
            self.dialect.iq_ack
        } else {
            self.dialect.audio_ack
        };
        if ack {
            self.reply(who, notice(&cmd.name, [t]));
        }
    }

    /// The tuning transceiver `t` hears with.
    fn tuning(&self, t: usize) -> Tuning {
        let s = &self.trx[t];
        let rit = if s.rit { s.rit_offset } else { 0 };
        Tuning {
            dial: i64::try_from(s.vfo[0]).unwrap_or(i64::MAX) + rit,
            filter: s.filter,
            digital: ["digu", "digl"]
                .iter()
                .any(|m| m.eq_ignore_ascii_case(&s.modulation)),
        }
    }

    /// A stream whose tick `n` is due at `origin` + `n` × `per` / `rate`, running tick `n` now: when
    /// that is more than [`BEHIND`] late, the tick to go on from, the first still to come.
    fn resume(&self, origin: Instant, n: u64, per: u64, rate: u32) -> Option<u64> {
        if self.now <= origin + secs(n * per, rate) + BEHIND {
            return None;
        }
        let elapsed = self.now.saturating_duration_since(origin).as_secs_f64();
        Some((elapsed * f64::from(rate.max(1)) / per.max(1) as f64) as u64 + 1)
    }

    fn rx_audio_tick(&mut self, conn: usize, trx: u32, id: u64) {
        let Some(client) = self.clients.get(&conn) else {
            return;
        };
        let Some(stream) = client
            .audio_streams
            .get(&trx)
            .copied()
            .filter(|s| s.id == id)
        else {
            return;
        };
        let fmt = client.audio;
        let (frames, length) = self.shape(fmt.samples(), fmt.channels);
        let k = stream.frames / frames;
        if let Some(next) =
            self.resume(stream.started + secs(frames, fmt.rate), k, frames, fmt.rate)
        {
            self.log(Event::Lost {
                conn,
                trx,
                kind: kind::RX_AUDIO,
                frames: next - k,
            });
            if let Some(s) = self
                .clients
                .get_mut(&conn)
                .and_then(|c| c.audio_streams.get_mut(&trx))
            {
                s.frames = next * frames;
            }
            let due = stream.started + secs(next * frames + frames, fmt.rate);
            return self.timer(due, Action::RxAudio { conn, trx, id });
        }
        let stall = self.find(|f| match f {
            Fault::RxAudioStall { after } => Some(*after),
            _ => None,
        });
        let stalled = stall.is_some_and(|after| self.now >= stream.started + after);
        let mut logged = stream.stall_logged;
        if stalled && !logged {
            logged = true;
            self.log(Event::Fault {
                name: "RxAudioStall",
                conn: Some(conn),
                detail: format!(
                    "receive audio on transceiver {trx} stops; the stream still counts as running"
                ),
            });
        }
        if self.running && !stalled {
            let tuning = self.tuning(trx as usize);
            let t0 = stream
                .started
                .saturating_duration_since(self.started)
                .as_secs_f64();
            let samples = synth::rx_audio(
                &self.receive,
                tuning,
                &self.loopback,
                fmt.rate,
                fmt.channels,
                stream.frames,
                frames as usize,
                t0,
            );
            let header = Header {
                receiver: trx,
                sample_rate: fmt.rate,
                format: self.format_code(fmt.sample_type),
                length,
                kind: kind::RX_AUDIO,
                channels: fmt.channels,
                ..Header::default()
            };
            let payload = frame::encode_samples(fmt.sample_type, &samples);
            self.fx.push(Effect::Frame {
                to: conn,
                bytes: header.frame(&payload),
            });
        }
        // The next frame goes out once all of its samples have elapsed.
        let next_frames = stream.frames + frames;
        let due = stream.started + secs(next_frames + frames, fmt.rate);
        if let Some(s) = self
            .clients
            .get_mut(&conn)
            .and_then(|c| c.audio_streams.get_mut(&trx))
        {
            s.frames = next_frames;
            s.stall_logged = logged;
        }
        self.timer(due, Action::RxAudio { conn, trx, id });
    }

    fn iq_tick(&mut self, conn: usize, trx: u32, id: u64) {
        let Some(client) = self.clients.get(&conn) else {
            return;
        };
        let Some(stream) = client.iq_streams.get(&trx).copied().filter(|s| s.id == id) else {
            return;
        };
        let rate = client.iq_rate;
        let (pairs, length) = self.shape(self.dialect.iq_values, 2);
        let k = stream.frames / pairs;
        if let Some(next) = self.resume(stream.started + secs(pairs, rate), k, pairs, rate) {
            self.log(Event::Lost {
                conn,
                trx,
                kind: kind::IQ,
                frames: next - k,
            });
            if let Some(s) = self
                .clients
                .get_mut(&conn)
                .and_then(|c| c.iq_streams.get_mut(&trx))
            {
                s.frames = next * pairs;
            }
            let due = stream.started + secs(next * pairs + pairs, rate);
            return self.timer(due, Action::Iq { conn, trx, id });
        }
        if self.running {
            let samples = synth::iq(
                &self.receive,
                self.trx[trx as usize].dds,
                rate,
                stream.frames,
                pairs as usize,
            );
            let header = Header {
                receiver: trx,
                sample_rate: rate,
                format: self.format_code(SampleType::Float32),
                length,
                kind: kind::IQ,
                channels: 2,
                ..Header::default()
            };
            let payload = frame::encode_samples(SampleType::Float32, &samples);
            self.fx.push(Effect::Frame {
                to: conn,
                bytes: header.frame(&payload),
            });
        }
        let next = stream.frames + pairs;
        if let Some(s) = self
            .clients
            .get_mut(&conn)
            .and_then(|c| c.iq_streams.get_mut(&trx))
        {
            s.frames = next;
        }
        self.timer(
            stream.started + secs(next + pairs, rate),
            Action::Iq { conn, trx, id },
        );
    }

    fn chrono_tick(&mut self, conn: usize, trx: u32, key: u64, n: u64, epoch: Instant) {
        let t = trx as usize;
        let Some(k) = self.trx[t].key.filter(|k| k.id == key) else {
            return;
        };
        if k.source != Source::Tci || !self.running {
            return;
        }
        let to_this = match k.by {
            Keyer::Client(c) => c == conn,
            Keyer::Operator => {
                self.chrono_to_all()
                    && self
                        .clients
                        .get(&conn)
                        .is_some_and(|c| !c.audio_streams.is_empty())
            }
        };
        let Some(client) = self.clients.get(&conn).cloned() else {
            return;
        };
        if !to_this {
            return;
        }
        if let Some(after) = self.find(|f| match f {
            Fault::ChronoStall { after } => Some(*after),
            _ => None,
        }) {
            if self.now >= epoch + after {
                if self.once(&format!("ChronoStall:{key}:{conn}")) {
                    self.log(Event::Fault {
                        name: "ChronoStall",
                        conn: Some(conn),
                        detail: format!("key {key}: TX_CHRONO stops; the radio stays keyed"),
                    });
                }
                return;
            }
        }
        let fmt = client.audio;
        let length = match self.dialect.chrono_length {
            ChronoLength::Fixed(l) => l,
            ChronoLength::ClientSamples => fmt.samples(),
        };
        let channels = fmt.channels.max(1);
        let (frames, length) = self.shape(length, channels);
        let burst = u64::from(
            self.find(|f| match f {
                Fault::ChronoBurst { burst } => Some(*burst),
                _ => None,
            })
            .unwrap_or(1)
            .max(1),
        );
        if let Some(next) = self.resume(epoch, n, frames, fmt.rate) {
            let next = next.div_ceil(burst) * burst;
            self.log(Event::Lost {
                conn,
                trx,
                kind: kind::TX_CHRONO,
                frames: next - n,
            });
            let due = epoch + secs(frames * next, fmt.rate);
            return self.timer(
                due,
                Action::Chrono {
                    conn,
                    trx,
                    key,
                    n: next,
                    epoch,
                },
            );
        }
        let header = Header {
            receiver: trx,
            sample_rate: fmt.rate,
            format: fmt.sample_type.code(),
            length,
            kind: kind::TX_CHRONO,
            channels,
            ..Header::default()
        };
        for _ in 0..burst {
            self.fx.push(Effect::Frame {
                to: conn,
                bytes: header.encode().to_vec(),
            });
        }
        if let Some(c) = self.clients.get_mut(&conn) {
            c.asked = Some(Ask { length, channels });
        }
        let next = n + burst;
        let due = epoch + secs(frames * next, fmt.rate);
        self.timer(
            due,
            Action::Chrono {
                conn,
                trx,
                key,
                n: next,
                epoch,
            },
        );
    }

    fn cmd_sensors(&mut self, who: Keyer, cmd: &Command) {
        let Keyer::Client(c) = who else {
            return;
        };
        if !matches!(cmd.args.len(), 1 | 2) {
            return self.ignore(who, cmd, Why::Arity);
        }
        let Some(on) = wire::boolean(cmd.arg(0)) else {
            return self.ignore(who, cmd, Why::Value);
        };
        let ms = match cmd.args.get(1) {
            None => None,
            Some(a) => match a.parse::<u64>().ok().filter(|ms| (30..=1000).contains(ms)) {
                Some(ms) => Some(ms),
                None => return self.ignore(who, cmd, Why::Value),
            },
        };
        let interval = if self.dialect.fixed_sensor_interval {
            self.dialect.sensor_interval
        } else {
            ms.map_or(self.dialect.sensor_interval, Duration::from_millis)
        };
        let rx = cmd.name == "rx_sensors_enable";
        let now = self.now;
        let Some(client) = self.clients.get_mut(&c) else {
            return;
        };
        let gen = if rx {
            client.rx_gen += 1;
            client.rx_sensors = on.then_some(interval);
            client.rx_gen
        } else {
            client.tx_gen += 1;
            client.tx_sensors = on.then_some(interval);
            client.tx_gen
        };
        if on {
            let action = if rx {
                Action::RxSensors { conn: c, gen }
            } else {
                Action::TxSensors { conn: c, gen }
            };
            self.timer(now + interval, action);
        }
    }

    fn rx_sensors_tick(&mut self, conn: usize, gen: u64) {
        let Some(interval) = self
            .clients
            .get(&conn)
            .filter(|c| c.rx_gen == gen)
            .and_then(|c| c.rx_sensors)
        else {
            return;
        };
        if self.running {
            let dbm = format!("{:.1}", self.meters.smeter_dbm);
            for form in self.dialect.rx_sensors.clone() {
                for t in 0..self.trx.len() {
                    match form {
                        SensorForm::Channel => {
                            for ch in 0..self.dialect.channels.clamp(1, 2) {
                                let line = notice(
                                    "rx_channel_sensors",
                                    [t.to_string(), ch.to_string(), dbm.clone()],
                                );
                                self.send(To::One(conn), line);
                            }
                        }
                        SensorForm::Receiver => {
                            let line = notice("rx_sensors", [t.to_string(), dbm.clone()]);
                            self.send(To::One(conn), line);
                        }
                    }
                }
            }
        }
        self.timer(self.now + interval, Action::RxSensors { conn, gen });
    }

    fn tx_sensors_tick(&mut self, conn: usize, gen: u64) {
        let Some(interval) = self
            .clients
            .get(&conn)
            .filter(|c| c.tx_gen == gen)
            .and_then(|c| c.tx_sensors)
        else {
            return;
        };
        let late = self
            .find(|f| matches!(f, Fault::TxSensorsLate { .. }).then_some(()))
            .is_some()
            && !self.fired.contains("TxSensorsLate:pushing");
        if self.running && !late {
            for t in 0..self.trx.len() {
                let s = &self.trx[t];
                let (power, swr) = match s.key {
                    Some(k) => {
                        let drive = if k.source == Source::Tune {
                            s.tune_drive
                        } else {
                            s.drive
                        };
                        (
                            self.meters.max_power_w * f64::from(drive) / 100.0,
                            self.meters.swr,
                        )
                    }
                    None => (0.0, 1.0),
                };
                let line = notice(
                    "tx_sensors",
                    [
                        t.to_string(),
                        format!("{:.1}", self.meters.mic_dbm),
                        format!("{power:.1}"),
                        format!("{power:.1}"),
                        format!("{swr:.1}"),
                    ],
                );
                self.send(To::One(conn), line);
            }
        }
        self.timer(self.now + interval, Action::TxSensors { conn, gen });
    }

    fn cmd_smeter(&mut self, who: Keyer, cmd: &Command) {
        if !self.dialect.rx_smeter_poll {
            return self.ignore(who, cmd, Why::Unsupported);
        }
        if cmd.args.len() != 2 {
            return self.ignore(who, cmd, Why::Arity);
        }
        let (Some(t), Some(ch)) = (self.trx_arg(cmd.arg(0)), self.channel_arg(cmd.arg(1))) else {
            return self.ignore(who, cmd, Why::Value);
        };
        let line = notice(
            "rx_smeter",
            [
                t.to_string(),
                ch.to_string(),
                format!("{:.1}", self.meters.smeter_dbm),
            ],
        );
        self.reply(who, line);
    }

    // ----- CW -----

    fn cmd_cw_setting(&mut self, who: Keyer, cmd: &Command) {
        let speed = cmd.name == "cw_macros_speed";
        match cmd.args.len() {
            0 => {
                let v = if speed {
                    self.keyer.speed
                } else {
                    self.keyer.delay
                };
                self.reply(who, notice(&cmd.name, [v]));
            }
            1 => {
                let range = if speed { 1..=100 } else { 0..=5_000 };
                let Some(v) = cmd.arg(0).parse::<u32>().ok().filter(|v| range.contains(v)) else {
                    return self.ignore(who, cmd, Why::Value);
                };
                if speed {
                    self.keyer.speed = v;
                } else {
                    self.keyer.delay = v;
                }
                self.broadcast(notice(&cmd.name, [v]));
            }
            _ => self.ignore(who, cmd, Why::Arity),
        }
    }

    fn cmd_cw_step(&mut self, who: Keyer, cmd: &Command) {
        if cmd.args.len() != 1 {
            return self.ignore(who, cmd, Why::Arity);
        }
        let Some(step) = cmd.arg(0).parse::<u32>().ok().filter(|s| *s <= 100) else {
            return self.ignore(who, cmd, Why::Value);
        };
        self.keyer.speed = if cmd.name.ends_with("up") {
            (self.keyer.speed + step).min(100)
        } else {
            self.keyer.speed.saturating_sub(step).max(1)
        };
        let speed = self.keyer.speed;
        self.broadcast(notice("cw_macros_speed", [speed]));
    }

    fn cmd_cw_terminal(&mut self, who: Keyer, cmd: &Command) {
        if cmd.args.is_empty() {
            let on = self.keyer.terminal;
            return self.reply(who, notice("cw_terminal", [flag(on)]));
        }
        let Some(on) = (cmd.args.len() == 1)
            .then(|| wire::boolean(cmd.arg(0)))
            .flatten()
        else {
            return self.ignore(who, cmd, Why::Value);
        };
        self.keyer.terminal = on;
        self.broadcast(notice("cw_terminal", [flag(on)]));
        if !on && self.keyer.job.is_none() {
            // Terminal mode off with the queue empty: the transceiver goes back to receive.
            if let Some(t) = self
                .trx
                .iter()
                .position(|s| s.key.is_some_and(|k| k.source == Source::Cw))
            {
                self.cw_release(t);
            }
        }
    }

    fn cmd_cw_macros(&mut self, who: Keyer, cmd: &Command) {
        if cmd.args.len() < 2 {
            return self.ignore(who, cmd, Why::Arity);
        }
        let Some(t) = self.trx_arg(cmd.arg(0)) else {
            return self.ignore(who, cmd, Why::Value);
        };
        let text = wire::cw_text(&cmd.args[1..].join(","));
        if self.keyer.job.is_some() {
            // Added in line after what is being sent (spec §3.5).
            self.keyer.queue.push_back((t as u32, who, text));
            return;
        }
        if !self.cw_may_key(t, who, &cmd.text) {
            return;
        }
        self.cw_start(t, who, text, None);
    }

    fn cmd_cw_msg(&mut self, who: Keyer, cmd: &Command) {
        match cmd.args.len() {
            1 => {
                // A callsign correction, while the callsign has not started.
                let call = cmd.arg(0).to_string();
                let now = self.now;
                let Some(job) = self.keyer.job.clone().filter(|j| j.message.is_some()) else {
                    return self.refuse(who, &cmd.text, Refusal::TooLate);
                };
                let elapsed_ms = now.saturating_duration_since(job.start).as_secs_f64() * 1000.0;
                let started = job.call_span.is_none_or(|(start, _)| elapsed_ms >= start);
                if started {
                    return self.refuse(who, &cmd.text, Refusal::TooLate);
                }
                let (base, times) = split_repeat(&call);
                let message = Message {
                    call: base,
                    times,
                    ..job.message.clone().unwrap_or_default()
                };
                self.keyer.job = None;
                self.log(Event::CwEnded {
                    trx: job.trx,
                    stopped: true,
                });
                self.cw_start_at(
                    job.trx as usize,
                    job.by,
                    String::new(),
                    Some(message),
                    job.start,
                );
            }
            4 => {
                let Some(t) = self.trx_arg(cmd.arg(0)) else {
                    return self.ignore(who, cmd, Why::Value);
                };
                if self.keyer.job.is_none() && !self.cw_may_key(t, who, &cmd.text) {
                    return;
                }
                // A message has priority: what is being sent stops and the message starts.
                if let Some(job) = self.keyer.job.take() {
                    self.log(Event::CwEnded {
                        trx: job.trx,
                        stopped: true,
                    });
                }
                self.keyer.queue.clear();
                let part = |s: &str| {
                    if s == "_" {
                        String::new()
                    } else {
                        wire::cw_text(s)
                    }
                };
                let (call, times) = split_repeat(&wire::cw_text(cmd.arg(2)));
                let message = Message {
                    prefix: part(cmd.arg(1)),
                    call,
                    times,
                    suffix: part(cmd.arg(3)),
                };
                self.cw_start(t, who, String::new(), Some(message));
            }
            _ => self.ignore(who, cmd, Why::Arity),
        }
    }

    /// The radio's checks before CW keys it: the key checks, and nothing else holding it.
    fn cw_may_key(&mut self, t: usize, who: Keyer, text: &str) -> bool {
        if self.trx[t].key.is_some_and(|k| k.source != Source::Cw) {
            self.refuse(who, text, Refusal::Busy);
            return false;
        }
        self.admit(t, who, text, false)
    }

    fn cw_start(&mut self, t: usize, who: Keyer, text: String, message: Option<Message>) {
        let keyed = self.trx[t].key.is_some_and(|k| k.source == Source::Cw);
        if !keyed {
            self.take_key(t, who, Source::Cw);
        }
        let delay = if keyed {
            0
        } else {
            u64::from(self.keyer.delay)
        };
        let start = self.now + Duration::from_millis(delay);
        self.cw_start_at(t, who, text, message, start);
    }

    fn cw_start_at(
        &mut self,
        t: usize,
        who: Keyer,
        text: String,
        message: Option<Message>,
        start: Instant,
    ) {
        let (text, call_span) = match &message {
            None => (text, None),
            Some(m) => {
                let (prefix, call, suffix) = (&m.prefix, m.calls(), &m.suffix);
                let full = [prefix.as_str(), call.as_str(), suffix.as_str()]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" ");
                let schedule = cw::schedule(&full, self.keyer.speed);
                let before = cw::schedule(prefix, self.keyer.speed).marks.len();
                let count = cw::schedule(&call, self.keyer.speed).marks.len();
                let span = (count > 0).then(|| {
                    (
                        schedule.marks[before].start_ms,
                        schedule.marks[before + count - 1].end_ms,
                    )
                });
                (full, span)
            }
        };
        let schedule = cw::schedule(&text, self.keyer.speed);
        let id = self.next();
        let total = schedule.total_ms;
        let last_start = schedule.marks.last().map_or(0.0, |m| m.start_ms);
        self.log(Event::CwStarted {
            trx: t as u32,
            text: text.clone(),
            ms: total.round() as u64,
        });
        let ms = |v: f64| Duration::from_secs_f64(v / 1000.0);
        self.timer(start + ms(total), Action::CwEnd { job: id });
        if self.keyer.terminal {
            self.timer(start + ms(last_start), Action::CwEmpty { job: id });
        }
        if let Some((_, end)) = call_span {
            self.timer(start + ms(end), Action::CwCallsign { job: id });
        }
        self.keyer.job = Some(CwJob {
            id,
            trx: t as u32,
            by: who,
            message,
            start,
            schedule,
            call_span,
        });
    }

    fn cw_end(&mut self, id: u64) {
        let Some(job) = self.keyer.job.clone().filter(|j| j.id == id) else {
            return;
        };
        self.keyer.job = None;
        self.log(Event::CwEnded {
            trx: job.trx,
            stopped: false,
        });
        if let Some((t, who, text)) = self.keyer.queue.pop_front() {
            return self.cw_start(t as usize, who, text, None);
        }
        if self.keyer.terminal {
            return; // terminal mode: the transceiver stays in transmit
        }
        self.cw_release(job.trx as usize);
    }

    /// The CW is done: unkey, unless [`Fault::HoldsCw`] keeps the radio keyed.
    fn cw_release(&mut self, t: usize) {
        if self.has(|f| matches!(f, Fault::HoldsCw)) {
            self.log(Event::Fault {
                name: "HoldsCw",
                conn: None,
                detail: format!("transceiver {t} stays keyed after its CW"),
            });
            return;
        }
        if self.trx[t].key.is_some_and(|k| k.source == Source::Cw) {
            self.unkey_now(t, Cause::CwDone, true);
        }
    }

    /// `cw_macros_stop;`: stop at once, drop what is queued, and unkey.
    fn cw_stop(&mut self, _who: Keyer) {
        self.cw_cancel();
        if let Some(t) = self
            .trx
            .iter()
            .position(|s| s.key.is_some_and(|k| k.source == Source::Cw))
        {
            self.cw_release(t);
        }
    }

    /// End what the keyer is sending and drop the queue.
    fn cw_cancel(&mut self) {
        if let Some(job) = self.keyer.job.take() {
            self.log(Event::CwEnded {
                trx: job.trx,
                stopped: true,
            });
        }
        self.keyer.queue.clear();
    }

    // ----- Spots, identity, generic parameters -----

    fn cmd_spot(&mut self, who: Keyer, cmd: &Command) {
        let by = match who {
            Keyer::Client(c) => c,
            Keyer::Operator => usize::MAX,
        };
        let store = self.dialect.spot_store;
        match (cmd.name.as_str(), cmd.args.len()) {
            ("spot", 4 | 5) => {
                let Ok(hz) = cmd.arg(2).parse::<u64>() else {
                    return self.ignore(who, cmd, Why::Value);
                };
                let colour = cmd.arg(3);
                let (argb, drawn) = match store {
                    SpotStore::PerCall => match colour.parse::<u32>() {
                        Ok(argb) => (argb, Some(argb)),
                        Err(_) => return self.ignore(who, cmd, Why::Value),
                    },
                    SpotStore::Appended => match hash_colour(colour) {
                        Some(argb) => (argb, Some(argb)),
                        None => (colour.parse().unwrap_or(0), None),
                    },
                };
                let call = cmd.arg(0).to_string();
                if call.is_empty() {
                    return self.ignore(who, cmd, Why::Value);
                }
                if store == SpotStore::PerCall {
                    self.spots.retain(|s| !s.call.eq_ignore_ascii_case(&call));
                }
                self.spots.push(Spot {
                    call,
                    mode: cmd.arg(1).to_string(),
                    hz,
                    argb,
                    text: cmd.arg(4).to_string(),
                    by,
                    drawn,
                });
                if self.spots.len() > MAX_SPOTS {
                    self.spots.remove(0);
                }
            }
            ("spot_delete", 1) if store == SpotStore::PerCall => {
                let call = cmd.arg(0);
                self.spots.retain(|s| !s.call.eq_ignore_ascii_case(call));
            }
            ("spot_delete", 2) if store == SpotStore::Appended => {
                let Ok(hz) = cmd.arg(1).parse::<u64>() else {
                    return self.ignore(who, cmd, Why::Value);
                };
                let call = cmd.arg(0);
                if let Some(i) = self
                    .spots
                    .iter()
                    .position(|s| s.call == call && s.hz.abs_diff(hz) < 100)
                {
                    self.spots.remove(i);
                }
            }
            ("spot_clear", 0) => self.spots.clear(),
            _ => self.ignore(who, cmd, Why::Arity),
        }
    }

    fn cmd_vfo_lock(&mut self, who: Keyer, cmd: &Command) {
        if cmd.args.len() != 2 {
            return self.ignore(who, cmd, Why::ServerOnly);
        }
        let (Some(t), Some(ch)) = (self.trx_arg(cmd.arg(0)), self.channel_arg(cmd.arg(1))) else {
            return self.ignore(who, cmd, Why::Value);
        };
        self.reply(
            who,
            notice(
                "vfo_lock",
                [t.to_string(), ch.to_string(), "false".to_string()],
            ),
        );
    }

    fn cmd_identity(&mut self, who: Keyer, cmd: &Command) {
        if !cmd.args.is_empty() {
            return self.ignore(who, cmd, Why::ServerOnly);
        }
        let ident = match cmd.name.as_str() {
            "vfo_limits" => Ident::VfoLimits,
            "if_limits" => Ident::IfLimits,
            "trx_count" => Ident::TrxCount,
            "channel_count" | "channels_count" => Ident::ChannelCount,
            "device" => Ident::Device,
            "receive_only" => Ident::ReceiveOnly,
            "modulations_list" => Ident::Modulations,
            _ => Ident::Protocol,
        };
        for line in self.identity_lines(ident) {
            self.reply(who, line);
        }
    }

    fn store_param(&mut self, cmd: &Command, keys: usize) {
        let key = format!("{}:{}", cmd.name, cmd.args[..keys].join(","));
        let line = notice(&cmd.name, cmd.args.iter());
        match self
            .params
            .iter_mut()
            .find(|(k, _)| k.as_deref() == Some(key.as_str()))
        {
            Some(entry) => entry.1 = line,
            None => self.params.push((Some(key), line)),
        }
    }

    fn cmd_generic(&mut self, who: Keyer, cmd: &Command, keys: usize) {
        let n = cmd.args.len();
        if n < keys {
            return self.ignore(who, cmd, Why::Arity);
        }
        let key = format!("{}:{}", cmd.name, cmd.args[..keys].join(","));
        if n == keys {
            if let Some((_, line)) = self
                .params
                .iter()
                .find(|(k, _)| k.as_deref() == Some(key.as_str()))
                .cloned()
            {
                self.reply(who, line);
            }
            return;
        }
        if !self.may_set(who, &key, cmd) {
            return;
        }
        self.store_param(cmd, keys);
        self.hold(who, key);
        self.broadcast(notice(&cmd.name, cmd.args.iter()));
    }

    fn tx_frequency_of(&self, t: usize) -> u64 {
        let s = &self.trx[t];
        let base = if s.split { s.vfo[1] } else { s.vfo[0] };
        let xit = if s.xit { s.xit_offset } else { 0 };
        base.saturating_add_signed(xit)
    }

    /// `tx_frequency:<hz>;` when transceiver 0's transmit frequency moved.
    fn tx_frequency_changed(&mut self, t: usize) {
        if t != 0 || !self.dialect.tx_frequency {
            return;
        }
        let f = self.tx_frequency_of(0);
        if self.last_tx_frequency != Some(f) {
            self.last_tx_frequency = Some(f);
            self.broadcast(notice("tx_frequency", [f]));
        }
    }
}

/// The key-argument count of an unmodelled parameter, if `name` is one.
fn generic_keys(name: &str) -> Option<usize> {
    GENERIC.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)
}

/// A callsign and its repeat count (`RA6LH$2` is RA6LH twice, spec §3.2.2).
fn split_repeat(call: &str) -> (String, usize) {
    match call.rsplit_once('$') {
        Some((base, n)) => match n.parse::<usize>() {
            Ok(n) if (1..=9).contains(&n) && !base.is_empty() => (base.to_string(), n),
            _ => (call.to_string(), 1),
        },
        None => (call.to_string(), 1),
    }
}

#[cfg(test)]
mod tests;
