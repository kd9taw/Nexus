//! Nexus as a TCI server, the app's side: the engine's answers to the
//! programs on this computer ([`EngineTci`]), the manager that hot-applies the switch, the state
//! programs are told, the receive audio they are sent, the transmit-audio requests (TX_CHRONO), the
//! connection log, and the server's line in the snapshot. The protocol, the listener and its guards
//! (loopback only, no web pages, the bounds) are `tempo_net::tci::server`'s; this file never touches
//! a socket, and nothing here can make the server listen anywhere but `127.0.0.1` and `::1`.
//!
//! WHAT A PROGRAM MAY DO, AND WHAT IT MAY NOT:
//! - **A program transmits only through the engine's TCI rules** (`tempo_app`'s `engine/tci.rs`).
//!   ⛔ Its key and its Tune are the engine's verdict and nothing else: "Programs on TCI may
//!   transmit" (the station's setting, off by default, changed only at the shack), then every gate
//!   in front of Nexus's own transmissions, then one owner of the transmitter, then the section
//!   (Phone for an over of its audio or the radio's microphone; FT8, FT4 and the other slot-timed
//!   modes refused until the loop bench, the operator's ruling of 2026-10-19). A refusal is told as
//!   TCI tells any key that did not take (`trx:0,false;`, `tune:0,false;`), and each program's
//!   `tx_enable` is the same verdict. Nothing a program sends arms transmit, lets go of the
//!   operator's Stop or a watchdog trip, or raises the power.
//! - **A program's CW** (`cw_macros`, `cw_msg`) is the engine's verdict too, in the CW section only,
//!   then the engine's text filter; Nexus's own CW keyer sends it at the programs' speed, which a
//!   program sets inside the keyer's range and which is never the operator's own.
//! - **A program's over** is armed by its key, fed by its transmit audio (read here into 12 kHz
//!   mono with the engine lock released, and taken by the engine only from the over's owner), keyed
//!   and re-gated by the radio loop on every tick, and asked for its audio by TX_CHRONO.
//! - **Every stop wins.** Stop TX, TX Off and the switch going off end a program's key in the
//!   engine; the next publish tells the server the program owns nothing, so it takes no more of
//!   its audio. A program's lost connection ends what it held at once, hard.
//! - **Every stop a program sends is honoured, ungated.** `trx:0,false` from the key's owner ends
//!   its over gracefully (what it sent plays out); from a program that keyed nothing it stops
//!   whatever is on the air, as WSJT-X's UDP Halt Tx does; `stop;` ends the
//!   program's own key and Tune; `tune:0,false` ends Tune whoever started it; `cw_macros_stop`
//!   ends CW from any origin.
//! - **The dial and the mode go through the CAT broker's own rules** (`shared_retune`,
//!   `shared_mode_change` in `lib.rs`): refused while anything is on the air, and the dial
//!   for 200 ms after a change no program made (the spec's §3.5: the radio program's own change
//!   holds the parameter that long).
//! - **Power only down**, through `set_rf_power`, so the operator's ceiling still holds.
//! - **A program's spots are marks on Nexus's band map, and nothing more** (`tempo_app`'s
//!   `engine/tci_spots.rs`): kept with the program that sent them while "Spots from programs" is
//!   on, bounded and aged there, gone with the program, the switch or the server. ⛔ A click on any
//!   spot in Nexus is queued by the engine, and the manager tells every program of it
//!   (`clicked_on_spot` and `rx_clicked_on_spot`, [`tell_click`]), which is all it does here; a
//!   click on a program's spot only tunes (`Engine::tci_program_spot_clicked`): never a section
//!   change, an arm, a QSO, or a Stop let go.
//!
//! THREADS AND LOCKS:
//! - The manager (`tci-server`) reads the switch, starts and stops the server, and while it serves
//!   builds the station's state AND publishes it under the engine lock, 20 times a second. That is
//!   tempo-net's contract: a snapshot built before a command and published after its answer would
//!   undo the answer's echo. It stops the server with the engine lock released, because
//!   `Server::stop` ends each program's transmissions through this backend, which takes that lock.
//! - The pump (`tci-rx-audio`) sends receive audio, from the server's own copy of the capture, and
//!   TX_CHRONO. It never takes the engine lock, so a CAT stall at the radio loop never stalls a
//!   program's audio.
//! - The server's reader threads call [`EngineTci`], which takes the engine lock briefly, as the
//!   CAT broker's backend does.
//! - The operator's Disconnect ([`disconnect`]) never holds the engine lock either.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tempo_app::dto::{TciProgramStatus, TciServerStatus};
use tempo_app::engine::{engine_lock, Engine, MeterFeed};
use tempo_audio::receive_audio::{ReceiveAudioFeed, ReceiveAudioReader};
use tempo_audio::tci_audio::{Format, FromProgram, Stream, ToProgram, TxHeader};
use tempo_net::tci::frame::{
    self, BlockHeader, BlockKind, Chrono, LengthMeaning, RawHeader, HEADER_LEN,
};
use tempo_net::tci::server::{
    AudioFormat, ClientId, Closed, Config, Cw, CwSpeed, Event, Handle, HandshakeRefusal, KeyKind,
    Limits, Program, Refusal, Server, Snapshot, Spot, TciBackend, UNKNOWN_LIMITS,
};
use tempo_net::tci::wire::{Call, Hz, Word, MAX_HZ};
use tempo_net::tci::Trx;

