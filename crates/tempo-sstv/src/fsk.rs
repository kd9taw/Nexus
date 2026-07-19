//! FSK callsign-ID decoding — the burst that trails an SSTV image.
//!
//! Faithful translation of slowrx's `fsk.c` `GetFSK()` (Oona Räisänen,
//! ISC License) — see `NOTICE.md`. The FSK IDs are 6-bit bytes, **LSB
//! first**, 45.45 baud (≈22 ms/bit), **1900 Hz = 1, 2100 Hz = 0**. The
//! text is framed by a `20 2A` leader and a `01` end marker; adding
//! `0x20` to each byte yields ASCII. Every constant here — tone pair,
//! baud, bit period, bit-sense, bit-order, ASCII offset, leader-sync and
//! terminator — is ported verbatim from `fsk.c`.
//!
//! Where slowrx runs a 2048-FFT over a 22 ms Hann window and sums the
//! low/high half-bands, this uses the crate's single-bin
//! [`crate::dsp::goertzel_power`] evaluated at the two exact tones — the
//! tones are 200 Hz apart (≈4 bins at our window), so the hard decision
//! `power(1900) > power(2100)` is unambiguous. Modelled on `vis.rs`'s
//! tone-slicing structure at the [`WORKING_SAMPLE_RATE_HZ`] working rate.
//!
//! This is best-effort and RX-only: an absent or garbled burst simply
//! yields `None` (the sanity gate rejects implausible text), so the
//! image always stands on its own.

use crate::resample::WORKING_SAMPLE_RATE_HZ;

/// FSK-ID symbol rate (slowrx `fsk.c`: "45.45 baud (22 ms/bit)").
const BAUD: f64 = 45.45;
/// 1900 Hz tone ⇒ bit 1 (slowrx `fsk.c` line 14).
const ONE_HZ: f64 = 1900.0;
/// 2100 Hz tone ⇒ bit 0 (slowrx `fsk.c` line 14).
const ZERO_HZ: f64 = 2100.0;
/// Data symbols are 6-bit bytes (slowrx `fsk.c`).
const BITS_PER_CHAR: usize = 6;
/// Add `0x20` to each 6-bit byte to get ASCII (slowrx `fsk.c` line 99).
const ASCII_OFFSET: u8 = 0x20;
/// slowrx `fsk.c` line 98: stop after `BytePtr > 9` (max 10 chars stored).
const MAX_CHARS: usize = 10;
/// slowrx `fsk.c` line 91: give up the leader scan after `TestPtr > 200`
/// half-bit steps (≈2.2 s) without a `20 2A` match.
const SYNC_SCAN_LIMIT: usize = 200;

/// 6-bit reversal LUT, verbatim from slowrx `fsk.c`. The leader-sync
/// reconstruction packs bits MSB-first-in-time, so each 6-bit char is
/// bit-reversed before comparison with the `20 2A` marker.
#[rustfmt::skip]
const BIT_REV: [u8; 64] = [
    0x00, 0x20, 0x10, 0x30,  0x08, 0x28, 0x18, 0x38,
    0x04, 0x24, 0x14, 0x34,  0x0c, 0x2c, 0x1c, 0x3c,
    0x02, 0x22, 0x12, 0x32,  0x0a, 0x2a, 0x1a, 0x3a,
    0x06, 0x26, 0x16, 0x36,  0x0e, 0x2e, 0x1e, 0x3e,
    0x01, 0x21, 0x11, 0x31,  0x09, 0x29, 0x19, 0x39,
    0x05, 0x25, 0x15, 0x35,  0x0d, 0x2d, 0x1d, 0x3d,
    0x03, 0x23, 0x13, 0x33,  0x0b, 0x2b, 0x1b, 0x3b,
    0x07, 0x27, 0x17, 0x37,  0x0f, 0x2f, 0x1f, 0x3f,
];

