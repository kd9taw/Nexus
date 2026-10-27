//! Nexus's TCI client as a CAT daemon: one session to an SDR program's TCI server (ExpertSDR3,
//! Thetis, piHPSDR, AetherSDR in front of a Flex), served to the radio loop as rigctld on the radio's
//! own port.
//!
//! ## Where it sits
//! `crate::service` starts [`TciDaemon`] in place of Hamlib's rigctld when a radio's Connection is
//! "TCI" (`tempo_app::settings::tci_reachable`). Like the native CI-V daemon, the OmniRig shim, the
//! Flex client and the Icom network client, it listens on the radio's own rigctld port on localhost,
//! so Nexus's `Rig`, the probe, the monitors, the handoff and every cockpit reach it as they reach any
//! radio. Nothing falls back to Hamlib on this connection: the bundled Hamlib cannot load its TCI
//! backend, and the operator chose the SDR program.
//!
//! ```text
//! TciDaemon ── tempo_net::tci::session::Connection ── WebSocket ── the SDR program's TCI server
//!     │                 ▲ the model, events, keys, stops
//!     └─ shim::TciShim ── rigctld on 127.0.0.1:<the radio's port> ── the radio loop's Rig
//! ```
//!
//! ## One session, then the next
//! The daemon owns one session (`tempo_net::tci::session`). It serves nothing until the server has
//! said `ready;`, and once the session has ended it is dead ([`TciDaemon::is_alive`]): the radio
//! loop drops it and asks for a new one, which [`registry`] spaces out on the reconnect ladder. A
//! key a session may have left on the air goes to the next one through [`registry`], whose stops
//! then go out first after `ready;`.
//!
//! ## Transmit
//! Every engine gate holds before the radio loop sends `T 1` (the Enable-TX latch, the stop hold,
//! the licence check on the emitted RF, identity, the watchdog); the shim and the session's
//! admission (`tempo_net::tci::admission`) only add reasons to refuse ([`shim`]).
//! - **Where an over's audio comes from** follows the operator's rule for the Flex client (ruling
//!   2026-10-03, "Follow the slice's mode"), by the mode the radio loop asked for: Nexus's audio for a
//!   digital over, the voice keyer and Remote's voice ([`TciDaemon::set_voice_key`]), the SDR
//!   program's microphone for Phone at the shack. A band restore is put back before a key ([`shim`]).
//! - **Never a silent over**: Nexus's audio is keyed only while its transmit route into this session
//!   is up: the radio loop installs the transmit tee ([`TciDaemon::tx_tee`], [`audio::TciTx`]) once
//!   the SDR program has taken Nexus's audio stream ([`TciDaemon::tx_route_ready`]), and only then
//!   says so ([`TciDaemon::set_tx_route`]). An over whose audio the program stops pulling part way
//!   through is ended by the daemon, and the radio loop reads why ([`TciDaemon::take_halt`]).
//! - **Stops are never gated**: `T 0`, the broker's disconnect fail-safe and teardown send them
//!   whatever the session believes.
//! - **Tune** is the SDR program's own carrier at its own tune drive (`tune:<trx>,true;`), started and
//!   ended with typed calls ([`TciDaemon::tune_on`], [`TciDaemon::tune_off`]): rigctld has no verb for
//!   a radio's tune carrier. The daemon ends it by itself past the hold it was given, whether or not
//!   the radio loop runs.
//!
//! ## Teardown
//! From its first step no new key goes out, and one already on its way through the shim goes out or
//! is refused before anything else happens. Then every stop of ours goes out through the live session,
//! the daemon waits a bounded time ([`UNKEY_WAIT`]) for the server to report the unkey, closes the
//! session (whose own close sends the stops again while a key may be up, then the close frame), and
//! hands what the server did not confirm to the next session ([`registry`]).
//!
//! ## What the radio loop reads
//! What the operator must be told about the transmitter ([`TciDaemon::alarm`],
//! [`TciDaemon::take_new_alarm`]), each over the session ended by itself
//! ([`TciDaemon::take_halt`]), why a key was kept off the air ([`TciDaemon::key_refused_since`]),
//! and whether a key of ours may be on the air ([`TciDaemon::keyed`]). Each key's timing goes to
//! the diagnostic log for the bench.
//!
//! ## Audio
//! [`TciDaemon::start_with_receive`] gives the session Nexus's own audio paths ([`audio`]): the
//! transceiver's receive audio at the modem rate, which the radio loop takes in place of the sound
//! card while the radio's TCI audio is on, and feeds to the waterfall and Remote Listen; and the
//! transmit route, Nexus's audio for an over of ours as the server pulls it.
//!
//! ## The panadapter
//! [`iq`]: the SDR program's IQ drawn in Nexus's RF scope while the radio's switch is on, on a
//! thread of its own ([`TciDaemon::want_pan`]).
//!
//! ## Spots
//! [`spots`]: Nexus's needed spots on the SDR program's panadapter while the radio's switch is on,
//! put up by a thread of their own ([`TciDaemon::offer_spots`]); a click on one of them tunes Nexus
//! and nothing else. Taken off in teardown, after the stops.
//!
//! ## Not yet
//! CW through the SDR program's keyer.
//!
//! Nexus's own design, not a port: the protocol core it drives (`tempo_net::tci`) is written from
//! Expert Electronics' TCI 2.0 specification.

