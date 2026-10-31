//! WSJT-X decodes nothing from a period whose audio is under half an LSB rms. WSJT-X 3.0.2,
//! `lib/decoder.f90:92-93`, at the top of `multimode_decoder`, before any mode's decoder runs:
//!
//! ```text
//! rms=sqrt(dot_product(float(id2(1:180000)),float(id2(1:180000)))/180000.0)
//! if(rms.lt.0.5) go to 800
//! ```
//!
//! `id2` is the period's 12 kHz int16 audio, the frame every pass hands the decoder (jt9 zeroes
//! what a pass has not heard yet, `lib/jt9.f90:427-441`, as the radio loop's frames do), and label
//! 800 is past every decoder call: no pass runs, the a7 replay included. Without it, one stray LSB
//! after a busy period was enough for the a7 replay to "hear" that period's stations again.
//!
//! The decoder is driven here exactly as the radio loop drives it, through the streaming entry the
//! engine uses, over the frames it builds: the period's first `len` samples at the front, the tail
//! zeroed (`RxRing::frame_latest_padded`).

use std::sync::{Mutex, MutexGuard};

use ft8::{a7_reset, decode_frame_a7_streaming, encode, gen_wave, Decode, Progress, NMAX};

/// The a7 table is process-wide: tests that run a period sequence must not interleave.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// One pass as the radio loop runs it: how much of the period's audio it holds, and its two
/// pass-shape flags.
#[derive(Clone, Copy, Debug)]
struct Pass {
    name: &'static str,
    len: usize,
    a7_final: bool,
    partial: bool,
}

/// WSJT-X's early pass at 41 half-symbols (11.808 s): cheap, no a7.
const EARLY: Pass = Pass {
    name: "early",
    len: 41 * 3456,
    a7_final: false,
    partial: true,
};
/// The full pass at 50 half-symbols (14.40 s): the period's decode, with the a7 save and replay.
const FULL: Pass = Pass {
    name: "full",
    len: 50 * 3456,
    a7_final: true,
    partial: false,
};
/// The boundary's own decode of the whole period, when the full pass did not run.
const BOUNDARY: Pass = Pass {
    name: "boundary",
    len: NMAX,
    a7_final: true,
    partial: false,
};
/// F6, the review re-decode of the retained period.
const REDECODE: Pass = Pass {
    name: "redecode",
    len: NMAX,
    a7_final: false,
    partial: false,
};
const PASSES: [Pass; 4] = [EARLY, FULL, BOUNDARY, REDECODE];

/// `audio`'s first `pass.len` samples at the front of a decoder frame, the rest zero.
fn frame(audio: &[i16], pass: Pass) -> Vec<i16> {
    let mut f = vec![0i16; NMAX];
    let n = pass.len.min(audio.len());
    f[..n].copy_from_slice(&audio[..n]);
    f
}

/// Run `pass` over `audio` as slot key `nutc`. Returns the decodes and how many sync candidates the
/// decoder tried. What it streamed on the way (what the engine acts on as it arrives) must be what
/// it returned.
fn run(audio: &[i16], pass: Pass, nutc: i32) -> (Vec<Decode>, usize) {
    let mut candidates = 0;
    let mut streamed = Vec::new();
    let decodes = decode_frame_a7_streaming(
        &frame(audio, pass),
        200,
        2900,
        3,
        "W9XYZ",
        "",
        0,
        1500,
        1500,
        nutc,
        pass.a7_final,
        pass.partial,
        true,
        false,
        false,
        &mut |p| {
            match p {
                Progress::Candidate => candidates += 1,
                Progress::Found(d) => streamed.push(d.message.clone()),
                Progress::Rescue => {}
            }
            false
        },
    );
    assert_eq!(
        streamed,
        messages(&decodes),
        "{}: streamed what it returned",
        pass.name
    );
    (decodes, candidates)
}

