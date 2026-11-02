//! One radio, one Nexus window: the claim a window holds on the radio it is driving.
//!
//! # Why
//!
//! Two Nexus processes driving one radio share its rigctld: the second finds the first one's daemon
//! answering on the radio's port and connects through it (`open_cat`'s coexist branch, which is
//! right for a rigctld WSJT-X started). When the first window closes, its daemon goes with it, and
//! the second window's CAT, unkey included, talks to nothing until the silence rebuild reopens the
//! port minutes later. The picker no longer causes this (`PICKER_HELD_RADIO`), but an operator
//! still could: switch a radio window to the radio another window runs, open a hand-named profile
//! on it, or press "Use one radio" with that radio open elsewhere. Those three refuse it now, with
//! [`REFUSAL`].
//!
//! # The mechanism, and why this one
//!
//! A window driving radio `id` holds an exclusive OS file lock on `<base config>/radio-claims/
//! radio-<id>.lock` for as long as it drives it ([`Claims::claim`]); switching moves the lock to the
//! new radio. Another process asking [`Claims::held_elsewhere`] tries the same lock and reads
//! "would block" as "a live window has it".
//!
//! - **A crashed window never locks a radio out.** The lock is held by an open file handle, and the
//!   OS drops every lock a process holds when the process ends, however it ends: killed, crashed,
//!   powered off mid-run. The file stays behind and means nothing; only a held lock refuses.
//!   (`flock` on Unix, `LockFileEx` on Windows, through std's `File::try_lock`.) A marker with a pid
//!   in it would need a liveness check and would be fooled by a reused pid; this has no such state.
//! - **It is the mechanism the profile lock already uses** (`hold_profile_lock`), which the picker's
//!   "in use" greying reads, so a stuck lock is a failure mode this code base has never had.
//! - **It is per radio, not per profile.** A radio window holds `tempo-r<id>`'s profile lock, but
//!   the single band-following window and a hand-named profile drive a radio under some other
//!   profile, so the profile lock cannot see them.
//!
//! The lock is advisory: only Nexus asks it. A filesystem that cannot lock reads as free
//! (fail-open), as the profile lock does: refusing every radio there would be worse than the
//! hazard. The key is the radio's id, which the base profile and every radio window share (a
//! radio window's roster is seeded from the base one and mirrored back).

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What the refusal says, everywhere it is said.
pub(crate) const REFUSAL: &str =
    "That radio is open in another Nexus window — switch to that window.";

/// This process's claim: the claim folder, and the radio it holds with the lock's handle.
pub(crate) struct Claims {
    dir: PathBuf,
    held: Mutex<Option<(u32, File)>>,
}

impl Claims {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            held: Mutex::new(None),
        }
    }

    fn path(&self, id: u32) -> PathBuf {
        self.dir.join(format!("radio-{id}.lock"))
    }

    /// Is `id` driven by ANOTHER live Nexus window? Never true for the radio this process holds,
    /// and never true for a lock file left by a window that has ended.
    pub(crate) fn held_elsewhere(&self, id: u32) -> bool {
        if self.holds(id) {
            return false;
        }
        let Ok(f) = File::open(self.path(id)) else {
            return false; // never claimed → free
        };
        match f.try_lock() {
            Ok(()) => {
                let _ = f.unlock(); // only a peek
                false
            }
            Err(TryLockError::WouldBlock) => true,
            Err(_) => false, // cannot lock here → fail-open
        }
    }

    /// Does this process hold `id`?
    pub(crate) fn holds(&self, id: u32) -> bool {
        self.lock_held().as_ref().is_some_and(|(h, _)| *h == id)
    }

    /// Take `id` for this process, letting go of the radio it held before. `Err(REFUSAL)` when
    /// another live window holds it; this process then keeps what it had.
    pub(crate) fn claim(&self, id: u32) -> Result<(), String> {
        let mut held = self.lock_held();
        if held.as_ref().is_some_and(|(h, _)| *h == id) {
            return Ok(());
        }
        let _ = std::fs::create_dir_all(&self.dir);
        let Ok(f) = open_for_lock(&self.path(id)) else {
            return Ok(()); // cannot make the file → fail-open, as the profile lock does
        };
        match f.try_lock() {
            Ok(()) => {
                *held = Some((id, f)); // the old handle drops here, and its lock with it
                Ok(())
            }
            Err(TryLockError::WouldBlock) => Err(REFUSAL.to_string()),
            Err(_) => Ok(()),
        }
    }

    fn lock_held(&self) -> std::sync::MutexGuard<'_, Option<(u32, File)>> {
        self.held.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn open_for_lock(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
}