pub mod audio;
pub mod iq;
pub mod registry;
pub mod shim;
pub mod spots;
#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::net::{SocketAddr, TcpListener, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tempo_app::settings::TciDialect;
use tempo_net::tci::admission::TxStart;
use tempo_net::tci::dialect::Preference;
use tempo_net::tci::reconnect::{End as LinkEnd, Step};
use tempo_net::tci::session::{
    Config, ConnectError, Connection, End, Event, Halt, OurKey, Phase, Snapshot, StopOutcome,
    Streams,
};
use tempo_net::tci::wire::{Stop, Word};
use tempo_net::tci::{Trx, MAX_TRX};

use crate::rigctld_server::{serve_connection, RigBackend};

/// How long past the session's own `ready;` deadline the daemon waits for the session to say how
/// it ended, before it gives up on it.
const READY_GRACE: Duration = Duration::from_secs(1);

/// How long teardown waits for the server to report the unkey of its stops. What it has not
/// confirmed by then is handed to the next session, which stops it first.
pub(crate) const UNKEY_WAIT: Duration = Duration::from_millis(1_000);

/// The longest a tune carrier may be held: the radio loop's own ceiling.
pub const TUNE_HOLD_MAX_MS: u64 = 60_000;

/// How long past its hold the daemon ends a tune carrier by itself: the SDR program latches the
/// carrier, and the radio loop that would release it can stall for a decode's length.
pub(crate) const TUNE_MARGIN: Duration = Duration::from_millis(2_000);

/// How long the watcher waits for an event before it looks at a tune's hold again.
const WATCH_POLL: Duration = Duration::from_millis(100);

/// How long a session that has ended is read for its last events.
const LAST_EVENTS: Duration = Duration::from_secs(3);

/// How long teardown waits for a key already on its way through the shim (its mode and drive put
/// back, then the key) to go out or be refused, before it sends its stops.
const KEYS_SETTLE: Duration = Duration::from_millis(1_000);

/// How long teardown waits for a rigctld client's call in flight to let the session go, so the
/// close happens inside teardown. Each wait in the shim holds the session for one short slice.
const RELEASE_WAIT: Duration = Duration::from_millis(500);

/// The most halts kept for the radio loop to read.
const HALTS_KEPT: usize = 8;

/// The words the alarm gives an unkey the server never confirmed.
const UNKEY_ALARM: &str =
    "the SDR program did not confirm the unkey — it may still be transmitting. Check the radio now.";

/// What the radio loop asks this daemon to connect to: the radio profile's TCI settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The SDR program's TCI server, an address or a name (`RadioProfile::tci_host`).
    pub host: String,
    /// Its port (`tci_port`).
    pub port: u16,
    /// Which of its transceivers this radio is (`tci_trx`).
    pub trx: u32,
    /// The operator's dialect setting (`tci_dialect`).
    pub dialect: TciDialect,
    /// Nexus's audio over TCI, both ways (`tci_audio`).
    pub audio: bool,
    /// Where the rigctld protocol is served on localhost.
    pub rigctld_port: u16,
}

impl Target {
    /// The server's address: a literal address as it is, a name resolved, an IPv4 address first
    /// (`localhost` may resolve to `::1` before `127.0.0.1`, and a server may listen on IPv4 only).
    pub fn server(&self) -> std::io::Result<SocketAddr> {
        let found: Vec<SocketAddr> = (self.host.trim(), self.port).to_socket_addrs()?.collect();
        found
            .iter()
            .find(|a| a.is_ipv4())
            .or_else(|| found.first())
            .copied()
            .ok_or_else(|| std::io::Error::other("the name resolved to no address"))
    }
}

/// The operator's dialect setting as the session takes it.
pub fn preference(dialect: TciDialect) -> Preference {
    match dialect {
        TciDialect::Auto => Preference::Auto,
        TciDialect::ExpertSdr3 => Preference::ExpertSdr3,
        TciDialect::Thetis => Preference::Thetis,
        TciDialect::PiHpsdr => Preference::PiHpsdr,
    }
}

/// Why a daemon did not start, in the operator's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartError {
    pub status: String,
}

impl StartError {
    fn new(status: impl Into<String>) -> StartError {
        StartError {
            status: status.into(),
        }
    }
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.status)
    }
}

impl std::error::Error for StartError {}

/// The reconnect ladder's clock, in milliseconds.
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// Milliseconds on a monotonic clock, from the first time anything here asked.
pub fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    u64::try_from(START.get_or_init(Instant::now).elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// How a daemon runs: the production waits, which the tests lengthen or shorten.
#[derive(Clone)]
pub(crate) struct Options {
    /// Changes the session's configuration (the tests shorten its windows).
    pub(crate) adjust: fn(&mut Config),
    /// [`shim::READBACK_WAIT`].
    pub(crate) readback: Duration,
    /// [`shim::REASSERT_WAIT`].
    pub(crate) reassert: Duration,
    /// [`UNKEY_WAIT`].
    pub(crate) unkey_wait: Duration,
    /// [`TUNE_MARGIN`].
    pub(crate) tune_margin: Duration,
    pub(crate) clock: Clock,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            adjust: |_| {},
            readback: shim::READBACK_WAIT,
            reassert: shim::REASSERT_WAIT,
            unkey_wait: UNKEY_WAIT,
            tune_margin: TUNE_MARGIN,
            clock: Arc::new(now_ms),
        }
    }
}

