//! The multithreaded FT8 decoder (WSJT-X 3.0.2 `lib/ft8var`, its port of JTDX's decoder).
//!
//! * One thread decodes the real WSJT-X sample exactly as WSJT-X's own `jt9 -8 -M -C 3 -E 3 -R 3
//!   -N 1` does: the same 20 records, in the same order, with the same SNR, DT and frequency. The
//!   golden list is jt9's own output (WSJT-X v3.0.2, built from its tag); `scripts/ft8var-parity.py`
//!   holds the same line on the lab corpora (559 files identical at vendoring).
//! * One thread is deterministic: the same audio after the same reset decodes byte-identically.
//! * On the sample it finds every message the standard decoder finds at Deep.
//! * The thread slices are WSJT-X's, including its 11- and 12-thread quirk.
//!
//! The decoder's local arrays live on the stack (it is compiled -fopenmp), so every decode here
//! runs on a thread with `OPENMP_THREAD_STACK`, as the app's decode thread does. Every test takes
//! `SERIAL`: the decoder keeps process-global state between periods.

use std::sync::Mutex;

use ft8::{decode_frame, decode_frame_var_raw, Decode, Ft8VarParams, NMAX};

static SERIAL: Mutex<()> = Mutex::new(());

fn read_wav_i16(path: &str) -> Vec<i16> {
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

fn sample() -> Vec<i16> {
    read_wav_i16(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ft8_sample.wav"
    ))
}

fn share_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libtempo/vendor/wsjtx")
}

/// `jt9 -8 -M -C 3 -E 3 -R 3 -N 1`'s parameters from disk (jt9.f90:336-412): nzhsym 49 (decoder
/// start 3), 200-4000 Hz, RX 1500 Hz, napwid 75, AP on, K1ABC / W9XYZ / EN37, and what jt9 leaves
/// unset left zero (no TX frequency, the calls not flagged standard, NUL base calls). The
/// fixture's name carries no time, so jt9's nutc is 0.
fn jt9_params() -> Ft8VarParams {
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

/// Run `f` on a thread with the stack the decoder needs.
fn on_decoder_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(ft8::OPENMP_THREAD_STACK)
        .spawn(f)
        .expect("spawn")
        .join()
        .expect("decoder thread")
}

/// A fresh process's ft8var state: what a new radio chain starts from.
fn fresh_var_state() {
    tempo_fast_sys::DecoderCtx::new().scoped(|| {});
}

fn var_decode(iwave: Vec<i16>, p: Ft8VarParams) -> Vec<Decode> {
    on_decoder_thread(move || {
        assert!(
            ft8::var_load_callsign_db(&share_dir()),
            "ALLCALL7.TXT must load from the vendored tree"
        );
        decode_frame_var_raw(&iwave, &p, &mut |_| {})
    })
}

/// Fortran's nint (halves away from zero), as jt9 prints the frequency.
fn nint(x: f32) -> i32 {
    let r = (x.abs() + 0.5).floor() as i32;
    if x < 0.0 {
        -r
    } else {
        r
    }
}

/// WSJT-X 3.0.2 `jt9 -8 -M -C 3 -E 3 -R 3 -N 1 ft8_sample.wav`, column for column: SNR, DT, the
/// frequency as jt9 prints it, and the message.
const JT9_GOLDEN: &[(i32, f32, i32, &str)] = &[
    (-7, -0.8, 1196, "CQ F5RXL IN94"),
    (0, -0.1, 2157, "WM3PEN EA6VQ -09"),
    (0, 0.3, 2571, "W1FC F5BZB -08"),
    (-9, 0.1, 723, "A92EE F5PSR -14"),
    (-14, 0.3, 641, "N1JFU EA6EE R-07"),
    (-11, -0.1, 2695, "K1BZM EA3GP -09"),
    (-13, 0.3, 590, "K1JT HA0DU KN07"),
    (-9, 0.4, 2734, "W1DIG SV9CVY -14"),
    (-13, 0.3, 2239, "N1API HA6FQ -23"),
    (-15, 0.1, 2279, "CQ EA2BFM IN83"),
    (-18, 0.7, 1513, "N1API F2VX 73"),
    (-12, 0.3, 465, "N1PJT HB9CQK -10"),
    (-15, 0.1, 1648, "K1JT EA3AGB -15"),
    (-14, 0.2, 2854, "XE2X HA2NP RR73"),
    (-15, 0.3, 399, "W0RSJ EA3BMU RR73"),
    (-10, 0.4, 472, "KD2UGC F6GCP R-23"),
    (-19, 0.5, 244, "K1BZM DK8NE -10"),
    (-18, 0.1, 2039, "K1JT HA5WA 73"),
    (-12, 0.2, 2522, "K1BZM EA3CJ JN01"),
    (-15, -0.1, 2546, "WA2FZW DL5AXX RR73"),
];

