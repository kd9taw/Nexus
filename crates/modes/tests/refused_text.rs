//! ⛔ FT8 AND FT4 ENCODE NOTHING FOR A TEXT THE ENCODER REFUSES. `Mode::encode` is what every FT8 and
//! FT4 over is built from (`TxWaveform::build`), and an empty answer is "do not key" to every
//! caller. WSJT-X 3.0.2's `genft8` answers a bad message with 79 zero tones (`lib/ft8/genft8.f90`
//! 20-26), which the vendored entry point passed on as an over: one steady carrier for 12.64 s.
//! A text the packer builds from fields it never set has no frame at all: `W9XYZ/M K1ABC TNX` went
//! out as a frame that decodes as `<...>`.
//!
//! Controls: every text the packer carries still encodes, whatever it decodes as (`W9XYZ K1ABC TNX`
//! is the 13-character free text `W9XYZ K1ABC T`, as WSJT-X sends it).

use modes::{make_mode, ModeKind};

#[test]
fn ft8_and_ft4_encode_nothing_for_a_text_the_encoder_refuses() {
    for kind in [ModeKind::Ft8, ModeKind::Ft4] {
        let mode = make_mode(kind);
        for text in [
            "W9XYZ K1ABC TNX 73",
            "W9XYZ/M K1ABC TNX 73",
            "W9XYZ/M K1ABC TNX",
            "W9XYZ K1ABC 73 GL",
            "W9XYZ K1ABC 5NN 001",
            "W9XYZ/M K1ABC/P",
            // A CQ naming a hashed call is a bad message; this one, `<...>`, only the vendored
            // encoder packs, and only its 79 zero tones say so.
            "CQ <...> FN42",
        ] {
            assert_eq!(mode.encode(text), Vec::<i32>::new(), "{kind:?} {text:?}");
        }
        for text in [
            "TNX 73",
            "W9XYZ K1ABC TNX",
            "W9XYZ/M <K1ABC> 73",
            "W9XYZ K1ABC 599 001",
            "CQ K1ABC FN42",
            "<...> W9XYZ -10",
        ] {
            let tones = mode.encode(text);
            assert!(
                !tones.is_empty() && tones.iter().any(|&t| t != 0),
                "CONTROL {kind:?} {text:?}: {tones:?}"
            );
        }
    }
}

// ---- The encoders that stop the process ---------------------------------------------------------

/// The child half of [`encode_alone`]: encodes the mode and text it is handed, and says how many
/// tones came back. Does nothing in an ordinary run.
#[test]
fn encode_in_a_child_process() {
    let Ok(job) = std::env::var(CHILD) else {
        return;
    };
    let (mode, text) = job.split_once('|').expect("mode|text");
    println!("TONES {}", make_mode(kind(mode)).encode(text).len());
}

const CHILD: &str = "NEXUS_REFUSED_TEXT_CHILD";

fn kind(mode: &str) -> ModeKind {
    match mode {
        "Q65" => ModeKind::Q65 {
            period_s: 30,
            submode: 0,
        },
        "MSK144" => ModeKind::Msk144 { period_s: 15 },
        "FST4" => ModeKind::Fst4 {
            period_s: 60,
            wspr: false,
        },
        "FST4W" => ModeKind::Fst4 {
            period_s: 120,
            wspr: true,
        },
        "WSPR" => ModeKind::Wspr,
        _ => panic!("no such mode {mode:?}"),
    }
}

/// `mode`'s tones for `text`, encoded in a process of its own, or what that process said as it
/// died: a gfortran `stop` or a segfault takes the whole process, which no assertion survives.
fn encode_alone(mode: &str, text: &str) -> Result<usize, String> {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "encode_in_a_child_process", "--nocapture"])
        .env(CHILD, format!("{mode}|{text}"))
        .output()
        .unwrap();
    let said =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    let tones = said
        .lines()
        .find_map(|l| l.strip_prefix("TONES "))
        .and_then(|n| n.parse().ok());
    match tones {
        Some(n) if out.status.success() => Ok(n),
        _ => Err(format!("{}: {said}", out.status)),
    }
}