/// This process's claims, in the BASE profile's config folder: shared by every window on purpose,
/// as the daemon ledger is.
pub(crate) fn claims() -> &'static Claims {
    static CLAIMS: std::sync::OnceLock<Claims> = std::sync::OnceLock::new();
    CLAIMS.get_or_init(|| Claims::new(crate::config_dir_for(None).join("radio-claims")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;

    /// `id` reads free within 2 s. A lock let go of is free at once, except that a process this
    /// test binary forks at that moment (another test starting a child) holds a copy of every open
    /// handle until its `exec` closes them, a few milliseconds. Nexus has the same few-millisecond
    /// window when it starts a daemon; nothing waits on it.
    fn soon_free(c: &Claims, id: u32) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while c.held_elsewhere(id) {
            if std::time::Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        true
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-claims-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// Two windows, here two claim holders with their own handles, which the OS treats exactly as
    /// two processes: the second is refused the radio the first drives, and gets it once the
    /// first lets go.
    #[test]
    fn a_radio_one_window_drives_is_refused_to_another() {
        let dir = temp_dir("two");
        let a = Claims::new(dir.clone());
        let b = Claims::new(dir.clone());
        assert_eq!(a.claim(1), Ok(()));
        assert!(b.held_elsewhere(1));
        assert_eq!(b.claim(1), Err(REFUSAL.to_string()));
        // CONTROLS: another radio is free, and a window never refuses itself.
        assert!(!b.held_elsewhere(2));
        assert_eq!(b.claim(2), Ok(()));
        assert!(!a.held_elsewhere(1));
        assert_eq!(a.claim(1), Ok(()));
        // A switches to 3: radio 1 is free again, for B.
        assert_eq!(a.claim(3), Ok(()));
        assert!(soon_free(&b, 1));
        assert_eq!(b.claim(1), Ok(()));
        assert!(!b.holds(2), "B let go of 2 when it took 1");
        assert!(soon_free(&a, 2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A refused claim leaves the window on the radio it had.
    #[test]
    fn a_refused_claim_keeps_the_radio_the_window_had() {
        let dir = temp_dir("keep");
        let a = Claims::new(dir.clone());
        let b = Claims::new(dir.clone());
        a.claim(1).unwrap();
        b.claim(2).unwrap();
        assert!(b.claim(1).is_err());
        assert!(b.holds(2));
        assert!(a.held_elsewhere(2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    const CHILD: &str = "NEXUS_CLAIM_CHILD_DIR";

    /// The child half of the crash test: claim radio 5 in the folder it is given, say so, and
    /// wait to be killed.
    #[test]
    fn claim_child_holds_radio_5_until_killed() {
        let Ok(dir) = std::env::var(CHILD) else {
            return; // only as the child
        };
        let c = Claims::new(PathBuf::from(dir));
        c.claim(5).expect("the child claims radio 5");
        println!("CLAIM-CHILD-HELD");
        std::thread::sleep(std::time::Duration::from_secs(120));
    }

    /// A REAL other process holds the radio, then dies without letting go (SIGKILL / TerminateProcess:
    /// no destructor, no cleanup). While it lives, its radio is refused; once it is dead, the lock
    /// file it left behind refuses nothing.
    #[test]
    fn a_window_that_crashed_never_locks_its_radio_out() {
        let dir = temp_dir("crash");
        let mut child = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "radio_claim::tests::claim_child_holds_radio_5_until_killed",
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
        assert!(me.held_elsewhere(5), "a live window's radio is refused");
        assert_eq!(me.claim(5), Err(REFUSAL.to_string()));

        child.kill().expect("kill the child, no cleanup");
        let _ = child.wait();
        assert!(
            dir.join("radio-5.lock").exists(),
            "premise: its file is left behind"
        );
        assert!(soon_free(&me, 5), "a dead window holds nothing");
        assert_eq!(me.claim(5), Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
