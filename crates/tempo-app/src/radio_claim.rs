//! One radio, one Nexus window: the claim a window holds on the radio it drives, and the watch a
//! window's monitor holds on a radio it only reads.
//!
//! # Why
//!
//! Two Nexus processes on one radio share its rigctld: the second finds the first one's daemon
//! answering on the radio's port and connects through it (`open_cat`'s coexist branch, right for a
//! rigctld WSJT-X started). When the first lets go (its window closes, or its monitor recycles), the
//! daemon goes with it, and the second window's CAT, unkey included, talks to nothing until the
//! silence rebuild reopens the port minutes later. That is the field crash class. So:
//! - a window refuses a radio another live window drives ([`REFUSAL`]): at its launch, on the
//!   operator's switch, on the band→radio routing switch, on "Use one radio";
//! - a window's monitor never watches a radio another live window drives, and lets go of one as soon
//!   as a window claims it;
//! - a window about to open a radio waits (bounded) for every other window's monitor to let go of
//!   it, so it can never adopt a monitor's daemon in the moment between.
//!
//! # What is claimed: the physical endpoint
//!
//! The key is what actually conflicts, the radio's CAT endpoint ([`cat_endpoint`]): its serial
//! port, or host:port for network CAT, the Icom network connection and TCI, or the OmniRig slot.
//! Two profiles with their own rosters naming the same COM port conflict; two radios on different
//! ports never do, whatever their ids. A radio with no CAT and no keying port has no endpoint and
//! claims nothing (nothing of it can be shared).
//!
//! # The mechanism, and why this one
//!
//! An exclusive OS file lock per endpoint, `<dir>/radio-<hex endpoint>.lock`, held for as long as
//! the window drives that radio ([`Claims::claim`]); a switch moves it. A monitor holds a SHARED lock
//! on `<dir>/watch-<hex endpoint>.lock` while it holds a connection to that radio
//! ([`Claims::watch`]). Asking is a `try_lock` that reads "would block" as "a live window has it".
//!
//! - **A crashed window never locks a radio out.** A lock is held by an open handle, and the OS
//!   drops every lock of a process when it ends, however it ends. The file left behind means
//!   nothing; only a held lock refuses. (`flock` on Unix, `LockFileEx` on Windows, through std's
//!   `File::try_lock`.) A pid marker would need a liveness check and could be fooled by a reused pid.
//! - **The profile lock already works this way** (`hold_profile_lock` in the app), which the
//!   picker's "in use" trusts.
//! - **It fails open only where the filesystem cannot lock**: a lock refused for any reason but
//!   "would block" reads as free, as the profile lock does. Refusing every radio there would be
//!   worse than the hazard.
//!
//! The claims live in one folder for every profile (the base profile's), set once at launch by
//! [`init`]. Before that, and in a process that never calls it (tests, headless tools), [`claims`] is
//! `None` and nothing is claimed, watched or refused: behaviour is exactly as before.
//!
//! ⚠️ TESTS never call [`init`]: it would turn claims on for EVERY test in the binary, and another
//! test's switch would move "this window's" claim. A test that needs the engine to claim uses
//! `claims_for_this_thread`, which only its own thread sees. Endpoints in tests are still
//! pid-unique made-up paths (`/dev/nexus-…-<pid>`), so two test processes never meet.

use std::collections::HashMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::settings::RadioProfile;

/// What the refusal says, everywhere it is said.
pub const REFUSAL: &str = "That radio is open in another Nexus window — switch to that window.";

/// The radio's physical CAT endpoint, the thing two windows can fight over, or `None` for a radio
/// with nothing to fight over (no CAT and no keying port). Spelled so that two spellings of one
/// port compare equal: a Windows COM name in any case, with or without its `\\.\` prefix, is one
/// endpoint; a Unix device path is case-sensitive, as the filesystem is.
pub fn cat_endpoint(p: &RadioProfile) -> Option<String> {
    endpoint_of(&EndpointParts {
        rig_model: p.rig_model,
        ptt_method: &p.ptt_method,
        rig_conn: &p.rig_conn,
        serial_port: &p.serial_port,
        rig_addr: &p.rig_addr,
        icom_lan_host: &p.icom_lan_host,
        icom_lan_port: p.icom_lan_port,
        tci_host: &p.tci_host,
        tci_port: p.tci_port,
        omnirig_slot: p.omnirig_slot,
    })
}

/// The fields [`cat_endpoint`] reads, for a caller that holds them outside a [`RadioProfile`] (the
/// radio loop's transport).
pub struct EndpointParts<'a> {
    pub rig_model: u32,
    pub ptt_method: &'a str,
    pub rig_conn: &'a str,
    pub serial_port: &'a str,
    pub rig_addr: &'a str,
    pub icom_lan_host: &'a str,
    pub icom_lan_port: u16,
    pub tci_host: &'a str,
    pub tci_port: u16,
    pub omnirig_slot: u8,
}

