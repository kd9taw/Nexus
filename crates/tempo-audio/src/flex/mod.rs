//! Nexus's own FlexRadio client as a CAT daemon (Beta, opt-in per radio): one SmartSDR session to
//! the radio, the rigctld shim that serves the radio's transmit slice to the radio loop, and the
//! slice report and typed intents the engine uses for every other slice.
//!
//! ## Where it sits
//! `crate::service` starts [`FlexDaemon`] in place of Hamlib's rigctld when the operator has
//! opted the radio in (`flex_native_cat`; operator ruling, 2026-10-03: Beta, opt-in per radio
//! profile, with SmartSDR CAT the default and the fallback). Like the native CI-V daemon and the
//! OmniRig shim it listens on the radio's own rigctld port on localhost, so Nexus's `Rig`, the
//! probe, the monitors, the handoff and every cockpit reach it as they reach any radio. Other
//! programs still reach the radio through Nexus's :4532 broker, whose PTT goes through the
//! engine's `broker_ptt`.
//!
//! ## Two seams (operator ruling, 2026-10-03, "Take all three")
//! - **The rigctld shim** ([`shim::FlexShim`]) serves ONE slice as the radio's dial: our transmit
//!   slice, else our lowest-numbered slice (receive only). Each of its verbs maps one to one onto
//!   a typed command or intent of the protocol core (the table is in [`shim`]). Its transmit verbs
//!   reach the wire only through the core's `Connection::start`, which runs the core's admission:
//!   a key needs every Flex-side condition admission checks, on top of every gate the engine ran
//!   before the loop sent `T 1`. Stops are never gated.
//! - **Typed intents** for every other slice of ours ([`FlexDaemon::apply`]), taken from the
//!   engine's `engine::slices`, and the slice report the engine builds its receiver set from
//!   ([`FlexDaemon::slices`]).
//!
//! ## Bring-up and teardown
//! A first connect finds no slice of ours, so the daemon makes one the way GUI clients do: a
//! panadapter, then a slice on it (the radio makes a new slice the transmit slice when it has
//! none). At teardown it first sends every stop of ours (what `T 0` sends), removes the slices and
//! panadapters it owns, and closes the session, whose own teardown sends those stops once more if
//! anything of ours may still be keyed and none has gone out.
//!
//! ## Audio (Beta, opt-in: `flex_native_audio` on a radio this client serves)
//! The session registers a UDP port, so DAX rides the same session ([`audio`]): receive audio for
//! each slice of ours in use through the refcounted DAX broker, and the over's transmit audio as
//! DAX TX in the tested format, at the operator's TX level, only while a key of ours is held. Where
//! the transmitter takes its audio from follows the operator's ruling of 2026-10-03 ([`routing`]):
//! the TX slice's mode decides, the flag is written only at the radio loop's quiet point
//! ([`FlexDaemon::sync_tx_routing`]) and never beside another program's DAX, and the operator's own
//! setting is put back at disconnect and at the next connect. While native audio is on, a digital
//! over is refused at `T 1` unless the radio takes its audio from Nexus's DAX
//! ([`shim::FlexShim`]): the radio's mic must never carry a digital over. With it off, a digital
//! over is refused while Nexus's own write still has the radio on DAX, which nothing feeds then:
//! never a silent over.
//!
//! ## Not yet
//! No panadapter stream from this session (the older native pan still opens its own), no
//! persisted client id (every connect registers as a new GUI client), and no tune carrier, ATU or
//! CW keyer: the core has a readback profile for each, but its admission refuses those starts
//! until a tester's bench confirms them. The stops for all of them are in place: `T 0`, teardown
//! and the radio loop's belief ([`FlexDaemon::keyed`]) cover a transmission of any kind. The CW
//! path is built up to that refusal: the WPM control sets the radio's keyer speed (`cw wpm`, which
//! keys nothing), each word goes to the radio's keyer timed at its speed, and a word the client
//! does not send, the refusal included, says why on the CW line.
//!
//! Nexus's own design, not a port: the protocol core it drives (`tempo_net::flex`) carries the
//! ported code and its attribution.

pub mod audio;
pub mod routing;
pub mod shim;
#[cfg(test)]
mod tests;