/// What the shim and the daemon share about this client.
pub(crate) struct ClientState {
    /// Nexus's audio over TCI is on for this radio (`tci_audio`).
    pub(crate) audio: bool,
    pub(crate) readback: Duration,
    pub(crate) reassert: Duration,
    /// Teardown has begun: no key goes out from here.
    pub(crate) closing: AtomicBool,
    /// Keys on their way through the shim: teardown sends its stops after them.
    pub(crate) keys_in_flight: AtomicUsize,
    /// The key being asked for carries Nexus's own voice (a voice-keyer message, Remote's voice):
    /// Nexus's audio, in a voice mode ([`TciDaemon::set_voice_key`]).
    pub(crate) voice: AtomicBool,
    /// Nexus's transmit audio reaches this session ([`TciDaemon::set_tx_route`]).
    pub(crate) tx_route: AtomicBool,
    /// The mode the radio loop last asked for, in the server's word: what an over is keyed as.
    pub(crate) asked_mode: Mutex<Option<Word>>,
    /// The last key the shim kept off the air: when, and why.
    pub(crate) refused: Mutex<Option<(Instant, String)>>,
}

impl ClientState {
    fn new(audio: bool, options: &Options) -> ClientState {
        ClientState {
            audio,
            readback: options.readback,
            reassert: options.reassert,
            closing: AtomicBool::new(false),
            keys_in_flight: AtomicUsize::new(0),
            voice: AtomicBool::new(false),
            tx_route: AtomicBool::new(false),
            asked_mode: Mutex::new(None),
            refused: Mutex::new(None),
        }
    }
}

/// What the operator must be told about the transmitter.
#[derive(Default)]
struct Alarm {
    /// The latest raised, kept.
    text: Option<String>,
    /// Raised since the radio loop last took it ([`TciDaemon::take_new_alarm`]).
    new: bool,
}

/// What the watcher keeps for the radio loop, and the session's claim on its transceiver.
#[derive(Default)]
struct Watch {
    alarm: Mutex<Alarm>,
    halts: Mutex<VecDeque<String>>,
    /// When a tune of ours ends by itself.
    tune_until: Mutex<Option<Instant>>,
    /// Ended by whichever sees the session end first: the watcher, or teardown.
    claim: Mutex<Option<registry::Claim>>,
    /// The transmit route, when the daemon has Nexus's own audio paths.
    tx: OnceLock<Arc<audio::TciTx>>,
    /// How long the server may leave an over of ours unpulled, part way through, while Nexus still
    /// has its audio: the session's chrono window (`Config::chrono_window_ms`), the limit its first
    /// pull is held to.
    pull_gap: Duration,
    /// Nexus's spots on the panadapter, and the clicks on them kept for the radio loop ([`spots`]).
    spots: Arc<spots::Shared>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// **What listens on the radio's rigctld TCP port** when its Connection is TCI.
pub struct TciDaemon {
    server: SocketAddr,
    trx: Trx,
    /// The only strong reference kept; the shim and the watcher hold weak ones, so dropping the
    /// daemon closes the session.
    conn: Option<Arc<Connection>>,
    /// Where the rigctld listener is bound: the radio's port, or the one the OS chose for 0.
    local_addr: SocketAddr,
    stop: Arc<AtomicBool>,
    tcp_thread: Option<JoinHandle<()>>,
    watch_thread: Option<JoinHandle<()>>,
    tx_intent: Arc<AtomicBool>,
    state: Arc<ClientState>,
    watch: Arc<Watch>,
    unkey_wait: Duration,
    tune_margin: Duration,
    clock: Clock,
    /// Nexus's own receive path, when the session was given it ([`Self::start_with_receive`]).
    receive: Option<Arc<audio::Receive>>,
    /// Nexus's own transmit route into the session, given with the receive path.
    tx: Option<Arc<audio::TciTx>>,
    /// The SDR program's panadapter ([`iq`]): dropped after the session's close, which stops its IQ.
    pan: iq::Pan,
}

impl TciDaemon {
    /// Connect to the SDR program's TCI server and serve the transceiver as rigctld on
    /// `127.0.0.1:<target.rigctld_port>`, once the server has said `ready;`. `streams` carries the
    /// session's receive audio, IQ and transmit audio. Blocks for at most the connect, the
    /// handshake and the server's `ready;` (about 12 s).
    pub fn start(target: &Target, streams: Box<dyn Streams>) -> Result<TciDaemon, StartError> {
        Self::start_with(target, streams, Options::default())
    }

    /// [`Self::start`] with Nexus's own audio paths as the session's streams ([`audio`]): its
    /// receive audio and its transmit route. What the radio loop starts.
    pub fn start_with_receive(target: &Target) -> Result<TciDaemon, StartError> {
        Self::start_receiving(target, Options::default())
    }

