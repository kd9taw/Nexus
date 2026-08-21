//! QPSK31 — the FEC layer of the PSK31 pair (Keyboard Modes Phase 3), written
//! from scratch for Nexus (no ported code; the constants below are interop
//! facts derived from the published reference implementations, recorded with
//! their sources).
//!
//! G3PLX QPSK31 is BPSK31's varicode stream passed through a rate-1/2, K=5
//! convolutional encoder whose 2-bit output selects one of four DIFFERENTIAL
//! phase shifts per 31.25 Bd symbol — same baud, same varicode, same occupied
//! bandwidth, one info bit per symbol (rate 1/2 × 2 bits/symbol), so the
//! character pacing is IDENTICAL to BPSK31 and the shared
//! `keyboard::PSK31` descriptor (frame_bits, look-ahead, ceiling) serves both
//! modes unchanged. What the code buys is error CORRECTION: a single wrong
//! symbol decision — the flutter/QSB hit that prints a wrong character in
//! BPSK — is corrected outright by the Viterbi decoder.
//!
//! ## The wire contract (the interop trap — sources, verified 2026-08-17)
//!
//! * **Code**: rate 1/2, K=5, generator polynomials `0x17`/`0x19`
//!   (fldigi `src/psk/psk.cxx`: `#define K 5`, `#define POLY1 0x17`,
//!   `#define POLY2 0x19`) — the classic maximal-free-distance (23,35)-octal
//!   K=5 pair written LSB-first. Encoder register: newest data bit shifts into
//!   bit 0 (fldigi `viterbi.cxx`: `shreg = (shreg << 1) | bit`), output
//!   `s = parity(reg & 0x17) | parity(reg & 0x19) << 1`.
//! * **Symbol → phase shift** (audio-domain phase ADVANCE per symbol
//!   boundary, USB/"normal"): `s=0 → 180°`, `s=1 → −90°`, `s=2 → 0°`,
//!   `s=3 → +90°`. Derived twice, independently, and they agree: (a) fldigi's
//!   `tx_bit`/`tx_carriers` chain (`sym = (4−s)&3` for non-reverse, ×4 into
//!   its 16-PSK vector table, synthesized as `I·cos + Q·sin`, i.e. the
//!   conjugate convention); (b) AE4JY's PSKCore (`PSKMod.cpp`), whose
//!   `ConvolutionCodeTable[32]` equals [`conv_out`]'s table with the top bit
//!   XOR'd (fldigi's own "top bit is flipped" note) and whose
//!   `SYM_P90`-style actions were traced through its `I·sin + Q·cos`
//!   synthesis to audio-domain angles. The [`tests::the_encoder_matches_the_published_convolution_table`]
//!   test pins table-for-table equivalence with PSKCore's published constant.
//! * **Structural cross-checks** (why this table is self-evidently the G3PLX
//!   design): continuous idle (all-zero data) drives the register to 0 →
//!   `s=0` → continuous 180° reversals — EXACTLY the BPSK31 idle, which is
//!   why a QPSK31 idle looks and sounds like a BPSK31 idle. Continuous ones
//!   drive it to 31 → `s=2` → steady carrier — exactly BPSK31's mark.
//! * **Sideband polarity**: QPSK31 is sideband-SENSITIVE (LSB conjugates the
//!   spectrum, so ±90° swap while 0°/180° are unaffected — the reason BPSK31
//!   doesn't care). PSKCore's QPSKL mode swaps `SYM_P90 ↔ SYM_M90`, and this
//!   module's `reverse` flag does the identical swap on TX and RX. The
//!   convention shipped here: **normal = USB** (the Keyboard section always
//!   runs USB); the cockpit's Reverse toggle is for working a
//!   deliberately-LSB station.
//! * **End of transmission**: fldigi's `tx_flush` keys 32 zero bits
//!   (`dcdbits`) through the encoder for QPSK before dropping carrier;
//!   [`QpskStream`]'s tail does the same ([`QPSK_FLUSH_BITS`]), which both
//!   flushes the encoder register and drains the far decoder's traceback
//!   before the carrier disappears.
//!
//! ## The decoder (soft Viterbi)
//!
//! [`Viterbi`] is a 16-state soft-decision decoder over that trellis. Branch
//! metrics are genuinely soft — `1 − cos(Δφ)` against each of the four ideal
//! shifts, computed from the differential symbol `d` without trig
//! (`±Re/|d|`, `±Im/|d|`) — rather than fldigi's hard 0/255 per-bit slices.
//! Survivor paths use register exchange in a `u64` per state with a traceback
//! depth of [`TRACEBACK`] = 64 symbols (~2 s at 31.25 Bd): comfortably past
//! the 5·K textbook floor, matching fldigi's shipped default (`_traceback =
//! k·12` = 60 for K=5), and exactly a `u64`'s width. [`Viterbi::flush`]
//! empties the survivor register when the carrier drops (squelch close), so
//! the tail of an over — the last thing the far station typed — is decoded
//! rather than stranded in the traceback.
//!
//! Per-character confidence: the Viterbi decorrelates output bits from any
//! single symbol, so the per-bit confidence handed to varicode is a
//! slow-averaged AXIS MARGIN of the received differential symbols (1 on a
//! constellation axis, 0 at the 45° decision boundary) — an honest smoothed
//! copy-quality reading rather than a fabricated per-bit posterior.