/// [`cat_endpoint`] on its parts.
pub fn endpoint_of(p: &EndpointParts<'_>) -> Option<String> {
    use crate::settings::{
        rig_conn_is_icom_lan, rig_conn_is_network, rig_conn_is_omnirig, rig_conn_is_tci,
    };
    let keys_a_line = matches!(p.ptt_method, "rts" | "dtr");
    if p.rig_model == 0 && !keys_a_line {
        return None; // VOX, no CAT: nothing of this radio can be shared
    }
    let host = |h: &str, port: u16| format!("{}:{port}", h.trim().to_ascii_lowercase());
    if p.rig_model != 0 {
        if rig_conn_is_icom_lan(p.rig_conn) && !p.icom_lan_host.trim().is_empty() {
            return Some(format!(
                "icomlan:{}",
                host(p.icom_lan_host, p.icom_lan_port)
            ));
        }
        if rig_conn_is_tci(p.rig_conn) && !p.tci_host.trim().is_empty() {
            return Some(format!("tci:{}", host(p.tci_host, p.tci_port)));
        }
        if rig_conn_is_omnirig(p.rig_conn) {
            return Some(format!("omnirig:{}", p.omnirig_slot));
        }
        if rig_conn_is_network(p.rig_conn, p.rig_addr) {
            return Some(format!("net:{}", p.rig_addr.trim().to_ascii_lowercase()));
        }
    }
    serial_endpoint(p.serial_port)
}

/// A serial port's endpoint: `\\.\COM10` and `com10` are one port on Windows.
fn serial_endpoint(port: &str) -> Option<String> {
    let port = port.trim();
    let port = port.strip_prefix(r"\\.\").unwrap_or(port);
    if port.is_empty() {
        return None;
    }
    // The name is whatever settings.json holds, so byte 3 may fall inside a character (`СОМ4`): that
    // name is no COM port, and it is an ordinary endpoint below.
    let is_com = port.split_at_checked(3).is_some_and(|(com, number)| {
        com.eq_ignore_ascii_case("com")
            && !number.is_empty()
            && number.chars().all(|c| c.is_ascii_digit())
    });
    Some(if is_com {
        format!("serial:{}", port.to_ascii_uppercase())
    } else {
        format!("serial:{port}")
    })
}

/// The claims of this process: the folder, the endpoint it drives with the lock's handle, and the
/// endpoints it watches with theirs.
///
/// Lock order, everywhere: `watching` before `held`.
pub struct Claims {
    dir: PathBuf,
    held: Mutex<Option<(String, File)>>,
    watching: Mutex<HashMap<String, Watch>>,
}

/// A shared watch lock this process holds on an endpoint.
struct Watch {
    _file: File,
    /// `Some` for a radio this window LEFT (a switch, a save, a Remote selection): watched only to
    /// cover the daemon it still ran there, for [`LEFT_GRACE`] from then. `None` for the monitor's
    /// own watch, held while the monitor wants the radio.
    left_at: Option<Instant>,
}

/// A claim taken but not yet this window's ([`Claims::tentative`]): the new radio's lock is held,
/// and the old one is untouched. [`Tentative::commit`] makes the switch of claims; dropping it
/// (the switch it was for was refused) lets go of the new radio and moves nothing.
pub struct Tentative<'a> {
    claims: &'a Claims,
    /// The endpoint to drive, `None` for a radio with no endpoint (commit then lets go of the old).
    endpoint: Option<String>,
    /// The new radio's lock, `None` when this window already drove it or the file could not be
    /// made (fail-open).
    file: Option<File>,
}

impl Tentative<'_> {
    /// The endpoint this reservation is for (`None`: a radio with no endpoint).
    pub fn endpoint(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }

    /// The switch went through: the new radio is this window's, and the old one is let go of,
    /// still watched for a while to cover its daemon.
    pub fn commit(self) {
        let Tentative {
            claims,
            endpoint,
            file,
        } = self;
        claims.install(endpoint, file);
    }
}

