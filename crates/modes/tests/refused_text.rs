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
