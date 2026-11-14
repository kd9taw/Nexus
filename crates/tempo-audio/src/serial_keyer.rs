//! Serial DTR/RTS keyline CW keyer — the classic "PC keys the rig's KEY jack" method.
//!
//! The app toggles a serial control line (DTR or RTS) with Morse timing; a simple
//! transistor/optoisolator interface keys the rig's straight-key line, and the RIG (in CW
//! mode) shapes the actual RF envelope. This is what N1MM+, fldigi, Win-Test, and DXLab
//! call "serial CW keying" — the clean way to key a rig that lacks CAT CW (e.g. the
//! IC-756PRO III) without buying a WinKeyer, and without the soundcard-through-SSB
//! workaround.
//!
//! Unlike the WinKeyer (hardware owns the timing) or the soundcard keyer (the audio
//! callback plays a pre-rendered tone), this keyer must generate the element timing
//! itself, so it owns a dedicated keying thread. The service loop hands it one word at a
//! time; the thread walks `tempo_core::cw::morse_key_events` and toggles the line, keying
//! up immediately on abort. (PC-generated timing has some OS jitter — inherent to serial
//! keying; a WinKeyer is the jitter-free upgrade.)
//!
//! ⛔ THE PORT IS HELD, NOT OWNED ([`crate::control_line::SharedControlLines`]). The conventional
//! keyline interface is DTR = key, RTS = PTT on ONE port, and serial PTT ([`crate::rig`]) holds
//! that port once it has keyed. This keyer opened it exclusively, so whichever of the two came
//! second was refused ("Access is denied.") and nothing keyed. Both now key through one handle;
//! each lowers only the line it raised, and a key-up whose write fails is retried (by the keying
//! thread, and again when the holder goes). That the two never claim the SAME line is the engine's
//! guard (`Engine::cw_keyline_ptt_conflict`), as for the FSK keyer.

/// Which control line keys the rig. **Dtr** is the CW convention (RTS = PTT); **Rts** is
/// offered because some interfaces wire it the other way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyLine {
    Dtr,
    Rts,
}

impl KeyLine {
    /// Parse `"dtr"`/`"rts"` (case-insensitive); anything else → `Dtr` (the default).
    pub fn parse(s: &str) -> Self {
        if s.eq_ignore_ascii_case("rts") {
            KeyLine::Rts
        } else {
            KeyLine::Dtr
        }
    }
}

#[cfg(feature = "serial")]
pub use imp::SerialKeyer;

#[cfg(feature = "serial")]
mod imp {
    use super::KeyLine;
    use crate::control_line::{ControlLinePins, SharedControlLines};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    /// Put the key line down (`true`) or up; whether the write went out.
    fn set_key(sp: &mut SharedControlLines, line: KeyLine, down: bool) -> bool {
        match line {
            KeyLine::Dtr => sp.set_dtr(down),
            KeyLine::Rts => sp.set_rts(down),
        }
        .is_ok()
    }

    /// How often the keying thread retries a key-up whose write failed ([`keyer_loop`]).
    const KEY_UP_RETRY_MS: u64 = 20;

    /// Failed key-ups in a row (about 100 ms of them) before the thread says the key may be stuck
    /// ([`SerialKeyer::key_up_failing`]): one refused write that the next retry clears says nothing.
    const KEY_UP_STUCK_TRIES: u32 = 5;

    /// What the keying thread is handed: a word to key at a speed, or a tune's key-down (#500)
    /// held until a stop or its own limit, whichever comes first.
    enum Job {
        Word(String, u32),
        Hold(Duration),
    }

