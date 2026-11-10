//! The amplifier TX delay on the slot overs (`slot::play_keyed_over`, `js8_slot::play_frame`): no
//! non-zero sample is handed to the output before the key plus the delay, whatever the tier, the
//! delay and how late the key came; a delay of 0 changes not one sample or deadline; FT8 and JS8 keep
//! their tones on the slot clock and blank, every other tier moves later by what its own lead-in
//! does not cover; FT4 keeps WSJT-X's own 20 ms. Each over is the tier's real wave.

use super::*;
use crate::backend::MockBackend;
use tempo_app::dto::Tier;

/// An engine on `tier` with an over to send and the amplifier TX delay at `amp_ms`.
fn engine_on(tier: Tier, amp_ms: u32) -> Engine {
    let mut eng = Engine::new("N0CALL", "EN37", 0);
    let mut s = eng.settings().clone();
    s.amp_tx_delay_ms = amp_ms;
    eng.apply_settings(s);
    eng.set_tier(tier);
    let _ = eng.take_slot_tx_abort();
    eng.set_tx_enabled(true);
    eng.broadcast("CQ TEST N0CALL EN37");
    eng
}

/// `tier`'s real over, as the engine builds it, built once per tier for the whole binary: most
/// tiers' builds take the process-wide modem lock, which other tests here time themselves against,
/// so these tests take it as little as they can.
fn over_on(tier: Tier) -> Vec<Vec<f32>> {
    static BUILT: std::sync::Mutex<Vec<(Tier, Vec<Vec<f32>>)>> = std::sync::Mutex::new(Vec::new());
    let mut built = BUILT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, waves)) = built.iter().find(|(t, _)| *t == tier) {
        return waves.clone();
    }
    let waves = engine_on(tier, 0).poll_tx(0);
    assert!(
        waves.iter().flatten().any(|&x| x != 0.0),
        "premise: {tier:?} builds an over"
    );
    built.push((tier, waves.clone()));
    waves
}

/// `ms` of 12 kHz audio in samples, worked out here rather than taken from the code under test.
fn samples(ms: f64) -> usize {
    (ms * 12.0).round() as usize
}

fn first_tone(played: &[f32]) -> Option<usize> {
    played.iter().position(|&x| x != 0.0)
}

/// What one over hands the output and when it ends.
#[derive(Debug, PartialEq)]
struct Played {
    samples: Vec<f32>,
    unit_ends: Vec<usize>,
    tx_until_ms: f64,
    audio_until_ms: Option<f64>,
}

fn played(backend: MockBackend, over: KeyedOver) -> Played {
    Played {
        samples: backend.played,
        unit_ends: backend.unit_ends,
        tx_until_ms: over.tx_until_ms,
        audio_until_ms: over.audio_until_ms,
    }
}

fn keyed_over(eng: &Engine, waves: &[Vec<f32>], keyed: f64, amp_ms: f64) -> Played {
    let mut backend = MockBackend::new();
    let over = play_keyed_over(eng, &mut backend, waves, keyed, keyed, amp_ms);
    played(backend, over)
}

/// `play_keyed_over` as it was before the amplifier TX delay, line for line: the reference a delay
/// of 0 must reproduce exactly.
fn before_play_keyed_over(
    eng: &Engine,
    backend: &mut impl AudioBackend,
    waves: &[Vec<f32>],
    keyed_ms: f64,
    now_ms: f64,
) -> KeyedOver {
    let period_ms = eng.active_slot_secs() * 1000.0;
    let Some(window_ms) = wsjtx_tx_window_ms(eng.tier()) else {
        let mut secs = 0.0f32;
        for w in waves {
            secs += w.len() as f32 / tempo_fast::SAMPLE_RATE;
            backend.play(w);
        }
        backend.mark_unit_end();
        return KeyedOver {
            tx_until_ms: tx_deadline_ms(keyed_ms, secs as f64 * 1000.0, period_ms),
            audio_until_ms: None,
        };
    };
    let at_ms = slot_clock_at(keyed_ms, now_ms, period_ms);
    let mut skip = slot_clock_skip(at_ms, period_ms);
    let end = wsjtx_over_end(at_ms, window_ms, period_ms);
    let on_the_window = KeyedOver {
        tx_until_ms: end.ptt_off_ms,
        audio_until_ms: Some(end.audio_stop_ms),
    };
    if !tones_after(waves, skip) {
        return on_the_window;
    }
    for w in waves {
        let rest = &w[skip.min(w.len())..];
        skip = skip.saturating_sub(w.len());
        if !rest.is_empty() {
            backend.play(rest);
        }
    }
    backend.mark_unit_end();
    on_the_window
}

