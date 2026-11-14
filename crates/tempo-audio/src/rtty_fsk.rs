//! True-FSK RTTY transmit path — bit-bang a serial control line (DTR or RTS) with
//! Baudot framing; the rig's FSK input does the RF shift (rig in RTTY/RTTY-L mode,
//! which unlocks its narrow RTTY filters). Modeled on the CW serial keyline keyer
//! ([`crate::serial_keyer`]): same dedicated keying thread + channel + stop generation +
//! `serial` feature gate. The serial DATA line (TxD) is useless for this — USB-serial
//! adapters can't do 5-bit/1.5-stop UART framing — so the FSK "data" goes out on a
//! control line, which is exactly what EXTFSK does.
//!
//! TIMING HONESTY: control-line FSK edges come from OS thread scheduling, so this path
//! is casual/Field-Day-grade — a loaded machine can jitter individual edges by a few
//! ms. The soundcard AFSK path ([`crate::rtty_afsk`]) is the timing-cleanest way to
//! transmit RTTY (soundcard-clocked, jitter-free); an external hardware keyer
//! (TinyFSK/Mortty — same keyline model, hardware owns the bit clock) is the
//! contest-grade FSK upgrade. What this keyer DOES guarantee: edges are scheduled
//! against ABSOLUTE f64 deadlines ([`fsk_schedule`]), so jitter never accumulates
//! into baud-rate drift.
//!
//! Line sense: ASSERTED = space, DEASSERTED = mark — the idle (deasserted) line is
//! then the RTTY idle/mark condition, matching the all-deasserted safe state on
//! open/drop. Rigs with the opposite FSK-input sense flip it in their polarity menu.
//! PTT rides the OTHER control line (or CAT), never the keyed one.
//!
//! REVERSE (#492) inverts that sense, as RTTY Reverse swaps the AFSK tones: mark becomes the
//! ASSERTED level and space the deasserted one. It used to reach only the tones, so on FSK
//! the switch flipped the decoder and left the transmitter as it was. The reference is
//! EXTFSK64's "Inv. FSK" (github.com/ja7ude/EXTFSK64, `Main.cpp`, `CFSK::Timer`):
//! `SetPort(m_nFSK, m_invFSK ? m_oFSK : !m_oFSK)` with `m_oFSK = 1` for mark — the normal
//! sense clears the line for mark (this keyer's sense), and Inv. FSK sets it, idle mark
//! included.
//!
//! ⛔ THE LINE HAS TWO IDLES, AND ONLY ONE OF THEM EVER ASSERTS IT ([`Level`]). Between
//! characters, between streamed chunks and through PTT's tail, the line sits at MARK in the
//! configured sense — asserted when inverted, exactly as EXTFSK holds it; anything else is a
//! start bit to the far end. But with the rig UNKEYED (before the first over, after Stop,
//! once an over has ended, and when the keyer goes) it RESTS deasserted in either sense —
//! where EXTFSK keeps an inverted line asserted for as long as the port is open. Deviating
//! is deliberate: a line held asserted while the station receives is the shape the stuck-PTT
//! rule forbids ([`crate::control_line::idle_both_lines`]) — on an interface jumpered to key
//! from that line it is a transmitter Nexus does not know it is holding. A released line
//! keys nothing, and the FSK input's level is meaningless with the rig in receive. The loop
//! rests the line ([`FskKeyer::rest`]) on every tick the rig is not keyed, so the rest
//! follows the unkey on every path that unkeys; a batch puts the line back at mark when it
//! starts, behind PTT ([`LEAD_IN_MS`]). Uninverted, mark and rest are the same level, so
//! the default sense keys exactly as it always has.

use crate::rtty_afsk::baudot_frame;

/// Which control line carries the FSK keying (PTT goes on the other, or via CAT).
/// Same type as the CW keyer; for FSK the common interface wiring is DTR = FSK,
/// RTS = PTT.
pub use crate::serial_keyer::KeyLine;

/// Mark held after PTT before the first start bit of an over, so the rig is fully in
/// transmit and the far end's decoder is sitting on mark before the first character — without
/// it the first character went out on the edge of the key-up and could be clipped. EXTFSK64
/// does the same: `CFSK::SetPTT(on)` calls `SetDelay(8)`, eight ticks of its 11 ms timer of
/// idle mark before the first bit (github.com/ja7ude/EXTFSK64, `Main.cpp`).
pub const LEAD_IN_MS: f64 = 88.0;

/// Where the keyer puts its line: an RTTY level, or released (the module doc's two idles).
#[cfg(feature = "serial")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Level {
    Mark,
    Space,
    /// Released (deasserted) whatever the sense: the rig is not keyed.
    Rest,
}