use std::net::{SocketAddr, TcpListener, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tempo_app::dto::FlexAudioRefusal;
use tempo_app::engine::receivers::RxOwner;
use tempo_app::engine::slices::{SliceIntent, SliceReport};
use tempo_net::flex::admission::another_dax_feeder;
use tempo_net::flex::encode::{AgcMode, Command, Mode, SliceFunction, Station, TxAudio};
use tempo_net::flex::model::{owner_of, Owner, StatusModel};
use tempo_net::flex::reconnect::End;
use tempo_net::flex::session::{self, ConnError, Connection, Event, Phase, Snapshot};
use tempo_net::flex::streams::{UDP_REGISTRATION_PORT, VITA_PORT};

use routing::{Memory, Restore, Routing, Step, View};

use crate::rigctld_server::{serve_connection, RigBackend};

/// The station name this client registers under (`client station`, `client program`): what the
/// radio's other clients show as the owner of Nexus's slices.
pub const STATION: &str = "Nexus";

/// How long the radio has to register Nexus and answer its subscriptions.
pub const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// How long each bring-up and teardown step may take.
const SETUP_TIMEOUT: Duration = Duration::from_secs(3);

/// The panadapter a first connect makes to put its slice on, in pixels. Nothing streams from it:
/// the daemon registers no UDP port.
const PAN_X: u16 = 1024;
const PAN_Y: u16 = 480;

/// How many of our recent session handles are kept per radio.
const RECENT_HANDLES_KEPT: usize = 4;

/// How long a mode the shim commanded stands for the slice's mode before the radio reports it.
const COMMANDED_FOR: Duration = Duration::from_secs(2);

/// How long before Nexus asks again for its DAX transmit stream.
const CREATE_TX_RETRY: Duration = Duration::from_secs(2);

/// How long teardown waits for the radio to prove an unkey before it puts the operator's DAX
/// source back. Past it the source is left, and the next connect puts it back.
const RESTORE_UNKEY_WAIT: Duration = Duration::from_millis(1_500);

/// What the shim, the audio threads and the daemon share about this client.
#[derive(Default)]
pub(crate) struct ClientState {
    /// The operator's native audio is on for this radio (the radio loop says so).
    pub(crate) native_audio: AtomicBool,
    /// The mode the shim last commanded for a slice, and when, until the radio reports it.
    pub(crate) commanded: Mutex<Option<(u8, String, Instant)>>,
    /// The last key the shim kept off the air: when, its words, and for the audio route its cause,
    /// for the radio loop to put on screen ([`FlexDaemon::key_refused_since`]). A CW word the
    /// client did not send has words and no audio cause.
    pub(crate) refused: Mutex<Option<(Instant, String, Option<FlexAudioRefusal>)>>,
    /// The CW speed the shim last set (`cw wpm`), and when, until the radio reports it: a CW word
    /// is timed at the slower of it and the radio's.
    pub(crate) cw_wpm: Mutex<Option<(u32, Instant)>>,
    /// The transmit audio routing ([`routing`]): carried out at the radio loop's quiet points
    /// ([`FlexDaemon::sync_tx_routing`]), and asked by the shim at `T 1` whether Nexus's own write
    /// leaves the radio on DAX ([`Routing::leaves_dax`]).
    pub(crate) routing: Mutex<Routing>,
}

/// The mode an over on `slice` goes out in: the one the shim commanded while the radio has not
/// reported it yet (for at most [`COMMANDED_FOR`]), else the radio's.
pub(crate) fn effective_mode(
    model: &StatusModel,
    slice: u8,
    state: &ClientState,
) -> Option<String> {
    let reported = model.slices.get(&slice).and_then(|s| s.mode.clone());
    let mut commanded = state
        .commanded
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    match commanded.as_ref() {
        Some((s, word, at)) if *s == slice => {
            if reported.as_deref() == Some(word.as_str()) || at.elapsed() >= COMMANDED_FOR {
                *commanded = None;
                reported
            } else {
                Some(word.clone())
            }
        }
        _ => reported,
    }
}

/// What the routing decides from ([`routing::View`]), read from the session's state: for the
/// radio loop's quiet points and the shim's `T 1` alike.
pub(crate) fn routing_view(snap: &Snapshot, state: &ClientState) -> View {
    let tx_slice = match snap.model.tx_slices().as_slice() {
        [slice]
            if owner_of(
                snap.model.slices.get(slice).and_then(|s| s.client_handle),
                snap.handle,
            ) == Owner::Ours =>
        {
            Some(*slice)
        }
        _ => None,
    };
    View {
        radio: snap.model.transmit.dax,
        tx_slice,
        tx_mode: tx_slice.and_then(|s| effective_mode(&snap.model, s, state)),
        other_feeder: another_dax_feeder(&snap.model, snap.handle, snap.dax_tx_stream).is_some(),
        dax_tx_stream: snap.dax_tx_stream.is_some(),
    }
}

/// Where the daemon's UDP goes, and where the operator's DAX source is remembered.
pub struct Options {
    /// The radio's VITA-49 port: DAX TX.
    pub vita: SocketAddr,
    /// The radio's UDP registration port: the one-byte datagram.
    pub registration: SocketAddr,
    pub memory: Arc<dyn Memory>,
}

impl Options {
    /// The radio's own ports, and the app's memory.
    pub fn for_radio(radio: SocketAddr) -> Options {
        Options {
            vita: SocketAddr::new(radio.ip(), VITA_PORT),
            registration: SocketAddr::new(radio.ip(), UDP_REGISTRATION_PORT),
            memory: Arc::new(AppMemory),
        }
    }
}

/// [`routing::FileMemory::app`], as an owned handle.
struct AppMemory;

impl Memory for AppMemory {
    fn recall(&self, radio: &str) -> Option<bool> {
        routing::FileMemory::app().recall(radio)
    }

    fn keep(&self, radio: &str, operator: Option<bool>) {
        routing::FileMemory::app().keep(radio, operator)
    }
}

/// Our recent sessions' client handles on each radio, newest first. A daemon that replaces a lost
/// one (the radio loop starts a fresh daemon when a session dies) hands them to its session, which
/// refuses to key while one of them holds the transmitter and lets a stop end that transmission.
/// Keyed by the radio's address, so two radios, or two simulators in a test run, never share one.
static RECENT_HANDLES: Mutex<Vec<(SocketAddr, Vec<u32>)>> = Mutex::new(Vec::new());

fn previous_handles(radio: SocketAddr) -> Vec<u32> {
    RECENT_HANDLES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|(addr, _)| *addr == radio)
        .map(|(_, handles)| handles.clone())
        .unwrap_or_default()
}

