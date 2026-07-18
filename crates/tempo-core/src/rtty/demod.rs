//! RTTY demodulator — a Rust port of the fldigi receive path.
//!
//! Ported from fldigi `src/cw_rtty/rtty.cxx` (Copyright (C) 2012 Dave Freese
//! W1HKJ, Stefan Fendt DL1SMF; descended from Tomi Manninen OH2BNS's gmfsk)
//! and `src/filters/fftfilt.cxx` (Copyright (C) 2006-2008 Dave Freese W1HKJ),
//! both GPLv3+ — license-compatible with Nexus (GPLv3). The design keeps
//! fldigi's structure: mark/space baseband mixers → 1024-point overlap-add FFT
//! filters with the Feher raised-cosine "enhanced Nyquist" response → the W7AY
//! SNR-optimized ATC slicer → straddle-point bit-clock recovery → phase-
//! difference AFC, here with acquire-then-freeze behavior (a locked decoder
//! must never walk onto a stronger neighbor mid-QSO — the known MMTTY gotcha).
//!
//! Differences from fldigi: 12 kHz sample rate (the app modem rate, vs 8 kHz),
//! 5-bit Baudot only (no ASCII/parity yet), and the ATC slicer's continuous
//! value is carried out as a per-character soft confidence instead of being
//! discarded — that soft metric plus the [`RttyDemod`] trait is the decoder-
//! ensemble seam.

use super::baudot::BaudotDecoder;
use microfft::Complex32;
use std::f64::consts::PI;

/// Input sample rate (Hz) — the app modem rate.
pub const SAMPLE_RATE: f32 = 12_000.0;

/// Overlap-add FFT block (fldigi FILTLEN scaled to 12 kHz: 512 @ 8 kHz).
const FLEN: usize = 1024;
const FLEN2: usize = FLEN / 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RttyConfig {
    /// Mark tone (Hz). Standard HF pair is 2125/2295 — swap the two to
    /// reverse the sense (fldigi's "reverse" flag).
    pub mark_hz: f32,
    /// Space tone (Hz); shift = space − mark (default 170 Hz).
    pub space_hz: f32,
    /// True 45.45 baud by default — never integerized to 45. 75.0 is the
    /// other common amateur rate.
    pub baud: f32,
    /// Unshift-on-space on decode (default ON — amateur convention).
    pub usos: bool,
    /// Automatic frequency correction: acquires on decoded characters, then
    /// freezes once locked.
    pub afc: bool,
}

impl Default for RttyConfig {
    fn default() -> Self {
        Self {
            mark_hz: 2125.0,
            space_hz: 2295.0,
            baud: 45.45,
            usos: true,
            afc: true,
        }
    }
}

/// One decoded character with its ATC soft metric: 0..1, the minimum slicer
/// margin over the character's sampled bits (start + 5 data + stop). Low
/// values mean "render me faint / don't trust me in the sequencer".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecodedChar {
    pub ch: char,
    pub confidence: f32,
}

/// Streaming demodulator seam: feed mono f32 audio at [`SAMPLE_RATE`], get
/// decoded characters. The future decoder ensemble runs N implementations of
/// this trait from one audio tap into one merge stage.
pub trait RttyDemod {
    fn feed(&mut self, samples: &[f32]) -> Vec<DecodedChar>;
}

/// |z| — num-complex is built no_std here (via microfft), so `norm()` is
/// unavailable.
fn mag(z: Complex32) -> f32 {
    (z.re * z.re + z.im * z.im).sqrt()
}

/// sin(πx)/(πx).
fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-10 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

/// fldigi `fftfilt::rtty_filter`: raised-cosine² lowpass per Feher's
/// "enhanced Nyquist" channel, amplitude-equalized by 1/sinc, cutoff scaled by
/// K = 1.4 (fldigi's CER-optimal value at −9 dB s/n). Defined directly in the
/// frequency domain with a linear-phase term (FLEN/4-sample delay).
fn rtty_filter(baud: f64) -> Vec<Complex32> {
    let f = 1.4 * baud / SAMPLE_RATE as f64;
    let mut filter = vec![Complex32::new(0.0, 0.0); FLEN];
    for i in 0..FLEN2 {
        let x = i as f64 / FLEN2 as f64; // 0..1 × Nyquist
        let dht = if x > 2.0 * f {
            0.0 // beyond cutoff (also dodges the 0/0 at sinc's zeros)
        } else {
            let c = (PI * x / (4.0 * f)).cos();
            c * c / sinc(2.0 * i as f64 * f)
        };
        let ph = i as f64 * 0.5 * PI;
        let (s, c) = ph.sin_cos();
        filter[i] = Complex32::new((dht * c) as f32, (-dht * s) as f32);
        filter[(FLEN - i) % FLEN] = Complex32::new((dht * c) as f32, (dht * s) as f32);
    }
    filter
}

