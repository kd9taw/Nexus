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
//! Only adapters that are not tunnels or virtual ones count (`tempo_stream::lan::left_out`), so a
//! full-tunnel VPN's private address is never where it listens, even with the route to the
//! internet leaving by it, and neither is WSL's or Hyper-V's switch. With more than one network
//! left the shack offers the operator a pick, a press at the shack only, kept in the switch's file
//! and read back only if it is a private address with a port that is not a system one. A picked
//! address that is gone for a while (sleep, a DHCP renewal, a cable out) is waited for, on, and
//! listened at again when it is back: since only the shack can turn this on, going off then would
//! leave a computer away from the shack locked out until someone walked back to it.
//!
//! ## Found by name, and Windows' firewall
//!
//! While it listens, the station advertises itself by name on that adapter only (DNS-SD through
//! Windows' own responder, `tempo_stream::lan::dnssd`), and withdraws the advert when it stops;
//! the shack says when Windows will not advertise it, so the address is typed. As ruled on
//! 2026-10-04 ("Windows' own prompt"), Nexus adds no firewall rule: Windows asks the first time it
//! listens, and every few seconds the station reads what the firewall says of the network it
//! listens on (`tempo_stream::lan::firewall`), so the shack can say what stands in the way.
//!
//! ## The ladder, each step reached only through the one before
//!
//! 1. TCP accept: the source is on the bound address's own subnet and under the caps ([`gate`]).
//! 2. TLS 1.3, raw public keys both ways ([`tls`]): the computer's key must be a paired one, or,
//!    while the pairing window is open, any key, for a pairing-only connection ([`pairing`]) that
//!    reaches nothing on the steps below. No application data is read before this completes.
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
//! keeps the reason for the shack to show, when "End remote control" is pressed and when there is
//! no LAN key. After a restart it comes back as it was left. The switch lives in its own file,
//! written only here: no Settings save, restore, import or Remote projection can turn it on, and a
//! form saved from an old copy cannot turn back on what went off by itself.
//!
//! ## Pairing, and only at the shack (the operator's rulings of 2026-10-04)
//!
//! The LAN key and the paired computers are the [`Book`]'s, in the OS credential store. A press
//! at the shack opens a ten-minute pairing window, and that press is the approval ("One press"):
//! a computer that proves its code is paired at once ([`pairing`]), and stays paired until it is
//! removed here ("Until revoked"). Turning this on or off, picking the network, making a code,
//! removing a computer and resetting the key are the shack's alone ("Only at the shack"): the
//! panel refuses each while its press comes through a stream, and nothing reaches them from a
//! remote road.
//!
//! **The grant.** A paired computer holds station control in the operations authority while LAN
//! is on, and only then: the authority reads who they are each time it reconciles, so a local
//! decision that clears every grant (Turn Remote on or off, take over, revoking a browser) leaves
//! them theirs, as `restore` does for hosted browsers. Turning LAN off, by the operator or by
//! itself, removing a computer and resetting the key each revoke it, after the authority has
//! stopped reading it, so the revoke stands.
//!
//! ## The other end
//!
//! The paired computer's side, its key, its pairing and its road, is `crate::lan_client`, which
//! speaks this wire with this module's own functions and versions.
mod book;
mod channel;
mod gate;
pub(crate) mod pairing;
#[cfg(test)]
pub(super) mod tests;
pub mod tls;

pub use book::Book;
pub(crate) use book::{device_id, valid_name, NAME_CHARS};

/// The LAN protocol, stream and operation versions this build speaks, in the order a hello names
/// them: the shack's own, and the ones its paired computers' window says.
pub(crate) const VERSIONS: (u8, u8, u8) = (
    channel::PROTOCOL_VERSION,
    tempo_stream::protocol::STREAM_VERSION,
    channel::OPERATION_VERSION,
);

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tempo_stream::lan::dnssd::Record;
use tempo_stream::lan::{Choice, Look, Network, NoNetwork};
use tokio::sync::{watch, Notify};

use super::operations::Authority;
use super::transport::Feeds;

