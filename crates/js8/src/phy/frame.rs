//! The `phy` ⇄ `proto` seam: the 87-bit JS8 word and its two halves.
//!
//! Layout (JS8.cpp:2662-2712, genjs8.f90:1003 `12b6.6,b3.3,b12.12`), MSB-first in 11 bytes:
//! bits 0..=71 payload (twelve 6-bit chars), bits 72..=74 the i3 value MSB-first (bit 72 =
//! Data 4, bit 73 = Last 2, bit 74 = First 1 — js8dec.f90 `i3bit = 4*d(73)+2*d(74)+d(75)`),
//! bits 75..=86 CRC-12 (Task B1.3), bit 87 unused and always 0.
//!
//! WHY a typed word and not text: the engine's `TxWaveform::Js8` carries a `Word87` (a typed
//! 87-bit value must never round-trip through a string — sentinels make consumers guess), and
//! `Decode.raw` carries `as_bytes()` back up. [`Word87::new`] is the ONLY constructor that
//! computes a CRC; [`Word87::from_bytes`] wraps decoder output verbatim so a caller can
//! [`Word87::verify`] it. The i3 placement is encapsulated HERE only and pinned by B4's
//! `tests/golden_frames.rs` (the ALL.TXT `<i3>` column); if that golden test disagrees, only
//! this file changes.
//!
//! [`Word87::from_lab`] / [`Word87::to_lab`] (plan interface addendum) are the lab/debug text
//! form `"<12 sixbit chars> <i3>"` (ALL.TXT's two columns) that B2/B3/B5 all parse — never a
//! private copy. They live here rather than deferring to `proto::alphabet::{sixbit_to_string,
//! sixbit_from_str}` (Task B1.7) because the module direction runs the other way: `proto`
//! depends on `phy`, never the reverse, so `phy` carries its own small, independently tested
//! copy of JS8.cpp:849-892's 64-character alphabet for this one lab-text purpose only.

use crate::phy::crc12::crc12;

/// The i3 transmission-type bits (varicode.h:35-40): First = 1, Last = 2, Data = 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct I3 {
    /// First frame of a message (clears any reassembly buffer at that offset).
    pub first: bool,
    /// Last frame of a message (closes the buffer).
    pub last: bool,
    /// "Fast data": the 72 payload bits are a JSC-dense text frame with no type header.
    pub data: bool,
}

impl I3 {
    /// `first | last << 1 | data << 2`.
    pub const fn to_u8(self) -> u8 {
        (self.first as u8) | ((self.last as u8) << 1) | ((self.data as u8) << 2)
    }

    /// Inverse of [`I3::to_u8`]; bits above the low three are ignored.
    pub const fn from_u8(v: u8) -> I3 {
        I3 {
            first: v & 1 != 0,
            last: v & 2 != 0,
            data: v & 4 != 0,
        }
    }
}

/// 72 payload bits MSB-first in 9 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Payload72([u8; 9]);

impl Payload72 {
    pub const fn from_bytes(b: [u8; 9]) -> Payload72 {
        Payload72(b)
    }

    pub const fn as_bytes(&self) -> &[u8; 9] {
        &self.0
    }

    /// The 72 bits as 0/1 bytes, MSB-first.
    pub fn bits(&self) -> [u8; 72] {
        let mut out = [0u8; 72];
        for (i, o) in out.iter_mut().enumerate() {
            *o = (self.0[i / 8] >> (7 - (i % 8))) & 1;
        }
        out
    }

    pub fn from_bits(bits: &[u8; 72]) -> Payload72 {
        let mut b = [0u8; 9];
        for (i, &bit) in bits.iter().enumerate() {
            if bit & 1 == 1 {
                b[i / 8] |= 0x80 >> (i % 8);
            }
        }
        Payload72(b)
    }

    /// The twelve 6-bit values (ALL.TXT's 12-char column via `proto::alphabet::SIXBIT`).
    pub fn chars12(&self) -> [u8; 12] {
        let bits = self.bits();
        let mut out = [0u8; 12];
        for (c, o) in out.iter_mut().enumerate() {
            *o = bits[6 * c..6 * c + 6]
                .iter()
                .fold(0u8, |acc, &b| (acc << 1) | b);
        }
        out
    }