impl Claims {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            held: Mutex::new(None),
            watching: Mutex::new(HashMap::new()),
        }
    }

    fn file(&self, kind: &str, endpoint: &str) -> PathBuf {
        let hex: String = endpoint.bytes().map(|b| format!("{b:02x}")).collect();
        self.dir.join(format!("{kind}-{hex}.lock"))
    }

    /// Is `endpoint` driven by ANOTHER live Nexus window? Never true for the one this process
    /// holds, never for a lock file left by a window that has ended.
    pub fn held_elsewhere(&self, endpoint: &str) -> bool {
        if self.holds(endpoint) {
            return false;
        }
        // Peeked SHARED: a claim (exclusive) still reads as held, and a peek can neither be
        // read as a claim by another peek nor refuse a claim beyond `claim`'s retry.
        locked_elsewhere(&self.file("radio", endpoint), Peek::Shared)
    }

    /// Does this process drive `endpoint`?
    pub fn holds(&self, endpoint: &str) -> bool {
        lock(&self.held)
            .as_ref()
            .is_some_and(|(h, _)| h == endpoint)
    }

    /// The new radio's lock, taken: `Ok(Some)`, or `Ok(None)` when the file cannot be made or this
    /// filesystem cannot lock (fail-open). "Would block" is asked again for up to `patience`
    /// before it is believed: another window's peek holds the lock shared for an instant, and must
    /// not read as a claim. Takes no lock of this process's.
    fn take(&self, endpoint: &str, patience: Duration) -> Result<Option<File>, String> {
        let _ = std::fs::create_dir_all(&self.dir);
        let Ok(f) = open_for_lock(&self.file("radio", endpoint)) else {
            return Ok(None);
        };
        let until = Instant::now() + patience;
        loop {
            match f.try_lock() {
                Ok(()) => return Ok(Some(f)),
                Err(TryLockError::WouldBlock) if Instant::now() < until => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(TryLockError::WouldBlock) => return Err(REFUSAL.to_string()),
                Err(_) => return Ok(None),
            }
        }
    }

    /// Make `endpoint` (with its lock `file`) this window's radio, and let go of the old one.
    ///
    /// The old radio is WATCHED (shared) before its lock drops, and stays watched for
    /// [`LEFT_GRACE`]: this window's loop or monitor still runs its daemon until the next tick or
    /// pass closes it, and another window that claims it in that moment must wait for that before
    /// it opens (its `wait_unwatched`). This window's own watch on the new radio goes: from now on
    /// it is this window's radio, and its open must wait only for OTHER windows' watches.
    fn install(&self, endpoint: Option<String>, file: Option<File>) {
        let mut watching = lock(&self.watching);
        let mut held = lock(&self.held);
        let old = held.as_ref().map(|(e, _)| e.clone());
        if old.is_some() && old == endpoint {
            return; // already this window's
        }
        if let Some(old) = &old {
            if let Ok(f) = open_for_lock(&self.file("watch", old)) {
                if f.try_lock_shared().is_ok() {
                    watching.insert(
                        old.clone(),
                        Watch {
                            _file: f,
                            left_at: Some(Instant::now()),
                        },
                    );
                }
            }
        }
        *held = match (endpoint.as_ref(), file) {
            (Some(e), Some(f)) => Some((e.clone(), f)),
            // No endpoint (VOX, Rig None), or a lock this filesystem cannot take: nothing held.
            _ => None,
        }; // the old handle drops here, its lock with it
        if let Some(e) = &endpoint {
            watching.remove(e);
        }
    }

    /// Take `endpoint` for this process, letting go of the one it held. `Err(REFUSAL)` when
    /// another live window holds it; this process then keeps what it had. Patient: asks "would
    /// block" again for up to [`CLAIM_RETRY`]. For callers that hold no lock; under the engine
    /// lock, use [`Claims::claim_at_once`].
    pub fn claim(&self, endpoint: &str) -> Result<(), String> {
        self.claim_with(Some(endpoint), CLAIM_RETRY)
    }

    /// [`Claims::claim`] without waiting: for a claim made under the engine lock, which must not
    /// sleep. A peek of another window's at that instant refuses it; the operator's own switch
    /// therefore claims patiently BEFORE it takes the engine lock, and the engine's claim then
    /// finds the radio already this window's.
    pub fn claim_at_once(&self, endpoint: &str) -> Result<(), String> {
        self.claim_with(Some(endpoint), Duration::ZERO)
    }

    /// Claim `endpoint`, or, for a radio with no endpoint (VOX, Rig None), let go of whatever
    /// this process held: it no longer drives that port. Patient, like [`Claims::claim`].
    pub fn claim_endpoint(&self, endpoint: Option<&str>) -> Result<(), String> {
        self.claim_with(endpoint, CLAIM_RETRY)
    }

    fn claim_with(&self, endpoint: Option<&str>, patience: Duration) -> Result<(), String> {
        self.tentative(endpoint, patience).map(Tentative::commit)
    }

    /// Reserve `endpoint` without moving this window's claim yet ([`Tentative`]): for a switch that
    /// can still be refused after the radio is reserved (a Remote selection's write boundary).
    pub fn tentative(
        &self,
        endpoint: Option<&str>,
        patience: Duration,
    ) -> Result<Tentative<'_>, String> {
        let file = match endpoint {
            Some(e) if !self.holds(e) => self.take(e, patience)?,
            _ => None,
        };
        // Already this window's: `file` is None and commit keeps the lock it has (`install`
        // returns before touching it). Never a cloned handle: on Windows closing one can unlock.
        Ok(Tentative {
            claims: self,
            endpoint: endpoint.map(str::to_string),
            file,
        })
    }

    /// The monitor may watch `endpoint`: true when it now holds (or already held) the shared watch,
    /// false when another live window drives the radio, or this one does. The drive is asked again
    /// after the watch is taken, so a window that claims in between is never missed: either its
    /// open sees the watch and waits for it, or this sees its claim and lets go.
    pub fn watch(&self, endpoint: &str) -> bool {
        // The `watching` lock first (the lock order), and only then whether this window drives
        // the radio: a claim lands either before (then this sees it) or after (then the claim
        // removes the watch this takes), never between.
        let mut watching = lock(&self.watching);
        if self.holds(endpoint) {
            return false; // this window drives it: never watched, it is not another window's
        }
        let taken_now = !watching.contains_key(endpoint);
        if taken_now {
            if self.held_elsewhere(endpoint) {
                return false;
            }
            let _ = std::fs::create_dir_all(&self.dir);
            let Ok(f) = open_for_lock(&self.file("watch", endpoint)) else {
                return true; // cannot make the file: fail-open
            };
            match f.try_lock_shared() {
                Ok(()) | Err(TryLockError::Error(_)) => {}
                // Only `wait_unwatched`'s peek takes it exclusively, for an instant: ask again on
                // the next pass rather than watch without the lock.
                Err(TryLockError::WouldBlock) => return false,
            }
            watching.insert(
                endpoint.to_string(),
                Watch {
                    _file: f,
                    left_at: None,
                },
            );
        }
        if self.held_elsewhere(endpoint) {
            // A watch taken in this very call has opened nothing yet, so it goes at once. One held
            // from an earlier pass covers a daemon this monitor still runs: it is KEPT, and let go
            // only by `unwatch_all_but` once `reconcile_pool` has closed that daemon. The claimant's
            // open waits on it until then.
            if taken_now {
                watching.remove(endpoint);
            }
            return false;
        }
        // Wanted by the monitor: a radio this window left is the monitor's to cover from now on.
        if let Some(w) = watching.get_mut(endpoint) {
            w.left_at = None;
        }
        true
    }

    /// Let go of every watch not in `keep`, except a radio this window left less than
    /// [`LEFT_GRACE`] ago. Called AFTER the monitor has closed its connections, so a window waiting
    /// in [`Claims::wait_unwatched`] goes on only once the daemon is gone.
    pub fn unwatch_all_but(&self, keep: &[String]) {
        self.unwatch_all_but_at(keep, Instant::now());
    }

    /// [`Claims::unwatch_all_but`] as of `now`.
    pub fn unwatch_all_but_at(&self, keep: &[String], now: Instant) {
        lock(&self.watching)
            .retain(|e, w| keep.contains(e) || w.left_at.is_some_and(|t| now < t + LEFT_GRACE));
    }

    /// Wait, up to `limit`, until no OTHER window's monitor watches `endpoint`. True when none
    /// does (at once, in the ordinary case: no other window, or none watching); false if the
    /// limit passed, and the open goes on as it always did.
    pub fn wait_unwatched(&self, endpoint: &str, limit: Duration) -> bool {
        // Every watch is peeked, this window's own included: a claim let go of this window's own
        // watch on the radio (`install`), so a watch still there is another window's.
        let path = self.file("watch", endpoint);
        let until = Instant::now() + limit;
        // Peeked EXCLUSIVE: watches are held shared, so only an exclusive try sees one.
        while locked_elsewhere(&path, Peek::Exclusive) {
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        true
    }
}

