//! Nexus's own Icom network client as a CAT daemon (Beta, opt-in per radio): one network session
//! to the radio's built-in server, the native CI-V daemon carried over it, and what was read from
//! the radio at connect.
//!
//! ## Where it sits
//! `crate::service` starts [`IcomLanDaemon`] in place of Hamlib's rigctld when a radio's
//! Connection is "Icom network (LAN / Wi-Fi)", on the six Icoms with a network server
//! (`crate::rigmodels::icom_lan_model`). Like the native CI-V daemon, the OmniRig shim and the
//! Flex client, it listens on the radio's own rigctld port on localhost, so Nexus's `Rig`, the
//! probe, the monitors, the handoff and every cockpit reach it as they reach any radio. Nothing
//! falls back to Hamlib on this connection: the operator chose the network.
//!
//! ```text
//! IcomLanDaemon ── session thread ── tempo_net::icom::session::Session (three UDP sockets)
//!     │                 ▲  delivered CI-V frames / frames to send
//!     │                 │
//!     └─ CivDaemon ── LanCivIo ── CivEngine: verbs, transceive, scope ── rigctld on 127.0.0.1
//! ```
//!
//! ## Nothing transmits over this connection
//! The CI-V daemon runs with keying refused ([`crate::civ::broker::KeyingPolicy::Refused`]):
//! `T 1`, CAT CW and VOX on send nothing, while every unkey and stop goes out. The engine refuses
//! the same overs before PTT on this connection, with its own reason. The session reserves no
//! transmit audio path and starts no audio stream. At connect the daemon READS the radio's
//! time-out timer and MOD Input settings ([`probe`]) and reports them; it writes no menu.
//!
//! ## One session per radio, and when to try again
//! The radio admits one network client, and holds a lost session's slot for its own timeout (about
//! 20 s on some radios, up to 3 minutes on others). [`registry`] keeps at most one live session per
//! radio address in the process, and the retry ladder (1, 2, 4, 8, 16 s, then every 30 s) between
//! attempts; a radio that ended the session itself, or refused the login, waits for the operator.
//!
//! ## The password
//! It comes from the OS keychain at the moment of use, through the credential source the desktop
//! shell registers ([`set_credential_source`]), and is moved into the session's login, which encodes
//! it and drops it. It is never in [`Target`], a channel message, a log line or the CI-V
//! diagnostic file: only CI-V payloads pass through [`io::LanCivIo`], and the login travels on
//! the control socket. Session notes say "network user: set", never the name.
//!
//! Nexus's own design, not a port: the protocol core it drives (`tempo_net::icom`) carries the
//! ported code and its attribution.

pub mod io;
pub mod probe;
pub mod registry;
#[cfg(test)]
mod tests;

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::num::NonZeroU16;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tempo_net::icom::caps::Model;
use tempo_net::icom::conf::{Config, Login, Secret, User};
use tempo_net::icom::reconnect::End;
use tempo_net::icom::session::{
    Action, CivError, ConnectError, Event, Locals, Loss, LossReason, Radio, Session, SocketError,
    Stage, State,
};
use tempo_net::icom::wire::{Role, PORT_CONTROL};

use crate::civ::broker::{CivDaemon, KeyingPolicy, Link};
use crate::civ::commands::IcomModel;

/// How long the CI-V engine waits for one reply over the network: two datagrams, maybe a
/// retransmit (the session asks after 100 ms), maybe Wi-Fi. Under the radio loop's 2.5 s CAT
/// window. The bench's diagnostic log gives the real round trips.
pub const LAN_DEADLINE: Duration = Duration::from_millis(1_000);

/// The whole connect, from the first probe to the CI-V stream open, is bounded by this.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Give up early when nothing at all has answered the first probe by now.
pub const FIRST_ANSWER_TIMEOUT: Duration = Duration::from_secs(3);

