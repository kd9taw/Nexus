//! BPSK31 modulator — the transmit half of the PSK31 pair, written from
//! scratch for Nexus (no ported code; `demod.rs` is the receive half).
//!
//! The waveform is the G3PLX shape: a carrier at the tuned audio offset whose
//! polarity holds through a `1` bit and reverses through a `0`, with every
//! reversal COSINE-SHAPED — the envelope swings through zero amplitude at the
//! bit boundary (`sin(πu)` over the half-symbols either side) instead of
//! stepping the phase at full amplitude. That shaping is what keeps a PSK31
//! signal ~60 Hz wide; a hard reversal would splatter hundreds of Hz. It is
//! also the RX twin of the demodulator's raised-cosine matched filter, so the
//! loopback tests in `demod.rs` are a real end-to-end proof, not a
//! coincidence of fixtures.
//!
//! **Idle is continuous reversals** (all-zero bits): the envelope pulses at
//! the symbol rate, the far end's sync and squelch hold, and varicode frames
//! nothing — the PSK31 key-up/key-down/latch-idle condition, by construction.
//!
//! **Drive**: samples peak at [`TX_DRIVE`], deliberately below full scale.
//! PSK31's envelope IS the information — a rig's ALC compressing it back to
//! constant amplitude regenerates the splatter the shaping removed (IMD, the
//! classic PSK31 trap) — so the default drive leaves headroom instead of
//! handing the operator a waveform that slams ALC at the same tx_level FT8
//! uses. There is deliberately NO auto-ALC negotiation; the cockpit's TX dock
//! carries the operator-facing hint.
//!
//! ONE generator, two entry points. [`PskStream`] renders a chunk of bits at a
//! time, carrying the carrier phase, the polarity and the shaping state across
//! calls — continuous ("latched") TX types into a transmission whose end is
//! not known when it starts, so the generator must be resumable (the
//! [`crate::rtty`]-AFSK lesson: every re-initialised local is a phase step or
//! a re-applied ramp on the air). [`bpsk_samples`] is the one-shot form for a
//! whole known message and is DEFINED as one chunk through the same stream, so
//! the two can never disagree; `chunking_is_invisible` pins that any split of
//! the same bits renders sample-for-sample the same audio.

use super::demod::{BAUD, SAMPLE_RATE};
use super::varicode::encode_bits;
use std::f64::consts::PI;

/// Peak sample amplitude — the modest default drive (see the module header:
/// ALC headroom, because ALC flattening the shaped envelope is the IMD trap).
pub const TX_DRIVE: f32 = 0.7;

/// Idle (reversal) bits keyed BEFORE a one-shot over's first character:
/// ~1.5 s at 31.25 Bd — enough for the far end's 16-phase sync, one full
/// 0.5 s AFC block and the quality squelch to open before text starts.
/// (The demod's own acquisition tests use idle preambles of this order.)
pub const PREAMBLE_IDLE_BITS: usize = 48;

/// Idle bits keyed AFTER the last character: ~1 s tail so the final `00`
/// separator lands well inside open squelch, then the shaped key-down.
pub const POSTAMBLE_IDLE_BITS: usize = 32;

/// TX generator settings. `Default` is the shipping configuration: the 1 kHz
/// PSK31 audio convention at the 12 kHz app modem rate, 31.25 Bd.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PskTxConfig {
    /// Tuned audio offset (Hz) — TX transmits where the demod is netted, the
    /// PSK31 transceive convention (answering off-frequency loses the QSO).
    pub center_hz: f32,
    pub sample_rate: u32,
    pub baud: f32,
}

impl Default for PskTxConfig {
    fn default() -> Self {
        Self {
            center_hz: super::demod::PskConfig::default().center_hz,
            sample_rate: SAMPLE_RATE as u32,
            baud: BAUD,
        }
    }
}

/// The wire bits of one one-shot PSK31 over: idle preamble (the key-up the
/// far end acquires on), the varicode text, idle postamble. Fed to
/// [`bpsk_samples`] by the radio loop's send-and-done path.
pub fn psk_over_bits(text: &str) -> Vec<bool> {
    let mut bits = vec![false; PREAMBLE_IDLE_BITS];
    bits.extend(encode_bits(text));
    bits.extend(std::iter::repeat_n(false, POSTAMBLE_IDLE_BITS));
    bits
}

/// Render a whole transmission at once: the same waveform as [`PskStream`]
/// chunked, ending in the shaped key-down. One implementation on purpose —
/// this IS one chunk through the stream, so the one-shot path and the latched
/// path cannot drift apart.
pub fn bpsk_samples(bits: &[bool], cfg: &PskTxConfig) -> Vec<f32> {
    PskStream::new(*cfg).chunk(bits, true)
}