    /// [`Self::start_with_receive`] with the tests' waits and clock.
    pub(crate) fn start_receiving(
        target: &Target,
        options: Options,
    ) -> Result<TciDaemon, StartError> {
        let receive = Arc::new(audio::Receive::default());
        let tx = Arc::new(audio::TciTx::default());
        let streams = Box::new(audio::SessionAudio::new(receive.clone(), tx.clone()));
        let mut daemon = Self::start_with(target, streams, options)?;
        tx.attach(Arc::downgrade(
            daemon.conn.as_ref().expect("held until drop"),
        ));
        let _ = daemon.watch.tx.set(tx.clone());
        daemon.receive = Some(receive);
        daemon.tx = Some(tx);
        Ok(daemon)
    }

    /// [`Self::start`] with the tests' waits and clock.
    pub(crate) fn start_with(
        target: &Target,
        streams: Box<dyn Streams>,
        options: Options,
    ) -> Result<TciDaemon, StartError> {
        let Some(trx) = Trx::new(target.trx) else {
            return Err(StartError::new(format!(
                "There is no TCI transceiver {} (0 to {})",
                target.trx,
                MAX_TRX - 1
            )));
        };
        let server = target.server().map_err(|e| {
            StartError::new(format!(
                "Nexus could not find the TCI server {}:{} ({e})",
                target.host.trim(),
                target.port
            ))
        })?;
        // The rigctld port first: a port something else holds costs no session with the server.
        let listener = TcpListener::bind(("127.0.0.1", target.rigctld_port))
            .and_then(|l| l.set_nonblocking(true).map(|()| l))
            .map_err(|e| {
                StartError::new(format!(
                    "Nexus could not serve CAT on 127.0.0.1:{} ({e})",
                    target.rigctld_port
                ))
            })?;
        let local_addr = listener
            .local_addr()
            .map_err(|e| StartError::new(format!("Nexus could not serve CAT ({e})")))?;
        let claim = registry::claim((server, trx), (options.clock)())
            .map_err(|refusal| StartError::new(refusal.text()))?;
        let mut config = Config::new(trx);
        config.preference = preference(target.dialect);
        config.audio = target.audio;
        config.carried = claim.carried();
        (options.adjust)(&mut config);
        let conn = match Connection::connect(server, config, streams) {
            Ok(conn) => Arc::new(conn),
            Err(e) => {
                let text = connect_failed_text(&e, server);
                tempo_core::applog::warn("cat", &format!("TCI ({server}): {text}"));
                claim.ended(&LinkEnd::Failed(e), config.carried, (options.clock)());
                return Err(StartError::new(text));
            }
        };
        let ready_wait = Duration::from_millis(config.ready_timeout_ms) + READY_GRACE;
        conn.wait_until(ready_wait, |s| {
            matches!(s.phase, Phase::Ready | Phase::Closed)
        });
        let phase = conn.read(|s| s.phase);
        if phase != Phase::Ready {
            // How it ended, from its last event; one that has not ended by now is given up on as
            // a server that never said `ready;`, with what it was handed still carried.
            let ended = (phase == Phase::Closed)
                .then(|| closed_end(&conn))
                .flatten();
            let (end, carried) = ended.unwrap_or((End::NoReady, config.carried));
            drop(conn);
            let text = not_ready_text(&end, server);
            tempo_core::applog::warn("cat", &format!("TCI ({server}): {text}"));
            claim.ended(&LinkEnd::Ended(end), carried, (options.clock)());
            return Err(StartError::new(text));
        }
        claim.connected();
        tempo_core::applog::info(
            "cat",
            &format!("TCI ({server}): {}", connected_text(&conn.snapshot())),
        );
        let state = Arc::new(ClientState::new(target.audio, &options));
        let tx_intent = Arc::new(AtomicBool::new(false));
        let shim: Arc<dyn RigBackend> = Arc::new(shim::TciShim::new(
            Arc::downgrade(&conn),
            trx,
            tx_intent.clone(),
            state.clone(),
        ));
        let stop = Arc::new(AtomicBool::new(false));
        let watch = Arc::new(Watch {
            claim: Mutex::new(Some(claim)),
            pull_gap: Duration::from_millis(config.chrono_window_ms),
            ..Watch::default()
        });
        let tcp_thread = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("tci-rigctld".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                // WinSock's accept() inherits the listener's non-blocking mode
                                // (the CI-V and OmniRig daemons paid for this): a non-blocking
                                // client stream churns reconnects, and a reconnect mid-key trips
                                // the disconnect fail-safe.
                                let _ = stream.set_nonblocking(false);
                                let _ = stream.set_nodelay(true);
                                let backend = Arc::clone(&shim);
                                std::thread::spawn(move || serve_connection(stream, backend));
                            }
                            // A transient accept error must not kill the listener.
                            Err(_) => std::thread::sleep(Duration::from_millis(50)),
                        }
                    }
                })
        };
        let tcp_thread = match tcp_thread {
            Ok(t) => t,
            Err(e) => return Err(StartError::new(format!("Nexus could not serve CAT ({e})"))),
        };
        let watch_thread = {
            let stop = stop.clone();
            let weak = Arc::downgrade(&conn);
            let watch = watch.clone();
            let clock = options.clock.clone();
            std::thread::Builder::new()
                .name("tci-watch".into())
                .spawn(move || watch_session(&weak, server, &stop, &watch, &clock))
        };
        // From here a failure still tears down through `Drop`: stops first, then the close.
        let mut daemon = TciDaemon {
            server,
            trx,
            conn: Some(conn),
            local_addr,
            stop,
            tcp_thread: Some(tcp_thread),
            watch_thread: None,
            tx_intent,
            state,
            watch,
            unkey_wait: options.unkey_wait,
            tune_margin: options.tune_margin,
            clock: options.clock,
            receive: None,
            tx: None,
            pan: iq::Pan::default(),
        };
        match watch_thread {
            Ok(t) => daemon.watch_thread = Some(t),
            Err(e) => {
                return Err(StartError::new(format!(
                    "Nexus could not watch its TCI session ({e})"
                )))
            }
        }
        Ok(daemon)
    }

    fn conn(&self) -> &Connection {
        self.conn.as_ref().expect("held until drop")
    }

    /// The session, for tests that wait on its state.
    #[cfg(test)]
    pub(crate) fn session(&self) -> &Connection {
        self.conn()
    }

    /// False once the session has ended (the server went away, stopped answering, or did not
    /// confirm an unkey); the radio loop then drops this daemon and asks for a new one.
    pub fn is_alive(&self) -> bool {
        self.conn().read(|s| s.phase != Phase::Closed)
    }

    /// The TCI server this daemon is a client of.
    pub fn server(&self) -> SocketAddr {
        self.server
    }

    /// The server's transceiver this daemon serves.
    pub fn trx(&self) -> Trx {
        self.trx
    }

    /// The registry's key for this daemon's transceiver ([`registry`]).
    pub fn key(&self) -> registry::Key {
        (self.server, self.trx)
    }

    /// The address the rigctld listener is bound to.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Whether a key of ours may be on the air: from its write until the server reports the
    /// transmitter off, kept once the session has ended. The radio loop counts it with its own
    /// keyed state (`was_keyed`), which cannot see what the session still believes. Cheap.
    pub fn keyed(&self) -> bool {
        self.conn().ours_on_air()
    }

    /// The session's state: the server's model of the radio, the dialect, the evidence and timing
    /// for the bench, the counts of what was refused.
    pub fn snapshot(&self) -> Snapshot {
        self.conn().snapshot()
    }

    /// The radio the server named (`device:`), once it has: what a contact made on this radio
    /// logs as its rig (`MY_RIG`), where the radio's model only says "TCI".
    pub fn device(&self) -> Option<String> {
        self.conn()
            .read(|s| s.model.device.known().map(|d| d.as_str().to_string()))
    }

    /// Who answered, in the words Test CAT shows: the program and the radio as the server named
    /// them, and the transceiver, counted from 1 as Settings counts it.
    pub fn status(&self) -> String {
        let s = self.snapshot();
        let program = match (&s.evidence.program, &s.evidence.version) {
            (Some(p), Some(v)) => format!("{p} {v}"),
            (Some(p), None) => p.clone(),
            _ => "an SDR program".to_string(),
        };
        let device = s
            .model
            .device
            .known()
            .map_or(String::new(), |d| format!(" on {}", d.as_str()));
        format!(
            "TCI: {program}{device}, transceiver {}.",
            s.trx.get().saturating_add(1)
        )
    }

    /// Nexus's own receive path, when this daemon was started with it
    /// ([`Self::start_with_receive`]).
    pub fn receive(&self) -> Option<&Arc<audio::Receive>> {
        self.receive.as_ref()
    }

    /// Nexus's transmit route into this session ([`audio::TciTx`]), when this daemon was started
    /// with its own audio paths ([`Self::start_with_receive`]): the radio loop's transmit tee while
    /// [`Self::tx_route_ready`] holds.
    pub fn tx_tee(&self) -> Option<crate::backend::TxTeeHandle> {
        self.tx.clone().map(|tx| tx as crate::backend::TxTeeHandle)
    }

    /// The transmit route itself, for tests that read what it holds.
    #[cfg(test)]
    pub(crate) fn tx(&self) -> Option<&Arc<audio::TciTx>> {
        self.tx.as_ref()
    }

    /// Whether Nexus's transmit audio can reach this session: the daemon has its transmit route,
    /// Audio over TCI is on for this radio, the session is ready, and the SDR program reports
    /// Nexus's audio stream for this transceiver running (its acknowledgement of `audio_start`).
    /// The radio loop installs the transmit tee only then, and takes it out when this goes false.
    pub fn tx_route_ready(&self) -> bool {
        self.tx.is_some()
            && self.state.audio
            && self
                .conn()
                .read(|s| s.phase == Phase::Ready && s.model.trx(self.trx).audio.is_true())
    }

    /// Tell the shim whether Nexus itself is transmitting, so the broker's disconnect fail-safe
    /// stands down while we are on the air. The same call as the CI-V, OmniRig and Flex daemons'.
    pub fn set_tx_intent(&self, on: bool) {
        self.tx_intent.store(on, Ordering::Relaxed);
    }

    /// Mark the key the radio loop is about to ask for as carrying Nexus's own voice (a voice-keyer
    /// message, Remote's voice): in a voice mode it then takes Nexus's audio, not the SDR's
    /// microphone. Set just before that one key, cleared just after it.
    pub fn set_voice_key(&self, on: bool) {
        self.state.voice.store(on, Ordering::SeqCst);
    }

    /// Whether Nexus's transmit audio reaches this session (its transmit tee is installed): a key
    /// that takes Nexus's audio is refused while it does not, so no over goes out silent. Off until
    /// the radio loop has installed the tee ([`Self::tx_tee`]), and off again before it goes.
    pub fn set_tx_route(&self, ready: bool) {
        self.state.tx_route.store(ready, Ordering::SeqCst);
    }

    /// Why the shim kept a key asked for at or after `asked` off the air, if it did: once. The
    /// rigctld answer the radio loop reads for that key, `RPRT -1`, carries no reason.
    pub fn key_refused_since(&self, asked: Instant) -> Option<String> {
        let refused = lock(&self.state.refused).take();
        match refused {
            Some((at, why)) if at >= asked => Some(why),
            _ => None,
        }
    }

    // ── The SDR program's own tune carrier ──────────────────────────────────────────────────

    /// Whether Tune is the SDR program's own carrier on this connection: always, so the radio loop
    /// starts and ends it with [`Self::tune_on`] and [`Self::tune_off`] and plays no tone.
    pub fn tunes_natively(&self) -> bool {
        true
    }

    /// Start the SDR program's tune carrier, at its own tune drive, held at most `hold_ms` (the
    /// radio loop's tune timeout, at most [`TUNE_HOLD_MAX_MS`]): the daemon ends it by itself at
    /// that plus [`TUNE_MARGIN`], whether or not the loop runs. The words for the PTT line when it
    /// does not key.
    pub fn tune_on(&self, hold_ms: u64) -> Result<(), String> {
        let hold = Duration::from_millis(hold_ms.min(TUNE_HOLD_MAX_MS));
        match self.conn().key(TxStart::Tune) {
            Ok(()) => {
                *lock(&self.watch.tune_until) = Some(Instant::now() + hold + self.tune_margin);
                Ok(())
            }
            Err(Some(refusal)) => Err(format!("Tune not keyed: {refusal}.")),
            Err(None) => Err("Tune not keyed: the connection to the SDR program is closed.".into()),
        }
    }

    /// End the tune carrier (`tune:<trx>,false;`). Never gated.
    pub fn tune_off(&self) -> bool {
        *lock(&self.watch.tune_until) = None;
        matches!(self.conn().stop(Stop::Tune(self.trx)), StopOutcome::Sent(_))
    }

    // ── What the radio loop is told ─────────────────────────────────────────────────────────

    /// What the operator must be told about the transmitter (the server did not confirm an unkey,
    /// the session ended during a transmission), once it has happened.
    pub fn alarm(&self) -> Option<String> {
        // The session publishes its closed state before it sends that step's events, so a daemon
        // that reads dead may not have heard the alarm yet: wait for the watcher to read the
        // session up to its last event (`Closed`) and end.
        if !self.is_alive() {
            let deadline = Instant::now() + LAST_EVENTS;
            while self.watch_thread.as_ref().is_some_and(|t| !t.is_finished())
                && Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        lock(&self.watch.alarm).text.clone()
    }

    /// An alarm raised since the last call, for the radio loop to show while the session lives:
    /// each one once. [`Self::alarm`] is the one it reads once the session has ended.
    pub fn take_new_alarm(&self) -> Option<String> {
        let mut alarm = lock(&self.watch.alarm);
        if std::mem::take(&mut alarm.new) {
            alarm.text.clone()
        } else {
            None
        }
    }

    /// An over of ours the session or the daemon ended by itself, and why, oldest first: the radio
    /// loop halts the engine's transmission with these words. Each once.
    pub fn take_halt(&self) -> Option<String> {
        lock(&self.watch.halts).pop_front()
    }
}

