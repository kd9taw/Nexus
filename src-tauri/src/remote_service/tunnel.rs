//! The tunnel through the room: Nexus on another computer driving this station over the internet,
//! this station's end.
//!
//! When an approved computer opens its end of a pipe at the Remote service, the station's room asks
//! this station, on its main socket, to open the other (`pipeOpen`, sent only to a station that
//! advertised `x-nexus-pipe-version: 1`), and from then on passes each binary message between the
//! two ends unread (`remote/src/pipe.ts`). This station opens its end as a socket of its own
//! (`stations/:id/pipe`, on the pairing's credential, with no Nagle), and inside it runs the LAN
//! road's own channel from its TLS on (`lan::Tunnel`): TLS 1.3 with raw keys both ways, this station
//! proving the key it holds for its pairing with the service (`station_key`, the key every page
//! already checks its stream against), and the computer the device key pinned when the operator
//! approved it here; then the same hello, protocol 2 and command lane, with internet roads beside
//! it in place of the LAN's. The room only ever holds ciphertext.
//!
//! - **Who may hold a session** ([`Approved`]): a computer (or a phone) the service lists as
//!   approved for this station, whose listed key is the one pinned here when the shack approved it
//!   as one, under that same approval. A browser's pin is never one, and neither is the pin of an
//!   approval before. Read at each handshake and again after it; when it changes, a session whose
//!   computer is no longer on it ends at once, and one whose approval reached the end the service
//!   last listed for it ends within a status tick. The room closes a pipe as an approval ends, too.
//! - **Named by its key** (`lan::tunnel_device_id`): the shack names each computer from the key it
//!   pinned, under a label of the tunnel's own, as the LAN names its computers from theirs, never by
//!   the service's id. The authority, the command lane's journal and the roads key everything they
//!   hold by that name, and they are shared with the relay's road and the LAN's: so a device the
//!   service names, or a LAN computer, can never be one the tunnel takes, nor hold what it holds.
//! - **No pairing and no picture**: nothing pairs through the room, and no stream is signalled on
//!   the tunnel. The hosted stream has its own road.
//! - **The pipe** ([`bridge`]): each binary message is bytes of the TLS stream, in order; anything
//!   else on it, or a message over the room's bound, ends it. Either end's close is the other's,
//!   and the session's lease goes with it, so whatever it kept on the air halts on the radio loop's
//!   next tick.
//! - **At most [`MAX_TUNNELS`] at once**, the room's own bound for a station's pipes; a request
//!   past it is left unanswered, and the room tells the computer to try again.
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::watch;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use super::lan::Roster;
use super::transport::{credential, identifier};
// The largest message either end of the pipe may send: the room's bound.
use tempo_client::tunnel::PIPE_MESSAGE_BYTES;

/// What a test runs as the list of approved computers changes ([`Approved::on_set`]).
#[cfg(test)]
pub(crate) type OnSet = Box<dyn Fn(&[String]) + Send>;

/// Tunnels one station carries at once: the room's bound on a station's pipes.
pub const MAX_TUNNELS: usize = 4;
/// How long the station's end of the pipe has to open.
const OPEN: Duration = Duration::from_secs(10);

/// The computers approved here for the tunnel, each by the SHA-256 of the key pinned when it was
/// approved: the name it is taken as, and when its approval ends (ms since the epoch, the service's
/// own clock as listed). Its own lock, never held while another is taken: TLS asks it at each
/// handshake.
pub(crate) struct Approved {
    keys: Mutex<BTreeMap<[u8; 32], (String, u64)>>,
    revision: watch::Sender<u64>,
    revisions: AtomicU64,
    /// For a test: run with the names on it each time it changes, before [`Approved::set`] returns,
    /// so a test sees what the rest of the station holds at the moment a name joins or leaves it.
    #[cfg(test)]
    pub(crate) on_set: Mutex<Option<OnSet>>,
}

impl Default for Approved {
    fn default() -> Self {
        Self {
            keys: Mutex::default(),
            revision: watch::channel(0).0,
            revisions: AtomicU64::new(0),
            #[cfg(test)]
            on_set: Mutex::default(),
        }
    }
}

impl Approved {
    /// Who may from now on: each approved computer's name and approval's end by its pinned key. A
    /// change is told to every session at once; the same set again tells nothing. A lock a panic
    /// poisoned still takes the change and tells it, so a removal is never lost: [`Approved::paired`]
    /// then takes nobody, and every session asked again ends. Returns the names no longer on it, whose
    /// station control the caller revokes.
    pub(crate) fn set(&self, keys: BTreeMap<[u8; 32], (String, u64)>) -> Vec<String> {
        let mut held = self
            .keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *held == keys {
            return Vec::new();
        }
        let dropped = held
            .values()
            .filter(|(name, _)| !keys.values().any(|(kept, _)| kept == name))
            .map(|(name, _)| name.clone())
            .collect();
        *held = keys;
        #[cfg(test)]
        let names: Vec<String> = held.values().map(|(name, _)| name.clone()).collect();
        drop(held);
        #[cfg(test)]
        if let Some(on_set) = self.on_set.lock().ok().as_deref().and_then(Option::as_ref) {
            on_set(&names);
        }
        self.revisions.fetch_add(1, Ordering::SeqCst);
        self.revision.send_modify(|r| *r = r.wrapping_add(1));
        dropped
    }

