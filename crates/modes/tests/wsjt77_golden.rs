//! ⭐ THE LOCK-FREE FT8/FT4 ENCODER AGAINST THE VENDORED ONE: the same tones for every message, the
//! same refusals, the same waveform to float rounding.
//!
//! `modes::wsjt77` replaces the vendored Fortran encoder on the transmit path so an over built at its
//! key never waits for a decode holding the modem. It may only do that if it sends exactly what
//! the vendored encoder sends, so every test here runs BOTH on the same input and compares:
//!
//! * every message type the 77-bit format has, in the forms Nexus composes (QSO steps, directed
//!   CQs, /P and /R, compound and hashed calls, Field Day, RTTY Roundup, EU VHF, DXpedition,
//!   telemetry, free text, WSPR forms) and at their edges (every report, every grid, every section
//!   and multiplier, serial and power ranges);
//! * a generated corpus of well-formed messages (a fixed-seed generator, so a failure reproduces);
//! * a generated corpus of arbitrary printable text, where the Rust encoder may decline a message
//!   (`None`: the vendored encoder makes it), and must match wherever it does not.
//!
//! The vendored encoder is called ONLY where the Rust one answers: some inputs stop the Fortran
//! program (a list-directed read with no `err=`), and those are among the ones it declines.

use modes::wsjt77::{ft4_refusal, ft4_tones, ft4_wave, ft8_refusal, ft8_tones, ft8_wave, Refusal};
use modes::{make_mode, ModeKind};

/// Both encoders on `msg`, both modes. `must_encode`: a message Nexus can send, which the Rust
/// encoder may not hand back to the vendored one.
fn same_tones(msg: &str, must_encode: bool) -> usize {
    let mut declined = 0;
    match ft8_tones(msg) {
        Some(tones) => assert_eq!(tones, ft8::encode(msg), "FT8 tones differ for {msg:?}"),
        None => {
            assert!(!must_encode, "FT8: the Rust encoder declined {msg:?}");
            declined += 1;
        }
    }
    match ft4_tones(msg) {
        Some(tones) => assert_eq!(tones, ft4::encode(msg), "FT4 tones differ for {msg:?}"),
        None => {
            assert!(!must_encode, "FT4: the Rust encoder declined {msg:?}");
            declined += 1;
        }
    }
    declined
}

/// A small fixed-seed generator (splitmix64), so every failure names a reproducible input.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
    fn chars(&mut self, alphabet: &[u8], n: usize) -> String {
        (0..n).map(|_| *self.pick(alphabet) as char).collect()
    }
}

const LETTERS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &[u8] = b"0123456789";

const CALLS: &[&str] = &[
    "K1ABC", "W9XYZ", "KD9TAW", "K2DEF", "G4ABC", "VE3XYZ", "JA1ABC", "9A1AA", "4X1AB", "A61AB",
    "VK2ABC", "ZL1ABC", "KH6AB", "KL7ABC", "N0A", "W1AW", "2E0ABC", "M0XYZ", "7Q7XX", "5B4AAB",
    "3DA0XYZ", "3X1ABC", "E51ABC", "OH2BH", "PA3XYZ", "GM4ABC", "DL1ABC", "HB9ABC", "W5A",
];
const PORTABLE: &[&str] = &[
    "K1ABC/R", "W9XYZ/R", "G4ABC/P", "PA3XYZ/P", "KD9TAW/R", "GM4ABC/P",
];
const COMPOUND: &[&str] = &[
    "PJ4/K1ABC",
    "KH1/KH7Z",
    "VP2V/K1ABC",
    "ZK3/K1ABC",
    "K1ABC/4",
    "W9XYZ/KH6",
    "YW18FIFA",
    "K1ABC/QRP",
    "3D2/K1ABC",
    "VP8/G4ABC",
    "KH6/W9XYZ",
    "TX5A",
];
const CQ_TOKENS: &[&str] = &[
    "DX", "NA", "EU", "TEST", "POTA", "SOTA", "WW", "FD", "RU", "QP", "040", "145", "ABCD", "JA",
];

