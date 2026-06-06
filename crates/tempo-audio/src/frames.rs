//! A rolling buffer of the most recent 4 seconds of received audio.
//!
//! Captured audio arrives continuously in small chunks; the FT1 decoder wants a
//! whole 4-second ([`FRAME_LEN`]) frame at each slot boundary. [`RxRing`] keeps
//! the latest `FRAME_LEN` samples; the runtime snapshots it once per RX slot.
//! The decoder performs its own fine timing search within the window, so exact
//! sub-sample alignment is not required here.

use std::collections::VecDeque;
use tempo_core::ft1;

/// Samples in one 4-second frame at 12 kHz (= `ft1::NMAX`).
pub const FRAME_LEN: usize = ft1::NMAX;

/// Rolling buffer holding the latest `cap` audio samples (one frame window).
///
/// `cap` is the tier's frame length — [`FRAME_LEN`] (48000, 4 s) for FT1, or the
/// longer DX1 capture window (15 s). The radio loop rebuilds the ring with the
/// right capacity when the operator switches tier.
#[derive(Debug)]
pub struct RxRing {
    buf: VecDeque<f32>,
    cap: usize,
}

impl Default for RxRing {
    fn default() -> Self {
        Self::new()
    }
}

impl RxRing {
    /// A ring sized for an FT1 frame ([`FRAME_LEN`] samples).
    pub fn new() -> Self {
        Self::with_capacity(FRAME_LEN)
    }

    /// A ring holding the latest `cap` samples (the tier's frame window).
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(cap),
            cap,
        }
    }

    /// The window length this ring retains.
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// Resize the retained window to `cap`, keeping the most recent samples.
    /// Used when the operator switches mode/tier (FT8 = 180000, FT4 = 72576,
    /// FT1 = 48000 samples) so the next decode frame is the right length.
    pub fn resize(&mut self, cap: usize) {
        self.cap = cap;
        while self.buf.len() > cap {
            self.buf.pop_front();
        }
        if cap > self.buf.capacity() {
            self.buf.reserve(cap - self.buf.len());
        }
    }

    /// Append captured samples, dropping the oldest beyond the capacity.
    pub fn push(&mut self, samples: &[f32]) {
        self.buf.extend(samples.iter().copied());
        while self.buf.len() > self.cap {
            self.buf.pop_front();
        }
    }

    /// The current frame: exactly `cap` samples, front-zero-padded if we have
    /// not yet captured a full window.
    pub fn frame(&self) -> Vec<f32> {
        if self.buf.len() == self.cap {
            return self.buf.iter().copied().collect();
        }
        let mut out = vec![0.0f32; self.cap - self.buf.len()];
        out.extend(self.buf.iter().copied());
        out
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
    pub fn is_full(&self) -> bool {
        self.buf.len() == self.cap
    }
    pub fn clear(&mut self) {
        self.buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_is_always_frame_len_and_holds_latest() {
        let mut r = RxRing::new();
        r.push(&[1.0; 1000]);
        let f = r.frame();
        assert_eq!(f.len(), FRAME_LEN);
        // Front zero-padded, latest samples at the end.
        assert_eq!(f[FRAME_LEN - 1], 1.0);
        assert_eq!(f[0], 0.0);

        // Overfill: keeps only the most recent FRAME_LEN.
        r.push(&vec![2.0; FRAME_LEN]);
        let f = r.frame();
        assert!(r.is_full());
        assert!(f.iter().all(|&x| x == 2.0));
    }
}
