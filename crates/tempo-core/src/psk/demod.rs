//! BPSK31 demodulator — written from scratch for Nexus (no ported code).
//!
//! Signal path, one sample at a time at the 12 kHz app modem rate:
//!   1. **NCO mix** at the tuned audio frequency (+ the live AFC correction) to
//!      complex baseband — a single quadrature oscillator, f64 phase.
//!   2. **Decimate ×24 to 500 Hz** (16 samples per symbol at 31.25 Bd) through a
//!      481-tap windowed-sinc anti-alias lowpass, computed polyphase-style only
//!      at the output rate.
//!   3. **Matched filter**: a one-symbol (16-tap) raised-cosine window — the RX
//!      twin of PSK31's cosine-shaped TX reversals.
//!   4. **16-phase symbol sync**: the symbol-lag differential `d = z·conj(z₋₁ₛ)`
//!      peaks in magnitude at the symbol centers, so each of the 16 sample
//!      phases accumulates a slow-averaged |d| and the strongest phase (with
//!      switching hysteresis) is the sampling instant.
//!   5. **Differential slicer**: at the sampling instant, `Re(d) > 0` = no
//!      reversal = bit 1, else bit 0 — BPSK31's differential encoding needs no
//!      absolute carrier phase.
//!   6. **Varicode decode** ([`super::varicode`]) with a per-character soft
//!      confidence = the minimum per-bit `|Re(d)|/|d|` (the phase-error margin:
//!      1 on a clean axis-aligned symbol, →0 at the ±90° decision boundary).
//!
//! **AFC** is slew-limited and clamped to ±25 Hz (the spec'd envelope): the
//! squared symbol stream `v = z²` removes the BPSK modulation, its sample-lag
//! rotation measures the residual carrier offset unambiguously to ±125 Hz, and
//! every half second the NCO walks at most [`AFC_MAX_STEP_HZ`] toward the
//! measurement. There is no acquire-then-freeze here (RTTY's rule): the clamp
//! and the slew bound the walk instead, so the decoder can never cross onto a
//! neighbor faster than the operator can see the readout move.
//!
//! **Squelch** mirrors the RTTY design point: a signal-presence metric gates
//! the PRINTED OUTPUT only — AFC, sync and slicing keep running underneath.
//! The metric is the slow-averaged `cos(2·arg(d))`: on a real BPSK signal the
//! differential phase clusters on the 0/π axis (metric → 1); on band noise it
//! is uniform (metric → 0). Below squelch, band noise would otherwise frame the
//! occasional coin-flip varicode character — a decoder that prints text from
//! noise is worse than none.

use super::varicode::VaricodeDecoder;
use crate::textmode::DecodedChar;
use microfft::Complex32;
use std::f64::consts::PI;

/// Input sample rate (Hz) — the app modem rate.
pub const SAMPLE_RATE: f32 = 12_000.0;

/// PSK31 symbol rate: 31.25 Bd exactly (8000/256, the G3PLX derivation).
pub const BAUD: f32 = 31.25;

/// Decimation factor: 12 kHz → 500 Hz.
const DECIM: usize = 24;

/// Samples per symbol at the decimated rate: 500 / 31.25.
const SYM_SPS: usize = 16;

/// Anti-alias FIR length (windowed sinc, Hamming). At 481 taps the transition
/// band is ~82 Hz: passband to ~100 Hz (a ±25 Hz-offset signal's whole main
/// lobe passes), ~53 dB stopband from ~140 Hz — everything that could fold
/// across the 250 Hz output Nyquist arrives attenuated below the slicer floor.
const FIR_LEN: usize = 481;

/// Anti-alias cutoff (Hz): signal main lobe (±31 Hz) + the full AFC clamp
/// (±25 Hz) with margin.
const FIR_CUTOFF_HZ: f64 = 100.0;

/// AFC measurement window: 0.5 s of 500 Hz samples per update.
const AFC_BLOCK: usize = 250;