/// ⛔ Q65, MSK144, FST4 AND FST4W ENCODE NOTHING FOR A TEXT `pack77` WRITES NO FRAME FOR, AND THE
/// PROCESS LIVES. Their vendored encoders (`genq65`, `genmsk_128_90`, `genfst4`) read the frame
/// `pack77` wrote with no `err=`; where it wrote none, gfortran stops the whole process (`Fortran
/// runtime error: Bad value during integer read`), a native abort no `catch_unwind` reaches. Free
/// text `TNX 73` beside a call makes the first text below; the others are the same class: a 5x9-less
/// RTTY serial, the leading blank `genfst4` and `genmsk_128_90` strip, a lowercase text, a `*`
/// repeat count, a non-ASCII character, and the calls `pack77_4` reads unset. Each is encoded in a
/// child process, so the old abort fails this test instead of ending the run.
#[test]
fn q65_msk144_and_fst4_refuse_a_text_with_no_frame_without_stopping() {
    for mode in ["Q65", "MSK144", "FST4", "FST4W"] {
        for text in [
            "W1AW KD9TAW TNX 73",
            "W9XYZ K1ABC TNX 73",
            "W9XYZ K1ABC 73 GL",
            " W9XYZ K1ABC TNX 73",
            "w9xyz k1abc tnx 73",
            "W9XYZ K1ABC/P 5*3 EMA",
            "W9XYZ K1ABC TNXÉ 73",
            "W9XYZ/M K1ABC TNX",
            "W9XYZ/M K1ABC/P",
        ] {
            assert_eq!(encode_alone(mode, text), Ok(0), "{mode} {text:?}");
        }
    }
}

/// The texts the packer carries are encoded exactly as the vendored encoder makes them, refusal or
/// none: free text (13 characters of a longer one), a standard message, an RTTY exchange, a hashed
/// call the frame only looks up, a CQ naming a hashed call (WSJT-X sends that frame in these modes;
/// FST4's encoder refuses it itself), MSK144's bracketed short message and FST4W's beacons.
#[test]
fn q65_msk144_and_fst4_encode_the_texts_the_packer_carries_as_before() {
    let vendored = |mode: &str, text: &str| -> Vec<i32> {
        match mode {
            "Q65" => q65::encode(text),
            "MSK144" => msk144::encode(text),
            "FST4" => fst4::encode(text, false),
            _ => fst4::encode(text, true),
        }
        .unwrap_or_default()
    };
    let carried = [
        "TNX 73",
        "W9XYZ K1ABC TNX",
        "HELLO WORLD THIS IS LONG TEXT",
        "CQ K1ABC FN42",
        "K1ABC W9XYZ -10",
        "W9XYZ K1ABC 599 001",
        "<...> W9XYZ -10",
        "W9XYZ/M <K1ABC> 73",
        "CQ <K1ABC> FN42",
    ];
    for mode in ["Q65", "MSK144", "FST4"] {
        let m = make_mode(kind(mode));
        for text in carried {
            let tones = m.encode(text);
            assert_eq!(tones, vendored(mode, text), "{mode} {text:?}");
            if !(mode == "FST4" && text == "CQ <K1ABC> FN42") {
                assert!(!tones.is_empty(), "{mode} {text:?} encodes");
            }
        }
    }
    assert_eq!(
        make_mode(kind("MSK144")).encode("<K1ABC W9XYZ> 73").len(),
        40
    );
    for text in ["K1ABC FN42 37", "<PJ4/K1ABC> FN42AB", "PJ4/K1ABC 37"] {
        let tones = make_mode(kind("FST4W")).encode(text);
        assert_eq!(tones, vendored("FST4W", text), "FST4W {text:?}");
        assert!(!tones.is_empty(), "FST4W {text:?} encodes");
    }
}

/// ⛔ WSPR ENCODES NOTHING FOR A TEXT UPSTREAM'S ENCODER CANNOT READ, AND THE PROCESS LIVES.
/// `get_wspr_channel_symbols` `strtok`s the fields it expects with no check that they are there:
/// `hello` hands `NULL` to `strlen`, `TNX 73` to `atoi`, and the process segfaults. Each is
/// encoded in a child process. Controls: the three beacon forms, as before.
#[test]
fn wspr_refuses_a_text_its_encoder_cannot_read_without_stopping() {
    for text in [
        "hello",
        "TNX 73",
        "CQ_ABC K1ABC",
        "~~~~",
        "K1ABC FN",
        "PJ4/K1ABC",
        "K1ABC/ 37",
        "<K1ABC> FN42AB",
        "<> FN42 37",
        "<K1ABC> FN42ABCD 37",
    ] {
        assert_eq!(encode_alone("WSPR", text), Ok(0), "WSPR {text:?}");
    }
    for text in [
        "K1ABC FN42 37",
        "PJ4/K1ABC 37",
        "K1ABC/P 37",
        "<PJ4/K1ABC> FK52UD 37",
    ] {
        assert_eq!(encode_alone("WSPR", text), Ok(162), "CONTROL WSPR {text:?}");
    }
}