/// The TCP port the shack listens on, and the UDP port of its stream, unless the operator sets
/// another: beside Field Day sync's 42073 and 42074.
pub const DEFAULT_PORT: u16 = tempo_stream::lan::PORT;
/// While on, how often the network is looked at again.
const LOOK_AGAIN: Duration = Duration::from_secs(1);
/// While listening, every how many looks the firewall is read again: a question answered at the
/// shack shows within seconds.
const FIREWALL_EVERY: u32 = 5;
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
}

impl Off {
    fn code(self) -> &'static str {
        match self {
            Off::EndedAtShack => "endedAtShack",
            Off::NoKey => "noKey",
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
    /// As it was left, or off: no file, a file that will not read, one this version does not
    /// understand, and one holding a pick that is not a private address or a system port (which no
    /// press here could have written) all read as off.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|switch| {
                switch.address.is_none_or(tempo_stream::lan::listenable) && switch.port >= 1024
            })
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
    /// While on and not listening, why (`noNetwork`, `chooseAddress`, `addressGone` for a pick
    /// this computer does not have right now, `portInUse`, `unavailable`); while off by itself,
    /// why (`endedAtShack`, `noKey`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    /// While on, the networks the operator may pick from: this computer's private addresses on
    /// adapters that are not tunnels or virtual ones.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub networks: Vec<NetworkView>,
    /// The address the operator picked, if any, on or off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picked: Option<String>,
    /// While listening: Windows advertises the station by name (`true`), or will not (`false`), and
    /// the address is typed. Absent until Windows says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub named: Option<bool>,
    /// While listening, what stands in the way in Windows' firewall on that network, when anything
    /// does (`tempo_stream::lan::firewall::says`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firewall: Option<&'static str>,
    /// The LAN key's fingerprint (SHA-256 of its SPKI, lowercase hex), once there is a key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The pairing window, while it is open: its code and when it closes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pairing: Option<book::PairingView>,
    /// The paired computers.
    pub devices: Vec<book::ComputerView>,
}

/// A network the operator may pick, as the shack shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NetworkView {
    /// This computer's address on it.
    pub address: String,
    /// Its adapter's name: "Wi-Fi", "Ethernet".
    pub name: String,
}

/// This computer's networks and where the listener goes for a pick: `tempo_stream::lan::look`, or
/// a test's own.
pub type Resolve = Arc<dyn Fn(Option<Ipv4Addr>) -> Look + Send + Sync>;

/// The station's advert by name while it listens: asked whether Windows says it stands, and
/// withdrawn when dropped, which may wait for Windows.
pub type Advert = Box<dyn Fn() -> Option<bool> + Send>;

/// Advertise the station by name (`tempo_stream::lan::dnssd::advertise`, or a test's own): `None`
/// where it cannot be.
pub type Advertise = Arc<dyn Fn(&Record) -> Option<Advert> + Send + Sync>;

/// What Windows' firewall says of the network a choice is on (`tempo_stream::lan::firewall`, or a
/// test's own). It may block for a moment.
pub type FirewallSays = Arc<dyn Fn(&Choice) -> Option<&'static str> + Send + Sync>;

/// The real ones, from Windows.
pub(super) fn advertise() -> Advertise {
    Arc::new(|record| {
        let advert = tempo_stream::lan::dnssd::advertise(record)?;
        Some(Box::new(move || advert.standing()) as Advert)
    })
}

pub(super) fn firewall_says() -> FirewallSays {
    Arc::new(|choice| {
        tempo_stream::lan::firewall::read(&choice.id)
            .as_ref()
            .and_then(tempo_stream::lan::firewall::says)
    })
}

/// What the listener serves from.
#[derive(Clone)]
pub(super) struct Deps {
    pub authority: Arc<Authority>,
    pub engine: crate::SharedEngine,
    pub feeds: Feeds,
    /// The LAN key and the paired computers.
    pub book: Arc<Book>,
    pub resolve: Resolve,
    pub advertise: Advertise,
    pub firewall: FirewallSays,
}

struct State {
    switch: Mutex<Switch>,
    /// The switch's `on`, for the operations authority, which reads it under its own lock and so
    /// must never wait on this one's file write.
    on: AtomicBool,
    status: Mutex<LanStatus>,
    path: PathBuf,
    wake: Notify,
    closed: AtomicBool,
    authority: Arc<Authority>,
    book: Arc<Book>,
    /// The gate of the listener started last, for a test to read ([`Lan::open_from`]).
    #[cfg(test)]
    gate: Mutex<Option<Arc<gate::Gate>>>,
}