/// AFC hard clamp (Hz) — the spec'd envelope around the tuned frequency.
pub const AFC_CLAMP_HZ: f32 = 25.0;

/// AFC slew limit: the farthest one 0.5 s update may walk the NCO (Hz). Bounds
/// the pull rate to ~12 Hz/s — fast enough to center a ±20 Hz mistune inside a
/// CQ preamble, far too slow to jump onto a neighbor between two glances at
/// the readout.
const AFC_MAX_STEP_HZ: f32 = 6.0;

/// Proportional gain on each AFC measurement (the offset itself integrates).
const AFC_GAIN: f32 = 0.5;

/// Squelch thresholds on the slow-averaged `cos(2·arg(d))` quality metric,
/// with hysteresis (open high, hold low — the RTTY pattern). On noise the
/// metric averages ~0 (σ ≈ 0.16 at the 20-symbol time constant), so OPEN sits
/// ~3σ above it; a real signal rides ≥0.8 even at the weak-copy floor.
const SQUELCH_OPEN: f32 = 0.50;
const SQUELCH_CLOSE: f32 = 0.35;

/// Sync-phase switching hysteresis: a challenger phase must beat the incumbent
/// by this factor before the sampling instant moves (re-timing mid-character
/// costs that character, so it must not happen on noise wobble).
const SYNC_HYSTERESIS: f32 = 1.25;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PskConfig {
    /// Tuned audio center (Hz) — where the operator clicked on the waterfall.
    pub center_hz: f32,
    /// Slew-limited AFC (±25 Hz around `center_hz`). On by default.
    pub afc: bool,
}

impl Default for PskConfig {
    fn default() -> Self {
        Self {
            // The PSK31 convention: rigs park the dial so the activity sits
            // around 1 kHz in the passband (DigiPan's default carrier).
            center_hz: 1000.0,
            afc: true,
        }
    }
}

/// The decode seam, which is NOT PSK's own: both types live in
/// [`crate::textmode`] so every keyboard mode decodes into the same ensemble.
/// Re-exported under a PSK name for symmetry with `RttyDemod`.
pub use crate::textmode::TextDemod as PskDemod;

/// |z|² — cheaper than |z| where only ratios matter.
fn norm_sq(z: Complex32) -> f32 {
    z.re * z.re + z.im * z.im
}

/// One-pole follower (the shared `decayavg` shape): `weight` = time constant
/// in updates.
fn decayavg(avg: f32, input: f32, weight: f32) -> f32 {
    if weight <= 1.0 {
        input
    } else {
        avg + (input - avg) / weight
    }
}

/// Windowed-sinc lowpass (Hamming), unity DC gain — the ×24 anti-alias filter.
fn design_fir() -> Vec<f32> {
    let fc = FIR_CUTOFF_HZ / SAMPLE_RATE as f64; // cycles/sample
    let c = (FIR_LEN - 1) as f64 / 2.0;
    let mut taps = Vec::with_capacity(FIR_LEN);
    let mut sum = 0.0f64;
    for i in 0..FIR_LEN {
        let x = i as f64 - c;
        let sinc = if x.abs() < 1e-9 {
            2.0 * fc
        } else {
            (2.0 * PI * fc * x).sin() / (PI * x)
        };
        let w = 0.54 - 0.46 * (2.0 * PI * i as f64 / (FIR_LEN - 1) as f64).cos();
        let t = sinc * w;
        sum += t;
        taps.push(t as f32);
    }
    // Normalize to unity DC gain so envelope-derived metrics keep their scale.
    let g = 1.0 / sum as f32;
    for t in &mut taps {
        *t *= g;
    }
    taps
}

/// One-symbol raised-cosine matched filter (16 taps at 500 Hz), unity gain.
fn matched_taps() -> [f32; SYM_SPS] {
    let mut w = [0.0f32; SYM_SPS];
    let mut sum = 0.0f32;
    for (i, t) in w.iter_mut().enumerate() {
        *t = 0.5 * (1.0 - (2.0 * PI * (i as f64 + 0.5) / SYM_SPS as f64).cos()) as f32;
        sum += *t;
    }
    for t in &mut w {
        *t /= sum;
    }
    w
}