    /// Inverse of [`Payload72::chars12`]; each value is masked to 6 bits (JS8.cpp:2682-2687 shifts).
    pub fn from_chars12(c: [u8; 12]) -> Payload72 {
        let mut bits = [0u8; 72];
        for (i, &v) in c.iter().enumerate() {
            for k in 0..6 {
                bits[6 * i + k] = (v >> (5 - k)) & 1;
            }
        }
        Payload72::from_bits(&bits)
    }

    /// Top 3 bits → `proto::FrameType` (000 Heartbeat … 1xx Data; varicode.h:42-58).
    pub const fn type_bits(&self) -> u8 {
        self.0[0] >> 5
    }
}

/// The 64-character alphabet JS8.cpp:849-892 uses for the sixbit lab text form: '0'..'9',
/// 'A'..'Z', 'a'..'z', '-', '+'. A local, independently tested copy — see the module header
/// for why `phy` cannot depend on the eventual canonical `proto::alphabet::SIXBIT` (Task B1.7).
const SIXBIT_CHARS: [u8; 64] = *b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-+";

/// 87 info bits MSB-first in 11 bytes: bits 0..=71 payload, 72..=74 i3, 75..=86 CRC-12, bit 87 unused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Word87([u8; 11]);

impl Word87 {
    /// Pack payload + i3 and stamp the CRC (the only constructor that computes a CRC).
    pub fn new(payload: Payload72, i3: I3) -> Word87 {
        let mut b = [0u8; 11];
        b[..9].copy_from_slice(payload.as_bytes());
        b[9] = i3.to_u8() << 5;
        let crc = crc12(&b);
        b[9] |= ((crc >> 7) & 0x1F) as u8;
        b[10] = ((crc & 0x7F) << 1) as u8;
        Word87(b)
    }

    /// Wrap decoder / `Decode.raw` bytes verbatim (no check — call [`Word87::verify`]).
    pub const fn from_bytes(b: [u8; 11]) -> Word87 {
        Word87(b)
    }

    pub const fn as_bytes(&self) -> &[u8; 11] {
        &self.0
    }

    /// 87 bits as 0/1 bytes, MSB-first — what the LDPC encoder consumes.
    pub fn bits(&self) -> [u8; 87] {
        let mut out = [0u8; 87];
        for (i, o) in out.iter_mut().enumerate() {
            *o = (self.0[i / 8] >> (7 - (i % 8))) & 1;
        }
        out
    }

    /// Inverse of [`Word87::bits`]; bit 87 is left 0.
    pub fn from_bits(bits: &[u8; 87]) -> Word87 {
        let mut b = [0u8; 11];
        for (i, &bit) in bits.iter().enumerate() {
            if bit & 1 == 1 {
                b[i / 8] |= 0x80 >> (i % 8);
            }
        }
        Word87(b)
    }

    pub fn payload72(&self) -> Payload72 {
        let mut p = [0u8; 9];
        p.copy_from_slice(&self.0[..9]);
        Payload72(p)
    }

    pub fn i3(&self) -> I3 {
        I3::from_u8(self.0[9] >> 5)
    }

    /// The CRC field as stored (bits 75..=86): `bytes[9] & 0x1F` is the high 5 bits, `bytes[10] >> 1` the low 7.
    pub fn crc12(&self) -> u16 {
        (u16::from(self.0[9] & 0x1F) << 7) | u16::from(self.0[10] >> 1)
    }

    /// stored CRC == `phy::crc12(bytes)`.
    pub fn verify(&self) -> bool {
        self.crc12() == crc12(&self.0)
    }

    /// Lab/debug text form: `"<12 sixbit chars> <i3>"` (ALL.TXT's two columns), e.g.
    /// `"0123456789AB 3"`. Round-trips with [`Word87::to_lab`]. Not used by the engine — the
    /// engine's `TxWaveform::Js8` carries a typed `Word87` — this exists for tests, the lab
    /// CLIs and fixtures that speak this exact form.
    pub fn from_lab(s: &str) -> Option<Word87> {
        let mut it = s.split_whitespace();
        let chars_str = it.next()?;
        let i3_str = it.next()?;
        if it.next().is_some() {
            return None; // exactly two fields
        }
        let chars_bytes = chars_str.as_bytes();
        if chars_bytes.len() != 12 {
            return None;
        }
        let mut chars12 = [0u8; 12];
        for (dst, &b) in chars12.iter_mut().zip(chars_bytes) {
            *dst = SIXBIT_CHARS.iter().position(|&c| c == b)? as u8;
        }
        let i3: u8 = i3_str.parse().ok()?;
        if i3 > 7 {
            return None;
        }
        Some(Word87::new(
            Payload72::from_chars12(chars12),
            I3::from_u8(i3),
        ))
    }