use super::demod::BAUD;
use super::modulator::{PskStream, PskTxConfig, PREAMBLE_IDLE_BITS};
use super::varicode::encode_bits;

/// Convolutional generator polynomials (K=5, rate 1/2) — fldigi psk.cxx's
/// `POLY1`/`POLY2`, the (23,35)-octal classic pair in LSB-first bit order.
pub const QPSK_POLY1: u8 = 0x17;
pub const QPSK_POLY2: u8 = 0x19;

/// Constraint length.
pub const QPSK_K: u32 = 5;

/// Viterbi survivor (traceback) depth in symbols — see the module header for
/// the derivation (5·K floor, fldigi's k·12 default, one `u64`).
const TRACEBACK: u32 = 64;

/// Zero bits keyed through the encoder when a transmission ends (the fldigi
/// `dcdbits` postamble): flushes the encoder register AND pushes the last
/// real characters through the far decoder's traceback before key-down.
pub const QPSK_FLUSH_BITS: usize = 32;

/// `parity(x)` as a 0/1 bit, const-evaluable.
const fn parity(x: u8) -> u8 {
    (x.count_ones() & 1) as u8
}

/// The 32-entry encoder output table: `OUT[reg]` for the 5-bit register with
/// the NEWEST data bit in bit 0 — `bit0 = parity(reg & POLY1)`,
/// `bit1 = parity(reg & POLY2)` (the fldigi convention; PSKCore's published
/// `ConvolutionCodeTable` is this with bit 1 inverted — pinned by test).
const fn build_out() -> [u8; 32] {
    let mut out = [0u8; 32];
    let mut i = 0usize;
    while i < 32 {
        out[i] = parity(i as u8 & QPSK_POLY1) | (parity(i as u8 & QPSK_POLY2) << 1);
        i += 1;
    }
    out
}

const OUT: [u8; 32] = build_out();

/// Encoder output for a 5-bit register value (newest bit in bit 0).
pub const fn conv_out(reg: u8) -> u8 {
    OUT[(reg & 0x1F) as usize]
}

/// The unit complex rotation one encoder symbol applies at a boundary, in the
/// modulator's convention (rotating (I,Q) by +j advances the audio phase
/// +90° — see `PskStream::render_pending`). `reverse` = the LSB polarity:
/// ±90° swap, 0°/180° unchanged.
fn shift_rot(sym: u8, reverse: bool) -> (f64, f64) {
    match (sym & 3, reverse) {
        (0, _) => (-1.0, 0.0),                 // 180°
        (2, _) => (1.0, 0.0),                  // 0°
        (1, false) | (3, true) => (0.0, -1.0), // −90°
        _ => (0.0, 1.0),                       // +90°
    }
}

/// The K=5 convolutional encoder — five data bits of history, newest in
/// bit 0. Starts all-zero (= idle), which is also what [`QPSK_FLUSH_BITS`]
/// zeros return it to at the end of an over.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConvEncoder {
    reg: u8,
}

impl ConvEncoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shift one data bit in, return the 2-bit channel symbol.
    pub fn encode(&mut self, bit: bool) -> u8 {
        self.reg = (self.reg << 1) | u8::from(bit);
        conv_out(self.reg)
    }
}

/// A RESUMABLE QPSK31 generator — [`PskStream`]'s machinery (carrier phase,
/// fractional symbol clock, shaped boundaries, chunk-invisibility) with the
/// K=5 encoder in front of it, its register carried across chunks exactly as
/// the carrier phase is. Same contract as the BPSK stream: `chunk(bits,
/// tail)` takes VARICODE bits, one symbol per bit; `tail` keys the
/// [`QPSK_FLUSH_BITS`] encoder flush and closes through the shaped key-down.
/// A tailed stream is finished — drop it.
pub struct QpskStream {
    gen: PskStream,
    enc: ConvEncoder,
    reverse: bool,
}