impl Drop for TciDaemon {
    fn drop(&mut self) {
        // From here no key goes out, whatever a rigctld client still connected asks; one already
        // past that look goes out or is refused before the stops do.
        self.state.closing.store(true, Ordering::SeqCst);
        let settle = Instant::now() + KEYS_SETTLE;
        while self.state.keys_in_flight.load(Ordering::SeqCst) > 0 && Instant::now() < settle {
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut carried = None;
        if let Some(conn) = &self.conn {
            // TX SAFETY: every stop of ours FIRST, through the live session, whatever it believes.
            let _ = conn.end_ours();
            if conn.ours_on_air() {
                conn.wait_until(self.unkey_wait, |s| {
                    s.keyed.is_none() || s.phase == Phase::Closed
                });
            }
            carried = conn.read(|s| s.keyed);
            // Then Nexus's spots off the panadapter, while the session is still up.
            self.watch.spots.close(conn);
        }
        self.stop.store(true, Ordering::Relaxed);
        for thread in [self.tcp_thread.take(), self.watch_thread.take()]
            .into_iter()
            .flatten()
        {
            let _ = thread.join();
        }
        // The last strong reference: the session's own close runs here, before drop returns. A
        // rigctld client's write in flight holds it for one slice of its wait at most.
        if let Some(conn) = self.conn.take() {
            let deadline = Instant::now() + RELEASE_WAIT;
            while Arc::strong_count(&conn) > 1 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            drop(conn);
        }
        // Released last, once the session has closed: the next one for this transceiver stops
        // first what this one did not see confirmed. Unless the watcher saw the session end first,
        // and already handed on what it carried.
        let claim = lock(&self.watch.claim).take();
        if let Some(claim) = claim {
            claim.ended(&LinkEnd::Ended(End::Closed), carried, (self.clock)());
            if let Some(key) = carried {
                tempo_core::applog::warn(
                    "cat",
                    &format!(
                        "TCI ({}): closed with {key:?} unconfirmed; the next session sends its \
                         stops first",
                        self.server
                    ),
                );
            }
        }
    }
}

/// How a session that has ended ended, and what it carried, from its last event.
fn closed_end(conn: &Connection) -> Option<(End, Option<OurKey>)> {
    let deadline = Instant::now() + LAST_EVENTS;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match conn.next_event(left)? {
            Event::Closed { end, carried } => return Some((end, carried)),
            _ if left.is_zero() => return None,
            _ => {}
        }
    }
}