/// `js8_slot::play_frame` as it was before the amplifier TX delay, line for line.
fn before_play_frame(
    eng: &Engine,
    backend: &mut impl AudioBackend,
    waves: &[Vec<f32>],
    keyed_ms: f64,
    slot: u64,
) -> KeyedOver {
    let period_ms = eng.active_slot_secs() * 1000.0;
    let start_ms = crate::js8_slot::frame_start_ms(eng, slot);
    let (silence, mut skip) = crate::js8_slot::placement(keyed_ms, start_ms);
    if silence > 0 {
        backend.play(&vec![0.0; silence]);
    }
    let mut handed = silence;
    for w in waves {
        let rest = &w[skip.min(w.len())..];
        skip = skip.saturating_sub(w.len());
        if !rest.is_empty() {
            backend.play(rest);
            handed += rest.len();
        }
    }
    backend.mark_unit_end();
    let audio_ms = handed as f64 * 1000.0 / f64::from(tempo_fast::SAMPLE_RATE);
    KeyedOver {
        tx_until_ms: (keyed_ms + audio_ms + TX_TAIL_MS)
            .min(start_ms + period_ms)
            .max(keyed_ms),
        audio_until_ms: None,
    }
}

/// Every transmitting tier the delay applies to, with the keys (ms into the slot: the key's own
/// latency) each is swept at. The long-period tiers take fewer: their overs are long, and their
/// lead-ins (0.5 s and more) take any delay the setting allows.
fn tiers() -> Vec<(Tier, &'static [f64])> {
    const FULL: &[f64] = &[0.0, 20.0, 150.0, 300.0, 480.0, 500.0, 520.0, 800.0];
    const FEW: &[f64] = &[0.0, 300.0, 520.0];
    vec![
        (Tier::Ft8, FULL),
        (Tier::Ft2, FULL),
        (Tier::TempoFast, FULL),
        (Tier::Msk144, FEW),
        (Tier::Q65, FEW),
        (Tier::Fst4, FEW),
        (Tier::Jt65, FEW),
    ]
}

const DELAYS: std::ops::RangeInclusive<u32> = 0..=500;

/// ⭐ T1. THE AMPLIFIER'S PROMISE, AT EVERY 10 MS OF THE SETTING: from the key, nothing sounds for
/// the delay, on every tier it applies to, however late the key came.
#[test]
fn first_tone_never_before_the_key_plus_the_amp_delay() {
    for (tier, lats) in tiers() {
        let eng = engine_on(tier, 0);
        let waves = over_on(tier);
        let slot_start = 1_000.0 * eng.active_slot_secs() * 1000.0;
        for d in DELAYS.step_by(10) {
            for &lat in lats {
                let p = keyed_over(&eng, &waves, slot_start + lat, f64::from(d));
                if let Some(i) = first_tone(&p.samples) {
                    assert!(
                        i >= samples(f64::from(d)),
                        "{tier:?} keyed {lat} ms in, delay {d} ms: first tone {:.1} ms after the key",
                        i as f64 / 12.0
                    );
                }
            }
        }
    }
}

/// T1, the gate: the setting reaches every slot tier but FT4, which keeps WSJT-X's own 20 ms, and
/// never a key the loop already held (the radio was keyed before the over).
#[test]
fn the_amp_delay_skips_ft4_and_a_held_key() {
    for tier in [Tier::Ft8, Tier::Ft2, Tier::TempoFast, Tier::Js8, Tier::Q65] {
        assert_eq!(
            slot_amp_delay_ms(&engine_on(tier, 300), false),
            300.0,
            "{tier:?}"
        );
        assert_eq!(
            slot_amp_delay_ms(&engine_on(tier, 300), true),
            0.0,
            "{tier:?} held"
        );
    }
    assert_eq!(
        slot_amp_delay_ms(&engine_on(Tier::Ft4, 300), false),
        0.0,
        "FT4"
    );
    assert_eq!(
        slot_amp_delay_ms(&engine_on(Tier::Ft8, 0), false),
        0.0,
        "off"
    );
}

