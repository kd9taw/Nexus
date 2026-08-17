//! PSK31 varicode — the RX-direction character layer.
//!
//! Varicode (G3PLX, the PSK31 spec) maps each of the 128 ASCII characters to a
//! variable-length bit string with two structural invariants that make the
//! stream self-framing with no start/stop bits:
//!   1. no code contains two consecutive zeros ("00" never appears inside one),
//!   2. every code begins and ends with a `1`.
//!
//! Characters on the wire are separated by exactly `00`, and the idle stream is
//! continuous zeros (phase reversals) — so idle frames nothing and prints
//! nothing, by construction rather than by a squelch.
//!
//! This module ships the DECODE direction only (Phase 1 is receive-only): the
//! canonical table plus a streaming [`VaricodeDecoder`] that turns sliced bits
//! into characters. The encode direction lives in test code (the test-only
//! modulator) until PSK TX lands in Phase 2, at which point it is promoted —
//! the table below is already bidirectional data.
//!
//! Bit order: each string is written in transmission order (first-sent bit
//! first). A `1` is "no phase reversal", a `0` is "a reversal" — the
//! demodulator's differential slicer hands exactly that stream to the decoder.
//!
//! ### Decoder state machine (the contract)
//! `push(bit, conf)` shifts one sliced bit in. When the last two bits are `00`
//! the accumulated bits BEFORE that separator are one complete code: look it
//! up, reset, and emit the character with the MINIMUM per-bit confidence seen
//! since the last reset (the RTTY precedent: a character is only as trustworthy
//! as its worst bit). An empty accumulation (idle) emits nothing; an unknown or
//! overlong code is dropped silently — on-air garbage must never print as if
//! it were copy.

use crate::textmode::DecodedChar;
use std::collections::HashMap;

/// The canonical PSK31 varicode table, indexed by ASCII code (0..=127), each
/// entry in transmission order. Source: the G3PLX PSK31 specification's
/// varicode assignment (frequency-ordered: the commonest English characters
/// carry the shortest codes — space `1`, `e` `11`, `t` `101`).
pub const VARICODE: [&str; 128] = [
    "1010101011", // NUL
    "1011011011", // SOH
    "1011101101", // STX
    "1101110111", // ETX
    "1011101011", // EOT
    "1101011111", // ENQ
    "1011101111", // ACK
    "1011111101", // BEL
    "1011111111", // BS
    "11101111",   // HT
    "11101",      // LF
    "1101101111", // VT
    "1011011101", // FF
    "11111",      // CR
    "1101110101", // SO
    "1110101011", // SI
    "1011110111", // DLE
    "1011110101", // DC1
    "1110101101", // DC2
    "1110101111", // DC3
    "1101011011", // DC4
    "1101101011", // NAK
    "1101101101", // SYN
    "1101010111", // ETB
    "1101111011", // CAN
    "1101111101", // EM
    "1110110111", // SUB
    "1101010101", // ESC
    "1101011101", // FS
    "1110111011", // GS
    "1011111011", // RS
    "1101111111", // US
    "1",          // space
    "111111111",  // !
    "101011111",  // "
    "111110101",  // #
    "111011011",  // $
    "1011010101", // %
    "1010111011", // &
    "101111111",  // '
    "11111011",   // (
    "11110111",   // )
    "101101111",  // *
    "111011111",  // +
    "1110101",    // ,
    "110101",     // -
    "1010111",    // .
    "110101111",  // /
    "10110111",   // 0
    "10111101",   // 1
    "11101101",   // 2
    "11111111",   // 3
    "101110111",  // 4
    "101011011",  // 5
    "101101011",  // 6
    "110101101",  // 7
    "110101011",  // 8
    "110110111",  // 9
    "11110101",   // :
    "110111101",  // ;
    "111101101",  // <
    "1010101",    // =
    "111010111",  // >
    "1010101111", // ?
    "1010111101", // @
    "1111101",    // A
    "11101011",   // B
    "10101101",   // C
    "10110101",   // D
    "1110111",    // E
    "11011011",   // F
    "11111101",   // G
    "101010101",  // H
    "1111111",    // I
    "111111101",  // J
    "101111101",  // K
    "11010111",   // L
    "10111011",   // M
    "11011101",   // N
    "10101011",   // O
    "11010101",   // P
    "111011101",  // Q
    "10101111",   // R
    "1101111",    // S
    "1101101",    // T
    "101010111",  // U
    "110110101",  // V
    "101011101",  // W
    "101110101",  // X
    "101111011",  // Y
    "1010101101", // Z
    "111110111",  // [
    "111101111",  // \
    "111111011",  // ]
    "1010111111", // ^
    "101101101",  // _
    "1011011111", // `
    "1011",       // a
    "1011111",    // b
    "101111",     // c
    "101101",     // d
    "11",         // e
    "111101",     // f
    "1011011",    // g
    "101011",     // h
    "1101",       // i
    "111101011",  // j
    "10111111",   // k
    "11011",      // l
    "111011",     // m
    "1111",       // n
    "111",        // o
    "111111",     // p
    "110111111",  // q
    "10101",      // r
    "10111",      // s
    "101",        // t
    "110111",     // u
    "1111011",    // v
    "1101011",    // w
    "11011111",   // x
    "1011101",    // y
    "111010101",  // z
    "1010110111", // {
    "110111011",  // |
    "1010110101", // }
    "1011010111", // ~
    "1110110101", // DEL
];

