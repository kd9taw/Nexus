//! Remote over this network: the shack's own listener, for a paired computer on the same network,
//! with no relay, no sign-in, no STUN and no internet (the operator's track of 2026-10-04, "Native
//! app over LAN"). The window on the other computer reaches the same station the relay does, by a
//! different road and never a weaker one: everything after the handshake is the hosted chain.
//!
//! ## Where it may listen (the operator's ruling of 2026-10-04: "any private network")
//!
//! On one private IPv4 address of this computer's, and only there: never `0.0.0.0`, never IPv6.
//! Private means exactly the RFC 1918 ranges, 10.0.0.0/8, 172.16.0.0/12 and 192.168.0.0/16
//! (`tempo_stream::lan::listenable`). Loopback, link-local 169.254/16, the carrier-NAT and overlay
//! space 100.64/10 (Tailscale), multicast, broadcast, the documentation ranges and every public
//! address are refused, at the choice and again at the bind. The address is the operator's pick,
//! or with none the one the route to the internet leaves by when that is private, or else this
//! computer's only private address; on whatever private network the shack is on, as ruled. The
//! listener follows that address while it is on, and looks again every second.
//!
//! ## The ladder, each step reached only through the one before
//!
//! 1. TCP accept: the source is on the bound address's own subnet and under the caps ([`gate`]).
//! 2. TLS 1.3, raw public keys both ways ([`tls`]): the computer's key must be a paired one. No
//!    application data is read before this completes.
//! 3. The hello ([`channel`]): the versions must be the shack's own, or the computer is told which
//!    side to update (as ruled on 2026-10-04). The shack stamps the session itself, and the device
//!    from the key.
//! 4. The grant and the lease: the operations authority, unchanged, with the lease bound to this
//!    connection (`operations::LanConnection`).
//! 5. The offer: `stream::admit`, unchanged: grant and lease (A3), the switches, the device key's
//!    signature from the same key as step 2 (A5), the offer check (A4), a window to capture. Only
//!    then is a UDP socket opened, on the same address, at the same port number.
//! 6. DTLS: the page's certificate is the one the device key signed (`Streaming::connected`).
//! 7. Presence: only a heartbeat naming a fresh picture renews it. Input, the held PTT and the
//!    microphone are taken only while it is live.
//!
//! ## Caps and limits
//!
//! | | |
//! |---|---|
//! | Connections open | 4 in all, 2 per address |
//! | New connections | 10 a minute per address |
//! | Failed handshakes | 5 in a minute, and the address is ignored for 5 minutes |
//! | The handshake (TLS, upgrade, hello) | 5 s |
//! | A connected computer's silence | 15 s, then it is gone (it pings every 5 s) |
//! | A message, either way | 8 KiB |
//! | Stream signals | 48 in 10 s, a close always allowed |
//!
//! The listener has a thread and a runtime of its own, and reaches the radio only through the
//! operations authority, which never blocks, so a flood can slow it and never the radio loop.
//!
//! ## The wire, after TLS: WebSocket text frames of JSON
//!
//! From the computer: `{"type":"hello","protocol":1,"stream":1,"operation":4}` first, then
//! `operationRequest` (`state`, `acquire`, `heartbeat`, `release` and `stopTransmit`, as on the
//! relay's lane, at operation version 4), and `streamSignal` exactly as a page hands it to the
//! relay.
//! From the shack: `welcome` (the session, device and LAN station ids it stamped) or `refused`
//! (`updateStation` / `updateComputer`), `operationResponse`, `streamSignal` and `streamState` as
//! the relay hands them to a page, and `status` (`rigKeyed`, for the page's ▲ TX and its
//! microphone's mute).
//!
//! ## Off by default, and off by itself
//!
//! The switch ([`Switch`]) is off until the operator turns it on, and off means no socket at all.
//! Turning it off closes the port and ends every session, each stream's presence with it, so
//! anything it kept on the air halts on the radio loop's next tick. It also goes off by itself, and
//! keeps the reason for the shack to show, when "End remote control" is pressed, when there is no
//! LAN key, and when the address the operator picked is gone. After a restart it comes back as it
//! was left. The switch lives in its own file, written only here: no Settings save, restore, import
//! or Remote projection can turn it on, and a form saved from an old copy cannot turn back on what
//! went off by itself.
//!
//! ## What comes later
//!
//! The LAN key and the paired computers' keys, in the OS keychain, arrive with pairing: until then
//! [`Keys::none`] holds no key, so the listener does not start. The window's own client, discovery
//! and the firewall come after that.
mod channel;
mod gate;
#[cfg(test)]
mod tests;
pub mod tls;

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tempo_stream::lan::{Network, NoNetwork};
use tokio::sync::{watch, Notify};