/// ⭐ T2. A DELAY OF 0 IS THE BUILD BEFORE IT, SAMPLE FOR SAMPLE: every tier, on time and late
/// keys, the same audio, the same unit marks, the same PTT and audio ends.
#[test]
fn a_delay_of_zero_changes_nothing() {
    let mut cases = tiers();
    cases.push((Tier::Ft4, &[0.0, 300.0, 520.0, 5_600.0]));
    cases.push((Tier::Ft8, &[11_000.0, 13_200.0]));
    for (tier, lats) in cases {
        let eng = engine_on(tier, 0);
        let waves = over_on(tier);
        let slot_start = 1_000.0 * eng.active_slot_secs() * 1000.0;
        for &lat in lats {
            let keyed = slot_start + lat;
            let mut backend = MockBackend::new();
            let before = before_play_keyed_over(&eng, &mut backend, &waves, keyed, keyed);
            assert_eq!(
                keyed_over(&eng, &waves, keyed, 0.0),
                played(backend, before),
                "{tier:?} keyed {lat} ms in"
            );
        }
    }
    // JS8: a frame keyed in its TX delay, on its period's start and late.
    let eng = engine_on(Tier::Js8, 0);
    let slot = 1_000_000;
    let start = crate::js8_slot::frame_start_ms(&eng, slot);
    let frame = js8_stand_in();
    for key in [-200.0, 0.0, 300.0, 900.0] {
        let mut a = MockBackend::new();
        let now = crate::js8_slot::play_frame(
            &eng,
            &mut a,
            std::slice::from_ref(&frame),
            start + key,
            slot,
            0.0,
        );
        let mut b = MockBackend::new();
        let before = before_play_frame(
            &eng,
            &mut b,
            std::slice::from_ref(&frame),
            start + key,
            slot,
        );
        assert_eq!(
            played(a, now),
            played(b, before),
            "JS8 keyed {key} ms from its start"
        );
    }
}

/// A JS8 frame on the slot clock: the speed's 0.5 s start delay of silence, then tones.
fn js8_stand_in() -> Vec<f32> {
    let mut w = vec![0.0f32; 6_000];
    w.extend((0..12_000).map(|i| (i as f32 * 0.1).sin() * 0.5 + 0.01));
    w
}

/// ⭐ T3. FT8 AND JS8 KEEP THE SLOT CLOCK: with any delay the over is the one a delay of 0 plays,
/// with what falls inside the delay blanked, never moved; the window's end does not move; and a
/// delay the lead-in already covers changes nothing at all.
#[test]
fn ft8_and_js8_blank_inside_the_delay_and_keep_the_slot_clock() {
    let eng = engine_on(Tier::Ft8, 0);
    let waves = over_on(Tier::Ft8);
    let slot_start = 1_000.0 * FT8_PERIOD_MS_FOR_TESTS;
    for d in DELAYS.step_by(10) {
        for lat in [
            0.0, 20.0, 150.0, 300.0, 480.0, 500.0, 520.0, 800.0, 11_000.0,
        ] {
            let keyed = slot_start + lat;
            let off = keyed_over(&eng, &waves, keyed, 0.0);
            let on = keyed_over(&eng, &waves, keyed, f64::from(d));
            let n = samples(f64::from(d));
            let blanked: Vec<f32> = off
                .samples
                .iter()
                .enumerate()
                .map(|(i, &x)| if i < n { 0.0 } else { x })
                .collect();
            assert_eq!(on.samples, blanked, "FT8 keyed {lat} ms in, delay {d} ms");
            assert_eq!(
                (on.tx_until_ms, on.audio_until_ms, on.unit_ends.clone()),
                (off.tx_until_ms, off.audio_until_ms, off.unit_ends.clone()),
                "FT8 keyed {lat} ms in, delay {d} ms: the window moved"
            );
            if lat + f64::from(d) <= 500.0 {
                assert_eq!(on, off, "FT8: the lead-in covers {lat} + {d} ms");
            }
        }
    }
    // JS8, keyed in its TX delay (200 ms before the period): the TX delay's silence and the
    // speed's start delay count toward the amplifier's.
    let eng = engine_on(Tier::Js8, 0);
    let slot = 1_000_000;
    let start = crate::js8_slot::frame_start_ms(&eng, slot);
    let frame = js8_stand_in();
    for d in DELAYS.step_by(10) {
        for key in [-200.0, 0.0, 300.0] {
            let play = |amp: f64| {
                let mut b = MockBackend::new();
                let o = crate::js8_slot::play_frame(
                    &eng,
                    &mut b,
                    std::slice::from_ref(&frame),
                    start + key,
                    slot,
                    amp,
                );
                played(b, o)
            };
            let (off, on) = (play(0.0), play(f64::from(d)));
            let n = samples(f64::from(d));
            assert!(
                first_tone(&on.samples).is_none_or(|i| i >= n),
                "JS8 keyed {key} ms from its start, delay {d} ms"
            );
            let blanked: Vec<f32> = off
                .samples
                .iter()
                .enumerate()
                .map(|(i, &x)| if i < n { 0.0 } else { x })
                .collect();
            assert_eq!(on.samples, blanked, "JS8 keyed {key} ms, delay {d} ms");
            assert_eq!(on.tx_until_ms, off.tx_until_ms, "JS8: the hold moved");
        }
    }
}

