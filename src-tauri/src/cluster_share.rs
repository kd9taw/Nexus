//! One cluster login per callsign on this computer, however many Nexus windows are open.
//!
//! Each window is its own process. Before this, each one logged into every DX-cluster node and
//! both RBN feeds under the same call, and a node that allows one session per call rejected or
//! bumped the other one, over and over, until it banned the call.
//!
//! **The lease.** The window that wants the cluster first takes an advisory lock on
//! `cluster-<CALL>.lock` in the machine-local base config dir. This is the same kind of std file
//! lock a radio window holds on its profile (`hold_profile_lock`), and it fails OPEN the same way:
//! only "another process holds it" makes a window follow. A lock that cannot be made at all (an
//! unwritable folder, a filesystem without locks) leaves the window logging in for itself, as
//! before this existed. The holder runs every cluster feed, human nodes and RBN alike, under ITS
//! settings: its node list and its SSID. The lock is keyed by the bare callsign, so windows set up
//! for different calls stay independent. It lives in the base config dir, never in the shared data
//! dir: that one can be a network folder for a multi-PC shack, and each computer there logs in for
//! itself.
//!
//! **The relay.** The holder listens on `127.0.0.1:<ephemeral>` and writes the port and a fresh
//! secret to `cluster-<CALL>.relay` beside the lock: owner-only on unix, the per-user config dir
//! on Windows. Only the lease holder writes the file. It removes whatever an earlier holder left
//! before it starts its relay (so a start that fails leaves no file naming a dead port), and the
//! relay removes the file when it stops, if the file still names it. Every window that finds the
//! lock taken follows: it connects, the two prove to each other that they know the secret, and
//! from then on the follower:
//! - takes each spot the holder's feeds parse (`P` lines), through the same intake it would use
//!   for a spot of its own (its own Unassisted switch still drops at the door);
//! - learns every couple of seconds what the holder is logged into (`S` lines), which is all its
//!   Now-Bar may claim: "through the other window", connected or not;
//! - sends a spot the operator posts in it up as a `POST DX <kHz> <call> <comment>` line. The
//!   holder accepts only that shape, rebuilds the line itself and puts it in its own outbox, where
//!   its node feeds send it once under the station's login.
//!
//! **Why a secret, when only loopback can reach the port.** Loopback is not this user: any other
//! user or program on the computer can connect to 127.0.0.1. Without the secret, one could post DX
//! spots under the operator's callsign through the holder's login, or, holding a port a dead
//! holder's file still names, feed a follower spots and a false "connected". So the secret is
//! never sent. The follower opens with `HELLO <nonce_f>`; the holder answers
//! `OK <proof> <nonce_h>`, its proof an HMAC-SHA256 under the secret over its label and both
//! nonces, which the follower checks (`ring::hmac::verify`) before it believes a line or sends one;
//! the follower then answers `AUTH <proof>`, the same over its own label. A listener that does not know
//! the secret learns nothing it could replay, and gets no post. The secret is 32 random bytes per
//! holder and is never logged.
//!
//! **What a stranger can cost the holder.** Nothing past the handshake: at most
//! [`MAX_HANDSHAKES`] connections are being authenticated at once (the rest are closed on accept),
//! each gets [`HELLO_DEADLINE`] in all to finish, and a handshake line is read one byte at a time
//! up to [`MAX_HANDSHAKE_LINE`]. After it, every line either side reads is capped at [`MAX_LINE`],
//! and a longer one ends the connection.
//!
//! **Handover.** The operating system drops the lock the moment the holder's process ends, and its
//! relay socket closes with it. A follower tries the lock again every [`RETRY`]; the first to take
//! it starts the feeds (the normal start path) and becomes the new holder. That is one new login
//! per node after at most one retry interval, with no storm. A follower that cannot reach a relay
//! also waits [`RETRY`] between attempts.
//!
//! **Lifetime.** A holder keeps the lease while its feeds run, as the RBN feeds keep their
//! once-latch. It lets go ([`Share::release`]) when its feeds have stopped and will not start again
//! (a callsign change with the cluster off or no real call left), and swaps it when the callsign
//! changes. Otherwise the lease goes when the process ends.