impl State {
    fn switch(&self) -> Switch {
        self.switch.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Change the switch, keep it, and wake the listener's thread. Said at once, so a status read
    /// straight after a press shows it; the listener then says where it listens.
    ///
    /// Off, however it went off: the pairing window closes, and no paired computer keeps station
    /// control. The authority stops reading them (`on`) before they are revoked, so the revoke
    /// stands.
    fn decide(&self, change: impl FnOnce(&mut Switch)) {
        if let Ok(mut switch) = self.switch.lock() {
            change(&mut switch);
            if switch.save(&self.path).is_err() {
                tempo_core::applog::warn("remote", "on this network: the switch was not saved");
            }
            self.on.store(switch.on, Ordering::SeqCst);
            // The networks stay listed while it is on, so a pick does not take the picker away.
            let networks = match (switch.on, self.status.lock()) {
                (true, Ok(status)) => status.networks.clone(),
                _ => Vec::new(),
            };
            self.report(LanStatus {
                on: switch.on,
                listening: None,
                reason: switch.off.map(Off::code),
                networks,
                ..LanStatus::default()
            });
        }
        if !self.on.load(Ordering::SeqCst) {
            self.book.close();
            release(&self.authority, &self.book.ids());
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
    /// The switch kept at `path`, and the thread that keeps the listener in step with it. The
    /// book is read there, on the listener's own thread, before anything listens.
    ///
    /// From here the operations authority reads the paired computers as holding station control
    /// whenever LAN is on (see the module header).
    pub(super) fn start(path: PathBuf, deps: Deps) -> Self {
        let switch = Switch::load(&path);
        let state = Arc::new(State {
            on: AtomicBool::new(switch.on),
            switch: Mutex::new(switch),
            status: Mutex::new(LanStatus::default()),
            path,
            wake: Notify::new(),
            closed: AtomicBool::new(false),
            authority: deps.authority.clone(),
            book: deps.book.clone(),
            #[cfg(test)]
            gate: Mutex::new(None),
        });
        // Weak: the state holds the authority, which holds this.
        let held = Arc::downgrade(&state);
        deps.authority.hold_lan_devices(Box::new(move || {
            held.upgrade()
                .filter(|state| state.on.load(Ordering::SeqCst))
                .map_or_else(Vec::new, |state| state.book.ids())
        }));
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
                reason: Some("unavailable"),
                ..LanStatus::default()
            });
        }
        Self { state }
    }

    /// The operator turned it on at the shack, on `address` and `port` (`None`: each as it was,
    /// the picked address kept).
    pub fn turn_on(&self, address: Option<&str>, port: Option<u16>) -> Result<(), &'static str> {
        let address = address.map(private).transpose()?;
        if port.is_some_and(|p| p < 1024) {
            return Err("invalidRequest");
        }
        self.state.decide(|switch| {
            switch.on = true;
            switch.address = address.or(switch.address);
            switch.port = port.unwrap_or(switch.port);
            switch.off = None;
        });
        Ok(())
    }

    /// The operator picked where it listens, at the shack: one of this computer's private
    /// addresses, or `None` to let the shack choose (as the module header says). Kept on or off.
    pub fn pick(&self, address: Option<&str>) -> Result<(), &'static str> {
        let address = address.map(private).transpose()?;
        self.state.decide(|switch| switch.address = address);
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

    /// Pair a computer, at the shack: the pairing window opens with a new code, and this press is
    /// the approval (as ruled on 2026-10-04, "One press"). Only while LAN is on.
    pub fn pair(&self) -> Result<(), &'static str> {
        if !self.state.on.load(Ordering::SeqCst) {
            return Err("invalidRequest");
        }
        self.state.book.open(Instant::now(), super::now_ms())
    }

    /// Close the pairing window before its ten minutes are up.
    pub fn cancel_pairing(&self) {
        self.state.book.close();
    }

    /// Remove a paired computer, at the shack: refused from now on, its session and anything it
    /// keeps on the air ended at once, its station control revoked, and then the store told. The
    /// store can block, so a caller off the radio's path makes this call.
    pub fn revoke(&self, device: &str) -> Result<(), &'static str> {
        if !self.state.book.forget(device) {
            return Err("invalidRequest");
        }
        release(&self.state.authority, &[device.to_string()]);
        self.state.book.keep()
    }

    /// Reset this station's network identity, at the shack: every paired computer out at once as a
    /// removal is, then a new LAN key with nobody paired to it, and the listener on the new key.
    /// The store can block, as for [`Lan::revoke`].
    pub fn reset(&self) -> Result<(), &'static str> {
        let gone = self.state.book.forget_all();
        release(&self.state.authority, &gone);
        let renewed = self.state.book.renew();
        self.state.wake.notify_one();
        renewed
    }

    pub fn status(&self) -> LanStatus {
        let mut status = self
            .state
            .status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default();
        let view = self.state.book.view(Instant::now());
        status.key = view.key;
        status.pairing = view.pairing;
        status.devices = view.devices;
        status.picked = self.state.switch().address.map(|ip| ip.to_string());
        status
    }

    /// How many connections from `source` the listener holds now, as its gate counts them: a
    /// connection is let go when it ends, a refused key's only once the computer's close reaches
    /// the shack or its drain gives up (`channel::drain`).
    #[cfg(test)]
    fn open_from(&self, source: std::net::IpAddr) -> usize {
        let gate = self.state.gate.lock().ok().and_then(|gate| gate.clone());
        gate.map_or(0, |gate| gate.open(source))
    }
}