    /// The names that may now, their approval not yet ended: who holds station control while hosted
    /// Remote is on. Nobody while the lock is poisoned. Takes this lock alone, as the operations
    /// authority asks it under its own.
    pub(crate) fn names(&self) -> Vec<String> {
        let Ok(keys) = self.keys.lock() else {
            return Vec::new();
        };
        let now = super::now_ms();
        keys.values()
            .filter(|(_, until)| now < *until)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// The device holding the key with this pin, if it may now: approved, and its approval not yet
    /// ended. Nobody while the lock is poisoned.
    pub(crate) fn paired(&self, pin: &[u8; 32]) -> Option<String> {
        let keys = self.keys.lock().ok()?;
        let (device, until) = keys.get(pin)?;
        (super::now_ms() < *until).then(|| device.clone())
    }

    /// How many times who may has changed, read without a lock.
    pub(crate) fn revisions(&self) -> u64 {
        self.revisions.load(Ordering::SeqCst)
    }

    /// Its lock poisoned, as a thread that panicked holding it would leave it.
    #[cfg(test)]
    pub(crate) fn poison(&self) {
        let _ = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _held = self.keys.lock();
                    panic!("the roster's lock, poisoned for a test");
                })
                .join()
        });
    }

    /// As the channel and its roads ask it.
    pub(crate) fn roster(self: &Arc<Self>) -> Roster {
        let (paired, subscribe) = (self.clone(), self.clone());
        Roster {
            paired: Arc::new(move |pin: &[u8; 32]| paired.paired(pin)),
            subscribe: Arc::new(move || subscribe.revision.subscribe()),
        }
    }
}

/// Where the station's end of a pipe is opened: the service, the station, and the pairing's own
/// credential, as the main socket is.
#[derive(Clone)]
pub(crate) struct Opener {
    pub origin: String,
    pub station_id: String,
    pub token: String,
}

impl Opener {
    /// The station's end of the pipe the room named, open, or why not.
    async fn open(
        &self,
        pipe_id: &str,
    ) -> Result<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        &'static str,
    > {
        if !identifier(&self.station_id) || !identifier(pipe_id) || !credential(&self.token) {
            return Err("invalidRequest");
        }
        let origin = self
            .origin
            .replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1);
        let mut request = format!("{origin}/api/remote/stations/{}/pipe", self.station_id)
            .into_client_request()
            .map_err(|_| "invalidRequest")?;
        let headers = request.headers_mut();
        headers.insert(
            "authorization",
            format!("Bearer {}", self.token)
                .parse()
                .map_err(|_| "invalidRequest")?,
        );
        headers.insert(
            "x-nexus-pipe-id",
            pipe_id.parse().map_err(|_| "invalidRequest")?,
        );
        // `true`: no Nagle. A Stop's frame, and its answer, go as they are written.
        let connect = tokio_tungstenite::connect_async_with_config(request, Some(config()), true);
        match tokio::time::timeout(OPEN, connect).await {
            Ok(Ok((socket, _))) => Ok(socket),
            _ => Err("serviceUnavailable"),
        }
    }
}

/// The pipe's socket: binary messages up to the room's bound, written as they come.
fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(PIPE_MESSAGE_BYTES))
        .max_frame_size(Some(PIPE_MESSAGE_BYTES))
        .write_buffer_size(0)
}

/// The pipe as one byte stream, for TLS (the client core's: both ends run the same).
pub(crate) use tempo_client::tunnel::bridge;

/// One tunnel, from the room's request to its end: the station's end of the pipe opened, and the
/// channel served over it until either end closes or Remote stops.
pub(crate) async fn run(
    opener: Opener,
    pipe_id: String,
    tunnel: super::lan::Tunnel,
    stop: watch::Receiver<bool>,
) {
    let socket = match opener.open(&pipe_id).await {
        Ok(socket) => socket,
        Err(why) => {
            tempo_core::applog::warn(
                "remote",
                &format!("through the room: the station's end of a pipe did not open ({why})"),
            );
            return;
        }
    };
    let (stream, pumps) = bridge(socket);
    tunnel.serve(stream, stop).await;
    pumps.abort();
}

/// What a connected station opens its ends of pipes with, and serves through them.
#[derive(Clone)]
pub(crate) struct Ends {
    pub opener: Opener,
    pub tunnel: super::lan::Tunnel,
}

/// How long a connecting station waits for the STUN server's address before its tunnel goes
/// without one: its internet roads then offer their host address alone, and a computer reaches
/// them through its relay.
const STUN_LOOKUP: Duration = Duration::from_secs(2);

/// The room's tunnel for a station connecting now, if it can serve one: a build of the service with
/// the LAN road, whose command lane it shares (`Control::tunnel`), and a station key TLS takes
/// (`Control::station_identity`). Its internet roads bind where the route to the internet leaves,
/// and learn their reflexive address from the STUN server the stream asks (`tempo_stream::stun`).
pub(super) async fn ends(
    client: &super::transport::Client,
    station_id: &str,
    token: &str,
    status: &super::SessionStatus,
) -> Option<Ends> {
    let (deps, identity, approved) = {
        let control = status.control.lock().ok()?;
        (
            control.tunnel.clone()?,
            control.station_identity.clone()?,
            control.approved.clone(),
        )
    };
    let lookup = tokio::task::spawn_blocking(|| {
        use std::net::ToSocketAddrs;
        tempo_stream::stun::SERVER
            .to_socket_addrs()
            .ok()?
            .find(std::net::SocketAddr::is_ipv4)
    });
    let stun = tokio::time::timeout(STUN_LOOKUP, lookup)
        .await
        .ok()
        .and_then(Result::ok)
        .flatten();
    let counted = approved.clone();
    let tunnel = super::lan::Tunnel::new(
        &deps,
        identity,
        approved.roster(),
        Arc::new(move || counted.revisions()),
        std::net::Ipv4Addr::UNSPECIFIED.into(),
        stun,
    )?;
    Some(Ends {
        opener: Opener {
            origin: client.origin().to_string(),
            station_id: station_id.to_string(),
            token: token.to_string(),
        },
        tunnel,
    })
}