use std::fs::File;
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use ring::hmac;
use serde::{Deserialize, Serialize};
use tempo_net::cluster::ClusterSpot;

/// How long a follower waits between tries of the lease and the relay: the bound on a handover,
/// and the most often a follower knocks.
pub(crate) const RETRY: Duration = Duration::from_secs(5);
/// How often the holder tells each follower what it is logged into.
const STATUS_EVERY: Duration = Duration::from_secs(2);
/// A follower that hears no status for this long takes the relay for dead.
const STATUS_SILENCE: Duration = Duration::from_secs(7);
/// How long a connection has, in all, to finish the handshake.
const HELLO_DEADLINE: Duration = Duration::from_secs(3);
/// Connections being authenticated at once; more are closed on accept.
const MAX_HANDSHAKES: usize = 16;
/// The longest handshake line. The longest real one, `OK <64 hex> <64 hex>`, is 132 bytes.
const MAX_HANDSHAKE_LINE: usize = 160;
/// Spots queued for one follower before newer ones are dropped. A follower that cannot keep up
/// loses spots; it never slows the holder's cluster sockets.
const FOLLOWER_QUEUE: usize = 4096;
/// Followers one holder serves at once.
const MAX_FOLLOWERS: usize = 8;
/// The longest line either side accepts once authenticated; a longer one ends the connection.
const MAX_LINE: usize = 4096;
/// The longest spot post a follower may hand the holder, which is far more than a `DX` line needs.
const MAX_POST: usize = 256;

/// Which of the holder's feeds a spot came from. The follower takes it through the matching
/// intake, so an RBN spot is still an RBN spot there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Source {
    Rbn,
    Node,
}

/// What the holder is logged into, as it tells its followers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HolderStatus {
    /// An RBN feed is logged in.
    pub rbn_connected: bool,
    /// Human node feeds are running.
    pub nodes_running: bool,
    /// A human node is logged in, so a spot posted now has somewhere to go.
    pub nodes_connected: bool,
    /// The holder's Phone label: the nodes logged in, or the ones being tried.
    pub nodes_label: Option<String>,
}

/// A spot on the relay: the parsed fields. Nothing the intake stamps later (receipt time,
/// corroborators, the RBN flag) is on the wire.
#[derive(Debug, Serialize, Deserialize)]
struct WireSpot {
    src: Source,
    spotter: String,
    dx: String,
    khz: f64,
    comment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    time: Option<String>,
}

impl WireSpot {
    fn of(src: Source, sp: &ClusterSpot) -> Self {
        Self {
            src,
            spotter: sp.spotter.clone(),
            dx: sp.dx_call.clone(),
            khz: sp.freq_khz,
            comment: sp.comment.clone(),
            time: sp.time_utc.clone(),
        }
    }

    fn into_spot(self) -> (Source, ClusterSpot) {
        (
            self.src,
            ClusterSpot {
                spotter: self.spotter,
                dx_call: self.dx,
                freq_khz: self.khz,
                comment: self.comment,
                time_utc: self.time,
                received_unix: 0,
                corroborators: Vec::new(),
                rbn: false,
            },
        )
    }
}

/// What a window does for the share. The desktop's implementation is in `lib.rs`; the tests
/// build windows of their own.
pub(crate) trait Hooks: Send + Sync + 'static {
    /// This window wants the cluster now: its switch is on, it is not Unassisted, and it has a
    /// real call.
    fn active(&self) -> bool;
    /// As holder: what this window is logged into.
    fn status(&self) -> HolderStatus;
    /// As holder: a follower's spot post, already checked, to send through this window's nodes.
    fn post(&self, line: String);
    /// As follower: a spot the holder's feeds parsed.
    fn take(&self, source: Source, spot: &ClusterSpot);
    /// As follower: the next spot the operator posted in this window, to send up.
    fn next_post(&self) -> Option<String>;
    /// A follower took the lease: start this window's feeds, which find it the holder now.
    fn promote(&self);
}