/// A RESUMABLE PSK31 generator — the state one latched transmission is.
///
/// Differential mapping (the demodulator's slicer inverted): bit `1` = the
/// polarity HOLDS through the next symbol, bit `0` = it REVERSES at the
/// boundary, cosine-shaped. Each `chunk` call renders one symbol per bit with
/// ONE SYMBOL of latency: a symbol's envelope depends on whether a reversal
/// ends it, which only the NEXT bit decides — so the current symbol is held
/// pending and rendered when that bit arrives. `tail` renders the pending
/// symbol with the key-down ramp (envelope to zero over its second half), so
/// a transmission always leaves the air through zero amplitude — ending a
/// carrier at full scale is a key click. n bits + tail ⇒ n+1 symbols.
///
/// GENERALIZED for QPSK31 (Keyboard Modes Phase 3): the polarity is a COMPLEX
/// unit value rather than ±1, and a symbol boundary applies one of FOUR phase
/// rotations. BPSK is the 0°/180° subset through [`PskStream::chunk`]; QPSK
/// pushes ±90° as well through [`PskStream::push_rotation`] (see
/// [`super::qpsk`], the only other caller). Boundary shaping is the
/// raised-cosine I/Q interpolation between the two symbol values over the
/// half-symbol either side of the boundary: for 180° the interpolation
/// collapses algebraically to the Phase 2 envelope-through-zero (±sin(πu) —
/// the BPSK waveform is unchanged), for ±90° the value crosses the CHORD
/// (envelope dips to 1/√2 ≈ −3 dB, never to zero), and 0° stays flat. The
/// spectrum discipline is therefore unchanged: every boundary is shaped, a
/// hard 90° step would splatter exactly as a hard reversal would, and
/// [`TX_DRIVE`] still bounds every sample.
pub struct PskStream {
    cfg: PskTxConfig,
    /// Carrier phase (rad), carried across chunks — phase continuity.
    phase: f64,
    /// Complex polarity (I, Q) of the PENDING (not yet rendered) symbol —
    /// always exactly one of (±1, 0)/(0, ±1), so equality tests are exact and
    /// rotations never accumulate rounding.
    cur: (f64, f64),
    /// Polarity of the last RENDERED symbol; `None` = nothing rendered yet,
    /// so the pending symbol begins with the shaped key-up ramp from zero.
    prev: Option<(f64, f64)>,
    /// Exact cumulative symbol count and the samples emitted for it — the
    /// fractional symbol clock (rounding never accumulates, the AFSK rule;
    /// at 12 kHz / 31.25 Bd a symbol is exactly 384 samples, but the
    /// arithmetic must not depend on that).
    t_syms: f64,
    n_out: u64,
}

/// Raised-cosine I/Q interpolation `w·a + (1−w)·b`, `w = ½(1+cos πv)` — the
/// boundary-transition shaper (v runs 0→1 across the one-symbol window
/// centered on the boundary).
fn interp(a: (f64, f64), b: (f64, f64), v: f64) -> (f64, f64) {
    let w = 0.5 * (1.0 + (PI * v).cos());
    (w * a.0 + (1.0 - w) * b.0, w * a.1 + (1.0 - w) * b.1)
}

impl PskStream {
    pub fn new(cfg: PskTxConfig) -> Self {
        Self {
            cfg,
            phase: 0.0,
            cur: (1.0, 0.0),
            // The stream's first symbol ramps up from zero — the shaped key-up.
            prev: None,
            t_syms: 0.0,
            n_out: 0,
        }
    }

    /// Render the pending symbol. `next` is the symbol value after it,
    /// deciding the boundary transition at its end; `None` closes the
    /// transmission with the key-down ramp to zero.
    fn render_pending(&mut self, next: Option<(f64, f64)>, out: &mut Vec<f32>) {
        let fs = self.cfg.sample_rate.max(1) as f64;
        let dp = 2.0 * PI * self.cfg.center_hz as f64 / fs;
        self.t_syms += 1.0;
        let target = self.t_syms * (fs / self.cfg.baud as f64);
        let start = self.n_out;
        // Fill to the stream's exact fractional boundary.
        let mut n = 0u64;
        while ((start + n) as f64) < target {
            n += 1;
        }
        let cur = self.cur;
        let prev = self.prev;
        for i in 0..n {
            let u = (i as f64 + 0.5) / n as f64; // position in symbol, 0..1
                                                 // The symbol value at this sample: held mid-symbol, interpolated
                                                 // across a differing boundary, amplitude-ramped at key-up/down.
            let (ci, cq) = if u < 0.5 {
                match prev {
                    None => (cur.0 * (PI * u).sin(), cur.1 * (PI * u).sin()),
                    Some(p) if p != cur => interp(p, cur, u + 0.5),
                    _ => cur,
                }
            } else {
                match next {
                    None => (cur.0 * (PI * u).sin(), cur.1 * (PI * u).sin()),
                    Some(nx) if nx != cur => interp(cur, nx, u - 0.5),
                    _ => cur,
                }
            };
            self.phase += dp;
            if self.phase > 2.0 * PI {
                self.phase -= 2.0 * PI;
            }
            // Audio = I·sin(ωt) + Q·cos(ωt): rotating (I,Q) by +90°
            // (multiplying by j) ADVANCES the audio phase by +90° — the sign
            // convention `super::qpsk`'s wire contract is stated in.
            let (s, c) = self.phase.sin_cos();
            out.push((TX_DRIVE as f64 * (ci * s + cq * c)) as f32);
        }
        self.n_out += n;
    }

