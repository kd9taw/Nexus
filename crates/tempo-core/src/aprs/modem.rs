//! AFSK-1200 (Bell 202) modem — the physical layer under HDLC/AX.25.
//!
//! 1200 baud, mark = 1200 Hz, space = 2200 Hz, run at the app modem rate of 12 kHz so a bit is
//! exactly 10 samples. The modulator is a continuous-phase NCO (no click at tone changes); the
//! demodulator is a pair of sliding quadrature correlators at the two tones whose difference
//! (the discriminator) is sampled once per bit by a timing-recovery PLL that re-centres on every
//! tone transition. Input/output are NRZI tone LEVELS (`true` = mark) — pair with
//! [`nrzi_encode`](super::hdlc::nrzi_encode) / [`nrzi_decode`](super::hdlc::nrzi_decode).
//!
//! Pure DSP + `Vec<f32>`, no soundcard — the tempo-audio layer will resample to the card rate and
//! frame with PTT (mirroring `rtty_afsk` + `rttyrx`). Unit-tested by clean + AWGN round-trips,
//! matching the RTTY demod's convention.

use core::f32::consts::TAU;

/// Modem sample rate (Hz). Matches the app's 12 kHz internal audio; 12000/1200 = 10 samples/bit.
pub const SAMPLE_RATE: f32 = 12_000.0;
/// AFSK baud rate.
pub const BAUD: f32 = 1_200.0;
/// Mark tone (a logical NRZI `true`).
pub const MARK_HZ: f32 = 1_200.0;
/// Space tone (a logical NRZI `false`).
pub const SPACE_HZ: f32 = 2_200.0;
/// Samples per bit (exactly 10 at 12 kHz / 1200 baud).
pub const SAMPLES_PER_BIT: usize = (SAMPLE_RATE / BAUD) as usize;

/// Modulate NRZI tone levels into 12 kHz audio: `true` → mark, `false` → space, one bit held for
/// [`SAMPLES_PER_BIT`] samples with continuous phase across bit boundaries.
pub fn modulate(levels: &[bool]) -> Vec<f32> {
    let mut out = Vec::with_capacity(levels.len() * SAMPLES_PER_BIT);
    let d_mark = TAU * MARK_HZ / SAMPLE_RATE;
    let d_space = TAU * SPACE_HZ / SAMPLE_RATE;
    let mut phase = 0.0f32;
    for &lvl in levels {
        let step = if lvl { d_mark } else { d_space };
        for _ in 0..SAMPLES_PER_BIT {
            out.push(phase.sin());
            phase += step;
            if phase >= TAU {
                phase -= TAU;
            }
        }
    }
    out
}