const FT8_PERIOD_MS_FOR_TESTS: f64 = 15_000.0;

/// ⭐ T4. A TIER PLAYED WHOLE FROM ITS KEY MOVES LATER BY WHAT ITS OWN LEAD-IN DOES NOT COVER, and
/// is held that much longer, never past its slot: FT2 (no lead-in) by the whole delay, FT1 by what
/// is past its 0.3 s lead-in, the long lead-ins not at all.
#[test]
fn a_tier_off_the_slot_clock_moves_later_by_its_shortfall() {
    for (tier, lats) in tiers().into_iter().filter(|(t, _)| *t != Tier::Ft8) {
        let eng = engine_on(tier, 0);
        let waves = over_on(tier);
        let lead = first_tone(&waves.concat()).unwrap();
        let period_ms = eng.active_slot_secs() * 1000.0;
        let slot_start = 1_000.0 * period_ms;
        for d in DELAYS.step_by(10) {
            let shortfall = samples(f64::from(d)).saturating_sub(lead);
            for &lat in lats {
                let keyed = slot_start + lat;
                let off = keyed_over(&eng, &waves, keyed, 0.0);
                let on = keyed_over(&eng, &waves, keyed, f64::from(d));
                let mut moved = vec![0.0f32; shortfall];
                moved.extend_from_slice(&off.samples);
                assert_eq!(
                    on.samples, moved,
                    "{tier:?} keyed {lat} ms in, delay {d} ms"
                );
                let audio_ms = moved.len() as f64 / 12.0;
                let want = tx_deadline_ms(keyed, audio_ms, period_ms);
                assert!(
                    (on.tx_until_ms - want).abs() < 0.01,
                    "{tier:?} keyed {lat} ms in, delay {d} ms: PTT at {}, not {want}",
                    on.tx_until_ms
                );
                assert!(
                    on.tx_until_ms <= slot_start + period_ms,
                    "{tier:?}: past its slot"
                );
            }
        }
    }
    // The tiers' own lead-ins, which decide what each takes.
    let lead = |t| first_tone(&over_on(t).concat()).unwrap();
    assert!(
        lead(Tier::Ft2) < 12,
        "FT2 starts at its key (a ramp's first sample is 0)"
    );
    assert!(
        (samples(300.0)..samples(300.0) + 12).contains(&lead(Tier::TempoFast)),
        "FT1 starts 0.3 s after its key"
    );
    for t in [Tier::Q65, Tier::Fst4, Tier::Jt65] {
        assert!(lead(t) >= samples(500.0), "{t:?} covers any delay");
    }
}

/// T4, FT2's budget: keyed up to 480 ms late with the longest delay, the over still keeps its whole
/// tones and its 250 ms tail inside its 3.75 s slot; past that the tail is what gives first.
#[test]
fn ft2_keeps_its_whole_tail_with_the_longest_delay_and_a_slow_key() {
    let eng = engine_on(Tier::Ft2, 0);
    let waves = over_on(Tier::Ft2);
    let slot_start = 1_000.0 * 3_750.0;
    for lat in [0.0, 100.0, 300.0, 480.0] {
        let keyed = slot_start + lat;
        let p = keyed_over(&eng, &waves, keyed, 500.0);
        assert!(
            (p.tx_until_ms - (keyed + 500.0 + 2_520.0 + TX_TAIL_MS)).abs() < 0.5,
            "FT2 keyed {lat} ms in: PTT at {:.1} ms after the key",
            p.tx_until_ms - keyed
        );
    }
    let keyed = slot_start + 600.0;
    let p = keyed_over(&eng, &waves, keyed, 500.0);
    assert_eq!(
        p.tx_until_ms,
        slot_start + 3_750.0,
        "clamped at the boundary"
    );
    assert!(
        p.tx_until_ms >= keyed + 500.0 + 2_520.0,
        "the tones themselves still fit"
    );
}