fn grid4(rng: &mut Rng) -> String {
    let r = b"ABCDEFGHIJKLMNOPQR";
    format!(
        "{}{}{}",
        rng.chars(r, 2),
        rng.chars(DIGITS, 1),
        rng.chars(DIGITS, 1)
    )
}

fn grid6(rng: &mut Rng) -> String {
    format!(
        "{}{}",
        grid4(rng),
        rng.chars(b"ABCDEFGHIJKLMNOPQRSTUVWX", 2)
    )
}

fn report(n: i32) -> String {
    format!("{n:+03}")
}

fn std_call(rng: &mut Rng) -> String {
    let prefix = match rng.below(4) {
        0 => rng.chars(LETTERS, 1),
        1 => rng.chars(LETTERS, 2),
        2 => format!("{}{}", rng.chars(DIGITS, 1), rng.chars(LETTERS, 1)),
        _ => format!("{}{}", rng.chars(LETTERS, 1), rng.chars(DIGITS, 1)),
    };
    let suffix_len = 1 + rng.below(3);
    format!(
        "{prefix}{}{}",
        rng.chars(DIGITS, 1),
        rng.chars(LETTERS, suffix_len)
    )
    .replace('Q', "K")
}

fn any_call(rng: &mut Rng) -> String {
    match rng.below(8) {
        0 => rng.pick(PORTABLE).to_string(),
        1 => rng.pick(COMPOUND).to_string(),
        2 => format!("{}/{}", std_call(rng), rng.pick(&["P", "R"])),
        _ => std_call(rng),
    }
}