/// Whether `level` asserts the line of a keyer keying `inverted` (RTTY Reverse): space is
/// asserted in the normal sense and mark when inverted; a rest is never asserted.
#[cfg(feature = "serial")]
pub(crate) fn asserted(level: Level, inverted: bool) -> bool {
    match level {
        Level::Mark => inverted,
        Level::Space => !inverted,
        Level::Rest => false,
    }
}

/// A transmission's precomputed line-edge schedule: `events` are
/// `(offset_ms_from_start, mark_level)` transitions from the idle-mark line
/// (consecutive equal levels are merged), `total_ms` is when the final stop bit ends.
#[derive(Debug, Clone)]
pub struct FskSchedule {
    pub events: Vec<(f64, bool)>,
    pub total_ms: f64,
}

/// Compute the keying schedule for a framed character stream (`data_bits` = groups of
/// 5 data bits per character, see [`baudot_frame`]) at `baud` (45.45 for standard
/// RTTY, 75 optional). Timestamps are exact cumulative f64 milliseconds — one bit at
/// 45.45 baud is 22.0022 ms and is NEVER integerized to 22 ms (that shortcut drifts
/// 1.65 ms by the 100th character and keeps growing). This is the pure, testable core;
/// the hardware thread only sleeps to these deadlines and toggles the line.
pub fn fsk_schedule(data_bits: &[bool], baud: f64) -> FskSchedule {
    debug_assert!(baud > 0.0);
    let ms_per_bit = 1000.0 / baud;
    let mut events = Vec::new();
    let mut t_bits = 0.0f64;
    let mut level = true; // the line idles in mark
    for (mark, width) in baudot_frame(data_bits) {
        if mark != level {
            events.push((t_bits * ms_per_bit, mark));
            level = mark;
        }
        t_bits += width;
    }
    FskSchedule {
        events,
        total_ms: t_bits * ms_per_bit,
    }
}

#[cfg(feature = "serial")]
pub use imp::FskKeyer;

#[cfg(feature = "serial")]
mod imp {
    use super::{asserted, fsk_schedule, KeyLine, Level};
    use crate::control_line::{ControlLinePins, SharedControlLines};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    /// Put `line` at `level` in the sense `inverted` picks (see the module doc on line sense).
    fn set_level(sp: &mut SharedControlLines, line: KeyLine, level: Level, inverted: bool) {
        let on = asserted(level, inverted);
        let _ = match line {
            KeyLine::Dtr => sp.set_dtr(on),
            KeyLine::Rts => sp.set_rts(on),
        };
    }

    /// The longest [`FskKeyer::mark_before_key`] holds up a key-up for the thread's answer.
    const MARK_WAIT_MS: u64 = 50;

    /// What the keying thread is handed.
    enum Cmd {
        /// A framed character stream at a baud, after a lead-in of mark (ms), under the stop
        /// count it was handed over in.
        Send(Vec<bool>, f64, f64, u64),
        /// The rig is unkeyed: release the line ([`Level::Rest`]).
        Rest,
        /// A key-up is next: put the line at mark now, and say so on the sender.
        Mark(mpsc::Sender<()>),
    }