/// T5. An immediate start of a tier off the slot clock counts the delay into the room it needs, so
/// FT2 keys only when its tones still fit after it; FT8 and FT4 go on WSJT-X's latest key alone.
#[test]
fn an_immediate_start_counts_the_delay_off_the_slot_clock() {
    assert_eq!(immediate_start_need_ms(2_520.0, false, 0.0), 2_520.0);
    assert_eq!(immediate_start_need_ms(2_520.0, false, 300.0), 2_820.0);
    assert_eq!(immediate_start_need_ms(13_140.0, true, 300.0), 13_140.0);
    // 3 s left in an FT2 slot: fits with no delay and with 480 ms, not with 500.
    let room_ms = 3_000.0;
    assert!(immediate_start_need_ms(2_520.0, false, 480.0) <= room_ms);
    assert!(immediate_start_need_ms(2_520.0, false, 500.0) > room_ms);
}

/// One cell of the timing sweep: an over keyed `lat` ms into its slot with the delay at `d` ms, as
/// the other station's receiver hears it.
struct Cell {
    /// From the key to the first tone, ms.
    gap_ms: f64,
    /// Where the first tone falls in the slot, ms.
    tone_in_slot_ms: f64,
    /// Tone audio (non-zero samples) the delay took away against a delay of 0, ms.
    lost_ms: f64,
    /// PTT past the last tone, ms (the tail), and whether PTT ran past the slot.
    tail_ms: f64,
    overrun: bool,
    /// Decoded by the tier's own decoder, and the DT it read.
    decoded: Option<f32>,
}

/// The slot as the other station's receiver hears it: `queued` laid from the key, `lat` ms in.
fn heard(frame_len: usize, queued: &[f32], lat: f64) -> Vec<i16> {
    let mut frame = vec![0i16; frame_len];
    let at = samples(lat);
    for (s, &x) in frame.iter_mut().skip(at).zip(queued) {
        *s = (x * 1_000.0) as i16;
    }
    frame
}

fn decode(kind: tempo_core::modes::ModeKind, frame: &[i16]) -> Option<f32> {
    let mode = tempo_core::modes::make_mode(kind);
    mode.decode_frame(
        frame, 200, 3_000, 3, "", "", 0, 1_500, 0, false, false, false,
    )
    .into_iter()
    .find(|d| d.message.contains("N0CALL"))
    .map(|d| d.dt)
}

/// `decode`: run the decoder on this cell (it is most of the sweep's time in a debug build).
fn sweep_cell(eng: &Engine, waves: &[Vec<f32>], lat: f64, d: f64, decode_it: bool) -> Cell {
    let kind = match eng.tier() {
        Tier::Ft8 => tempo_core::modes::ModeKind::Ft8,
        _ => tempo_core::modes::ModeKind::Ft2,
    };
    let period_ms = eng.active_slot_secs() * 1000.0;
    let slot_start = 1_000.0 * period_ms;
    let keyed = slot_start + lat;
    let on = keyed_over(eng, waves, keyed, d);
    let off = keyed_over(eng, waves, keyed, 0.0);
    let tones = |p: &Played| p.samples.iter().filter(|&&x| x != 0.0).count();
    let first = first_tone(&on.samples);
    let last = on.samples.iter().rposition(|&x| x != 0.0);
    let frame_len = tempo_core::modes::make_mode(kind).frame_samples();
    Cell {
        gap_ms: first.map_or(f64::NAN, |i| i as f64 / 12.0),
        tone_in_slot_ms: first.map_or(f64::NAN, |i| lat + i as f64 / 12.0),
        lost_ms: (tones(&off) - tones(&on)) as f64 / 12.0,
        tail_ms: last.map_or(f64::NAN, |i| {
            on.tx_until_ms - (keyed + (i + 1) as f64 / 12.0)
        }),
        overrun: on.tx_until_ms > slot_start + period_ms,
        decoded: decode_it
            .then(|| decode(kind, &heard(frame_len, &on.samples, lat)))
            .flatten(),
    }
}