/// Remember `handle` as a recent session of ours on `radio`. The radio loop's tests seed it as an
/// earlier session would have.
pub(crate) fn remember_handle(radio: SocketAddr, handle: u32) {
    let mut all = RECENT_HANDLES
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let entry = match all.iter().position(|(addr, _)| *addr == radio) {
        Some(i) => &mut all[i].1,
        None => {
            all.push((radio, Vec::new()));
            &mut all.last_mut().expect("just pushed").1
        }
    };
    entry.retain(|h| *h != handle);
    entry.insert(0, handle);
    entry.truncate(RECENT_HANDLES_KEPT);
}

/// The radio's SmartSDR API address for `ip`: a literal IP straight through, a name resolved, the
/// API's port (4992) always.
pub fn radio_addr(ip: &str) -> std::io::Result<SocketAddr> {
    let addr = format!("{}:{}", ip.trim(), tempo_net::flexcat::FLEX_API_PORT);
    addr.parse().or_else(|_| {
        addr.to_socket_addrs()?.next().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unresolvable Flex API address",
            )
        })
    })
}

/// A reply's verdict: the radio's success code, or a sentence.
fn verdict(reply: Result<tempo_net::flex::wire::Reply, ConnError>) -> Result<String, String> {
    match reply {
        Ok(r) if r.code == 0 => Ok(r.message),
        Ok(r) => Err(format!("the radio refused it (0x{:08X})", r.code)),
        Err(ConnError::Timeout) => Err("the radio did not answer in time".into()),
        Err(ConnError::Closed) => Err("the connection to the radio is closed".into()),
        Err(ConnError::Refused(e)) => Err(format!("not sent: {e:?}")),
    }
}

/// **What listens on the radio's rigctld TCP port** when its CAT is Nexus's own Flex client.
pub struct FlexDaemon {
    radio: SocketAddr,
    /// The only strong reference the daemon keeps; the shim and the watcher hold weak ones, so
    /// dropping the daemon closes the session.
    conn: Option<Arc<Connection>>,
    /// Where the rigctld listener is bound: the radio's port, or the one the OS chose for 0.
    local_addr: SocketAddr,
    stop: Arc<AtomicBool>,
    tcp_thread: Option<JoinHandle<()>>,
    watch_thread: Option<JoinHandle<()>>,
    tx_intent: Arc<AtomicBool>,
    /// What the operator must be told about the transmitter, once it happens (sticky).
    alarm: Arc<Mutex<Alarm>>,
    state: Arc<ClientState>,
    audio: Option<audio::Audio>,
    memory: Arc<dyn Memory>,
    /// This radio in the memory.
    memory_key: String,
    /// When Nexus last asked for its DAX transmit stream.
    asked_tx_stream: Mutex<Option<Instant>>,
    /// The last routing refusal logged, so one refusal is logged once.
    routing_refusal: Mutex<Option<String>>,
}

impl FlexDaemon {
    /// Start against the radio at `ip` (its SmartSDR API, port 4992), serving rigctld on
    /// `127.0.0.1:<tcp_port>`.
    pub fn start_for_ip(ip: &str, tcp_port: u16) -> std::io::Result<FlexDaemon> {
        Self::start(radio_addr(ip)?, tcp_port)
    }

    /// Start against the radio at `radio`, serving rigctld on `127.0.0.1:<tcp_port>`.
    pub fn start(radio: SocketAddr, tcp_port: u16) -> std::io::Result<FlexDaemon> {
        let station = Station::new(STATION).expect("the station name is a safe token");
        let mut config = session::Config::new(station);
        config.previous_handles = previous_handles(radio);
        Self::start_with(radio, tcp_port, config)
    }

    /// Start with a given session configuration (tests shorten its timings).
    pub(crate) fn start_with(
        radio: SocketAddr,
        tcp_port: u16,
        config: session::Config,
    ) -> std::io::Result<FlexDaemon> {
        Self::start_full(radio, tcp_port, config, Options::for_radio(radio))
    }