/// Overlap-add fast-convolution filter (fldigi `fftfilt`): collect FLEN/2
/// samples, zero-padded FFT, multiply by the frequency response, inverse FFT,
/// add the saved overlap.
struct FftFilt {
    filter: Vec<Complex32>,   // H(ω), FLEN bins
    timedata: Vec<Complex32>, // input block; upper half stays zero (padding)
    ovlbuf: Vec<Complex32>,   // FLEN2 overlap carried to the next block
    inptr: usize,
}

impl FftFilt {
    fn new(baud: f64) -> Self {
        Self {
            filter: rtty_filter(baud),
            timedata: vec![Complex32::new(0.0, 0.0); FLEN],
            ovlbuf: vec![Complex32::new(0.0, 0.0); FLEN2],
            inptr: 0,
        }
    }

    /// Push one baseband sample; when a block completes, fill `out` with
    /// FLEN2 filtered samples and return true.
    fn run(&mut self, z: Complex32, out: &mut [Complex32; FLEN2]) -> bool {
        self.timedata[self.inptr] = z;
        self.inptr += 1;
        if self.inptr < FLEN2 {
            return false;
        }
        self.inptr = 0;
        let mut freq: [Complex32; FLEN] = self.timedata[..].try_into().unwrap();
        // Both transforms are in-place; the returned reference is redundant.
        let _ = microfft::complex::cfft_1024(&mut freq);
        for (v, h) in freq.iter_mut().zip(&self.filter) {
            *v *= *h;
        }
        let _ = microfft::inverse::ifft_1024(&mut freq);
        for i in 0..FLEN2 {
            out[i] = self.ovlbuf[i] + freq[i];
            self.ovlbuf[i] = freq[i + FLEN2];
        }
        true
    }
}

/// fldigi `decayavg`: one-pole follower, `weight` = time constant in samples.
fn decayavg(avg: f32, input: f32, weight: f32) -> f32 {
    if weight <= 1.0 {
        input
    } else {
        avg + (input - avg) / weight
    }
}

/// Async character reception (1 start + 5 data + stop), clocked one bit-slicer
/// sample at a time. Counters are in samples; every bit is judged at the
/// straddle point (the middle of the symbol-length bit window).
#[derive(Debug, Clone, Copy)]
enum RxState {
    Idle,
    Start {
        counter: usize,
    },
    Data {
        counter: usize,
        bitcnt: u32,
        data: u8,
        cmin: f32,
    },
    Stop {
        counter: usize,
        data: u8,
        cmin: f32,
    },
}

pub struct RttyDemodulator {
    cfg: RttyConfig,
    symlen: usize, // samples per bit: SAMPLE_RATE / baud
    // Mark/space quadrature mixers (f64 phase — it accumulates forever).
    mark_phase: f64,
    space_phase: f64,
    mark_filt: FftFilt,
    space_filt: FftFilt,
    // ATC envelope/noise followers (fast attack, slow decay & the reverse).
    mark_env: f32,
    space_env: f32,
    mark_noise: f32,
    space_noise: f32,
    // Sliced-bit ring spanning one symbol (fldigi bit_buf) + per-sample
    // slicer confidence; ring_marks = count of mark bits for straddle detect.
    bits: Vec<bool>,
    confs: Vec<f32>,
    ring_pos: usize,
    ring_marks: usize,
    // Filtered-mark history for the AFC phase-difference measurement.
    hist: Vec<Complex32>,
    hist_pos: usize,
    state: RxState,
    baudot: BaudotDecoder,
    last_emitted: char,
    // AFC: tone correction applied to both mixers; freezes after lock_run
    // consecutive small errors.
    afc_offset: f32,
    afc_frozen: bool,
    lock_run: u32,
}

