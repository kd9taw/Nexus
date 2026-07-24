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

use super::hdlc::NRZI_INIT;

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

/// Timing-recovery PLL gain: fraction of the phase error pulled out at each tone transition.
const PLL_GAIN: f32 = 0.30;

/// Quadrature reference taps `(cos_mark, sin_mark, cos_space, sin_space)`, one per bit-window sample.
type RefTaps = [(f32, f32, f32, f32); SAMPLES_PER_BIT];

fn ref_tables() -> RefTaps {
    let mut t = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); SAMPLES_PER_BIT];
    for (k, slot) in t.iter_mut().enumerate() {
        let am = TAU * MARK_HZ * k as f32 / SAMPLE_RATE;
        let asp = TAU * SPACE_HZ * k as f32 / SAMPLE_RATE;
        *slot = (am.cos(), am.sin(), asp.cos(), asp.sin());
    }
    t
}

/// Discriminator over one bit window (chronological order): mark energy − space energy.
fn discriminate(win: &[f32], t: &RefTaps) -> f32 {
    let (mut mi, mut mq, mut si, mut sq) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for (&x, &(cm, sm, cs, ss)) in win.iter().zip(t.iter()) {
        mi += x * cm;
        mq += x * sm;
        si += x * cs;
        sq += x * ss;
    }
    (mi * mi + mq * mq) - (si * si + sq * sq)
}

/// Stateful streaming AFSK demodulator: feed audio chunks, get the logical (NRZI-decoded) bits back.
/// Carries the correlator window, the timing PLL, and the NRZI reference across calls, so a frame
/// spread over several audio drains decodes seamlessly (the shape the live RX thread needs).
pub struct Demod {
    tables: RefTaps,
    ring: [f32; SAMPLES_PER_BIT],
    pos: usize,
    filled: usize,
    clk: f32,
    prev_sign: bool,
    started: bool,
    nrzi_prev: bool,
}

impl Default for Demod {
    fn default() -> Self {
        Self::new()
    }
}

impl Demod {
    pub fn new() -> Self {
        Self {
            tables: ref_tables(),
            ring: [0.0; SAMPLES_PER_BIT],
            pos: 0,
            filled: 0,
            clk: 0.0,
            prev_sign: false,
            started: false,
            nrzi_prev: NRZI_INIT,
        }
    }

    /// Feed audio; return the logical bits recovered (NRZI already decoded), ready for a
    /// [`Deframer`](super::hdlc::Deframer).
    pub fn feed(&mut self, samples: &[f32]) -> Vec<bool> {
        let spb = SAMPLES_PER_BIT;
        let half = spb as f32 / 2.0;
        let mut out = Vec::new();
        for &x in samples {
            self.ring[self.pos] = x;
            self.pos = (self.pos + 1) % spb;
            if self.filled < spb {
                self.filled += 1;
                if self.filled < spb {
                    continue;
                }
            }
            // Window in chronological order (oldest at self.pos).
            let mut win = [0.0f32; SAMPLES_PER_BIT];
            for (k, w) in win.iter_mut().enumerate() {
                *w = self.ring[(self.pos + k) % spb];
            }
            let sign = discriminate(&win, &self.tables) >= 0.0;
            if !self.started {
                self.prev_sign = sign;
                self.started = true;
            }
            if sign != self.prev_sign {
                self.clk += (half - self.clk) * PLL_GAIN;
                self.prev_sign = sign;
            }
            self.clk += 1.0;
            if self.clk >= spb as f32 {
                self.clk -= spb as f32;
                out.push(sign == self.nrzi_prev); // NRZI decode: no change = 1
                self.nrzi_prev = sign;
            }
        }
        out
    }
}

/// One-shot AFSK demodulation → NRZI tone LEVELS (`true` = mark), one per bit. Prefer [`Demod`]
/// for streamed/live audio; this stays for whole-buffer decoding + tests.
pub fn demodulate(samples: &[f32]) -> Vec<bool> {
    let spb = SAMPLES_PER_BIT;
    if samples.len() < spb {
        return Vec::new();
    }
    let tables = ref_tables();
    let half = spb as f32 / 2.0;
    let mut clk = 0.0f32;
    let mut prev = false;
    let mut started = false;
    let mut out = Vec::with_capacity(samples.len() / spb + 2);
    for i in (spb - 1)..samples.len() {
        let sign = discriminate(&samples[i + 1 - spb..=i], &tables) >= 0.0;
        if !started {
            prev = sign;
            started = true;
        }
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
    fn streaming_demod_matches_the_one_shot_and_is_chunk_invariant() {
        use super::super::hdlc::nrzi_decode;
        let audio = modulate(&nrzi_encode(&encode_frame(&sample_frame().encode(), 16, 2)));
        // Whole-buffer Demod == nrzi_decode(demodulate(whole)).
        let expected = nrzi_decode(&demodulate(&audio));
        assert_eq!(Demod::new().feed(&audio), expected, "whole-buffer streaming matches one-shot");
        // Fed in awkward chunks, the carried state yields the exact same bits.
        let mut d = Demod::new();
        let mut got = Vec::new();
        for chunk in audio.chunks(37) {
            got.extend(d.feed(chunk));
        }
        assert_eq!(got, expected, "chunked feed is invariant");
    }

    #[test]
    fn streaming_demod_plus_deframer_recovers_a_split_frame() {
        use super::super::hdlc::Deframer;
        let bytes = sample_frame().encode();
        let audio = modulate(&nrzi_encode(&encode_frame(&bytes, 16, 2)));
        let mut demod = Demod::new();
        let mut deframer = Deframer::new();
        let mut frames = Vec::new();
        // 50 ms-ish drains (600 samples) — a frame spans many of them.
        for chunk in audio.chunks(600) {
            frames.extend(deframer.push(&demod.feed(chunk)));
        }
        assert!(frames.iter().any(|f| f == &bytes), "frame recovered across streamed chunks");
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
