//! The JTDX-style FT4 decoder stops adding decodes once its per-period list is full, instead of
//! writing past it.
//!
//! JTDX's FT4 decoder appends each new decode of a period to a local `decodes(100)` with no check
//! against its size. No FT4 band this test can build reaches 100 distinct decodes, so the test
//! lowers the bound through the test-only `jtdx_set_decode_caps` (the app never calls it) and
//! drives the real WSJT-X sample, 16 decodes, into it. That proves the guard stops the list at the
//! bound and that what came before is untouched; that the shipped bound is the array's own size
//! is in the source (`min(size(decodes), ...)`), not in this test.
//!
//! Its own test binary (one process): the bound is process-global, and no other decode may run
//! while it is lowered.

use ft4::{decode_frame_jtdx, load_callsign_db, reset_jtdx, NMAX};

fn sample() -> Vec<i16> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ft4_sample.wav");
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
            s.resize(NMAX, 0);
            return s;
        }
        i = body + sz + (sz & 1);
    }
    panic!("no data chunk in {path}");
}

fn decode() -> Vec<(String, i32, u32, u32)> {
    reset_jtdx();
    decode_frame_jtdx(&sample(), 200, 2900, 3, "", "", 0, 0, false, false)
        .iter()
        .map(|d| (d.message.clone(), d.snr, d.dt.to_bits(), d.freq.to_bits()))
        .collect()
}

#[test]
fn a_full_decode_list_stops_the_period_at_its_bound() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../libtempo/vendor/wsjtx");
    assert!(
        load_callsign_db(std::path::Path::new(dir)),
        "ALLCALL7.TXT loads"
    );
    let full = decode();
    assert_eq!(full.len(), 16, "the sample's 16 decodes: {full:?}");

    const CAP: i32 = 4;
    // SAFETY: no decode is running; this binary runs no other test.
    unsafe { tempo_fast_sys::jtdx_set_decode_caps(0, CAP) };
    let capped = decode();
    // SAFETY: as above.
    unsafe { tempo_fast_sys::jtdx_set_decode_caps(0, 0) };

    assert_eq!(
        capped,
        full[..CAP as usize],
        "at the bound the list stops: the first {CAP} decodes, exactly as found, and no more"
    );
    assert_eq!(
        decode(),
        full,
        "the restored bound decodes the period whole"
    );
}