impl RttyDemodulator {
    pub fn new(cfg: RttyConfig) -> Self {
        assert!(cfg.baud > 0.0 && cfg.mark_hz != cfg.space_hz);
        let symlen = (SAMPLE_RATE / cfg.baud + 0.5) as usize;
        Self {
            cfg,
            symlen,
            mark_phase: 0.0,
            space_phase: 0.0,
            mark_filt: FftFilt::new(cfg.baud as f64),
            space_filt: FftFilt::new(cfg.baud as f64),
            mark_env: 0.0,
            space_env: 0.0,
            mark_noise: 0.0,
            space_noise: 0.0,
            bits: vec![false; symlen],
            confs: vec![0.0; symlen],
            ring_pos: 0,
            ring_marks: 0,
            hist: vec![Complex32::new(0.0, 0.0); symlen + 1],
            hist_pos: 0,
            state: RxState::Idle,
            baudot: BaudotDecoder::new(cfg.usos),
            last_emitted: '\0',
            afc_offset: 0.0,
            afc_frozen: false,
            lock_run: 0,
        }
    }

    pub fn config(&self) -> &RttyConfig {
        &self.cfg
    }

    /// Current AFC tone correction (Hz, added to both mark and space).
    pub fn afc_offset_hz(&self) -> f32 {
        self.afc_offset
    }

    /// True once the AFC has acquired and frozen.
    pub fn afc_locked(&self) -> bool {
        self.afc_frozen
    }

    /// Full reset (new tuning): clears DSP state, shift state and the AFC —
    /// which re-acquires.
    pub fn reset(&mut self) {
        *self = Self::new(self.cfg);
    }

    /// Quadrature mixer to baseband: multiply by e^{jφ}, φ stepping −2πf/sr
    /// per sample (fldigi `rtty::mixer`).
    fn mix(phase: &mut f64, f_hz: f64, x: f32) -> Complex32 {
        let (s, c) = phase.sin_cos();
        let z = Complex32::new((c * x as f64) as f32, (s * x as f64) as f32);
        *phase -= 2.0 * PI * f_hz / SAMPLE_RATE as f64;
        if *phase < -2.0 * PI {
            *phase += 2.0 * PI;
        }
        z
    }

    // Ring accessors: after a push, ring_pos is the OLDEST slot; the ring
    // spans exactly one symbol, so the middle is the straddle point.
    fn oldest(&self) -> bool {
        self.bits[self.ring_pos]
    }
    fn newest(&self) -> bool {
        self.bits[(self.ring_pos + self.symlen - 1) % self.symlen]
    }
    fn mid_is_mark(&self) -> bool {
        self.bits[(self.ring_pos + self.symlen / 2) % self.symlen]
    }
    fn mid_conf(&self) -> f32 {
        self.confs[(self.ring_pos + self.symlen / 2) % self.symlen]
    }

    /// One filtered mark/space sample → ATC slice → bit ring → UART clock.
    fn process(&mut self, m: Complex32, s: Complex32, out: &mut Vec<DecodedChar>) {
        let mm = mag(m);
        let sm = mag(s);
        let sl = self.symlen as f32;
        // fldigi's follower time constants: envelope attacks in a quarter
        // symbol and sags over 16; noise drops in a quarter and creeps up
        // over 48 (so a long mark barely lifts the floor).
        self.mark_env = decayavg(
            self.mark_env,
            mm,
            if mm > self.mark_env {
                sl / 4.0
            } else {
                sl * 16.0
            },
        );
        self.mark_noise = decayavg(
            self.mark_noise,
            mm,
            if mm < self.mark_noise {
                sl / 4.0
            } else {
                sl * 48.0
            },
        );
        self.space_env = decayavg(
            self.space_env,
            sm,
            if sm > self.space_env {
                sl / 4.0
            } else {
                sl * 16.0
            },
        );
        self.space_noise = decayavg(
            self.space_noise,
            sm,
            if sm < self.space_noise {
                sl / 4.0
            } else {
                sl * 48.0
            },
        );
        let nf = self.mark_noise.min(self.space_noise);
        let mclipped = mm.min(self.mark_env).max(nf);
        let sclipped = sm.min(self.space_env).max(nf);
        let me = (self.mark_env - nf).max(0.0);
        let se = (self.space_env - nf).max(0.0);
        // W7AY SNR-optimized ATC (fldigi's "v3"): channel amplitudes weight
        // the clipped detector outputs, so the stronger-SNR channel dominates
        // the decision under selective fading.
        let v3 = (mclipped - nf) * me - (sclipped - nf) * se - 0.25 * (me * me - se * se);
        let bit = v3 > 0.0;
        // Soft metric: |v3| against its full-scale value for the stronger
        // channel (a solid single-tone bit scores 0.75·env²).
        let norm = 0.75 * me.max(se) * me.max(se);
        let conf = if norm > 1e-12 {
            (v3.abs() / norm).min(1.0)
        } else {
            0.0
        };
        if self.bits[self.ring_pos] {
            self.ring_marks -= 1;
        }
        self.bits[self.ring_pos] = bit;
        self.confs[self.ring_pos] = conf;
        if bit {
            self.ring_marks += 1;
        }
        self.ring_pos = (self.ring_pos + 1) % self.symlen;
        self.hist[self.hist_pos] = m;
        self.hist_pos = (self.hist_pos + 1) % self.hist.len();
        self.clock_tick(out);
    }

