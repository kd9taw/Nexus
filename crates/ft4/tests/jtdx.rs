//! The JTDX-style FT4 decoder (JTDX 2.2.159's, `ft4jtdx_cabi.f90`), through the Rust API.
//!
//! Decode for decode, the vendored port was proven equal to JTDX's own unmodified FT4 decoder on
//! a bench (blind, AP-armed, CQ-only, depths 1-3; see the FT4 decoder notes in the CHANGELOG).
//! These tests pin what CI can hold it to: the real WSJT-X sample, a synthetic crowd, silence on
//! noise, the streaming and plain calls agreeing, and a reset making it start afresh.

use ft4::{
    decode_frame, decode_frame_jtdx, decode_frame_streaming_jtdx, encode, gen_wave,
    load_callsign_db, reset_jtdx, Decode, NMAX, SAMPLE_RATE,
};

/// Load the callsign database the JTDX-style checks read (ALLCALL7.TXT as Nexus ships it), as
/// the engine does at start-up. Once per process; every test calls it first.
fn db() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../libtempo/vendor/wsjtx");
    assert!(
        load_callsign_db(std::path::Path::new(dir)),
        "ALLCALL7.TXT loads from {dir}"
    );
}

fn read_wav_i16(path: &str) -> Vec<i16> {
    let b = std::fs::read(path).expect("read wav fixture");
    let mut i = 12usize;
    while i + 8 <= b.len() {
        let sz = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let body = i + 8;
        if &b[i..i + 4] == b"data" {
            let end = (body + sz).min(b.len());
            return b[body..end]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
        }
        i = body + sz + (sz & 1);
    }
    panic!("no data chunk in {path}");
}