/// The operator's words for a connect that failed.
fn connect_failed_text(e: &ConnectError, server: SocketAddr) -> String {
    match e {
        ConnectError::Unreachable(why) => format!(
            "Nothing answered at {server}: start the SDR program and turn on its TCI server, and \
             check the address and port ({why})"
        ),
        ConnectError::Handshake(why) => {
            format!("{server} answered, but not as a TCI server ({why})")
        }
        ConnectError::Spawn(why) => format!("Nexus could not start its TCI session ({why})"),
    }
}

/// The operator's words for a session that ended before `ready;`.
fn not_ready_text(end: &End, server: SocketAddr) -> String {
    match end {
        End::NoReady => {
            format!("The SDR program at {server} did not finish connecting (it never said ready)")
        }
        End::Lost => {
            format!("The SDR program at {server} closed the connection before it was ready")
        }
        End::Stale => format!("The SDR program at {server} stopped answering before it was ready"),
        End::ProtocolError(why) => {
            format!("The SDR program at {server} sent something Nexus could not read ({why})")
        }
        End::UnkeyUnconfirmed => {
            format!("The SDR program at {server} did not confirm the unkey of an earlier session")
        }
        End::Closed => "The TCI session closed before it was ready".to_string(),
    }
}

/// The log's words for a session that reached `ready;`: who answered, and what Nexus learned of
/// it (the bench reads these back).
fn connected_text(s: &Snapshot) -> String {
    let program = match (&s.evidence.program, &s.evidence.version) {
        (Some(p), Some(v)) => format!("{p} {v}"),
        (Some(p), None) => p.clone(),
        _ => "a server with no protocol line".to_string(),
    };
    let device = s
        .model
        .device
        .known()
        .map_or(String::new(), |d| format!(" on {}", d.as_str()));
    format!(
        "connected to {program}{device}, transceiver {}; speaking {} (drive {:?}, S-meter {:?}, key \
         source {})",
        s.trx,
        s.dialect.name,
        s.dialect.drive,
        s.dialect.smeter,
        if s.dialect.trx_source {
            "named"
        } else {
            "not named"
        }
    )
}