/// Longest code in the table (10 bits). An accumulation past this with no `00`
/// separator is garbage (noise sliced into an endless run) — the decoder
/// resets rather than letting the register grow without bound.
const MAX_CODE_BITS: u32 = 10;

/// Pack a code's bits into a lookup key: the bits read in transmission order,
/// first bit most significant, prefixed by a sentinel `1` so codes of
/// different lengths can never collide (`"1"` → 0b11, `"11"` → 0b111, …).
fn key_of(bits: &str) -> u16 {
    let mut k: u16 = 1;
    for b in bits.bytes() {
        k = (k << 1) | u16::from(b == b'1');
    }
    k
}

/// Streaming varicode decoder: sliced bits in, characters out. Carries the
/// per-bit soft confidence through to the emitted character (minimum over the
/// character's bits — see the module header).
pub struct VaricodeDecoder {
    /// Code → character lookup, keyed by [`key_of`].
    table: HashMap<u16, char>,
    /// Bit register: sentinel-prefixed accumulation, same shape as the key.
    reg: u16,
    /// Bits currently in `reg` (0 = empty / just reset).
    nbits: u32,
    /// Minimum per-bit confidence since the last reset.
    cmin: f32,
}

impl Default for VaricodeDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl VaricodeDecoder {
    pub fn new() -> Self {
        Self {
            table: VARICODE
                .iter()
                .enumerate()
                .map(|(i, bits)| (key_of(bits), i as u8 as char))
                .collect(),
            reg: 1,
            nbits: 0,
            cmin: 1.0,
        }
    }

    /// Reset the accumulation (new tuning / squelch close): drop any partial
    /// code so a stale half-character can't weld onto the next real one.
    pub fn reset(&mut self) {
        self.reg = 1;
        self.nbits = 0;
        self.cmin = 1.0;
    }

    /// Feed one sliced bit (`true` = no reversal, `false` = reversal) with its
    /// slicer confidence (0..1). Returns a completed character when this bit
    /// closes a `00` separator over a non-empty, known code.
    pub fn push(&mut self, bit: bool, conf: f32) -> Option<DecodedChar> {
        // A lone pending zero when a 1 arrives is idle spill (the odd half of
        // an idle run or a third separator zero): no code starts with 0, so
        // the character begins HERE. Without this the first character after
        // idle prepends that zero and drops as an unknown code.
        if bit && self.nbits == 1 && self.reg == 0b10 {
            self.reset();
        }
        self.reg = (self.reg << 1) | u16::from(bit);
        self.nbits += 1;
        self.cmin = self.cmin.min(conf.clamp(0.0, 1.0));
        // Two trailing zeros = the inter-character separator: everything
        // before them is one complete code.
        if self.nbits >= 2 && self.reg & 0b11 == 0 {
            let code = self.reg >> 2; // still sentinel-prefixed — key-shaped
            let codelen = self.nbits - 2;
            let confidence = self.cmin;
            self.reset();
            if codelen == 0 {
                return None; // idle: separator after separator frames nothing
            }
            return self
                .table
                .get(&code)
                .map(|&ch| DecodedChar { ch, confidence });
        }
        // Bound the register: past the longest legal code plus its separator
        // there is nothing this accumulation can ever decode to — drop it so
        // noise can't grow an unbounded run (and so `reg` never overflows its
        // 16 bits). Codes are ≤ 10 bits + sentinel + up to 1 pending zero.
        if self.nbits > MAX_CODE_BITS + 1 {
            // Keep the LAST bit: it may be the first zero of a real separator
            // (or the first bit of the next code); everything older is junk.
            let last = self.reg & 1 == 1;
            self.reset();
            self.reg = (self.reg << 1) | u16::from(last);
            self.nbits = 1;
            self.cmin = self.cmin.min(conf.clamp(0.0, 1.0));
        }
        None
    }
}

#[cfg(test)]
pub(crate) mod testenc {
    //! TEST-ONLY encode direction (promoted to shipping code with Phase 2 TX).
    use super::VARICODE;