    /// fldigi `rtty::rx` — the async state machine, one sample per tick.
    fn clock_tick(&mut self, out: &mut Vec<DecodedChar>) {
        self.state = match self.state {
            RxState::Idle => {
                // Straddle-point start detect: a mark→space edge across the
                // one-symbol ring, balanced about its middle. `correction`
                // then walks the sampling point onto the start bit's center.
                let correction = self.ring_marks;
                let tol = (self.symlen / 29).max(2) as i32; // fldigi: <6 @ symlen 176
                if self.oldest()
                    && !self.newest()
                    && (self.symlen as i32 / 2 - correction as i32).abs() < tol
                {
                    RxState::Start {
                        counter: correction,
                    }
                } else {
                    RxState::Idle
                }
            }
            RxState::Start { counter } => {
                let counter = counter - 1;
                if counter > 0 {
                    RxState::Start { counter }
                } else if !self.mid_is_mark() {
                    // Confirmed space at the start bit's center.
                    RxState::Data {
                        counter: self.symlen,
                        bitcnt: 0,
                        data: 0,
                        cmin: self.mid_conf(),
                    }
                } else {
                    RxState::Idle
                }
            }
            RxState::Data {
                counter,
                bitcnt,
                data,
                cmin,
            } => {
                let counter = counter - 1;
                if counter > 0 {
                    RxState::Data {
                        counter,
                        bitcnt,
                        data,
                        cmin,
                    }
                } else {
                    let data = data | ((self.mid_is_mark() as u8) << bitcnt);
                    let cmin = cmin.min(self.mid_conf());
                    if bitcnt + 1 == 5 {
                        RxState::Stop {
                            counter: self.symlen,
                            data,
                            cmin,
                        }
                    } else {
                        RxState::Data {
                            counter: self.symlen,
                            bitcnt: bitcnt + 1,
                            data,
                            cmin,
                        }
                    }
                }
            }
            RxState::Stop {
                counter,
                data,
                cmin,
            } => {
                let counter = counter - 1;
                if counter > 0 {
                    RxState::Stop {
                        counter,
                        data,
                        cmin,
                    }
                } else {
                    if self.mid_is_mark() {
                        // Valid stop bit → a real frame (idle LTRS included:
                        // the AFC deliberately tracks through diddle).
                        let cmin = cmin.min(self.mid_conf());
                        self.frame_done(data, cmin, out);
                    }
                    RxState::Idle
                }
            }
        };
    }

    fn frame_done(&mut self, data: u8, cmin: f32, out: &mut Vec<DecodedChar>) {
        self.update_afc();
        if let Some(ch) = self.baudot.decode(data) {
            // CR and LF both render as newline, collapsed — fldigi likewise
            // suppresses the doubled CR CR / LF LF contest software sends.
            let ch = if ch == '\r' { '\n' } else { ch };
            if ch == '\n' && self.last_emitted == '\n' {
                return;
            }
            self.last_emitted = ch;
            out.push(DecodedChar {
                ch,
                confidence: cmin.clamp(0.0, 1.0),
            });
        }
    }

