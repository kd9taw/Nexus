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
    let is_com = port.len() > 3
        && port[..3].eq_ignore_ascii_case("com")
        && port[3..].chars().all(|c| c.is_ascii_digit());
    Some(if is_com {
        format!("serial:{}", port.to_ascii_uppercase())
    } else {
        format!("serial:{port}")
    })
}

/// The claims of this process: the folder, the endpoint it drives with the lock's handle, and the
/// endpoints its monitor watches with theirs.
pub struct Claims {
    dir: PathBuf,
    held: Mutex<Option<(String, File)>>,
    watching: Mutex<HashMap<String, File>>,
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
        locked_elsewhere(&self.file("radio", endpoint))
    }

    /// Does this process drive `endpoint`?
    pub fn holds(&self, endpoint: &str) -> bool {
        lock(&self.held)
            .as_ref()
            .is_some_and(|(h, _)| h == endpoint)
    }

    /// Take `endpoint` for this process, letting go of the one it held. `Err(REFUSAL)` when
    /// another live window holds it; this process then keeps what it had.
    pub fn claim(&self, endpoint: &str) -> Result<(), String> {
        let mut held = lock(&self.held);
        if held.as_ref().is_some_and(|(h, _)| h == endpoint) {
            return Ok(());
        }
        let _ = std::fs::create_dir_all(&self.dir);
        let Ok(f) = open_for_lock(&self.file("radio", endpoint)) else {
            return Ok(()); // cannot make the file: fail-open
        };
        match f.try_lock() {
            Ok(()) => {
                *held = Some((endpoint.to_string(), f)); // the old handle drops, its lock with it
                Ok(())
            }
            Err(TryLockError::WouldBlock) => Err(REFUSAL.to_string()),
            Err(_) => Ok(()),
        }
    }

    /// The monitor may watch `endpoint`: true when it now holds (or already held) the shared watch,
    /// false when another live window drives the radio. The drive is asked again after the watch
    /// is taken, so a window that claims in between is never missed: either its open sees the
    /// watch and waits for it, or this sees its claim and lets go.
    pub fn watch(&self, endpoint: &str) -> bool {
        let mut watching = lock(&self.watching);
        if !watching.contains_key(endpoint) {
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
            watching.insert(endpoint.to_string(), f);
        }
        if self.held_elsewhere(endpoint) {
            watching.remove(endpoint);
            return false;
        }
        true
    }

    /// Let go of every watch not in `keep`. Called AFTER the monitor has closed its connections, so
    /// a window waiting in [`Claims::wait_unwatched`] goes on only once the daemon is gone.
    pub fn unwatch_all_but(&self, keep: &[String]) {
        lock(&self.watching).retain(|e, _| keep.contains(e));
    }

    /// Wait, up to `limit`, until no OTHER window's monitor watches `endpoint`. True when none
    /// does (at once, in the ordinary case: no other window, or none watching); false if the
    /// limit passed, and the open goes on as it always did.
    pub fn wait_unwatched(&self, endpoint: &str, limit: Duration) -> bool {
        if lock(&self.watching).contains_key(endpoint) {
            return true; // our own monitor: an in-process handoff, not another window
        }
        let path = self.file("watch", endpoint);
        let until = Instant::now() + limit;
        while locked_elsewhere(&path) {
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        true
    }
}

/// Another handle holds `path`'s lock (exclusive or shared). A missing file, or one this
/// filesystem cannot lock, holds nothing.
fn locked_elsewhere(path: &Path) -> bool {
    let Ok(f) = File::open(path) else {
        return false;
    };
    match f.try_lock() {
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
    CLAIMS.get()
}

/// Is `p` driven by another live window? False with no claims, and for a radio with no endpoint.
pub fn profile_held_elsewhere(p: &RadioProfile) -> bool {
    match (claims(), cat_endpoint(p)) {
        (Some(c), Some(e)) => c.held_elsewhere(&e),
        _ => false,
    }
}

/// Claim `p` for this process. `Ok` with no claims, and for a radio with no endpoint.
pub fn claim_profile(p: &RadioProfile) -> Result<(), String> {
    match (claims(), cat_endpoint(p)) {
        (Some(c), Some(e)) => c.claim(&e),
        _ => Ok(()),
    }
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
        // CONTROLS: a radio nobody drives stays watched, and a window's own watch never makes
        // its own open wait.
        assert!(monitor.watch("serial:COM9"));
        monitor.unwatch_all_but(&["serial:COM9".to_string()]);
        assert!(
            monitor.wait_unwatched("serial:COM9", Duration::ZERO),
            "own watch"
        );
        assert!(
            !driver.wait_unwatched("serial:COM9", Duration::ZERO),
            "another's watch"
        );
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