/// Demodulate 12 kHz AFSK audio back into NRZI tone levels (`true` = mark), one per bit.
pub fn demodulate(samples: &[f32]) -> Vec<bool> {
    let spb = SAMPLES_PER_BIT;
    if samples.len() < spb {
        return Vec::new();
    }
    // Quadrature reference tables over one bit window (the absolute phase reference is immaterial —
    // only the magnitude at each tone is used).
    let mut cos_m = [0.0f32; SAMPLES_PER_BIT];
    let mut sin_m = [0.0f32; SAMPLES_PER_BIT];
    let mut cos_s = [0.0f32; SAMPLES_PER_BIT];
    let mut sin_s = [0.0f32; SAMPLES_PER_BIT];
    for k in 0..spb {
        let am = TAU * MARK_HZ * k as f32 / SAMPLE_RATE;
        let as_ = TAU * SPACE_HZ * k as f32 / SAMPLE_RATE;
        cos_m[k] = am.cos();
        sin_m[k] = am.sin();
        cos_s[k] = as_.cos();
        sin_s[k] = as_.sin();
    }

    // Per-sample discriminator over a trailing one-bit window: mark energy − space energy.
    let n = samples.len();
    let mut disc = vec![0.0f32; n];
    for i in (spb - 1)..n {
        let w = &samples[i + 1 - spb..=i];
        let (mut mi, mut mq, mut si, mut sq) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for k in 0..spb {
            let x = w[k];
            mi += x * cos_m[k];
            mq += x * sin_m[k];
            si += x * cos_s[k];
            sq += x * sin_s[k];
        }
        disc[i] = (mi * mi + mq * mq) - (si * si + sq * sq);
    }

    // Timing-recovery PLL: `clk` runs 0..spb and samples the tone decision when it wraps (bit
    // centre). A tone transition marks a bit boundary → nudge `clk` toward spb/2 so the wrap lands
    // mid-bit. Preamble flags give many transitions to lock before data arrives.
    const PLL_GAIN: f32 = 0.30;
    let half = spb as f32 / 2.0;
    let mut clk = 0.0f32;
    let mut prev = disc[spb - 1] >= 0.0;
    let mut out = Vec::with_capacity(n / spb + 2);
    for &d in disc.iter().skip(spb - 1) {
        let sign = d >= 0.0;
        if sign != prev {
            clk += (half - clk) * PLL_GAIN;
            prev = sign;
        }
        clk += 1.0;
        if clk >= spb as f32 {
            clk -= spb as f32;
            out.push(sign);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::frame::{Address, Frame};
    use super::super::hdlc::{deframe, encode_frame, nrzi_decode, nrzi_encode};
    use super::*;

    fn sample_frame() -> Frame {
        Frame::ui(
            Address::new("APRS", 0),
            Address::new("N0CALL", 9),
            vec![Address::new("WIDE1", 1), Address::new("WIDE2", 1)],
            b"!4903.50N/07201.75W-Nexus APRS test",
        )
    }

    /// Deterministic Box–Muller AWGN scaled to a target SNR over the signal's own power.
    fn add_awgn(sig: &mut [f32], snr_db: f32, seed: u64) {
        let sig_pow: f32 = sig.iter().map(|x| x * x).sum::<f32>() / sig.len().max(1) as f32;
        let noise_pow = sig_pow / 10f32.powf(snr_db / 10.0);
        let sigma = noise_pow.sqrt();
        let mut s = seed | 1;
        let mut next = || {
            // xorshift64* → uniform (0,1)
            s ^= s >> 12;
            s ^= s << 25;
            s ^= s >> 27;
            ((s.wrapping_mul(0x2545F4914F6CDD1D) >> 11) as f32 + 1.0) / (1u64 << 53) as f32
        };
        for x in sig.iter_mut() {
            let (u1, u2) = (next(), next());
            let g = (-2.0 * u1.ln()).sqrt() * (TAU * u2).cos();
            *x += sigma * g;
        }
    }

    #[test]
    fn modulate_holds_each_bit_for_ten_samples() {
        assert_eq!(modulate(&[true, false, true]).len(), 3 * SAMPLES_PER_BIT);
        assert_eq!(SAMPLES_PER_BIT, 10);
    }

    #[test]
    fn a_steady_mark_sits_near_1200_hz() {
        // 20 mark bits → count zero crossings → frequency. 1200 Hz over 200 samples @ 12 kHz.
        let audio = modulate(&[true; 20]);
        let crossings = audio.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        let secs = audio.len() as f32 / SAMPLE_RATE;
        let freq = crossings as f32 / 2.0 / secs;
        assert!((freq - MARK_HZ).abs() < 60.0, "mark tone ~1200 Hz, got {freq}");
    }

    #[test]
    fn clean_afsk_round_trips_a_frame() {
        let f = sample_frame();
        let bytes = f.encode();
        let audio = modulate(&nrzi_encode(&encode_frame(&bytes, 16, 2)));
        let frames = deframe(&nrzi_decode(&demodulate(&audio)));
        assert_eq!(frames.len(), 1, "one frame recovered from clean audio");
        assert_eq!(frames[0], bytes);
        assert_eq!(Frame::decode(&frames[0]), Some(f));
    }

    #[test]
    fn afsk_recovers_a_frame_through_awgn() {
        let f = sample_frame();
        let bytes = f.encode();
        let mut audio = modulate(&nrzi_encode(&encode_frame(&bytes, 24, 3)));
        add_awgn(&mut audio, 12.0, 0xC0FFEE);
        let frames = deframe(&nrzi_decode(&demodulate(&audio)));
        assert!(
            frames.iter().any(|fr| fr == &bytes),
            "frame survives 12 dB AWGN: recovered {} frame(s)",
            frames.len()
        );
    }
}
