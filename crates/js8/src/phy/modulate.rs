//! Transmit side of `phy`: word → 79 tones → slot-positioned audio.
//!
//! Tones (genjs8.f90:66-77, JS8.cpp:2719-2795): Costas blocks at 0 / 36 / 72 (the speed's
//! kernel — ORIGINAL for Normal, MODIFIED otherwise, which is WHY `encode_word` takes the
//! speed), the LDPC codeword's first 87 bits (colorder-permuted parity) as tones 7..=35 and its
//! last 87 (the message) as tones 43..=71, three bits per tone MSB-first,
//! `tone = 4·b0 + 2·b1 + b2` — NO Gray map (JS8.cpp:2781-2790).
//!
//! Wave (JS8Call Modulator.cpp:52-65, :159-181, read as spec): `delay_ms` of silence so the
//! buffer is slot-positioned as `Mode::gen_wave` requires (the period's first sample is the
//! buffer's first sample), then 79 symbols of continuous-phase sine at
//! `f0 + tone·spacing`, full amplitude, with `amp *= 0.98` per sample once
//! `ic > (79 − 0.017)·samples_per_symbol`. Rectangular symbols — deliberately NOT
//! gen_ft8wave's GFSK: JS8Call's decoder is the compatibility target and its own modulator
//! is rectangular. Output is `f32` in [−1, 1]; the engine scales.

use crate::phy::ldpc::encode87;
use crate::phy::{Speed, Word87};

/// 79 tone indices 0..=7 for `word` at `speed`.
pub fn encode_word(word: &Word87, speed: Speed) -> [u8; 79] {
    let cw = encode87(&word.bits());
    let costas = speed.costas();
    let mut tones = [0u8; 79];
    tones[0..7].copy_from_slice(&costas[0]);
    tones[36..43].copy_from_slice(&costas[1]);
    tones[72..79].copy_from_slice(&costas[2]);
    for k in 0..58 {
        let tone = 4 * cw[3 * k] + 2 * cw[3 * k + 1] + cw[3 * k + 2];
        // genjs8.f90:69-76: data tone k goes to 7 + k for the first 29, then skips block B.
        let pos = if k < 29 { 7 + k } else { 43 + (k - 29) };
        tones[pos] = tone;
    }
    tones
}

/// SLOT-POSITIONED wave: `speed.delay_ms()` of silence, then 79 symbols of continuous-phase
/// rectangular 8-FSK at `f0_hz`, `sample_rate` samples/s (12 000 in the engine; 48 000
/// reproduces Modulator.cpp's own frame rate).
pub fn modulate(tones: &[u8; 79], speed: Speed, f0_hz: f32, sample_rate: f32) -> Vec<f32> {
    modulate_moved(tones, speed, &[(0, f0_hz)], sample_rate)
}

