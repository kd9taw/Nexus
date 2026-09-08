//! CRC-12 oracle: an INDEPENDENT implementation — modulo-2 polynomial long division over a
//! u128 holding the whole 88-bit word — compared with `js8::phy::crc12` on 10 000
//! LCG-random words, plus the six boost-computed vectors as the positive control that the
//! oracle itself is right (an oracle that merely mirrors the implementation proves nothing —
//! feedback-negative-results-need-a-positive-control).
//!
//! The two agree only if the bit order (MSB-first), the register width (12), the polynomial
//! (0xC06), the augmentation (88 bits divided, not 75 + implicit zeros) and the `^ 42` all
//! match; none of those is shared code.

use js8::phy::crc12;

/// Boost `augmented_crc<12, 0xc06>` = remainder of the 88-bit message polynomial divided by
/// P(x) = x¹² + x¹¹ + x¹⁰ + x² + x (0x1C06 with the implicit x¹² bit), then `^ 42`.
fn crc12_polynomial_oracle(word: &[u8; 11]) -> u16 {
    let mut m: u128 = 0;
    for (i, &b) in word.iter().enumerate() {
        m |= u128::from(b) << (8 * (10 - i));
    }
    // Bit index i (MSB-first, 0 = first on the air) is u128 bit 87 − i; bits 75..=87 → low 13 bits.
    m &= !((1u128 << 13) - 1);
    const P: u128 = 0x1C06;
    for shift in (0..=75).rev() {
        if m & (1u128 << (shift + 12)) != 0 {
            m ^= P << shift;
        }
    }
    (m & 0xFFF) as u16 ^ 42
}

/// Deterministic LCG (Knuth MMIX constants) — the same shape crates/ft8/tests/decode_parity.rs uses.
fn lcg(state: &mut u64) -> u32 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*state >> 33) as u32
}

#[test]
fn oracle_reproduces_the_boost_vectors() {
    let vectors: [([u8; 11], u16); 6] = [
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
    for (bytes, want) in vectors {
        assert_eq!(
            crc12_polynomial_oracle(&bytes),
            want,
            "oracle on {bytes:02x?}"
        );
        assert_eq!(crc12(&bytes), want, "impl on {bytes:02x?}");
    }
}

#[test]
fn implementation_agrees_with_the_polynomial_oracle_on_10k_random_words() {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut disagreements = Vec::new();
    for _ in 0..10_000 {
        let mut w = [0u8; 11];
        for b in w.iter_mut() {
            *b = (lcg(&mut state) & 0xFF) as u8;
        }
        // Random garbage in the CRC field too — both sides must ignore bits 75..=87.
        let a = crc12(&w);
        let b = crc12_polynomial_oracle(&w);
        if a != b {
            disagreements.push((w, a, b));
        }
    }
    assert!(
        disagreements.is_empty(),
        "first disagreements: {:02x?}",
        &disagreements[..disagreements.len().min(5)]
    );
}

#[test]
fn every_single_bit_flip_in_the_75_message_bits_changes_the_crc() {
    // A CRC with a degree-12 polynomial detects every single-bit error over this length.
    let base = [
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x11, 0x80, 0,
    ];
    let want = crc12(&base);
    for bit in 0..75 {
        let mut w = base;
        w[bit / 8] ^= 0x80 >> (bit % 8);
        assert_ne!(crc12(&w), want, "flip of bit {bit} not detected");
    }
}