/// What [`Share::claim`] decided for this window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Claim {
    /// This window holds the lease (or no lease could be made): run the feeds.
    Hold,
    /// Another window holds it: run none. A follower thread now takes its spots.
    Follow,
}

/// A follower's view of the relay, for its Now-Bar.
#[derive(Debug, Default)]
struct Following {
    /// The relay proved itself and its status is current.
    up: AtomicBool,
    /// The holder's last status.
    status: Mutex<HolderStatus>,
}

enum Role {
    /// This window logs in. The lock file stays open while the lease is held (`None`: no lock
    /// could be made, so this window logs in for itself), and the relay serves followers.
    Holder {
        _lease: Option<File>,
        relay: Option<Relay>,
    },
    Follower(Arc<Following>),
}

struct Current {
    /// The lease key (the bare call, upper-cased) this role is for.
    key: String,
    role: Role,
    /// Stops the role's threads: the follower loop, or the relay.
    stop: Arc<AtomicBool>,
}

/// One window's part in the share. Cheap to clone; the clones are the same window.
#[derive(Clone)]
pub(crate) struct Share {
    dir: PathBuf,
    hooks: Arc<dyn Hooks>,
    current: Arc<Mutex<Option<Current>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The lease key for a call: upper-cased, anything but letters and digits made `_`, so a
/// portable call (`KD9TAW/P`) names a file.
fn lease_key(call: &str) -> String {
    call.trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn lock_path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("cluster-{key}.lock"))
}

fn relay_path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("cluster-{key}.relay"))
}

/// What trying the lease came to.
enum Lease {
    Held(File),
    /// Another process holds it: the one answer that makes a window follow.
    Taken,
    /// No lock could be made or tried; the reason, for the log.
    Broken(String),
}

/// Try the lease.
fn try_lease(dir: &Path, key: &str) -> Lease {
    if let Err(e) = std::fs::create_dir_all(dir) {
        return Lease::Broken(e.to_string());
    }
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path(dir, key))
    {
        Ok(f) => f,
        Err(e) => return Lease::Broken(e.to_string()),
    };
    match file.try_lock() {
        Ok(()) => Lease::Held(file),
        Err(std::fs::TryLockError::WouldBlock) => Lease::Taken,
        Err(std::fs::TryLockError::Error(e)) => Lease::Broken(e.to_string()),
    }
}