fn messages(d: &[Decode]) -> Vec<&str> {
    d.iter().map(|d| d.message.as_str()).collect()
}

fn sum_sq(f: &[i16]) -> i64 {
    f.iter().map(|&s| i64::from(s) * i64::from(s)).sum()
}

fn read_wav_i16(path: &str) -> Vec<i16> {
    let b = std::fs::read(path).expect("read wav fixture");
    let mut i = 12usize; // skip "RIFF"<size>"WAVE"
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

/// The off-air WSJT-X sample: a busy band.
fn busy_band() -> Vec<i16> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ft8_sample.wav");
    let mut s = read_wav_i16(path);
    s.resize(NMAX, 0);
    s
}

const MSG: &str = "CQ K1ABC FN42";

/// `MSG` at 1500 Hz from the 0.5 s start, alone, turned down until the frame `pass` hands the
/// decoder has a sum of squares of exactly `target` (its rms is `sqrt(target / 180000)` LSB). The
/// gain is bisected to just under the target and the rest is made up with single LSBs in the
/// silent lead-in.
fn signal_at(pass: Pass, target: i64) -> Vec<i16> {
    let mut wave = vec![0f32; 6_000];
    wave.extend(gen_wave(&encode(MSG), ft8::SAMPLE_RATE, 1500.0));
    wave.resize(NMAX, 0.0);
    let at = |g: f32| -> Vec<i16> {
        let s: Vec<i16> = wave.iter().map(|&w| (w * g).round() as i16).collect();
        frame(&s, pass)
    };
    let (mut lo, mut hi) = (0f32, 4f32);
    for _ in 0..60 {
        let g = (lo + hi) / 2.0;
        if sum_sq(&at(g)) <= target {
            lo = g;
        } else {
            hi = g;
        }
    }
    let mut f = at(lo);
    let short = target - sum_sq(&f);
    assert!(
        (0..6_000).contains(&short),
        "{}: {short} LSBs to make up",
        pass.name
    );
    f[..short as usize].fill(1);
    assert_eq!(sum_sq(&f), target, "{}", pass.name);
    f
}

/// The floor is WSJT-X's to the sample: a period at exactly 0.5 LSB rms over the whole 180000
/// samples (sum of squares 45000) is decoded, one LSB less (44999, rms 0.49999) is not. It holds on
/// every pass the radio loop runs, and the decoder never starts below it.
#[test]
fn the_floor_is_half_an_lsb_rms_over_the_whole_frame_on_every_pass() {
    let _serial = serial();
    for pass in PASSES {
        a7_reset();
        let (d, _) = run(&signal_at(pass, 45_000), pass, 0);
        assert!(
            messages(&d).contains(&MSG),
            "{}: at 0.5 LSB rms the signal decodes: {:?}",
            pass.name,
            messages(&d)
        );
        a7_reset();
        let (d, candidates) = run(&signal_at(pass, 44_999), pass, 0);
        assert!(
            d.is_empty(),
            "{}: under 0.5 LSB rms nothing decodes: {:?}",
            pass.name,
            messages(&d)
        );
        assert_eq!(candidates, 0, "{}: the decoder never ran", pass.name);
    }
    // The other two entries are held to the same floor.
    let (at, under) = (signal_at(REDECODE, 45_000), signal_at(REDECODE, 44_999));
    let plain = |f: &[i16]| ft8::decode_frame(f, 200, 2900, 3, "", "", 0, 1500, true, false, false);
    let a7 = |f: &[i16]| {
        ft8::decode_frame_a7(
            f, 200, 2900, 3, "", "", 0, 1500, 1500, 0, false, false, true, false, false,
        )
    };
    assert!(messages(&plain(&at)).contains(&MSG), "decode_frame at 0.5");
    assert!(plain(&under).is_empty(), "decode_frame under 0.5");
    assert!(messages(&a7(&at)).contains(&MSG), "decode_frame_a7 at 0.5");
    assert!(a7(&under).is_empty(), "decode_frame_a7 under 0.5");
}

