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

use crate::rtty_afsk::baudot_frame;

/// Which control line carries the FSK keying (PTT goes on the other, or via CAT).
/// Same type as the CW keyer; for FSK the common interface wiring is DTR = FSK,
/// RTS = PTT.
pub use crate::serial_keyer::KeyLine;

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
    use super::{fsk_schedule, KeyLine};
    use serialport::SerialPort;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    /// Asserted while space, deasserted while mark (see the module doc on line sense).
    fn set_space(sp: &mut dyn SerialPort, line: KeyLine, space: bool) {
        let _ = match line {
            KeyLine::Dtr => sp.write_data_terminal_ready(space),
            KeyLine::Rts => sp.write_request_to_send(space),
        };
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
    fn keyer_loop(
        mut set: impl FnMut(bool),
        rx: mpsc::Receiver<(Vec<bool>, f64, u64)>,
        stops: Arc<AtomicU64>,
    ) {
        set(false); // idle: mark
        while let Ok((bits, baud, stop)) = rx.recv() {
            if stops.load(Ordering::SeqCst) != stop {
                continue; // handed over before a stop: dropped, never keyed
            }
            let sched = fsk_schedule(&bits, baud);
            let start = Instant::now();
            let mut aborted = false;
            // Sleep to `at_ms` (absolute, from `start`) in ≤8 ms slices; true = aborted.
            let wait_until = |set: &mut dyn FnMut(bool), at_ms: f64| -> bool {
                loop {
                    if stops.load(Ordering::SeqCst) != stop {
                        set(false); // abort: back to mark
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
                if wait_until(&mut set, at_ms) {
                    aborted = true;
                    break;
                }
                set(!mark);
            }
            if !aborted {
                // Hold the final level through the end of the last stop bit.
                wait_until(&mut set, sched.total_ms);
            }
            set(false); // between sends: mark (the RTTY idle)
        }
        set(false); // channel closed (keyer dropped) → mark + exit
    }

    /// An open FSK keyline with its own keying thread.
    pub struct FskKeyer {
        tx: Option<mpsc::Sender<(Vec<bool>, f64, u64)>>,
        /// How many stops there have been ([`keyer_loop`]).
        stops: Arc<AtomicU64>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl FskKeyer {
        /// Open `port` and spawn the keying thread. The port's baud rate carries no
        /// meaning — only a control line is toggled, no data bytes are sent — so
        /// [`crate::control_line`] picks a rate the port will actually accept (the
        /// keyed BAUD is the RTTY baud, passed to `send`, and is unrelated). Both lines
        /// come back deasserted from the opener — the stuck-PTT rule, see
        /// [`crate::control_line::idle_both_lines`]. Deasserted is also the correct FSK
        /// idle: mark.
        pub fn open(port: &str, line: KeyLine) -> std::io::Result<Self> {
            let mut sp = crate::control_line::open_control_line_port(port)?;
            Ok(Self::keying(move |space| set_space(&mut *sp, line, space)))
        }

        /// Spawn the keying thread over `set`, which puts the line at space (`true`) or mark.
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

        /// Queue a framed character stream (groups of 5 data bits) to key at `baud`
        /// (non-blocking — the thread does the timing).
        pub fn send(&self, data_bits: Vec<bool>, baud: f64) {
            if let Some(tx) = &self.tx {
                let _ = tx.send((data_bits, baud, self.stops.load(Ordering::SeqCst)));
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
        use super::FskKeyer;
        use std::sync::{mpsc, Arc, Mutex};
        use std::time::{Duration, Instant};

        /// A keyer whose line records each level it is put at (`true` = space). Its thread is
        /// held at its first (idle mark) write until `go` is sent, so a test can hand a batch
        /// over and stop before the thread takes it.
        fn held_keyer() -> (FskKeyer, Arc<Mutex<Vec<bool>>>, mpsc::Sender<()>) {
            let levels = Arc::new(Mutex::new(Vec::new()));
            let seen = Arc::clone(&levels);
            let (go, wait) = mpsc::channel::<()>();
            let mut held = true;
            let keyer = FskKeyer::keying(move |space| {
                if std::mem::take(&mut held) {
                    let _ = wait.recv();
                }
                seen.lock().unwrap().push(space);
            });
            (keyer, levels, go)
        }

        fn spaces(levels: &Mutex<Vec<bool>>) -> usize {
            levels.lock().unwrap().iter().filter(|s| **s).count()
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

        /// A message is in the keyer's hands before the thread takes it, and the operator stops:
        /// none of it may key afterwards. It used to: the thread cleared the stop as it took each
        /// batch, so a batch handed over before the stop keyed in full.
        #[test]
        fn a_batch_handed_over_before_a_stop_is_never_keyed_after_it() {
            let (keyer, levels, go) = held_keyer();
            // RYRY…: every character has several space runs.
            keyer.send([false, true, false, true, false].repeat(4), 45.45);
            keyer.clear();
            go.send(()).unwrap();
            settle(&levels);
            // POSITIVE CONTROL: one LTRS sent after the stop keys (one space run, the start
            // bit), so the line is really read.
            keyer.send(vec![true; 5], 45.45);
            settle(&levels);
            assert_eq!(
                spaces(&levels),
                1,
                "only the LTRS sent after the stop may key; the line went {:?}",
                levels.lock().unwrap()
            );
            assert_eq!(
                levels.lock().unwrap().last(),
                Some(&false),
                "the line ends at mark"
            );
        }

        /// A stop in the middle of a message returns the line to mark within one 8 ms slice, and
        /// the rest of the message and the one queued behind it never key.
        #[test]
        fn a_stop_mid_message_returns_to_mark_at_once_and_drops_the_rest() {
            let (keyer, levels, go) = held_keyer();
            keyer.send([false, true, false, true, false].repeat(30), 45.45); // about 5 s
            keyer.send([false, true, false, true, false].repeat(4), 45.45);
            go.send(()).unwrap();
            let start = Instant::now();
            while spaces(&levels) < 2 && start.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(spaces(&levels) >= 2, "control: the message must be keying");
            keyer.clear();
            let stopped = Instant::now();
            while levels.lock().unwrap().last() != Some(&false)
                && stopped.elapsed() < Duration::from_millis(500)
            {
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(levels.lock().unwrap().last(), Some(&false), "back to mark");
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
