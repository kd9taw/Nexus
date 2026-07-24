//! HDLC framing for AX.25: flag delimiting, bit-stuffing, and NRZI — the bit layer between an
//! AX.25 [`Frame`](super::frame::Frame) and the AFSK-1200 modem.
//!
//! On air, a frame is bracketed by `0x7E` flags (`0 1111110`). The bytes between the flags are
//! sent **LSB-first**, and after any run of five `1` bits the transmitter inserts a `0`
//! ("bit-stuffing") so the six-ones flag pattern can never occur inside the data. The resulting
//! logical bit stream is then **NRZI**-encoded: a `0` toggles the tone, a `1` holds it — which is
//! what keys the AFSK mark/space tones. Receive is the mirror: NRZI-decode → hunt for flags →
//! de-stuff → reassemble bytes.
//!
//! TX chain:  `Frame::encode()` → [`encode_frame`] → [`nrzi_encode`] → AFSK tones.
//! RX chain:  AFSK tones → [`nrzi_decode`] → [`deframe`] → `Frame::decode()`.

/// The HDLC flag byte that brackets every frame.
pub const FLAG: u8 = 0x7E;
/// NRZI reference level shared by [`nrzi_encode`]/[`nrzi_decode`] so a round-trip is exact. (On a
/// real receiver the leading flags re-sync this; NRZI is differential, so the absolute level of
/// this reference is immaterial once framing locks.)
const NRZI_INIT: bool = true;

fn push_byte_lsb(bits: &mut Vec<bool>, byte: u8) {
    for i in 0..8 {
        bits.push((byte >> i) & 1 != 0);
    }
}

/// Encode raw frame bytes (AX.25 addresses…info + FCS, i.e. [`Frame::encode`](super::frame::Frame::encode))
/// into the logical HDLC bit stream: `preamble` opening flags, the bit-stuffed frame, then
/// `trailing` closing flags. Flags themselves are never stuffed. `preamble`/`trailing` are clamped
/// to at least 1 so the stream is always properly delimited.
pub fn encode_frame(frame_bytes: &[u8], preamble: usize, trailing: usize) -> Vec<bool> {
    let mut bits = Vec::with_capacity((preamble + trailing) * 8 + frame_bytes.len() * 9);
    for _ in 0..preamble.max(1) {
        push_byte_lsb(&mut bits, FLAG);
    }
    let mut ones = 0u8;
    for &byte in frame_bytes {
        for i in 0..8 {
            let bit = (byte >> i) & 1 != 0;
            bits.push(bit);
            if bit {
                ones += 1;
                if ones == 5 {
                    bits.push(false); // stuff a 0 after five 1s
                    ones = 0;
                }
            } else {
                ones = 0;
            }
        }
    }
    for _ in 0..trailing.max(1) {
        push_byte_lsb(&mut bits, FLAG);
    }
    bits
}

/// NRZI-encode a logical bit stream into tone levels: a `0` bit toggles the level, a `1` holds it.
pub fn nrzi_encode(bits: &[bool]) -> Vec<bool> {
    let mut level = NRZI_INIT;
    let mut out = Vec::with_capacity(bits.len());
    for &b in bits {
        if !b {
            level = !level; // 0 = transition
        }
        out.push(level);
    }
    out
}

/// NRZI-decode tone levels back into logical bits: same level as the previous sample → `1`, a
/// change → `0`.
pub fn nrzi_decode(levels: &[bool]) -> Vec<bool> {
    let mut prev = NRZI_INIT;
    let mut out = Vec::with_capacity(levels.len());
    for &lvl in levels {
        out.push(lvl == prev);
        prev = lvl;
    }
    out
}

fn bits_to_bytes(bits: &[bool]) -> Option<Vec<u8>> {
    if bits.is_empty() || !bits.len().is_multiple_of(8) {
        return None; // not byte-aligned → misframed, drop it (the FCS would reject it anyway)
    }
    Some(
        bits.chunks(8)
            .map(|c| c.iter().enumerate().fold(0u8, |acc, (i, &b)| acc | (u8::from(b) << i)))
            .collect(),
    )
}