pub struct PskDemodulator {
    cfg: PskConfig,
    // NCO (f64 phase — it accumulates for the life of the arm).
    nco_phase: f64,
    // Anti-alias FIR over the mixed baseband: ring of the last FIR_LEN samples.
    fir: Vec<f32>,
    ring: Vec<Complex32>,
    ring_pos: usize,
    decim_count: usize,
    // Matched filter ring (decimated rate).
    mf_taps: [f32; SYM_SPS],
    mf_ring: [Complex32; SYM_SPS],
    mf_pos: usize,
    // Symbol-lag delay line: z and z one symbol ago.
    sym_ring: [Complex32; SYM_SPS],
    sym_pos: usize,
    // 16-phase sync: per-phase slow-averaged |d| and the chosen sampling phase.
    phase_energy: [f32; SYM_SPS],
    sync_phase: usize,
    sample_idx: usize,
    // AFC: squared-signal lag product accumulated over AFC_BLOCK samples.
    afc_hz: f32,
    afc_acc: Complex32,
    afc_prev: Complex32,
    afc_count: usize,
    // Squelch: slow-averaged cos(2·arg(d)) at the sampling instant + gate state.
    quality: f32,
    sql_open: bool,
    varicode: VaricodeDecoder,
}

impl PskDemodulator {
    pub fn new(cfg: PskConfig) -> Self {
        assert!(cfg.center_hz > 0.0);
        Self {
            cfg,
            nco_phase: 0.0,
            fir: design_fir(),
            ring: vec![Complex32::new(0.0, 0.0); FIR_LEN],
            ring_pos: 0,
            decim_count: 0,
            mf_taps: matched_taps(),
            mf_ring: [Complex32::new(0.0, 0.0); SYM_SPS],
            mf_pos: 0,
            sym_ring: [Complex32::new(0.0, 0.0); SYM_SPS],
            sym_pos: 0,
            phase_energy: [0.0; SYM_SPS],
            sync_phase: 0,
            sample_idx: 0,
            afc_hz: 0.0,
            afc_acc: Complex32::new(0.0, 0.0),
            afc_prev: Complex32::new(0.0, 0.0),
            afc_count: 0,
            quality: 0.0,
            sql_open: false,
            varicode: VaricodeDecoder::new(),
        }
    }

    pub fn config(&self) -> &PskConfig {
        &self.cfg
    }

    /// Current AFC correction (Hz, added to the tuned center).
    pub fn afc_offset_hz(&self) -> f32 {
        self.afc_hz
    }

    /// Whether the squelch currently reads a signal (the cockpit's carrier hint).
    pub fn signal_present(&self) -> bool {
        self.sql_open
    }

    /// Full reset (new tuning): clears DSP state and the AFC, which re-pulls.
    pub fn reset(&mut self) {
        *self = Self::new(self.cfg);
    }
}