/// Every message type, in the forms Nexus composes, at their edges.
fn typed_corpus() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let (to, de) = ("K1ABC", "W9XYZ");
    // Standard QSO steps, every report.
    for n in -50..=49 {
        v.push(format!("{to} {de} {}", report(n)));
        v.push(format!("{to} {de} R{}", report(n)));
    }
    for tail in [
        "RRR", "RR73", "73", "FN42", "R FN42", "AA00", "RR99", "JJ00",
    ] {
        v.push(format!("{to} {de} {tail}"));
    }
    v.push(format!("{to} {de}"));
    // Every 4-character grid.
    for a in b'A'..=b'R' {
        for b in b'A'..=b'R' {
            for c in b'0'..=b'9' {
                for d in b'0'..=b'9' {
                    let g = String::from_utf8(vec![a, b, c, d]).unwrap();
                    if (c ^ d) & 3 == 0 {
                        v.push(format!("CQ {de} {g}"));
                    }
                    if c == d {
                        v.push(format!("{to} {de} R {g}"));
                    }
                }
            }
        }
    }
    // Every call shape against every other, a CQ for each, and the directed forms.
    let all: Vec<&str> = CALLS
        .iter()
        .chain(PORTABLE)
        .chain(COMPOUND)
        .copied()
        .collect();
    for a in &all {
        v.push(format!("CQ {a} FN42"));
        v.push(format!("CQ {a}"));
        v.push(format!("QRZ {a} FN42"));
        v.push(format!("DE {a} FN42"));
        for t in CQ_TOKENS {
            v.push(format!("CQ {t} {a} FN42"));
            v.push(format!("CQ {t} {a}"));
        }
        for b in &all {
            v.push(format!("{a} {b} -07"));
            v.push(format!("{a} {b} R-12"));
            v.push(format!("{a} {b} RR73"));
            v.push(format!("{a} {b} EN37"));
            v.push(format!("<{a}> {b} RRR"));
            v.push(format!("{a} <{b}> 73"));
            v.push(format!("<{a}> {b} R+05"));
        }
    }
    // Hashed calls in every position the formats allow.
    for m in [
        "<PJ4/K1ABC> W9XYZ -10",
        "W9XYZ <PJ4/K1ABC> R-10",
        "<K1ABC> W9XYZ RR73",
        "<WA9XYZ> PJ4/KA1ABC RR73",
        "PJ4/KA1ABC <WA9XYZ> 73",
        "<W9XYZ> YW18FIFA RRR",
        "CQ YW18FIFA",
        "CQ PJ4/K1ABC",
        "<W9XYZ> ZK3/K1ABC RR73",
        "<ZK3/K1ABC> W9XYZ R EN37",
        "W9XYZ <ZK3/K1ABC> FN42",
        "<KH1/KH7Z> K1ABC",
        "K1ABC <KH1/KH7Z>",
    ] {
        v.push(m.into());
    }
    // ARRL Field Day: every class, every section, with and without the roger.
    for ntx in 1..=32 {
        for class in ["A", "B", "C", "D", "E", "F"] {
            v.push(format!("{to} {de} {ntx}{class} WI"));
            v.push(format!("{to} {de} R {ntx}{class} EMA"));
        }
    }
    for sec in [
        "AB", "AK", "AL", "AR", "AZ", "BC", "CO", "CT", "DE", "EB", "EMA", "ENY", "EPA", "EWA",
        "GA", "GH", "IA", "ID", "IL", "IN", "KS", "KY", "LA", "LAX", "NS", "MB", "MDC", "ME", "MI",
        "MN", "MO", "MS", "MT", "NC", "ND", "NE", "NFL", "NH", "NL", "NLI", "NM", "NNJ", "NNY",
        "TER", "NTX", "NV", "OH", "OK", "ONE", "ONN", "ONS", "OR", "ORG", "PAC", "PR", "QC", "RI",
        "SB", "SC", "SCV", "SD", "SDG", "SF", "SFL", "SJV", "SK", "SNJ", "STX", "SV", "TN", "UT",
        "VA", "VI", "VT", "WCF", "WI", "WMA", "WNY", "WPA", "WTX", "WV", "WWA", "WY", "DX", "PE",
        "NB",
    ] {
        v.push(format!("{to} {de} 3A {sec}"));
        v.push(format!("{to} {de} R 12F {sec}"));
    }
    for m in [
        "W9XYZ <PJ4/K1ABC> 2B EMA",
        "<PJ4/K1ABC> W9XYZ R 1D WI",
        "W9XYZ K1ABC/QRP 2B EMA",
        "W9XYZ <K1ABC/QRP> 2B EMA",
        "W9XYZ PJ4/K1ABC 2B EMA",
        "W9XYZ K1ABC/P 2B EMA",
        "CQ FD K1ABC FN42",
        "CQ FD K1ABC/P FN42",
    ] {
        v.push(m.into());
    }
    // ARRL RTTY Roundup: every multiplier, serials, every 5x9 report, TU and R.
    let named = [
        "AL", "AK", "AZ", "AR", "CA", "CO", "CT", "DE", "FL", "GA", "HI", "ID", "IL", "IN", "IA",
        "KS", "KY", "LA", "ME", "MD", "MA", "MI", "MN", "MS", "MO", "MT", "NE", "NV", "NH", "NJ",
        "NM", "NY", "NC", "ND", "OH", "OK", "OR", "PA", "RI", "SC", "SD", "TN", "TX", "UT", "VT",
        "VA", "WA", "WV", "WI", "WY", "NB", "NS", "QC", "ON", "MB", "SK", "AB", "BC", "NWT", "NF",
        "LB", "NU", "YT", "PEI", "DC", "DR", "FR", "GD", "GR", "OV", "ZH", "ZL",
    ];
    let numbered = (1..=99).map(|n| format!("X{n:02}"));
    for mult in named.iter().map(|m| m.to_string()).chain(numbered) {
        v.push(format!("{to} {de} 579 {mult}"));
        v.push(format!("TU; {to} {de} R 599 {mult}"));
    }
    for serial in [
        1, 13, 99, 100, 999, 1000, 2047, 4096, 7999, 8000, 8191, 8192, 9999,
    ] {
        v.push(format!("{to} G4ABC 559 {serial:04}"));
        v.push(format!("TU; {to} G4ABC R 529 {serial:04}"));
        v.push(format!("{to} G4ABC R 589 {serial}"));
    }
    for rst in ["529", "539", "549", "559", "569", "579", "589", "599"] {
        v.push(format!("{to} {de} {rst} MA"));
        v.push(format!("{to} {de} R {rst} 0013"));
    }
    // EU VHF contest (two hashed calls, report and serial, 6-character locator).
    for (nx, g) in [
        (520_001, "IO91NP"),
        (590_003, "JO22AA"),
        (594_095, "RR99XX"),
        (559_999, "AA00AA"),
        (572_047, "KN15MM"),
        (572_048, "KN15MM"),
    ] {
        v.push(format!("<PA3XYZ> <G4ABC/P> R {nx} {g}"));
        v.push(format!("<PA3XYZ> <G4ABC/P> {nx} {g}"));
    }
    v.push("<PA3XYZ> <G4ABC/P> R 520000 IO91NP".into());
    v.push("<PA3XYZ> <G4ABC/P> R 594096 IO91NP".into());
    // DXpedition mode, every report it can carry and past them.
    for n in -40..=40 {
        v.push(format!("K1ABC RR73; W9XYZ <KH1/KH7Z> {}", report(n)));
    }
    v.push("K1ABC RR73; W9XYZ <KH1/KH7Z> -99".into());
    // Telemetry: one word of hex.
    for t in [
        "123456789ABCDEF012",
        "7FFFFF000000000000",
        "800000000000000000",
        "0",
        "1",
        "73",
        "ABCDEF",
        "FFFFFFFFFFFFFFFFFF",
        "0123456789abcdef01",
        "-1",
        "+1",
        "12345G",
    ] {
        v.push(t.into());
    }
    // Free text, the WSPR forms, and edges of the parse.
    for t in [
        "TNX BOB 73 GL",
        "QRV 2M FT8",
        "GL ES 73",
        "TNX QSO 73",
        "HELLO",
        "5/9 NJ2X",
        "PSE QSY 7.074",
        "TEST?",
        "CQ",
        "QRZ",
        "DE",
        "RR73",
        "73",
        "",
        " ",
        "THIS IS A VERY LONG FREE TEXT MESSAGE",
        "cq k1abc fn42",
        "CQ   K1ABC    FN42",
        "  CQ K1ABC FN42",
        "K1ABC FN42 37",
        "K1ABC FN42 0",
        "PJ4/K1ABC 37",
        "K1ABC/4 23",
        "$DX K1ABC -10",
        "K1ABC W9XYZ FN42AB",
        "<...> W9XYZ -10",
        "@@@",
        "<>",
    ] {
        v.push(t.into());
    }
    v
}