use crate::{conn_log, SharedEngine};

/// The connection log's lane.
const LANE: &str = "TCI server";
/// How often the manager publishes the station while the server runs: 20 times a second, the most
/// the publisher is allowed.
const TICK: Duration = Duration::from_millis(50);
/// …and how often it reads the switch while the server is not running.
const IDLE_TICK: Duration = Duration::from_millis(1000);
/// A port whose bind failed is tried again after this; a different port at once.
const BIND_RETRY: Duration = Duration::from_secs(30);
/// How often the pump looks for receive audio to send and transmit audio to ask for.
const PUMP_TICK: Duration = Duration::from_millis(10);
/// A program's dial write is refused this long after a change no program made (the spec's §3.5).
const OPERATOR_HOLD: Duration = Duration::from_millis(200);
/// Nexus's own transmit buffering, told as `tx_stream_audio_buffering`: the TCI over's lead-in
/// (60 ms) and look-ahead (40 ms), the Remote microphone's numbers.
const TX_BUFFERING_MS: u32 = 100;
/// What an armed over asks for ahead of real time: the same buffering.
const LEAD: Duration = Duration::from_millis(TX_BUFFERING_MS as u64);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The port the operator asked the server to serve on (`Settings::tci_server_port`), or `None`
/// while "Share by TCI" (`Settings::tci_server`) is off, its default.
fn wanted(e: &Engine) -> Option<u16> {
    let s = e.settings();
    s.tci_server.then_some(s.tci_server_port)
}

/// The connected program the engine names by its number (`ClientId::get`), if it is connected.
fn program_id(programs: &[Program], n: Option<u64>) -> Option<ClientId> {
    let n = n?;
    programs.iter().map(|p| p.id).find(|id| id.get() == n)
}

/// The program whose TCI over is armed, keyed or playing out: the one the pump asks for transmit
/// audio. A microphone key or a program's Tune has no over to feed.
fn armed_over(e: &Engine, programs: &[Program]) -> Option<ClientId> {
    if !e.tci_over_armed() {
        return None;
    }
    program_id(programs, e.tci_owner())
}

/// The dial in Hz.
fn dial_hz(e: &Engine) -> i64 {
    (e.settings().dial_mhz * 1_000_000.0).round() as i64
}

fn hz(v: i64) -> Hz {
    Hz::new(v.clamp(-MAX_HZ, MAX_HZ)).unwrap_or_else(|| unreachable!("clamped to the range"))
}

/// A 0.0–1.0 power as TCI's 0–100.
fn percent(frac: f32) -> u32 {
    (frac * 100.0).round().clamp(0.0, 100.0) as u32
}

/// The power the station is on, as `drive`: commanded, else read back from the radio; 0 while
/// Nexus knows neither (TCI has no word for "unknown").
fn drive(e: &Engine) -> u32 {
    e.effective_rf_power().map(percent).unwrap_or(0)
}

/// The power a Tune keys at, as `tune_drive`: the Tune power, never above the operator's own level
/// (`Settings::tune_power_pct`, applied as the lower of the two), or that level when it is unset.
fn tune_drive(e: &Engine) -> u32 {
    let tune = e.settings().tune_power_pct.map(u32::from);
    match (tune, e.effective_rf_power().map(percent)) {
        (Some(t), Some(d)) => t.min(d),
        (Some(t), None) => t.min(100),
        (None, Some(d)) => d,
        (None, None) => 0,
    }
}

/// The mode the radio was commanded, as a TCI word ([`tempo_net::tci::server::MODULATIONS`]). The
/// CAT broker's reading of the same mode (`EngineRig::mode`), with its fallback to the sideband.
fn tci_mode(e: &Engine) -> Word {
    let commanded = e.rig_mode_effective();
    let commanded = if commanded.trim().is_empty() {
        e.settings().sideband.clone()
    } else {
        commanded
    };
    use crate::CatFamily as F;
    let word = match crate::parse_cat_mode(&commanded) {
        Some(m) => match (m.family, m.lsb) {
            (F::Ssb, Some(true)) => "lsb",
            (F::Ssb, _) => "usb",
            (F::Data, Some(true)) => "digl",
            (F::Data, _) => "digu",
            (F::Cw, _) => "cw",
            // TCI has no RTTY: Nexus's AFSK keys on the lower sideband, and FSK's mark is the dial.
            (F::Rtty, _) => "digl",
            (F::Am, _) => "am",
            (F::Fm | F::DataFm, _) => "nfm",
        },
        None => "usb",
    };
    Word::new(word).unwrap_or_else(|| unreachable!("a TCI mode word"))
}

/// The receive level in dBm from the S-meter's dB over S9: S9 is −73 dBm below 30 MHz and −93 dBm
/// at and above (IARU Region 1 Technical Recommendation R.1), as the radios' own meters are scaled.
fn rx_dbm(dial: i64, over_s9_db: i32) -> f64 {
    let s9 = if dial < 30_000_000 { -73.0 } else { -93.0 };
    s9 + f64::from(over_s9_db)
}