/// An address the operator gave: a private IPv4 address, or refused.
fn private(text: &str) -> Result<Ipv4Addr, &'static str> {
    text.parse::<Ipv4Addr>()
        .ok()
        .filter(|ip| tempo_stream::lan::listenable(*ip))
        .ok_or("invalidRequest")
}

/// These paired computers keep no station control: each is revoked as the shack revokes a
/// browser's, which ends what its lease keeps on the air at once (`permit_station`).
fn release(authority: &Authority, devices: &[String]) {
    for device in devices {
        let _ = authority.permit_station(device, false);
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
    // The LAN key and the paired computers, from the credential store, before anything listens.
    deps.book.load();
    let mut running: Option<Listener> = None;
    let mut looks: u32 = 0;
    while !state.closed.load(Ordering::SeqCst) {
        let want = state.switch();
        if !want.on {
            if let Some(listener) = running.take() {
                listener.stop().await;
            }
            state.report(LanStatus {
                on: false,
                reason: want.off.map(Off::code),
                ..LanStatus::default()
            });
            state.wake.notified().await;
            continue;
        }
        let look = (deps.resolve)(want.address);
        let reason = match look.network {
            Err(missing) => {
                if let Some(listener) = running.take() {
                    listener.stop().await;
                }
                // A pick this computer does not have right now is waited for, on: see the module
                // header.
                Some(match missing {
                    NoNetwork::Unavailable => "unavailable",
                    NoNetwork::Gone => "addressGone",
                    NoNetwork::Choose => "chooseAddress",
                })
            }
            // On the same network and port, with the same key: as it was.
            Ok(network)
                if running
                    .as_ref()
                    .is_some_and(|l| l.is(network, want.port, deps.book.generation())) =>
            {
                None
            }
            Ok(network) => {
                if let Some(listener) = running.take() {
                    listener.stop().await;
                }
                let Some((identity, generation)) = deps.book.identity() else {
                    off_by_itself(&state, Off::NoKey);
                    continue;
                };
                match Listener::start(network, want.port, &deps, identity, generation).await {
                    Ok(mut listener) => {
                        tempo_core::applog::info(
                            "remote",
                            &format!("on this network: listening on {}", listener.at()),
                        );
                        listener.advert = advert(&deps, look.chosen(), network, want.port);
                        looks = 0;
                        #[cfg(test)]
                        if let Ok(mut gate) = state.gate.lock() {
                            *gate = Some(listener.gate.clone());
                        }
                        running = Some(listener);
                        None
                    }
                    Err(reason) => Some(reason),
                }
            }
        };
        if let (Some(listener), Some(choice)) = (running.as_mut(), look.chosen()) {
            // Read on a task of its own, never on this loop's path, so Off never waits behind
            // Windows' firewall: its answer is taken at the first look after it comes, and
            // another read starts only once none is under way.
            if let Some(read) = listener.reading.take_if(|read| read.is_finished()) {
                listener.firewall = read.await.unwrap_or(None);
            }
            if looks.is_multiple_of(FIREWALL_EVERY) && listener.reading.is_none() {
                let (says, choice) = (deps.firewall.clone(), choice.clone());
                listener.reading = Some(tokio::task::spawn_blocking(move || says(&choice)));
            }
        }
        looks = looks.wrapping_add(1);
        state.report(LanStatus {
            on: true,
            listening: running.as_ref().map(Listener::at),
            reason,
            networks: look
                .choices
                .iter()
                .map(|choice| NetworkView {
                    address: choice.network.address().to_string(),
                    name: choice.name.clone(),
                })
                .collect(),
            named: running.as_ref().and_then(Listener::named),
            firewall: running.as_ref().and_then(|listener| listener.firewall),
            ..LanStatus::default()
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

/// The station's advert by name for a listener at `network` and `port`, on the adapter of
/// `choice`: "Nexus" and its key's tag, never the computer's name (`tempo_stream::lan::dnssd`).
/// `None` when Windows will not advertise it, or there is no key to name it by.
fn advert(deps: &Deps, choice: Option<&Choice>, network: Network, port: u16) -> Option<Advert> {
    let key = deps.book.view(Instant::now()).key?;
    let record = Record::new(
        &key,
        channel::PROTOCOL_VERSION,
        SocketAddrV4::new(network.address(), port),
        choice.map_or(0, |choice| choice.index),
    )?;
    (deps.advertise)(&record)
}

/// The listening socket and the connections it has accepted.
struct Listener {
    network: Network,
    port: u16,
    /// The generation of the LAN key it presents: a reset moves it, and the listener restarts.
    generation: u64,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
    /// Its advert by name, withdrawn when it stops.
    advert: Option<Advert>,
    /// What Windows' firewall says of its network, as last read.
    firewall: Option<&'static str>,
    /// The firewall read under way, if one is. Left to end on its own when the listener stops.
    reading: Option<tokio::task::JoinHandle<Option<&'static str>>>,
    /// Its gate, for a test to read.
    #[cfg(test)]
    gate: Arc<gate::Gate>,
}

impl Listener {
    /// Listen on `network`'s address at `port`, TCP. Refuses any address that is not private,
    /// however it was chosen.
    async fn start(
        network: Network,
        port: u16,
        deps: &Deps,
        identity: tls::Identity,
        generation: u64,
    ) -> Result<Self, &'static str> {
        if !tempo_stream::lan::listenable(network.address()) {
            return Err("noNetwork");
        }
        let (paired, pairing) = deps.book.verifier();
        let tls = tls::server(&identity, paired, pairing).map_err(|_| "unavailable")?;
        let desk = pairing::Desk::new(deps.book.clone(), &identity).ok_or("unavailable")?;
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
            desk,
            network,
            port,
            gate: Arc::new(gate::Gate::default()),
            handshake: channel::HANDSHAKE,
            silence: channel::SILENCE,
        };
        let (stop, stopped) = watch::channel(false);
        #[cfg(test)]
        let gate = shared.gate.clone();
        let task = tokio::spawn(accept(socket, shared, stopped));
        Ok(Self {
            network,
            port,
            generation,
            stop,
            task,
            advert: None,
            firewall: None,
            reading: None,
            #[cfg(test)]
            gate,
        })
    }

    fn is(&self, network: Network, port: u16, generation: u64) -> bool {
        self.network == network && self.port == port && self.generation == generation
    }

    fn at(&self) -> String {
        format!("{}:{}", self.network.address(), self.port)
    }

    /// Is it advertised by name? With no advert, it is not.
    fn named(&self) -> Option<bool> {
        self.advert.as_ref().map_or(Some(false), |advert| advert())
    }

    /// Close the port and end every connection: each ends its stream, the stream's presence at
    /// once, and its lease, before this returns. Then the advert is withdrawn, off this thread,
    /// since that waits for Windows.
    async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
        if let Some(advert) = self.advert {
            let _ = tokio::task::spawn_blocking(move || drop(advert)).await;
        }
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