    /// Inverse of [`Word87::from_lab`].
    pub fn to_lab(&self) -> String {
        let mut s = String::with_capacity(14);
        for v in self.payload72().chars12() {
            s.push(SIXBIT_CHARS[(v & 0x3F) as usize] as char);
        }
        s.push(' ');
        s.push((b'0' + self.i3().to_u8()) as char);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "0123456789AB" as 6-bit values 0..=11 (JS8.cpp:849 alphabet order), packed 4 chars per
    /// 3 bytes (JS8.cpp:2680-2690).
    const CHARS: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
    const PAYLOAD: [u8; 9] = [0x00, 0x10, 0x83, 0x10, 0x51, 0x87, 0x20, 0x92, 0x8B];

    #[test]
    fn payload_packs_twelve_sixbit_chars_msb_first() {
        let p = Payload72::from_chars12(CHARS);
        assert_eq!(*p.as_bytes(), PAYLOAD);
        assert_eq!(p.chars12(), CHARS);
        assert_eq!(p.type_bits(), 0b000, "top 3 bits of char 0 = 0");
        let all63 = Payload72::from_chars12([63; 12]);
        assert_eq!(*all63.as_bytes(), [0xFF; 9]);
        assert_eq!(all63.type_bits(), 0b111);
        assert_eq!(
            Payload72::from_chars12([0x40 | 5; 12]).chars12(),
            [5; 12],
            "values are masked to 6 bits"
        );
    }

    #[test]
    fn payload_bits_round_trip_and_are_msb_first() {
        let p = Payload72::from_bytes([0x80, 0, 0, 0, 0, 0, 0, 0, 0x01]);
        let b = p.bits();
        assert_eq!(b[0], 1);
        assert_eq!(b[71], 1);
        assert_eq!(b[1..71].iter().sum::<u8>(), 0);
        assert_eq!(Payload72::from_bits(&b), p);
    }

    #[test]
    fn i3_value_is_first_bit0_last_bit1_data_bit2() {
        // varicode.h:35-40: JS8CallFirst = 1, JS8CallLast = 2, JS8CallData = 4.
        assert_eq!(
            I3 {
                first: true,
                last: false,
                data: false
            }
            .to_u8(),
            1
        );
        assert_eq!(
            I3 {
                first: false,
                last: true,
                data: false
            }
            .to_u8(),
            2
        );
        assert_eq!(
            I3 {
                first: false,
                last: false,
                data: true
            }
            .to_u8(),
            4
        );
        assert_eq!(
            I3::from_u8(7),
            I3 {
                first: true,
                last: true,
                data: true
            }
        );
        assert_eq!(
            I3::from_u8(0x0B),
            I3::from_u8(3),
            "only the low 3 bits are the i3 value"
        );
        assert_eq!(I3::default().to_u8(), 0);
    }

    #[test]
    fn word_layout_matches_js8_cpp_encode_and_the_boost_crc() {
        // JS8.cpp:2662-2712: bytes[9] = (type & 7) << 5 | (crc >> 7) & 0x1F; bytes[10] = (crc & 0x7F) << 1.
        // CRC of this payload with i3 = 1 is 0x406 (boost oracle, Task B1.3) → 0x28, 0x0C.
        let w = Word87::new(
            Payload72::from_bytes(PAYLOAD),
            I3 {
                first: true,
                last: false,
                data: false,
            },
        );
        let mut want = [0u8; 11];
        want[..9].copy_from_slice(&PAYLOAD);
        want[9] = 0x28;
        want[10] = 0x0C;
        assert_eq!(*w.as_bytes(), want);
        assert_eq!(w.crc12(), 0x406);
        assert!(w.verify());
        assert_eq!(w.payload72(), Payload72::from_bytes(PAYLOAD));
        assert_eq!(
            w.i3(),
            I3 {
                first: true,
                last: false,
                data: false
            }
        );
    }

    #[test]
    fn i3_occupies_bits_72_to_74_msb_first() {
        // genjs8.f90:1003 format `12b6.6,b3.3,b12.12` and js8dec.f90 `i3bit = 4*d(73)+2*d(74)+d(75)`
        // (1-based): bit 72 carries Data (4), bit 73 Last (2), bit 74 First (1).
        let w = Word87::new(
            Payload72::from_bytes([0; 9]),
            I3 {
                first: false,
                last: false,
                data: true,
            },
        );
        let b = w.bits();
        assert_eq!((b[72], b[73], b[74]), (1, 0, 0));
        let w = Word87::new(
            Payload72::from_bytes([0; 9]),
            I3 {
                first: true,
                last: false,
                data: false,
            },
        );
        let b = w.bits();
        assert_eq!((b[72], b[73], b[74]), (0, 0, 1));
        assert_eq!(w.as_bytes()[9] >> 5, 0b001);
    }

    #[test]
    fn bits_round_trip_and_bit_87_is_always_zero() {
        let w = Word87::new(Payload72::from_bytes([0xA5; 9]), I3::from_u8(6));
        let b = w.bits();
        assert_eq!(b.len(), 87);
        assert_eq!(Word87::from_bits(&b), w);
        assert_eq!(w.as_bytes()[10] & 1, 0);
        let mut dirty = *w.as_bytes();
        dirty[10] |= 1;
        assert_eq!(
            Word87::from_bytes(dirty).bits(),
            b,
            "bit 87 never appears in bits()"
        );
    }

    #[test]
    fn verify_rejects_any_single_bit_flip_and_from_bytes_does_not_check() {
        let w = Word87::new(Payload72::from_bytes(PAYLOAD), I3::from_u8(3));
        for bit in 0..87 {
            let mut bytes = *w.as_bytes();
            bytes[bit / 8] ^= 0x80 >> (bit % 8);
            let flipped = Word87::from_bytes(bytes);
            assert!(!flipped.verify(), "flip of bit {bit} accepted");
        }
        assert!(w.verify());
    }

    #[test]
    fn lab_text_form_round_trips() {
        // Interface addendum (plan §"Interface addenda"): Word87::from_lab/to_lab, the lab text
        // form "<12 sixbit chars> <i3>" — the ALL.TXT two-column shape B2/B3/B5 all parse.
        let w = Word87::new(Payload72::from_chars12(CHARS), I3::from_u8(3));
        assert_eq!(w.to_lab(), "0123456789AB 3");
        assert_eq!(Word87::from_lab("0123456789AB 3"), Some(w));
    }

    #[test]
    fn lab_text_form_covers_the_full_sixbit_alphabet() {
        let chars = [61, 61, 61, 61, 61, 61, 61, 61, 61, 61, 62, 63];
        let w = Word87::new(Payload72::from_chars12(chars), I3::default());
        assert_eq!(w.to_lab(), "zzzzzzzzzz-+ 0");
        assert_eq!(Word87::from_lab("zzzzzzzzzz-+ 0"), Some(w));
    }

    #[test]
    fn from_lab_rejects_malformed_input() {
        assert_eq!(Word87::from_lab("0123456789AB"), None, "missing i3 field");
        assert_eq!(
            Word87::from_lab("0123456789AB 3 extra"),
            None,
            "extra field"
        );
        assert_eq!(Word87::from_lab("0123456789A 3"), None, "eleven chars");
        assert_eq!(Word87::from_lab("0123456789ABC 3"), None, "thirteen chars");
        assert_eq!(Word87::from_lab("0123456789AB 8"), None, "i3 out of range");
        assert_eq!(
            Word87::from_lab("0123456789A/ 3"),
            None,
            "'/' is outside the 64-char alphabet"
        );
        assert_eq!(Word87::from_lab("0123456789AB x"), None, "i3 not a number");
    }

    /// The drift guard team-lead required after B1.2's `phy::frame` doc comment claimed a
    /// hand-mirrored copy was "independently tested" with nothing actually enforcing it: a
    /// comment is not a guard. `phy` cannot depend on `proto` (module direction), so
    /// `SIXBIT_CHARS` here and `proto::alphabet::SIXBIT` (Task B1.8, JS8.cpp:849-892's same
    /// 64-character alphabet) are two copies by construction — this test is what keeps them
    /// byte-identical instead of a comment's word.
    #[test]
    fn sixbit_chars_matches_proto_alphabet_sixbit_byte_for_byte() {
        assert_eq!(
            SIXBIT_CHARS,
            crate::proto::alphabet::SIXBIT,
            "phy::frame::SIXBIT_CHARS has drifted from proto::alphabet::SIXBIT — fix whichever \
             one no longer matches JS8.cpp:849-892"
        );
    }
}