/// The messages Nexus's sequencers compose, which the Rust encoder must always make itself.
fn nexus_corpus() -> Vec<String> {
    let mut v = Vec::new();
    for to in CALLS.iter().chain(PORTABLE) {
        for de in ["KD9TAW", "K1ABC/P", "W9XYZ/R"] {
            for n in [-24, -10, -1, 0, 7, 15, 30, 49] {
                v.push(format!("{to} {de} {}", report(n)));
                v.push(format!("{to} {de} R{}", report(n)));
            }
            for tail in ["FN42", "EN37", "RRR", "RR73", "73", "R FN42"] {
                v.push(format!("{to} {de} {tail}"));
            }
            v.push(format!("CQ {de} EN52"));
            v.push(format!("CQ DX {de} EN52"));
            v.push(format!("CQ POTA {de} EN52"));
            v.push(format!("{to} {de} 2A WI"));
            v.push(format!("{to} {de} R 2A WI"));
            v.push(format!("{to} {de} 579 MA"));
            v.push(format!("TU; {to} {de} R 599 0013"));
        }
    }
    for de in COMPOUND {
        v.push(format!("CQ {de}"));
        v.push(format!("<W9XYZ> {de} RR73"));
        v.push(format!("<W9XYZ> {de} RRR"));
        v.push(format!("<W9XYZ> {de} 73"));
        v.push(format!("{de} <W9XYZ>"));
        v.push(format!("W9XYZ <{de}> -10"));
        v.push(format!("<{de}> W9XYZ R-10"));
    }
    v
}

