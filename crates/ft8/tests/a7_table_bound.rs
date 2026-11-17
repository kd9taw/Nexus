//! The Standard FT8 decoder's a7 table (`lib/ft8/ft8_a7.f90`, 200 entries per sequence) never
//! counts past its size.
//!
//! Upstream's `ft8_a7_save` counts every decode it is handed and only skips the write past 200,
//! so the count runs on; the next same-parity slot then copies that many entries out of a
//! 200-entry table, and the save's own search reads and writes past it. Decoding one slot again
//! (same `nutc`, the a7 pass on) saves its stations again, so eleven passes over the WSJT-X
//! sample hand the table more than 200 saves in one slot.
//!
//! Its own test binary: the a7 table is process-global modem state.

use ft8::{decode_frame_a7, NMAX};

fn sample() -> Vec<i16> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ft8_sample.wav");
    let b = std::fs::read(path).expect("read wav fixture");
    let mut i = 12usize;
    while i + 8 <= b.len() {
        let sz = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let body = i + 8;
        if &b[i..i + 4] == b"data" {
            let end = (body + sz).min(b.len());
            let mut s: Vec<i16> = b[body..end]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            s.resize(s.len().max(NMAX), 0);
            return s;
        }
        i = body + sz + (sz & 1);
    }
    panic!("no data chunk in {path}");
}

fn decode(iwave: &[i16], nutc: i32) -> usize {
    decode_frame_a7(
        iwave, 200, 4000, 3, "K1ABC", "W9XYZ", 0, 1500, 1500, nutc, true, false, true, false, false,
    )
    .len()
}

#[test]
fn the_a7_table_stops_counting_at_its_size() {
    let iwave = sample();
    ft8::a7_reset();
    let n = decode(&iwave, 0);
    let per_pass = ft8::a7_saved();
    assert!(
        n >= 15 && per_pass >= 15,
        "premise: {n} decodes, {per_pass} saved"
    );

    // The same slot again and again: each pass saves the slot's stations once more.
    for _ in 0..(200 / per_pass + 1) {
        decode(&iwave, 0);
    }
    assert_eq!(
        ft8::a7_saved(),
        200,
        "a full table holds 200 entries and counts no further"
    );

    // The next same-parity slot takes the full table over (the slot swap) and decodes as before.
    assert_eq!(
        decode(&iwave, 30),
        n,
        "the next slot decodes the sample whole"
    );
    ft8::a7_reset();
}