/// Why every transmit is refused on this connection, as the operator reads it: the engine's own
/// refusal before PTT says the same.
pub const KEYING_REFUSED: &str = tempo_app::settings::ICOM_LAN_TX_REFUSED;

/// How long a frame handed to the session thread waits for it to say whether it went out.
const SEND_WAIT: Duration = Duration::from_millis(500);

/// How long the session thread sleeps when there is nothing to do. The session asks to be polled
/// at least every 20 ms.
const IDLE_SLEEP: Duration = Duration::from_millis(2);

/// How long a teardown waits for the session to finish its disconnects before leaving them.
const CLOSE_WAIT: Duration = Duration::from_secs(2);

/// What the radio loop asks this daemon to connect to. Holds no password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The radio's own address ("IP Address (LAN)" on an IC-7760).
    pub host: Ipv4Addr,
    /// The radio's control port (50001 unless the operator moved it on the radio).
    pub control_port: u16,
    pub model: IcomModel,
    /// The network user the radio was set up with. Not secret, but not logged.
    pub user: String,
    /// The radio profile whose keychain entry holds the password.
    pub profile_id: u32,
    /// Where the rigctld protocol is served on localhost.
    pub rigctld_port: u16,
    /// The operator's D1/D2/D3 choice, for the CI-V daemon.
    pub data_mode: u8,
}

impl Target {
    /// The registry's key for this radio: its address and control port.
    pub fn key(&self) -> SocketAddrV4 {
        SocketAddrV4::new(self.host, self.control_port)
    }
}

/// The protocol core's model for a network Icom.
pub fn net_model(model: IcomModel) -> Option<Model> {
    Some(match model {
        IcomModel::Ic7610 => Model::Ic7610,
        IcomModel::Ic9700 => Model::Ic9700,
        IcomModel::Ic705 => Model::Ic705,
        IcomModel::Ic905 => Model::Ic905,
        IcomModel::Ic7760 => Model::Ic7760,
        IcomModel::Ic7300Mk2 => Model::Ic7300Mk2,
        IcomModel::Ic7300 => return None,
    })
}

/// Where a radio's password comes from: `Ok(None)` when none is saved, `Err` when the store could
/// not be read (the reason never carries the password).
pub type CredentialSource = dyn Fn(u32) -> Result<Option<String>, String> + Send + Sync;

static CREDENTIALS: OnceLock<Mutex<Option<Arc<CredentialSource>>>> = OnceLock::new();

/// Registers where passwords come from: the desktop shell's OS keychain, at start-up.
pub fn set_credential_source(source: Arc<CredentialSource>) {
    *CREDENTIALS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(source);
}

fn registered_password(profile_id: u32) -> Result<Option<String>, String> {
    let source = CREDENTIALS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    match source {
        Some(source) => source(profile_id),
        None => Ok(None),
    }
}

/// The retry ladder's clock, in milliseconds.
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// Milliseconds on a monotonic clock, from the first time anything here asked.
pub fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// The operator's words for a connect that failed, for the radio at `host`. Each names only what
/// can be told apart: a wrong address, the radio's network control off and a slot still held for
/// an earlier session all look like silence.
pub fn connect_failed_text(e: &ConnectError, host: Ipv4Addr, model: IcomModel) -> String {
    match e {
        ConnectError::NoAnswer(_) => format!(
            "No answer from {host}: check the address and that Network Control is ON. If wfview, \
             RS-BA1 or an earlier session had the radio, it can take up to 3 minutes to free it. \
             On another network, use a VPN."
        ),
        ConnectError::LoginRefused => "The radio refused the network user or password".into(),
        ConnectError::Busy(_) => "The radio is busy with another session".into(),
        ConnectError::NoRadio(_) => format!(
            "The radio at {host} did not offer an {}",
            net_model(model).map_or("Icom", Model::radio_name)
        ),
        ConnectError::RateNotOffered(_) => {
            "The radio offered no audio format for Nexus to name in its connection request".into()
        }
        ConnectError::NoCivPort => "The radio did not open its CI-V port for Nexus".into(),
        ConnectError::Request(_) => "Nexus could not build its request to the radio".into(),
    }
}