/// ⭐ THE TIMING SWEEP, WITH A DECODE POSITIVE CONTROL. FT8 and FT2, every delay the operator is
/// likely to set against keys from on time to 700 ms late: from the key to the first tone, where the
/// tone falls in the slot, tones lost, PTT's tail, any overrun, and whether the tier's own decoder
/// (FT8: the vendored WSJT-X decoder; FT2: Decodium's) still decodes the over as the other station
/// hears it (in the run that decodes). The signal is noise-free, so a
/// decode here says the timing is right, not how much margin a weak signal keeps. Printed as a table
/// (`--nocapture`), asserted:
/// - the gap is never under the delay, and nothing ever runs past the slot;
/// - FT8: the first tone stays where a delay of 0 puts it until the blanking reaches the tones, and
///   every cell decodes at DT within 0.2 s of the 0-delay cell, but for the late keys whose
///   blanking eats into the first Costas array, where DT holds and only the decode may go;
/// - FT2: the first tone moves exactly by the delay, the full 250 ms tail survives while key and
///   delay together stay under 980 ms, and every cell decodes, its DT moved by the delay.
///
/// POSITIVE CONTROL: the 0-delay over decodes (the decoder sees what the sweep feeds it), and
/// NEGATIVE CONTROL: half of it does not (the check can fail).
///
/// `decode_at`: the delays each key is decoded at (none: the timing alone, at every delay).
fn sweep(decode_at: &[f64]) {
    const DELAYS: [f64; 8] = [0.0, 20.0, 50.0, 100.0, 200.0, 300.0, 400.0, 500.0];
    const LATS: [f64; 5] = [0.0, 50.0, 150.0, 300.0, 700.0];
    let decoding = !decode_at.is_empty();
    for tier in [Tier::Ft8, Tier::Ft2] {
        let eng = engine_on(tier, 0);
        let waves = over_on(tier);
        let kind = match tier {
            Tier::Ft8 => tempo_core::modes::ModeKind::Ft8,
            _ => tempo_core::modes::ModeKind::Ft2,
        };
        let frame_len = tempo_core::modes::make_mode(kind).frame_samples();
        // Positive control, then negative.
        if decoding {
            let whole = waves.concat();
            assert!(
                decode(kind, &heard(frame_len, &whole, 0.0)).is_some(),
                "{tier:?}: positive control"
            );
            let half: Vec<f32> = whole[..whole.len() / 2].to_vec();
            assert!(
                decode(kind, &heard(frame_len, &half, 0.0)).is_none(),
                "{tier:?}: negative control"
            );
        }

        println!("{tier:?}: delay_ms key_late_ms | gap_ms tone_in_slot_ms lost_ms tail_ms overrun | decoded dt");
        for lat in LATS {
            let base = sweep_cell(&eng, &waves, lat, 0.0, decoding);
            for d in DELAYS {
                let decoded = decode_at.contains(&d);
                let c = sweep_cell(&eng, &waves, lat, d, decoded);
                println!(
                    "{tier:?}: {d:>5} {lat:>5} | {:>7.1} {:>8.1} {:>6.1} {:>6.1} {} | {:?}",
                    c.gap_ms, c.tone_in_slot_ms, c.lost_ms, c.tail_ms, c.overrun, c.decoded
                );
                assert!(c.gap_ms >= d - 0.1, "{tier:?} {d}/{lat}: gap {}", c.gap_ms);
                assert!(!c.overrun, "{tier:?} {d}/{lat}: PTT past the slot");
                match tier {
                    Tier::Ft8 => {
                        if c.lost_ms == 0.0 {
                            assert_eq!(
                                c.tone_in_slot_ms, base.tone_in_slot_ms,
                                "{d}/{lat}: FT8 tone moved"
                            );
                        }
                        if lat + d <= 500.0 {
                            assert_eq!(
                                c.lost_ms, 0.0,
                                "{d}/{lat}: FT8 lost tones inside its lead-in"
                            );
                        }
                        if let (Some(dt), Some(dt0)) = (c.decoded, base.decoded) {
                            assert!(
                                (dt - dt0).abs() < 0.2,
                                "{d}/{lat}: FT8 DT moved {dt0} -> {dt}"
                            );
                        }
                        if decoded && c.lost_ms <= 160.0 {
                            assert!(
                                c.decoded.is_some(),
                                "{d}/{lat}: FT8 lost {} ms and no decode",
                                c.lost_ms
                            );
                        }
                    }
                    _ => {
                        assert!(
                            (c.tone_in_slot_ms - base.tone_in_slot_ms - d).abs() < 0.1,
                            "{d}/{lat}: FT2 tone not moved by exactly the delay"
                        );
                        assert_eq!(c.lost_ms, 0.0, "{d}/{lat}: FT2 lost tones");
                        if lat + d <= 980.0 {
                            assert!(
                                c.tail_ms >= TX_TAIL_MS - 0.1,
                                "{d}/{lat}: FT2 tail {}",
                                c.tail_ms
                            );
                        }
                        if decoded {
                            let (dt, dt0) = (
                                c.decoded
                                    .unwrap_or_else(|| panic!("{d}/{lat}: FT2 no decode")),
                                base.decoded.expect("FT2 0-delay decode"),
                            );
                            assert!(
                                (dt - dt0 - (d / 1000.0) as f32).abs() < 0.1,
                                "{d}/{lat}: FT2 DT {dt0} -> {dt}, not moved by the delay"
                            );
                        }
                    }
                }
            }
        }
    }
}