/// The station as programs are told it, read from the engine; the caller holds the engine lock.
fn station(e: &Engine, programs: &[Program], smeter_db: Option<i32>) -> Snapshot {
    let dial = dial_hz(e);
    Snapshot {
        radio: u64::from(e.settings().active_radio),
        // Nexus has the radio's own range only as a yes or no per frequency (`rig_covers_mhz`,
        // which a program's dial write asks), so programs are told the wide one.
        vfo_limits: (hz(UNKNOWN_LIMITS.0), hz(UNKNOWN_LIMITS.1)),
        receive_only: e.connection_tx_refusal().is_some(),
        dial: hz(dial),
        mode: tci_mode(e),
        drive: drive(e),
        tune_drive: tune_drive(e),
        // A program's key counts from its admission, armed or keyed: the engine's arbiter holds it
        // (tempo-net's contract 2).
        trx: e.on_air(),
        tune: e.tuning(),
        // ⛔ The program whose key or Tune owns the transmitter, as the engine holds it (contract
        // 3): once a stop has ended it, the server takes no more of its audio.
        owner: program_id(programs, e.tci_owner()),
        // The engine's verdict for a `tci` key from each program, the one its key gets (contract 4).
        tx_enable: programs
            .iter()
            .map(|p| (p.id, e.tci_tx_enable(p.id.get(), p.audio)))
            .collect(),
        // What programs' CW keys at: a program's own speed, else the operator's (never moved by one).
        cw_speed: e.tci_cw_wpm(),
        tx_buffering_ms: TX_BUFFERING_MS,
        rx_dbm: smeter_db.map(|db| rx_dbm(dial, db)),
        // Nexus reads its power and SWR only while keyed, has no engine accessor for them, and
        // reads no microphone level, which `tx_sensors` needs: not told yet.
        tx: None,
    }
}

/// The 200 ms rule's memory (the spec's §3.5): the dial as last seen, and when it last moved
/// without a program moving it, which is the operator's change (or Nexus's own: Doppler, a channel
/// park), and holds the dial against programs for [`OPERATOR_HOLD`].
#[derive(Default)]
struct DialWatch {
    seen: Option<i64>,
    moved_at: Option<Instant>,
}

impl DialWatch {
    /// Look at the dial: the manager every tick, and a program's write before it decides, so a
    /// change is seen by whichever looks first.
    fn look(&mut self, dial: i64, now: Instant) {
        if self.seen.is_some_and(|seen| seen != dial) {
            self.moved_at = Some(now);
        }
        self.seen = Some(dial);
    }

    /// A program moved it.
    fn moved_by_program(&mut self, dial: i64) {
        self.seen = Some(dial);
    }

    fn holds(&self, now: Instant) -> bool {
        self.moved_at
            .is_some_and(|at| now.saturating_duration_since(at) < OPERATOR_HOLD)
    }
}

/// The server's backend: every question a program's command asks, answered from the engine. See
/// the module header for what it admits; the trait's own header (tempo-net) for when it is called.
pub(crate) struct EngineTci {
    engine: SharedEngine,
    /// The server's copy of the receive audio: `audio_start` is answered from whether it has a
    /// source.
    feed: Arc<ReceiveAudioFeed>,
    dial: Mutex<DialWatch>,
    /// The last thing each connected program was refused, in the log's words, for the status.
    refused: Mutex<HashMap<ClientId, &'static str>>,
    /// The last line this lane wrote, so a line is never written twice in a row (the broker's
    /// rule, `refusal_is_new`).
    last_line: Mutex<Option<String>>,
    /// Each program's transmit audio as tempo-codec reads it: its filter and its budget, kept
    /// across its overs, the filter started afresh at each.
    from_programs: Mutex<HashMap<ClientId, FromProgram>>,
    /// Transmit-audio frames dropped: refused as bad or over budget, or not for an over the
    /// sender owns.
    tx_dropped: AtomicU64,
}

impl EngineTci {
    fn new(engine: SharedEngine, feed: Arc<ReceiveAudioFeed>) -> EngineTci {
        EngineTci {
            engine,
            feed,
            dial: Mutex::new(DialWatch::default()),
            refused: Mutex::new(HashMap::new()),
            last_line: Mutex::new(None),
            from_programs: Mutex::new(HashMap::new()),
            tx_dropped: AtomicU64::new(0),
        }
    }

    /// One transmit frame (its header, then the values it claims) as 12 kHz mono, read with the
    /// program's own stream; `None` when tempo-codec refuses it.
    fn read_tx(
        &self,
        client: ClientId,
        format: AudioFormat,
        frame: &[u8],
        now: Instant,
    ) -> Option<Vec<f32>> {
        let stream = stream_of(format)?;
        let head = RawHeader::read(frame)?;
        let header = TxHeader {
            receiver: head.receiver,
            sample_rate: head.sample_rate,
            format: head.format,
            codec: head.codec,
            length: head.length,
        };
        let payload = frame.get(HEADER_LEN..)?;
        let mut from = lock(&self.from_programs);
        from.entry(client)
            .or_default()
            .accept(stream, header, payload, now)
            .ok()
    }

    fn log(&self, level: &'static str, line: String) {
        if crate::refusal_is_new(&self.last_line, &line) {
            conn_log(LANE, level, line);
        }
    }
}