/// How long a radio this window LEFT stays watched ([`Claims::install`]): long enough for the
/// radio loop's next tick to tear down its daemon there, or hand it to the monitor (which then
/// watches it as its own), even a tick held up by a slow CAT read.
const LEFT_GRACE: Duration = Duration::from_secs(3);

/// How long a claim retries "would block" before it is believed (another window's peek).
const CLAIM_RETRY: Duration = Duration::from_millis(50);

/// How a peek asks a lock file.
#[derive(Clone, Copy)]
enum Peek {
    /// Radio files: their one holder holds them exclusive, and a shared peek sees that without
    /// ever looking like a holder to another peek.
    Shared,
    /// Watch files: their holders hold them shared, so only an exclusive try sees them.
    Exclusive,
}

/// Another handle holds `path`'s lock the way `peek` can see. A missing file, or one this
/// filesystem cannot lock, holds nothing.
fn locked_elsewhere(path: &Path, peek: Peek) -> bool {
    let Ok(f) = File::open(path) else {
        return false;
    };
    let tried = match peek {
        Peek::Shared => f.try_lock_shared(),
        Peek::Exclusive => f.try_lock(),
    };
    match tried {
        Ok(()) => {
            let _ = f.unlock(); // only a peek
            false
        }
        Err(TryLockError::WouldBlock) => true,
        Err(_) => false,
    }
}

fn open_for_lock(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

static CLAIMS: OnceLock<Claims> = OnceLock::new();

/// Set the claims folder, once, at launch: the base profile's, shared by every window on purpose.
/// A second call changes nothing.
pub fn init(dir: PathBuf) {
    let _ = CLAIMS.set(Claims::new(dir));
}

/// This process's claims, or `None` before [`init`] (and in a process that never calls it).
pub fn claims() -> Option<&'static Claims> {
    #[cfg(test)]
    if let Some(c) = THIS_THREAD.with(std::cell::Cell::get) {
        return Some(c);
    }
    CLAIMS.get()
}

#[cfg(test)]
thread_local! {
    static THIS_THREAD: std::cell::Cell<Option<&'static Claims>> = const { std::cell::Cell::new(None) };
}

/// TESTS: claims for the calling thread only, in `dir`, standing in for [`init`]'s. A test's engine
/// runs on its own thread, so its switches claim here and nowhere else: no other test running
/// beside it can move "this window's" claim, and nothing it claims can refuse another test.
#[cfg(test)]
pub fn claims_for_this_thread(dir: PathBuf) -> &'static Claims {
    let c: &'static Claims = Box::leak(Box::new(Claims::new(dir)));
    THIS_THREAD.with(|t| t.set(Some(c)));
    c
}

/// Is `p` driven by another live window? False with no claims, and for a radio with no endpoint.
pub fn profile_held_elsewhere(p: &RadioProfile) -> bool {
    match (claims(), cat_endpoint(p)) {
        (Some(c), Some(e)) => c.held_elsewhere(&e),
        _ => false,
    }
}

/// Claim `p` for this process, at once: for a claim made under the engine lock (the engine's own
/// switch), which must not sleep. `Ok` with no claims; a radio with no endpoint lets go of the old.
pub fn claim_profile(p: &RadioProfile) -> Result<(), String> {
    match claims() {
        Some(c) => c.claim_with(cat_endpoint(p).as_deref(), Duration::ZERO),
        None => Ok(()),
    }
}

/// [`claim_profile`], patiently ([`CLAIM_RETRY`]): for a caller that holds no lock.
pub fn claim_profile_patiently(p: &RadioProfile) -> Result<(), String> {
    match claims() {
        Some(c) => c.claim_endpoint(cat_endpoint(p).as_deref()),
        None => Ok(()),
    }
}