#[test]
fn one_thread_decodes_the_sample_exactly_as_wsjtx_jt9_does() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    fresh_var_state();
    let got: Vec<(i32, f32, i32, String)> = var_decode(sample(), jt9_params())
        .iter()
        .map(|d| {
            (
                d.snr,
                (d.dt * 10.0).round() / 10.0,
                nint(d.freq),
                d.message.trim().to_string(),
            )
        })
        .collect();
    let want: Vec<(i32, f32, i32, String)> = JT9_GOLDEN
        .iter()
        .map(|&(s, dt, f, m)| (s, dt, f, m.to_string()))
        .collect();
    assert_eq!(
        got, want,
        "Nexus's ft8var must be WSJT-X's, record for record"
    );
}

#[test]
fn one_thread_is_deterministic() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    fresh_var_state();
    // Every field, floats bit for bit.
    let bits = |v: Vec<Decode>| -> Vec<(String, i32, u32, u32, i32, u32)> {
        v.iter()
            .map(|d| {
                let (dt, f, q) = (d.dt.to_bits(), d.freq.to_bits(), d.qual.to_bits());
                (d.message.clone(), d.snr, dt, f, d.nap, q)
            })
            .collect()
    };
    let a = bits(var_decode(sample(), jt9_params()));
    fresh_var_state();
    let b = bits(var_decode(sample(), jt9_params()));
    assert!(!a.is_empty());
    assert_eq!(a, b, "the same audio from the same state decodes the same");
}

#[test]
fn finds_every_message_the_standard_decoder_finds_on_the_sample() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    fresh_var_state();
    let var: std::collections::HashSet<String> = var_decode(sample(), jt9_params())
        .iter()
        .map(|d| d.message.trim().to_string())
        .collect();
    let iw = sample();
    let std_decodes = on_decoder_thread(move || {
        decode_frame(
            &iw, 200, 4000, 3, "K1ABC", "W9XYZ", 0, 1500, true, false, false,
        )
    });
    let missed: Vec<_> = std_decodes
        .iter()
        .map(|d| d.message.trim().to_string())
        .filter(|m| !var.contains(m))
        .collect();
    assert!(
        var.len() >= std_decodes.len(),
        "ft8var {} decodes, standard {}",
        var.len(),
        std_decodes.len()
    );
    assert!(
        missed.is_empty(),
        "the standard decoder alone found {missed:?}"
    );
}

#[test]
fn thread_slices_are_wsjtx_s_including_its_11_and_12_thread_quirk() {
    // decoder.f90:352-1060: nfdelta = nint(|nfb-nfa|/n), edges nfa+k*nfdelta, the last slice
    // runs to nfb; at 11 and 12 threads the next-to-last edge adds `nfdelt` (undeclared: 0).
    assert_eq!(ft8::var_slices(200, 4000, 1), vec![(200, 4000)]);
    assert_eq!(
        ft8::var_slices(200, 4000, 2),
        vec![(200, 2100), (2101, 4000)]
    );
    let s4 = ft8::var_slices(200, 4000, 4);
    assert_eq!(
        s4,
        vec![(200, 1150), (1151, 2100), (2101, 3050), (3051, 4000)]
    );
    let s12 = ft8::var_slices(200, 4000, 12);
    // nfdelta = nint(3800/12) = 317: edges 517, 834, ... 3370 (k = 10), then 3370 again.
    assert_eq!(s12[9], (3054, 3370));
    assert_eq!(
        s12[10],
        (3371, 3370),
        "the 11th slice is empty, as WSJT-X's"
    );
    assert_eq!(s12[11], (3371, 4000), "and the 12th takes its share");
    let s11 = ft8::var_slices(200, 4000, 11);
    assert_eq!(
        s11[9].0,
        s11[9].1 + 1,
        "11 threads: the 10th slice is empty"
    );
}