/// 32 random bytes as hex, or `None` when the operating system gives none.
fn random_hex() -> Option<String> {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; 32];
    ring::rand::SystemRandom::new().fill(&mut bytes).ok()?;
    Some(hex(&bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// A 64-hex-digit nonce, as both sides send.
fn is_nonce(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Which side a proof is from: each side signs its own label, so one side's proof can never be
/// played back as the other's.
#[derive(Clone, Copy)]
enum Side {
    Holder,
    Follower,
}

/// The message a side signs: its label, then both nonces, the follower's first.
fn transcript(side: Side, follower_nonce: &str, holder_nonce: &str) -> String {
    let label = match side {
        Side::Holder => "holder",
        Side::Follower => "follower",
    };
    format!("nexus cluster relay {label} {follower_nonce} {holder_nonce}")
}

fn prove(secret: &str, side: Side, follower_nonce: &str, holder_nonce: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    hex(hmac::sign(
        &key,
        transcript(side, follower_nonce, holder_nonce).as_bytes(),
    )
    .as_ref())
}

/// Whether `proof` is that side's proof over the two nonces. In constant time (ring).
fn proven(secret: &str, side: Side, follower_nonce: &str, holder_nonce: &str, proof: &str) -> bool {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    unhex(proof).is_some_and(|tag| {
        hmac::verify(
            &key,
            transcript(side, follower_nonce, holder_nonce).as_bytes(),
            &tag,
        )
        .is_ok()
    })
}

/// Read one handshake line, a byte at a time so nothing past it is taken, by `deadline` and no
/// longer than [`MAX_HANDSHAKE_LINE`]. `None` for anything else.
fn handshake_line(stream: &mut TcpStream, deadline: Instant) -> Option<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let left = deadline.checked_duration_since(Instant::now())?;
        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
            return None;
        }
        match stream.read(&mut byte) {
            Ok(1) if byte[0] == b'\n' => return String::from_utf8(line).ok(),
            Ok(1) => {
                line.push(byte[0]);
                if line.len() > MAX_HANDSHAKE_LINE {
                    return None;
                }
            }
            Ok(_) => return None,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
}

/// What one bounded read of a line came to.
enum Line {
    /// A whole line, newline included.
    Got(Vec<u8>),
    /// Nothing more yet (the read timed out); what arrived is kept in the buffer.
    Wait,
    /// The connection ended, failed, or sent a line longer than [`MAX_LINE`].
    End,
}

/// Read the next line into `buf`, never holding more than [`MAX_LINE`] bytes of it.
fn next_line(reader: &mut BufReader<TcpStream>, buf: &mut Vec<u8>) -> Line {
    loop {
        let avail = match reader.fill_buf() {
            Ok(a) => a,
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Line::Wait
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Line::End,
        };
        if avail.is_empty() {
            return Line::End;
        }
        let (take, whole) = match avail.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (avail.len(), false),
        };
        buf.extend_from_slice(&avail[..take]);
        reader.consume(take);
        if buf.len() > MAX_LINE {
            return Line::End;
        }
        if whole {
            return Line::Got(std::mem::take(buf));
        }
    }
}

/// Write `text` to `path` owner-only, through a temporary file that a rename publishes, so a
/// follower never reads half of it.
fn write_owner_only(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("relay.tmp");
    #[cfg(unix)]
    let mut f = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?
    };
    #[cfg(not(unix))]
    let mut f = File::create(&tmp)?;
    #[cfg(unix)]
    {
        // A file left from before keeps its old mode through `open`; set it on every write.
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}

/// The relay's port and secret, as the holder wrote them.
fn read_relay(dir: &Path, key: &str) -> Option<(u16, String)> {
    let text = std::fs::read_to_string(relay_path(dir, key)).ok()?;
    let mut parts = text.split_whitespace();
    let port = parts.next()?.parse().ok()?;
    Some((port, parts.next()?.to_string()))
}

/// Sleep `d` in short steps; false once `stop` is set.
fn nap(d: Duration, stop: &AtomicBool) -> bool {
    let until = Instant::now() + d;
    while Instant::now() < until {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50).min(until - Instant::now()));
    }
    !stop.load(Ordering::Relaxed)
}

impl Share {
    /// This window's share, with its lease and relay files in `dir`.
    pub(crate) fn new(dir: PathBuf, hooks: Arc<dyn Hooks>) -> Self {
        Self {
            dir,
            hooks,
            current: Arc::new(Mutex::new(None)),
        }
    }