impl QpskStream {
    pub fn new(cfg: PskTxConfig, reverse: bool) -> Self {
        Self {
            gen: PskStream::new(cfg),
            enc: ConvEncoder::new(),
            reverse,
        }
    }

    /// Render the next chunk of varicode bits; `tail` closes the transmission
    /// (encoder flush + shaped key-down). n bits ⇒ n symbols; the tail adds
    /// [`QPSK_FLUSH_BITS`] + 1 more.
    pub fn chunk(&mut self, bits: &[bool], tail: bool) -> Vec<f32> {
        // Capacity estimate only — the generator's fractional clock is the
        // truth about sample counts.
        let spb = super::demod::SAMPLE_RATE as f64 / BAUD as f64;
        let mut out = Vec::with_capacity(
            ((bits.len() + 1 + if tail { QPSK_FLUSH_BITS } else { 0 }) as f64 * spb) as usize + 1,
        );
        for &b in bits {
            let rot = shift_rot(self.enc.encode(b), self.reverse);
            self.gen.push_rotation(rot, &mut out);
        }
        if tail {
            for _ in 0..QPSK_FLUSH_BITS {
                let rot = shift_rot(self.enc.encode(false), self.reverse);
                self.gen.push_rotation(rot, &mut out);
            }
            self.gen.close(&mut out);
        }
        out
    }
}

/// The wire bits of one one-shot QPSK31 over: the idle preamble the far end
/// acquires (sync, AFC, squelch, and the Viterbi's traceback warm-up), then
/// the varicode text. The POSTAMBLE is not here — it is the stream tail's
/// [`QPSK_FLUSH_BITS`] encoder flush, so the on-air shape matches BPSK's
/// (`psk_over_bits`) idle-for-idle without double-counting.
pub fn qpsk_over_bits(text: &str) -> Vec<bool> {
    let mut bits = vec![false; PREAMBLE_IDLE_BITS];
    bits.extend(encode_bits(text));
    bits
}

/// Render a whole QPSK31 transmission at once — one chunk through
/// [`QpskStream`], so the one-shot path and the latched path cannot drift
/// apart (the `bpsk_samples` rule).
pub fn qpsk_samples(bits: &[bool], cfg: &PskTxConfig, reverse: bool) -> Vec<f32> {
    QpskStream::new(*cfg, reverse).chunk(bits, true)
}

/// Soft branch costs for one received differential symbol `d = (re, im)`
/// (unit-normalized by the caller): `costs[s] = 1 − cos(φ − θ_s)` for each of
/// the four ideal shifts, computed without trig. `reverse` conjugates the
/// measurement (the LSB polarity, RX side).
pub(crate) fn shift_costs(re: f32, im: f32, reverse: bool) -> [f32; 4] {
    let im = if reverse { -im } else { im };
    // cos-similarity to each shift: s=0 (180°) → −re, s=1 (−90°) → −im,
    // s=2 (0°) → +re, s=3 (+90°) → +im.
    [1.0 + re, 1.0 + im, 1.0 - re, 1.0 - im]
}

/// Axis margin of a unit-normalized differential symbol: 1 exactly on a
/// constellation axis, 0 at the 45° decision boundary — the QPSK analogue of
/// the BPSK slicer's `|Re d|/|d|` phase-error margin.
pub(crate) fn axis_margin(re: f32, im: f32) -> f32 {
    const COS45: f32 = std::f32::consts::FRAC_1_SQRT_2;
    ((re.abs().max(im.abs()) - COS45) / (1.0 - COS45)).clamp(0.0, 1.0)
}

/// 16-state soft-decision Viterbi decoder for the K=5 trellis, register-
/// exchange survivors (one `u64` per state = the [`TRACEBACK`] window).
///
/// State = the last 4 data bits, newest in bit 0. A transition on input `b`
/// from state `σ` forms the register `(σ<<1)|b`; its emitted symbol indexes
/// the branch cost, and the successor state is `((σ<<1)|b) & 0xF`.
pub struct Viterbi {
    /// Accumulated path metric per state (lower = better), re-zeroed against
    /// the running minimum each step so it never grows unbounded.
    metrics: [f32; 16],
    /// Register-exchange survivor bits per state (newest decision in bit 0).
    paths: [u64; 16],
    /// Symbols consumed since reset, saturating at [`TRACEBACK`].
    depth: u32,
}

impl Default for Viterbi {
    fn default() -> Self {
        Self::new()
    }
}

impl Viterbi {
    pub fn new() -> Self {
        Self {
            metrics: [0.0; 16],
            paths: [0; 16],
            depth: 0,
        }
    }