struct Rng(u64);
impl Rng {
    fn next_f64(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn gauss(&mut self) -> f32 {
        let u1 = (self.next_f64() + 1e-12).min(1.0);
        let u2 = self.next_f64();
        ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
    }
}

/// `signals` (message, audio frequency, SNR in dB in 2500 Hz) summed into one frame of AWGN.
fn frame(signals: &[(&str, f32, f32)], seed: u64) -> Vec<i16> {
    let bw_ratio = 2500.0 / (SAMPLE_RATE / 2.0);
    let mut dd = vec![0f32; NMAX];
    for &(msg, f0, snr_db) in signals {
        let sig = (2.0 * bw_ratio).sqrt() * 10f32.powf(0.05 * snr_db);
        for (d, &w) in dd.iter_mut().zip(&gen_wave(&encode(msg), SAMPLE_RATE, f0)) {
            *d += sig * w;
        }
    }
    let mut rng = Rng(seed);
    dd.iter()
        .map(|&s| ((s + rng.gauss()) * 100.0).clamp(-32768.0, 32767.0) as i16)
        .collect()
}

fn jtdx_blind(iwave: &[i16]) -> Vec<Decode> {
    decode_frame_jtdx(iwave, 200, 2900, 3, "", "", 0, 0, false, false)
}

fn messages(v: &[Decode]) -> Vec<String> {
    let mut m: Vec<String> = v.iter().map(|d| d.message.clone()).collect();
    m.sort();
    m
}

/// On the real WSJT-X FT4 sample (a busy contest period) the JTDX-style decoder finds exactly the
/// messages the Standard decoder finds: measured, 16 each, the same 16.
#[test]
fn jtdx_decodes_the_wsjtx_sample_as_standard_does() {
    db();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ft4_sample.wav");
    let mut s = read_wav_i16(path);
    s.resize(NMAX, 0);
    reset_jtdx();
    let jtdx = jtdx_blind(&s);
    let standard = decode_frame(&s, 200, 2900, 3, "", "", 0, 0, false);
    assert_eq!(jtdx.len(), 16, "JTDX-style decodes: {:?}", messages(&jtdx));
    assert_eq!(messages(&jtdx), messages(&standard));
    assert!(
        jtdx.iter()
            .all(|d| d.qual == 1.0 && d.sync == 0.0 && (0..=6).contains(&d.nap)),
        "every decode is CRC-checked (quality 1), with JTDX's AP types 0-6: {jtdx:?}"
    );
}

/// Four signals, one of them weak and on the RX frequency, all decode; the RX frequency's comes
/// first, as JTDX tries the candidates within 20 Hz of it first.
#[test]
fn jtdx_decodes_a_crowd_rx_frequency_first() {
    db();
    let iwave = frame(
        &[
            ("CQ K1ABC FN31", 700.0, -8.0),
            ("W9XYZ K2DEF EM79", 1100.0, -8.0),
            ("CQ N3GHI FM19", 2200.0, -8.0),
            ("W9XYZ K4JKL R-07", 1500.0, -14.0),
        ],
        0x51A7,
    );
    reset_jtdx();
    let d = decode_frame_jtdx(&iwave, 200, 2900, 3, "W9XYZ", "", 0, 1500, false, false);
    assert_eq!(
        messages(&d),
        [
            "CQ K1ABC FN31",
            "CQ N3GHI FM19",
            "W9XYZ K2DEF EM79",
            "W9XYZ K4JKL R-07"
        ]
    );
    assert_eq!(d[0].message, "W9XYZ K4JKL R-07");
}

/// The streaming call hands on each decode as it is found, in the order the plain call returns
/// them, and returns exactly what the plain call returns.
#[test]
fn jtdx_streaming_hands_on_what_the_plain_call_returns() {
    db();
    let iwave = frame(
        &[
            ("CQ K1ABC FN31", 900.0, -10.0),
            ("W9XYZ K2DEF EM79", 1800.0, -10.0),
        ],
        7,
    );
    let fields = |v: &[Decode]| -> Vec<String> { v.iter().map(|d| format!("{d:?}")).collect() };
    reset_jtdx();
    let plain = jtdx_blind(&iwave);
    reset_jtdx();
    let mut found = Vec::new();
    let streamed =
        decode_frame_streaming_jtdx(&iwave, 200, 2900, 3, "", "", 0, 0, false, false, &mut |d| {
            found.push(d.clone())
        });
    assert_eq!(plain.len(), 2, "{plain:?}");
    assert_eq!(fields(&streamed), fields(&plain));
    assert_eq!(fields(&found), fields(&plain));
}

/// The decoder carries the running average DT of earlier periods into the next one; after
/// `reset_jtdx` the same periods decode exactly as they did the first time.
#[test]
fn jtdx_reset_starts_afresh() {
    db();
    let periods: Vec<Vec<i16>> = (0..4)
        .map(|k| {
            frame(
                &[
                    ("CQ K1ABC FN31", 800.0 + 100.0 * k as f32, -12.0),
                    ("W9XYZ K2DEF EM79", 1700.0, -15.0),
                    ("CQ N3GHI FM19", 2400.0, -16.0),
                ],
                100 + k,
            )
        })
        .collect();
    let run = || -> Vec<String> {
        reset_jtdx();
        periods
            .iter()
            .flat_map(|p| jtdx_blind(p))
            .map(|d| format!("{d:?}"))
            .collect()
    };
    let first = run();
    assert!(first.len() >= 8, "{first:?}");
    assert_eq!(run(), first);
}

/// Pure noise decodes to nothing, AP armed with real calls over every QSO stage, CQ-only on and
/// off. A false decode reaches the log and the upload funnel.
#[test]
fn jtdx_decodes_nothing_from_noise() {
    db();
    reset_jtdx();
    for f in 0..60u64 {
        let iwave = frame(&[], 0x4EED_0000 + f);
        let progress = (f % 6) as i32;
        let d = decode_frame_jtdx(
            &iwave,
            200,
            2900,
            3,
            "K1ABC",
            "W9XYZ",
            progress,
            1500,
            f % 2 == 1,
            false,
        );
        assert!(d.is_empty(), "frame {f} (progress {progress}): {d:?}");
    }
}

