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
//! Since 1.13.0 this module also ENCODES the burst — [`encode_fsk_id`], off
//! by default, appended to a transmitted picture when the operator switches it
//! on. The encoder reads the same constants as the decoder, so the two cannot
//! describe different waveforms, and the test that matters is the round trip:
//! encode a callsign, decode it with [`decode_fsk_id`], get the callsign back.
//!
//! Decoding is best-effort: an absent or garbled burst simply yields `None`
//! (the sanity gate rejects implausible text), so the image always stands on
//! its own.

use crate::resample::WORKING_SAMPLE_RATE_HZ;
use crate::tone::ToneWriter;

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

/// Characters the burst can carry: the 6-bit alphabet, narrowed to what a
/// callsign is made of AND to what [`decode_fsk_id`]'s sanity gate will accept
/// back. Encoding something the decoder would refuse is how a round trip
/// silently stops being a round trip.
fn is_sendable(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit() || c == '/'
}

/// Exact duration of the burst [`encode_fsk_id`] would produce for `call`,
/// seconds; `0.0` when the callsign cannot be sent. The transmit path adds this
/// to what it tells the operator the rig will be keyed for.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn fsk_id_seconds(call: &str) -> f64 {
    match sendable_text(call) {
        // Two leader bytes + the text + the end marker, six bits each.
        Some(text) => {
            let bits = (2 + text.chars().count() + 1) * BITS_PER_CHAR;
            bits as f64 / BAUD
        }
        None => 0.0,
    }
}

/// `call` reduced to what can go on the air: upper-cased, everything outside
/// the sendable alphabet dropped, capped at [`MAX_CHARS`] — the same ceiling
/// the decoder stops reading at. `None` when fewer than three characters
/// survive, which is [`decode_fsk_id`]'s own floor.
fn sendable_text(call: &str) -> Option<String> {
    let text: String = call
        .trim()
        .to_ascii_uppercase()
        .chars()
        .filter(|c| is_sendable(*c))
        .take(MAX_CHARS)
        .collect();
    (text.chars().count() >= 3).then_some(text)
}

/// Append the FSK callsign-ID burst for `call` to `tone`, in the framing
/// [`decode_fsk_id`] reads: the `20 2A` leader, 6-bit LSB-first characters
/// (ASCII − `0x20`), the `01` end marker, 1900 Hz for a one and 2100 Hz for a
/// zero at [`BAUD`]. Returns whether anything was written.
///
/// Writing into the caller's [`ToneWriter`] rather than returning its own
/// buffer keeps the phase continuous with the picture that precedes it — the
/// same discipline the scanline emitters follow, and the reason a station
/// hears one transmission rather than a picture with a click on the end.
///
/// ## Safety, since this is the transmit path
///
/// This function only lengthens a buffer. It cannot key a radio, cannot
/// lengthen a transmission that is already on the air, and cannot outlive one
/// the operator stopped: the samples it appends go into the very `Vec` the
/// engine measures to set the PTT deadline and to check the TX watchdog
/// budget, and the audio loop's abort flushes the output ring whatever is left
/// in it. Everything that governs the picture governs the burst, because they
/// are the same buffer.
pub(crate) fn emit_fsk_id(tone: &mut ToneWriter, call: &str) -> bool {
    let Some(text) = sendable_text(call) else {
        return false;
    };
    let mut bytes = vec![0x20_u8, 0x2a]; // leader
    bytes.extend(text.chars().map(|c| (c as u8) - ASCII_OFFSET));
    bytes.push(0x01); // end marker

    let bit_secs = 1.0 / BAUD;
    for b in bytes {
        for k in 0..BITS_PER_CHAR {
            // LSB first, exactly as the decoder reads them back.
            let hz = if (b >> k) & 1 == 1 { ONE_HZ } else { ZERO_HZ };
            tone.fill_secs(hz, bit_secs);
        }
    }
    true
}

/// Standalone FSK-ID burst for `call` at `sample_rate_hz` — [`emit_fsk_id`]
/// into a fresh [`ToneWriter`]. `None` when the callsign cannot be sent.
///
/// The transmit path uses [`emit_fsk_id`] so the burst shares the picture's
/// phase; this is the standalone form the round-trip tests drive, and it is
/// compiled only for them — a second production entry point into the transmit
/// path is a second thing to audit.
#[cfg(test)]
#[must_use]
pub(crate) fn encode_fsk_id(call: &str, sample_rate_hz: u32) -> Option<Vec<f32>> {
    let mut tone = ToneWriter::with_pre_silence_samples_at(0, sample_rate_hz);
    emit_fsk_id(&mut tone, call).then(|| tone.into_vec())
}