    /// The keying thread: block for a job, key a word's Morse elements on the line or hold a
    /// tune's key-down, key up on abort (checked every ~8 ms so Stop TX is snappy), and exit
    /// when the channel closes.
    ///
    /// ⛔ A STOP IS A GENERATION, NOT A FLAG. Every job carries the stop count it was handed
    /// over under, and a stop bumps the count, so a job handed over before a stop never keys
    /// after it, wherever the thread was when the stop landed. A flag the thread cleared as it
    /// took each word lost every stop that landed between a word's hand-over and its read: a
    /// word already waiting in the channel then keyed in full after Stop TX.
    ///
    /// ⛔ A KEY-UP THAT FAILS IS RETRIED HERE, NOT LEFT FOR LATER. `set` answers whether its write
    /// went out. On a break-in rig the key line alone is the transmitter, and the port may stay
    /// open under another holder (serial PTT), so no port close will drop it: a key-up that did
    /// not go out is tried again every [`KEY_UP_RETRY_MS`] before the thread does anything else,
    /// until it goes out or the keyer is dropped (`closing`) — whose holder then tries once more
    /// as it lets go ([`SharedControlLines`]'s drop).
    ///
    /// After [`KEY_UP_STUCK_TRIES`] failures in a row it raises `failing` (the radio loop puts that
    /// in the CW pane, [`SerialKeyer::key_up_failing`]) and logs ONE warning; a key-up that goes
    /// out lowers it, and a later run of failures logs once more. Never a line per retry.
    ///
    /// ⚠️ ONLY A KEY THAT WENT DOWN IS HELD TO COMING UP (`down`: a key-down write went out and no
    /// key-up has since). Some ports refuse every control-line write (some virtual and Bluetooth
    /// ports, a pty): their key never went down, so a failed key-up there is tried once, as it
    /// always was, and neither wedges the thread nor says a key is stuck that never keyed.
    ///
    /// ⛔ …BUT A WORKING PORT'S FAILED KEY-DOWN COUNTS AS DOWN. A write's error does not prove the
    /// line did not move, so once the port has taken any write (`worked`), a key-down that reports
    /// failure is held to coming up as one that went out. Only a port that has never taken a write
    /// keeps the one-try key-up.
    fn keyer_loop(
        mut set: impl FnMut(bool) -> bool,
        rx: mpsc::Receiver<(Job, u64)>,
        stops: Arc<AtomicU64>,
        closing: Arc<AtomicBool>,
        failing: Arc<AtomicBool>,
    ) {
        let key_up = |set: &mut dyn FnMut(bool) -> bool, down: &mut bool| {
            let mut tries = 0;
            while !set(false) {
                if !*down {
                    return; // the key never went down: nothing is held, one try as ever
                }
                tries += 1;
                if tries == KEY_UP_STUCK_TRIES {
                    failing.store(true, Ordering::SeqCst);
                    tempo_core::applog::warn(
                        "tx",
                        "CW serial keyline: the key-up write keeps failing — the key may be stuck \
                         down; retrying until it goes out",
                    );
                }
                if closing.load(Ordering::SeqCst) {
                    return;
                }
                thread::sleep(Duration::from_millis(KEY_UP_RETRY_MS));
            }
            *down = false;
            if failing.swap(false, Ordering::SeqCst) {
                tempo_core::applog::info("tx", "CW serial keyline: the key-up went out");
            }
        };
        // Whether the port has taken any write, and whether a key-down went out (or, on such a
        // port, may have) with no key-up since: the key is held to coming up.
        let worked = std::cell::Cell::new(false);
        let mut set = |level: bool| {
            let ok = set(level);
            if ok {
                worked.set(true);
            }
            ok
        };
        let mut down = false;
        key_up(&mut set, &mut down); // idle: key up

        // Sleep `ms` in 8 ms slices; false the moment a stop lands.
        let wait = |ms: u64, stop: u64| {
            let mut left = ms;
            while left > 0 {
                if stops.load(Ordering::SeqCst) != stop {
                    return false;
                }
                let chunk = left.min(8);
                thread::sleep(Duration::from_millis(chunk));
                left -= chunk;
            }
            true
        };
        while let Ok((job, stop)) = rx.recv() {
            if stops.load(Ordering::SeqCst) != stop {
                continue; // handed over before a stop: dropped, never keyed
            }
            match job {
                Job::Word(text, wpm) => {
                    for (key_down, ms) in tempo_core::cw::morse_key_events(&text, wpm) {
                        if key_down {
                            down |= set(true) || worked.get();
                        } else {
                            key_up(&mut set, &mut down);
                        }
                        if !wait(ms as u64, stop) {
                            break;
                        }
                    }
                }
                // ⛔ BOUNDED BY ITSELF. The loop ends a tune with a stop on every exit path; the
                // limit is this thread's own, so a loop that never comes back still lets the
                // key up, as a WinKeyer's 100 s tune watchdog does.
                Job::Hold(limit) => {
                    down |= set(true) || worked.get();
                    wait(limit.as_millis() as u64, stop);
                }
            }
            key_up(&mut set, &mut down); // key up between jobs, and at once on a stop
        }
        key_up(&mut set, &mut down); // channel closed (keyer dropped) → key up + exit
    }