/// Best-effort decode of the FSK callsign ID in `working_audio` (11025 Hz).
/// `hedr_shift_hz` is the radio-mistuning offset carried from VIS; it shifts
/// the 1900/2100 Hz tone pair (slowrx `fsk.c`: `1900 + HedrShift`).
///
/// Returns `Some(text)` only when a plausible ID was recovered — the MVP
/// sanity gate requires ≥3 chars, all in `[A-Z0-9/ ]`. Anything else
/// (no leader, garbled bits, off-alphabet output) returns `None`, so a
/// wrong decode degrades to no badge rather than garbage.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub(crate) fn decode_fsk_id(working_audio: &[f32], hedr_shift_hz: f64) -> Option<String> {
    let sample_rate = f64::from(WORKING_SAMPLE_RATE_HZ);
    let bit_samples = sample_rate / BAUD;
    let half_bit = bit_samples / 2.0;
    let win = bit_samples.round() as usize;
    let one_hz = ONE_HZ + hedr_shift_hz;
    let zero_hz = ZERO_HZ + hedr_shift_hz;

    // One hard-decision bit over the one-bit window starting at `start`
    // (fractional samples). slowrx `fsk.c` line 73: `Bit = (LoPow > HiPow)`,
    // where LoPow is the 1900 Hz (=1) band and HiPow the 2100 Hz (=0) band.
    let slice_bit = |start: f64| -> Option<u8> {
        if start < 0.0 {
            return None;
        }
        let s = start.round() as usize;
        let end = s.checked_add(win)?;
        if end > working_audio.len() {
            return None;
        }
        let w = &working_audio[s..end];
        let p_one = crate::dsp::goertzel_power(w, one_hz);
        let p_zero = crate::dsp::goertzel_power(w, zero_hz);
        Some(u8::from(p_one > p_zero))
    };

    // Phase 1 — recover the bit clock from the `20 2A` leader (slowrx
    // `fsk.c` lines 75-92). The scan hops a HALF bit each step; the
    // every-other-sample extraction reads the 12 leader bits at one clock
    // phase, and the `20 2A` match locks it. `data_start` is then the window
    // start of the first data bit (one full bit past the last leader bit).
    let mut test_bits = [0u8; 24];
    let mut test_ptr: usize = 0;
    let mut start = 0.0_f64;
    let data_start = loop {
        let bit = slice_bit(start)?;
        test_bits[test_ptr % 24] = bit;
        // Only meaningful once 24 half-bit samples (the whole leader) exist;
        // before that the C reads wrapped-garbage indices that never match.
        if test_ptr >= 23 {
            let mut test_num: u32 = 0;
            for i in 0..12 {
                let idx = (test_ptr - (23 - i * 2)) % 24;
                test_num |= u32::from(test_bits[idx]) << (11 - i);
            }
            if BIT_REV[((test_num >> 6) & 0x3f) as usize] == 0x20
                && BIT_REV[(test_num & 0x3f) as usize] == 0x2a
            {
                break start + half_bit;
            }
        }
        test_ptr += 1;
        if test_ptr > SYNC_SCAN_LIMIT {
            return None;
        }
        start += half_bit;
    };

    // Phase 2 — read 6-bit LSB-first chars until the terminator (slowrx
    // `fsk.c` lines 93-104). `AsciiByte < 0x0d` (which includes the `01` end
    // marker) or a full 10 chars stops the loop; a partial char at the end of
    // the buffer is dropped.
    let mut cur = data_start;
    let mut text = String::new();
    'chars: loop {
        let mut ascii_byte: u8 = 0;
        for bit_ptr in 0..BITS_PER_CHAR {
            match slice_bit(cur) {
                Some(bit) => ascii_byte |= bit << bit_ptr,
                None => break 'chars,
            }
            cur += bit_samples;
        }
        if ascii_byte < 0x0d || text.len() >= MAX_CHARS {
            break;
        }
        text.push(char::from(ascii_byte + ASCII_OFFSET));
    }

    // MVP sanity gate: only surface a plausible callsign; suppress garbage so
    // a mis-framed decode shows no badge instead of noise.
    let trimmed = text.trim();
    if trimmed.chars().count() >= 3
        && trimmed
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '/' || c == ' ')
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}

#[cfg(test)]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// Synthesize an FSK-ID burst for `text` using the decoder's OWN framing:
    /// the `20 2A` leader, 6-bit LSB-first chars (ASCII − `0x20`), 1900 Hz = 1
    /// / 2100 Hz = 0 at [`BAUD`], and the `01` end marker. Continuous phase so
    /// symbol transitions don't smear the Goertzel decision.
    fn synth_fsk(text: &str) -> Vec<f32> {
        let mut bytes = vec![0x20_u8, 0x2a]; // leader
        bytes.extend(text.bytes().map(|c| c - ASCII_OFFSET));
        bytes.push(0x01); // end marker

        let mut bits: Vec<u8> = Vec::new();
        for b in bytes {
            for k in 0..BITS_PER_CHAR {
                bits.push((b >> k) & 1);
            }
        }
        let sample_rate = f64::from(WORKING_SAMPLE_RATE_HZ);
        let bit_samples = sample_rate / BAUD;
        let n_bits = bits.len();
        let tone_samples = (n_bits as f64 * bit_samples).ceil() as usize;
        let pad = bit_samples.round() as usize; // room for the last bit's window

        let mut out = Vec::with_capacity(tone_samples + pad);
        let mut phase = 0.0_f64;
        for s in 0..tone_samples {
            let bit_idx = ((s as f64 / bit_samples) as usize).min(n_bits - 1);
            let f = if bits[bit_idx] == 1 { ONE_HZ } else { ZERO_HZ };
            phase += 2.0 * PI * f / sample_rate;
            out.push(phase.sin() as f32);
        }
        out.extend(std::iter::repeat_n(0.0_f32, pad));
        out
    }

    #[test]
    fn decodes_known_callsign() {
        let audio = synth_fsk("KD9TAW");
        assert_eq!(decode_fsk_id(&audio, 0.0).as_deref(), Some("KD9TAW"));
    }

    #[test]
    fn decodes_callsign_with_slash() {
        let audio = synth_fsk("W1AW/4");
        assert_eq!(decode_fsk_id(&audio, 0.0).as_deref(), Some("W1AW/4"));
    }

    #[test]
    fn rejects_short_buffer() {
        // Far too short to hold a `20 2A` leader — no lock, no output.
        assert!(decode_fsk_id(&[0.0_f32; 64], 0.0).is_none());
    }

    #[test]
    fn rejects_noise() {
        // White-ish noise never matches the leader pattern.
        let mut x: u64 = 0x1234_5678_9abc_def0;
        let noise: Vec<f32> = (0..WORKING_SAMPLE_RATE_HZ as usize)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                ((x.cast_signed() as f64) / (i64::MAX as f64)) as f32 * 0.3
            })
            .collect();
        assert!(decode_fsk_id(&noise, 0.0).is_none());
    }
}