/// Reserve `p` for this process without moving the claim yet ([`Claims::tentative`]). `None` with
/// no claims set up. At once when `patient` is false (under the engine lock).
pub fn tentative_profile(
    p: &RadioProfile,
    patient: bool,
) -> Result<Option<Tentative<'static>>, String> {
    let patience = if patient { CLAIM_RETRY } else { Duration::ZERO };
    match claims() {
        Some(c) => c.tentative(cat_endpoint(p).as_deref(), patience).map(Some),
        None => Ok(None),
    }
}

/// Commit `reserved` (made for radio `id` before the engine lock was taken) only if the engine is
/// about to switch to that radio, as `s` stands NOW, under the lock: `id` is not already active,
/// still exists, is enabled, and its endpoint is still the one reserved. Otherwise the reservation
/// drops (the engine will not switch, or will switch to a different port, whose own at-once claim
/// then decides), and the claim stays on the radio this window still drives. True when committed.
pub fn commit_if_switching(
    reserved: Tentative<'_>,
    s: &crate::settings::Settings,
    id: u32,
) -> bool {
    let still = s.active_radio != id
        && s.radios
            .iter()
            .any(|r| r.id == id && r.enabled && cat_endpoint(r).as_deref() == reserved.endpoint());
    if still {
        reserved.commit();
    }
    still
}