    /// An open serial keyline keyer with its own keying thread.
    pub struct SerialKeyer {
        tx: Option<mpsc::Sender<(Job, u64)>>,
        /// How many stops there have been ([`keyer_loop`]).
        stops: Arc<AtomicU64>,
        /// Set by the drop: the thread stops retrying a failed key-up and exits ([`keyer_loop`]).
        closing: Arc<AtomicBool>,
        /// Raised by the thread while a key-up keeps failing ([`Self::key_up_failing`]).
        failing: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl SerialKeyer {
        /// Open `port` and spawn the keying thread. The baud rate carries no meaning here —
        /// only the DTR/RTS control line is toggled, no data bytes are sent — so
        /// [`crate::control_line`] picks a rate the port will actually accept (some rigs
        /// refuse a given rate at open; the FTX-1 refuses 1200). Both lines start idle
        /// (deasserted) — the opener guarantees it, see
        /// [`crate::control_line::idle_both_lines`] for the stuck-PTT rule that pair of
        /// writes used to live here to enforce.
        ///
        /// The port is HELD, not owned ([`SharedControlLines`]): serial PTT on the other line of
        /// the same port keys through the same handle, and a port another holder already has is
        /// joined as it is — its lines are not idled. The holder lives in the keying thread's
        /// closure, so it goes — lowering the key line if it was left down — however that thread
        /// ends: the keyer dropped, or a panic unwinding it.
        pub fn open(port: &str, line: KeyLine) -> std::io::Result<Self> {
            let mut sp = SharedControlLines::open(port)?;
            Ok(Self::keying_checked(move |down| {
                set_key(&mut sp, line, down)
            }))
        }

        /// Spawn the keying thread over `set`, which puts the key line down (`true`) or up and
        /// cannot fail (a test's recording line).
        #[cfg(test)]
        pub(crate) fn keying(mut set: impl FnMut(bool) + Send + 'static) -> Self {
            Self::keying_checked(move |down| {
                set(down);
                true
            })
        }

        /// [`Self::keying`] over a line whose write can fail: `set` answers whether it went out,
        /// and a key-up that did not is retried ([`keyer_loop`]).
        pub(crate) fn keying_checked(set: impl FnMut(bool) -> bool + Send + 'static) -> Self {
            let (tx, rx) = mpsc::channel();
            let stops = Arc::new(AtomicU64::new(0));
            let closing = Arc::new(AtomicBool::new(false));
            let failing = Arc::new(AtomicBool::new(false));
            let (stops_thread, closing_thread, failing_thread) =
                (stops.clone(), closing.clone(), failing.clone());
            let handle = thread::spawn(move || {
                keyer_loop(set, rx, stops_thread, closing_thread, failing_thread)
            });
            Self {
                tx: Some(tx),
                stops,
                closing,
                failing,
                handle: Some(handle),
            }
        }

        /// Queue `text` to be keyed at `wpm` (non-blocking — the thread does the timing).
        pub fn send(&self, text: &str, wpm: u32) {
            if let Some(tx) = &self.tx {
                let job = Job::Word(text.to_string(), wpm);
                let _ = tx.send((job, self.stops.load(Ordering::SeqCst)));
            }
        }

        /// Hold the key down for a tune (#500) until [`Self::clear`], the keyer's drop, or
        /// `limit`, whichever comes first (non-blocking).
        pub fn hold(&self, limit: Duration) {
            if let Some(tx) = &self.tx {
                let _ = tx.send((Job::Hold(limit), self.stops.load(Ordering::SeqCst)));
            }
        }

        /// Whether the key line's key-up has been failing for a while ([`keyer_loop`]): the key
        /// may be stuck down, and the thread is still retrying.
        pub fn key_up_failing(&self) -> bool {
            self.failing.load(Ordering::SeqCst)
        }

        /// Abort NOW: key up and drop any queued words (Stop TX).
        pub fn clear(&self) {
            self.stops.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl Drop for SerialKeyer {
        fn drop(&mut self) {
            self.stops.fetch_add(1, Ordering::SeqCst); // interrupt a word in progress
            self.closing.store(true, Ordering::SeqCst); // end a failed key-up's retries
            self.tx = None; // close the channel → the thread keys up and exits
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    /// A key line whose key-up NEVER goes out (the port died under it). The thread retries, paced,
    /// and says so; the keyer's drop still returns.
    #[cfg(test)]
    mod dead_port_tests {
        use super::SerialKeyer;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::{mpsc, Arc};
        use std::time::{Duration, Instant};

        /// ⛔ THE DROP RETURNS ON A DEAD PORT, and the retries are paced (~50 a second), not a hot
        /// spin. Without the drop's `closing` the retry never ends and the join hangs the radio
        /// loop: a port change, a backend change or app shutdown would wedge on a dead adapter.
        #[test]
        fn a_dead_port_neither_hangs_the_drop_nor_spins() {
            let tries = Arc::new(AtomicUsize::new(0));
            let t = Arc::clone(&tries);
            let keyer = SerialKeyer::keying_checked(move |down| {
                if !down {
                    t.fetch_add(1, Ordering::SeqCst);
                }
                down // a key-down "works"; every key-up fails, forever
            });
            keyer.hold(Duration::from_millis(50));
            std::thread::sleep(Duration::from_millis(1050)); // ~1 s of failing key-ups
            let n = tries.load(Ordering::SeqCst);
            assert!((5..=80).contains(&n), "paced retries, ~50/s: {n}");
            assert!(
                keyer.key_up_failing(),
                "the keyer says the key may be stuck"
            );
            let (done, wait) = mpsc::channel();
            std::thread::spawn(move || {
                drop(keyer);
                let _ = done.send(());
            });
            assert!(
                wait.recv_timeout(Duration::from_secs(2)).is_ok(),
                "the drop returns although the key-up never goes out"
            );
        }

        /// ⛔ A KEY-DOWN THAT REPORTS FAILURE ON A PORT THAT WORKS MAY STILL HAVE KEYED: a write's
        /// error does not prove the line did not move. Once the port has taken a write, such a
        /// key-down counts as down, so a key-up that then fails is retried and said, not tried once.
        #[test]
        fn an_ambiguous_key_down_on_a_working_port_is_held_to_coming_up() {
            let keyups = Arc::new(AtomicUsize::new(0));
            let k = Arc::clone(&keyups);
            let first = AtomicBool::new(true);
            let keyer = SerialKeyer::keying_checked(move |down| {
                if down {
                    return false; // the key-down "fails"
                }
                k.fetch_add(1, Ordering::SeqCst);
                first.swap(false, Ordering::SeqCst) // the idle key-up works; every later one fails
            });
            keyer.hold(Duration::from_millis(10));
            let start = Instant::now();
            while !keyer.key_up_failing() && start.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(
                keyer.key_up_failing(),
                "retried and said: {} key-ups",
                keyups.load(Ordering::SeqCst)
            );
        }

        /// A port that refuses EVERY control-line write (some virtual and Bluetooth ports, a pty):
        /// its key never went down, so a refused key-up is tried once as it always was. The thread
        /// is not wedged on it (it still takes each word), and says no key is stuck.
        #[test]
        fn a_port_that_refuses_every_write_neither_wedges_nor_says_stuck() {
            let downs = Arc::new(AtomicUsize::new(0));
            let d = Arc::clone(&downs);
            let keyer = SerialKeyer::keying_checked(move |down| {
                if down {
                    d.fetch_add(1, Ordering::SeqCst);
                }
                false
            });
            keyer.send("E", 60);
            keyer.send("E", 60);
            let start = Instant::now();
            while downs.load(Ordering::SeqCst) < 2 && start.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(downs.load(Ordering::SeqCst), 2, "both words were taken");
            std::thread::sleep(Duration::from_millis(200));
            assert!(
                !keyer.key_up_failing(),
                "no key went down: none is said stuck"
            );
        }

        /// One refused key-up that the next retry clears says nothing; a run of them raises the
        /// stuck flag, and the key-up going out lowers it again.
        #[test]
        fn the_stuck_flag_rises_on_a_run_of_failures_and_falls_when_the_key_comes_up() {
            let refuse = Arc::new(AtomicBool::new(false));
            let r = Arc::clone(&refuse);
            let keyer = SerialKeyer::keying_checked(move |down| down || !r.load(Ordering::SeqCst));
            keyer.hold(Duration::from_secs(60));
            std::thread::sleep(Duration::from_millis(50));
            refuse.store(true, Ordering::SeqCst);
            keyer.clear();
            let start = Instant::now();
            while !keyer.key_up_failing() && start.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(keyer.key_up_failing(), "a run of failed key-ups is said");
            refuse.store(false, Ordering::SeqCst);
            let start = Instant::now();
            while keyer.key_up_failing() && start.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(
                !keyer.key_up_failing(),
                "the key-up went out: no longer said"
            );
        }
    }

    /// The keyline and serial PTT on ONE port: CW keys DTR, PTT holds RTS, through one handle
    /// ([`SharedControlLines`]). The port is a [`crate::control_line::fake_ports`] port, which
    /// opens exclusively as a COM port does, so a keyer that opened it for itself is refused.
    #[cfg(test)]
    mod shared_port_tests {
        use super::{KeyLine, SerialKeyer};
        use crate::control_line::fake_ports::{self, State};
        use crate::control_line::{ControlLinePins, SharedControlLines};
        use std::time::{Duration, Instant};

        /// Wait (at most 2 s) until `port`'s lines satisfy `want`.
        fn wait_for(port: &str, want: impl Fn(State) -> bool) -> State {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(2) {
                let s = fake_ports::state(port);
                if want(s) {
                    return s;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            fake_ports::state(port)
        }

        /// Serial PTT up on RTS of `port`, held as `Rig::serial_ptt` holds it.
        fn ptt_up(port: &str) -> SharedControlLines {
            fake_ports::install(port);
            let mut ptt = SharedControlLines::open(port).expect("PTT opens the port");
            ptt.set_rts(true).unwrap();
            ptt
        }

        /// ★ THE BUG: with serial PTT holding the port, the keyer's own open was refused ("Access
        /// is denied.") and nothing keyed. It joins the open handle now, keys DTR without touching
        /// RTS, and a word's end leaves the key up and PTT where PTT put it.
        #[test]
        fn the_keyline_keys_on_the_port_serial_ptt_holds() {
            let port = "tempo-test-cw-shared-open";
            let ptt = ptt_up(port);
            let keyer =
                SerialKeyer::open(port, KeyLine::Dtr).expect("the keyer shares the port PTT holds");
            assert!(fake_ports::state(port).rts, "joining does not idle PTT");
            keyer.send("E", 60);
            let s = wait_for(port, |s| s.dtr);
            assert!(s.dtr && s.rts, "the key goes down under PTT: {s:?}");
            let s = wait_for(port, |s| !s.dtr);
            assert!(
                !s.dtr && s.rts,
                "the word ends keyed up, PTT untouched: {s:?}"
            );
            // The keyer goes mid-word: its line drops, PTT's stays.
            keyer.send("TTTT", 5);
            assert!(wait_for(port, |s| s.dtr).dtr, "control: keying");
            drop(keyer);
            let s = fake_ports::state(port);
            assert!(
                !s.dtr && s.rts && s.held,
                "only the keyer's line drops: {s:?}"
            );
            drop(ptt);
            let s = fake_ports::state(port);
            assert!(
                !s.rts && !s.dtr && !s.held,
                "the last holder closes it: {s:?}"
            );
        }

        /// …and the other way round: PTT lets go (the rig drops its port) while a tune holds the
        /// key down. PTT's line drops; the key, the keyer's, stays down until the keyer lets it up.
        #[test]
        fn ptt_letting_go_leaves_the_key_where_the_keyer_put_it() {
            let port = "tempo-test-cw-shared-ptt-goes";
            let ptt = ptt_up(port);
            let keyer = SerialKeyer::open(port, KeyLine::Dtr).unwrap();
            keyer.hold(Duration::from_secs(60));
            assert!(wait_for(port, |s| s.dtr).dtr, "control: held down");
            drop(ptt);
            let s = fake_ports::state(port);
            assert!(!s.rts && s.dtr && s.held, "only PTT's line drops: {s:?}");
            keyer.clear();
            let s = wait_for(port, |s| !s.dtr);
            assert!(!s.dtr, "the stop lets the key up: {s:?}");
        }

        /// ⛔ A KEY-UP WHOSE WRITE FAILS IS RETRIED. PTT keeps the port open, so no port close will
        /// drop the key: the keying thread tries again until the key-up goes out.
        #[test]
        fn a_failed_key_up_is_retried_by_the_keying_thread() {
            let port = "tempo-test-cw-shared-retry";
            let ptt = ptt_up(port);
            let keyer = SerialKeyer::open(port, KeyLine::Dtr).unwrap();
            keyer.hold(Duration::from_secs(60));
            assert!(wait_for(port, |s| s.dtr).dtr, "control: held down");
            fake_ports::fail_next_write(port);
            keyer.clear();
            let s = wait_for(port, |s| !s.dtr);
            assert!(!s.dtr && s.rts, "the failed key-up was retried: {s:?}");
            drop(keyer);
            drop(ptt);
        }

        /// …and a key-up that fails as the keyer goes (the port setting cleared, shutdown) is
        /// retried by the holder as it lets go: the drop does not return with the key down.
        #[test]
        fn a_failed_key_up_at_the_keyers_drop_is_retried_on_the_way_out() {
            let port = "tempo-test-cw-shared-drop-retry";
            let ptt = ptt_up(port);
            let keyer = SerialKeyer::open(port, KeyLine::Dtr).unwrap();
            keyer.hold(Duration::from_secs(60));
            assert!(wait_for(port, |s| s.dtr).dtr, "control: held down");
            fake_ports::fail_next_write(port);
            drop(keyer);
            let s = fake_ports::state(port);
            assert!(
                !s.dtr && s.rts && s.held,
                "the key is up when the drop returns: {s:?}"
            );
            drop(ptt);
        }

        /// The keying thread dies (a panic) with the key down: unwinding drops its holder, which
        /// lets the key up, and PTT, the other holder's, is untouched.
        #[test]
        fn a_keying_thread_that_panics_leaves_the_key_up() {
            let port = "tempo-test-cw-shared-panic";
            let ptt = ptt_up(port);
            let mut sp = SharedControlLines::open(port).unwrap();
            let mut was_down = false;
            let keyer = SerialKeyer::keying(move |down| {
                if !down && was_down {
                    panic!("test: the keying thread dies with the key down");
                }
                was_down |= down;
                let _ = sp.set_dtr(down);
            });
            keyer.send("E", 60);
            let start = Instant::now();
            while !keyer.handle.as_ref().unwrap().is_finished()
                && start.elapsed() < Duration::from_secs(2)
            {
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(
                fake_ports::log(port).iter().any(|s| s.dtr),
                "premise: the key went down before the thread died"
            );
            // Before the keyer's own drop: the unwind alone let the key up.
            let s = fake_ports::state(port);
            assert!(
                !s.dtr && s.rts && s.held,
                "the dead thread's key is up: {s:?}"
            );
            drop(keyer);
            drop(ptt);
        }
    }

    /// A stop ends the keying, wherever in the macro it lands. The line is a recorder, so these
    /// run without a serial port; the thread, the channel and the stop are the real ones.
    #[cfg(test)]
    mod stop_tests {
        use super::SerialKeyer;
        use std::sync::{mpsc, Arc, Mutex};
        use std::time::{Duration, Instant};

        /// A keyer whose line records each level it is put at. Its thread is held at its first
        /// key-up until `go` is sent, so a test can hand words over and stop before it reads one.
        fn held_keyer() -> (SerialKeyer, Arc<Mutex<Vec<bool>>>, mpsc::Sender<()>) {
            let levels = Arc::new(Mutex::new(Vec::new()));
            let seen = Arc::clone(&levels);
            let (go, wait) = mpsc::channel::<()>();
            let mut held = true;
            let keyer = SerialKeyer::keying(move |down| {
                if std::mem::take(&mut held) {
                    let _ = wait.recv();
                }
                seen.lock().unwrap().push(down);
            });
            (keyer, levels, go)
        }

        fn key_downs(levels: &Mutex<Vec<bool>>) -> usize {
            levels.lock().unwrap().iter().filter(|d| **d).count()
        }

        /// Wait until the line has been still for 300 ms (at most 5 s).
        fn settle(levels: &Mutex<Vec<bool>>) {
            let start = Instant::now();
            let mut last = (levels.lock().unwrap().len(), Instant::now());
            while start.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(20));
                let n = levels.lock().unwrap().len();
                if n != last.0 {
                    last = (n, Instant::now());
                } else if last.1.elapsed() > Duration::from_millis(300) {
                    return;
                }
            }
        }

        /// The rest of a macro is in the keyer's hands before the thread reads it, and the
        /// operator stops: none of it may key afterwards. It used to: the thread cleared the stop
        /// as it took each word, so a word handed over before the stop was keyed in full.
        #[test]
        fn a_word_handed_over_before_a_stop_is_never_keyed_after_it() {
            let (keyer, levels, go) = held_keyer();
            keyer.send("TEST", 60);
            keyer.clear();
            go.send(()).unwrap();
            // POSITIVE CONTROL: a word sent after the stop keys, so the line is really read.
            keyer.send("E", 60);
            settle(&levels);
            assert_eq!(
                key_downs(&levels),
                1,
                "only the E sent after the stop may key; the line went {:?}",
                levels.lock().unwrap()
            );
            assert_eq!(
                levels.lock().unwrap().last(),
                Some(&false),
                "the key ends up"
            );
        }

        /// A stop in the middle of a word keys up within one 8 ms slice, and the rest of the
        /// word and the word queued behind it never key.
        #[test]
        fn a_stop_mid_word_keys_up_at_once_and_drops_the_rest() {
            let (keyer, levels, go) = held_keyer();
            keyer.send("0000", 20); // 76 elements of 60 ms: about 4.5 s
            keyer.send("TEST", 20);
            go.send(()).unwrap();
            let start = Instant::now();
            while key_downs(&levels) < 2 && start.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(key_downs(&levels) >= 2, "control: the word must be keying");
            keyer.clear();
            let stopped = Instant::now();
            while levels.lock().unwrap().last() != Some(&false)
                && stopped.elapsed() < Duration::from_millis(500)
            {
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(levels.lock().unwrap().last(), Some(&false), "keyed up");
            let downs = key_downs(&levels);
            settle(&levels);
            assert_eq!(key_downs(&levels), downs, "nothing keys after the stop");
            assert!(
                downs < 20,
                "the word was cut, not finished: {downs} key-downs"
            );
        }

        /// Wait (at most 2 s) for the line's last level to be `down`.
        fn last_is(levels: &Mutex<Vec<bool>>, down: bool) -> bool {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(2) {
                if levels.lock().unwrap().last() == Some(&down) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            false
        }

        /// #500: a tune's hold keeps the key down, with no element timing, until a stop.
        #[test]
        fn a_tune_hold_keys_down_until_a_stop() {
            let (keyer, levels, go) = held_keyer();
            keyer.hold(Duration::from_secs(60));
            go.send(()).unwrap();
            assert!(last_is(&levels, true), "the hold keys down");
            std::thread::sleep(Duration::from_millis(100));
            assert_eq!(
                levels.lock().unwrap().last(),
                Some(&true),
                "…and stays down: {:?}",
                levels.lock().unwrap()
            );
            keyer.clear();
            assert!(last_is(&levels, false), "a stop keys up at once");
            settle(&levels);
            assert_eq!(key_downs(&levels), 1, "and nothing keys again");
        }

        /// #500: the hold ends by its own limit if no stop ever comes.
        #[test]
        fn a_tune_hold_keys_up_at_its_own_limit() {
            let (keyer, levels, go) = held_keyer();
            keyer.hold(Duration::from_millis(50));
            go.send(()).unwrap();
            assert!(last_is(&levels, true), "control: the hold keys down");
            assert!(last_is(&levels, false), "the limit keys up with no stop");
        }

        /// #500: dropping the keyer (the port closing, app shutdown) ends a hold, keyed up.
        #[test]
        fn dropping_the_keyer_ends_a_tune_hold_keyed_up() {
            let (keyer, levels, go) = held_keyer();
            keyer.hold(Duration::from_secs(60));
            go.send(()).unwrap();
            assert!(last_is(&levels, true), "control: the hold keys down");
            let start = Instant::now();
            drop(keyer);
            assert!(
                start.elapsed() < Duration::from_secs(1),
                "the drop does not wait the hold out"
            );
            assert_eq!(levels.lock().unwrap().last(), Some(&false), "keyed up");
        }

        /// A hold handed over before a stop never keys after it, as a word does not.
        #[test]
        fn a_tune_hold_handed_over_before_a_stop_never_keys() {
            let (keyer, levels, go) = held_keyer();
            keyer.hold(Duration::from_secs(60));
            keyer.clear();
            go.send(()).unwrap();
            settle(&levels);
            assert_eq!(
                key_downs(&levels),
                0,
                "the line went {:?}",
                levels.lock().unwrap()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyline_parses_case_insensitively_and_defaults_to_dtr() {
        assert_eq!(KeyLine::parse("dtr"), KeyLine::Dtr);
        assert_eq!(KeyLine::parse("DTR"), KeyLine::Dtr);
        assert_eq!(KeyLine::parse("rts"), KeyLine::Rts);
        assert_eq!(KeyLine::parse("RTS"), KeyLine::Rts);
        assert_eq!(KeyLine::parse(""), KeyLine::Dtr); // unknown → the CW convention
        assert_eq!(KeyLine::parse("garbage"), KeyLine::Dtr);
    }
}