/// The reported case: a busy period, then the next same-parity period silent but for one sample of
/// one LSB (rms 0.0024 LSB). Before the floor the full pass's a7 replay "decoded" four of the busy
/// period's stations out of it at -24 dB (three CQs and a report); WSJT-X decodes nothing there,
/// and now neither pass does.
#[test]
fn one_lsb_after_a_busy_period_decodes_nothing() {
    let _serial = serial();
    a7_reset();
    let busy = busy_band();
    // Period A, as the radio loop decodes it: the early pass, then the full pass (a7 save).
    run(&busy, EARLY, 0);
    let (a, _) = run(&busy, FULL, 0);
    assert!(
        a.len() >= 18,
        "the premise, a busy band: {:?}",
        messages(&a)
    );

    let mut quiet = vec![0i16; NMAX];
    quiet[90_000] = 1;
    for pass in [EARLY, FULL] {
        let (d, _) = run(&quiet, pass, 30);
        assert!(
            d.is_empty(),
            "{}: one LSB after a busy period decodes nothing: {:?}",
            pass.name,
            messages(&d)
        );
    }
}

/// What the floor leaves alone. Exact zeros after a busy period decode nothing, as they always did.
/// Noise just above the floor, a quiet card's hiss, still reaches the decoder. And a weak station
/// at a normal receive level still decodes on every pass.
#[test]
fn zeros_quiet_noise_and_a_weak_signal_are_decoded_as_before() {
    let _serial = serial();
    a7_reset();
    let busy = busy_band();
    run(&busy, EARLY, 0);
    run(&busy, FULL, 0);
    for pass in [EARLY, FULL] {
        let (d, _) = run(&vec![0i16; NMAX], pass, 30);
        assert!(d.is_empty(), "{}: zeros: {:?}", pass.name, messages(&d));
    }

    // Deterministic Gaussian noise (LCG + Box-Muller), sigma LSB.
    let noise = |sigma: f64, mut seed: u64| -> Vec<i16> {
        let mut u = move || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((seed >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        };
        (0..NMAX)
            .map(|_| {
                let (u1, u2) = (u(), u());
                let g = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
                (sigma * g).round() as i16
            })
            .collect()
    };
    for (sigma, nutc) in [(1.0, 60), (4.0, 90)] {
        let hiss = noise(sigma, 0x0206 + nutc as u64);
        assert!(
            sum_sq(&frame(&hiss, EARLY)) >= 45_000,
            "sigma {sigma}: above the floor on every pass"
        );
        for pass in [EARLY, FULL] {
            let (_, candidates) = run(&hiss, pass, nutc);
            assert!(
                candidates > 0,
                "sigma {sigma}, {}: noise above the floor reaches the decoder",
                pass.name
            );
        }
    }

    // A station at -18 dB in 2500 Hz, in band noise of 100 LSB rms (the level WSJT-X's ft8sim writes).
    let weak = {
        let wave = gen_wave(&encode(MSG), ft8::SAMPLE_RATE, 1500.0);
        let sig = (2.0 * 2500.0 / 6000.0f64).sqrt() * 10f64.powf(0.05 * -18.0);
        let hiss = noise(100.0, 0x5EED);
        (0..NMAX)
            .map(|i| {
                let w = i
                    .checked_sub(6_000)
                    .and_then(|j| wave.get(j))
                    .map_or(0.0, |&w| f64::from(w));
                (f64::from(hiss[i]) + 100.0 * sig * w).round() as i16
            })
            .collect::<Vec<i16>>()
    };
    for pass in PASSES {
        a7_reset();
        let (d, _) = run(&weak, pass, 120);
        assert!(
            messages(&d).contains(&MSG),
            "{}: a weak station at a normal level decodes: {:?}",
            pass.name,
            messages(&d)
        );
    }
}