    /// Fresh acquisition (squelch close handled via [`Self::flush`] first).
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Consume one received symbol's four branch costs; once the traceback
    /// window has filled, emit the data bit decided [`TRACEBACK`] symbols
    /// ago along the best survivor.
    pub fn step(&mut self, costs: [f32; 4]) -> Option<bool> {
        let mut nm = [0.0f32; 16];
        let mut np = [0u64; 16];
        for (n, (nm_n, np_n)) in nm.iter_mut().zip(np.iter_mut()).enumerate() {
            // Predecessors of state n: (n>>1) and (n>>1)|8; the emitted
            // symbols on those transitions are OUT[n] and OUT[n|16] (the
            // register is the state plus its dropped oldest bit).
            let p0 = n >> 1;
            let p1 = p0 | 8;
            let m0 = self.metrics[p0] + costs[OUT[n] as usize];
            let m1 = self.metrics[p1] + costs[OUT[n | 16] as usize];
            let (m, p) = if m0 <= m1 { (m0, p0) } else { (m1, p1) };
            *nm_n = m;
            *np_n = (self.paths[p] << 1) | (n & 1) as u64;
        }
        // Normalize so the metrics stay small forever.
        let min = nm.iter().fold(f32::INFINITY, |a, &b| a.min(b));
        for (m, v) in self.metrics.iter_mut().zip(nm) {
            *m = v - min;
        }
        self.paths = np;
        self.depth = self.depth.saturating_add(1).min(TRACEBACK);
        (self.depth >= TRACEBACK)
            .then(|| (self.paths[self.best_state()] >> (TRACEBACK - 1)) & 1 == 1)
    }

    /// Drain every not-yet-emitted decision along the best survivor, oldest
    /// first — called when the carrier drops so the far station's last
    /// characters decode instead of stranding in the traceback.
    pub fn flush(&mut self) -> Vec<bool> {
        let k = self.depth.min(TRACEBACK - 1);
        let path = self.paths[self.best_state()];
        (0..k).rev().map(|i| (path >> i) & 1 == 1).collect()
    }