    /// Phase-difference AFC, measured over the mark history ending in the stop
    /// bit (mark held): a tone offset δ rotates the baseband by 2πδ/sr per
    /// sample. Coherent sum of conj(hᵢ)·hᵢ₊₁ weights strong samples naturally.
    fn update_afc(&mut self) {
        if !self.cfg.afc || self.afc_frozen {
            return;
        }
        let n = self.hist.len();
        let mut acc = Complex32::new(0.0, 0.0);
        for i in 0..n - 1 {
            let a = self.hist[(self.hist_pos + i) % n];
            let b = self.hist[(self.hist_pos + i + 1) % n];
            acc += a.conj() * b;
        }
        let delta = acc.im.atan2(acc.re) * SAMPLE_RATE / (2.0 * std::f32::consts::PI);
        if delta.abs() > self.cfg.baud / 2.0 {
            // Out of the believable pull range for one frame — junk (fldigi
            // zeroes these too).
            self.lock_run = 0;
            return;
        }
        // Proportional walk; the offset itself is the integrator. Clamped so
        // the AFC can never cross onto the neighboring tone.
        let max_off = 0.45 * (self.cfg.space_hz - self.cfg.mark_hz).abs();
        self.afc_offset = (self.afc_offset + 0.35 * delta).clamp(-max_off, max_off);
        // Acquire-then-freeze: several consecutive near-zero errors = locked.
        if delta.abs() < 1.5 {
            self.lock_run += 1;
            if self.lock_run >= 8 {
                self.afc_frozen = true;
            }
        } else {
            self.lock_run = 0;
        }
    }
}