/// The timing sweep at every delay and key, without the decoder: in the suite.
#[test]
fn amp_delay_timing_sweep() {
    sweep(&[]);
}

/// The timing sweep with the decode positive control, at every delay and key. ⚠️ IGNORED IN THE
/// SUITE, RUN BY NAME: the decoders take the process-wide modem lock for most of a minute in a debug
/// build, and other tests in this binary hold that lock themselves and time what they do against
/// it, so running this beside them makes them fail at random. Run it alone:
/// `cargo test -p tempo-audio --features device,serial --lib amp_delay_timing_sweep_decodes -- --ignored --nocapture`.
#[test]
#[ignore = "takes the process-wide modem lock for most of a minute; run by name"]
fn amp_delay_timing_sweep_decodes_as_the_other_station_hears_it() {
    sweep(&[0.0, 20.0, 50.0, 100.0, 200.0, 300.0, 400.0, 500.0]);
}

/// ⚠️ FT1'S LIMIT, PINNED: its over starts its tones 0.3 s after its key and ends them about 0.16 s
/// before its 4 s slot does, so key latency and what of the delay is past the 0.3 s lead-in come out
/// of that 0.16 s. Within it every tone goes out; past it PTT drops at the boundary on the last
/// tones, as it does for a key that late with no delay. A delay up to 0.3 s costs FT1 nothing; the
/// longest cost the over's last tones even on time.
#[test]
fn ft1_keeps_its_tones_while_key_and_delay_fit_its_slack() {
    let eng = engine_on(Tier::TempoFast, 0);
    let waves = over_on(Tier::TempoFast);
    let period_ms = eng.active_slot_secs() * 1000.0;
    let all = waves.concat();
    let last_tone_ms = (all.iter().rposition(|&x| x != 0.0).unwrap() + 1) as f64 / 12.0;
    let slack_ms = period_ms - last_tone_ms;
    assert!(
        (150.0..=180.0).contains(&slack_ms),
        "FT1's slack moved: {slack_ms:.1} ms"
    );
    let slot_start = 1_000.0 * period_ms;
    for d in DELAYS.step_by(10) {
        for lat in [0.0, 50.0, 150.0] {
            let keyed = slot_start + lat;
            let p = keyed_over(&eng, &waves, keyed, f64::from(d));
            let tones_end =
                keyed + (p.samples.iter().rposition(|&x| x != 0.0).unwrap() + 1) as f64 / 12.0;
            let past_lead = (f64::from(d) - 300.0).max(0.0);
            assert_eq!(
                tones_end <= p.tx_until_ms,
                lat + past_lead <= slack_ms + 0.1,
                "FT1 keyed {lat} ms in, delay {d} ms: tones end {:.1} ms, PTT {:.1} ms",
                tones_end - slot_start,
                p.tx_until_ms - slot_start
            );
        }
    }
}