    fn best_state(&self) -> usize {
        let mut best = 0;
        for s in 1..16 {
            if self.metrics[s] < self.metrics[best] {
                best = s;
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::psk::demod::{PskConfig, PskDemodulator, PskModeKind, SAMPLE_RATE};
    use crate::psk::modulator::TX_DRIVE;
    use crate::textmode::{DecodedChar, TextDemod};

    /// AE4JY's PSKCore `ConvolutionCodeTable[32]` (PSKMod.cpp / PSKTables.h,
    /// the published QPSK31 reference constant), where the entry is the
    /// phase-shift ACTION code {0 = no change, 1 = +90°, 2 = 180°, 3 = −90°}
    /// for the 5-bit register. Quoted verbatim.
    const PSKCORE_TABLE: [u8; 32] = [
        2, 1, 3, 0, 3, 0, 2, 1, 0, 3, 1, 2, 1, 2, 0, 3, 1, 2, 0, 3, 0, 3, 1, 2, 3, 0, 2, 1, 2, 1,
        3, 0,
    ];

    #[test]
    fn the_encoder_matches_the_published_convolution_table() {
        // THE INTEROP PIN. Our encoder symbol s maps to the air shift
        // {0:180°, 1:−90°, 2:0°, 3:+90°}; PSKCore's action code maps
        // {0:0°, 1:+90°, 2:180°, 3:−90°}. Angle-for-angle those two
        // conventions coincide exactly when s = code XOR 2 — fldigi's own
        // "top bit is flipped" relationship. One table equality therefore
        // pins polynomials, register direction AND the shift assignment
        // against the published reference constant.
        for reg in 0..32u8 {
            assert_eq!(
                conv_out(reg),
                PSKCORE_TABLE[reg as usize] ^ 2,
                "register {reg:05b}"
            );
        }
    }

    #[test]
    fn idle_and_steady_mark_reduce_to_the_bpsk_extremes() {
        // Idle (all zeros) = continuous 180° reversals — the BPSK31 idle;
        // steady ones = 0° = steady carrier — the BPSK31 mark. The G3PLX
        // design's structural cross-check, and what makes a QPSK31 idle
        // indistinguishable from BPSK31 on the waterfall.
        assert_eq!(conv_out(0b00000), 0, "idle must be the 180° symbol");
        assert_eq!(conv_out(0b11111), 2, "steady ones must be the 0° symbol");
        assert_eq!(shift_rot(0, false), (-1.0, 0.0));
        assert_eq!(shift_rot(2, false), (1.0, 0.0));
        // The sideband swap touches ONLY the ±90° pair.
        assert_eq!(shift_rot(1, false), shift_rot(3, true));
        assert_eq!(shift_rot(3, false), shift_rot(1, true));
        assert_eq!(shift_rot(0, false), shift_rot(0, true));
        assert_eq!(shift_rot(2, false), shift_rot(2, true));
    }

    /// Hard costs for a known symbol: 0 for the sent one, 2 for the rest
    /// (the clean-channel extreme of the soft metric).
    fn hard_costs(sym: u8) -> [f32; 4] {
        let mut c = [2.0f32; 4];
        c[sym as usize] = 0.0;
        c
    }

    /// Encode `data` and run the symbol stream through a decoder, returning
    /// the decoded bits (flushed at the end).
    fn decode_symbols(syms: &[u8]) -> Vec<bool> {
        let mut vit = Viterbi::new();
        let mut out = Vec::new();
        for &s in syms {
            if let Some(b) = vit.step(hard_costs(s)) {
                out.push(b);
            }
        }
        out.extend(vit.flush());
        out
    }

    fn encode_all(data: &[bool]) -> Vec<u8> {
        let mut enc = ConvEncoder::new();
        data.iter().map(|&b| enc.encode(b)).collect()
    }

    /// Deterministic xorshift bit/float source for the tests.
    fn xorshift(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    #[test]
    fn viterbi_roundtrips_a_clean_symbol_stream() {
        // Known input → encoder → decoder → the same bits, exactly (the
        // encoder-known-output obligation, both directions of the trellis).
        let mut seed = 0xDEADBEEFCAFE1234u64;
        let data: Vec<bool> = (0..200).map(|_| xorshift(&mut seed) & 1 == 1).collect();
        let got = decode_symbols(&encode_all(&data));
        assert!(got.len() >= data.len(), "decoder lost bits");
        assert_eq!(&got[..data.len()], &data[..], "clean roundtrip differs");
    }

    #[test]
    fn viterbi_corrects_a_single_symbol_error() {
        // THE WHOLE POINT OF QPSK31: one wrong symbol decision — which in
        // BPSK31 is a wrong printed character — decodes to the exact sent
        // bits. d_free = 7 for this code; a single hard error is well inside
        // its correction radius wherever it lands.
        let mut seed = 0x9E3779B97F4A7C15u64;
        let data: Vec<bool> = (0..120).map(|_| xorshift(&mut seed) & 1 == 1).collect();
        let clean = encode_all(&data);
        for hit in [7usize, 40, 60, 100] {
            let mut syms = clean.clone();
            syms[hit] = (syms[hit] + 1) & 3; // a 90°-neighbor mis-slice
            let got = decode_symbols(&syms);
            assert_eq!(
                &got[..data.len()],
                &data[..],
                "single symbol error at {hit} was not corrected"
            );
        }
    }

    #[test]
    fn viterbi_degrades_gracefully_under_a_burst() {
        // A burst past the correction radius must degrade (wrong bits) and
        // recover after it — never panic, never stay wedged. The bits before
        // the burst and well after it must still be exact.
        let mut seed = 0x0123456789ABCDEFu64;
        let data: Vec<bool> = (0..300).map(|_| xorshift(&mut seed) & 1 == 1).collect();
        let mut syms = encode_all(&data);
        for s in syms.iter_mut().skip(140).take(12) {
            *s = (*s + 2) & 3; // 12 symbols hit with the worst (180°) error
        }
        let got = decode_symbols(&syms);
        assert_eq!(got.len(), data.len(), "length drifted");
        assert_eq!(&got[..100], &data[..100], "pre-burst copy corrupted");
        assert_eq!(
            &got[220..],
            &data[220..],
            "decoder did not recover after the burst"
        );
    }

    // ----- TX→RX loopback: the SHIPPING QPSK modulator through the SHIPPING
    // demodulator in QPSK mode. Helpers mirror `super::demod::tests`
    // (deliberately duplicated: that module's test lines are pinned by the
    // Phase 1/2 zero-edit rule). -----

    const MSG: &str = "CQ CQ de KD9TAW KD9TAW pse k";

    fn qcfg(reverse: bool) -> PskConfig {
        PskConfig {
            mode: PskModeKind::Qpsk31,
            qpsk_reverse: reverse,
            ..PskConfig::default()
        }
    }

    /// A full one-shot QPSK over through the real modulator, with lead-in
    /// silence and a 2 s dead-air tail: the squelch reads dead air as
    /// no-signal, closes (~0.8 s at the 20-symbol constant), and the close
    /// drains the Viterbi traceback — the same path a real end-of-over
    /// takes against band noise.
    fn tx_over(text: &str, reverse: bool) -> Vec<f32> {
        let mut audio = vec![0.0f32; 1200];
        audio.extend(qpsk_samples(
            &qpsk_over_bits(text),
            &PskTxConfig::default(),
            reverse,
        ));
        audio.extend(vec![0.0f32; 24_000]);
        audio
    }

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

    /// LCS copy accuracy (the demod test convention).
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

    /// Deterministic AWGN at `snr_db` in the ham-standard 3 kHz bandwidth
    /// (the RTTY/PSK test convention).
    fn add_awgn(audio: &mut [f32], snr_db: f32, mut seed: u64) {
        let ps = audio.iter().map(|x| x * x).sum::<f32>() / audio.len() as f32;
        let sigma = (2.0 * ps / 10f32.powf(snr_db / 10.0)).sqrt();
        for x in audio.iter_mut() {
            let u1 = (xorshift(&mut seed) >> 11) as f64 / (1u64 << 53) as f64;
            let u2 = (xorshift(&mut seed) >> 11) as f64 / (1u64 << 53) as f64;
            let g = (-2.0 * u1.max(1e-12).ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
            *x += sigma * g as f32;
        }
    }

    fn white_noise(secs: f32, amp: f32, mut seed: u64) -> Vec<f32> {
        (0..(SAMPLE_RATE * secs) as usize)
            .map(|_| {
                let u1 = (xorshift(&mut seed) >> 11) as f64 / (1u64 << 53) as f64;
                let u2 = (xorshift(&mut seed) >> 11) as f64 / (1u64 << 53) as f64;
                let g =
                    (-2.0 * u1.max(1e-12).ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
                amp * g as f32
            })
            .collect()
    }

    #[test]
    fn qpsk_chunking_is_invisible() {
        // The stream contract extended to the encoder: however the varicode
        // bits are split across chunk calls, the samples AND the carried
        // encoder register agree with the one-shot render exactly.
        let cfg = PskTxConfig::default();
        let bits = qpsk_over_bits("cq de KD9TAW");
        let one_shot = qpsk_samples(&bits, &cfg, false);
        let mut s = QpskStream::new(cfg, false);
        let mut streamed = Vec::new();
        let chunks: Vec<&[bool]> = bits.chunks(7).collect();
        for (i, c) in chunks.iter().enumerate() {
            streamed.extend(s.chunk(c, i == chunks.len() - 1));
        }
        assert_eq!(streamed.len(), one_shot.len(), "chunking changed duration");
        let worst = streamed
            .iter()
            .zip(&one_shot)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-6, "chunking is not invisible: worst={worst}");
    }

    #[test]
    fn qpsk_envelope_dips_to_the_chord_on_90_and_zero_on_180() {
        // The shaping contract: ±90° boundaries pass through the chord
        // (≈ 0.707·drive, never zero), 180° through zero, and the drive
        // bounds every sample — the IMD discipline carried to four phases.
        let cfg = PskTxConfig::default();
        // Alternating ones after a preamble produce a mix of shifts; probe
        // targeted boundaries instead by hand-built symbol runs.
        let mut s = QpskStream::new(cfg, false);
        // Drive the encoder with bits chosen to hit a ±90 boundary: from
        // idle (reg 0), a single 1 gives reg=00001 → s=3 (+90°).
        let audio = s.chunk(&[false, false, false, true, false, false], true);
        let peak = audio.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
        assert!(peak <= TX_DRIVE + 1e-3, "drive exceeded: {peak}");
        // Boundary 4 (between symbols 4 and 5 of the stream, 0-indexed
        // samples at 384·k) is the +90° shift the `true` bit produced.
        let b = 4 * 384;
        let near: f32 = audio[b - 2..b + 2]
            .iter()
            .map(|x| x.abs())
            .fold(0.0, f32::max);
        assert!(
            (near - std::f32::consts::FRAC_1_SQRT_2 * TX_DRIVE).abs() < 0.08,
            "90° boundary envelope {near}, want ≈ chord {}",
            std::f32::consts::FRAC_1_SQRT_2 * TX_DRIVE
        );
        // Boundary 1 is idle→idle: a 180° reversal, through zero.
        let b = 384;
        let near: f32 = audio[b - 2..b + 2]
            .iter()
            .map(|x| x.abs())
            .fold(0.0, f32::max);
        assert!(
            near < 0.06,
            "180° boundary envelope {near} — not through zero"
        );
    }

    #[test]
    fn qpsk_tx_to_rx_loopback_decodes_verbatim() {
        // Clean channel: what the QPSK modulator keys is what the QPSK
        // demodulator prints — mixed case and punctuation included.
        let text = text_of(&decode_all(
            &mut PskDemodulator::new(qcfg(false)),
            &tx_over(MSG, false),
        ));
        assert!(text.contains(MSG), "loopback copy: {text:?}");
        let msg2 = "Name is Op, QTH: Madison [EN53] = 5w @ dipole; hw? k";
        let text2 = text_of(&decode_all(
            &mut PskDemodulator::new(qcfg(false)),
            &tx_over(msg2, false),
        ));
        assert!(text2.contains(msg2), "loopback copy: {text2:?}");
    }

    #[test]
    fn qpsk_loopback_holds_minus_8_db() {
        // THE STATED OPERATING POINT, measured not assumed: −8 dB in 3 kHz —
        // parity with the BPSK31 demod's pinned floor (the rate-1/2 coding
        // gain buys back the 90°-spacing loss at matched throughput; the
        // `margins` diagnostic on this seed shows full copy at −10 dB with
        // degradation from ~−12 dB, tracking BPSK's curve).
        let mut audio = tx_over(MSG, false);
        add_awgn(&mut audio, -8.0, 0x9E3779B97F4A7C15);
        let text = text_of(&decode_all(&mut PskDemodulator::new(qcfg(false)), &audio));
        let acc = accuracy(MSG, &text);
        assert!(acc >= 0.8, "-8 dB loopback accuracy {acc} ({text:?})");
    }

    /// Locate a shaped-through-zero (180°) symbol boundary in the text body
    /// of an over and negate everything from it on — a PHASE SLIP: the
    /// canonical single-symbol channel error (one differential flips, the
    /// rest of the transmission stays self-consistent, and slipping at a
    /// zero-envelope instant adds no click for the FIR to smear).
    fn phase_slip_mid_message(air: &mut [f32]) {
        let k = (80..140)
            .find(|k| air[k * 384 - 2..k * 384 + 2].iter().all(|x| x.abs() < 0.02))
            .expect("no 180° boundary found in the text body");
        for x in &mut air[k * 384..] {
            *x = -*x;
        }
    }

    #[test]
    fn a_phase_slip_on_the_wire_is_corrected_where_bpsk_prints_garbage() {
        // The coding gain END TO END, A/B against BPSK: a mid-message 180°
        // phase slip is exactly ONE symbol error — far inside this code's
        // d_free = 7 — so QPSK copies verbatim; the IDENTICAL slip on a
        // BPSK31 over flips a sliced bit and mangles the printed copy, which
        // is both the control proving the hit is real and the whole reason
        // QPSK31 exists.
        let mut qpsk_air = qpsk_samples(&qpsk_over_bits(MSG), &PskTxConfig::default(), false);
        phase_slip_mid_message(&mut qpsk_air);
        let mut full = vec![0.0f32; 1200];
        full.extend(qpsk_air);
        full.extend(vec![0.0f32; 24_000]);
        let text = text_of(&decode_all(&mut PskDemodulator::new(qcfg(false)), &full));
        assert!(
            text.contains(MSG),
            "the phase slip was not corrected: {text:?}"
        );

        // THE CONTROL: the same slip must corrupt an (uncoded) BPSK31 over,
        // or the assertion above proved nothing about correction.
        use crate::psk::modulator::{bpsk_samples, psk_over_bits};
        let mut bpsk_air = bpsk_samples(&psk_over_bits(MSG), &PskTxConfig::default());
        phase_slip_mid_message(&mut bpsk_air);
        let mut full = vec![0.0f32; 1200];
        full.extend(bpsk_air);
        full.extend(vec![0.0f32; 24_000]);
        let text = text_of(&decode_all(
            &mut PskDemodulator::new(PskConfig::default()),
            &full,
        ));
        assert!(
            !text.contains(MSG),
            "the control failed: the same slip left BPSK copy intact, so the \
             QPSK assertion measured nothing (got {text:?})"
        );
    }

    #[test]
    fn reversed_tx_decodes_only_with_the_reverse_toggle() {
        // THE INTEROP TRAP, both polarities. A reversed (LSB) transmission
        // into a normal decoder must NOT print the message; with the
        // decoder's Reverse toggle set it must print verbatim — and the
        // mirror pair likewise.
        let rev_air = tx_over(MSG, true);
        let text = text_of(&decode_all(&mut PskDemodulator::new(qcfg(false)), &rev_air));
        assert!(
            accuracy(MSG, &text) < 0.3,
            "reversed TX printed through a normal decoder: {text:?}"
        );
        let text = text_of(&decode_all(&mut PskDemodulator::new(qcfg(true)), &rev_air));
        assert!(
            text.contains(MSG),
            "reverse RX did not copy reverse TX: {text:?}"
        );
        let norm_air = tx_over(MSG, false);
        let text = text_of(&decode_all(&mut PskDemodulator::new(qcfg(true)), &norm_air));
        assert!(
            accuracy(MSG, &text) < 0.3,
            "normal TX printed through a reversed decoder: {text:?}"
        );
    }

    #[test]
    fn cross_mode_decodes_are_garbage_not_text() {
        // THE NEGATIVE CONTROL against a silently-wrong shared-path refactor:
        // a BPSK31 over into the QPSK demod (and vice versa) must not print
        // the message. Mode confusion = garbage or silence, correctly.
        use crate::psk::modulator::{bpsk_samples, psk_over_bits};
        let mut bpsk_air = vec![0.0f32; 1200];
        bpsk_air.extend(bpsk_samples(&psk_over_bits(MSG), &PskTxConfig::default()));
        bpsk_air.extend(vec![0.0f32; 2400]);
        let text = text_of(&decode_all(
            &mut PskDemodulator::new(qcfg(false)),
            &bpsk_air,
        ));
        assert!(
            accuracy(MSG, &text) < 0.3,
            "a BPSK over printed through the QPSK demod: {text:?}"
        );
        let qpsk_air = tx_over(MSG, false);
        let text = text_of(&decode_all(
            &mut PskDemodulator::new(PskConfig::default()),
            &qpsk_air,
        ));
        assert!(
            accuracy(MSG, &text) < 0.3,
            "a QPSK over printed through the BPSK demod: {text:?}"
        );
    }

    #[test]
    fn qpsk_idle_holds_squelch_open_and_prints_nothing() {
        // Latched with nothing typed, QPSK idles on 180° reversals (the BPSK
        // idle, by the code's structure): the far end must SEE a signal and
        // PRINT nothing.
        let mut s = QpskStream::new(PskTxConfig::default(), false);
        let idle = s.chunk(&vec![false; 320], false);
        let mut demod = PskDemodulator::new(qcfg(false));
        let chars = decode_all(&mut demod, &idle);
        assert!(chars.is_empty(), "QPSK idle printed {:?}", text_of(&chars));
        assert!(
            demod.signal_present(),
            "10 s of QPSK idle did not open the far end's squelch"
        );
    }

    #[test]
    fn qpsk_noise_only_is_squelched_silent() {
        // The QPSK demod's own noise negative control (its squelch metric is
        // cos 4φ, not BPSK's cos 2φ — it needs its own proof).
        for seed in [
            0x0123456789ABCDEFu64,
            0x2545F4914F6CDD1D,
            0x9E3779B97F4A7C15,
        ] {
            let noise = white_noise(5.0, 0.2, seed);
            let chars = decode_all(&mut PskDemodulator::new(qcfg(false)), &noise);
            assert!(
                chars.len() <= 3,
                "noise must be near-silent, got {} chars: {:?}",
                chars.len(),
                text_of(&chars)
            );
        }
    }

    #[test]
    fn qpsk_afc_pulls_in_an_offset() {
        // The AFC's modulation stripper is the FOURTH power in QPSK (the
        // square only removes 180s) — both signs, like the BPSK pair.
        for off in [15.0f32, -15.0] {
            let cfg = PskTxConfig {
                center_hz: PskConfig::default().center_hz + off,
                ..PskTxConfig::default()
            };
            let mut audio = vec![0.0f32; 1200];
            let mut s = QpskStream::new(cfg, false);
            let mut bits = vec![false; 96];
            bits.extend(encode_bits(MSG));
            audio.extend(s.chunk(&bits, true));
            audio.extend(vec![0.0f32; 24_000]);
            let mut demod = PskDemodulator::new(qcfg(false));
            let text = text_of(&decode_all(&mut demod, &audio));
            assert!(
                (demod.afc_offset_hz() - off).abs() < 5.0,
                "AFC should walk to ≈{off} Hz, got {}",
                demod.afc_offset_hz()
            );
            let acc = accuracy(MSG, &text);
            assert!(
                acc >= 0.8,
                "off-tune copy after AFC pull-in: {acc} ({text:?})"
            );
        }
    }

    #[test]
    #[ignore] // diagnostic: cargo test -p tempo-core psk::qpsk::tests::margins -- --ignored --nocapture
    fn margins() {
        for snr in [10.0f32, 0.0, -5.0, -8.0, -10.0, -12.0] {
            let mut audio = tx_over(MSG, false);
            add_awgn(&mut audio, snr, 0x2545F4914F6CDD1D);
            let chars = decode_all(&mut PskDemodulator::new(qcfg(false)), &audio);
            let text = text_of(&chars);
            let conf = chars.iter().map(|d| d.confidence).sum::<f32>() / chars.len().max(1) as f32;
            println!(
                "snr {snr:>6} dB  acc {:.2}  conf {conf:.2}  {text:?}",
                accuracy(MSG, &text)
            );
        }
    }
}