impl RttyDemod for RttyDemodulator {
    fn feed(&mut self, samples: &[f32]) -> Vec<DecodedChar> {
        let mut out = Vec::new();
        let mut mout = [Complex32::new(0.0, 0.0); FLEN2];
        let mut sout = [Complex32::new(0.0, 0.0); FLEN2];
        for &x in samples {
            let mark_f = (self.cfg.mark_hz + self.afc_offset) as f64;
            let space_f = (self.cfg.space_hz + self.afc_offset) as f64;
            let zm = Self::mix(&mut self.mark_phase, mark_f, x);
            let zs = Self::mix(&mut self.space_phase, space_f, x);
            let m_ready = self.mark_filt.run(zm, &mut mout);
            let s_ready = self.space_filt.run(zs, &mut sout);
            debug_assert_eq!(m_ready, s_ready); // filters run in lockstep
            if m_ready {
                for i in 0..FLEN2 {
                    self.process(mout[i], sout[i], &mut out);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rtty::baudot::{BaudotEncoder, LTRS};

    /// Phase-continuous AFSK generator (single NCO, frequency switched per
    /// bit): 1 start (space) + 5 data LSB-first + 1.5 stop (mark). Fractional
    /// bit lengths accumulate in f64 sample time — true 45.45 baud.
    fn afsk(codes: &[u8], cfg: &RttyConfig, offset_hz: f32) -> Vec<f32> {
        let sr = SAMPLE_RATE as f64;
        let bit_len = sr / cfg.baud as f64;
        let mark = (cfg.mark_hz + offset_hz) as f64;
        let space = (cfg.space_hz + offset_hz) as f64;
        let mut out = Vec::new();
        let mut phase = 0.0f64;
        let mut t_end = 0.0f64;
        let push_bit =
            |out: &mut Vec<f32>, phase: &mut f64, t_end: &mut f64, bit: bool, len: f64| {
                *t_end += len * bit_len;
                let f = if bit { mark } else { space };
                while (out.len() as f64) < *t_end {
                    *phase += 2.0 * PI * f / sr;
                    if *phase > 2.0 * PI {
                        *phase -= 2.0 * PI;
                    }
                    out.push(phase.sin() as f32);
                }
            };
        for &c in codes {
            push_bit(&mut out, &mut phase, &mut t_end, false, 1.0);
            for i in 0..5 {
                push_bit(&mut out, &mut phase, &mut t_end, (c >> i) & 1 == 1, 1.0);
            }
            push_bit(&mut out, &mut phase, &mut t_end, true, 1.5);
        }
        out
    }

    /// A realistic over: leading silence, LTRS diddle preamble, the text,
    /// trailing diddle + silence (flushes the FFT blocks and state machine).
    fn signal(text: &str, cfg: &RttyConfig, diddles: usize, offset_hz: f32) -> Vec<f32> {
        let mut codes = vec![LTRS; diddles];
        codes.extend(BaudotEncoder::new(cfg.usos).encode(text));
        codes.extend([LTRS, LTRS]);
        let mut audio = vec![0.0f32; 1200];
        audio.extend(afsk(&codes, cfg, offset_hz));
        audio.extend(vec![0.0f32; 2400]);
        audio
    }

    /// Deterministic AWGN at `snr_db` referenced to the ham-standard 3 kHz
    /// bandwidth (white noise over the full 6 kHz Nyquist band carries twice
    /// the power of its in-3-kHz share). Box–Muller over xorshift64.
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
    fn decode_all(demod: &mut RttyDemodulator, audio: &[f32]) -> Vec<DecodedChar> {
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

    const MSG: &str = "CQ CQ DE KD9TAW";

    #[test]
    fn clean_copy_is_exact() {
        let cfg = RttyConfig::default();
        let mut demod = RttyDemodulator::new(cfg);
        let chars = decode_all(&mut demod, &signal(MSG, &cfg, 4, 0.0));
        let text = text_of(&chars);
        assert!(text.contains(MSG), "clean copy: {text:?}");
        assert!(chars.iter().all(|d| (0.0..=1.0).contains(&d.confidence)));
        let mean = chars.iter().map(|d| d.confidence).sum::<f32>() / chars.len() as f32;
        assert!(mean > 0.5, "clean confidence should be high, got {mean}");
    }

    #[test]
    fn figures_and_shifts_survive_the_full_dsp() {
        let cfg = RttyConfig::default();
        let mut demod = RttyDemodulator::new(cfg);
        let text = text_of(&decode_all(
            &mut demod,
            &signal("UR RST 599 599 HW? BK", &cfg, 4, 0.0),
        ));
        assert!(text.contains("UR RST 599 599 HW? BK"), "got {text:?}");
    }

    #[test]
    fn seventy_five_baud_copies_clean() {
        let cfg = RttyConfig {
            baud: 75.0,
            ..RttyConfig::default()
        };
        let mut demod = RttyDemodulator::new(cfg);
        let text = text_of(&decode_all(&mut demod, &signal(MSG, &cfg, 4, 0.0)));
        assert!(text.contains(MSG), "75 baud copy: {text:?}");
    }

    #[test]
    fn chunked_feed_matches_state_carry() {
        // Same audio through 479-sample chunks vs one call — identical output
        // proves all state carries across feed() boundaries.
        let cfg = RttyConfig::default();
        let audio = signal(MSG, &cfg, 4, 0.0);
        let mut d1 = RttyDemodulator::new(cfg);
        let mut d2 = RttyDemodulator::new(cfg);
        assert_eq!(decode_all(&mut d1, &audio), d2.feed(&audio));
    }

    #[test]
    fn awgn_10db_copies_majority() {
        let cfg = RttyConfig::default();
        let mut audio = signal(MSG, &cfg, 6, 0.0);
        add_awgn(&mut audio, 10.0, 0x2545F4914F6CDD1D);
        let mut demod = RttyDemodulator::new(cfg);
        let text = text_of(&decode_all(&mut demod, &audio));
        let acc = accuracy(MSG, &text);
        assert!(acc >= 0.7, "10 dB copy accuracy {acc} ({text:?})");
    }

    #[test]
    fn awgn_5db_still_yields_partial_copy() {
        let cfg = RttyConfig::default();
        let mut audio = signal(MSG, &cfg, 6, 0.0);
        add_awgn(&mut audio, 5.0, 0x9E3779B97F4A7C15);
        let mut demod = RttyDemodulator::new(cfg);
        let text = text_of(&decode_all(&mut demod, &audio));
        let acc = accuracy(MSG, &text);
        assert!(acc >= 0.3, "5 dB partial copy accuracy {acc} ({text:?})");
    }

    #[test]
    fn confidence_degrades_with_noise() {
        let cfg = RttyConfig::default();
        let mean_conf = |snr: Option<f32>| {
            let mut audio = signal(MSG, &cfg, 6, 0.0);
            if let Some(db) = snr {
                add_awgn(&mut audio, db, 0xDEADBEEFCAFE1234);
            }
            let chars = decode_all(&mut RttyDemodulator::new(cfg), &audio);
            assert!(!chars.is_empty());
            chars.iter().map(|d| d.confidence).sum::<f32>() / chars.len() as f32
        };
        let clean = mean_conf(None);
        let noisy = mean_conf(Some(5.0));
        assert!(
            noisy < clean,
            "soft metric must fall with SNR: clean {clean}, 5 dB {noisy}"
        );
    }

    #[test]
    fn afc_acquires_an_offset_signal_then_freezes() {
        let cfg = RttyConfig::default();
        // +12 Hz off-tune with a diddle preamble for the AFC to chew on.
        let audio = signal(MSG, &cfg, 14, 12.0);
        let mut demod = RttyDemodulator::new(cfg);
        let text = text_of(&decode_all(&mut demod, &audio));
        assert!(
            (demod.afc_offset_hz() - 12.0).abs() < 4.0,
            "AFC should walk to ≈+12 Hz, got {}",
            demod.afc_offset_hz()
        );
        assert!(demod.afc_locked(), "AFC should freeze after acquiring");
        let acc = accuracy(MSG, &text);
        assert!(acc >= 0.8, "off-tune copy after AFC: {acc} ({text:?})");
    }

    #[test]
    fn frozen_afc_never_walks_onto_a_neighbor() {
        let cfg = RttyConfig::default();
        let mut demod = RttyDemodulator::new(cfg);
        decode_all(&mut demod, &signal(MSG, &cfg, 14, 0.0));
        assert!(demod.afc_locked());
        let locked_at = demod.afc_offset_hz();
        // A second, much stronger off-frequency signal appears mid-QSO — the
        // MMTTY gotcha. Frozen AFC must not move.
        let mut neighbor = signal(MSG, &cfg, 14, 60.0);
        for x in neighbor.iter_mut() {
            *x *= 2.0;
        }
        decode_all(&mut demod, &neighbor);
        assert_eq!(demod.afc_offset_hz(), locked_at);
    }

    #[test]
    fn afc_disabled_stays_put() {
        let cfg = RttyConfig {
            afc: false,
            ..RttyConfig::default()
        };
        let mut demod = RttyDemodulator::new(cfg);
        decode_all(&mut demod, &signal(MSG, &cfg, 8, 12.0));
        assert_eq!(demod.afc_offset_hz(), 0.0);
        assert!(!demod.afc_locked());
    }

    #[test]
    fn noise_only_emits_little_and_reset_reacquires() {
        let cfg = RttyConfig::default();
        // Pure band noise at a real level: the start-bit straddle test plus
        // the stop-bit check reject nearly all of it (no squelch layer yet).
        let mut seed = 0x0123456789ABCDEFu64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let noise: Vec<f32> = (0..(SAMPLE_RATE * 5.0) as usize)
            .map(|_| {
                let g = (-2.0 * rnd().max(1e-12).ln()).sqrt()
                    * (2.0 * std::f64::consts::PI * rnd()).cos();
                0.2 * g as f32
            })
            .collect();
        let mut demod = RttyDemodulator::new(cfg);
        let chars = decode_all(&mut demod, &noise);
        assert!(chars.len() < 40, "noise-only chatter: {}", chars.len());
        demod.reset();
        assert_eq!(demod.afc_offset_hz(), 0.0);
        assert!(!demod.afc_locked());
    }

    #[test]
    #[ignore] // diagnostic: cargo test -p tempo-core margins -- --ignored --nocapture
    fn margins() {
        for snr in [20.0f32, 10.0, 5.0, 2.0, 0.0, -2.0] {
            let cfg = RttyConfig::default();
            let mut audio = signal(MSG, &cfg, 6, 0.0);
            add_awgn(&mut audio, snr, 0x2545F4914F6CDD1D);
            let chars = decode_all(&mut RttyDemodulator::new(cfg), &audio);
            let text = text_of(&chars);
            let conf = chars.iter().map(|d| d.confidence).sum::<f32>() / chars.len().max(1) as f32;
            println!(
                "snr {snr:>5} dB  acc {:.2}  conf {conf:.2}  {text:?}",
                accuracy(MSG, &text)
            );
        }
    }
}