/// Linking the decoder must not take stack from every thread of the process. Thread-local storage
/// is carved out of each thread's stack, and upstream's subtractft8var kept 1.44 MB of it
/// (`save` + `!$omp threadprivate`): with it, a 2 MiB thread that never decodes FT8 overflowed.
/// A 1 MiB thread here, in a binary that links ft8var, must still have 900 KB to use.
#[test]
fn linking_the_decoder_leaves_every_thread_its_stack() {
    #[inline(never)]
    fn use_stack(n: usize) -> u64 {
        let mut buf = [0u8; 64 * 1024];
        std::hint::black_box(&mut buf);
        buf[n % buf.len()] = n as u8;
        let here = u64::from(std::hint::black_box(buf[n % buf.len()]));
        if n == 0 {
            here
        } else {
            here + use_stack(n - 1)
        }
    }
    // Keep the decoder in this binary: the test is about what linking it costs.
    std::hint::black_box(decode_frame_var_raw as *const ());
    let used = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| use_stack(14)) // 15 frames of 64 KiB: 960 KiB
        .expect("spawn")
        .join();
    assert!(
        used.is_ok(),
        "a 1 MiB thread could not use 960 KiB of its stack"
    );
}

// --- JTDX's SWL mode (libtempo/ft8var_swl.f90) -------------------------------------------------

/// `msg` at 1200 Hz starting `dt` seconds after the 0.5 s nominal start, ~+20 dB over a
/// deterministic noise floor, in a 15 s period.
fn synth_at_dt(msg: &str, dt: f32) -> Vec<i16> {
    let tones = ft8::encode(msg);
    assert!(!tones.is_empty(), "{msg} must encode");
    let wave = ft8::gen_wave(&tones, 12_000.0, 1200.0);
    let mut out = vec![0f32; NMAX];
    let mut seed = 0x1234_5678u32;
    for s in out.iter_mut() {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        *s = ((seed >> 8) % 601) as f32 - 300.0;
    }
    let start = ((0.5 + dt) * 12_000.0).round() as isize;
    for (i, &v) in wave.iter().enumerate() {
        let at = start + i as isize;
        if (0..NMAX as isize).contains(&at) {
            out[at as usize] += v * 3_000.0;
        }
    }
    out.iter()
        .map(|&v| v.clamp(-32767.0, 32767.0) as i16)
        .collect()
}

fn messages(d: &[Decode]) -> std::collections::HashSet<String> {
    d.iter().map(|d| d.message.trim().to_string()).collect()
}

#[test]
fn swl_off_is_wsjtx_s_decoder_and_swl_finds_what_it_finds_on_the_sample() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    fresh_var_state();
    let off = messages(&var_decode(sample(), jt9_params()));
    fresh_var_state();
    let on = messages(&var_decode(
        sample(),
        Ft8VarParams {
            lswl: 1,
            ..jt9_params()
        },
    ));
    let missed: Vec<_> = off.difference(&on).collect();
    assert!(missed.is_empty(), "only SWL off found {missed:?}");
    // And SWL off is still exactly jt9 -M: the golden test above runs it with lswl 0.
}

#[test]
fn swl_searches_dt_out_to_3_5_s_where_wsjtx_stops_at_2_5_s() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let late = synth_at_dt("CQ W9XYZ EN37", 2.7);
    fresh_var_state();
    let off = messages(&var_decode(late.clone(), jt9_params()));
    fresh_var_state();
    let on = messages(&var_decode(
        late,
        Ft8VarParams {
            lswl: 1,
            ..jt9_params()
        },
    ));
    assert!(
        !off.contains("CQ W9XYZ EN37"),
        "control: DT +2.7 s is outside WSJT-X's search, got {off:?}"
    );
    assert!(on.contains("CQ W9XYZ EN37"), "SWL found {on:?}");
}

#[test]
fn a_callsign_database_load_that_found_no_file_is_retried_from_the_right_folder() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let nowhere = std::env::temp_dir().join("ft8var-no-allcall-here");
    assert!(on_decoder_thread(
        || ft8::var_load_callsign_db(&share_dir())
    ));
    assert!(
        !on_decoder_thread(move || ft8::var_load_callsign_db(&nowhere)),
        "control: no ALLCALL7.TXT there, and the folder changed"
    );
    assert!(
        on_decoder_thread(|| ft8::var_load_callsign_db(&share_dir())),
        "the load from the right folder takes effect again"
    );
    // Loaded twice, the arrays hold the file once (a doubled load would overrun them), and the
    // sample decodes exactly as jt9 does.
    fresh_var_state();
    let got: Vec<(i32, i32, String)> = var_decode(sample(), jt9_params())
        .iter()
        .map(|d| (d.snr, nint(d.freq), d.message.trim().to_string()))
        .collect();
    let want: Vec<(i32, i32, String)> = JT9_GOLDEN
        .iter()
        .map(|&(s, _, f, m)| (s, f, m.to_string()))
        .collect();
    assert_eq!(got, want);
}

