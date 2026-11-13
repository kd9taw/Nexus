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
    use serialport::SerialPort;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    fn set_key(sp: &mut dyn SerialPort, line: KeyLine, down: bool) {
        let _ = match line {
            KeyLine::Dtr => sp.write_data_terminal_ready(down),
            KeyLine::Rts => sp.write_request_to_send(down),
        };
    }

    /// The keying thread: block for a word, key its Morse elements on the line, key up on
    /// abort (checked every ~8 ms so Stop TX is snappy), and exit when the channel closes.
    ///
    /// ⛔ A STOP IS A GENERATION, NOT A FLAG. Every word carries the stop count it was handed
    /// over under, and a stop bumps the count, so a word handed over before a stop never keys
    /// after it, wherever the thread was when the stop landed. A flag the thread cleared as it
    /// took each word lost every stop that landed between a word's hand-over and its read: a
    /// word already waiting in the channel then keyed in full after Stop TX.
    fn keyer_loop(
        mut set: impl FnMut(bool),
        rx: mpsc::Receiver<(String, u32, u64)>,
        stops: Arc<AtomicU64>,
    ) {
        set(false); // idle: key up
        while let Ok((text, wpm, stop)) = rx.recv() {
            if stops.load(Ordering::SeqCst) != stop {
                continue; // handed over before a stop: dropped, never keyed
            }
            'word: for (down, ms) in tempo_core::cw::morse_key_events(&text, wpm) {
                set(down);
                // Sleep the element in slices so an abort keys up within ~8 ms.
                let mut left = ms;
                while left > 0 {
                    if stops.load(Ordering::SeqCst) != stop {
                        break 'word;
                    }
                    let chunk = left.min(8);
                    thread::sleep(Duration::from_millis(chunk as u64));
                    left -= chunk;
                }
            }
            set(false); // key up between words, and at once on a stop
        }
        set(false); // channel closed (keyer dropped) → key up + exit
    }

    /// An open serial keyline keyer with its own keying thread.
    pub struct SerialKeyer {
        tx: Option<mpsc::Sender<(String, u32, u64)>>,
        /// How many stops there have been ([`keyer_loop`]).
        stops: Arc<AtomicU64>,
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
        pub fn open(port: &str, line: KeyLine) -> std::io::Result<Self> {
            let mut sp = crate::control_line::open_control_line_port(port)?;
            Ok(Self::keying(move |down| set_key(&mut *sp, line, down)))
        }

        /// Spawn the keying thread over `set`, which puts the key line down (`true`) or up.
        pub(crate) fn keying(set: impl FnMut(bool) + Send + 'static) -> Self {
            let (tx, rx) = mpsc::channel();
            let stops = Arc::new(AtomicU64::new(0));
            let stops_thread = stops.clone();
            let handle = thread::spawn(move || keyer_loop(set, rx, stops_thread));
            Self {
                tx: Some(tx),
                stops,
                handle: Some(handle),
            }
        }

        /// Queue `text` to be keyed at `wpm` (non-blocking — the thread does the timing).
        pub fn send(&self, text: &str, wpm: u32) {
            if let Some(tx) = &self.tx {
                let _ = tx.send((text.to_string(), wpm, self.stops.load(Ordering::SeqCst)));
            }
        }

        /// Abort NOW: key up and drop any queued words (Stop TX).
        pub fn clear(&self) {
            self.stops.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl Drop for SerialKeyer {
        fn drop(&mut self) {
            self.stops.fetch_add(1, Ordering::SeqCst); // interrupt a word in progress
            self.tx = None; // close the channel → the thread keys up and exits
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
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
