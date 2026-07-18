//! Soundcard AFSK RTTY transmit path — the timing-cleanest way to put RTTY on the air.
//!
//! The rig sits in LSB (or a USB data mode with [`AfskConfig::reverse`]) and the app
//! plays a two-tone FSK waveform: mark 2125 Hz / space 2295 Hz (170 Hz shift),
//! 45.45 baud, async Baudot framing (1 start + 5 data + 1.5 stop bits per character).
//! Because the SOUND CARD clocks the bits, timing is jitter-free — this is the robust
//! default TX path (and the IC-9700/USB-D path). Contrast [`crate::rtty_fsk`], whose
//! bit edges come from OS thread scheduling.
//!
//! Two constraints this generator is built around:
//! - **True 45.45 baud, never integerized.** 12000 / 45.45 = 264.026… samples per
//!   bit — not an integer. Bit boundaries are accumulated in fractional samples so the
//!   cumulative error stays under one sample for any message length; rounding each bit
//!   to 264 samples would walk the far end's clock recovery off the straddle point
//!   mid-message.
//! - **Phase continuity + shaped edges.** Mark and space NCOs run continuously and a
//!   bit edge raised-cosine cross-fades between them (the W7AY dual-oscillator
//!   scheme) — no phase steps, no key clicks, narrow keying sidebands.
//!
//! The ITA2/Baudot encoding itself (LTRS/FIGS, USOS, diddle) lives in
//! `tempo_core::rtty`; this module takes the already-encoded 5-bit stream. Like
//! `tempo_core::cw::morse_samples`, rendering is one-shot per transmission.

/// Standard mark tone (Hz): the LOWER audio tone, which on LSB lands as the HIGHER RF —
/// the on-air RTTY convention.
pub const MARK_HZ: f32 = 2125.0;
/// Standard space tone (Hz): mark + 170 Hz shift.
pub const SPACE_HZ: f32 = 2295.0;
/// Standard RTTY rate: exactly 45.45 baud (22.0022 ms/bit), NOT 45.
pub const BAUD_45: f64 = 45.45;
/// Raised-cosine cross-fade length at each bit edge (and the key-down/up envelope), s.
pub const EDGE_RAMP_S: f64 = 0.002;

/// AFSK generator settings. `Default` is the on-air standard: 2125/2295 Hz, 45.45 baud,
/// 12 kHz mono, LSB convention.
#[derive(Debug, Clone)]
pub struct AfskConfig {
    pub mark_hz: f32,
    pub space_hz: f32,
    pub baud: f64,
    pub sample_rate: u32,
    /// Swap which tone the mark/space bits key. The standard convention is LSB with
    /// mark on 2125 Hz (lower audio → higher RF); a rig run in USB/DATA-U (e.g.
    /// IC-9700) flips the sideband, so set `reverse` there to keep the RF mark/space
    /// sense correct.
    pub reverse: bool,
}

impl Default for AfskConfig {
    fn default() -> Self {
        Self {
            mark_hz: MARK_HZ,
            space_hz: SPACE_HZ,
            baud: BAUD_45,
            sample_rate: 12_000,
            reverse: false,
        }
    }
}

/// Async Baudot framing: each consecutive group of 5 `data_bits` (one character's data
/// bits, in over-the-air order — ITA2 transmits LSB first) becomes
/// 1 start bit (space) + the 5 data bits + a 1.5-bit stop (mark), as
/// `(mark_level, width_in_bits)` pairs. `data_bits.len()` must be a multiple of 5; a
/// trailing partial group is ignored. Shared by the AFSK generator and the true-FSK
/// keyer schedule ([`crate::rtty_fsk::fsk_schedule`]).
pub fn baudot_frame(data_bits: &[bool]) -> Vec<(bool, f64)> {
    debug_assert!(data_bits.len() % 5 == 0, "5 data bits per Baudot character");
    let mut out = Vec::with_capacity((data_bits.len() / 5) * 7);
    for ch in data_bits.chunks_exact(5) {
        out.push((false, 1.0)); // start bit: space
        for &b in ch {
            out.push((b, 1.0));
        }
        out.push((true, 1.5)); // stop: mark, 1.5 bits
    }
    out
}

/// Render a framed character stream (groups of 5 data bits, see [`baudot_frame`]) to
/// AFSK PCM — mono f32 in -1..1 at `cfg.sample_rate`.
pub fn afsk_char_samples(data_bits: &[bool], cfg: &AfskConfig) -> Vec<f32> {
    afsk_samples(&baudot_frame(data_bits), cfg)
}