use super::operations::Authority;
use super::transport::Feeds;

/// The TCP port the shack listens on, and the UDP port of its stream, unless the operator sets
/// another: beside Field Day sync's 42073 and 42074.
pub const DEFAULT_PORT: u16 = 42075;
/// While on, how often the network is looked at again.
const LOOK_AGAIN: Duration = Duration::from_secs(1);
/// How long a stopping listener waits for its connections to end before it aborts them.
const WIND_DOWN: Duration = Duration::from_secs(2);

fn default_port() -> u16 {
    DEFAULT_PORT
}

/// Why the switch went off by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Off {
    /// "End remote control" was pressed at the shack.
    EndedAtShack,
    /// This station has no LAN key to prove itself with.
    NoKey,
    /// The address the operator picked is no longer this computer's.
    AddressGone,
}

impl Off {
    fn code(self) -> &'static str {
        match self {
            Off::EndedAtShack => "endedAtShack",
            Off::NoKey => "noKey",
            Off::AddressGone => "addressGone",
        }
    }
}

/// Remote over this network as the operator left it, in `remote-lan.json` beside `settings.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Switch {
    #[serde(default)]
    pub on: bool,
    /// The address the operator picked, if any (see the module header).
    #[serde(default)]
    pub address: Option<Ipv4Addr>,
    #[serde(default = "default_port")]
    pub port: u16,
    /// Why it went off by itself, until it is turned on again.
    #[serde(default)]
    pub off: Option<Off>,
}

impl Default for Switch {
    fn default() -> Self {
        Self {
            on: false,
            address: None,
            port: DEFAULT_PORT,
            off: None,
        }
    }
}

impl Switch {
    /// As it was left, or off: no file, a file that will not read, or one this version does not
    /// understand all read as off.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Written whole and moved into place, so a crash mid-write leaves the last one.
    fn save(&self, path: &Path) -> std::io::Result<()> {
        let partial = path.with_extension("json.partial");
        std::fs::write(&partial, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&partial, path)
    }
}

/// Remote over this network as the shack sees it now.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanStatus {
    pub on: bool,
    /// Where it listens now, `address:port`, the stream's UDP port the same number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listening: Option<String>,
    /// While on and not listening, why (`noNetwork`, `chooseAddress`, `portInUse`, `unavailable`);
    /// while off by itself, why (`endedAtShack`, `noKey`, `addressGone`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

/// What the LAN road needs from the station's identity, which pairing keeps.
#[derive(Clone)]
pub struct Keys {
    /// The station's LAN identity, read when the listener starts. `None`: no key, no listener.
    pub identity: Arc<dyn Fn() -> Option<tls::Identity> + Send + Sync>,
    /// Which paired computer holds a key.
    pub paired: tls::Paired,
}

impl Keys {
    /// No LAN key and no paired computer: the state until pairing exists.
    pub fn none() -> Self {
        Self {
            identity: Arc::new(|| None),
            paired: Arc::new(|_| None),
        }
    }
}

/// Where the listener goes for a pick: `tempo_stream::lan::network`, or a test's own network.
pub type Resolve = Arc<dyn Fn(Option<Ipv4Addr>) -> Result<Network, NoNetwork> + Send + Sync>;

/// What the listener serves from.
#[derive(Clone)]
pub(super) struct Deps {
    pub authority: Arc<Authority>,
    pub engine: crate::SharedEngine,
    pub feeds: Feeds,
    pub keys: Keys,
    pub resolve: Resolve,
}

