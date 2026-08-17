//! Keyboard text modes — the mode-neutral decode seam.
//!
//! RTTY was the first keyboard mode here and its demodulator grew the right
//! shape by itself: feed mono audio in, get characters out, each carrying a soft
//! confidence the print stage and the sequencer can weigh. PSK31 and the
//! keyboard modes after it decode entirely differently (varicode over a BPSK
//! costas loop rather than Baudot over an FSK slicer) and produce exactly the
//! same thing, so the shape belongs HERE rather than inside any one mode.
//!
//! What this buys is the decoder ensemble: N implementations of [`TextDemod`],
//! in one mode or across several, fanning out from one audio tap into one
//! merge/print stage — which is only expressible if the type they agree on is
//! not named after a mode. Nothing in this module knows a modulation.
//!
//! TX has its own mode-neutral seam, and it is not here: the transmit-safety
//! machinery (the continuous-TX latch, its two wall clocks, the per-tick gate
//! predicate) lives in `tempo_app::keyboard`, because every gate it re-checks is
//! engine state.

/// One decoded character with its demodulator's soft confidence: 0..1, where 1
/// is a clean symbol and low values mean "render me faint / don't trust me in
/// the sequencer". Each mode maps its own slicer metric onto this range — RTTY
/// uses the minimum ATC margin over the character's sampled bits — so a
/// consumer can compare characters from ONE decoder but must not read across
/// modes as if the scales were calibrated together.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecodedChar {
    pub ch: char,
    pub confidence: f32,
}

/// Streaming demodulator seam: feed mono f32 audio at the mode's sample rate,
/// get decoded characters.
///
/// `feed` is deliberately the whole contract. It is incremental (the
/// implementation carries its own filter state, bit clock and AFC across calls),
/// it returns only what COMPLETED during this call — an empty `Vec` is the
/// normal answer, not an error — and it never blocks, so one audio tap can drive
/// several implementations in turn on the same thread.
pub trait TextDemod {
    fn feed(&mut self, samples: &[f32]) -> Vec<DecodedChar>;
}
