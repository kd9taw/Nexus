//! CRC-12 over the 87-bit JS8 word — the acceptance test the BP decoder and the frame layer share.
//!
//! JS8Call computes `boost::augmented_crc<12, 0xc06>(bytes, 11) ^ 42` (JS8.cpp:896-901,
//! lib/crc12.cpp:10, lib/js8/genjs8.f90:44-62) over the 11-byte MSB-first word with the CRC
//! field (bits 75..=86) and the unused bit 87 zeroed first (chkcrc12a.f90:16-19). Boost's
//! "augmented" CRC is plain modulo-2 long division of the whole 88-bit bit-string by
//! x¹² + x¹¹ + x¹⁰ + x² + x with a zero initial register and no reflection — the register
//! form below shifts each data bit in and reduces when a bit falls out of the 12-bit window.
//! Then `^ 42` (the "TODO jsherer" constant that every JS8Call on the air uses).
//!
//! WHY this lives in `phy` and nowhere else: JS8Call's BP loop uses `checkCRC12` as its
//! acceptance test (JS8.cpp:1387-1392), and the message layer verifies the same field. Two
//! copies that drift would silently zero the decode rate; `tests/crc12_oracle.rs` proves
//! this one against an independent polynomial-division implementation and the boost pins.

/// The truncated generator polynomial (lib/crc12.cpp `POLY 0xc06`).
const POLY: u16 = 0x0C06;
/// JS8Call's post-XOR constant (genjs8.f90:49, chkcrc12a.f90:19, JS8.cpp:901).
const XOR_OUT: u16 = 42;

/// CRC-12 of the 87-bit word held in `word_bytes` (MSB-first, 11 bytes). Bits 75..=87 of the
/// input are ignored (zeroed here), so it may be called on a stamped word to verify it.
pub fn crc12(word_bytes: &[u8; 11]) -> u16 {
    let mut b = *word_bytes;
    b[9] &= 0xE0; // keep the 3 i3 bits, drop the top 5 CRC bits
    b[10] = 0; // drop the low 7 CRC bits and the unused bit 87
    let mut rem: u16 = 0;
    for byte in b {
        for k in (0..8).rev() {
            let bit = u16::from((byte >> k) & 1);
            let top = (rem >> 11) & 1;
            rem = ((rem << 1) | bit) & 0x0FFF;
            if top == 1 {
                rem ^= POLY;
            }
        }
    }
    rem ^ XOR_OUT
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Six inputs computed by the real `boost::augmented_crc<12, 0xc06>` (the call JS8Call
    /// makes) via ~/work/twowayfd/paritylab/js8/crc12_boost_oracle.cpp on 2026-09-05. Values are
    /// AFTER the `^ 42`. A wrong bit order, a wrong polynomial, a wrong register width or a
    /// missing augmentation each break at least one of these.
    const BOOST_VECTORS: [([u8; 11], u16); 6] = [
        ([0; 11], 0x02A),
        (
            [
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xe0, 0,
            ],
            0xE84,
        ),
        (
            [
                0x00, 0x10, 0x83, 0x10, 0x51, 0x87, 0x20, 0x92, 0x8b, 0x20, 0,
            ],
            0x406,
        ),
        ([0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 0xBD6),
        ([0, 0, 0, 0, 0, 0, 0, 0, 0, 0x20, 0], 0x420),
        (
            [
                0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x11, 0x80, 0,
            ],
            0x866,
        ),
    ];

    #[test]
    fn matches_boost_augmented_crc_on_the_pinned_vectors() {
        for (bytes, want) in BOOST_VECTORS {
            assert_eq!(crc12(&bytes), want, "bytes {bytes:02x?}");
        }
    }

    #[test]
    fn ignores_whatever_sits_in_the_crc_field() {
        // chkcrc12a.f90:16-17 / JS8.cpp:919-921: the receiver zeroes bits 75..=86 before
        // recomputing — so the fn must give the same answer with the CRC field populated.
        let (clean, want) = BOOST_VECTORS[2];
        let mut stamped = clean;
        stamped[9] |= 0x1F;
        stamped[10] = 0xFE;
        assert_eq!(crc12(&stamped), want);
        // bit 87 (byte 10 bit 0) is zeroed too — it is outside the 87-bit word.
        stamped[10] = 0x01;
        assert_eq!(crc12(&stamped), want);
    }

    #[test]
    fn result_fits_twelve_bits() {
        for (bytes, _) in BOOST_VECTORS {
            assert_eq!(crc12(&bytes) & !0x0FFF, 0);
        }
    }
}