/// A connection-log line for what the server reports, and its level.
fn describe(event: &Event) -> (&'static str, String) {
    match event {
        Event::Connected { client, peer } => ("info", format!("{client} connected from {peer}")),
        Event::Closed { client, why } => {
            let (level, text) = match why {
                Closed::Program => ("info", "it closed the connection"),
                Closed::Stopped => ("info", "the server stopped"),
                Closed::Disconnected => ("info", "the operator disconnected it"),
                Closed::Lost => ("error", "the connection failed"),
                Closed::Silent => ("error", "it stopped answering"),
                Closed::Flood => ("error", "it sent commands too fast for too long"),
                Closed::TooBig => ("error", "it sent a message too large"),
                Closed::Garbage => ("error", "it sent messages that are not TCI"),
                Closed::Protocol => ("error", "it broke the WebSocket protocol"),
                Closed::Backlog => ("error", "it stopped reading what Nexus sent"),
            };
            (level, format!("{client} disconnected: {text}"))
        }
        Event::Refused { peer, why } => {
            let text = match why {
                HandshakeRefusal::Origin(origin) => format!(
                    "refused a web page ({origin}) from {peer}: only programs on this computer \
                     connect, never a browser page"
                ),
                HandshakeRefusal::Host(host) => format!(
                    "refused a connection from {peer} naming the host {host:?}: only 127.0.0.1, \
                     localhost or [::1] with the server's port"
                ),
                HandshakeRefusal::Version => {
                    format!("refused a connection from {peer}: not WebSocket version 13")
                }
                HandshakeRefusal::Malformed => {
                    format!("refused a connection from {peer}: not a WebSocket handshake")
                }
                HandshakeRefusal::Full => {
                    format!("refused a connection from {peer}: as many programs as Nexus takes are connected")
                }
                HandshakeRefusal::Busy => {
                    format!("refused a connection from {peer}: too many were opening at once")
                }
                HandshakeRefusal::Timeout => {
                    format!("closed a connection from {peer}: its handshake did not finish in time")
                }
                HandshakeRefusal::Incomplete => {
                    format!("a connection from {peer} ended before its handshake did")
                }
                HandshakeRefusal::NotLocal => {
                    format!("refused a connection from {peer}: only this computer may connect")
                }
            };
            ("error", text)
        }
        Event::Refusal { client, refusal } => {
            ("error", format!("refused {client}: {}", refusal.text()))
        }
        Event::CommandRefused { client, what } => (
            "info",
            format!(
                "{client} asked for {}, which Nexus does not offer through TCI",
                what.text()
            ),
        ),
    }
}

impl TciBackend for EngineTci {
    fn disconnected(&self, client: ClientId) {
        // ⛔ A LOST PROGRAM ENDS WHAT IT HELD, HARD, NOW: its over drops with
        // the abort armed in this call, so the radio loop flushes and unkeys on its next tick; its
        // microphone key and its Tune end at once. The server calls this after the program's
        // transmit calls in flight have returned and never makes another for it.
        // Its spots go with it, under the same hold of the engine lock.
        {
            let mut e = engine_lock(&self.engine);
            e.tci_disconnected(client.get());
            e.tci_program_spot_clear(client.get());
        }
        lock(&self.from_programs).remove(&client);
        lock(&self.refused).remove(&client);
    }

    fn event(&self, event: Event) {
        match &event {
            Event::Refusal { client, refusal } => {
                lock(&self.refused).insert(*client, refusal.text());
            }
            Event::CommandRefused { client, what } => {
                lock(&self.refused).insert(*client, what.text());
            }
            Event::Closed { client, .. } => {
                lock(&self.refused).remove(client);
            }
            Event::Connected { .. } | Event::Refused { .. } => {}
        }
        let (level, line) = describe(&event);
        self.log(level, line);
    }

    fn set_dial(&self, _client: ClientId, hz_asked: Hz) -> Result<Hz, Refusal> {
        let mut e = engine_lock(&self.engine);
        let now = Instant::now();
        let mut watch = lock(&self.dial);
        watch.look(dial_hz(&e), now);
        // Never under an over: a retune mid-over halts it. Asked and applied under
        // this one hold of the engine lock, so no key comes between the two.
        if let Some(refusal) = e.tci_retune_refusal() {
            return Err(refusal);
        }
        if watch.holds(now) {
            return Err(Refusal::OperatorChange);
        }
        let asked = u64::try_from(hz_asked.get()).map_err(|_| Refusal::Limits)?;
        if e.rig_covers_mhz(asked as f64 / 1_000_000.0) == Some(false) {
            return Err(Refusal::Limits);
        }
        crate::shared_retune(&mut e, asked);
        let dial = dial_hz(&e);
        watch.moved_by_program(dial);
        Ok(hz(dial))
    }

    fn set_mode(&self, client: ClientId, mode: &Word) -> Result<Word, Refusal> {
        let mut e = engine_lock(&self.engine);
        // What the program asked, whatever comes of it: a data mode holds its overs like FT8.
        e.tci_note_modulation(client.get(), mode);
        if let Some(refusal) = e.tci_retune_refusal() {
            return Err(refusal);
        }
        crate::shared_mode_change(&mut e, mode.as_str()).map_err(|_| Refusal::Family)?;
        Ok(tci_mode(&e))
    }