    /// Decide this window's part for `call`, and start it. Asked by every start of the cluster
    /// feeds. The same call keeps the part it has; a different call lets the old lease go first.
    pub(crate) fn claim(&self, call: &str) -> Claim {
        let key = lease_key(call);
        let mut cur = lock(&self.current);
        if let Some(c) = cur.as_ref() {
            if c.key == key {
                return match c.role {
                    Role::Holder { .. } => Claim::Hold,
                    Role::Follower(_) => Claim::Follow,
                };
            }
        }
        if let Some(old) = cur.take() {
            old.stop.store(true, Ordering::SeqCst);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let lease = match try_lease(&self.dir, &key) {
            Lease::Held(file) => Some(file),
            Lease::Taken => {
                let following = Arc::new(Following::default());
                *cur = Some(Current {
                    key: key.clone(),
                    role: Role::Follower(following.clone()),
                    stop: stop.clone(),
                });
                let me = self.clone();
                std::thread::spawn(move || me.follow(&key, &stop, &following));
                return Claim::Follow;
            }
            Lease::Broken(why) => {
                tempo_core::applog::warn(
                    "cluster",
                    &format!(
                        "no cluster lease could be made ({why}); this window logs in for itself"
                    ),
                );
                None
            }
        };
        let relay = lease.as_ref().and_then(|_| self.serve(&key, &stop));
        *cur = Some(Current {
            key,
            role: Role::Holder {
                _lease: lease,
                relay,
            },
            stop,
        });
        Claim::Hold
    }

    /// Start the relay for `key`. A relay that cannot start leaves this window holding the
    /// lease and running the feeds: other windows then wait, which is better than a second login.
    fn serve(&self, key: &str, stop: &Arc<AtomicBool>) -> Option<Relay> {
        match Relay::start(&self.dir, key, self.hooks.clone(), stop.clone()) {
            Ok(relay) => Some(relay),
            Err(e) => {
                tempo_core::applog::warn(
                    "cluster",
                    &format!("the cluster relay for other windows could not start: {e}"),
                );
                None
            }
        }
    }

    /// As holder: hand a spot this window's feed parsed to every follower. Nothing when this
    /// window follows, or no follower is connected.
    pub(crate) fn relay(&self, source: Source, spot: &ClusterSpot) {
        let cur = lock(&self.current);
        if let Some(Current {
            role: Role::Holder {
                relay: Some(relay), ..
            },
            ..
        }) = cur.as_ref()
        {
            relay.send(source, spot);
        }
    }

    /// As follower: whether the relay is up, and what the holder last said it is logged into.
    /// `None` when this window holds the lease or has no part yet.
    pub(crate) fn following(&self) -> Option<(bool, HolderStatus)> {
        match lock(&self.current).as_ref().map(|c| &c.role) {
            Some(Role::Follower(f)) => Some((f.up.load(Ordering::SeqCst), lock(&f.status).clone())),
            _ => None,
        }
    }

    /// Let go of this window's part: a holder's lease and relay, or a follower's thread. For a
    /// window whose feeds have stopped and will not start again, so a follower can take over;
    /// the process's end does the same. The next [`Self::claim`] decides afresh.
    pub(crate) fn release(&self) {
        let ended = lock(&self.current).take();
        if let Some(c) = ended {
            c.stop.store(true, Ordering::SeqCst);
        }
    }

    /// The follower loop: take the lease when it is free, otherwise follow the relay, and wait
    /// [`RETRY`] between tries either way.
    fn follow(&self, key: &str, stop: &Arc<AtomicBool>, following: &Following) {
        while !stop.load(Ordering::SeqCst) {
            following.up.store(false, Ordering::SeqCst);
            if self.hooks.active() {
                match try_lease(&self.dir, key) {
                    Lease::Held(file) => {
                        self.take_over(key, stop, Some(file));
                        return;
                    }
                    // As in `claim`: a lock that cannot be tried any more fails open.
                    Lease::Broken(why) => {
                        tempo_core::applog::warn(
                            "cluster",
                            &format!(
                                "the cluster lease can no longer be tried ({why}); this window \
                                 logs in for itself"
                            ),
                        );
                        self.take_over(key, stop, None);
                        return;
                    }
                    Lease::Taken => {
                        if let Some((port, secret)) = read_relay(&self.dir, key) {
                            self.session(port, &secret, stop, following);
                            following.up.store(false, Ordering::SeqCst);
                        }
                    }
                }
            }
            if !nap(RETRY, stop) {
                return;
            }
        }
    }

    /// The lease came free: become the holder, unless this part was replaced meanwhile, and
    /// start the feeds.
    fn take_over(&self, key: &str, stop: &Arc<AtomicBool>, lease: Option<File>) {
        {
            let mut cur = lock(&self.current);
            let still_mine = cur
                .as_ref()
                .is_some_and(|c| Arc::ptr_eq(&c.stop, stop) && !stop.load(Ordering::SeqCst));
            if !still_mine {
                return;
            }
            let relay = lease.as_ref().and_then(|_| self.serve(key, stop));
            *cur = Some(Current {
                key: key.to_string(),
                role: Role::Holder {
                    _lease: lease,
                    relay,
                },
                stop: stop.clone(),
            });
        }
        tempo_core::applog::info(
            "cluster",
            "the window that held the cluster logins let them go; this window holds them now",
        );
        self.hooks.promote();
    }

    /// One session with the relay, until it ends, this window stops wanting the cluster, or the
    /// part stops. Nothing is believed, and nothing is sent, before the relay proves the secret.
    fn session(&self, port: u16, secret: &str, stop: &AtomicBool, following: &Following) {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
            return;
        };
        let _ = stream.set_nodelay(true);
        if !self.authenticate(&mut stream, secret) {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return;
        }
        let Ok(reader) = stream.try_clone() else {
            return;
        };
        let _ = reader.set_read_timeout(Some(Duration::from_millis(200)));
        let mut reader = BufReader::new(reader);
        let mut line = Vec::new();
        let mut heard = Instant::now();
        loop {
            if stop.load(Ordering::SeqCst) || !self.hooks.active() {
                break;
            }
            if heard.elapsed() > STATUS_SILENCE {
                break; // no status: the relay is gone
            }
            match next_line(&mut reader, &mut line) {
                Line::Got(l) => {
                    if self.take_line(&l, following) {
                        heard = Instant::now();
                    }
                }
                Line::Wait => {}
                Line::End => break,
            }
            // Only while the relay's status is current: a post must not leave this window for a
            // relay that is not there.
            if following.up.load(Ordering::SeqCst) {
                while let Some(post) = self.hooks.next_post() {
                    let post = post.trim_end_matches(['\r', '\n']);
                    if stream
                        .write_all(format!("POST {post}\n").as_bytes())
                        .is_err()
                    {
                        let _ = stream.shutdown(std::net::Shutdown::Both);
                        return;
                    }
                }
            }
        }
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }

