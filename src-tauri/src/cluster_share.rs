//! One cluster login per callsign on this computer, however many Nexus windows are open.
//!
//! Each window is its own process. Before this, each one logged into every DX-cluster node and
//! both RBN feeds under the same call, and a node that allows one session per call rejected or
//! bumped the other one, over and over, until it banned the call.
//!
//! **The lease.** The window that wants the cluster first takes an advisory lock on
//! `cluster-<CALL>.lock` in the machine-local base config dir. This is the same kind of std file
//! lock a radio window holds on its profile (`hold_profile_lock`). The holder runs every cluster
//! feed, human nodes and RBN alike, under ITS settings: its node list and its SSID. The lock is
//! keyed by the bare callsign, so windows set up for different calls stay independent. It lives in
//! the base config dir, never in the shared data dir: that one can be a network folder for a
//! multi-PC shack, and each computer there logs in for itself.
//!
//! **The relay.** The holder listens on `127.0.0.1:<ephemeral>` and writes the port and a fresh
//! secret to `cluster-<CALL>.relay` beside the lock, owner-only on unix (the per-user config dir
//! on Windows). Every window that finds the lock taken follows: it connects, proves the secret,
//! and from then on:
//! - takes each spot the holder's feeds parse (`P` lines), through the same intake it would use
//!   for a spot of its own (its own Unassisted switch still drops at the door);
//! - learns every couple of seconds what the holder is logged into (`S` lines), which is all its
//!   Now-Bar may claim: "through the other window", connected or not;
//! - sends a spot the operator posts in it up as a `POST DX …` line. The holder puts it in its own
//!   outbox, where its node feeds send it once under the station's login.
//!
//! **Why a secret, when only loopback can reach the port.** Loopback is not this user: any other
//! user or program on the computer can connect to 127.0.0.1. Without the secret, one could post DX
//! spots under the operator's callsign through the holder's login. The spots it would read are
//! public anyway, but the secret also gates those. The secret is 32 random bytes per holder,
//! compared in constant time, and never logged.
//!
//! **Handover.** The operating system drops the lock the moment the holder's process ends, and its
//! relay socket closes with it. A follower tries the lock again every [`RETRY`]; the first to take
//! it starts the feeds (the normal start path) and becomes the new holder. That is one new login
//! per node after at most one retry interval, with no storm. A follower that cannot reach a relay
//! also waits [`RETRY`] between attempts.
//!
//! **Lifetime.** A holder keeps the lease for as long as its process runs, as the RBN feeds keep
//! their once-latch. The lease changes hands only when the callsign changes (the old lease is let
//! go and the new call's is taken) or when the process ends.

use std::fs::File;
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tempo_net::cluster::ClusterSpot;

/// How long a follower waits between tries of the lease and the relay: the bound on a handover,
/// and the most often a follower knocks.
pub(crate) const RETRY: Duration = Duration::from_secs(5);
/// How often the holder tells each follower what it is logged into.
const STATUS_EVERY: Duration = Duration::from_secs(2);
/// A follower that hears no status for this long takes the relay for dead.
const STATUS_SILENCE: Duration = Duration::from_secs(7);
/// How long a new connection has to prove the secret.
const HELLO_DEADLINE: Duration = Duration::from_secs(3);
/// Spots queued for one follower before newer ones are dropped. A follower that cannot keep up
/// loses spots; it never slows the holder's cluster sockets.
const FOLLOWER_QUEUE: usize = 4096;
/// Followers one holder serves at once.
const MAX_FOLLOWERS: usize = 8;
/// The longest line either side accepts.
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
    /// This window holds the lease: run the feeds.
    Hold,
    /// Another window holds it: run none. A follower thread now takes its spots.
    Follow,
}

/// A follower's view of the relay, for its Now-Bar.
#[derive(Debug, Default)]
struct Following {
    /// The relay accepted this window and its status is current.
    up: AtomicBool,
    /// The holder's last status.
    status: Mutex<HolderStatus>,
}