    /// Advance the stream one symbol whose BOUNDARY applies the unit complex
    /// rotation `rot`: render the pending symbol against the value the
    /// rotation produces, then make that value pending. `pub(crate)`: the
    /// QPSK wrapper in [`super::qpsk`] is the only caller outside this file.
    pub(crate) fn push_rotation(&mut self, rot: (f64, f64), out: &mut Vec<f32>) {
        // c′ = c·rot — exact on the {±1, ±j} lattice, so no drift accumulates.
        let next = (
            self.cur.0 * rot.0 - self.cur.1 * rot.1,
            self.cur.0 * rot.1 + self.cur.1 * rot.0,
        );
        self.render_pending(Some(next), out);
        self.prev = Some(self.cur);
        self.cur = next;
    }

    /// Close the stream through the shaped key-down (the pending symbol's
    /// envelope ramps to zero over its second half). `pub(crate)` for the
    /// QPSK wrapper; BPSK callers use `chunk(&[], true)`.
    pub(crate) fn close(&mut self, out: &mut Vec<f32>) {
        self.render_pending(None, out);
    }

    /// Render the next chunk of wire bits (`false` = reversal). `tail` closes
    /// the transmission: the pending symbol goes out with the key-down ramp.
    /// A tailed stream is finished — drop it; the next over builds a fresh one
    /// (resuming a closed envelope would re-key mid-shape).
    pub fn chunk(&mut self, bits: &[bool], tail: bool) -> Vec<f32> {
        let fs = self.cfg.sample_rate.max(1) as f64;
        let spb = fs / self.cfg.baud as f64;
        let mut out = Vec::with_capacity(((bits.len() + 1) as f64 * spb) as usize + 1);
        for &b in bits {
            // bit 1 = the polarity holds (0°), bit 0 = it reverses (180°).
            self.push_rotation(if b { (1.0, 0.0) } else { (-1.0, 0.0) }, &mut out);
        }
        if tail {
            // Close through zero amplitude — the shaped key-down.
            self.close(&mut out);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Worst sample-for-sample disagreement (length mismatch = total).
    fn max_diff(a: &[f32], b: &[f32]) -> f64 {
        if a.len() != b.len() {
            return f64::INFINITY;
        }
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max) as f64
    }

    /// Zero-crossing frequency estimate (the rtty_afsk test's meter).
    fn measured_hz(x: &[f32], fs: f64) -> f64 {
        let crossings = x.windows(2).filter(|w| w[0] * w[1] < 0.0).count();
        crossings as f64 * fs / (2.0 * x.len() as f64)
    }

    #[test]
    fn chunking_is_invisible() {
        // THE STREAM CONTRACT (the AFSK precedent): however the same bits are
        // split across chunk calls — per character, or at boundaries that fall
        // mid-varicode-code — the concatenation is sample-for-sample the
        // one-shot render. Anything that differs (a phase step, a re-applied
        // key ramp, a re-rounded symbol clock) shows up here.
        let cfg = PskTxConfig::default();
        let bits = psk_over_bits("cq de KD9TAW");
        let one_shot = bpsk_samples(&bits, &cfg);

        // POSITIVE CONTROL: a FRESH stream per chunk — the naive way to
        // stream, re-initialising the phase and re-ramping both ends. It must
        // disagree grossly, or the comparison below measures nothing.
        let mut naive = Vec::new();
        for chunk in bits.chunks(7) {
            naive.extend(PskStream::new(cfg).chunk(chunk, true));
        }
        assert!(
            max_diff(&naive, &one_shot) > 0.5,
            "the fresh-stream-per-chunk control must NOT match (diff={})",
            max_diff(&naive, &one_shot)
        );

        // Odd 7-bit splits (never on a character boundary), tail on the last.
        let mut s = PskStream::new(cfg);
        let mut streamed = Vec::new();
        let chunks: Vec<&[bool]> = bits.chunks(7).collect();
        for (i, c) in chunks.iter().enumerate() {
            streamed.extend(s.chunk(c, i == chunks.len() - 1));
        }
        assert_eq!(streamed.len(), one_shot.len(), "chunking changed duration");
        assert!(
            max_diff(&streamed, &one_shot) < 1e-6,
            "chunking is not invisible: worst disagreement={}",
            max_diff(&streamed, &one_shot)
        );

        // …and one bit at a time — the latched-typing extreme.
        let mut s2 = PskStream::new(cfg);
        let mut per_bit = Vec::new();
        for (i, &b) in bits.iter().enumerate() {
            per_bit.extend(s2.chunk(&[b], i == bits.len() - 1));
        }
        assert!(
            max_diff(&per_bit, &one_shot) < 1e-6,
            "per-bit chunking differs"
        );
    }

    #[test]
    fn the_symbol_clock_is_exact() {
        // n bits + tail = n+1 symbols; at 12 kHz / 31.25 Bd a symbol is
        // exactly 384 samples, and the fractional clock must land on it.
        let cfg = PskTxConfig::default();
        for n in [1usize, 5, 31, 100] {
            let audio = bpsk_samples(&vec![false; n], &cfg);
            assert_eq!(audio.len(), (n + 1) * 384, "n={n}");
        }
        // Un-tailed: one symbol of latency (the pending symbol waits for the
        // bit that decides its end shape).
        let mut s = PskStream::new(cfg);
        assert_eq!(s.chunk(&[true; 10], false).len(), 10 * 384);
    }

    #[test]
    fn reversals_pass_through_zero_and_steady_carrier_stays_flat() {
        let cfg = PskTxConfig::default();
        // Idle = continuous reversals: the envelope must touch ~zero at every
        // symbol boundary (the cosine shaping) and reach the full drive at
        // every symbol center.
        let idle = bpsk_samples(&[false; 20], &cfg);
        for k in 1..20 {
            let b = k * 384;
            let near: f32 = idle[b - 2..b + 2]
                .iter()
                .map(|x| x.abs())
                .fold(0.0, f32::max);
            assert!(
                near < 0.06,
                "boundary {k}: envelope {near} — reversal not shaped"
            );
            let mid = &idle[b + 186..b + 198];
            let peak = mid.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
            assert!(peak > 0.9 * TX_DRIVE, "center {k}: peak {peak} too low");
        }
        // A run of 1s (no reversals): constant envelope between the key ramps.
        let steady = bpsk_samples(&[true; 20], &cfg);
        let interior = &steady[2 * 384..18 * 384];
        // Every carrier peak in the interior sits at the drive level.
        let mut peaks = Vec::new();
        for w in interior.windows(3) {
            if w[1].abs() >= w[0].abs() && w[1].abs() >= w[2].abs() && w[1].abs() > 0.1 {
                peaks.push(w[1].abs());
            }
        }
        let lo = peaks.iter().cloned().fold(f32::INFINITY, f32::min);
        assert!(
            lo > 0.95 * TX_DRIVE,
            "steady-carrier envelope sagged to {lo}"
        );
    }

    #[test]
    fn the_drive_bounds_every_sample_and_the_carrier_sits_at_the_offset() {
        let cfg = PskTxConfig {
            center_hz: 1500.0,
            ..PskTxConfig::default()
        };
        let audio = bpsk_samples(&psk_over_bits("test"), &cfg);
        let peak = audio.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
        assert!(peak <= TX_DRIVE + 1e-3, "peak {peak} exceeds the drive");
        assert!(peak > 0.9 * TX_DRIVE, "peak {peak} never reaches the drive");
        // TX at the tuned offset: a steady stretch measures the carrier there.
        let steady = bpsk_samples(&[true; 60], &cfg);
        let hz = measured_hz(&steady[4 * 384..56 * 384], 12_000.0);
        assert!((hz - 1500.0).abs() < 5.0, "carrier at {hz} Hz, wanted 1500");
    }

    #[test]
    fn a_transmission_ends_through_zero() {
        // The key-down: the final samples ramp to ~zero, so a stop between
        // symbols is never a full-amplitude key click.
        let cfg = PskTxConfig::default();
        let audio = bpsk_samples(&[true; 10], &cfg);
        let tailn = audio.len();
        let last: f32 = audio[tailn - 4..]
            .iter()
            .map(|x| x.abs())
            .fold(0.0, f32::max);
        assert!(last < 0.06, "transmission ended at amplitude {last}");
    }
}