    /// The keying thread: block for a character batch, walk its edge schedule against
    /// absolute deadlines (so per-sleep jitter can't accumulate into baud drift),
    /// return to mark on abort (checked every ~8 ms so Stop TX is snappy), and exit
    /// when the channel closes.
    ///
    /// ⛔ A STOP IS A GENERATION, NOT A FLAG — the rule [`crate::serial_keyer`] documents.
    /// Every batch carries the stop count it was handed over under, and a stop bumps the
    /// count, so a batch handed over before a stop never keys after it, wherever the thread
    /// was when the stop landed. A flag the thread cleared as it took each batch lost every
    /// stop that landed between a batch's hand-over and its read: the batch then keyed in
    /// full after Stop TX.
    fn keyer_loop(mut set: impl FnMut(Level), rx: mpsc::Receiver<Cmd>, stops: Arc<AtomicU64>) {
        set(Level::Rest); // nothing keyed yet
        while let Ok(cmd) = rx.recv() {
            let (bits, baud, lead_ms, stop) = match cmd {
                Cmd::Send(bits, baud, lead_ms, stop) => (bits, baud, lead_ms, stop),
                Cmd::Rest => {
                    set(Level::Rest);
                    continue;
                }
                Cmd::Mark(done) => {
                    set(Level::Mark);
                    let _ = done.send(());
                    continue;
                }
            };
            if stops.load(Ordering::SeqCst) != stop {
                continue; // handed over before a stop: dropped, never keyed
            }
            let sched = fsk_schedule(&bits, baud);
            let start = Instant::now();
            // The over's idle, held through the lead-in (a no-op unless the line rested).
            set(Level::Mark);
            let mut aborted = false;
            // Sleep to `at_ms` (absolute, from `start`) in ≤8 ms slices; true = aborted.
            let wait_until = |set: &mut dyn FnMut(Level), at_ms: f64| -> bool {
                loop {
                    if stops.load(Ordering::SeqCst) != stop {
                        set(Level::Mark); // abort: back to mark (PTT may still be up)
                        return true;
                    }
                    let now_ms = start.elapsed().as_secs_f64() * 1000.0;
                    if now_ms >= at_ms {
                        return false;
                    }
                    let slice_ms = (at_ms - now_ms).min(8.0);
                    thread::sleep(Duration::from_secs_f64(slice_ms / 1000.0));
                }
            };
            for &(at_ms, mark) in &sched.events {
                if wait_until(&mut set, lead_ms + at_ms) {
                    aborted = true;
                    break;
                }
                set(if mark { Level::Mark } else { Level::Space });
            }
            if !aborted {
                // Hold the final level through the end of the last stop bit.
                wait_until(&mut set, lead_ms + sched.total_ms);
            }
            set(Level::Mark); // between sends: mark (the RTTY idle), until the rig unkeys
        }
        set(Level::Rest); // channel closed (keyer dropped) → released + exit
    }