    fn set_drive(&self, _client: ClientId, level: u32) -> Result<u32, Refusal> {
        // Down only, through `set_rf_power`, so the operator's ceiling still binds.
        // A level Nexus does not know cannot be shown to be lower.
        engine_lock(&self.engine).tci_set_drive(level)
    }

    fn set_tune_drive(&self, _client: ClientId, level: u32) -> Result<u32, Refusal> {
        let e = engine_lock(&self.engine);
        let now = tune_drive(&e);
        match level.cmp(&now) {
            std::cmp::Ordering::Equal => Ok(now),
            std::cmp::Ordering::Greater => Err(Refusal::Raise),
            // Lower would rewrite the operator's own Tune power setting, and a program's Tune keys
            // at the station's: not offered.
            std::cmp::Ordering::Less => Err(Refusal::Tier),
        }
    }

    fn key(&self, client: ClientId, kind: KeyKind, audio_started: bool) -> Result<(), Refusal> {
        // ⛔ THE ENGINE'S VERDICT AND NOTHING ELSE: `Ok` arms an over of the
        // program's audio or holds the radio's microphone for it; a refusal names the first gate
        // that failed. A re-assert of the key it holds changes nothing.
        let program = client.get();
        let (verdict, fresh) = {
            let mut e = engine_lock(&self.engine);
            let held = e.tci_over_armed() && e.tci_owner() == Some(program);
            (e.tci_key(program, kind, audio_started), !held)
        };
        if verdict.is_ok() && kind == KeyKind::Tci && fresh {
            // A new over: what the filter holds of the program's last one is not this one's. Its
            // transmit audio comes from this same reader thread, so none is in flight now.
            if let Some(from) = lock(&self.from_programs).get_mut(&client) {
                from.reset();
            }
        }
        verdict
    }

    fn release(&self, client: ClientId) {
        // From the key's owner, the graceful end of its own over (what it sent plays out) or of
        // its microphone key or Tune. From any other program: what is on the air
        // stops, as for WSJT-X's Halt Tx, and holds until the operator arms.
        let halted = engine_lock(&self.engine).tci_release(client.get());
        if halted {
            self.log(
                "info",
                format!("{client} sent trx:0,false: Nexus stopped transmitting, as for Halt Tx"),
            );
        }
    }

    fn tune_on(&self, client: ClientId) -> Result<(), Refusal> {
        // The engine's verdict, the licence judged at the carrier; Nexus's tune timeout bounds it,
        // and it never starts or restarts the watchdog clock.
        engine_lock(&self.engine).tci_tune_on(client.get())
    }

    fn tune_off(&self, client: ClientId) {
        // Whoever started it. Nothing when nothing tunes: a release with nothing to
        // release still invalidates the over Nexus has planned.
        engine_lock(&self.engine).tci_tune_off(client.get());
    }

    fn end_own(&self, client: ClientId) {
        // `stop;`: the program's own key, over and Tune, hard, and nothing else.
        engine_lock(&self.engine).tci_end_own(client.get());
    }