/// Messages `genft8` and `genft4` call bad: `ft4_encode` refuses them and `ft8_encode` returns 79
/// zero tones (it refuses nothing). A CQ with a roger or a report, a CQ to a hashed call, a report
/// or a Field Day class too wide for its field, free text of nothing the alphabet holds, a call no
/// prefix could make.
const BAD: &[&str] = &[
    "CQ K1ABC R FN42",
    "CQ K1ABC RRR",
    "CQ K1ABC 73",
    "CQ K1ABC -10",
    "CQ DX K1ABC R-10",
    "CQ <K1ABC> FN42",
    "CQ <PJ4/K1ABC> FN42",
    "K1ABC W9XYZ +500",
    "K1ABC W9XYZ R-32500",
    "@@@",
    "<>",
    "QU1RK W9XYZ FN42",
    "K1ABC W9XYZ 3I WI",
];

/// ⭐ The same refusals: where the vendored encoder calls a message bad, the Rust one does too.
#[test]
fn the_vendored_refusals_are_the_rust_encoders() {
    for msg in BAD {
        assert!(
            ft4::encode(msg).is_empty(),
            "premise: ft4_encode refuses {msg:?}"
        );
        assert_eq!(
            ft8::encode(msg),
            vec![0; 79],
            "premise: ft8_encode sends zeros for {msg:?}"
        );
        assert_eq!(ft4_tones(msg), Some(Vec::new()), "FT4 {msg:?}");
        assert_eq!(ft8_tones(msg), Some(vec![0; 79]), "FT8 {msg:?}");
    }
}

/// ⭐ Every message type, at its edges: the same tones from both encoders, and every message
/// Nexus composes made by the Rust one.
#[test]
fn every_message_type_encodes_to_the_vendored_tones() {
    let typed = typed_corpus();
    let nexus = nexus_corpus();
    let mut declined = 0;
    for msg in &typed {
        let n = same_tones(msg, false);
        if n > 0 {
            eprintln!("wsjt77 golden: declined {msg:?} ({n} of FT8/FT4)");
        }
        declined += n;
    }
    for msg in &nexus {
        same_tones(msg, true);
    }
    eprintln!(
        "wsjt77 golden: {} typed + {} Nexus messages x FT8/FT4, {declined} declined to the vendored encoder",
        typed.len(),
        nexus.len()
    );
    // The corpus is not a token one, and it is mostly made by the Rust encoder.
    assert!(
        typed.len() > 20_000,
        "the typed corpus shrank to {}",
        typed.len()
    );
    assert!(declined * 100 < typed.len(), "{declined} declined");
}