    /// Start with a given session configuration and UDP destinations (tests point them at the
    /// simulator).
    pub(crate) fn start_full(
        radio: SocketAddr,
        tcp_port: u16,
        mut config: session::Config,
        options: Options,
    ) -> std::io::Result<FlexDaemon> {
        // The session registers this port (`client udpport`), so DAX rides the one session.
        let udp = UdpSocket::bind(("0.0.0.0", 0))?;
        config.connect.udp_port = Some(udp.local_addr()?.port());
        // The one-byte registration datagram goes from this socket to the radio's port 4992. The
        // session sends it after registration, just before `client udpport`, where upstream sends
        // it (`tempo_net::flex::handshake`): the radio learns our UDP endpoint from its source,
        // which firmware that answers `client udpport` with "not supported" needs. ⚠️ NEEDS-BENCH
        // on a real Flex.
        let registering = udp.try_clone()?;
        let registration = options.registration;
        config.udp_registration = Some(session::UdpRegistration::new(move || {
            let _ = registering.send_to(&[0u8], registration);
        }));
        let conn = Arc::new(Connection::connect(radio, config)?);
        let ready = conn.wait_until(READY_TIMEOUT, |s| {
            matches!(s.phase, Phase::Ready | Phase::Closed)
        });
        let snap = conn.snapshot();
        if let Some(handle) = snap.handle {
            remember_handle(radio, handle);
        }
        if !ready || snap.phase != Phase::Ready {
            let why = drain_reason(&conn, snap.phase == Phase::Closed).unwrap_or_else(|| {
                if ready {
                    "the radio closed the connection".to_string()
                } else {
                    "the radio did not register Nexus in time".to_string()
                }
            });
            return Err(std::io::Error::other(why));
        }
        if let Err(why) = bring_up(&conn) {
            remove_ours(&conn);
            return Err(std::io::Error::other(why));
        }
        let memory_key = radio.to_string();
        let remembered = options.memory.recall(&memory_key);
        let state = Arc::new(ClientState {
            routing: Mutex::new(Routing::new(remembered)),
            ..ClientState::default()
        });
        let audio = audio::Audio::start(Arc::downgrade(&conn), state.clone(), udp, options.vita)?;
        let listener = TcpListener::bind(("127.0.0.1", tcp_port))?;
        let local_addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let tx_intent = Arc::new(AtomicBool::new(false));
        let alarm = Arc::new(Mutex::new(Alarm::default()));
        let shim: Arc<dyn RigBackend> = Arc::new(shim::FlexShim::new(
            Arc::downgrade(&conn),
            tx_intent.clone(),
            state.clone(),
        ));
        let tcp_thread = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("flex-rigctld".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                // WinSock's accept() inherits the listener's non-blocking mode
                                // (see the CI-V and OmniRig daemons, which paid for this): a
                                // non-blocking client stream churns reconnects, and a reconnect
                                // mid-key trips the disconnect fail-safe.
                                let _ = stream.set_nonblocking(false);
                                let _ = stream.set_nodelay(true);
                                let backend = Arc::clone(&shim);
                                std::thread::spawn(move || serve_connection(stream, backend));
                            }
                            // A transient accept error must not kill the listener.
                            Err(_) => std::thread::sleep(Duration::from_millis(50)),
                        }
                    }
                })?
        };
        let watch_thread = {
            let stop = stop.clone();
            let weak = Arc::downgrade(&conn);
            let alarm = alarm.clone();
            std::thread::Builder::new()
                .name("flex-watch".into())
                .spawn(move || watch(weak, radio, &stop, &alarm))?
        };
        Ok(FlexDaemon {
            radio,
            conn: Some(conn),
            local_addr,
            stop,
            tcp_thread: Some(tcp_thread),
            watch_thread: Some(watch_thread),
            tx_intent,
            alarm,
            state,
            audio: Some(audio),
            memory: options.memory,
            memory_key,
            asked_tx_stream: Mutex::new(None),
            routing_refusal: Mutex::new(None),
        })
    }

    fn conn(&self) -> &Connection {
        self.conn.as_ref().expect("held until drop")
    }

    /// The session, for tests that wait on its state.
    #[cfg(test)]
    pub(crate) fn session(&self) -> &Connection {
        self.conn()
    }

    /// False once the session has ended (the radio went away, refused us, or did not confirm an
    /// unkey); the radio loop then starts a fresh daemon.
    pub fn is_alive(&self) -> bool {
        self.conn().snapshot().phase != Phase::Closed
    }

    /// The radio this daemon is a client of.
    pub fn radio(&self) -> SocketAddr {
        self.radio
    }

    /// The address the rigctld listener is bound to.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Whether a transmission of ours may be on the air through this client: a start of ours whose
    /// end its readback has not proven (still so once the session has ended keyed), or the radio
    /// naming this session or an earlier one of ours as the transmitter while it keys or tunes.
    /// The radio loop counts it with its own keyed state, which cannot see a transmission the loop
    /// did not key itself. Cheap: no copy of the session's state.
    pub fn keyed(&self) -> bool {
        self.conn().ours_on_air()
    }

    /// Tell the shim whether Nexus itself is transmitting, so the broker's disconnect fail-safe
    /// stands down while we are on the air. The same call as the CI-V and OmniRig daemons'.
    pub fn set_tx_intent(&self, on: bool) {
        self.tx_intent.store(on, Ordering::Relaxed);
    }

    /// What the operator must be told about the transmitter — the radio did not confirm an unkey,
    /// the session ended during a transmission, or a lost session of ours may still hold it — once
    /// it has happened.
    pub fn alarm(&self) -> Option<String> {
        // The session publishes its closed state before it sends that step's events, so a daemon
        // that reads dead may not have heard the alarm yet: wait for the watcher to read the
        // session up to its last event (`Closed`) and end, at most `SETUP_TIMEOUT`.
        if !self.is_alive() {
            let deadline = std::time::Instant::now() + SETUP_TIMEOUT;
            while self.watch_thread.as_ref().is_some_and(|t| !t.is_finished())
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        self.alarm
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .text
            .clone()
    }

    /// An alarm raised since the last call, for the radio loop to show while the session lives:
    /// each one once. [`Self::alarm`] is the one it reads once the session has ended.
    pub fn take_new_alarm(&self) -> Option<String> {
        let mut alarm = lock(&self.alarm);
        if std::mem::take(&mut alarm.new) {
            alarm.text.clone()
        } else {
            None
        }
    }

    // ── Audio (Beta, opt-in) ──────────────────────────────────────────────────────────────

    /// Whether the operator's native audio is on for this radio. While it is, every slice of ours
    /// in use streams its DAX receive audio and the transmit route can come up; while it is off,
    /// the streams go after the broker's grace window and the transmit route is down.
    pub fn set_native_audio(&self, on: bool) {
        self.state.native_audio.store(on, Ordering::Relaxed);
    }

    /// The served slice's 12 kHz receive audio since the last call: the radio loop's receive
    /// source while native audio is on.
    pub fn take_audio(&self) -> Vec<f32> {
        self.audio
            .as_ref()
            .map(|a| a.take_audio())
            .unwrap_or_default()
    }

    /// The DAX transmit route, for the radio loop's sound card to hand overs to.
    pub fn tx_tee(&self) -> Option<crate::backend::TxTeeHandle> {
        let tx: crate::backend::TxTeeHandle = self.audio.as_ref()?.tx();
        Some(tx)
    }

    /// Whether the DAX transmit route is up: native audio on, Nexus's own transmit stream exists,
    /// and no other program feeds DAX.
    pub fn tx_route_ready(&self) -> bool {
        self.audio.as_ref().is_some_and(|a| a.tx().ready())
    }

    /// The radio's own DAX source flag (`transmit dax`): whether the transmitter takes its audio
    /// from DAX rather than the radio's mic, as last read while native audio was on. Cheap.
    pub fn radio_dax(&self) -> Option<bool> {
        self.audio.as_ref().and_then(audio::Audio::radio_dax)
    }

    /// Why the shim kept a key asked for at or after `asked` off the air, if it did: its words,
    /// once, and for the audio route its cause. A CW word the client did not send (`b`) carries
    /// words only, for the CW line. The rigctld answer the radio loop reads for that key or word,
    /// `RPRT -1`, carries no reason.
    pub fn key_refused_since(&self, asked: Instant) -> Option<(String, Option<FlexAudioRefusal>)> {
        let refused = lock(&self.state.refused).take();
        match refused {
            Some((at, why, cause)) if at >= asked => Some((why, cause)),
            _ => None,
        }
    }

    /// Whether another program feeds the radio's DAX transmit audio (SmartSDR's DAX).
    pub fn other_dax_feeder(&self) -> bool {
        let snap = self.conn().snapshot();
        another_dax_feeder(&snap.model, snap.handle, snap.dax_tx_stream).is_some()
    }

    /// Whether the radio has the mic while native audio is on: Nexus is the only program feeding
    /// DAX, and the radio takes its transmit audio from its own mic, as the routing has it for
    /// Phone at the shack. Audio Nexus makes (a recorded voice message, an APRS packet, an SSTV
    /// picture) would then go over DAX, which the radio ignores, and the mic would carry the
    /// over; the voice keyer, APRS and SSTV refuse while this holds (operator rulings,
    /// 2026-10-04). Beside another program's DAX the flag is not Nexus's to judge, as for the
    /// digital refusal at `T 1`. Cheap: what the audio control thread last read.
    pub fn radio_has_mic(&self) -> bool {
        self.state.native_audio.load(Ordering::Relaxed)
            && self
                .audio
                .as_ref()
                .is_some_and(|a| a.radio_dax() == Some(false) && !a.other_feeder())
    }

    /// Whether Nexus's own write still has the radio taking its transmit audio from DAX in place
    /// of the operator's own setting, its mic ([`Routing::leaves_dax`]), as the radio reports it
    /// now: true from native audio going off until the routing puts the mic back at a quiet point.
    pub fn leaves_dax(&self) -> bool {
        let view = routing_view(&self.conn().snapshot(), &self.state);
        lock(&self.state.routing).leaves_dax(&view)
    }

    /// Our receive streams and their channels.
    #[cfg(test)]
    pub(crate) fn rx_streams(&self) -> std::collections::BTreeMap<u32, u8> {
        self.audio
            .as_ref()
            .map(audio::Audio::rx_streams)
            .unwrap_or_default()
    }

    /// Carry out the next step of the transmit audio routing, if any (operator ruling,
    /// 2026-10-03; [`routing`]). **Call it only at a quiet point**: the radio loop calls it at the
    /// end of a tick in which nothing of ours is keyed or about to be, which is what keeps the
    /// flag out of the keying path. `browser_voice`: the stream's browser voice is live. Returns
    /// what it did, for the log.
    pub fn sync_tx_routing(&self, browser_voice: bool) -> Option<String> {
        let conn = self.conn();
        let snap = conn.snapshot();
        if snap.phase != Phase::Ready {
            return None;
        }
        let view = routing_view(&snap, &self.state);
        let native = self.state.native_audio.load(Ordering::Relaxed);
        let now = monotonic_ms();
        let step = lock(&self.state.routing).step(&view, native, browser_voice, now)?;
        let (audio, done) = match step {
            Step::CreateDaxTx => {
                let mut asked = lock(&self.asked_tx_stream);
                if asked.is_some_and(|at| at.elapsed() < CREATE_TX_RETRY) {
                    return None;
                }
                *asked = Some(Instant::now());
                (
                    TxAudio::CreateDaxTx,
                    "asked the radio for Nexus's DAX transmit stream".to_string(),
                )
            }
            Step::Write { dax, why } => (
                TxAudio::DaxSource { dax },
                format!(
                    "transmit audio from {} ({why:?})",
                    if dax { "DAX" } else { "the radio's mic" }
                ),
            ),
        };
        match conn.route(audio) {
            Ok(_) => {
                if let Step::Write { dax, why } = step {
                    let keep = lock(&self.state.routing).written(dax, why, now);
                    self.memory.keep(&self.memory_key, keep);
                }
                *lock(&self.routing_refusal) = None;
                tempo_core::applog::info("cat", &format!("Flex client ({}): {done}", self.radio));
                Some(done)
            }
            Err(Some(refusal)) => {
                let why = refusal.to_string();
                let mut last = lock(&self.routing_refusal);
                if last.as_deref() != Some(why.as_str()) {
                    tempo_core::applog::info(
                        "cat",
                        &format!("Flex client ({}): not now: {why}", self.radio),
                    );
                    *last = Some(why);
                }
                None
            }
            Err(None) => None,
        }
    }

    /// Put the operator's DAX source back as this connection ends, if Nexus left it changed:
    /// never while keyed (the unkey must be proven first), never beside another program's DAX.
    /// What cannot be put back now stays in the memory for the next connect.
    fn restore_routing(&self, conn: &Connection) {
        // Nothing owed (the common case): no wait at all.
        let owed = |snap: &Snapshot| {
            lock(&self.state.routing).restore_on_disconnect(&routing_view(snap, &self.state))
        };
        if owed(&conn.snapshot()) == Restore::Nothing {
            self.memory.keep(&self.memory_key, None);
            return;
        }
        let unkeyed = conn.wait_until(RESTORE_UNKEY_WAIT, |s| !s.keyed || s.phase == Phase::Closed);
        let snap = conn.snapshot();
        match owed(&snap) {
            Restore::Nothing => self.memory.keep(&self.memory_key, None),
            Restore::Write(dax) if unkeyed && snap.phase == Phase::Ready => {
                if conn.route(TxAudio::DaxSource { dax }).is_ok() {
                    self.memory.keep(&self.memory_key, None);
                    tempo_core::applog::info(
                        "cat",
                        &format!(
                            "Flex client ({}): put the operator's transmit audio source back ({})",
                            self.radio,
                            if dax { "DAX" } else { "the radio's mic" }
                        ),
                    );
                }
            }
            // Keyed, refused, unknown or beside another program's DAX: the next connect does it.
            Restore::Write(_) | Restore::Later => {}
        }
    }

    /// Every slice the radio reports, ours and other clients', in the radio's order, in the
    /// engine's words.
    pub fn slices(&self) -> Vec<SliceReport> {
        let snap = self.conn().snapshot();
        slice_reports(&snap.model, snap.handle)
    }

    /// Send one intent for a slice of ours that is not the transmit slice. The engine checked
    /// the same two things when it queued it; this is the second door, at the wire, against the
    /// radio's latest report, because the transmit flag and the owner can move in between.
    pub fn apply(&self, index: u8, intent: &SliceIntent) -> Result<(), String> {
        let conn = self.conn();
        let snap = conn.snapshot();
        let slice = snap
            .model
            .slices
            .get(&index)
            .ok_or("the radio no longer reports that slice")?;
        if owner_of(slice.client_handle, snap.handle) != Owner::Ours {
            return Err("that slice is not ours".into());
        }
        if slice.tx == Some(true) {
            return Err("that slice is now the transmit slice".into());
        }
        let command = match intent {
            SliceIntent::Tune { mhz } => Command::SliceTune {
                slice: index,
                freq_hz: mhz * 1e6,
                keep_pan: false,
            },
            SliceIntent::Mode { mode } => {
                let word = shim::flex_mode_for(mode, slice.mode_list.as_deref())
                    .ok_or("the radio has no such mode")?;
                Command::SliceMode {
                    slice: index,
                    mode: Mode::new(&word).ok_or("not a mode word")?,
                }
            }
            SliceIntent::Filter { low_hz, high_hz } => Command::SliceFilter {
                slice: index,
                low_hz: *low_hz,
                high_hz: *high_hz,
            },
            SliceIntent::Agc { speed } => Command::SliceAgcMode {
                slice: index,
                mode: agc_mode(speed).ok_or("a slice has no such AGC speed")?,
            },
            SliceIntent::AfGain { gain } => Command::SliceAudioLevel {
                slice: index,
                level: (gain.clamp(0.0, 1.0) * 100.0).round() as i32,
            },
            SliceIntent::Mute { muted } => Command::SliceAudioMute {
                slice: index,
                mute: *muted,
            },
            SliceIntent::NoiseBlanker { on } => Command::SliceDsp {
                slice: index,
                function: SliceFunction::Nb,
                on: *on,
            },
            SliceIntent::NoiseReduction { on } => Command::SliceDsp {
                slice: index,
                function: SliceFunction::Nr,
                on: *on,
            },
            SliceIntent::AutoNotch { on } => Command::SliceDsp {
                slice: index,
                function: SliceFunction::Anf,
                on: *on,
            },
        };
        verdict(conn.request(command, shim::REQUEST_TIMEOUT)).map(|_| ())
    }
}