struct State {
    switch: Mutex<Switch>,
    status: Mutex<LanStatus>,
    path: PathBuf,
    wake: Notify,
    closed: AtomicBool,
}

impl State {
    fn switch(&self) -> Switch {
        self.switch.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Change the switch, keep it, and wake the listener's thread. Said at once, so a status read
    /// straight after a press shows it; the listener then says where it listens.
    fn decide(&self, change: impl FnOnce(&mut Switch)) {
        if let Ok(mut switch) = self.switch.lock() {
            change(&mut switch);
            if switch.save(&self.path).is_err() {
                tempo_core::applog::warn("remote", "on this network: the switch was not saved");
            }
            self.report(LanStatus {
                on: switch.on,
                listening: None,
                reason: switch.off.map(Off::code),
            });
        }
        self.wake.notify_one();
    }

    fn report(&self, status: LanStatus) {
        if let Ok(mut current) = self.status.lock() {
            *current = status;
        }
    }
}

/// Remote over this network's switch and its listener, which runs on a thread of its own.
pub struct Lan {
    state: Arc<State>,
}

impl Lan {
    /// The switch kept at `path`, and the thread that keeps the listener in step with it.
    pub(super) fn start(path: PathBuf, deps: Deps) -> Self {
        let state = Arc::new(State {
            switch: Mutex::new(Switch::load(&path)),
            status: Mutex::new(LanStatus::default()),
            path,
            wake: Notify::new(),
            closed: AtomicBool::new(false),
        });
        let running = state.clone();
        let spawned = std::thread::Builder::new()
            .name("nexus-remote-lan".into())
            .spawn(move || {
                if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    runtime.block_on(supervise(running, deps));
                }
            });
        if spawned.is_err() {
            state.report(LanStatus {
                on: state.switch().on,
                listening: None,
                reason: Some("unavailable"),
            });
        }
        Self { state }
    }

    /// The operator turned it on at the shack, on `address` (`None`: as the module header says) and
    /// `port` (`None`: as it was).
    pub fn turn_on(&self, address: Option<&str>, port: Option<u16>) -> Result<(), &'static str> {
        let address = match address {
            None => None,
            Some(text) => Some(
                text.parse::<Ipv4Addr>()
                    .ok()
                    .filter(|ip| tempo_stream::lan::listenable(*ip))
                    .ok_or("invalidRequest")?,
            ),
        };
        if port.is_some_and(|p| p < 1024) {
            return Err("invalidRequest");
        }
        self.state.decide(|switch| {
            switch.on = true;
            switch.address = address;
            switch.port = port.unwrap_or(switch.port);
            switch.off = None;
        });
        Ok(())
    }

    /// The operator turned it off at the shack.
    pub fn turn_off(&self) {
        self.state.decide(|switch| {
            switch.on = false;
            switch.off = None;
        });
    }

    /// "End remote control" was pressed at the shack: every LAN session ends and the port closes,
    /// and the shack says why until the operator turns it on again.
    pub fn end_at_shack(&self) {
        if self.state.switch().on {
            off_by_itself(&self.state, Off::EndedAtShack);
        }
    }

    pub fn status(&self) -> LanStatus {
        self.state
            .status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }
}

impl Drop for Lan {
    fn drop(&mut self) {
        self.state.closed.store(true, Ordering::SeqCst);
        self.state.wake.notify_one();
    }
}

fn off_by_itself(state: &State, why: Off) {
    tempo_core::applog::info(
        "remote",
        &format!("on this network: turned off ({})", why.code()),
    );
    state.decide(|switch| {
        switch.on = false;
        switch.off = Some(why);
    });
}

