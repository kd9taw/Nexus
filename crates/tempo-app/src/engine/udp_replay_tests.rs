//! What a logger's UDP Replay re-sends ([`Engine::band_activity`]): every decode Band Activity
//! holds, as its UDP Decode went out, to the pane's own depth. The wire itself, the clears and a
//! band change are pinned through the radio loop (`tempo-audio` `service/tests/udp_decode_time.rs`).

use super::*;

fn decode(message: &str) -> modes::Decode {
    modes::Decode {
        message: message.into(),
        sync: 10.0,
        snr: -10,
        dt: 0.1,
        freq: 1500.0,
        nap: 0,
        qual: 1.0,
        rv: None,
        raw: None,
        mode: Some(modes::ModeKind::Ft8),
    }
}

fn replayed(e: &Engine) -> Vec<String> {
    e.band_activity().map(|(_, d)| d.message.clone()).collect()
}

/// Band Activity keeps its newest 3000 decodes (`MAX_HISTORY`, `ui/src/decodeHistory.ts`), and so
/// does what a Replay re-sends: the oldest go first.
#[test]
fn a_replay_holds_the_newest_decodes_band_activity_keeps() {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_tier(Tier::Ft8);
    for slot in 1..=301u64 {
        let period = (0..10)
            .map(|i| decode(&format!("CQ K{slot}X{i} EN52")))
            .collect();
        e.fold_decodes(None, period, slot, false);
    }
    let kept = replayed(&e);
    assert_eq!(
        (
            kept.len(),
            kept.first().map(String::as_str),
            kept.last().map(String::as_str)
        ),
        (3000, Some("CQ K2X0 EN52"), Some("CQ K301X9 EN52")),
        "3010 decodes heard: (kept, oldest, newest)"
    );
}

/// A Hound's Band Activity row is the Fox's message as it went out, which is what the live UDP
/// Decode carried ([`Engine::wire_decodes`]), not the two halves the sequencer reads.
#[test]
fn a_replay_holds_the_fox_message_as_it_went_out() {
    let mut e = Engine::new("W9XYZ", "EN37", 0);
    e.set_tier(Tier::Ft8);
    e.settings.special_op = crate::settings::SpecialOp::Hound;
    e.fold_decodes(None, vec![decode("CQ DX PJ4DX")], 1, false);
    let _ = e.call_station_ctx("PJ4DX", None, Some("CQ DX PJ4DX"), Some(-10), Some(1500.0));
    e.fold_decodes(None, vec![decode("K1ABC RR73; W9XYZ PJ4DX -08")], 3, false);
    assert!(
        e.decode_history
            .iter()
            .any(|(_, d)| d.message == "W9XYZ PJ4DX -08"),
        "premise: the sequencer reads the Fox message split"
    );
    let wire: Vec<String> = e.wire_decodes().iter().map(|d| d.message.clone()).collect();
    assert_eq!(
        (replayed(&e), wire),
        (
            vec![
                "CQ DX PJ4DX".to_string(),
                "K1ABC RR73; W9XYZ PJ4DX -08".to_string()
            ],
            vec!["K1ABC RR73; W9XYZ PJ4DX -08".to_string()]
        ),
        "(replayed, the live Decode's text)"
    );
}
