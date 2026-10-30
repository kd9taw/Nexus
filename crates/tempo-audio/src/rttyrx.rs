//! The RTTY RX decode thread — the armed-decoder-on-the-RX-path pattern (see
//! `aicw.rs`). While the operator has the RTTY cockpit armed (`rtty_arm`), the
//! engine's radio loop accumulates 12 kHz RX audio in a drain buffer; this
//! thread empties it every ~100 ms, runs the `tempo_core::rtty` demodulator
//! OFF-lock, and pushes decoded characters (+ per-char ATC confidence) and the
//! AFC state back into the engine for the `get_rtty_state` poll.
//!
//! The same audio feeds the tuning scope (`tempo_core::rtty::scope`), whose resonators sit on
//! the demod's AFC-corrected mark and space, and its newest frame goes to the engine for the
//! `get_rtty_scope` poll. While anything of ours holds the transmitter the scope is cleared and
//! the frame goes dark: the receive audio is then our own signal (or the rig's muted input),
//! and drawing it as a tuned "+" would invite a nudge that tunes nothing. While nothing is
//! drawing it (`Engine::rtty_scope_wanted`: the pane closed, another screen in front, the
//! window hidden) no frame is built at all.
//!
//! RX ONLY: nothing here keys PTT, emits TX audio, or touches `rtty_afsk`/
//! `rtty_fsk`. Disarmed = the buffer stays empty and this loop does nothing but
//! a brief flag check, so everyone else pays nothing.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempo_app::engine::{engine_lock, Engine};
use tempo_core::rtty::scope::{ScopeFrame, TuningScope};
use tempo_core::rtty::{demod::SAMPLE_RATE, tone_pair, RttyConfig, RttyDemod, RttyDemodulator};

use crate::service::SHUTDOWN;

/// Drain cadence: short enough that decoded text flows char-by-char, long
/// enough that the disarmed idle cost is negligible (one lock + bool read).
const POLL: Duration = Duration::from_millis(100);

/// The points one scope frame carries: about 85 ms at 3 kS/s, a little under one poll's worth,
/// so a frame is never mostly a repeat of the last. 256 pairs of i8 keep it near 2 KB as JSON.
const SCOPE_POINTS: usize = 256;

/// Spawn the RTTY RX decode thread (call once at startup, beside `spawn_ai_cw`).
pub fn spawn_rtty_rx(engine: Arc<Mutex<Engine>>) {
    std::thread::Builder::new()
        .name("rtty-rx".into())
        .spawn(move || run(engine))
        .expect("spawn rtty-rx");
}

fn run(engine: Arc<Mutex<Engine>>) {
    // The demodulator lives here, not in the engine: its FFT state is decode-
    // thread-private, and dropping it on disarm makes every re-arm a clean
    // acquire (fresh AFC, fresh bit clock). Tone pair/baud follow the operator's
    // RTTY settings + the netted audio center (mark/space straddle center by
    // ±shift/2; reverse swaps the pair — see `tempo_core::rtty::tone_pair`); a
    // settings change, a waterfall net, or the cockpit's AFC-reset drops +
    // rebuilds it the same clean-acquire way.
    let mut demod: Option<RttyDemodulator> = None;
    // (baud, shift, reverse, center) the demod was built with
    let mut applied = (0.0f64, 0u32, false, 0.0f32);
    // The tuning scope, built on first use and kept across demod rebuilds (it follows the tones).
    let mut scope: Option<TuningScope> = None;
    loop {
        if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(POLL);
        let (armed, cfg, reset, keyed, scope_wanted) = {
            let mut e = engine_lock(&engine);
            (
                e.rtty_armed(),
                rx_config(&e),
                e.take_rtty_afc_reset(),
                // THE ONE ARBITER every TX gate asks: any keying source the engine can see.
                e.tx_owner().is_some(),
                e.rtty_scope_wanted(std::time::Instant::now()),
            )
        };
        if !armed {
            demod = None;
            scope = None;
            continue;
        }
        if reset || cfg != applied {
            demod = None; // rebuild below: fresh AFC acquire on the new config
            applied = cfg;
        }
        let audio = engine_lock(&engine).take_rtty_audio();
        if audio.is_empty() {
            continue;
        }
        let d = demod.get_or_insert_with(|| demod_for(applied));
        // The heavy part — mixers, FFT filters, ATC, clock recovery — off-lock.
        let chars = d.feed(&audio);
        let (afc_hz, afc_locked) = (d.afc_offset_hz(), d.afc_locked());
        let cfg = d.config();
        let tones = (cfg.mark_hz + afc_hz, cfg.space_hz + afc_hz);
        let frame = scope_step(&mut scope, scope_wanted, keyed, tones, &audio);
        let mut e = engine_lock(&engine);
        e.push_rtty_decode(&chars, afc_hz, afc_locked);
        e.push_rtty_scope(frame);
    }
}

/// One drain's worth of the tuning scope: follow the AFC-corrected `tones`, run `audio` through
/// it and return the newest frame, or clear it and return `None` while `keyed` (our own
/// transmission gates it off, and it starts clean afterwards). Not `wanted` (nothing is drawing
/// it): no work at all, and the scope is dropped, so it starts clean when it is shown again.
fn scope_step(
    scope: &mut Option<TuningScope>,
    wanted: bool,
    keyed: bool,
    tones: (f32, f32),
    audio: &[f32],
) -> Option<ScopeFrame> {
    if !wanted {
        *scope = None;
        return None;
    }
    let s = scope.get_or_insert_with(|| TuningScope::new(SAMPLE_RATE, tones.0, tones.1));
    if keyed {
        s.clear();
        return None;
    }
    s.retune(tones.0, tones.1);
    s.feed(audio);
    Some(s.frame(SCOPE_POINTS))
}