/// Well-formed messages of every type from a fixed-seed generator: always the Rust encoder's own,
/// always the vendored tones.
#[test]
fn generated_messages_encode_to_the_vendored_tones() {
    let mut rng = Rng(0x5EED_0F77);
    for _ in 0..40_000 {
        let kind = rng.below(14);
        let msg = match kind {
            0 => format!("CQ {} {}", any_call(&mut rng), grid4(&mut rng)),
            1 => format!(
                "CQ {} {} {}",
                rng.pick(CQ_TOKENS),
                std_call(&mut rng),
                grid4(&mut rng)
            ),
            2 => format!(
                "{} {} {}",
                any_call(&mut rng),
                std_call(&mut rng),
                report(rng.below(100) as i32 - 50)
            ),
            3 => format!(
                "{} {} R{}",
                std_call(&mut rng),
                any_call(&mut rng),
                report(rng.below(100) as i32 - 50)
            ),
            4 => format!(
                "{} {} {}",
                std_call(&mut rng),
                std_call(&mut rng),
                rng.pick(&["RRR", "RR73", "73"])
            ),
            5 => format!(
                "{} {} {}{}",
                std_call(&mut rng),
                any_call(&mut rng),
                if rng.below(2) == 0 { "R " } else { "" },
                grid4(&mut rng)
            ),
            6 => format!(
                "<{}> {} {}",
                any_call(&mut rng),
                std_call(&mut rng),
                rng.pick(&["RRR", "RR73", "73", "-10", "R+03"])
            ),
            7 => format!(
                "{} {} {}{}{} {}",
                std_call(&mut rng),
                std_call(&mut rng),
                if rng.below(2) == 0 { "R " } else { "" },
                1 + rng.below(32),
                rng.chars(b"ABCDEF", 1),
                rng.pick(&["WI", "EMA", "STX", "NNJ", "DX", "ONE", "SCV"])
            ),
            8 => format!(
                "{}{} {} {}5{}9 {}",
                if rng.below(2) == 0 { "TU; " } else { "" },
                std_call(&mut rng),
                std_call(&mut rng),
                if rng.below(2) == 0 { "R " } else { "" },
                2 + rng.below(8),
                if rng.below(2) == 0 {
                    rng.pick(&["MA", "TX", "ON", "NWT", "X42", "DC"])
                        .to_string()
                } else {
                    format!("{:04}", 1 + rng.below(7999))
                }
            ),
            9 => format!(
                "<{}> <{}> {}{} {}",
                std_call(&mut rng),
                any_call(&mut rng),
                if rng.below(2) == 0 { "R " } else { "" },
                520_001 + rng.below(74_095),
                grid6(&mut rng)
            ),
            10 => format!(
                "{} RR73; {} <{}> {}",
                std_call(&mut rng),
                std_call(&mut rng),
                any_call(&mut rng),
                report(rng.below(60) as i32 - 30)
            ),
            11 => {
                let n = 1 + rng.below(18);
                let first = rng.chars(b"01234567", 1);
                format!("{first}{}", rng.chars(b"0123456789ABCDEF", n - 1))
            }
            12 => {
                let n = 1 + rng.below(13);
                rng.chars(b" 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ+-./?", n)
                    .trim()
                    .to_string()
            }
            _ => format!(
                "{} {} {}",
                std_call(&mut rng),
                grid4(&mut rng),
                rng.below(61)
            ),
        };
        // Random free text (12) can be shaped like an exchange whose vendored packing reads an
        // unset local; every other kind is a well-formed message the Rust encoder must make.
        same_tones(&msg, kind != 12);
    }
}

/// Arbitrary printable text, shaped and unshaped: wherever the Rust encoder answers, the vendored
/// tones and the vendored refusal.
#[test]
fn arbitrary_text_encodes_to_the_vendored_tones_or_is_declined() {
    let mut rng = Rng(0xA5B1_7E77);
    let printable: Vec<u8> = (0x20..0x7F).collect();
    let tokens: &[&str] = &[
        "CQ",
        "QRZ",
        "DE",
        "TU;",
        "RR73;",
        "R",
        "RRR",
        "RR73",
        "73",
        "-10",
        "+05",
        "R-07",
        "R+12",
        "599",
        "579",
        "0013",
        "MA",
        "WI",
        "3A",
        "12F",
        "EMA",
        "FN42",
        "IO91NP",
        "520001",
        "<K1ABC>",
        "<PJ4/K1ABC>",
        "PJ4/K1ABC",
        "K1ABC",
        "W9XYZ",
        "K1ABC/P",
        "G4ABC/R",
        "$DX",
        "123ABC",
        "-",
        "+",
        "/",
        ",",
        "5/",
        "1/2",
        "<",
        ">",
        "<...>",
        "CQ_DX",
        "X99",
    ];
    let (mut total, mut declined) = (0usize, 0usize);
    for _ in 0..40_000 {
        let msg = if rng.below(2) == 0 {
            let n = rng.below(41);
            rng.chars(&printable, n)
        } else {
            let n = 1 + rng.below(6);
            (0..n)
                .map(|_| rng.pick(tokens).to_string())
                .collect::<Vec<_>>()
                .join(" ")
        };
        total += 2;
        declined += same_tones(&msg, false);
    }
    eprintln!("wsjt77 arbitrary text: {declined} of {total} declined to the vendored encoder");
    assert!(declined * 10 < total, "{declined} of {total} declined");
}