impl PskDemodulator {
    /// One decimated (500 Hz) baseband sample through matched filter → sync →
    /// slicer → varicode. The whole symbol-domain path lives here.
    fn process_decimated(&mut self, y: Complex32, out: &mut Vec<DecodedChar>) {
        // Matched filter: ring dot-product (16 real taps × complex samples).
        self.mf_ring[self.mf_pos] = y;
        self.mf_pos = (self.mf_pos + 1) % SYM_SPS;
        let mut m = Complex32::new(0.0, 0.0);
        for (i, t) in self.mf_taps.iter().enumerate() {
            let s = self.mf_ring[(self.mf_pos + i) % SYM_SPS];
            m.re += t * s.re;
            m.im += t * s.im;
        }
        // Symbol-lag differential: d = m · conj(m one symbol ago). The delay
        // ring is read BEFORE this sample overwrites the oldest slot.
        let prev_sym = self.sym_ring[self.sym_pos];
        self.sym_ring[self.sym_pos] = m;
        self.sym_pos = (self.sym_pos + 1) % SYM_SPS;
        let d = m * prev_sym.conj();

        // AFC: the squared signal removes the BPSK modulation; its sample-lag
        // rotation is 2× the residual carrier offset, unambiguous to ±125 Hz.
        if self.cfg.afc {
            let v = m * m;
            let w = v * self.afc_prev.conj();
            self.afc_prev = v;
            self.afc_acc += w;
            self.afc_count += 1;
            if self.afc_count >= AFC_BLOCK {
                let two_omega = self.afc_acc.im.atan2(self.afc_acc.re); // rad/sample, ×2
                let delta_hz =
                    two_omega / 2.0 * (SAMPLE_RATE / DECIM as f32) / (2.0 * std::f32::consts::PI);
                // Slew-limited proportional walk; the offset integrates.
                let step = (AFC_GAIN * delta_hz).clamp(-AFC_MAX_STEP_HZ, AFC_MAX_STEP_HZ);
                self.afc_hz = (self.afc_hz + step).clamp(-AFC_CLAMP_HZ, AFC_CLAMP_HZ);
                self.afc_acc = Complex32::new(0.0, 0.0);
                self.afc_count = 0;
            }
        }

        // 16-phase symbol sync: every phase accumulates the slow-averaged |d|
        // (the differential is strongest at the symbol centers). ~1 s constant:
        // fast enough to catch a keying station's preamble, slow enough that
        // one noise burst can't steal the sampling instant.
        let p = self.sample_idx % SYM_SPS;
        self.sample_idx = self.sample_idx.wrapping_add(1);
        let dm = norm_sq(d).sqrt();
        self.phase_energy[p] = decayavg(self.phase_energy[p], dm, 32.0);
        if p == SYM_SPS - 1 {
            // Once per symbol: let a clearly better phase take over.
            let best = (0..SYM_SPS)
                .max_by(|&a, &b| {
                    self.phase_energy[a]
                        .partial_cmp(&self.phase_energy[b])
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(0);
            if best != self.sync_phase
                && self.phase_energy[best] > SYNC_HYSTERESIS * self.phase_energy[self.sync_phase]
            {
                self.sync_phase = best;
            }
        }

        // The sampling instant: slice, gate, decode.
        if p == self.sync_phase {
            let dn = norm_sq(d).sqrt();
            if dn > 1e-12 {
                // Signal-quality squelch metric: cos(2·arg d) — +1 with the
                // differential on the 0/π axis (real BPSK), ~0 on noise.
                let q_inst = (d.re * d.re - d.im * d.im) / (dn * dn);
                self.quality = decayavg(self.quality, q_inst, 20.0);
                let was_open = self.sql_open;
                self.sql_open = if self.sql_open {
                    self.quality >= SQUELCH_CLOSE
                } else {
                    self.quality >= SQUELCH_OPEN
                };
                if was_open && !self.sql_open {
                    // Signal gone: drop any partial code so band noise can't
                    // weld half a character onto the next real one.
                    self.varicode.reset();
                }
                let bit = d.re > 0.0; // no reversal = 1
                let conf = d.re.abs() / dn; // phase-error margin
                if let Some(ch) = self.varicode.push(bit, conf) {
                    // The squelch gates the printed OUTPUT only — sync, AFC
                    // and the slicer keep running underneath (the RTTY rule).
                    if self.sql_open {
                        out.push(ch);
                    }
                }
            }
        }
    }
}

impl crate::textmode::TextDemod for PskDemodulator {
    fn feed(&mut self, samples: &[f32]) -> Vec<DecodedChar> {
        let mut out = Vec::new();
        for &x in samples {
            // NCO mix to baseband: multiply by e^{jφ}, φ stepping −2πf/sr per
            // sample (the RTTY mixer convention), f = tuned center + AFC.
            let (s, c) = self.nco_phase.sin_cos();
            let z = Complex32::new((c * x as f64) as f32, (s * x as f64) as f32);
            let f = (self.cfg.center_hz + self.afc_hz) as f64;
            self.nco_phase -= 2.0 * PI * f / SAMPLE_RATE as f64;
            if self.nco_phase < -2.0 * PI {
                self.nco_phase += 2.0 * PI;
            }
            // Anti-alias ring; compute an output only at the decimated rate
            // (polyphase economy — the FIR runs at 500 Hz, not 12 kHz).
            self.ring[self.ring_pos] = z;
            self.ring_pos = (self.ring_pos + 1) % FIR_LEN;
            self.decim_count += 1;
            if self.decim_count == DECIM {
                self.decim_count = 0;
                let mut y = Complex32::new(0.0, 0.0);
                for (i, t) in self.fir.iter().enumerate() {
                    let s = self.ring[(self.ring_pos + i) % FIR_LEN];
                    y.re += t * s.re;
                    y.im += t * s.im;
                }
                self.process_decimated(y, &mut out);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::psk::varicode::testenc::encode_bits;
    use crate::textmode::TextDemod;

    /// TEST-ONLY BPSK31 modulator (TX is Phase 2; this never ships). Cosine-
    /// shaped reversals, the on-air waveform: the baseband polarity holds
    /// through a `1` symbol and swings through a half-sine to its negation
    /// across a `0` (reversal) boundary, and the carrier rides on top.
    fn bpsk(bits: &[bool], center_hz: f32, offset_hz: f32) -> Vec<f32> {
        let sr = SAMPLE_RATE as f64;
        let sym_len = sr / BAUD as f64; // 384 samples exactly
        let f = (center_hz + offset_hz) as f64;
        // Per-symbol polarity from the differential rule: 0 = reversal.
        let mut pol = Vec::with_capacity(bits.len() + 1);
        let mut p = 1.0f64;
        pol.push(p);
        for &b in bits {
            if !b {
                p = -p;
            }
            pol.push(p);
        }
        let total = ((pol.len() as f64) * sym_len) as usize;
        let mut out = Vec::with_capacity(total);
        let mut phase = 0.0f64;
        for n in 0..total {
            let t = n as f64 / sym_len; // time in symbols
            let k = t as usize; // current symbol index
            let u = t - k as f64; // position in symbol 0..1
            let cur = pol[k];
            let prev = if k > 0 { pol[k - 1] } else { cur };
            let next = if k + 1 < pol.len() { pol[k + 1] } else { cur };
            // Half-sine envelope shaping across a differing boundary; flat
            // elsewhere. sin(πu) is 0 at the boundary and 1 mid-symbol, so a
            // reversal passes through zero amplitude — the PSK31 shape.
            let env = if u < 0.5 {
                if prev != cur {
                    (PI * u).sin()
                } else {
                    1.0
                }
            } else if next != cur {
                (PI * u).sin()
            } else {
                1.0
            };
            phase += 2.0 * PI * f / sr;
            if phase > 2.0 * PI {
                phase -= 2.0 * PI;
            }
            out.push((cur * env * phase.sin()) as f32);
        }
        out
    }

    /// A realistic over: leading silence, an idle-reversal preamble (the PSK31
    /// key-up), the varicode text, trailing idle + silence to flush the FIR
    /// and the symbol clock.
    fn signal(text: &str, center_hz: f32, idle_syms: usize, offset_hz: f32) -> Vec<f32> {
        let mut bits = vec![false; idle_syms];
        bits.extend(encode_bits(text));
        bits.extend(std::iter::repeat_n(false, 32));
        let mut audio = vec![0.0f32; 1200];
        audio.extend(bpsk(&bits, center_hz, offset_hz));
        audio.extend(vec![0.0f32; 2400]);
        audio
    }

    /// Deterministic AWGN at `snr_db` referenced to the ham-standard 3 kHz
    /// bandwidth (the RTTY test convention — a white-noise floor over the full
    /// 6 kHz Nyquist carries twice its in-3-kHz share). Box–Muller / xorshift64.
    fn add_awgn(audio: &mut [f32], snr_db: f32, mut seed: u64) {
        let ps = audio.iter().map(|x| x * x).sum::<f32>() / audio.len() as f32;
        let sigma = (2.0 * ps / 10f32.powf(snr_db / 10.0)).sqrt();
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for x in audio.iter_mut() {
            let u1 = rnd().max(1e-12);
            let u2 = rnd();
            let g = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
            *x += sigma * g as f32;
        }
    }

    /// Feed in uneven chunks (as live audio arrives) and collect everything.
    fn decode_all(demod: &mut PskDemodulator, audio: &[f32]) -> Vec<DecodedChar> {
        let mut out = Vec::new();
        for chunk in audio.chunks(479) {
            out.extend(demod.feed(chunk));
        }
        out
    }

    fn text_of(chars: &[DecodedChar]) -> String {
        chars.iter().map(|d| d.ch).collect()
    }

    /// Copy accuracy as LCS(expected, got)/len(expected) — robust to the
    /// insertions/deletions a noisy channel produces.
    fn accuracy(expected: &str, got: &str) -> f32 {
        let a: Vec<char> = expected.chars().collect();
        let b: Vec<char> = got.chars().collect();
        let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
        for i in 1..=a.len() {
            for j in 1..=b.len() {
                dp[i][j] = if a[i - 1] == b[j - 1] {
                    dp[i - 1][j - 1] + 1
                } else {
                    dp[i - 1][j].max(dp[i][j - 1])
                };
            }
        }
        dp[a.len()][b.len()] as f32 / a.len() as f32
    }

    /// Deterministic white noise, `secs` seconds at `amp` level.
    fn white_noise(secs: f32, amp: f32, mut seed: u64) -> Vec<f32> {
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        (0..(SAMPLE_RATE * secs) as usize)
            .map(|_| {
                let g = (-2.0 * rnd().max(1e-12).ln()).sqrt()
                    * (2.0 * std::f64::consts::PI * rnd()).cos();
                amp * g as f32
            })
            .collect()
    }

    const MSG: &str = "CQ CQ de KD9TAW KD9TAW pse k";

    #[test]
    fn clean_copy_is_exact() {
        let cfg = PskConfig::default();
        let mut demod = PskDemodulator::new(cfg);
        let chars = decode_all(&mut demod, &signal(MSG, cfg.center_hz, 48, 0.0));
        let text = text_of(&chars);
        assert!(text.contains(MSG), "clean copy: {text:?}");
        assert!(chars.iter().all(|d| (0.0..=1.0).contains(&d.confidence)));
        let mean = chars.iter().map(|d| d.confidence).sum::<f32>() / chars.len() as f32;
        assert!(mean > 0.7, "clean confidence should be high, got {mean}");
    }

    #[test]
    fn mixed_case_and_punctuation_survive() {
        // Full-ASCII varicode — the whole point over Baudot: case + brackets +
        // the classic brag-line characters all round-trip through the DSP.
        let cfg = PskConfig::default();
        let mut demod = PskDemodulator::new(cfg);
        let msg = "Name is Op, QTH: Madison [EN53] = 5w @ dipole; hw? BTU de KD9TAW k";
        let text = text_of(&decode_all(
            &mut demod,
            &signal(msg, cfg.center_hz, 48, 0.0),
        ));
        assert!(text.contains(msg), "got {text:?}");
    }

    #[test]
    fn chunked_feed_matches_state_carry() {
        // Same audio through 479-sample chunks vs one call — identical output
        // proves all state carries across feed() boundaries.
        let cfg = PskConfig::default();
        let audio = signal(MSG, cfg.center_hz, 48, 0.0);
        let mut d1 = PskDemodulator::new(cfg);
        let mut d2 = PskDemodulator::new(cfg);
        assert_eq!(decode_all(&mut d1, &audio), d2.feed(&audio));
    }

    #[test]
    fn awgn_0db_copies_clean() {
        // 0 dB in 3 kHz ≈ 19.8 dB Eb/N0 in PSK31's ~31 Hz — a strong signal by
        // PSK standards; the demod must copy it essentially verbatim.
        let cfg = PskConfig::default();
        let mut audio = signal(MSG, cfg.center_hz, 64, 0.0);
        add_awgn(&mut audio, 0.0, 0x2545F4914F6CDD1D);
        let text = text_of(&decode_all(&mut PskDemodulator::new(cfg), &audio));
        let acc = accuracy(MSG, &text);
        assert!(acc >= 0.9, "0 dB copy accuracy {acc} ({text:?})");
    }

    #[test]
    fn awgn_minus8db_holds_copy() {
        // −8 dB in 3 kHz (≈ 11.8 dB Eb/N0 in PSK31's ~31 Hz): the STATED
        // operating point this build holds, pinned with margin — the ignored
        // `margins` diagnostic shows full copy at −10 dB and degradation
        // starting near −12 dB on its seed. WSJT-X-class this is not (FT8
        // lives 10 dB deeper), but it comfortably covers the real PSK31
        // operating range, where stations are worked by ear-visible traces.
        let cfg = PskConfig::default();
        let mut audio = signal(MSG, cfg.center_hz, 64, 0.0);
        add_awgn(&mut audio, -8.0, 0x9E3779B97F4A7C15);
        let text = text_of(&decode_all(&mut PskDemodulator::new(cfg), &audio));
        let acc = accuracy(MSG, &text);
        assert!(acc >= 0.8, "-8 dB copy accuracy {acc} ({text:?})");
    }

    #[test]
    fn confidence_degrades_with_noise() {
        let cfg = PskConfig::default();
        let mean_conf = |snr: Option<f32>| {
            let mut audio = signal(MSG, cfg.center_hz, 64, 0.0);
            if let Some(db) = snr {
                add_awgn(&mut audio, db, 0xDEADBEEFCAFE1234);
            }
            let chars = decode_all(&mut PskDemodulator::new(cfg), &audio);
            assert!(!chars.is_empty());
            chars.iter().map(|d| d.confidence).sum::<f32>() / chars.len() as f32
        };
        let clean = mean_conf(None);
        let noisy = mean_conf(Some(-5.0));
        assert!(
            noisy < clean,
            "soft metric must fall with SNR: clean {clean}, -5 dB {noisy}"
        );
    }

    #[test]
    fn afc_pulls_in_a_plus_20hz_offset() {
        // The operator clicked 20 Hz off the signal (or the other station
        // drifted). The slew-limited AFC must walk onto it inside the idle
        // preamble and the copy must then be clean.
        let cfg = PskConfig::default();
        let mut demod = PskDemodulator::new(cfg);
        let text = text_of(&decode_all(
            &mut demod,
            &signal(MSG, cfg.center_hz, 96, 20.0),
        ));
        assert!(
            (demod.afc_offset_hz() - 20.0).abs() < 5.0,
            "AFC should walk to ≈+20 Hz, got {}",
            demod.afc_offset_hz()
        );
        let acc = accuracy(MSG, &text);
        assert!(
            acc >= 0.8,
            "off-tune copy after AFC pull-in: {acc} ({text:?})"
        );
    }

    #[test]
    fn afc_pulls_in_a_minus_20hz_offset() {
        // Both directions — a sign error here decodes one side and walks away
        // from the other.
        let cfg = PskConfig::default();
        let mut demod = PskDemodulator::new(cfg);
        let text = text_of(&decode_all(
            &mut demod,
            &signal(MSG, cfg.center_hz, 96, -20.0),
        ));
        assert!(
            (demod.afc_offset_hz() + 20.0).abs() < 5.0,
            "AFC should walk to ≈−20 Hz, got {}",
            demod.afc_offset_hz()
        );
        let acc = accuracy(MSG, &text);
        assert!(
            acc >= 0.8,
            "off-tune copy after AFC pull-in: {acc} ({text:?})"
        );
    }

    #[test]
    fn afc_stays_inside_the_clamp() {
        // A signal 60 Hz off is SOMEBODY ELSE. The clamp must hold the decoder
        // near where the operator put it, whatever the measurement says.
        let cfg = PskConfig::default();
        let mut demod = PskDemodulator::new(cfg);
        decode_all(&mut demod, &signal(MSG, cfg.center_hz, 96, 60.0));
        assert!(
            demod.afc_offset_hz().abs() <= AFC_CLAMP_HZ + 1e-3,
            "AFC walked past the clamp: {}",
            demod.afc_offset_hz()
        );
    }

    #[test]
    fn afc_disabled_stays_put() {
        let cfg = PskConfig {
            afc: false,
            ..PskConfig::default()
        };
        let mut demod = PskDemodulator::new(cfg);
        decode_all(&mut demod, &signal(MSG, cfg.center_hz, 64, 12.0));
        assert_eq!(demod.afc_offset_hz(), 0.0);
    }

    #[test]
    fn noise_only_is_squelched_silent() {
        // THE NEGATIVE CONTROL. Band noise sliced by a differential decision
        // still frames the odd varicode character; the quality squelch must
        // keep it near-silent. Not exactly zero — that would only overfit one
        // lucky noise realization — but a handful across seeds, where an
        // ungated decoder streams garbage continuously.
        for seed in [
            0x0123456789ABCDEFu64,
            0x2545F4914F6CDD1D,
            0x9E3779B97F4A7C15,
        ] {
            let noise = white_noise(5.0, 0.2, seed);
            let chars = decode_all(&mut PskDemodulator::new(PskConfig::default()), &noise);
            assert!(
                chars.len() <= 3,
                "noise must be near-silent, got {} chars: {:?}",
                chars.len(),
                text_of(&chars)
            );
        }
    }

    #[test]
    fn squelch_still_opens_on_a_weak_signal() {
        // The gate protects silence without eating weak copy: at the stated
        // −5 dB floor the squelch must open and print (the same guarantee the
        // RTTY squelch test pins).
        let cfg = PskConfig::default();
        let mut audio = signal(MSG, cfg.center_hz, 64, 0.0);
        add_awgn(&mut audio, -5.0, 0x0123456789ABCDEF);
        let text = text_of(&decode_all(&mut PskDemodulator::new(cfg), &audio));
        let acc = accuracy(MSG, &text);
        assert!(
            acc >= 0.5,
            "-5 dB copy through the squelch: {acc} ({text:?})"
        );
    }

    #[test]
    fn reset_reacquires() {
        let cfg = PskConfig::default();
        let mut demod = PskDemodulator::new(cfg);
        decode_all(&mut demod, &signal(MSG, cfg.center_hz, 96, 15.0));
        assert!(demod.afc_offset_hz().abs() > 5.0, "AFC should have moved");
        demod.reset();
        assert_eq!(demod.afc_offset_hz(), 0.0);
        assert!(!demod.signal_present());
    }

    #[test]
    #[ignore] // diagnostic: cargo test -p tempo-core psk::demod::tests::margins -- --ignored --nocapture
    fn margins() {
        for snr in [10.0f32, 0.0, -5.0, -8.0, -10.0, -12.0] {
            let cfg = PskConfig::default();
            let mut audio = signal(MSG, cfg.center_hz, 64, 0.0);
            add_awgn(&mut audio, snr, 0x2545F4914F6CDD1D);
            let chars = decode_all(&mut PskDemodulator::new(cfg), &audio);
            let text = text_of(&chars);
            let conf = chars.iter().map(|d| d.confidence).sum::<f32>() / chars.len().max(1) as f32;
            println!(
                "snr {snr:>6} dB  acc {:.2}  conf {conf:.2}  {text:?}",
                accuracy(MSG, &text)
            );
        }
    }
}