/// The (baud, shift, reverse, center) the demodulator is built for, as the engine reports them.
/// `reverse` is the effective sense: the operator's Reverse XOR an AFSK rig on USB.
fn rx_config(e: &Engine) -> (f64, u32, bool, f32) {
    (
        e.rtty_baud(),
        e.rtty_shift_hz(),
        e.rtty_reverse(),
        e.rtty_center_hz(),
    )
}

/// A fresh demodulator for one [`rx_config`].
fn demod_for((baud, shift, reverse, center): (f64, u32, bool, f32)) -> RttyDemodulator {
    let (mark, space) = tone_pair(center, shift as f32, reverse);
    RttyDemodulator::new(RttyConfig {
        mark_hz: mark,
        space_hz: space,
        baud: baud as f32,
        ..RttyConfig::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rtty_afsk::{afsk_char_samples, AfskConfig};
    use tempo_core::rtty::{baudot::LTRS, code_bits, BaudotEncoder};

    const MSG: &str = "CQ CQ DE K1ABC";

    fn carrier(f: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / SAMPLE_RATE).sin() * 0.5)
            .collect()
    }

    #[test]
    fn the_scope_draws_on_receive_and_goes_dark_while_we_transmit() {
        let mut scope = None;
        let audio = carrier(2125.0, 1200);
        let rx =
            scope_step(&mut scope, true, false, (2125.0, 2295.0), &audio).expect("a frame on RX");
        assert_eq!(rx.xy.len(), 2 * SCOPE_POINTS);
        assert!(rx.peak > 0.0);

        // Keyed: no frame, and the picture is cleared rather than held.
        assert_eq!(
            scope_step(&mut scope, true, true, (2125.0, 2295.0), &audio),
            None
        );
        assert!(scope.as_ref().is_some_and(|s| s.is_empty()));

        // Back on receive it starts from the new audio only.
        let after = scope_step(&mut scope, true, false, (2125.0, 2295.0), &audio[..40]).unwrap();
        assert_eq!(after.xy.len(), 2 * 40 / tempo_core::rtty::scope::DECIMATE);
    }

    /// Nobody drawing the scope: no frame and no scope state, whatever the audio.
    #[test]
    fn the_scope_does_no_work_while_nothing_draws_it() {
        let mut scope = None;
        let audio = carrier(2125.0, 1200);
        assert_eq!(
            scope_step(&mut scope, false, false, (2125.0, 2295.0), &audio),
            None
        );
        assert!(scope.is_none(), "no scope was built");

        // Shown again: it draws at once, and dropping it while hidden starts it clean.
        assert!(scope_step(&mut scope, true, false, (2125.0, 2295.0), &audio).is_some());
        assert_eq!(
            scope_step(&mut scope, false, false, (2125.0, 2295.0), &audio),
            None
        );
        assert!(scope.is_none(), "hidden again: the scope is dropped");
    }

    #[test]
    fn the_scope_follows_the_afc_corrected_tones() {
        let mut scope = None;
        scope_step(
            &mut scope,
            true,
            false,
            (2125.0, 2295.0),
            &carrier(2125.0, 120),
        );
        scope_step(
            &mut scope,
            true,
            false,
            (2137.0, 2307.0),
            &carrier(2137.0, 120),
        );
        assert_eq!(scope.unwrap().tones(), (2137.0, 2307.0));
    }

    fn engine(side: &str) -> Engine {
        let mut e = Engine::new("K1ABC", "EN50", 0);
        let mut s = e.settings().clone();
        s.rtty_afsk_sideband = side.into();
        e.apply_settings(s);
        e.set_operating_mode("rtty", false);
        e
    }

    /// A station with the standard sense (mark HIGH on RF, 170 Hz shift) heard on a rig on USB,
    /// netted to the 2210 Hz default centre. USB puts audio = RF − dial, so the mark is the
    /// HIGHER audio tone: 2295 Hz, with space at 2125 Hz.
    fn heard_on_usb() -> Vec<f32> {
        let mut codes = vec![LTRS; 4];
        codes.extend(BaudotEncoder::new(true).encode(MSG));
        codes.extend([LTRS, LTRS]);
        let cfg = AfskConfig {
            mark_hz: 2295.0,
            space_hz: 2125.0,
            ..AfskConfig::default()
        };
        let mut audio = vec![0.0f32; 1200];
        audio.extend(afsk_char_samples(&code_bits(&codes), &cfg));
        audio.extend(vec![0.0f32; 2400]);
        audio
    }

    fn copy(e: &Engine, audio: &[f32]) -> String {
        let mut d = demod_for(rx_config(e));
        let mut text = String::new();
        for chunk in audio.chunks(479) {
            text.extend(d.feed(chunk).iter().map(|c| c.ch));
        }
        text
    }

    /// The AFSK sideband set to USB, the decoder copies a USB-sense signal. The control: the same
    /// audio through the LSB default does not copy, so the setting is what made the difference.
    #[test]
    fn the_decoder_copies_a_usb_sense_signal_when_set_to_usb() {
        let audio = heard_on_usb();
        let usb = engine("usb");
        assert_eq!(rx_config(&usb), (45.45, 170, true, 2210.0));
        let text = copy(&usb, &audio);
        assert!(text.contains(MSG), "USB copy: {text:?}");
        let lsb = engine("lsb");
        let text = copy(&lsb, &audio);
        assert!(
            !text.contains(MSG),
            "CONTROL: LSB must not copy USB sense: {text:?}"
        );
    }
}