impl Drop for FlexDaemon {
    fn drop(&mut self) {
        // TX SAFETY: every stop of ours FIRST, while the session is up: the open operation's own,
        // or what the radio shows an earlier session of ours may hold (a tune carrier, a CWX word,
        // an over). A stop asks only "is it ours?", so this costs nothing when nothing of ours is
        // on the air.
        if let Some(conn) = &self.conn {
            let _ = conn.end_ours();
            self.restore_routing(conn);
            remove_ours(conn);
        }
        if let Some(mut audio) = self.audio.take() {
            audio.stop();
        }
        self.stop.store(true, Ordering::Relaxed);
        for thread in [self.tcp_thread.take(), self.watch_thread.take()]
            .into_iter()
            .flatten()
        {
            let _ = thread.join();
        }
        // The last strong reference: the session's teardown runs here, before drop returns.
        self.conn.take();
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Milliseconds on a monotonic clock shared by every daemon in the process.
fn monotonic_ms() -> u64 {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    u64::try_from(EPOCH.get_or_init(Instant::now).elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Make the first slice of ours when the radio has none: a panadapter, then a slice on it, the
/// order GUI clients use. The radio makes a new slice the transmit slice when it has none.
fn bring_up(conn: &Connection) -> Result<(), String> {
    let snap = conn.snapshot();
    if shim::served_slice(&snap.model, snap.handle).is_some() {
        return Ok(());
    }
    let reply = verdict(conn.request(
        Command::PanafallCreate { x: PAN_X, y: PAN_Y },
        SETUP_TIMEOUT,
    ))?;
    // The reply names the pan and its waterfall: `0x40000000,0x42000000`.
    let pan = reply
        .split(',')
        .next()
        .and_then(|id| u32::from_str_radix(id.trim().trim_start_matches("0x"), 16).ok())
        .ok_or_else(|| format!("the radio's answer named no panadapter ({reply:?})"))?;
    // The slice is created on our pan, so the pan's status must say it is ours first; its centre
    // is where the slice starts, until the radio loop tunes it.
    let ready = conn.wait_until(SETUP_TIMEOUT, |s| {
        s.model
            .owner(tempo_net::flex::model::ObjectRef::Pan(pan), s.handle)
            == Owner::Ours
            && s.model.pans.get(&pan).and_then(|p| p.center_mhz).is_some()
    });
    if !ready {
        return Err("the radio did not report the new panadapter".into());
    }
    let center_mhz = conn
        .snapshot()
        .model
        .pans
        .get(&pan)
        .and_then(|p| p.center_mhz)
        .ok_or("the panadapter has no centre")?;
    verdict(conn.request(
        Command::SliceCreate {
            pan: Some(pan),
            freq_hz: center_mhz * 1e6,
            mode: None,
        },
        SETUP_TIMEOUT,
    ))?;
    if conn.wait_until(SETUP_TIMEOUT, |s| {
        shim::served_slice(&s.model, s.handle).is_some()
    }) {
        Ok(())
    } else {
        Err("the radio did not report the new slice as ours".into())
    }
}

/// Remove every slice and panadapter of ours, best effort, each step bounded. The radio does not
/// free a panadapter's waterfall with it, so that goes too.
fn remove_ours(conn: &Connection) {
    let snap = conn.snapshot();
    let ours = |h: Option<u32>| owner_of(h, snap.handle) == Owner::Ours;
    for (index, slice) in &snap.model.slices {
        if ours(slice.client_handle) {
            let _ = conn.request(Command::SliceRemove { slice: *index }, SETUP_TIMEOUT);
        }
    }
    for (pan, delta) in &snap.model.pans {
        if ours(delta.client_handle) {
            let _ = conn.request(Command::PanRemove { pan: *pan }, SETUP_TIMEOUT);
            if let Some(waterfall) = delta.waterfall {
                let _ = conn.request(Command::WaterfallRemove { waterfall }, SETUP_TIMEOUT);
            }
        }
    }
}

/// Why a session that never became ready ended, from its events. The session publishes a step's
/// state before it sends that step's events, so a session already reading closed may not have
/// sent its reason yet: a closed one is read up to its last event (`Closed`).
fn drain_reason(conn: &Connection, closed: bool) -> Option<String> {
    let wait = if closed {
        SETUP_TIMEOUT
    } else {
        Duration::ZERO
    };
    let mut why = None;
    while let Some(event) = conn.next_event(wait) {
        match event {
            Event::RegistrationRejected { code, detail } => {
                why = Some(format!(
                    "the radio refused Nexus as a client (0x{code:08X} {detail})"
                ));
            }
            Event::ProtocolRejected if why.is_none() => {
                why = Some("the radio's API version is not one this client knows".into());
            }
            Event::Closed { .. } => break,
            _ => {}
        }
    }
    why
}

/// What the operator must be told about the transmitter.
#[derive(Default)]
struct Alarm {
    /// The latest raised, kept.
    text: Option<String>,
    /// Raised since the radio loop last took it ([`FlexDaemon::take_new_alarm`]).
    new: bool,
}

/// Drain the session's events, log the ones that matter, and keep what the operator must be told.
/// Ends at the session's last event (`Closed`: nothing follows it), or when the daemon stops.
fn watch(
    conn: std::sync::Weak<Connection>,
    radio: SocketAddr,
    stop: &AtomicBool,
    alarm: &Mutex<Alarm>,
) {
    let raise = |text: &str| {
        tempo_core::applog::warn("cat", &format!("Flex client ({radio}): {text}"));
        let mut alarm = lock(alarm);
        alarm.text = Some(text.to_string());
        alarm.new = true;
    };
    while !stop.load(Ordering::Relaxed) {
        let Some(conn) = conn.upgrade() else { break };
        let mut event = conn.next_event(Duration::from_millis(200));
        // The session publishes a step's state before it sends that step's events, so one already
        // reading closed may not have sent its last ones yet, the unkey alarm among them: a closed
        // session is read up to its last event (`Closed`), waiting at most `SETUP_TIMEOUT`. Once
        // it has sent them all, its channel answers at once, with nothing: stop rather than spin.
        let ended = event.is_none() && conn.snapshot().phase == Phase::Closed;
        if ended {
            event = conn.next_event(SETUP_TIMEOUT);
        }
        drop(conn);
        let Some(event) = event else {
            if ended {
                break;
            }
            continue;
        };
        match event {
            Event::Handle(handle) => remember_handle(radio, handle),
            Event::UnkeyUnconfirmed => raise(
                "the radio did not confirm the unkey — it may still be transmitting. Check the \
                 radio now.",
            ),
            Event::UnkeyedOnMissedPing { misses } => raise(&format!(
                "the radio stopped answering during a transmission ({misses} missed pings); \
                 Nexus sent the unkey first"
            )),
            Event::PreviousSessionHoldsTransmitter { handle } => raise(&format!(
                "an earlier Nexus session (0x{handle:08X}) still holds the transmitter; Nexus \
                 will not key until it is released"
            )),
            Event::ProtocolRejected => tempo_core::applog::warn(
                "cat",
                &format!(
                    "Flex client ({radio}): the radio's API version is not reviewed for \
                     transmit — receive only"
                ),
            ),
            Event::Closed { end, was_keyed, .. } => {
                tempo_core::applog::warn(
                    "cat",
                    &format!("Flex client ({radio}): session ended ({end:?}, keyed: {was_keyed})"),
                );
                // A session that ends with a transmission of ours unconfirmed (the connection
                // lost, the radio silent) leaves the transmitter unknown: the radio may still be
                // transmitting. An unconfirmed unkey has already said so, in its own words.
                if was_keyed && end != End::UnkeyUnconfirmed {
                    let what = match end {
                        End::Lost => "the connection to the radio was lost",
                        End::KeepaliveLost => "the radio stopped answering",
                        _ => "the session with the radio ended",
                    };
                    raise(&format!(
                        "{what} during a transmission — it may still be transmitting. Check the \
                         radio now."
                    ));
                }
                break;
            }
            _ => {}
        }
    }
}

/// The engine's AGC words to a slice's.
fn agc_mode(speed: &str) -> Option<AgcMode> {
    Some(match speed {
        "off" => AgcMode::Off,
        "slow" => AgcMode::Slow,
        "mid" => AgcMode::Med,
        "fast" => AgcMode::Fast,
        _ => return None,
    })
}

/// A slice's AGC word in the engine's words; the radio's own where the engine has none.
fn agc_speed(word: &str) -> String {
    match word {
        "med" => "mid".to_string(),
        other => other.to_string(),
    }
}

/// Every slice in `model`, ours and other clients', as the engine's reports.
pub(crate) fn slice_reports(model: &StatusModel, ours: Option<u32>) -> Vec<SliceReport> {
    model
        .slices
        .iter()
        .map(|(index, s)| SliceReport {
            index: *index,
            letter: s.letter.clone(),
            owner: match owner_of(s.client_handle, ours) {
                Owner::Ours => RxOwner::Ours,
                Owner::Foreign(_) => RxOwner::Foreign,
                Owner::Unknown => RxOwner::Unknown,
            },
            transmits: s.tx == Some(true),
            dial_mhz: s.frequency_mhz,
            mode: s.mode.as_deref().map(shim::rigctld_mode),
            filter_low_hz: s.filter_low,
            filter_high_hz: s.filter_high,
            agc: s.agc_mode.as_deref().map(agc_speed),
            nb: s.nb,
            nr: s.nr,
            anf: s.anf,
            af_gain: s
                .audio_level
                .map(|level| (level / 100.0).clamp(0.0, 1.0) as f32),
            muted: s.audio_mute,
            rit_hz: match (s.rit_on, s.rit_freq) {
                (Some(true), Some(hz)) => Some(hz),
                (Some(false), _) => Some(0),
                _ => None,
            },
        })
        .collect()
}