    /// An open FSK keyline with its own keying thread.
    pub struct FskKeyer {
        tx: Option<mpsc::Sender<Cmd>>,
        /// How many stops there have been ([`keyer_loop`]).
        stops: Arc<AtomicU64>,
        /// Whether the line was last told to rest (nothing sent since), so [`Self::rest`],
        /// called on every unkeyed loop tick, hands the thread one message and not one a tick.
        rested: AtomicBool,
        /// Reverse: mark asserted ([`asserted`]).
        inverted: bool,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl FskKeyer {
        /// Open `port` and spawn the keying thread. The port's baud rate carries no
        /// meaning — only a control line is toggled, no data bytes are sent — so
        /// [`crate::control_line`] picks a rate the port will actually accept (the
        /// keyed BAUD is the RTTY baud, passed to `send`, and is unrelated). Both lines
        /// come back deasserted from the opener — the stuck-PTT rule, see
        /// [`crate::control_line::idle_both_lines`]. Deasserted is also the correct FSK
        /// idle: mark — and, for an `inverted` (Reverse) keyer, the released rest (see the
        /// module doc's two idles).
        ///
        /// The port is HELD, not owned ([`crate::control_line::SharedControlLines`], #492):
        /// serial PTT on the other line of the same port keys through the same handle.
        pub fn open(port: &str, line: KeyLine, inverted: bool) -> std::io::Result<Self> {
            let mut sp = crate::control_line::SharedControlLines::open(port)?;
            let mut keyer = Self::keying(move |level| set_level(&mut sp, line, level, inverted));
            keyer.inverted = inverted;
            Ok(keyer)
        }

        /// Spawn the keying thread over `set`, which puts the line at a [`Level`].
        pub(crate) fn keying(set: impl FnMut(Level) + Send + 'static) -> Self {
            let (tx, rx) = mpsc::channel();
            let stops = Arc::new(AtomicU64::new(0));
            let stops_thread = stops.clone();
            let handle = thread::spawn(move || keyer_loop(set, rx, stops_thread));
            Self {
                tx: Some(tx),
                stops,
                rested: AtomicBool::new(true),
                inverted: false,
                handle: Some(handle),
            }
        }

        /// Whether this keyer keys the inverted sense (Reverse) — the loop reopens it when
        /// the operator's Reverse no longer matches.
        pub fn inverted(&self) -> bool {
            self.inverted
        }

        /// Queue a framed character stream (groups of 5 data bits) to key at `baud`, after
        /// `lead_ms` of mark ([`super::LEAD_IN_MS`] on the batch that follows a key-up, 0 for
        /// a batch that continues an over) — non-blocking, the thread does the timing.
        pub fn send(&self, data_bits: Vec<bool>, baud: f64, lead_ms: f64) {
            if let Some(tx) = &self.tx {
                self.rested.store(false, Ordering::SeqCst);
                let _ = tx.send(Cmd::Send(
                    data_bits,
                    baud,
                    lead_ms,
                    self.stops.load(Ordering::SeqCst),
                ));
            }
        }

        /// A key-up is next: an INVERTED line goes from its released rest to mark BEFORE PTT, so
        /// the rig never keys up on a space (the CAT round trip of the key can be tens of ms).
        /// Waits for the thread, at most [`MARK_WAIT_MS`]: an idle thread answers at once, and
        /// one still keying a batch is at mark between its bits anyway. In the normal sense
        /// mark is the released rest, so there is nothing to do.
        pub fn mark_before_key(&self) {
            if !self.inverted {
                return;
            }
            if let Some(tx) = &self.tx {
                let (done, marked) = mpsc::channel();
                self.rested.store(false, Ordering::SeqCst); // the next unkeyed tick rests it
                if tx.send(Cmd::Mark(done)).is_ok() {
                    let _ = marked.recv_timeout(Duration::from_millis(MARK_WAIT_MS));
                }
            }
        }

        /// The rig is unkeyed: release the line once whatever is queued ahead has keyed out.
        /// Cheap to call on every tick — only the first call after a send reaches the thread.
        pub fn rest(&self) {
            if let Some(tx) = &self.tx {
                if !self.rested.swap(true, Ordering::SeqCst) {
                    let _ = tx.send(Cmd::Rest);
                }
            }
        }

        /// Abort NOW: back to mark and drop any queued characters (Stop TX).
        pub fn clear(&self) {
            self.stops.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl Drop for FskKeyer {
        fn drop(&mut self) {
            self.stops.fetch_add(1, Ordering::SeqCst); // interrupt a batch in progress
            self.tx = None; // close the channel → the thread returns to mark and exits
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    /// A stop ends the keying wherever it lands. The line is a recorder, so these run without a
    /// serial port; the thread, the channel and the stop are the real ones.
    #[cfg(test)]
    mod stop_tests {
        use super::{FskKeyer, Level};
        use std::sync::{mpsc, Arc, Mutex};
        use std::time::{Duration, Instant};

        /// A keyer whose line records each level it is put at. Its thread is held at its first
        /// (idle) write until `go` is sent, so a test can hand a batch over and stop before the
        /// thread takes it.
        fn held_keyer() -> (FskKeyer, Arc<Mutex<Vec<Level>>>, mpsc::Sender<()>) {
            let levels = Arc::new(Mutex::new(Vec::new()));
            let seen = Arc::clone(&levels);
            let (go, wait) = mpsc::channel::<()>();
            let mut held = true;
            let keyer = FskKeyer::keying(move |level| {
                if std::mem::take(&mut held) {
                    let _ = wait.recv();
                }
                seen.lock().unwrap().push(level);
            });
            (keyer, levels, go)
        }

        fn spaces(levels: &Mutex<Vec<Level>>) -> usize {
            levels
                .lock()
                .unwrap()
                .iter()
                .filter(|l| **l == Level::Space)
                .count()
        }

        /// Wait until the line has been still for 300 ms (at most 5 s).
        fn settle(levels: &Mutex<Vec<Level>>) {
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

        /// A message is in the keyer's hands before the thread takes it, and the operator stops:
        /// none of it may key afterwards. It used to: the thread cleared the stop as it took each
        /// batch, so a batch handed over before the stop keyed in full.
        #[test]
        fn a_batch_handed_over_before_a_stop_is_never_keyed_after_it() {
            let (keyer, levels, go) = held_keyer();
            // RYRY…: every character has several space runs.
            keyer.send([false, true, false, true, false].repeat(4), 45.45, 0.0);
            keyer.clear();
            go.send(()).unwrap();
            settle(&levels);
            // POSITIVE CONTROL: one LTRS sent after the stop keys (one space run, the start
            // bit), so the line is really read.
            keyer.send(vec![true; 5], 45.45, 0.0);
            settle(&levels);
            assert_eq!(
                spaces(&levels),
                1,
                "only the LTRS sent after the stop may key; the line went {:?}",
                levels.lock().unwrap()
            );
            assert_eq!(
                levels.lock().unwrap().last(),
                Some(&Level::Mark),
                "the line ends at mark"
            );
            // A rest releases the line once the rig is unkeyed, and only the first of a run of
            // rests (the loop calls it every unkeyed tick) reaches the thread.
            let n = levels.lock().unwrap().len();
            keyer.rest();
            keyer.rest();
            settle(&levels);
            assert_eq!(levels.lock().unwrap()[n..], [Level::Rest]);
        }

        /// A stop in the middle of a message returns the line to mark within one 8 ms slice, and
        /// the rest of the message and the one queued behind it never key.
        #[test]
        fn a_stop_mid_message_returns_to_mark_at_once_and_drops_the_rest() {
            let (keyer, levels, go) = held_keyer();
            keyer.send([false, true, false, true, false].repeat(30), 45.45, 0.0); // about 5 s
            keyer.send([false, true, false, true, false].repeat(4), 45.45, 0.0);
            go.send(()).unwrap();
            let start = Instant::now();
            while spaces(&levels) < 2 && start.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(spaces(&levels) >= 2, "control: the message must be keying");
            keyer.clear();
            let stopped = Instant::now();
            while levels.lock().unwrap().last() != Some(&Level::Mark)
                && stopped.elapsed() < Duration::from_millis(500)
            {
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(
                levels.lock().unwrap().last(),
                Some(&Level::Mark),
                "back to mark"
            );
            let n = spaces(&levels);
            settle(&levels);
            assert_eq!(spaces(&levels), n, "nothing keys after the stop");
            assert!(n < 40, "the message was cut, not finished: {n} space runs");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rtty_afsk::BAUD_45;

    // Spec values: 1000/45.45 = 22.0022 ms/bit, 7.5 bits/char = 165.0165 ms.
    const BIT_MS: f64 = 22.0022;
    const CHAR_MS: f64 = 165.0165;

    /// Reverse inverts mark and space on the line; a rest is released in either sense.
    #[cfg(feature = "serial")]
    #[test]
    fn reverse_inverts_mark_and_space_and_a_rest_is_always_released() {
        assert!(!asserted(Level::Mark, false) && asserted(Level::Space, false));
        assert!(asserted(Level::Mark, true) && !asserted(Level::Space, true));
        assert!(!asserted(Level::Rest, false) && !asserted(Level::Rest, true));
    }

    #[test]
    fn schedule_single_char_start_bit_edges() {
        // LTRS (all data bits mark): only two edges — down to space at t=0 (start
        // bit), back to mark one bit later (data + stop merge into one mark run).
        let s = fsk_schedule(&[true; 5], BAUD_45);
        assert_eq!(s.events.len(), 2);
        assert!(!s.events[0].1);
        assert!(s.events[0].0.abs() < 1e-9);
        assert!(s.events[1].1);
        assert!((s.events[1].0 - BIT_MS).abs() < 1e-3);
        assert!((s.total_ms - CHAR_MS).abs() < 1e-3);
    }

    #[test]
    fn schedule_stop_bit_is_one_and_a_half_bits() {
        // All-space data: one space run of 6 bits (start + 5 data), then the mark
        // stop — which must last 1.5 bits = 33.0033 ms.
        let s = fsk_schedule(&[false; 5], BAUD_45);
        assert_eq!(s.events.len(), 2);
        assert!((s.events[1].0 - 6.0 * BIT_MS).abs() < 1e-2);
        assert!((s.total_ms - s.events[1].0 - 33.0033).abs() < 1e-2);
    }

    #[test]
    fn schedule_cumulative_error_under_half_ms_over_100_chars() {
        // The never-integerize regression test: rounding the bit to 22.0 ms would put
        // the 100th character 1.65 ms early — every char boundary must stay within
        // 0.5 ms of k × 165.0165.
        let mut bits = Vec::new();
        for _ in 0..100 {
            bits.extend_from_slice(&[true; 5]);
        }
        let s = fsk_schedule(&bits, BAUD_45);
        assert_eq!(s.events.len(), 200);
        for k in 0..100 {
            let char_start = k as f64 * CHAR_MS;
            assert!(
                (s.events[2 * k].0 - char_start).abs() < 0.5,
                "char {k} start"
            );
            assert!(
                (s.events[2 * k + 1].0 - (char_start + BIT_MS)).abs() < 0.5,
                "char {k} data edge"
            );
        }
        assert!((s.total_ms - 100.0 * CHAR_MS).abs() < 0.5);
    }

    #[test]
    fn schedule_at_50_baud_keys_20_ms_bits_and_150_ms_characters() {
        // The true-FSK keyline at 50 Bd: a 20 ms bit, the 1.5-bit stop 30 ms, a character
        // 150 ms — and the 100th character exactly on 100 × 150 ms.
        let s = fsk_schedule(&[false; 5], 50.0);
        assert!(
            (s.events[1].0 - 120.0).abs() < 1e-9,
            "start + 5 data = 6 × 20 ms"
        );
        assert!((s.total_ms - 150.0).abs() < 1e-9);
        let s = fsk_schedule(&[true; 500], 50.0);
        assert!((s.total_ms - 100.0 * 150.0).abs() < 1e-6);
    }

    #[test]
    fn schedule_empty_stream_is_silent() {
        let s = fsk_schedule(&[], BAUD_45);
        assert!(s.events.is_empty());
        assert_eq!(s.total_ms, 0.0);
    }
}