/// The words for an over the session ended by itself.
fn halt_words(halt: &Halt) -> String {
    match halt {
        Halt::NoChrono => "the SDR program did not start taking Nexus's audio, so Nexus ended the \
                           over (it could have been transmitting its microphone)"
            .to_string(),
        Halt::NoEcho => "the SDR program did not confirm the key, so Nexus ended the over".into(),
        Halt::SdrStopped => "the SDR program stopped, so Nexus ended the over".into(),
        Halt::Silent => {
            "the SDR program stopped answering during the over; Nexus sent the unkey".into()
        }
        Halt::ServerEnded => {
            "the SDR program ended the over itself (its operator, or its own transmit timeout)"
                .into()
        }
        Halt::WrongSource(source) => format!(
            "the SDR program took its {source} input, not Nexus's audio, so Nexus ended the over"
        ),
        Halt::AudioStopped => "Nexus's audio stream to the SDR program stopped, so Nexus ended \
                               the over (it would have taken its microphone)"
            .to_string(),
    }
}

/// The alarm's words for a session that ended while a key of ours may have been on the air.
fn lost_alarm(end: &End) -> String {
    let what = match end {
        End::Lost => "the connection to the SDR program was lost",
        End::Stale => "the SDR program stopped answering",
        End::ProtocolError(_) => "the SDR program sent something Nexus could not read",
        _ => "the session with the SDR program ended",
    };
    format!("{what} during a transmission — it may still be transmitting. Check the radio now.")
}

/// End a tune of ours that has outlasted its hold, whether or not the radio loop runs.
fn end_a_tune_past_its_hold(conn: &Connection, server: SocketAddr, watch: &Watch) {
    let mut until = lock(&watch.tune_until);
    let Some(at) = *until else {
        return;
    };
    let tuning = conn.read(|s| s.over_open && s.keyed.is_some_and(|k| k.kind == TxStart::Tune));
    if !tuning {
        *until = None;
        return;
    }
    if Instant::now() < at {
        return;
    }
    *until = None;
    drop(until);
    let _ = conn.end_ours();
    let words = "the tune carrier outlasted its hold, so Nexus ended it";
    tempo_core::applog::warn("cat", &format!("TCI ({server}): {words}"));
    push_halt(watch, words.to_string());
}