/// The texts the encoder refuses because the packer would build their frame from a field it never
/// set (`Refusal::NoFrame`), named here so that the verdict below is checked, not trusted: the
/// encoder's own refusal deciding what to expect could never catch it refusing a message it should
/// send. The typed and Nexus corpora hold no such text (the walk checks that too), so the walk adds
/// these, the ones Nexus's free text and Tx rows can make (`wsjt77`'s unit tests).
const NO_FRAME: &[&str] = &[
    "W9XYZ K1ABC TNX 73",
    "W9XYZ/M K1ABC TNX 73",
    "W9XYZ K1ABC 73 GL",
    "W9XYZ K1ABC 5NN 001",
    "W9XYZ K1ABC TNX 599 001",
    "W9XYZ K1ABC 2B XYZ",
    "W9XYZ/M K1ABC TNX",
    "W9XYZ/M K1ABC/P",
    "<...> PJ4/K1ABC RR73",
    "K1ABC/ABCD 37",
];

/// The modes' own encode (the transmit path): the Rust tones where it answers, the vendored
/// encoder's where it declines, so the same tones either way; and nothing where the encoder refuses
/// the message, which is exactly where the vendored encoder answers a bad message (FT8's 79 zero
/// tones, FT4's none) and for the [`NO_FRAME`] texts, the only ones `Refusal::NoFrame` names. The
/// vendored encoder is not asked for those: its frame for them is whatever its memory held.
#[test]
fn the_ft8_and_ft4_modes_encode_the_vendored_tones() {
    let (ft8_mode, ft4_mode) = (make_mode(ModeKind::Ft8), make_mode(ModeKind::Ft4));
    let walked: Vec<String> = typed_corpus()
        .into_iter()
        .step_by(7)
        .chain(nexus_corpus())
        .chain(NO_FRAME.iter().map(|m| m.to_string()))
        .collect();
    for msg in &walked {
        let no_frame = NO_FRAME.contains(&msg.as_str());
        assert_eq!(
            (
                ft8_refusal(msg) == Some(Refusal::NoFrame),
                ft4_refusal(msg) == Some(Refusal::NoFrame)
            ),
            (no_frame, no_frame),
            "FT8, FT4 refuse {msg:?} as a text with no frame"
        );
        let (want8, want4) = if no_frame {
            (Vec::new(), Vec::new())
        } else {
            let (vendored8, vendored4) = (ft8::encode(msg), ft4::encode(msg));
            let bad8 = vendored8.iter().all(|&t| t == 0);
            (if bad8 { Vec::new() } else { vendored8 }, vendored4)
        };
        assert_eq!(ft8_mode.encode(msg), want8, "FT8 {msg:?}");
        assert_eq!(ft4_mode.encode(msg), want4, "FT4 {msg:?}");
    }
}

/// The largest sample difference between two waves of one length, and how many samples differ.
fn wave_difference(rust: &[f32], vendored: &[f32]) -> (f32, usize) {
    assert_eq!(rust.len(), vendored.len(), "wave lengths differ");
    rust.iter()
        .zip(vendored)
        .fold((0f32, 0usize), |(max, n), (a, b)| {
            ((a - b).abs().max(max), n + usize::from(a != b))
        })
}

