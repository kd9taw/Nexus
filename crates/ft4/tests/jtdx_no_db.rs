//! The JTDX-style FT4 decoder with NO callsign database loaded: an operator who runs only FT4,
//! before (or without) the engine pointing the decoders at ALLCALL7.TXT.
//!
//! JTDX's false-decode checks look a call up in that database. Not yet read and not marked
//! invalid, it holds nothing, and every check would REJECT: CQ decodes below -15 dB dropped and,
//! for a `<call> <call> R <grid>` decode, the whole period. So the FT4 entry runs the shared
//! loader itself; with no file to read it marks the database invalid, and the checks then pass
//! every call. This file is its own test binary because the database is loaded once per process.

use ft4::{decode_frame_jtdx, encode, gen_wave, reset_jtdx, NMAX, SAMPLE_RATE};

/// The frame `jtdx.rs` uses for the reject-one-decode case: an `R` message from two calls the
/// database does not hold, at 400 Hz, then eight ordinary messages.
fn frame() -> Vec<i16> {
    let mut msgs = vec![("K9QZX W8QZJ R EN52", 400.0f32)];
    for (i, m) in [
        "CQ K1ABC FN42",
        "W9XYZ G4ABC IO91",
        "CQ KD9TAW EN52",
        "W1AW K1ABC -12",
        "G4ABC W9XYZ R-08",
        "CQ W1AW FN31",
        "KD9TAW G4ABC RR73",
        "K1ABC W1AW 73",
    ]
    .into_iter()
    .enumerate()
    {
        msgs.push((m, 700.0 + 230.0 * i as f32));
    }
    let bw_ratio = 2500.0 / (SAMPLE_RATE / 2.0);
    let sig = (2.0 * bw_ratio).sqrt() * 10f32.powf(0.05 * -8.0);
    let mut dd = vec![0f32; NMAX];
    for (m, f0) in msgs {
        for (d, &w) in dd.iter_mut().zip(&gen_wave(&encode(m), SAMPLE_RATE, f0)) {
            *d += sig * w;
        }
    }
    // A fixed pseudo-random noise floor, the same every run.
    let mut seed = 0x9E37_79B9u32;
    dd.iter()
        .map(|&s| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let n = ((seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 3.4;
            ((s + n) * 100.0).clamp(-32768.0, 32767.0) as i16
        })
        .collect()
}

#[test]
fn without_the_callsign_database_nothing_is_rejected() {
    reset_jtdx();
    let d = decode_frame_jtdx(&frame(), 200, 2900, 3, "", "", 0, 0, false, false);
    let msgs: Vec<&str> = d.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        d.len(),
        9,
        "every signal decodes, none is rejected: {msgs:?}"
    );
    assert!(msgs.contains(&"K9QZX W8QZJ R EN52"), "{msgs:?}");
}