/// [`modulate`]'s wave with its offset moved while it plays: `moves` are `(sample, offset)` pairs
/// in the order of their samples, the first at sample 0, each offset holding from its sample of
/// the wave until the next move. The tones sit on whichever offset holds, and the phase runs on
/// unbroken across a move, as JS8Call's Modulator takes a new audio frequency at the next sample
/// it makes, its phase carried on (`m_audioFrequency != m_audioFrequency0` → `m_dphi`, `m_phi +=
/// m_dphi`, Modulator.cpp:217-229): what "Allow Tx frequency changes while transmitting" does to a
/// frame on the air. Up to its first move's sample the wave is [`modulate`]'s at the first offset,
/// sample for sample, and it is as long.
pub fn modulate_moved(
    tones: &[u8; 79],
    speed: Speed,
    moves: &[(usize, f32)],
    sample_rate: f32,
) -> Vec<f32> {
    let fs = f64::from(sample_rate);
    let spsym = (speed.nsps() as f64 * fs / 12_000.0).round() as usize;
    let delay = (f64::from(speed.delay_ms()) * fs / 1000.0).round() as usize;
    let n = 79 * spsym;
    let fade_from = ((79.0 - 0.017) * spsym as f64) as usize;
    let spacing = f64::from(speed.tone_spacing_hz());
    let tau = 2.0 * std::f64::consts::PI;

    let mut out = vec![0f32; delay + n];
    let mut phi = 0f64;
    let mut amp = 1f64;
    let mut moved = 0;
    for ic in 0..n {
        let tone = f64::from(tones[ic / spsym]);
        while moved + 1 < moves.len() && moves[moved + 1].0 <= delay + ic {
            moved += 1;
        }
        let f = moves.get(moved).map_or(0.0, |m| m.1);
        let dphi = tau * (f64::from(f) + tone * spacing) / fs;
        phi += dphi;
        if phi > tau {
            phi -= tau;
        }
        if ic > fade_from {
            amp *= 0.98;
        }
        out[delay + ic] = (amp * phi.sin()) as f32;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phy::ldpc::encode87;
    use crate::phy::{DecodeParams, Payload72, I3};

    fn word() -> Word87 {
        Word87::new(
            Payload72::from_bytes([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x11]),
            I3::from_u8(3),
        )
    }

    #[test]
    fn tone_layout_is_costas_parity_costas_message_costas() {
        // genjs8.f90:66-77 / JS8.cpp:2719-2795.
        for speed in Speed::ALL {
            let w = word();
            let tones = encode_word(&w, speed);
            let costas = speed.costas();
            assert_eq!(&tones[0..7], &costas[0], "{speed:?} block A");
            assert_eq!(&tones[36..43], &costas[1], "{speed:?} block B");
            assert_eq!(&tones[72..79], &costas[2], "{speed:?} block C");
            assert!(tones.iter().all(|&t| t < 8));
            // Message tones 43..=71 are the 87 message bits, 3 per tone, MSB first, NO Gray map.
            let bits = w.bits();
            for (k, tone) in tones[43..72].iter().enumerate() {
                let want = 4 * bits[3 * k] + 2 * bits[3 * k + 1] + bits[3 * k + 2];
                assert_eq!(*tone, want, "{speed:?} message tone {k}");
            }
            // Parity tones 7..=35 are cw[0..87) grouped the same way.
            let cw = encode87(&bits);
            for (k, tone) in tones[7..36].iter().enumerate() {
                let want = 4 * cw[3 * k] + 2 * cw[3 * k + 1] + cw[3 * k + 2];
                assert_eq!(*tone, want, "{speed:?} parity tone {k}");
            }
        }
    }

    #[test]
    fn normal_and_fast_differ_only_in_the_sync_blocks() {
        let a = encode_word(&word(), Speed::Normal);
        let b = encode_word(&word(), Speed::Fast);
        assert_eq!(&a[7..36], &b[7..36]);
        assert_eq!(&a[43..72], &b[43..72]);
        assert_ne!(&a[0..7], &b[0..7]);
    }

    #[test]
    fn wave_is_slot_positioned_and_fits_every_period() {
        // Bounded airtime: len == delay + 79·nsps < period·12000, leading silence exact.
        for speed in Speed::ALL {
            let wave = modulate(&encode_word(&word(), speed), speed, 1000.0, 12_000.0);
            let delay = speed.delay_ms() as usize * 12;
            assert_eq!(wave.len(), delay + 79 * speed.nsps(), "{speed:?}");
            assert!(
                wave.len() < speed.period_s() as usize * 12_000,
                "{speed:?} does not fit its period"
            );
            assert!(
                wave[..delay].iter().all(|&s| s == 0.0),
                "{speed:?} leading silence"
            );
            assert!(
                wave[delay..].iter().any(|&s| s.abs() > 0.9),
                "{speed:?} full amplitude"
            );
            assert!(
                wave.iter().all(|s| s.is_finite() && s.abs() <= 1.0),
                "{speed:?} range"
            );
        }
    }

    #[test]
    fn wave_scales_with_sample_rate() {
        let w12 = modulate(
            &encode_word(&word(), Speed::Normal),
            Speed::Normal,
            1000.0,
            12_000.0,
        );
        let w48 = modulate(
            &encode_word(&word(), Speed::Normal),
            Speed::Normal,
            1000.0,
            48_000.0,
        );
        assert_eq!(
            w48.len(),
            4 * w12.len(),
            "Modulator.cpp runs at 48 kHz with 4·NSPS per symbol"
        );
    }

    /// Non-coherent 8-tone detector over one symbol: the bin powers at f0 + t·spacing. Tone
    /// spacing equals fs/nsps so the eight bins are exactly orthogonal over a symbol.
    fn detect_tone(x: &[f32], f0: f32, spacing: f32, fs: f32) -> u8 {
        let mut best = (0u8, -1.0f64);
        for t in 0..8u8 {
            let f = f64::from(f0 + t as f32 * spacing);
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (k, &s) in x.iter().enumerate() {
                let ph = 2.0 * std::f64::consts::PI * f * k as f64 / f64::from(fs);
                re += f64::from(s) * ph.cos();
                im -= f64::from(s) * ph.sin();
            }
            let p = re * re + im * im;
            if p > best.1 {
                best = (t, p);
            }
        }
        best.0
    }

    #[test]
    fn every_symbol_is_recovered_by_a_tone_detector_at_every_speed() {
        for speed in Speed::ALL {
            let tones = encode_word(&word(), speed);
            let wave = modulate(&tones, speed, 1000.0, 12_000.0);
            let delay = speed.delay_ms() as usize * 12;
            let nsps = speed.nsps();
            for (k, &want) in tones.iter().enumerate() {
                let sym = &wave[delay + k * nsps..delay + (k + 1) * nsps];
                assert_eq!(
                    detect_tone(sym, 1000.0, speed.tone_spacing_hz(), 12_000.0),
                    want,
                    "{speed:?} symbol {k}"
                );
            }
        }
    }

    #[test]
    fn phase_is_continuous_across_symbol_boundaries() {
        // No phase jump at a tone change: adjacent samples differ by less than one step of the
        // highest tone (f0 + 7·spacing at Normal = 1043.75 Hz → |Δ| ≤ 2π·1043.75/12000 ≈ 0.55).
        let tones = encode_word(&word(), Speed::Normal);
        let wave = modulate(&tones, Speed::Normal, 1000.0, 12_000.0);
        let delay = 500 * 12;
        for k in 1..79 {
            let i = delay + k * 1920;
            assert!((wave[i] - wave[i - 1]).abs() < 0.6, "jump at symbol {k}");
        }
    }

    #[test]
    fn fade_out_only_touches_the_final_fraction_of_the_last_symbol() {
        let tones = encode_word(&word(), Speed::Turbo);
        let wave = modulate(&tones, Speed::Turbo, 1000.0, 12_000.0);
        let delay = 100 * 12;
        let i0 = ((79.0 - 0.017) * 600.0) as usize;
        // Envelope before i0 is full scale (peak of any 20-sample window ≥ 0.9 at 20 Hz spacing / 1000 Hz).
        let body = &wave[delay..delay + i0];
        for win in body.chunks(20).take(body.len() / 20 - 1) {
            assert!(win.iter().map(|s| s.abs()).fold(0.0f32, f32::max) > 0.9);
        }
        // The last sample is faded: 0.98^(~10) ≈ 0.82 upper bound on |amp|.
        assert!(wave[wave.len() - 1].abs() < 0.85);
    }

    /// The offset moved at sample `at` (JS8Call's Modulator taking a new frequency mid-frame): up to
    /// `at` the wave is the one keyed at the old offset, sample for sample; a whole symbol after it
    /// sits on the new offset (its sign changes count the new tone's cycles, 300 Hz × 160 ms = 48
    /// cycles more than the old's); and no sample around the move jumps further than the wave at the
    /// new offset ever steps. Moved at the first sample it is the new offset's wave; never moved, the
    /// old one's.
    #[test]
    fn a_moved_wave_is_the_old_wave_up_to_the_move_and_the_new_offset_after_it() {
        let tones = encode_word(&word(), Speed::Normal);
        let old = modulate(&tones, Speed::Normal, 1500.0, 12_000.0);
        let new = modulate(&tones, Speed::Normal, 1800.0, 12_000.0);
        let delay = 500 * 12;
        let at = delay + 30 * 1920 + 123;
        let move_at = |at: usize| {
            modulate_moved(
                &tones,
                Speed::Normal,
                &[(0, 1500.0), (at, 1800.0)],
                12_000.0,
            )
        };
        let moved = move_at(at);
        assert_eq!(moved.len(), old.len());
        assert_eq!(moved[..at], old[..at], "the old wave up to the move");
        let crossings = |w: &[f32]| {
            w.windows(2)
                .filter(|p| (p[0] < 0.0) != (p[1] < 0.0))
                .count()
        };
        let sym = |w: &[f32], k: usize| w[delay + k * 1920..delay + (k + 1) * 1920].to_vec();
        for k in [31, 40, 70] {
            let (got, at_new, at_old) = (
                crossings(&sym(&moved, k)),
                crossings(&sym(&new, k)),
                crossings(&sym(&old, k)),
            );
            assert!(got.abs_diff(at_new) <= 1, "symbol {k}: {got} vs {at_new}");
            assert!(got.abs_diff(at_old) >= 90, "symbol {k}: {got} vs {at_old}");
        }
        let step = |w: &[f32], r: std::ops::Range<usize>| {
            w[r].windows(2)
                .map(|p| (p[1] - p[0]).abs())
                .fold(0.0f32, f32::max)
        };
        let steady = step(&new, delay..new.len() - 1920);
        assert!(
            step(&moved, at - 50..at + 50) <= steady + 1e-6,
            "no jump at the move: {} against {steady}",
            step(&moved, at - 50..at + 50)
        );
        assert_eq!(
            move_at(0),
            new,
            "moved before it starts: the new offset's wave"
        );
        assert_eq!(move_at(old.len()), old, "never moved: the old one");
        // Moved twice: the second move carries on from where the first left the phase.
        let twice = modulate_moved(
            &tones,
            Speed::Normal,
            &[(0, 1500.0), (at, 1800.0), (at + 4000, 1650.0)],
            12_000.0,
        );
        assert_eq!(
            twice[..at + 4000],
            moved[..at + 4000],
            "the first move as it was"
        );
        assert!(
            step(&twice, at + 3950..at + 4050) <= steady + 1e-6,
            "no jump at the second move"
        );
    }

    #[test]
    fn stock_decode_params_are_js8calls() {
        let p = DecodeParams::stock();
        assert_eq!(
            (p.nfa, p.nfb, p.depth, p.subtract_passes),
            (100.0, 4000.0, 3, 2)
        );
    }
}