/// A settings save that would put this window's active radio on an endpoint another live window
/// drives (the operator edited its COM port, or its network address, to the other window's
/// radio) is refused whole, with [`REFUSAL`], and the window keeps the radio it has. `live` is
/// the settings now, `after` the settings the save would leave. `None` when the endpoint does not
/// change, when nothing else holds the new one, and with no claims.
pub fn save_refusal(
    live: &crate::settings::Settings,
    after: &crate::settings::Settings,
    claims: Option<&Claims>,
) -> Option<String> {
    let active = |s: &crate::settings::Settings| {
        s.radios
            .iter()
            .find(|r| r.id == s.active_radio)
            .and_then(cat_endpoint)
    };
    let now = active(after)?;
    if active(live).as_ref() == Some(&now) {
        return None; // the save leaves the radio where it is
    }
    claims?.held_elsewhere(&now).then(|| REFUSAL.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;

    /// `endpoint` reads free within 2 s. A lock let go of is free at once, except that a process
    /// forked at that moment (another test starting a child) holds a copy of every open handle
    /// until its `exec` closes them, a few milliseconds. Nexus has the same window when it starts a
    /// daemon; nothing waits on it.
    fn soon_free(c: &Claims, endpoint: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while c.held_elsewhere(endpoint) {
            if Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-claims-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn radio(conn: &str, port: &str, addr: &str) -> RadioProfile {
        RadioProfile {
            rig_model: 1029,
            ptt_method: "cat".into(),
            rig_conn: conn.into(),
            serial_port: port.into(),
            rig_addr: addr.into(),
            ..RadioProfile::default()
        }
    }

    /// A port name is whatever settings.json holds, and a restored backup or a hand edit can hold
    /// any text. The claim runs at startup, so a name that panicked here stopped Nexus before its
    /// window, at every launch. `СОМ4` is Cyrillic: it reads like COM4 and is not that port.
    #[test]
    fn a_port_name_in_any_letters_is_an_endpoint_and_never_a_panic() {
        let com4 = serial_endpoint("COM4");
        for port in [r"СОМ4", r"ком4", r"\\.\СОМ4", r"\\.\ком4"] {
            let endpoint = serial_endpoint(port);
            assert!(endpoint.is_some(), "{port} names a port");
            assert_ne!(endpoint, com4, "{port} is not COM4");
        }
        // Each width of character, at each of the first positions, so that byte 3 falls inside it
        // wherever it can.
        for c in ['é', 'м', '€', '📻'] {
            for at in 0..=5 {
                let mut port = String::from("COM45");
                port.insert(at, c);
                let endpoint = serial_endpoint(&port);
                assert_eq!(endpoint, Some(format!("serial:{port}")), "kept as written");
                assert_ne!(endpoint, com4, "{port} is not COM4");
            }
        }
        // A name like that still takes and gives back a claim: the lock file is named in hex.
        let dir = temp_dir("non-ascii-port");
        let claims = Claims::new(dir.clone());
        let endpoint = serial_endpoint("СОМ4").unwrap();
        claims.claim(&endpoint).unwrap();
        assert!(claims.holds(&endpoint));
    }

    /// The Windows spellings of an ASCII port stay one endpoint, and every other ASCII name stays
    /// as it is written.
    #[test]
    fn an_ascii_port_name_keeps_its_endpoint() {
        for (port, want) in [
            ("com10", "serial:COM10"),
            (r"\\.\COM10", "serial:COM10"),
            ("COM4", "serial:COM4"),
            ("  com4 ", "serial:COM4"),
            ("/dev/ttyUSB0", "serial:/dev/ttyUSB0"),
            ("com", "serial:com"),
            ("COMX", "serial:COMX"),
            ("com4x", "serial:com4x"),
        ] {
            assert_eq!(serial_endpoint(port).as_deref(), Some(want), "{port}");
        }
        assert_eq!(serial_endpoint("   "), None);
        assert_eq!(serial_endpoint(r"\\.\"), None);
    }

    /// What conflicts is the port, not the roster: two profiles naming one COM port (in any
    /// spelling) are one endpoint; different ports never are, whatever the ids.
    #[test]
    fn the_endpoint_is_the_port_the_radio_is_on() {
        let a = RadioProfile {
            id: 0,
            ..radio("serial", "COM4", "")
        };
        let b = RadioProfile {
            id: 7,
            ..radio("serial", r"\\.\com4", "")
        };
        assert_eq!(cat_endpoint(&a), cat_endpoint(&b));
        assert_eq!(cat_endpoint(&a).as_deref(), Some("serial:COM4"));
        let c = RadioProfile {
            id: 0,
            ..radio("serial", "COM8", "")
        };
        assert_ne!(cat_endpoint(&a), cat_endpoint(&c), "same id, other port");
        // Unix device paths are case-sensitive, as the filesystem is.
        assert_ne!(
            cat_endpoint(&radio("serial", "/dev/ttyUSB0", "")),
            cat_endpoint(&radio("serial", "/dev/TTYUSB0", ""))
        );
        // Network CAT: host:port, the serial field ignored.
        assert_eq!(
            cat_endpoint(&radio("network", "COM4", "Shack-PC:4532")).as_deref(),
            Some("net:shack-pc:4532")
        );
        // No CAT and no keying port: nothing to share.
        let vox = RadioProfile {
            rig_model: 0,
            ptt_method: "vox".into(),
            serial_port: "COM4".into(),
            ..RadioProfile::default()
        };
        assert_eq!(cat_endpoint(&vox), None);
        // …but a serial keying line is the port.
        let rts = RadioProfile {
            ptt_method: "rts".into(),
            ..vox.clone()
        };
        assert_eq!(cat_endpoint(&rts).as_deref(), Some("serial:COM4"));
        assert_eq!(cat_endpoint(&radio("serial", "", "")), None);
    }

    /// Two windows, here two holders with their own handles, which the OS treats exactly as two
    /// processes: the second is refused the radio the first drives, and gets it once the first
    /// lets go.
    #[test]
    fn a_radio_one_window_drives_is_refused_to_another() {
        let dir = temp_dir("two");
        let a = Claims::new(dir.clone());
        let b = Claims::new(dir.clone());
        assert_eq!(a.claim("serial:COM4"), Ok(()));
        assert!(b.held_elsewhere("serial:COM4"));
        assert_eq!(b.claim("serial:COM4"), Err(REFUSAL.to_string()));
        // CONTROLS: another port is free, and a window never refuses itself.
        assert!(!b.held_elsewhere("serial:COM8"));
        assert_eq!(b.claim("serial:COM8"), Ok(()));
        assert!(!a.held_elsewhere("serial:COM4"));
        assert_eq!(a.claim("serial:COM4"), Ok(()));
        // A switches to another radio: COM4 is free again, for B.
        assert_eq!(a.claim("serial:COM9"), Ok(()));
        assert!(soon_free(&b, "serial:COM4"));
        assert_eq!(b.claim("serial:COM4"), Ok(()));
        assert!(
            !b.holds("serial:COM8"),
            "B let go of COM8 when it took COM4"
        );
        assert!(soon_free(&a, "serial:COM8"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A refused claim leaves the window on the radio it had.
    #[test]
    fn a_refused_claim_keeps_the_radio_the_window_had() {
        let dir = temp_dir("keep");
        let a = Claims::new(dir.clone());
        let b = Claims::new(dir.clone());
        a.claim("serial:COM4").unwrap();
        b.claim("serial:COM8").unwrap();
        assert!(b.claim("serial:COM4").is_err());
        assert!(b.holds("serial:COM8"));
        assert!(a.held_elsewhere("serial:COM8"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A monitor never watches a radio another window drives, and a window about to open a radio
    /// another window's monitor watches waits until that monitor lets go.
    #[test]
    fn a_monitor_yields_a_radio_to_the_window_that_drives_it() {
        let dir = temp_dir("watch");
        let driver = Claims::new(dir.clone());
        let monitor = Claims::new(dir.clone());
        // Nobody drives COM8 yet: the monitor may watch it.
        assert!(monitor.watch("serial:COM8"));
        assert!(monitor.watch("serial:COM8"), "idempotent");
        // A window claims COM8. Its open waits for the monitor's watch, still held.
        driver.claim("serial:COM8").unwrap();
        assert!(!driver.wait_unwatched("serial:COM8", Duration::from_millis(150)));
        // The monitor's next pass sees the claim and refuses it; it closes its connection, then
        // lets go of the watch.
        assert!(!monitor.watch("serial:COM8"));
        monitor.unwatch_all_but(&[]);
        assert!(driver.wait_unwatched("serial:COM8", Duration::from_secs(2)));
        // A radio another window drives is never watched in the first place.
        assert!(!monitor.watch("serial:COM8"));
        // CONTROLS: a radio nobody drives stays watched, and blocks another window's open;
        // a window that claims a radio its own monitor watched lets go of that watch, so its own
        // open does not wait on itself.
        assert!(monitor.watch("serial:COM9"));
        monitor.unwatch_all_but(&["serial:COM9".to_string()]);
        assert!(
            !driver.wait_unwatched("serial:COM9", Duration::ZERO),
            "another's watch"
        );
        monitor.claim("serial:COM9").unwrap();
        assert!(
            monitor.wait_unwatched("serial:COM9", Duration::ZERO),
            "own watch, let go of by the claim"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Settings whose active radio (id 0) is on `port`, with a second radio on COM9.
    fn station(port: &str) -> crate::settings::Settings {
        crate::settings::Settings {
            radios: vec![
                RadioProfile {
                    id: 0,
                    ..radio("serial", port, "")
                },
                RadioProfile {
                    id: 1,
                    ..radio("serial", "COM9", "")
                },
            ],
            active_radio: 0,
            ..crate::settings::Settings::default()
        }
    }

    /// The operator edits the active radio's port, in Settings, to the port another window's
    /// radio is on: the save is refused, in the operator's words. Saving it onto a free port, or
    /// saving without touching the port, is not.
    #[test]
    fn a_save_that_moves_the_radio_onto_another_windows_port_is_refused() {
        let dir = temp_dir("save");
        let here = Claims::new(dir.clone());
        let other_window = Claims::new(dir.clone());
        other_window.claim("serial:COM8").unwrap();
        here.claim("serial:COM4").unwrap();
        let live = station("COM4");

        assert_eq!(
            save_refusal(&live, &station("COM8"), Some(&here)).as_deref(),
            Some(REFUSAL),
            "COM8 is the other window's"
        );
        // A different spelling of that port is that port.
        assert_eq!(
            save_refusal(&live, &station(r"\\.\com8"), Some(&here)).as_deref(),
            Some(REFUSAL)
        );
        // CONTROLS: a free port, an unchanged port, and no claims set up.
        assert_eq!(save_refusal(&live, &station("COM6"), Some(&here)), None);
        assert_eq!(save_refusal(&live, &station("COM4"), Some(&here)), None);
        assert_eq!(save_refusal(&live, &station("COM8"), None), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// M1: a monitor that sees another window's claim on a radio it already watches keeps the
    /// watch until it has closed its daemon (`unwatch_all_but`, after `reconcile_pool`). Letting
    /// go at the sight of the claim told the claimant "no monitor" while the daemon still ran.
    #[test]
    fn a_monitor_keeps_its_watch_until_it_has_closed_the_daemon() {
        let dir = temp_dir("keepwatch");
        let driver = Claims::new(dir.clone());
        let monitor = Claims::new(dir.clone());
        assert!(monitor.watch("serial:COM8"));
        driver.claim("serial:COM8").unwrap();
        assert!(!monitor.watch("serial:COM8"), "the monitor sees the claim");
        assert!(
            !driver.wait_unwatched("serial:COM8", Duration::ZERO),
            "and still holds the watch: its daemon is not closed yet"
        );
        monitor.unwatch_all_but(&[]);
        assert!(driver.wait_unwatched("serial:COM8", Duration::from_secs(2)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// S1: another window ASKING about a radio (a peek) never refuses a claim on it. Peeks take
    /// the lock shared, which still reads a real claim as held, and a claim retries a moment.
    #[test]
    fn a_peek_never_refuses_a_claim() {
        let dir = temp_dir("peek");
        let me = Claims::new(dir.clone());
        let asker = Claims::new(dir.clone());
        // Make the file exist, then let it go.
        me.claim("serial:COM4").unwrap();
        me.claim_endpoint(Some("serial:COM6")).unwrap();
        assert!(!asker.held_elsewhere("serial:COM4"), "premise: free");
        // A peek in flight: the asker's handle on the file, held as a peek holds it, briefly.
        let peek = File::open(asker.file("radio", "serial:COM4")).unwrap();
        peek.try_lock_shared().expect("a peek's lock");
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            drop(peek);
        });
        assert!(!me.held_elsewhere("serial:COM4"), "a peek is not a claim");
        assert_eq!(
            me.claim("serial:COM4"),
            Ok(()),
            "a peek never refuses a claim"
        );
        t.join().unwrap();
        // CONTROL: a real claim still reads as held, through a shared peek.
        assert!(asker.held_elsewhere("serial:COM4"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// S2: a window that claims a radio its own monitor watched lets go of its own watch, and its
    /// open still waits for ANOTHER window's watch on that radio.
    #[test]
    fn its_own_watch_never_hides_another_windows_watch() {
        let dir = temp_dir("ownwatch");
        let me = Claims::new(dir.clone());
        let other = Claims::new(dir.clone());
        assert!(me.watch("serial:COM8"));
        assert!(other.watch("serial:COM8"));
        me.claim("serial:COM8").unwrap();
        assert!(
            !me.wait_unwatched("serial:COM8", Duration::ZERO),
            "the other window's monitor still watches it"
        );
        other.unwatch_all_but(&[]);
        assert!(me.wait_unwatched("serial:COM8", Duration::from_secs(2)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// S3: switching (or saving) to a radio with no endpoint, VOX or Rig None, lets go of the old
    /// port: this window no longer drives it.
    #[test]
    fn a_radio_with_no_endpoint_lets_go_of_the_old_port() {
        let dir = temp_dir("noendpoint");
        let me = Claims::new(dir.clone());
        let other = Claims::new(dir.clone());
        me.claim("serial:COM4").unwrap();
        me.claim_endpoint(None).unwrap();
        assert!(!me.holds("serial:COM4"));
        assert!(soon_free(&other, "serial:COM4"));
        assert_eq!(other.claim("serial:COM4"), Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The screen never shows the refusal's English: it maps this exact sentence to its catalog
    /// entry (`ui/src/radioRefusal.ts`). The two copies must stay one sentence.
    #[test]
    fn the_screen_knows_the_refusal_word_for_word() {
        let ui = include_str!("../../../ui/src/radioRefusal.ts");
        assert!(
            ui.contains(&format!("'{REFUSAL}'")),
            "ui/src/radioRefusal.ts must carry REFUSAL exactly"
        );
    }

    /// M1 (review 3): a claim taken tentatively and never committed (the switch it was for was
    /// refused, e.g. at the write boundary) leaves the window's claim exactly where it was, and the
    /// new radio free. Committed, it moves.
    #[test]
    fn a_tentative_claim_moves_nothing_until_it_is_committed() {
        let dir = temp_dir("tentative");
        let me = Claims::new(dir.clone());
        let other = Claims::new(dir.clone());
        me.claim("serial:COM4").unwrap();
        let t = me.tentative(Some("serial:COM8"), Duration::ZERO).unwrap();
        assert!(me.holds("serial:COM4"), "still on the radio it drives");
        assert!(
            other.held_elsewhere("serial:COM8"),
            "the new radio is reserved meanwhile"
        );
        drop(t); // the switch was refused
        assert!(me.holds("serial:COM4"));
        assert!(
            soon_free(&other, "serial:COM8"),
            "the new radio is free again"
        );
        assert!(other.held_elsewhere("serial:COM4"));
        // Committed: the claim moves.
        let t = me.tentative(Some("serial:COM8"), Duration::ZERO).unwrap();
        t.commit();
        assert!(me.holds("serial:COM8"));
        assert!(!me.holds("serial:COM4"));
        // Refused while another window holds it: nothing moves.
        other.claim("serial:COM9").unwrap();
        assert!(me.tentative(Some("serial:COM9"), Duration::ZERO).is_err());
        assert!(me.holds("serial:COM8"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// S1 (review 3): a window leaving a radio keeps a watch on it for a while, so another window
    /// that claims it at once waits for this window's daemon on it to be closed before it opens.
    #[test]
    fn a_radio_left_stays_watched_until_its_daemon_can_be_closed() {
        let dir = temp_dir("left");
        let a = Claims::new(dir.clone());
        let b = Claims::new(dir.clone());
        a.claim("serial:COM8").unwrap();
        a.claim("serial:COM9").unwrap(); // a switch: A's daemon on COM8 still runs for a moment
        assert_eq!(b.claim("serial:COM8"), Ok(()), "COM8 is free to claim");
        assert!(
            !b.wait_unwatched("serial:COM8", Duration::ZERO),
            "but B's open waits: A has not closed its COM8 daemon yet"
        );
        // A monitor pass right away keeps it (the grace covers the loop's teardown)…
        a.unwatch_all_but(&[]);
        assert!(!b.wait_unwatched("serial:COM8", Duration::ZERO));
        // …and a pass after the grace lets it go.
        a.unwatch_all_but_at(&[], Instant::now() + LEFT_GRACE + Duration::from_millis(1));
        assert!(b.wait_unwatched("serial:COM8", Duration::from_secs(2)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// N5 (review 3): a claim made under the engine lock never sleeps: it is taken at once or
    /// refused. (The operator's own switch claims patiently BEFORE it takes the engine lock.)
    #[test]
    fn a_claim_at_once_never_waits() {
        let dir = temp_dir("atonce");
        let me = Claims::new(dir.clone());
        let other = Claims::new(dir.clone());
        other.claim("serial:COM4").unwrap();
        let started = Instant::now();
        assert!(me.claim_at_once("serial:COM4").is_err());
        assert!(started.elapsed() < CLAIM_RETRY, "refused at once");
        assert_eq!(me.claim_at_once("serial:COM6"), Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    const CHILD: &str = "NEXUS_CLAIM_CHILD_DIR";

    /// The child half of the crash test: claim COM5 in the folder it is given, say so, and wait to
    /// be killed.
    #[test]
    fn claim_child_holds_com5_until_killed() {
        let Ok(dir) = std::env::var(CHILD) else {
            return; // only as the child
        };
        let c = Claims::new(PathBuf::from(dir));
        c.claim("serial:COM5").expect("the child claims COM5");
        println!("CLAIM-CHILD-HELD");
        std::thread::sleep(Duration::from_secs(120));
    }

    /// A REAL other process holds the radio, then dies without letting go (SIGKILL /
    /// TerminateProcess: no destructor, no cleanup). While it lives, its radio is refused; once it
    /// is dead, the lock file it left behind refuses nothing.
    #[test]
    fn a_window_that_crashed_never_locks_its_radio_out() {
        let dir = temp_dir("crash");
        let mut child = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "radio_claim::tests::claim_child_holds_com5_until_killed",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, &dir)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("start the child window");
        let out = child.stdout.take().expect("its stdout");
        let mut held = false;
        for line in std::io::BufReader::new(out).lines() {
            // libtest prints the test's name on the same line as its first output.
            if line.expect("a line").contains("CLAIM-CHILD-HELD") {
                held = true;
                break;
            }
        }
        assert!(held, "the child claimed the radio");
        let me = Claims::new(dir.clone());
        assert!(
            me.held_elsewhere("serial:COM5"),
            "a live window's radio is refused"
        );
        assert_eq!(me.claim("serial:COM5"), Err(REFUSAL.to_string()));

        child.kill().expect("kill the child, no cleanup");
        let _ = child.wait();
        assert!(
            std::fs::read_dir(&dir).unwrap().count() > 0,
            "premise: its lock file is left behind"
        );
        assert!(soon_free(&me, "serial:COM5"), "a dead window holds nothing");
        assert_eq!(me.claim("serial:COM5"), Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