/// Recover frames from a logical (already NRZI-decoded) bit stream: hunt for flags, de-stuff the
/// data, and reassemble bytes. Returns every byte-aligned frame found between flag pairs (each is
/// AX.25 addresses…info + the 2-byte FCS, ready for `Frame::decode`). Runs of ≥7 ones (abort) and
/// non-byte-aligned segments are discarded.
pub fn deframe(bits: &[bool]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut frame: Vec<bool> = Vec::new();
    let mut ones = 0u32;
    let mut in_frame = false;
    for &b in bits {
        if b {
            if ones < 5 {
                if in_frame {
                    frame.push(true);
                }
                ones += 1;
            } else {
                ones += 1; // sixth-or-later 1: part of a flag/abort, never data — don't collect
            }
        } else {
            let run = ones;
            ones = 0;
            match run {
                5 => {} // stuffed zero → discard
                6 => {
                    // Flag: the tentatively-collected opening `0` + five `1`s (6 bits) belong to
                    // this flag, not the frame — trim them, then emit whatever preceded.
                    if in_frame {
                        frame.truncate(frame.len().saturating_sub(6));
                        if let Some(bytes) = bits_to_bytes(&frame) {
                            frames.push(bytes);
                        }
                    }
                    frame.clear();
                    in_frame = true;
                }
                r if r >= 7 => {
                    in_frame = false; // abort → resync on the next flag
                    frame.clear();
                }
                _ => {
                    if in_frame {
                        frame.push(false);
                    }
                }
            }
        }
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::super::frame::{Address, Frame};
    use super::*;

    /// Count the longest run of consecutive `true`s.
    fn max_run(bits: &[bool]) -> u32 {
        let (mut best, mut cur) = (0u32, 0u32);
        for &b in bits {
            cur = if b { cur + 1 } else { 0 };
            best = best.max(cur);
        }
        best
    }

    #[test]
    fn bit_stuffing_prevents_six_consecutive_ones_in_the_data() {
        // 0xFF bytes would be eight 1s in a row without stuffing.
        let bits = encode_frame(&[0xFF, 0xFF, 0xFF], 1, 1);
        // Only the flags may contain six 1s; the stuffed data region never does. Since the flags
        // are exactly six-1 runs, the max run across the whole stream is 6 (from flags), and no
        // run exceeds 6 (which would mean data leaked a sixth 1).
        assert_eq!(max_run(&bits), 6, "only flags reach six 1s; stuffed data caps at five");
    }

    #[test]
    fn stuffing_inserts_a_zero_after_five_ones() {
        // 0x1F = 0b00011111 → LSB-first that's 1,1,1,1,1,0,0,0. The five leading 1s force a stuff.
        let bits = encode_frame(&[0x1F], 0, 0);
        // Strip the single opening + closing flag (8 bits each).
        let data = &bits[8..bits.len() - 8];
        assert_eq!(&data[..6], &[true, true, true, true, true, false], "stuffed 0 after five 1s");
    }

    #[test]
    fn nrzi_round_trips() {
        let logical = vec![true, false, false, true, true, true, false, true, false, false];
        assert_eq!(nrzi_decode(&nrzi_encode(&logical)), logical);
    }

    #[test]
    fn nrzi_zero_transitions_one_holds() {
        // From the true reference: a 1 holds (true), a 0 toggles (→false), a 0 toggles (→true).
        assert_eq!(nrzi_encode(&[true, false, false]), vec![true, false, true]);
    }

    #[test]
    fn hdlc_bit_layer_round_trips_a_frame() {
        let f = Frame::ui(
            Address::new("APRS", 0),
            Address::new("N0CALL", 9),
            vec![Address::new("WIDE1", 1), Address::new("WIDE2", 1)],
            b"!4903.50N/07201.75W-Nexus APRS",
        );
        let frame_bytes = f.encode();
        // Full TX→RX bit chain: stuff+flag → NRZI → (air) → NRZI⁻¹ → deframe.
        let tx = nrzi_encode(&encode_frame(&frame_bytes, 8, 2));
        let got = deframe(&nrzi_decode(&tx));
        assert_eq!(got.len(), 1, "exactly one frame recovered");
        assert_eq!(got[0], frame_bytes, "frame bytes survive the HDLC/NRZI round trip");
        assert_eq!(Frame::decode(&got[0]), Some(f), "and reparse to the original frame");
    }

    #[test]
    fn recovers_two_back_to_back_frames() {
        let a = Frame::ui(Address::new("APRS", 0), Address::new("N0CALL", 1), vec![], b">first").encode();
        let b = Frame::ui(Address::new("APRS", 0), Address::new("N0CALL", 2), vec![], b">second").encode();
        // Share flags between them: flag … A … flag … B … flag.
        let mut bits = encode_frame(&a, 1, 1);
        bits.extend(encode_frame(&b, 0, 1));
        let got = deframe(&nrzi_decode(&nrzi_encode(&bits)));
        assert_eq!(got, vec![a, b]);
    }

    #[test]
    fn a_stream_with_no_flags_yields_nothing() {
        assert!(deframe(&nrzi_decode(&nrzi_encode(&[true; 40]))).is_empty());
        assert!(deframe(&[]).is_empty());
    }
}