    /// The follower's half of the handshake: a nonce out, the holder's proof checked, ours back.
    fn authenticate(&self, stream: &mut TcpStream, secret: &str) -> bool {
        let deadline = Instant::now() + HELLO_DEADLINE;
        let Some(mine) = random_hex() else {
            return false;
        };
        if stream
            .write_all(format!("HELLO {mine}\n").as_bytes())
            .is_err()
        {
            return false;
        }
        let Some(answer) = handshake_line(stream, deadline) else {
            return false;
        };
        let mut words = answer.split(' ');
        let (Some("OK"), Some(proof), Some(theirs), None) =
            (words.next(), words.next(), words.next(), words.next())
        else {
            return false;
        };
        if !is_nonce(theirs) || !proven(secret, Side::Holder, &mine, theirs, proof) {
            return false;
        }
        let auth = prove(secret, Side::Follower, &mine, theirs);
        stream
            .write_all(format!("AUTH {auth}\n").as_bytes())
            .is_ok()
    }

    /// One line from the relay. True when it was a status: the relay is alive.
    fn take_line(&self, line: &[u8], following: &Following) -> bool {
        let Ok(text) = std::str::from_utf8(line) else {
            return false;
        };
        let text = text.trim_end();
        if let Some(json) = text.strip_prefix("S ") {
            if let Ok(status) = serde_json::from_str::<HolderStatus>(json) {
                *lock(&following.status) = status;
                following.up.store(true, Ordering::SeqCst);
                return true;
            }
        } else if let Some(json) = text.strip_prefix("P ") {
            if let Ok(wire) = serde_json::from_str::<WireSpot>(json) {
                let (source, spot) = wire.into_spot();
                self.hooks.take(source, &spot);
            }
        }
        false
    }
}

/// The followers a relay feeds, each with its own id so it can leave the list itself.
type Followers = Arc<Mutex<Vec<(u64, SyncSender<String>)>>>;

/// The holder's loopback listener and the followers it feeds.
struct Relay {
    outs: Followers,
}