    fn tx_audio(&self, client: ClientId, format: AudioFormat, frame: &[u8]) {
        // ⛔ AUDIO ONLY FOR OUR KEY: read with the engine lock released, then
        // offered to the engine, which takes it only for the over this program owns and has not
        // let go (tempo-net's contract 6: the over can end between the server's check and this).
        // Anything else is dropped, never kept for a later key.
        let arrived = Instant::now();
        let taken = self
            .read_tx(client, format, frame, arrived)
            .is_some_and(|mono| {
                engine_lock(&self.engine).tci_tx_audio(client.get(), &mono, arrived)
            });
        if !taken {
            self.tx_dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn audio_start(&self, _client: ClientId, _format: AudioFormat) -> Result<(), Refusal> {
        // What the decoders hear, through the server's own copy of it: the sound card's capture,
        // the Flex's DAX audio while it is the receive source, or the SDR program's on Nexus's
        // own TCI connection. The Icom network connection carries no audio, so there it is the
        // sound card's. With none of them, there is nothing to send.
        if self.feed.source().is_some() {
            Ok(())
        } else {
            Err(Refusal::NoSource)
        }
    }

    fn audio_stop(&self, _client: ClientId) {
        // The pump stops sending when the program's stream reads stopped.
    }

    fn cw(&self, client: ClientId, cw: Cw) -> Result<(), Refusal> {
        // ⛔ THE ENGINE'S VERDICT AND TEXT FILTER AND NOTHING ELSE: the switch and
        // the CW tier, every station gate, one owner, the CW section, then only text Nexus's keyer
        // can send; `Ok` queues it for Nexus's own CW keyer at the programs' speed. It never arms.
        engine_lock(&self.engine).tci_cw(client.get(), &cw)
    }

    fn cw_stop(&self, client: ClientId) {
        // From any origin, and CW only: the CW stop also cuts a slot over in flight.
        engine_lock(&self.engine).tci_cw_stop(client.get());
    }

    fn cw_speed(&self, _client: ClientId, speed: CwSpeed) -> Result<u32, Refusal> {
        // The programs' own speed, inside the keyer's range; never the operator's.
        engine_lock(&self.engine).tci_cw_speed(speed)
    }

    fn spot(&self, client: ClientId, spot: Spot) -> Result<(), Refusal> {
        // A mark on the band map with the program that sent it, and nothing else: it never tunes
        // and never keys. Refused while programs' spots are off, and outside the range programs are
        // told (`station`'s `vfo_limits`).
        engine_lock(&self.engine).tci_program_spot(client.get(), &spot, UNKNOWN_LIMITS)
    }

    fn spot_delete(&self, client: ClientId, call: &Call) {
        // The sender's own spot only.
        engine_lock(&self.engine).tci_program_spot_delete(client.get(), call.as_str());
    }

    fn spot_clear(&self, client: ClientId) {
        // The sender's own spots only.
        engine_lock(&self.engine).tci_program_spot_clear(client.get());
    }
}

/// A program's stream as tempo-codec's frames read it.
fn stream_of(f: AudioFormat) -> Option<Stream> {
    Stream::new(
        f.rate.hz(),
        Format::from_code(f.sample_type.code())?,
        u32::try_from(f.channels.count()).ok()?,
        f.samples,
    )
}

/// Receive audio as one program's frames: `mono` at `source_rate` in, every whole frame it
/// completes out, encoded in the program's own rate, format and channels.
fn program_frames(
    to: &mut ToProgram,
    format: AudioFormat,
    source_rate: u32,
    mono: &[f32],
) -> Vec<Vec<u8>> {
    let (Some(stream), Some(trx)) = (stream_of(format), Trx::new(0)) else {
        return Vec::new();
    };
    let header = BlockHeader {
        trx,
        kind: BlockKind::RxAudio,
        sample_rate: stream.rate(),
        format: format.sample_type,
        channels: format.channels,
    };
    to.push(source_rate, stream, mono)
        .into_iter()
        .filter_map(|values| frame::encode_block(&header, LengthMeaning::Values, &values).ok())
        .collect()
}

/// TX_CHRONO for one armed over: at the arm, the over's lead asked for at once; then one request
/// per frame's worth of real time. A turn that comes late asks for the lead again rather than for
/// the time it missed, so a program is never asked for more than the lead ahead of real time.
struct Pacer {
    client: ClientId,
    format: AudioFormat,
    chrono: Chrono,
    /// Frames each request asks for, and the stream's rate.
    frames: u64,
    rate: u64,
    since: Instant,
    /// Frames asked for since `since`.
    asked: u64,
}

impl Pacer {
    fn new(client: ClientId, format: AudioFormat, now: Instant) -> Option<Pacer> {
        let stream = stream_of(format)?;
        let chrono = Chrono {
            trx: Trx::new(0)?,
            sample_rate: stream.rate(),
            format: format.sample_type,
            channels: format.channels,
            length: u32::try_from(stream.values()).ok()?,
        };
        Some(Pacer {
            client,
            format,
            chrono,
            frames: u64::try_from(stream.frames()).ok()?.max(1),
            rate: u64::from(stream.rate()),
            since: now,
            asked: 0,
        })
    }

    /// How many requests are due at `now`.
    fn due(&mut self, now: Instant) -> u64 {
        let frames_in = |d: Duration| {
            u64::try_from(d.as_micros() * u128::from(self.rate) / 1_000_000).unwrap_or(u64::MAX)
        };
        let lead = frames_in(LEAD);
        let target = frames_in(now.saturating_duration_since(self.since)).saturating_add(lead);
        if target > self.asked.saturating_add(lead + self.frames) {
            self.asked = target - lead;
        }
        let n = target.saturating_sub(self.asked) / self.frames;
        self.asked += n * self.frames;
        n
    }
}

/// The pump's state between turns.
#[derive(Default)]
struct PumpState {
    reader: Option<ReceiveAudioReader>,
    streams: HashMap<ClientId, ToProgram>,
    pacer: Option<Pacer>,
}

impl PumpState {
    /// One turn: receive audio to each program that started it, then TX_CHRONO to the program
    /// whose over is `armed`, if any.
    fn turn(
        &mut self,
        handle: &Handle,
        feed: &Arc<ReceiveAudioFeed>,
        armed: Option<ClientId>,
        now: Instant,
    ) {
        let programs = handle.programs();
        self.receive(handle, feed, &programs, now);
        self.request(handle, &programs, armed, now);
    }

    /// Receive audio from the server's copy of the capture. Nothing is copied while no program
    /// listens: the reader is let go, and the feed idles.
    fn receive(
        &mut self,
        handle: &Handle,
        feed: &Arc<ReceiveAudioFeed>,
        programs: &[Program],
        now: Instant,
    ) {
        let listening: Vec<&Program> = programs.iter().filter(|p| p.audio).collect();
        self.streams
            .retain(|id, _| listening.iter().any(|p| p.id == *id));
        if listening.is_empty() {
            self.reader = None;
            return;
        }
        if self.reader.is_none() {
            self.reader = feed.source().and_then(|source| feed.subscribe(source).ok());
        }
        let Some(reader) = &self.reader else {
            return;
        };
        let mut ended = false;
        loop {
            match reader.read(now) {
                Ok(Some(block)) => {
                    for p in &listening {
                        let to = self.streams.entry(p.id).or_default();
                        for bytes in program_frames(to, p.format, block.source.rate, &block.samples)
                        {
                            handle.send_rx_audio(p.id, bytes);
                        }
                    }
                }
                Ok(None) => break,
                // The capture changed or closed: subscribe again on a later turn, to whatever
                // it is then.
                Err(_) => {
                    ended = true;
                    break;
                }
            }
        }
        if ended {
            self.reader = None;
        }
    }

    /// TX_CHRONO to the program whose over is armed and to no one else: the engine says whose over
    /// is armed (`armed`), the server says whose `tci` key it admitted, and both must name it.
    fn request(
        &mut self,
        handle: &Handle,
        programs: &[Program],
        armed: Option<ClientId>,
        now: Instant,
    ) {
        let owner = armed.and_then(|client| {
            programs
                .iter()
                .find(|p| p.id == client && p.key == Some(KeyKind::Tci))
        });
        let Some(owner) = owner else {
            self.pacer = None;
            return;
        };
        if !self
            .pacer
            .as_ref()
            .is_some_and(|p| p.client == owner.id && p.format == owner.format)
        {
            self.pacer = Pacer::new(owner.id, owner.format, now);
        }
        if let Some(pacer) = self.pacer.as_mut() {
            for _ in 0..pacer.due(now) {
                handle.send_chrono(pacer.client, &pacer.chrono);
            }
        }
    }
}

/// The pump's thread, while the server runs.
struct Pump {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Pump {
    fn start(
        handle: Handle,
        feed: Arc<ReceiveAudioFeed>,
        armed: Arc<Mutex<Option<ClientId>>>,
    ) -> Pump {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("tci-rx-audio".into())
            .spawn(move || {
                let mut pump = PumpState::default();
                while !flag.load(Ordering::Relaxed) {
                    let armed = *lock(&armed);
                    pump.turn(&handle, &feed, armed, Instant::now());
                    std::thread::sleep(PUMP_TICK);
                }
            })
            .ok();
        Pump { stop, thread }
    }

    fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The running server's handle, for the operator's Disconnect; `None` while it is off.
#[derive(Default)]
pub(crate) struct Serving(Mutex<Option<Handle>>);

impl Serving {
    fn disconnect(&self, id: u64) -> bool {
        // The handle is taken out and the lock let go before the server is called.
        let handle = lock(&self.0).clone();
        let Some(handle) = handle else {
            return false;
        };
        match handle.programs().into_iter().find(|p| p.id.get() == id) {
            Some(p) => {
                handle.disconnect(p.id);
                true
            }
            None => false,
        }
    }
}

/// The server the running program's manager serves, for [`disconnect`].
static SERVING: OnceLock<Arc<Serving>> = OnceLock::new();

/// The operator's Disconnect: program `id` (its number in the snapshot's status) has what it keyed
/// ended, then it is closed. Never under the engine lock (tempo-net's lock rule).
pub(crate) fn disconnect(id: u64) -> bool {
    SERVING.get().is_some_and(|serving| serving.disconnect(id))
}

/// ⛔ Tell every program of the operator's click on a spot in Nexus, `call` at `hz` (the spot's
/// own frequency), in both spellings (`clicked_on_spot:<call>,<hz>;` and
/// `rx_clicked_on_spot:0,0,<call>,<hz>;`), and do nothing else: telling never tunes, arms or keys,
/// and never reaches the backend, so the manager tells under the engine lock as it publishes.
/// The engine queued the click only while programs' spots are on and only for a callsign
/// (`Engine::take_tci_spot_clicks`).
fn tell_click(handle: &Handle, call: &str, hz: i64) {
    if let (Some(call), Some(hz)) = (Call::new(call), Hz::new(hz)) {
        handle.spot_clicked(&call, hz);
    }
}

/// A running server: the port the operator asked for (it may be serving on the next one), the
/// server, its pump.
struct Running {
    asked: u16,
    server: Server,
    pump: Pump,
}

/// A bind that failed, so it is neither retried nor reported every tick.
struct Failed {
    port: u16,
    at: Instant,
    why: String,
}

/// The sentence for a server that could not bind.
fn bind_message(port: u16, err: &std::io::Error) -> String {
    let next = port.saturating_add(1);
    if err.kind() == std::io::ErrorKind::AddrInUse {
        format!(
            "Programs cannot connect by TCI: ports {port} and {next} on this computer are taken, \
             usually by an SDR program's own TCI server or two other Nexus windows. Close the one \
             you do not need, or give Nexus's TCI server a port nothing else uses. Retrying quietly."
        )
    } else {
        format!(
            "Programs cannot connect by TCI: this computer refused port {port} ({err}). Give \
             Nexus's TCI server another port. Retrying quietly."
        )
    }
}

/// The manager: hot-applies the switch and the port, and publishes the station while it serves.
pub(crate) struct Manager {
    engine: SharedEngine,
    feed: Arc<ReceiveAudioFeed>,
    meters: MeterFeed,
    backend: Arc<EngineTci>,
    /// The program whose over is armed, as the engine said it at the last tick, for the pump.
    armed: Arc<Mutex<Option<ClientId>>>,
    serving: Arc<Serving>,
    running: Option<Running>,
    failed: Option<Failed>,
    retry: Duration,
}

impl Manager {
    fn new(engine: SharedEngine, feed: Arc<ReceiveAudioFeed>, meters: MeterFeed) -> Manager {
        Manager {
            backend: Arc::new(EngineTci::new(engine.clone(), feed.clone())),
            engine,
            feed,
            meters,
            armed: Arc::new(Mutex::new(None)),
            serving: Arc::new(Serving::default()),
            running: None,
            failed: None,
            retry: BIND_RETRY,
        }
    }

    /// One look: the switch, then the station. Whether the server is running.
    fn tick(&mut self) -> bool {
        let want = {
            let mut e = engine_lock(&self.engine);
            // Programs' spots age out, and all go while they are off, the clicks waiting with them.
            e.tci_program_spots_tick(crate::now_unix());
            wanted(&e)
        };
        self.apply(want);
        self.publish();
        self.running.is_some()
    }

    /// Start, stop or move the server to what the operator asked for. Called with the engine lock
    /// released: stopping a server ends its programs' transmissions through the backend.
    fn apply(&mut self, want: Option<u16>) {
        if want == self.running.as_ref().map(|r| r.asked) {
            return;
        }
        if let Some(running) = self.running.take() {
            self.stop(running);
        }
        let Some(port) = want else {
            self.failed = None;
            return;
        };
        if self
            .failed
            .as_ref()
            .is_some_and(|f| f.port == port && f.at.elapsed() < self.retry)
        {
            return;
        }
        let first = station(&engine_lock(&self.engine), &[], self.meters.smeter_db());
        let config = Config {
            port,
            limits: Limits::default(),
        };
        let backend: Arc<dyn TciBackend> = self.backend.clone();
        match Server::start(config, backend, first) {
            Ok(server) => {
                let handle = server.handle();
                let pump = Pump::start(handle.clone(), self.feed.clone(), self.armed.clone());
                *lock(&self.serving.0) = Some(handle);
                conn_log(
                    LANE,
                    "info",
                    format!(
                        "programs on this computer can connect by TCI to 127.0.0.1:{}",
                        server.port()
                    ),
                );
                self.failed = None;
                self.running = Some(Running {
                    asked: port,
                    server,
                    pump,
                });
            }
            Err(err) => {
                let why = bind_message(port, &err);
                if self.failed.as_ref().is_none_or(|f| f.port != port) {
                    tempo_core::applog::error("tci", &why);
                    conn_log(LANE, "error", why.clone());
                }
                self.failed = Some(Failed {
                    port,
                    at: Instant::now(),
                    why,
                });
            }
        }
    }

    /// Stop a server: the Disconnect can no longer reach it, the pump stops, then the server ends
    /// every program's transmissions, tells each `trx:0,false;` and closes (tempo-net's order).
    fn stop(&mut self, running: Running) {
        *lock(&self.serving.0) = None;
        running.pump.stop();
        // Each program's end takes its spots with it (`EngineTci::disconnected`).
        running.server.stop();
        *lock(&self.armed) = None;
        conn_log(
            LANE,
            "info",
            "stopped: programs can no longer connect by TCI",
        );
    }

    /// The station to every program, and the server's line to the snapshot: built and published
    /// under the engine lock (tempo-net's contract 1). Neither call reaches the backend.
    fn publish(&mut self) {
        let mut e = engine_lock(&self.engine);
        let status = match &self.running {
            Some(running) => {
                let handle = running.server.handle();
                let programs = handle.programs();
                lock(&self.backend.dial).look(dial_hz(&e), Instant::now());
                handle.publish(station(&e, &programs, self.meters.smeter_db()));
                // The operator's clicks on spots since the last tick, to every program.
                for (call, hz) in e.take_tci_spot_clicks() {
                    tell_click(&handle, &call, hz);
                }
                *lock(&self.armed) = armed_over(&e, &programs);
                let refused = lock(&self.backend.refused);
                Some(TciServerStatus {
                    port: Some(running.server.port()),
                    error: None,
                    spots: e.tci_program_spot_store().generation(),
                    programs: programs
                        .iter()
                        .map(|p| TciProgramStatus {
                            id: p.id.get(),
                            peer: p.peer.to_string(),
                            audio: p.audio,
                            keyed: p.key.is_some(),
                            refused: refused.get(&p.id).map(|r| (*r).to_string()),
                        })
                        .collect(),
                })
            }
            None => {
                // No server to tell: a click made now is not told later.
                e.take_tci_spot_clicks();
                self.failed.as_ref().map(|f| TciServerStatus {
                    port: None,
                    error: Some(f.why.clone()),
                    spots: 0,
                    programs: Vec::new(),
                })
            }
        };
        e.set_tci_server_status(status);
    }
}

/// Start the manager. Nothing is bound until the operator turns the server on.
pub(crate) fn spawn(engine: SharedEngine, feed: Arc<ReceiveAudioFeed>, meters: MeterFeed) {
    let mut manager = Manager::new(engine, feed, meters);
    let _ = SERVING.set(manager.serving.clone());
    let _ = std::thread::Builder::new()
        .name("tci-server".into())
        .spawn(move || loop {
            let on = manager.tick();
            std::thread::sleep(if on { TICK } else { IDLE_TICK });
        });
}

#[cfg(test)]
mod tests;
