//! WSJT-X 3.0.2's FT8 filter for rover (`/R`) and `TU; ` decodes (`lib/ft8/ft8b.f90:452-455`):
//! with no Special Operating Activity selected it discards a decode of message type 1-3 whose
//! text holds `/R` or starts `TU; `, since outside a contest those are almost always a corrupt
//! decode. In a contest (any activity) both decode.

use ft8::{decode_frame, encode, gen_wave, NMAX, SAMPLE_RATE};

/// Deterministic LCG + Box-Muller, as `decode_parity.rs`.
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

/// A 15 s frame holding `msg` at 1500 Hz and -5 dB: far above the threshold, so a missing
/// decode is the message layer, not sensitivity.
fn frame(msg: &str, seed: u64) -> Vec<i16> {
    let tones = encode(msg);
    let wave = gen_wave(&tones, SAMPLE_RATE, 1500.0);
    let sig = (2.0 * 2500.0 / (SAMPLE_RATE / 2.0)).sqrt() * 10f32.powf(0.05 * -5.0);
    let mut dd = vec![0f32; NMAX];
    for (i, &w) in wave.iter().enumerate() {
        if 6_000 + i < NMAX {
            dd[6_000 + i] += sig * w;
        }
    }
    let mut rng = Rng(seed);
    dd.iter()
        .map(|&s| (((s + rng.gauss()) * 100.0).clamp(-32768.0, 32767.0)) as i16)
        .collect()
}

/// Decode `msg` with AP armed for this station (`mycall` set, as the false "calling you" decodes
/// were), outside a contest (`activity = false`) or in one.
fn decoded(msg: &str, activity: bool) -> Vec<String> {
    decode_frame(
        &frame(msg, 7),
        200,
        2900,
        3,
        "KD9TAW",
        "",
        0,
        1500,
        true,
        false,
        activity,
    )
    .into_iter()
    .map(|d| d.message)
    .collect()
}

/// The rover and contest forms: type 1 with one or two `/R` calls, and the RTTY Roundup
/// (type 3) `TU; ` forms from upstream's own test list (`lib/ft8/ft8_testmsg.f90`).
const FILTERED: [&str; 6] = [
    "CQ TEST K1ABC/R FN42",
    "KD9TAW W1AW/R R FN42",
    "W1AW/R KD9TAW EN52",
    "WA9XYZ/R KA1ABC/R R FN42",
    "TU; KA0DEF K1ABC R 569 MA",
    "TU; G3AAA K1ABC R 559 MA",
];

/// Ordinary messages, calls that hold an `R` or a slash without being a rover, and `/R` in a
/// message type the filter does not read (free text is type 0): none is touched by the filter,
/// in a contest or out of one.
const UNTOUCHED: [&str; 10] = [
    "CQ W1AW FN31",
    "KD9TAW W1AW R-08",
    "W1AW KD9TAW RR73",
    "CQ R1ABC KO85",
    "K1ABC W9XYZ R 1D WI",
    "KD9TAW W1AW/P R JO22",
    "CQ TEST K1ABC FN42",
    "TNX W1AW/R 73",
    "<KD9TAW> PJ4/K1ABC/R",
    "PJ4/K1ABC/R <KD9TAW> RRR",
];

#[test]
fn outside_a_contest_rover_and_tu_decodes_are_dropped() {
    for msg in FILTERED {
        let got = decoded(msg, false);
        assert!(
            got.iter()
                .all(|m| !m.contains("/R") && !m.starts_with("TU; ")),
            "'{msg}' outside a contest must not decode, got {got:?}"
        );
    }
}

#[test]
fn in_a_contest_rover_and_tu_decodes_show() {
    for msg in FILTERED {
        let got = decoded(msg, true);
        assert!(
            got.iter().any(|m| m == msg),
            "'{msg}' in a contest must decode, got {got:?}"
        );
    }
}

#[test]
fn other_messages_decode_either_way() {
    for msg in UNTOUCHED {
        for activity in [false, true] {
            let got = decoded(msg, activity);
            assert!(
                got.iter().any(|m| m == msg),
                "'{msg}' (activity {activity}) must decode, got {got:?}"
            );
        }
    }
}
