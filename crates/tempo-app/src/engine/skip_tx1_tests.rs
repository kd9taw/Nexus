//! Skip Tx1 is the station's own setting, and the snapshot says it, so a computer driving the
//! station from afar draws the box from the station's value (as the shack's screen does) and never
//! from a copy of its own. What the setting DOES is unchanged: the golden below pins the first over
//! of a double-click and of a roster click, for a plain call and a nonstandard one, both ways.

use super::tests::dec_snr;
use super::*;
use crate::dto::Tier;

/// The snapshot carries the station's Skip Tx1, off at launch, and follows each change.
#[test]
fn the_snapshot_says_the_stations_skip_tx1() {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    assert!(!e.snapshot().radio.skip_tx1, "off at launch, as before");
    e.set_skip_tx1(true);
    assert!(e.snapshot().radio.skip_tx1, "the station's value: on");
    e.set_skip_tx1(false);
    assert!(
        !e.snapshot().radio.skip_tx1,
        "the station's value: off again"
    );
}

/// On the wire the field is `radio.skipTx1`, the name the screen reads; a snapshot from a station
/// that does not send it reads off, the launch value.
#[test]
fn the_wire_names_it_skip_tx1_and_an_older_station_reads_off() {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_skip_tx1(true);
    let mut wire = serde_json::to_value(e.snapshot()).unwrap();
    assert_eq!(wire["radio"]["skipTx1"], serde_json::json!(true));
    wire["radio"].as_object_mut().unwrap().remove("skipTx1");
    let older: AppSnapshot = serde_json::from_value(wire).unwrap();
    assert!(!older.radio.skip_tx1);
}

/// The first over the sequencer queues for one heard CQ, by a double-click (the decode's text and
/// SNR) or by a roster click (the call alone).
fn first_over(mycall: &str, cq: &str, skip: bool, double_click: bool) -> Option<String> {
    let mut e = Engine::new(mycall, "FN31", 0);
    e.set_tier(Tier::Ft8);
    e.set_skip_tx1(skip);
    e.ingest_decodes_for_test(&[dec_snr(cq, -7)], 2);
    let Msg::Cq { de, .. } = Msg::parse(cq) else {
        panic!("premise: a CQ, {cq:?}")
    };
    if double_click {
        e.work_station(&de, None, Some(cq), Some(-7), None)
            .expect("the double-click is taken");
    } else {
        e.call_station(&de);
    }
    // Reading the snapshot, as a driving computer now does, is not a write.
    let _ = e.snapshot();
    e.snapshot().qso.and_then(|q| q.tx_now)
}

/// ★ GOLDEN: the sequencing is the same text, by value, as before the setting reached the snapshot.
/// Skip Tx1 on opens a double-clicked plain call's answer at the report; off, at the grid. A roster
/// click carries no heard SNR for a CQ, and a nonstandard own call cannot send a bare report, so
/// both open at the grid either way.
#[test]
fn golden_the_first_over_skip_tx1_queues_is_unchanged() {
    // (my call, heard CQ, skip, first over by a double-click, first over by a roster click)
    let golden: [(&str, &str, bool, &str, &str); 4] = [
        (
            "K2DEF",
            "CQ W9XYZ EM48",
            false,
            "W9XYZ K2DEF FN31",
            "W9XYZ K2DEF FN31",
        ),
        (
            "K2DEF",
            "CQ W9XYZ EM48",
            true,
            "W9XYZ K2DEF -07",
            "W9XYZ K2DEF FN31",
        ),
        (
            "PJ4/K2DEF",
            "CQ W9XYZ EM48",
            false,
            "<W9XYZ> PJ4/K2DEF",
            "<W9XYZ> PJ4/K2DEF",
        ),
        (
            "PJ4/K2DEF",
            "CQ W9XYZ EM48",
            true,
            "<W9XYZ> PJ4/K2DEF",
            "<W9XYZ> PJ4/K2DEF",
        ),
    ];
    for (mycall, cq, skip, by_double_click, by_roster) in golden {
        for (double_click, expected) in [(true, by_double_click), (false, by_roster)] {
            assert_eq!(
                first_over(mycall, cq, skip, double_click).as_deref(),
                Some(expected),
                "{mycall} answering {cq:?}, skip {skip}, double-click {double_click}"
            );
        }
    }
}