impl Relay {
    /// Listen on loopback, write the port and a new secret beside the lock, and serve until
    /// `stop`; then remove the file, if it is still this relay's.
    fn start(
        dir: &Path,
        key: &str,
        hooks: Arc<dyn Hooks>,
        stop: Arc<AtomicBool>,
    ) -> std::io::Result<Self> {
        let path = relay_path(dir, key);
        // This window holds the lease, so whatever is there is an earlier holder's: gone, so no
        // follower dials a port that is no longer the relay's, even if this start fails.
        let _ = std::fs::remove_file(&path);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let secret = random_hex().ok_or_else(|| {
            std::io::Error::other("the operating system gave no random bytes for the secret")
        })?;
        let written = format!("{port} {secret}\n");
        write_owner_only(&path, &written)?;
        let outs: Followers = Arc::default();
        let accepting = outs.clone();
        let handshakes = Arc::new(AtomicUsize::new(0));
        let ids = Arc::new(AtomicU64::new(0));
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        // Bound to loopback, so nothing else can arrive; checked all the same.
                        if !peer.ip().is_loopback() || lock(&accepting).len() >= MAX_FOLLOWERS {
                            continue;
                        }
                        // A stranger may only ever hold a few threads, and briefly.
                        if handshakes.fetch_add(1, Ordering::SeqCst) >= MAX_HANDSHAKES {
                            handshakes.fetch_sub(1, Ordering::SeqCst);
                            continue;
                        }
                        let id = ids.fetch_add(1, Ordering::SeqCst);
                        let (outs, hooks, stop, secret, handshakes) = (
                            accepting.clone(),
                            hooks.clone(),
                            stop.clone(),
                            secret.clone(),
                            handshakes.clone(),
                        );
                        std::thread::spawn(move || {
                            serve_follower(stream, &secret, &*hooks, &outs, id, &stop, &handshakes)
                        });
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(100)),
                }
            }
            if std::fs::read_to_string(&path).is_ok_and(|t| t == written) {
                let _ = std::fs::remove_file(&path);
            }
        });
        Ok(Self { outs })
    }

    fn send(&self, source: Source, spot: &ClusterSpot) {
        let mut outs = lock(&self.outs);
        if outs.is_empty() {
            return;
        }
        let Ok(json) = serde_json::to_string(&WireSpot::of(source, spot)) else {
            return;
        };
        let line = format!("P {json}\n");
        outs.retain(|(_, tx)| {
            !matches!(
                tx.try_send(line.clone()),
                Err(TrySendError::Disconnected(_))
            )
        });
    }
}

/// A callsign's shape: letters, digits and `/`, with at least one of each of the first two.
fn callsign_shaped(call: &str) -> bool {
    (3..=16).contains(&call.len())
        && call.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'/')
        && call.bytes().any(|b| b.is_ascii_digit())
        && call.bytes().any(|b| b.is_ascii_alphabetic())
}

/// A follower's spot post, if it is a DX spot: `DX <kHz> <call> [comment]`, a real frequency and
/// a callsign-shaped DX call, a sane length, no control characters. Rebuilt here by the same
/// formatter the window's own spots use, so nothing but a spot can reach a node through it.
fn checked_post(line: &str) -> Option<String> {
    let post = line.strip_prefix("POST ")?.trim_end_matches(['\r', '\n']);
    if post.len() > MAX_POST || post.chars().any(char::is_control) {
        return None;
    }
    let mut words = post.split_whitespace();
    if words.next()? != "DX" {
        return None;
    }
    let khz: f64 = words.next()?.parse().ok()?;
    if !khz.is_finite() || khz <= 0.0 {
        return None;
    }
    let call = words.next()?;
    if !callsign_shaped(call) {
        return None;
    }
    let comment = words.collect::<Vec<_>>().join(" ");
    Some(tempo_net::cluster::format_dx_spot(khz, call, &comment))
}

/// Gives back a handshake slot once, however the handshake ends.
struct HandshakeSlot<'a>(Option<&'a AtomicUsize>);

impl HandshakeSlot<'_> {
    fn done(&mut self) {
        if let Some(n) = self.0.take() {
            n.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

impl Drop for HandshakeSlot<'_> {
    fn drop(&mut self) {
        self.done();
    }
}

/// Takes a follower off the list when its connection ends, so a closed one never holds a place.
struct Listed<'a> {
    outs: &'a Mutex<Vec<(u64, SyncSender<String>)>>,
    id: u64,
}

impl Drop for Listed<'_> {
    fn drop(&mut self) {
        lock(self.outs).retain(|(id, _)| *id != self.id);
    }
}