#[cfg(test)]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::expect_used,
    clippy::float_cmp
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

    // -----------------------------------------------------------------
    // #FSK-ID TX — the round trip, and the control that says it can fail.
    // -----------------------------------------------------------------

    /// ⭐ THE TEST THAT MATTERS. The production encoder's burst, read back by
    /// the production decoder, is the callsign that went in. Not "a burst was
    /// produced" and not "the framing looks right" — the same string out.
    ///
    /// Several callsigns, because the ones that break a 6-bit framing are the
    /// ones with digits in odd places and a portable suffix, not `W1AW`.
    #[test]
    fn encoded_ids_decode_back_to_the_same_callsign() {
        for call in ["KD9TAW", "W1AW", "G0ABC", "VK2XYZ/P", "9A1CRA", "DL8ZZ/QRP"] {
            let audio = encode_fsk_id(call, WORKING_SAMPLE_RATE_HZ).expect("encodable");
            assert_eq!(
                decode_fsk_id(&audio, 0.0).as_deref(),
                Some(call),
                "{call} did not survive its own encoder",
            );
        }
    }

    /// ⭐ THE CONTROL. Without it the round trip above proves nothing about the
    /// WIRE: a decoder and an encoder that agreed on the wrong tone pair, the
    /// wrong bit order or the wrong baud would agree with each other perfectly.
    /// Swap the two tones and the same round trip must FAIL — so the test is
    /// reading the signal and not just its own arithmetic.
    #[test]
    fn the_control_a_burst_with_the_tones_swapped_does_not_decode() {
        // Hand-built with 1900 and 2100 exchanged; everything else identical to
        // what `emit_fsk_id` does.
        let call = "KD9TAW";
        let mut bytes = vec![0x20_u8, 0x2a];
        bytes.extend(call.bytes().map(|c| c - ASCII_OFFSET));
        bytes.push(0x01);
        let mut tone = ToneWriter::with_pre_silence_samples_at(0, WORKING_SAMPLE_RATE_HZ);
        for b in bytes {
            for k in 0..BITS_PER_CHAR {
                // Inverted on purpose: 1 → 2100, 0 → 1900.
                let hz = if (b >> k) & 1 == 1 { ZERO_HZ } else { ONE_HZ };
                tone.fill_secs(hz, 1.0 / BAUD);
            }
        }
        let audio = tone.into_vec();
        assert_ne!(
            decode_fsk_id(&audio, 0.0).as_deref(),
            Some(call),
            "a tone-swapped burst decoded to the right callsign — the round trip \
             above is measuring arithmetic, not a waveform",
        );
    }

    /// The burst is about a second. Stated as a number because it is what the
    /// transmit path adds to the operator's key-down time and to the watchdog
    /// budget, and "about a second" is not a number an engine can check.
    #[test]
    fn a_callsign_burst_is_about_one_second() {
        // 2 leader + 6 characters + 1 end marker, six bits each at 45.45 baud.
        let want = 9.0 * 6.0 / BAUD;
        let got = fsk_id_seconds("KD9TAW");
        assert!((got - want).abs() < 1e-9, "{got} vs {want}");
        assert!(
            (1.0..1.5).contains(&got),
            "a six-character ID should be ≈1.2 s, got {got}"
        );
        // And the length really does follow the callsign.
        assert!(fsk_id_seconds("W1AW") < got);
    }

    /// A callsign the burst cannot carry produces NO burst at all — never a
    /// mangled one. The decoder's own sanity gate is the floor (three
    /// characters, `[A-Z0-9/]`), so anything the encoder emits is something the
    /// decoder will accept back.
    #[test]
    fn an_unsendable_callsign_produces_no_burst() {
        for call in ["", "  ", "K9", "!!", "-"] {
            assert!(
                encode_fsk_id(call, WORKING_SAMPLE_RATE_HZ).is_none(),
                "{call:?} should not produce a burst",
            );
            assert_eq!(fsk_id_seconds(call), 0.0, "{call:?} should cost no airtime");
        }
    }

    /// Lower case and punctuation are normalised rather than refused, and the
    /// text is capped where the decoder stops reading — so the callsign that
    /// goes on the air is the one that comes back, not a prefix of it.
    #[test]
    fn callsigns_are_normalised_to_what_the_decoder_will_read_back() {
        let audio = encode_fsk_id("kd9taw/p", WORKING_SAMPLE_RATE_HZ).expect("encodable");
        assert_eq!(decode_fsk_id(&audio, 0.0).as_deref(), Some("KD9TAW/P"));

        // MAX_CHARS is the decoder's stopping point; a longer string is cut to
        // it on the WAY OUT, so the two halves still agree.
        let long = "ABCDEFGHIJKLMNOP";
        let audio = encode_fsk_id(long, WORKING_SAMPLE_RATE_HZ).expect("encodable");
        let back = decode_fsk_id(&audio, 0.0).expect("decodes");
        assert_eq!(back.chars().count(), MAX_CHARS);
        assert_eq!(back, long[..MAX_CHARS]);
    }

    /// The burst still reads when the radio is off frequency, the same way the
    /// decoder already handles a mistuned picture — the tone pair shifts with
    /// `hedr_shift_hz` on both sides.
    #[test]
    fn a_mistuned_burst_still_reads() {
        let shift = 40.0;
        let mut tone = ToneWriter::with_pre_silence_samples_at(0, WORKING_SAMPLE_RATE_HZ);
        // Re-emit at the shifted tones by hand; the production encoder always
        // transmits on frequency, it is the RECEIVER that is off.
        let mut bytes = vec![0x20_u8, 0x2a];
        bytes.extend(b"KD9TAW".iter().map(|c| c - ASCII_OFFSET));
        bytes.push(0x01);
        for b in bytes {
            for k in 0..BITS_PER_CHAR {
                let hz = if (b >> k) & 1 == 1 { ONE_HZ } else { ZERO_HZ };
                tone.fill_secs(hz + shift, 1.0 / BAUD);
            }
        }
        let audio = tone.into_vec();
        assert_eq!(decode_fsk_id(&audio, shift).as_deref(), Some("KD9TAW"));
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