/// Render a raw `(mark_level, width_in_bits)` sequence to AFSK PCM. This is the layer
/// under [`afsk_char_samples`] — the seam for non-framed output (diddle idle, tuning
/// carriers, tests).
pub fn afsk_samples(levels: &[(bool, f64)], cfg: &AfskConfig) -> Vec<f32> {
    use std::f64::consts::{PI, TAU};
    debug_assert!(cfg.baud > 0.0);
    let fs = cfg.sample_rate.max(1) as f64;
    let spb = fs / cfg.baud; // fractional samples per bit — kept fractional

    // Pass 1: expand levels to a per-sample tone selection. `t_bits` accumulates the
    // EXACT cumulative bit position; each level run fills to its fractional boundary,
    // so per-bit rounding never accumulates (drift < 1 sample forever).
    let mut tone_mark: Vec<bool> = Vec::new();
    let mut t_bits = 0.0f64;
    for &(mark, width) in levels {
        t_bits += width;
        let target = t_bits * spb;
        while (tone_mark.len() as f64) < target {
            tone_mark.push(mark);
        }
    }
    let n = tone_mark.len();
    if n == 0 {
        return Vec::new();
    }

    // Pass 2: dual continuous NCOs + raised-cosine cross-fade at edges. `w_lin` walks
    // linearly toward the current bit's tone; the raised-cosine of it weights the mark
    // oscillator (space gets the complement, so the summed amplitude never exceeds 1).
    let (f_mark, f_space) = if cfg.reverse {
        (cfg.space_hz as f64, cfg.mark_hz as f64)
    } else {
        (cfg.mark_hz as f64, cfg.space_hz as f64)
    };
    let dm = TAU * f_mark / fs;
    let ds = TAU * f_space / fs;
    let ramp_n = ((fs * EDGE_RAMP_S) as usize).max(1);
    let env_r = ramp_n.min(n / 2).max(1);
    let step = 1.0 / ramp_n as f64;
    let mut pm = 0.0f64;
    let mut ps = 0.0f64;
    let mut w_lin = if tone_mark[0] { 1.0f64 } else { 0.0 };
    let mut out = Vec::with_capacity(n);
    for (i, &mark) in tone_mark.iter().enumerate() {
        pm = (pm + dm) % TAU;
        ps = (ps + ds) % TAU;
        w_lin = if mark {
            (w_lin + step).min(1.0)
        } else {
            (w_lin - step).max(0.0)
        };
        let wm = 0.5 * (1.0 - (PI * w_lin).cos());
        // Raised-cosine key envelope at TX start/end kills the on/off clicks.
        let env = if i < env_r {
            0.5 * (1.0 - (PI * (i + 1) as f64 / env_r as f64).cos())
        } else if i >= n - env_r {
            0.5 * (1.0 - (PI * (n - i) as f64 / env_r as f64).cos())
        } else {
            1.0
        };
        out.push((env * (wm * pm.sin() + (1.0 - wm) * ps.sin())) as f32);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_is_start_five_data_and_one_and_a_half_stop() {
        let f = baudot_frame(&[true, false, false, true, true]);
        assert_eq!(f.len(), 7);
        assert_eq!(f[0], (false, 1.0)); // start = space
        let data: Vec<bool> = f[1..6].iter().map(|&(b, _)| b).collect();
        assert_eq!(data, vec![true, false, false, true, true]);
        assert!(f[1..6].iter().all(|&(_, w)| w == 1.0));
        assert_eq!(f[6], (true, 1.5)); // stop = mark, 1.5 bits
        let total: f64 = f.iter().map(|&(_, w)| w).sum();
        assert!((total - 7.5).abs() < 1e-12);
    }

    #[test]
    fn bit_clock_never_drifts_more_than_one_sample() {
        // 12000/45.45 = 264.026… samples/bit. An integerized bit would be short by
        // 0.026 samples — 26 samples off by bit 1000; the fractional accumulator must
        // stay within 1 sample of the exact product at every prefix.
        let levels: Vec<(bool, f64)> = (0..1000).map(|i| (i % 2 == 0, 1.0)).collect();
        let cfg = AfskConfig::default();
        let spb = 12000.0 / BAUD_45;
        for k in [1usize, 3, 45, 100, 454, 1000] {
            let n = afsk_samples(&levels[..k], &cfg).len() as f64;
            assert!((n - k as f64 * spb).abs() < 1.0, "k={k} n={n}");
        }
    }

    fn measured_hz(x: &[f32], fs: f64) -> f64 {
        let crossings = x.windows(2).filter(|w| w[0] * w[1] < 0.0).count();
        crossings as f64 * fs / (2.0 * x.len() as f64)
    }

    #[test]
    fn mark_and_space_tones_via_zero_crossings() {
        let cfg = AfskConfig::default();
        let mark = afsk_samples(&[(true, 400.0)], &cfg);
        let space = afsk_samples(&[(false, 400.0)], &cfg);
        assert!((measured_hz(&mark, 12000.0) - 2125.0).abs() < 5.0);
        assert!((measured_hz(&space, 12000.0) - 2295.0).abs() < 5.0);
    }

    #[test]
    fn reverse_swaps_tones_for_usb_operation() {
        let cfg = AfskConfig {
            reverse: true,
            ..AfskConfig::default()
        };
        let mark = afsk_samples(&[(true, 400.0)], &cfg);
        let space = afsk_samples(&[(false, 400.0)], &cfg);
        assert!((measured_hz(&mark, 12000.0) - 2295.0).abs() < 5.0);
        assert!((measured_hz(&space, 12000.0) - 2125.0).abs() < 5.0);
    }

    #[test]
    fn no_phase_discontinuity_at_bit_edges() {
        use std::f64::consts::PI;
        // Max-transition data (alternating bits) across 20 chars. A continuous tone's
        // largest sample-to-sample step is its phase step (|sin a − sin b| ≤ |a − b|);
        // the cross-fade adds at most π/ramp_n. A hard phase reset would jump ~2.
        let mut bits = Vec::new();
        for _ in 0..20 {
            bits.extend_from_slice(&[true, false, true, false, true]);
        }
        let cfg = AfskConfig::default();
        let x = afsk_char_samples(&bits, &cfg);
        let max_step = x
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max) as f64;
        let ramp_n = (12000.0 * EDGE_RAMP_S).round();
        let bound = 2.0 * PI * 2295.0 / 12000.0 + PI / ramp_n + 0.05;
        assert!(max_step < bound, "max_step={max_step} bound={bound}");
        assert!(bound < 1.6); // a discontinuity-sized jump must still fail
    }
}