/// The operator's words for a working session that stopped working.
pub fn lost_text(reason: LossReason) -> &'static str {
    match reason {
        LossReason::LinkTimeout => "Session lost: the radio stopped answering",
        LossReason::PeerDisconnect => "Session lost: another program took the radio",
        LossReason::SocketError => "Session lost: the network kept failing",
    }
}

/// Why a start did not happen, in the operator's words.
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

/// What the session thread tells the daemon. Written only by that thread.
#[derive(Debug, Default)]
struct Shared {
    state: Option<State>,
    connected: Option<Radio>,
    failed: Option<ConnectError>,
    lost: Option<Loss>,
    closed: bool,
}

/// A frame for the radio, and where to say whether it went.
type Outgoing = (Vec<u8>, mpsc::SyncSender<Result<(), CivError>>);

/// The thread that owns the three sockets and the session.
struct SessionThread {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SessionThread {
    fn shared(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Asks the session to close (a handshake gives its token back and disconnects; a connected
    /// session closes the CI-V stream first), waits for it to finish, and says how it ended.
    fn close(&mut self) -> End {
        self.stop.store(true, Ordering::Relaxed);
        let until = Instant::now() + CLOSE_WAIT;
        while !self.shared().closed && Instant::now() < until {
            std::thread::sleep(IDLE_SLEEP);
        }
        if self.shared().closed {
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
        let shared = self.shared();
        match (&shared.failed, shared.lost) {
            (Some(e), _) => End::Failed(e.clone()),
            (None, Some(loss)) => End::Lost(loss.reason),
            (None, None) => End::Closed,
        }
    }
}

impl Drop for SessionThread {
    fn drop(&mut self) {
        if self.thread.is_some() {
            let _ = self.close();
        }
    }
}

/// One radio over its network connection. See the module notes.
pub struct IcomLanDaemon {
    /// Dropped first, so its safety key-up still has a live session to go out on.
    civ: Option<CivDaemon>,
    session: SessionThread,
    /// Released last, recording how the session ended.
    claim: Option<registry::Claim>,
    clock: Clock,
    target: Target,
    radio: Radio,
    findings: probe::Findings,
}

impl IcomLanDaemon {
    /// Connects to the radio and starts the CI-V daemon over the session, with the password from
    /// the registered credential source. Blocks for at most [`CONNECT_TIMEOUT`].
    pub fn start(target: &Target) -> Result<IcomLanDaemon, StartError> {
        Self::start_with(target, &registered_password, Arc::new(now_ms))
    }

    /// [`Self::start`] with a credential source of its own, and the clock the retry ladder runs
    /// on (milliseconds; [`now_ms`] in the app, a fake one in the tests).
    pub fn start_with(
        target: &Target,
        credentials: &CredentialSource,
        clock: Clock,
    ) -> Result<IcomLanDaemon, StartError> {
        let Some(model) = net_model(target.model) else {
            return Err(StartError::new(
                "This Icom has no network connection; use Serial (USB)",
            ));
        };
        let claim =
            registry::claim(target.key(), clock()).map_err(|r| StartError::new(r.text()))?;
        // The password, read at the moment of use. Nothing is sent without one.
        let password = match credentials(target.profile_id) {
            Ok(Some(p)) => p,
            Ok(None) => return Err(StartError::new("No password is saved for this radio")),
            Err(why) => {
                return Err(StartError::new(format!(
                    "Nexus could not read this radio's password from the system keychain ({why})"
                )))
            }
        };
        let login = match (User::new(&target.user), Secret::new(password)) {
            (Ok(user), Ok(password)) => Login { user, password },
            (Err(e), _) | (_, Err(e)) => return Err(StartError::new(capitalised(&e.to_string()))),
        };
        let mut config = Config::new(target.host, model, login);
        if let Some(port) = NonZeroU16::new(target.control_port) {
            config.control_port = port;
        }
        let (mut session, (inbox, outbox)) = spawn_session(config, target.host).map_err(|e| {
            StartError::new(format!(
                "Nexus could not open its network sockets toward {}: {e}",
                target.host
            ))
        })?;
        tempo_core::applog::info(
            "cat",
            &format!(
                "Icom network: connecting to {} at {} (network user: set)",
                model.radio_name(),
                target.host
            ),
        );
        let radio = match wait_connected(&session) {
            Ok(radio) => radio,
            Err(e) => {
                // A connect that walks away part-way gives back what the radio granted.
                let end = match session.close() {
                    End::Closed => End::Failed(e.clone()),
                    other => other,
                };
                let text = connect_failed_text(&e, target.host, target.model);
                tempo_core::applog::warn("cat", &format!("Icom network: {text}"));
                claim.ended(&end, clock(), &text);
                return Err(StartError::new(text));
            }
        };
        let default_addr = target.model.default_civ_addr();
        if radio.civ_addr != default_addr {
            tempo_core::applog::info(
                "cat",
                &format!(
                    "Icom network: the radio's CI-V address is {:02X}h, not the {}'s default \
                     {default_addr:02X}h; using the radio's",
                    radio.civ_addr,
                    model.radio_name()
                ),
            );
        }
        let link = Link {
            deadline: LAN_DEADLINE,
            keying: KeyingPolicy::Refused(KEYING_REFUSED),
        };
        let civ = match CivDaemon::start_with_link(
            Box::new(io::LanCivIo::new(inbox, outbox)),
            radio.civ_addr,
            target.rigctld_port,
            target.data_mode,
            Some(target.model),
            link,
        ) {
            Ok(civ) => civ,
            Err(e) => {
                let end = session.close();
                let text = format!(
                    "Nexus could not serve CAT on 127.0.0.1:{} ({e})",
                    target.rigctld_port
                );
                claim.ended(&end, clock(), &text);
                return Err(StartError::new(text));
            }
        };
        claim.connected();
        let findings = probe::read(
            &civ.engine_handle(),
            radio.civ_addr,
            target.model,
            target.data_mode,
        );
        tempo_core::applog::info(
            "cat",
            &format!(
                "Icom network: connected to {} at {}. {}",
                radio.name,
                target.host,
                probe::summary(target.model, target.data_mode, &findings)
            ),
        );
        Ok(IcomLanDaemon {
            civ: Some(civ),
            session,
            claim: Some(claim),
            clock,
            target: target.clone(),
            radio,
            findings,
        })
    }

    /// False once the session is lost or the CI-V engine has stopped: the radio loop then drops
    /// this daemon and asks for a new one, which the retry ladder spaces out.
    pub fn is_alive(&self) -> bool {
        self.session.shared().lost.is_none() && self.civ.as_ref().is_some_and(CivDaemon::is_alive)
    }

    /// The CI-V daemon carried over the session: the scope, the Sub levels and the receiver
    /// naming work through it unchanged.
    pub fn native(&self) -> &CivDaemon {
        self.civ
            .as_ref()
            .expect("the CI-V daemon lives as long as the daemon")
    }

    /// Where the rigctld protocol is served.
    pub fn local_addr(&self) -> SocketAddr {
        self.native().local_addr()
    }

    /// The radio this session connected to, as its capabilities entry described it.
    pub fn radio(&self) -> &Radio {
        &self.radio
    }

    /// What was read from the radio at connect.
    pub fn findings(&self) -> &probe::Findings {
        &self.findings
    }

    /// The registry's key for this radio.
    pub fn key(&self) -> SocketAddrV4 {
        self.target.key()
    }

    /// The connection as the operator reads it in Rig & CAT and Test CAT: who it is connected to,
    /// what was read at connect, and what may need a look.
    pub fn status(&self) -> String {
        let mut text = format!(
            "Connected to {} over the network (Beta: receive and control only).",
            self.radio.name
        );
        if !self.radio.name_matches() {
            text.push_str(&format!(
                " The radio says it is an {}, not an {}.",
                self.radio.name, self.radio.expected
            ));
        }
        text.push(' ');
        text.push_str(&probe::summary(
            self.target.model,
            self.target.data_mode,
            &self.findings,
        ));
        text
    }

    /// Why the session stopped, once it has.
    pub fn loss(&self) -> Option<&'static str> {
        self.session.shared().lost.map(|l| lost_text(l.reason))
    }
}

impl Drop for IcomLanDaemon {
    fn drop(&mut self) {
        // The CI-V daemon first: its Drop sends the key-up while the session is still there to
        // carry it. Then the session's own teardown: the CI-V stream close, the token given back,
        // the disconnects.
        drop(self.civ.take());
        let end = self.session.close();
        if let Some(claim) = self.claim.take() {
            let text = match &end {
                End::Lost(reason) => lost_text(*reason).to_string(),
                End::Failed(e) => connect_failed_text(e, self.target.host, self.target.model),
                End::Closed => String::new(),
            };
            if let End::Lost(reason) = &end {
                tempo_core::applog::warn("cat", &format!("Icom network: {}", lost_text(*reason)));
            }
            claim.ended(&end, (self.clock)(), &text);
        }
    }
}

/// "the network user name is…" → "The network user name is…".
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Waits for the session to connect, within [`CONNECT_TIMEOUT`], and gives up after
/// [`FIRST_ANSWER_TIMEOUT`] when nothing has answered the first probe.
fn wait_connected(thread: &SessionThread) -> Result<Radio, ConnectError> {
    let start = Instant::now();
    loop {
        {
            let shared = thread.shared();
            if let Some(radio) = &shared.connected {
                return Ok(radio.clone());
            }
            if let Some(e) = &shared.failed {
                return Err(e.clone());
            }
            let stage = match shared.state {
                Some(State::Connecting(stage)) => Some(stage),
                _ => None,
            };
            let waited = start.elapsed();
            if waited >= CONNECT_TIMEOUT {
                return Err(ConnectError::NoAnswer(stage.unwrap_or(Stage::ControlProbe)));
            }
            if waited >= FIRST_ANSWER_TIMEOUT && stage == Some(Stage::ControlProbe) {
                return Err(ConnectError::NoAnswer(Stage::ControlProbe));
            }
        }
        std::thread::sleep(IDLE_SLEEP);
    }
}

/// Opens the three sockets toward the radio and starts the session on its own thread. Returns
/// the thread, the CI-V frames it delivers, and where to hand it frames to send.
#[allow(clippy::type_complexity)] // two channel ends, named where they are used
fn spawn_session(
    config: Config,
    host: Ipv4Addr,
) -> std::io::Result<(
    SessionThread,
    (mpsc::Receiver<Vec<u8>>, mpsc::Sender<Outgoing>),
)> {
    let port = config.control_port.get();
    let open = || -> std::io::Result<UdpSocket> {
        let s = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        s.connect((host, port))?;
        s.set_nonblocking(true)?;
        Ok(s)
    };
    let sockets = [open()?, open()?, open()?];
    let local = |s: &UdpSocket| -> std::io::Result<SocketAddrV4> {
        match s.local_addr()? {
            SocketAddr::V4(a) => Ok(a),
            SocketAddr::V6(_) => Err(std::io::Error::other("not an IPv4 socket")),
        }
    };
    let locals = Locals {
        control: local(&sockets[0])?,
        civ: local(&sockets[1])?,
        audio: local(&sockets[2])?,
    };
    let (inbox_tx, inbox_rx) = mpsc::channel::<Vec<u8>>();
    let (outbox_tx, outbox_rx) = mpsc::channel::<Outgoing>();
    let shared = Arc::new(Mutex::new(Shared::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let thread = {
        let (shared, stop) = (shared.clone(), stop.clone());
        std::thread::Builder::new()
            .name("icomlan-session".into())
            .spawn(move || {
                let start = Instant::now();
                let ms = || start.elapsed().as_millis() as u64;
                let (session, actions) = Session::start(config, locals, ms());
                let mut owner = Owner {
                    session,
                    sockets,
                    host,
                    inbox: Some(inbox_tx),
                    shared,
                };
                owner.perform(actions, ms());
                owner.run(&outbox_rx, &stop, ms);
            })?
    };
    Ok((
        SessionThread {
            shared,
            stop,
            thread: Some(thread),
        },
        (inbox_rx, outbox_tx),
    ))
}

/// The session's one owner: the sockets, the session, and the ends of the channels.
struct Owner {
    session: Session,
    sockets: [UdpSocket; 3],
    host: Ipv4Addr,
    /// Dropped when the session is lost or closed, so the CI-V engine reading it stops.
    inbox: Option<mpsc::Sender<Vec<u8>>>,
    shared: Arc<Mutex<Shared>>,
}

impl Owner {
    fn shared(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn run(&mut self, outbox: &mpsc::Receiver<Outgoing>, stop: &AtomicBool, ms: impl Fn() -> u64) {
        let mut closing = false;
        let mut buf = [0u8; 2048];
        loop {
            let now = ms();
            if stop.load(Ordering::Relaxed) && !closing {
                closing = true;
                let actions = self.session.close(now);
                self.perform(actions, now);
            }
            // Frames the CI-V engine handed over, each answered at once.
            while let Ok((frame, reply)) = outbox.try_recv() {
                let result = self.session.send_civ(&frame, now).map(|actions| {
                    self.perform(actions, now);
                });
                let _ = reply.try_send(result);
            }
            // Whatever arrived.
            for role in [Role::Control, Role::Civ, Role::Audio] {
                loop {
                    match self.sockets[role as usize].recv(&mut buf) {
                        Ok(n) => {
                            let actions = self.session.on_datagram(role, &buf[..n], now);
                            self.perform(actions, now);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(e) => {
                            self.session.on_socket_error(role, SocketError::of(&e), now);
                            break;
                        }
                    }
                }
            }
            let actions = self.session.poll(now);
            self.perform(actions, now);
            let state = self.session.state();
            self.shared().state = Some(state);
            if state == State::Closed || self.shared().closed {
                break;
            }
            std::thread::sleep(IDLE_SLEEP);
        }
        self.inbox = None;
        self.shared().closed = true;
    }

    fn perform(&mut self, actions: Vec<Action>, now: u64) {
        for action in actions {
            match action {
                Action::Send(d) => {
                    if let Err(e) = self.sockets[d.role as usize].send(&d.bytes) {
                        self.session
                            .on_socket_error(d.role, SocketError::of(&e), now);
                    }
                }
                Action::Redirect { role, port } => {
                    if let Err(e) = self.sockets[role as usize].connect((self.host, port)) {
                        self.session.on_socket_error(role, SocketError::of(&e), now);
                    }
                }
                Action::Deliver(frame) => {
                    if let Some(inbox) = &self.inbox {
                        let _ = inbox.send(frame);
                    }
                }
                Action::Event(event) => self.event(event),
            }
        }
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Connected(radio) => self.shared().connected = Some(radio),
            Event::Failed(e) => self.shared().failed = Some(e),
            Event::Lost(loss) => {
                self.shared().lost = Some(loss);
                // The CI-V engine reading this stops, and its daemon is no longer alive.
                self.inbox = None;
            }
            Event::Closed => self.shared().closed = true,
        }
    }
}

/// The control port a radio listens on unless its operator moved it.
pub const DEFAULT_CONTROL_PORT: u16 = PORT_CONTROL;