/// ⭐ The waveform, sample for sample against `gen_ft8wave` / `gen_ft4wave`: equal to float
/// rounding everywhere (a few units in the last place, where gfortran's vectorised cosine shapes
/// the amplitude ramps), and on x86-64 identical outside those ramps.
#[test]
fn the_waveforms_match_the_vendored_synthesis() {
    const TOLERANCE: f32 = 1e-6;
    let messages: Vec<String> = nexus_corpus().into_iter().step_by(17).collect();
    let mut worst = (0f32, 0f32);
    for (i, msg) in messages.iter().enumerate() {
        let f0 = [200.0, 1500.0, 1234.5, 2950.0][i % 4];
        let tones = ft8::encode(msg);
        let (rust, vendored) = (
            ft8_wave(&tones, 12_000.0, f0),
            ft8::gen_wave(&tones, 12_000.0, f0),
        );
        let (max, _) = wave_difference(&rust, &vendored);
        assert!(max <= TOLERANCE, "FT8 {msg:?} at {f0} Hz: {max}");
        worst.0 = worst.0.max(max);
        // Outside the two 240-sample ramps nothing is vectorised: the same libm, the same order.
        if cfg!(target_arch = "x86_64") {
            let n = rust.len();
            assert_eq!(
                wave_difference(&rust[240..n - 240], &vendored[240..n - 240]).1,
                0,
                "FT8 {msg:?} at {f0} Hz differs between the ramps"
            );
        }
        let tones = ft4::encode(msg);
        if tones.is_empty() {
            continue;
        }
        let (rust, vendored) = (
            ft4_wave(&tones, 12_000.0, f0),
            ft4::gen_wave(&tones, 12_000.0, f0),
        );
        let (max, _) = wave_difference(&rust, &vendored);
        assert!(max <= TOLERANCE, "FT4 {msg:?} at {f0} Hz: {max}");
        worst.1 = worst.1.max(max);
        if cfg!(target_arch = "x86_64") {
            // FT4's ramps are a whole symbol each, at both ends of its 105 symbols.
            let end = 105 * 576;
            assert_eq!(
                wave_difference(&rust[576..end - 576], &vendored[576..end - 576]).1,
                0,
                "FT4 {msg:?} at {f0} Hz differs between the ramps"
            );
            assert!(rust[end..].iter().all(|&s| s == 0.0), "FT4 tail is silence");
        }
    }
    eprintln!(
        "wsjt77 waves: {} messages, largest sample difference FT8 {:e}, FT4 {:e}",
        messages.len(),
        worst.0,
        worst.1
    );
}

/// The Rust waveform decodes, with the vendored decoder, to the message it was made from.
#[test]
fn the_rust_waveform_decodes_with_the_vendored_decoder() {
    for msg in [
        "CQ KD9TAW EN52",
        "K1ABC W9XYZ R-12",
        "W9XYZ K1ABC RR73",
        "K1ABC W9XYZ 2A WI",
    ] {
        let mode = make_mode(ModeKind::Ft8);
        let wave = mode.gen_wave(&ft8_tones(msg).unwrap(), 12_000.0, 1_500.0);
        let mut frame = vec![0i16; ft8::NMAX];
        for (s, &x) in frame.iter_mut().zip(&wave) {
            *s = (x * 1_000.0) as i16;
        }
        let decoded = ft8::decode_frame(&frame, 200, 3_000, 3, "", "", 0, 0, false, false, false);
        assert!(
            decoded.iter().any(|d| d.message.trim() == msg),
            "FT8 {msg:?} decoded as {:?}",
            decoded.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let mode = make_mode(ModeKind::Ft4);
        let wave = mode.gen_wave(&ft4_tones(msg).unwrap(), 12_000.0, 1_500.0);
        let mut frame = vec![0i16; ft4::NMAX];
        for (s, &x) in frame.iter_mut().zip(&wave) {
            *s = (x * 1_000.0) as i16;
        }
        let decoded = ft4::decode_frame(&frame, 200, 3_000, 3, "", "", 0, 0, false);
        assert!(
            decoded.iter().any(|d| d.message.trim() == msg),
            "FT4 {msg:?} decoded as {:?}",
            decoded.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}
