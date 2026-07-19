//! RTTY — Baudot/ITA2 character layer + demodulator.
//!
//! [`baudot`] is the pure ITA2 5-bit codec (LTRS/FIGS shift planes, USOS,
//! US-TTY figures conventions, diddle idle). [`demod`] is the receive DSP —
//! a Rust port of the fldigi RTTY demodulator: baseband mark/space mixers →
//! 1024-point overlap-add FFT filters → SNR-optimized ATC slicer →
//! straddle-point bit clock → acquire-then-freeze AFC.
//!
//! Every decoded character carries a soft confidence (0..1) taken from the
//! ATC slicer, and the demodulator sits behind the [`RttyDemod`] trait — the
//! seam for the future decoder ensemble (N profile instances fanning out from
//! one audio stream into one merge/print stage). TX lives elsewhere (AFSK in
//! tempo-audio, FSK keyline in the service layer); both frame their bit
//! streams with the shared [`baudot::BaudotEncoder`].
//!
//! [`seq`] is the auto-sequencer — a pure text-pattern QSO state machine over
//! the free-running decoded stream (RTTY has no slot clock), with table-driven
//! exchange schemas and a human-initiate gate.

pub mod baudot;
pub mod demod;
pub mod seq;

pub use baudot::{code_bits, encodable, BaudotDecoder, BaudotEncoder};
pub use demod::{DecodedChar, RttyConfig, RttyDemod, RttyDemodulator};
pub use seq::{Action, RttySeq, SeqState};

/// The mark/space audio tone pair (Hz) for a netted `center_hz`, `shift_hz`, and
/// sense — the SINGLE source of truth shared by the RX demod thread's tones and
/// the cockpit's waterfall mark/space cursors, so the two can never disagree.
/// The pair straddles the center by ±shift/2; in normal sense the mark is the
/// LOWER tone (space the higher), and `reverse` swaps them. Returns `(mark, space)`.
pub fn tone_pair(center_hz: f32, shift_hz: f32, reverse: bool) -> (f32, f32) {
    let half = shift_hz / 2.0;
    let (lo, hi) = (center_hz - half, center_hz + half);
    if reverse {
        (hi, lo)
    } else {
        (lo, hi)
    }
}

#[cfg(test)]
mod tests {
    use super::tone_pair;

    #[test]
    fn tone_pair_straddles_center() {
        // 2210 Hz center at the 170 Hz standard shift = today's nominal 2125/2295 pair.
        assert_eq!(tone_pair(2210.0, 170.0, false), (2125.0, 2295.0));
        // Reverse swaps mark and space.
        assert_eq!(tone_pair(2210.0, 170.0, true), (2295.0, 2125.0));
        // A netted-lower center keeps the same ±shift/2 straddle.
        assert_eq!(tone_pair(1500.0, 170.0, false), (1415.0, 1585.0));
    }
}