/// FT1 with no delay, keyed on its boundary, as the loop plays it: the first tone 0.3 s after the
/// key (the lead-in the decoder needs to hear a peer a little fast), the last tone inside the slot
/// where `tx_over_secs` says, and PTT dropped at the boundary.
#[test]
fn ft1_on_time_with_no_delay_sounds_from_its_lead_in_to_inside_its_slot() {
    let eng = engine_on(Tier::TempoFast, 0);
    let waves = over_on(Tier::TempoFast);
    let period_ms = eng.active_slot_secs() * 1000.0;
    let slot_start = 1_000.0 * period_ms;
    let p = keyed_over(&eng, &waves, slot_start, 0.0);
    let first = first_tone(&p.samples).expect("tones");
    assert!(
        (samples(300.0)..samples(300.0) + 12).contains(&first),
        "FT1's first tone {:.1} ms after its key, not 300 ms",
        first as f64 / 12.0
    );
    let end_ms = (p.samples.iter().rposition(|&x| x != 0.0).unwrap() + 1) as f64 / 12.0;
    assert!(
        end_ms < period_ms - 150.0,
        "FT1's tones end {end_ms:.1} ms into a {period_ms} ms slot"
    );
    assert!(
        (eng.tx_over_secs() * 1000.0 - end_ms).abs() < 10.0,
        "tx_over_secs {} but the tones end at {end_ms:.1} ms",
        eng.tx_over_secs()
    );
    assert_eq!(
        p.tx_until_ms,
        slot_start + period_ms,
        "PTT at the boundary"
    );
}

/// FT1 at the longest delay with a slow key, 500 ms with 200 ms of latency: well past its slack, so
/// the end of the over is cut, and PTT drops exactly at the slot boundary, never after it, so
/// nothing of the over goes out in the next slot.
#[test]
fn ft1_ptt_drops_at_the_slot_boundary_with_a_long_delay() {
    let eng = engine_on(Tier::TempoFast, 0);
    let waves = over_on(Tier::TempoFast);
    let period_ms = eng.active_slot_secs() * 1000.0;
    let slot_start = 1_000.0 * period_ms;
    let keyed = slot_start + 200.0;
    let p = keyed_over(&eng, &waves, keyed, 500.0);
    let tones_end = keyed + (p.samples.iter().rposition(|&x| x != 0.0).unwrap() + 1) as f64 / 12.0;
    assert!(
        tones_end > slot_start + period_ms,
        "premise: the over runs past its slot"
    );
    assert_eq!(
        p.tx_until_ms,
        slot_start + period_ms,
        "FT1's PTT not dropped at the slot boundary"
    );
}

/// FT1 shifted by the longest delay with a slow key, 500 ms with 200 ms of latency, still decodes as
/// the other station hears its slot, with the end of the over cut at the boundary. POSITIVE
/// CONTROL: the over with no delay decodes; NEGATIVE CONTROL: its first quarter alone does not. The
/// signal is noise-free. ⚠️ Ignored in the suite and run by name, as the FT8/FT2 decode sweep is:
/// FT1's decoder takes the process-wide modem lock, which other tests in this binary time
/// themselves against.
#[test]
#[ignore = "takes the process-wide modem lock; run by name"]
fn ft1_shifted_past_its_slack_still_decodes_with_its_end_cut() {
    let kind = tempo_core::modes::ModeKind::TempoFast;
    let eng = engine_on(Tier::TempoFast, 0);
    let waves = over_on(Tier::TempoFast);
    let period_ms = eng.active_slot_secs() * 1000.0;
    let slot_start = 1_000.0 * period_ms;
    let frame_len = samples(period_ms);
    let whole = waves.concat();
    assert!(
        decode(kind, &heard(frame_len, &whole, 0.0)).is_some(),
        "positive control"
    );
    let quarter: Vec<f32> = whole[..whole.len() / 4].to_vec();
    assert!(
        decode(kind, &heard(frame_len, &quarter, 0.0)).is_none(),
        "negative control: a quarter of an FT1 over decoded"
    );
    let lat = 200.0;
    let p = keyed_over(&eng, &waves, slot_start + lat, 500.0);
    // What the receiver's slot holds: the over from the key, up to the slot's end, where PTT drops.
    let sent = samples(p.tx_until_ms - (slot_start + lat)).min(p.samples.len());
    let heard_slot = heard(frame_len, &p.samples[..sent], lat);
    assert!(
        p.samples[sent..].iter().any(|&x| x != 0.0),
        "premise: the end of the over is cut"
    );
    assert!(
        decode(kind, &heard_slot).is_some(),
        "FT1 at 500 ms + 200 ms latency, its end cut, did not decode"
    );
}