/// The holder's half of the handshake: the follower's nonce in, our proof out, theirs checked.
fn authenticate_follower(stream: &mut TcpStream, secret: &str) -> bool {
    let deadline = Instant::now() + HELLO_DEADLINE;
    let Some(hello) = handshake_line(stream, deadline) else {
        return false;
    };
    let Some(theirs) = hello.strip_prefix("HELLO ").filter(|n| is_nonce(n)) else {
        return false;
    };
    let Some(mine) = random_hex() else {
        return false;
    };
    let proof = prove(secret, Side::Holder, theirs, &mine);
    if stream
        .write_all(format!("OK {proof} {mine}\n").as_bytes())
        .is_err()
    {
        return false;
    }
    handshake_line(stream, deadline)
        .and_then(|auth| auth.strip_prefix("AUTH ").map(str::to_string))
        .is_some_and(|auth| proven(secret, Side::Follower, theirs, &mine, &auth))
}

/// One follower: the handshake, then status and spots down, posts up.
fn serve_follower(
    mut stream: TcpStream,
    secret: &str,
    hooks: &dyn Hooks,
    outs: &Mutex<Vec<(u64, SyncSender<String>)>>,
    id: u64,
    stop: &Arc<AtomicBool>,
    handshakes: &AtomicUsize,
) {
    let mut slot = HandshakeSlot(Some(handshakes));
    let _ = stream.set_nonblocking(false);
    if !authenticate_follower(&mut stream, secret) {
        let _ = stream.shutdown(std::net::Shutdown::Both);
        return;
    }
    slot.done();
    let (tx, rx) = sync_channel::<String>(FOLLOWER_QUEUE);
    lock(outs).push((id, tx));
    let _listed = Listed { outs, id };
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let _ = writer.set_write_timeout(Some(Duration::from_secs(5)));
    let _ = writer.set_nodelay(true);
    let done = Arc::new(AtomicBool::new(false));
    let writer_done = done.clone();
    let writer_stop = stop.clone();
    // The writer thread sends the status this thread keeps current: `hooks` stays on this one.
    let status = Arc::new(Mutex::new(hooks.status()));
    let writer_status = status.clone();
    let writing = std::thread::spawn(move || {
        let status_line = |s: &HolderStatus| {
            serde_json::to_string(s)
                .map(|j| format!("S {j}\n"))
                .unwrap_or_default()
        };
        let mut next_status = Instant::now();
        while !writer_done.load(Ordering::SeqCst) && !writer_stop.load(Ordering::SeqCst) {
            if Instant::now() >= next_status {
                let line = status_line(&lock(&writer_status));
                if writer.write_all(line.as_bytes()).is_err() {
                    break;
                }
                next_status = Instant::now() + STATUS_EVERY;
            }
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(line) => {
                    if writer.write_all(line.as_bytes()).is_err() {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        writer_done.store(true, Ordering::SeqCst);
        let _ = writer.shutdown(std::net::Shutdown::Both);
    });
    // Posts up; and the status kept current for the writer, which cannot ask the hooks itself.
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let Ok(read_half) = stream.try_clone() else {
        done.store(true, Ordering::SeqCst);
        let _ = writing.join();
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut line = Vec::new();
    let mut refreshed = Instant::now();
    while !done.load(Ordering::SeqCst) && !stop.load(Ordering::SeqCst) {
        if refreshed.elapsed() >= STATUS_EVERY / 2 {
            *lock(&status) = hooks.status();
            refreshed = Instant::now();
        }
        match next_line(&mut reader, &mut line) {
            Line::Got(l) => {
                if let Some(post) = std::str::from_utf8(&l).ok().and_then(checked_post) {
                    hooks.post(post);
                }
            }
            Line::Wait => {}
            Line::End => break,
        }
    }
    done.store(true, Ordering::SeqCst);
    let _ = stream.shutdown(std::net::Shutdown::Both);
    let _ = writing.join();
}

#[cfg(test)]
#[path = "cluster_share_tests.rs"]
mod tests;