/// Keep the listener in step with the switch and the network, until the service goes.
async fn supervise(state: Arc<State>, deps: Deps) {
    let mut running: Option<Listener> = None;
    while !state.closed.load(Ordering::SeqCst) {
        let want = state.switch();
        if !want.on {
            if let Some(listener) = running.take() {
                listener.stop().await;
            }
            state.report(LanStatus {
                on: false,
                listening: None,
                reason: want.off.map(Off::code),
            });
            state.wake.notified().await;
            continue;
        }
        let reason = match (deps.resolve)(want.address) {
            Err(NoNetwork::Gone) => {
                if let Some(listener) = running.take() {
                    listener.stop().await;
                }
                off_by_itself(&state, Off::AddressGone);
                continue;
            }
            Err(missing) => {
                if let Some(listener) = running.take() {
                    listener.stop().await;
                }
                Some(match missing {
                    NoNetwork::Unavailable => "unavailable",
                    _ => "chooseAddress",
                })
            }
            Ok(network) if running.as_ref().is_some_and(|l| l.is(network, want.port)) => None,
            Ok(network) => {
                if let Some(listener) = running.take() {
                    listener.stop().await;
                }
                let Some(identity) = (deps.keys.identity)() else {
                    off_by_itself(&state, Off::NoKey);
                    continue;
                };
                match Listener::start(network, want.port, &deps, identity).await {
                    Ok(listener) => {
                        tempo_core::applog::info(
                            "remote",
                            &format!("on this network: listening on {}", listener.at()),
                        );
                        running = Some(listener);
                        None
                    }
                    Err(reason) => Some(reason),
                }
            }
        };
        state.report(LanStatus {
            on: true,
            listening: running.as_ref().map(Listener::at),
            reason,
        });
        tokio::select! {
            _ = state.wake.notified() => {}
            _ = tokio::time::sleep(LOOK_AGAIN) => {}
        }
    }
    if let Some(listener) = running.take() {
        listener.stop().await;
    }
}

/// The listening socket and the connections it has accepted.
struct Listener {
    network: Network,
    port: u16,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl Listener {
    /// Listen on `network`'s address at `port`, TCP. Refuses any address that is not private,
    /// however it was chosen.
    async fn start(
        network: Network,
        port: u16,
        deps: &Deps,
        identity: tls::Identity,
    ) -> Result<Self, &'static str> {
        if !tempo_stream::lan::listenable(network.address()) {
            return Err("noNetwork");
        }
        let tls = tls::server(&identity, deps.keys.paired.clone()).map_err(|_| "unavailable")?;
        let socket = tokio::net::TcpListener::bind(SocketAddr::new(network.address().into(), port))
            .await
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::AddrInUse => "portInUse",
                _ => "noNetwork",
            })?;
        let shared = channel::Shared {
            authority: deps.authority.clone(),
            engine: deps.engine.clone(),
            feeds: deps.feeds.clone(),
            tls,
            identity: Arc::new(identity),
            paired: deps.keys.paired.clone(),
            network,
            port,
            gate: Arc::new(gate::Gate::default()),
            handshake: channel::HANDSHAKE,
            silence: channel::SILENCE,
        };
        let (stop, stopped) = watch::channel(false);
        let task = tokio::spawn(accept(socket, shared, stopped));
        Ok(Self {
            network,
            port,
            stop,
            task,
        })
    }

    fn is(&self, network: Network, port: u16) -> bool {
        self.network == network && self.port == port
    }

    fn at(&self) -> String {
        format!("{}:{}", self.network.address(), self.port)
    }

    /// Close the port and end every connection: each ends its stream, the stream's presence at
    /// once, and its lease, before this returns.
    async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

async fn accept(
    socket: tokio::net::TcpListener,
    shared: channel::Shared,
    mut stop: watch::Receiver<bool>,
) {
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = stop.changed() => break,
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
            accepted = socket.accept() => match accepted {
                Ok((stream, peer)) => {
                    connections.spawn(channel::connection(stream, peer, shared.clone(), stop.clone()));
                }
                // Out of descriptors, say: wait a moment rather than spin.
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            },
        }
    }
    drop(socket);
    // Every connection has seen `stop` and is ending; any that has not ended by now is aborted,
    // and its lease still goes as it is dropped.
    let _ = tokio::time::timeout(WIND_DOWN, async {
        while connections.join_next().await.is_some() {}
    })
    .await;
    connections.shutdown().await;
}
