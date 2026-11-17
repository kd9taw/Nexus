//! The JTDX-style FT8 decoder (`lib/ft8var`) stops adding decodes once its per-period list is
//! full, instead of writing past it.
//!
//! Upstream appends each new decode of a period to ft8_mod1's `allmessages(200)` with no check
//! against its size. No band, natural or synthetic, that this test can build reaches 200 distinct
//! decodes, so the test lowers the bound through the test-only `jtdx_set_decode_caps` (the app
//! never calls it) and drives the real WSJT-X sample, 20 decodes, into it. That proves the guard
//! stops the list at the bound and that what came before is untouched; that the shipped bound is
//! the array's own size is in the source (`min(size(allmessages), ...)`), not in this test.
//!
//! Its own test binary (one process): the bound is process-global, and no other decode may run
//! while it is lowered.

use ft8::{decode_frame_var_raw, Decode, Ft8VarParams, NMAX};

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

/// One thread, so the decode order is deterministic (ft8var.rs's jt9 parameters).
fn params() -> Ft8VarParams {
    let mut p = Ft8VarParams {
        nzhsym: 49,
        nfa: 200,
        nfb: 4000,
        nfqso: 1500,
        napwid: 75,
        nmt: 1,
        nft8cycles: 3,
        nft8rxfsens: 3,
        ncandthin: 100,
        lft8lowth: 1,
        lft8subpass: 1,
        lft8apon: 1,
        lcommonft8b: 1,
        lwidedxcsearch: 1,
        ..Ft8VarParams::default()
    };
    p.mycall = *b"K1ABC       ";
    p.hiscall = *b"W9XYZ       ";
    p.mybcall = [0; 12];
    p.hisbcall = [0; 12];
    p.hisgrid = *b"EN37  ";
    p
}

/// A fresh state, then one decode of the sample on a thread with the decoder's stack.
fn decode() -> Vec<(String, i32, u32, u32)> {
    tempo_fast_sys::DecoderCtx::new().scoped(|| {});
    let iwave = sample();
    let p = params();
    let v: Vec<Decode> = std::thread::Builder::new()
        .stack_size(ft8::OPENMP_THREAD_STACK)
        .spawn(move || {
            let share = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../libtempo/vendor/wsjtx");
            assert!(ft8::var_load_callsign_db(&share), "ALLCALL7.TXT loads");
            decode_frame_var_raw(&iwave, &p, &mut |_| {})
        })
        .expect("spawn")
        .join()
        .expect("decoder thread");
    v.iter()
        .map(|d| (d.message.clone(), d.snr, d.dt.to_bits(), d.freq.to_bits()))
        .collect()
}

#[test]
fn a_full_decode_list_stops_the_period_at_its_bound() {
    let full = decode();
    assert_eq!(full.len(), 20, "the sample's 20 decodes: {full:?}");

    const CAP: i32 = 5;
    // SAFETY: no decode is running; this binary runs no other test.
    unsafe { tempo_fast_sys::jtdx_set_decode_caps(CAP, 0) };
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