enum Role {
    /// The lease is held: the lock file stays open, and the relay serves followers.
    Holder {
        _lease: File,
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

/// Take the lease, if no other window holds it.
fn try_lease(dir: &Path, key: &str) -> Option<File> {
    let _ = std::fs::create_dir_all(dir);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path(dir, key))
        .ok()?;
    file.try_lock().ok().map(|()| file)
}

/// 32 random bytes as hex, or `None` when the operating system gives none.
fn new_secret() -> Option<String> {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; 32];
    ring::rand::SystemRandom::new().fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Equal, in time that does not depend on where they first differ.
fn same_secret(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
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
    let (port, secret) = text.trim().split_once(' ')?;
    Some((port.parse().ok()?, secret.to_string()))
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
        if let Some(lease) = try_lease(&self.dir, &key) {
            let relay = self.serve(&key, &stop);
            *cur = Some(Current {
                key,
                role: Role::Holder {
                    _lease: lease,
                    relay,
                },
                stop,
            });
            return Claim::Hold;
        }
        let following = Arc::new(Following::default());
        *cur = Some(Current {
            key: key.clone(),
            role: Role::Follower(following.clone()),
            stop: stop.clone(),
        });
        let me = self.clone();
        std::thread::spawn(move || me.follow(&key, &stop, &following));
        Claim::Follow
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

    /// Let go of the lease and stop the relay or the follower: what the process's end does.
    #[cfg(test)]
    pub(crate) fn close(&self) {
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
                if let Some(lease) = try_lease(&self.dir, key) {
                    self.take_over(key, stop, lease);
                    return;
                }
                if let Some((port, secret)) = read_relay(&self.dir, key) {
                    self.session(port, &secret, stop, following);
                    following.up.store(false, Ordering::SeqCst);
                }
            }
            if !nap(RETRY, stop) {
                return;
            }
        }
    }

    /// The lease came free: become the holder, unless this part was replaced meanwhile, and
    /// start the feeds.
    fn take_over(&self, key: &str, stop: &Arc<AtomicBool>, lease: File) {
        {
            let mut cur = lock(&self.current);
            let still_mine = cur
                .as_ref()
                .is_some_and(|c| Arc::ptr_eq(&c.stop, stop) && !stop.load(Ordering::SeqCst));
            if !still_mine {
                return;
            }
            let relay = self.serve(key, stop);
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
            "the window that held the cluster logins closed; this window holds them now",
        );
        self.hooks.promote();
    }

    /// One session with the relay, until it ends, this window stops wanting the cluster, or the
    /// part stops.
    fn session(&self, port: u16, secret: &str, stop: &AtomicBool, following: &Following) {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
            return;
        };
        let _ = stream.set_nodelay(true);
        if stream
            .write_all(format!("HELLO {secret}\n").as_bytes())
            .is_err()
        {
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
                break; // no status: the relay is gone, or never accepted this window
            }
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(_) if line.ends_with(b"\n") => {
                    if self.take_line(&line, following) {
                        heard = Instant::now();
                    }
                    line.clear();
                }
                Ok(_) => {}
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(_) => break,
            }
            if line.len() > MAX_LINE {
                break;
            }
            // Only while the relay is up: a post must not leave this window for a relay that
            // has not accepted it.
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

    /// One line from the relay. True when it was a status: the relay is alive and accepted us.
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

/// The holder's loopback listener and the followers it feeds.
struct Relay {
    outs: Arc<Mutex<Vec<SyncSender<String>>>>,
}

impl Relay {
    /// Listen on loopback, write the port and a new secret beside the lock, and serve until
    /// `stop`.
    fn start(
        dir: &Path,
        key: &str,
        hooks: Arc<dyn Hooks>,
        stop: Arc<AtomicBool>,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let secret = new_secret().ok_or_else(|| {
            std::io::Error::other("the operating system gave no random bytes for the secret")
        })?;
        write_owner_only(&relay_path(dir, key), &format!("{port} {secret}\n"))?;
        let outs: Arc<Mutex<Vec<SyncSender<String>>>> = Arc::default();
        let accepting = outs.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        // Bound to loopback, so nothing else can arrive; checked all the same.
                        if !peer.ip().is_loopback() || lock(&accepting).len() >= MAX_FOLLOWERS {
                            continue;
                        }
                        let (outs, hooks, stop, secret) = (
                            accepting.clone(),
                            hooks.clone(),
                            stop.clone(),
                            secret.clone(),
                        );
                        std::thread::spawn(move || {
                            serve_follower(stream, &secret, &*hooks, &outs, &stop)
                        });
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(100)),
                }
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
        outs.retain(|tx| {
            !matches!(
                tx.try_send(line.clone()),
                Err(TrySendError::Disconnected(_))
            )
        });
    }
}

/// A follower's spot post, if it is one: a single `DX` line of sane length with no control
/// characters, so nothing but a spot can reach the node through it. Returned with the CR/LF a
/// node line ends in.
fn checked_post(line: &str) -> Option<String> {
    let post = line.strip_prefix("POST ")?.trim_end_matches(['\r', '\n']);
    let ok = post.len() <= MAX_POST
        && post.starts_with("DX ")
        && post.split_whitespace().count() >= 3
        && !post.chars().any(char::is_control);
    ok.then(|| format!("{post}\r\n"))
}

/// One follower: prove the secret, then status and spots down, posts up.
fn serve_follower(
    stream: TcpStream,
    secret: &str,
    hooks: &dyn Hooks,
    outs: &Mutex<Vec<SyncSender<String>>>,
    stop: &Arc<AtomicBool>,
) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(HELLO_DEADLINE));
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half.take(MAX_LINE as u64));
    let mut hello = String::new();
    let proved = reader.read_line(&mut hello).is_ok()
        && hello
            .trim_end()
            .strip_prefix("HELLO ")
            .is_some_and(|s| same_secret(s.as_bytes(), secret.as_bytes()));
    if !proved {
        let _ = stream.shutdown(std::net::Shutdown::Both);
        return;
    }
    let (tx, rx) = sync_channel::<String>(FOLLOWER_QUEUE);
    lock(outs).push(tx);
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
    // The same reader as the hello, so nothing it buffered is lost; the hello's size cap lifts.
    reader.get_mut().set_limit(u64::MAX);
    let _ = reader
        .get_ref()
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(200)));
    let mut line = Vec::new();
    let mut refreshed = Instant::now();
    while !done.load(Ordering::SeqCst) && !stop.load(Ordering::SeqCst) {
        if refreshed.elapsed() >= STATUS_EVERY / 2 {
            *lock(&status) = hooks.status();
            refreshed = Instant::now();
        }
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) if line.ends_with(b"\n") => {
                if let Some(post) = std::str::from_utf8(&line).ok().and_then(checked_post) {
                    hooks.post(post);
                }
                line.clear();
            }
            Ok(_) => {}
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
        if line.len() > MAX_LINE {
            break;
        }
    }
    done.store(true, Ordering::SeqCst);
    let _ = stream.shutdown(std::net::Shutdown::Both);
    let _ = writing.join();
}

#[cfg(test)]
#[path = "cluster_share_tests.rs"]
mod tests;