/// End an over of ours whose audio the SDR program stopped pulling part way through while Nexus
/// still had some of it to send: the radio would stay keyed with nothing of ours to send until the
/// over's own end (never a silent over; the Flex client's route going ends an
/// over the same way). Only once this key's first pull has come (until then the session's own chrono
/// window rules), and only past the window that first pull is held to.
fn end_an_over_the_server_stopped_pulling(conn: &Connection, server: SocketAddr, watch: &Watch) {
    let Some(tx) = watch.tx.get() else {
        return;
    };
    if !conn.audio_over_open() || tx.queued() == 0 {
        return;
    }
    let pulled = conn.read(|s| s.timing.first_chrono_ms.is_some());
    if !pulled
        || tx
            .last_pull()
            .is_none_or(|at| at.elapsed() <= watch.pull_gap)
    {
        return;
    }
    let _ = conn.end_ours();
    let words = "the SDR program stopped taking Nexus's audio part way through the over, so Nexus \
                 ended it";
    tempo_core::applog::warn("cat", &format!("TCI ({server}): {words}"));
    push_halt(watch, words.to_string());
}

fn push_halt(watch: &Watch, words: String) {
    let mut halts = lock(&watch.halts);
    if halts.len() >= HALTS_KEPT {
        halts.pop_front();
    }
    halts.push_back(words);
}

/// Drain the session's events, log the ones that matter, and keep what the radio loop must be
/// told: the alarms and the overs the session ended. Ends at the session's last event (`Closed`:
/// nothing follows it), or when the daemon stops. Each key's timing is one line in the diagnostic
/// log, for the bench.
fn watch_session(
    conn: &Weak<Connection>,
    server: SocketAddr,
    stop: &AtomicBool,
    watch: &Watch,
    clock: &Clock,
) {
    let raise = |text: &str| {
        tempo_core::applog::warn("cat", &format!("TCI ({server}): {text}"));
        let mut alarm = lock(&watch.alarm);
        alarm.text = Some(text.to_string());
        alarm.new = true;
    };
    let log = |text: &str| tempo_core::applog::info("cat", &format!("TCI ({server}): {text}"));
    while !stop.load(Ordering::Relaxed) {
        let Some(conn) = conn.upgrade() else { break };
        end_a_tune_past_its_hold(&conn, server, watch);
        end_an_over_the_server_stopped_pulling(&conn, server, watch);
        let mut event = conn.next_event(WATCH_POLL);
        // The session publishes a step's state before it sends that step's events, so one already
        // reading closed may not have sent its last ones yet: read it up to `Closed`. Once it has
        // sent them all its channel answers at once, with nothing: stop rather than spin.
        let ended = event.is_none() && conn.read(|s| s.phase == Phase::Closed);
        if ended {
            event = conn.next_event(LAST_EVENTS);
        }
        let timing = conn.read(|s| s.timing);
        drop(conn);
        let Some(event) = event else {
            if ended {
                break;
            }
            continue;
        };
        match event {
            Event::Ready { .. } => {}
            Event::KeyConfirmed(kind) => log(&format!(
                "the SDR program confirmed our {kind:?} key in {} ms",
                timing.echo_ms.map_or("?".into(), |ms| ms.to_string())
            )),
            Event::Halted(halt) => {
                let words = halt_words(&halt);
                tempo_core::applog::warn("cat", &format!("TCI ({server}): {words}"));
                push_halt(watch, words);
            }
            Event::Unkeyed => {
                log(&format!(
                "the SDR program reports our unkey; that key: echo {} ms, first TX_CHRONO {} ms",
                timing.echo_ms.map_or("?".into(), |ms| ms.to_string()),
                timing.first_chrono_ms.map_or("-".into(), |ms| ms.to_string())
            ))
            }
            Event::UnkeyUnconfirmed => raise(UNKEY_ALARM),
            Event::ForeignKey { on } => log(if on {
                "another program, or the SDR program's operator, is transmitting"
            } else {
                "the other transmission ended"
            }),
            Event::Running(on) => log(if on {
                "the SDR program started"
            } else {
                "the SDR program stopped"
            }),
            // Kept for the radio loop, which tunes to it, when it is one of ours; never a key.
            Event::SpotClicked { call, hz, channel } => {
                let taken = watch.spots.clicked(&call, hz, channel);
                log(&spots::click_words(&call, hz, taken));
            }
            Event::Closed { end, carried } => {
                tempo_core::applog::warn(
                    "cat",
                    &format!("TCI ({server}): session ended ({end:?}, carried: {carried:?})"),
                );
                let claim = lock(&watch.claim).take();
                if let Some(claim) = claim {
                    for step in claim.ended(&LinkEnd::Ended(end.clone()), carried, clock()) {
                        match step {
                            Step::Retry { after_ms } => {
                                log(&format!("the next session may start in {after_ms} ms"));
                            }
                            Step::WaitForOperator(hold) => {
                                log(&format!("waiting for the operator: {hold:?}"));
                            }
                            Step::StillKeyed(_) => {}
                        }
                    }
                }
                // A session that ends with a key of ours unconfirmed leaves the transmitter
                // unknown. An unconfirmed unkey has already said so, in its own words.
                if carried.is_some() && end != End::UnkeyUnconfirmed {
                    raise(&lost_alarm(&end));
                }
                break;
            }
        }
    }
}