    /// Varicode-encode `text` into wire bits: each character's code followed by
    /// the `00` separator. Characters outside ASCII are dropped (the on-air
    /// alphabet is the table, exactly).
    pub fn encode_bits(text: &str) -> Vec<bool> {
        let mut out = Vec::new();
        for ch in text.chars() {
            let i = ch as usize;
            if i > 127 {
                continue;
            }
            out.extend(VARICODE[i].bytes().map(|b| b == b'1'));
            out.push(false);
            out.push(false);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_structurally_sound() {
        // The three invariants that make the stream self-framing. A typo that
        // breaks any of them is a decode failure on the air, not a style nit.
        let mut seen = std::collections::HashSet::new();
        for (i, code) in VARICODE.iter().enumerate() {
            assert!(!code.is_empty(), "char {i}: empty code");
            assert!(
                code.bytes().all(|b| b == b'0' || b == b'1'),
                "char {i}: non-bit in {code:?}"
            );
            assert!(!code.contains("00"), "char {i}: {code:?} contains 00");
            assert!(
                code.starts_with('1') && code.ends_with('1'),
                "char {i}: {code:?} must start and end with 1"
            );
            assert!(code.len() <= MAX_CODE_BITS as usize, "char {i}: too long");
            assert!(seen.insert(*code), "char {i}: duplicate code {code:?}");
        }
        // Frequency ordering spot checks — the table's identity, not just its shape.
        assert_eq!(VARICODE[b' ' as usize], "1");
        assert_eq!(VARICODE[b'e' as usize], "11");
        assert_eq!(VARICODE[b't' as usize], "101");
    }

    #[test]
    fn round_trips_all_128_characters() {
        // Encode every ASCII character and stream the bits back through the
        // decoder — the decode table and the encode table are the same data,
        // so a mismatch is a decoder-logic bug.
        let mut dec = VaricodeDecoder::new();
        for i in 0..128u8 {
            let ch = i as char;
            let mut got = None;
            for bit in testenc::encode_bits(&ch.to_string()) {
                if let Some(d) = dec.push(bit, 1.0) {
                    assert!(got.is_none(), "char {i}: decoded twice");
                    got = Some(d);
                }
            }
            let d = got.unwrap_or_else(|| panic!("char {i} ({ch:?}): nothing decoded"));
            assert_eq!(d.ch, ch, "char {i} round-trip");
            assert_eq!(d.confidence, 1.0);
        }
    }

    #[test]
    fn idle_prints_nothing() {
        // Continuous reversals (the PSK31 idle) are all zeros: nothing frames.
        let mut dec = VaricodeDecoder::new();
        for _ in 0..500 {
            assert!(dec.push(false, 1.0).is_none(), "idle must stay silent");
        }
    }

    #[test]
    fn unknown_and_overlong_codes_are_dropped() {
        let mut dec = VaricodeDecoder::new();
        // 1111111111 (10 ones) is not in the table — terminate it: no emit.
        for _ in 0..10 {
            assert!(dec.push(true, 1.0).is_none());
        }
        assert!(dec.push(false, 1.0).is_none());
        assert!(dec.push(false, 1.0).is_none(), "unknown code must drop");
        // A run of ones past the longest legal code (noise): the decoder must
        // bound its register and still decode cleanly afterwards.
        for _ in 0..64 {
            dec.push(true, 1.0);
        }
        dec.push(false, 1.0);
        dec.push(false, 1.0);
        let mut got = None;
        for bit in testenc::encode_bits("e") {
            if let Some(d) = dec.push(bit, 1.0) {
                got = Some(d);
            }
        }
        assert_eq!(
            got.map(|d| d.ch),
            Some('e'),
            "decoder must recover after garbage"
        );
    }

    #[test]
    fn character_confidence_is_the_minimum_bit_confidence() {
        // 'e' = "11" + "00": four pushes, worst bit 0.25 → char confidence 0.25.
        let mut dec = VaricodeDecoder::new();
        let confs = [0.9, 0.25, 0.8, 0.7];
        let mut got = None;
        for (bit, conf) in testenc::encode_bits("e").into_iter().zip(confs) {
            if let Some(d) = dec.push(bit, conf) {
                got = Some(d);
            }
        }
        let d = got.expect("char must decode");
        assert_eq!(d.ch, 'e');
        assert!((d.confidence - 0.25).abs() < 1e-6, "got {}", d.confidence);
    }

    #[test]
    fn reset_drops_a_partial_code() {
        let mut dec = VaricodeDecoder::new();
        // Half of 'A' ("1111101"), then reset, then a clean 'e' — the partial
        // must not weld onto it.
        for bit in [true, true, true] {
            dec.push(bit, 1.0);
        }
        dec.reset();
        let mut got = None;
        for bit in testenc::encode_bits("e") {
            if let Some(d) = dec.push(bit, 1.0) {
                got = Some(d);
            }
        }
        assert_eq!(got.map(|d| d.ch), Some('e'));
    }
}