/// Pure noise (Gaussian, σ 300, seeds 1219 and 1294 of the 3000-period AP-armed noise check) from
/// which the decoder, as `jt9 -8 -M -C 3 -E 3 -R 3 -N 1` (and WSJT-X's own jt9 the same), takes a
/// message that passes the CRC: a line whose two calls are both unresolved hashes, and a CQ whose
/// call is no call. Nexus shows each marked unchecked, so it is never sequenced, logged, handed
/// to other programs or uploaded on its own; the standard decoder finds nothing in either.
#[test]
fn noise_decodes_of_a_shape_no_station_sends_are_marked() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for (file, want) in [
        ("ft8var_noise_two_hashes.wav", "<...> <...> R 521761 CG33TI"),
        ("ft8var_noise_cq_no_call.wav", "CQ GI576RBFOEB"),
    ] {
        let wav = read_wav_i16(&format!(
            "{}/tests/fixtures/{file}",
            env!("CARGO_MANIFEST_DIR")
        ));
        fresh_var_state();
        let d = var_decode(wav.clone(), jt9_params());
        assert_eq!(
            d.iter().map(|d| d.message.trim()).collect::<Vec<_>>(),
            [want],
            "{file}: jt9's one decode"
        );
        assert!(d[0].unchecked, "{file}: marked");
        let standard = on_decoder_thread(move || {
            decode_frame(
                &wav, 200, 4000, 3, "K1ABC", "W9XYZ", 0, 1500, true, false, false,
            )
        });
        assert!(
            standard.is_empty(),
            "{file}: the standard decoder finds nothing"
        );
    }
}

/// Pure noise (seeds 50365 and 52560 of the review's fresh noise check) from which SWL mode's
/// wider search takes a CRC-passing message: telemetry, and a nonstandard call beside an unresolved
/// hash. Both are marked unchecked with SWL on.
#[test]
fn swl_noise_decodes_of_a_shape_no_station_sends_are_marked() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for (file, want) in [
        ("ft8var_noise_swl_telemetry.wav", "4BDB9B0B2A69EBA968"),
        ("ft8var_noise_swl_nonstd_hash.wav", "<...> K38MCFJYGQX 73"),
    ] {
        let wav = read_wav_i16(&format!(
            "{}/tests/fixtures/{file}",
            env!("CARGO_MANIFEST_DIR")
        ));
        fresh_var_state();
        let mut p = jt9_params();
        p.lswl = 1;
        let d = var_decode(wav, p);
        let found: Vec<_> = d.iter().filter(|d| d.message.trim() == want).collect();
        assert_eq!(found.len(), 1, "{file}: SWL decodes {want:?}: {d:?}");
        assert!(found[0].unchecked, "{file}: marked");
    }
}

/// Pure noise (seed 52889 of the review's fresh noise check) from which the decoder, as WSJT-X's own
/// `jt9 -8 -M -C 3 -E 3 -R 3 -N 1` does, takes `CQ N8DEK/R CN77` with a-priori help at a quality
/// under 0.17: the line WSJT-X prints with `?` (lib/decoder.f90 ft8_decodedvar, `if(nap.ne.0)` ...
/// `if(qual.lt.0.17) decoded0(37:37)='?'`). The maintainer's ruling: match WSJT-X. It is NOT
/// marked unchecked; it carries the a-priori type and quality that make the UDP Decode
/// low-confidence and keep it from PSK Reporter (tempo-audio's wire and spot rules).
#[test]
fn a_low_confidence_noise_decode_is_wsjtx_s_question_mark_not_a_mark() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let wav = read_wav_i16(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ft8var_noise_low_confidence.wav"
    ));
    fresh_var_state();
    let d = var_decode(wav, jt9_params());
    assert_eq!(
        d.iter().map(|d| d.message.trim()).collect::<Vec<_>>(),
        ["CQ N8DEK/R CN77"],
        "jt9's one decode"
    );
    assert!(
        d[0].nap != 0 && d[0].qual < 0.17,
        "WSJT-X's `?`: {:?}",
        d[0]
    );
    assert!(!d[0].unchecked, "shown as WSJT-X shows it, not marked `^`");
}
