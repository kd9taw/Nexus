//! The clean-up tools' rules, on records: which contacts are one logged twice, that a merge loses
//! no field any copy held, and what a bulk edit and a fix change — and leave alone.

use super::*;
use crate::logbook::{
    adif_record_own_log, parse_adif, ContestFields, QslRcvd, QslVia, UploadOutcome, UploadStatus,
};

const T0: u64 = 1_788_000_000;

fn contact(call: &str, when: u64, seq: u64) -> QsoRecord {
    let mut r = parse_adif("<CALL:4>W1AW<BAND:3>20m<MODE:3>FT8<EOR>").remove(0);
    r.call = call.to_string();
    r.when_unix = when;
    r.freq_mhz = 0.0;
    r.id = Some(format!("~{seq:016x}").parse().expect("an id"));
    r
}

fn key(r: &QsoRecord) -> DupeKey {
    DupeKey {
        id: r.id.unwrap(),
        call: r.call.clone(),
        band: r.band.clone(),
        mode: r.mode.clone(),
        when_unix: r.when_unix,
    }
}

fn stamp(when_unix: i64) -> Option<UploadStatus> {
    Some(UploadStatus {
        outcome: UploadOutcome::Accepted,
        when_unix,
        detail: None,
    })
}

/// The log's own window groups what its guard would have refused — 300 s in, a lower-case
/// spelling of one call, case aside on band and mode — and nothing across a band, a mode or
/// another call, a portable one included. A group is measured from its first contact, so a third
/// copy 600 s after the first starts another; the possible tier's hour takes in a pair the rule
/// leaves apart.
#[test]
fn the_duplicate_groups_are_the_logs_own_rule_and_the_possible_tier_is_wider() {
    let mut rows = vec![
        contact("W1AW", T0, 1),
        contact("w1aw", T0 + 300, 2),
        contact("W1AW", T0 + 600, 3),
        contact("W1AW", T0 + 901, 4),
        contact("K1ABC", T0, 5),
        contact("K1ABC", T0 + 10, 6),
        contact("K1ABC", T0 + 20, 7),
        contact("N0XYZ", T0, 8),
        contact("N0XYZ", T0 + 2_000, 9),
        contact("W1AW/P", T0 + 100, 10),
    ];
    rows[5].band = "40m".into();
    rows[6].mode = "FT4".into();
    let ids = |rs: &[usize]| rs.iter().map(|&i| rows[i].id.unwrap()).collect::<Vec<_>>();

    let rule = dupe_groups(
        rows.iter().map(key).collect(),
        crate::logbook::dedup::DEDUP_WINDOW_SECS,
    );
    assert_eq!(rule, vec![ids(&[0, 1])], "the rule's groups");

    let possible = dupe_groups(rows.iter().map(key).collect(), POSSIBLE_WINDOW_SECS);
    assert_eq!(
        possible,
        vec![ids(&[7, 8]), ids(&[0, 1, 2, 3])],
        "newest first"
    );

    // Each pair the rule groups is one the guard calls a duplicate, and the 301 s one is not.
    use crate::logbook::dedup::is_recent_duplicate;
    assert!(is_recent_duplicate(&rows[0], &rows[1]));
    assert!(!is_recent_duplicate(&rows[2], &rows[3]));
}

/// Two copies of one contact, holding between them every kind of field there is, and agreeing
/// wherever both hold one.
fn two_copies() -> (QsoRecord, QsoRecord) {
    let mut a = contact("W1AW", T0, 1);
    a.grid = Some("FN31pr".into());
    a.rst_sent = Some("-10".into());
    a.qsl_rcvd = QslRcvd {
        lotw: true,
        ..QslRcvd::default()
    };
    a.confirmed = true;
    a.award_confirmed = true;
    a.credit_granted = vec!["DXCC".into()];
    a.upload.lotw = stamp(100);
    a.dxcc = Some(291);
    a.country = Some("United States".into());
    a.extra = vec![("CQZ".into(), "5".into())];
    a.qsl_sent = crate::logbook::QslSent {
        sent: true,
        via: Some(QslVia::Bureau),
        date_unix: Some(T0 - T0 % 86_400),
        cleared_unix: None,
    };

    let mut b = contact("W1AW", T0 + 60, 2);
    b.grid = Some("FN31PR".into());
    b.name = Some("Hiram".into());
    b.qth = Some("Newington".into());
    b.state = Some("CT".into());
    b.rst_rcvd = Some("-07".into());
    b.notes = Some("first QSO on the new antenna".into());
    b.comment = Some("tnx".into());
    b.tx_power = Some(50.0);
    b.freq_mhz = 14.074_5;
    b.time_off_unix = Some(T0 + 120);
    b.prop_mode = Some("F2".into());
    b.operator = Some("K2DEF".into());
    b.station_callsign = Some("K2DEF".into());
    b.my_grid = Some("FN20".into());
    b.my_rig = Some("IC-7300".into());
    b.ota.their_program = Some("POTA".into());
    b.ota.their_ref = Some("US-1234".into());
    b.ota.my_program = Some("SOTA".into());
    b.ota.my_ref = Some("W2/GC-001".into());
    b.ota.iota = Some("NA-001".into());
    b.qsl_rcvd = QslRcvd {
        eqsl: true,
        card: true,
        ..QslRcvd::default()
    };
    b.confirmed = true;
    b.award_confirmed = true;
    b.credit_submitted = vec!["WAS".into()];
    b.upload.qrz = stamp(200);
    b.upload.eqsl = stamp(201);
    b.upload.clublog = stamp(202);
    b.extra = vec![
        ("CQZ".into(), "5".into()),
        ("ITUZ".into(), "8".into()),
        ("SUBMODE".into(), "".into()),
    ];
    b.contest = Some(Box::new(ContestFields {
        contest_id: "ARRL-FIELD-DAY".into(),
        ..ContestFields::default()
    }));
    (a, b)
}

/// The ADIF tags of a record as the operator's own log writes it, with their values.
fn tags(r: &QsoRecord) -> Vec<(String, String)> {
    let text = adif_record_own_log(r);
    let mut out = Vec::new();
    let mut rest = text.as_str();
    while let Some(open) = rest.find('<') {
        let Some(close) = rest[open..].find('>') else {
            break;
        };
        let head = &rest[open + 1..open + close];
        rest = &rest[open + close + 1..];
        let mut it = head.split(':');
        let name = it.next().unwrap_or("").to_ascii_uppercase();
        let Some(len) = it.next().and_then(|l| l.parse::<usize>().ok()) else {
            continue;
        };
        out.push((name, rest[..len].to_string()));
        rest = &rest[len..];
    }
    out
}

/// ★ NO FIELD LOST, at the record. Each copy holds what the other lacks; the merge keeps the
/// first and the result is asserted WHOLE against the record built by hand: every field either
/// copy held is in it, confirmations and credit unioned, each upload stamp kept. Then, as a
/// second witness over every field the log writes, each tag either copy carries is in the
/// merged contact's own-log record with its value — case aside, as a grid is compared — the
/// second copy's time, id and upload stamps aside, the kept contact's own.
#[test]
fn a_merge_keeps_every_field_either_copy_held() {
    let (a, b) = two_copies();
    let merged = merge(&[a.clone(), b.clone()], 0, &HashMap::new());
    assert!(merged.conflicts.is_empty(), "{:?}", merged.conflicts);

    let mut want = a.clone();
    want.name = b.name.clone();
    want.qth = b.qth.clone();
    want.state = b.state.clone();
    want.rst_rcvd = b.rst_rcvd.clone();
    want.notes = b.notes.clone();
    want.comment = b.comment.clone();
    want.tx_power = b.tx_power;
    want.freq_mhz = b.freq_mhz;
    want.time_off_unix = b.time_off_unix;
    want.prop_mode = b.prop_mode.clone();
    want.operator = b.operator.clone();
    want.station_callsign = b.station_callsign.clone();
    want.my_grid = b.my_grid.clone();
    want.my_rig = b.my_rig.clone();
    want.ota = b.ota.clone();
    want.qsl_rcvd = QslRcvd {
        lotw: true,
        eqsl: true,
        card: true,
        qrz: false,
    };
    want.credit_submitted = vec!["WAS".into()];
    // The upload stamps are the kept copy's own: never another copy's.
    want.extra = vec![
        ("CQZ".into(), "5".into()),
        ("ITUZ".into(), "8".into()),
        ("SUBMODE".into(), "".into()),
    ];
    want.contest = b.contest.clone();
    assert_eq!(merged.record, want);

    let held = tags(&merged.record);
    for copy in [&a, &b] {
        for (tag, value) in tags(copy) {
            let kept_identity =
                copy.id != a.id && matches!(tag.as_str(), "APP_NEXUS_ID" | "QSO_DATE" | "TIME_ON");
            let others_stamp =
                copy.id != a.id && crate::logbook::history::UPLOAD_TAGS.contains(&tag.as_str());
            if kept_identity || others_stamp {
                continue;
            }
            assert!(
                held.iter()
                    .any(|(t, v)| *t == tag && v.eq_ignore_ascii_case(&value)),
                "{tag}={value} from {:?} is not in the merged contact",
                copy.id
            );
        }
    }
}

/// A field the copies disagree on is a conflict, the kept copy's value standing unless the caller
/// chose another; the one a confirmed copy holds, the QSL-sent mark, the contest exchange and
/// operator-written text are guarded, and a guarded conflict left unchosen is named.
#[test]
fn a_disagreement_is_a_conflict_and_a_confirmed_copys_is_guarded() {
    let (mut a, mut b) = two_copies();
    a.state = Some("MA".into());
    b.qsl_sent = crate::logbook::QslSent {
        sent: true,
        via: Some(QslVia::Direct),
        date_unix: None,
        cleared_unix: None,
    };
    a.contest = Some(Box::new(ContestFields {
        contest_id: "CQ-WW-CW".into(),
        ..ContestFields::default()
    }));
    a.qsl_rcvd = QslRcvd::default();
    a.confirmed = false;
    a.award_confirmed = false;
    b.qsl_rcvd = QslRcvd::default();
    b.confirmed = false;
    b.award_confirmed = false;
    a.comment = Some("one".into());
    b.comment = Some("two".into());

    let merged = merge(&[a.clone(), b.clone()], 0, &HashMap::new());
    let by_tag = |tag: &str| merged.conflicts.iter().find(|c| c.tag == tag).unwrap();
    assert_eq!(
        merged.record.comment.as_deref(),
        Some("one"),
        "the kept copy's"
    );
    assert!(by_tag("COMMENT").guarded, "operator-written text");
    assert!(!by_tag("STATE").guarded, "no copy is confirmed");
    assert!(by_tag("QSL_SENT").guarded && by_tag("CONTEST").guarded);
    let mut unchosen = merged.unchosen(&HashMap::new());
    unchosen.sort();
    assert_eq!(unchosen, vec!["COMMENT", "CONTEST", "QSL_SENT"]);

    let choices: HashMap<String, RecordId> = [
        ("COMMENT".to_string(), b.id.unwrap()),
        ("CONTEST".to_string(), b.id.unwrap()),
        ("QSL_SENT".to_string(), a.id.unwrap()),
    ]
    .into();
    let chosen = merge(&[a.clone(), b.clone()], 0, &choices);
    assert!(chosen.unchosen(&choices).is_empty());
    assert_eq!(chosen.record.comment.as_deref(), Some("two"));
    assert_eq!(chosen.record.contest, b.contest);
    assert_eq!(chosen.record.qsl_sent, a.qsl_sent);

    // Confirmed, the copy's state is a confirmation's detail: guarded.
    b.qsl_rcvd.lotw = true;
    let guarded = merge(&[a, b], 0, &HashMap::new());
    assert!(guarded
        .conflicts
        .iter()
        .any(|c| c.tag == "STATE" && c.guarded));
}

/// The fullest copy is the suggestion; of equally full ones, the first.
#[test]
fn the_suggested_copy_is_the_fullest() {
    let (a, b) = two_copies();
    assert_eq!(suggested_keep(&[a.clone(), b.clone()]), 1);
    let bare = contact("W1AW", T0, 3);
    assert_eq!(suggested_keep(&[bare.clone(), bare]), 0);
}

fn no_country(_: &str, _: Option<u32>) -> Option<String> {
    None
}

/// A bulk edit sets and clears what it names and nothing else — never a confirmation, never an
/// upload stamp — and takes the "Nexus filled this" mark off a grid it changes. A park goes only
/// on a side with no reference or one of its own programme.
#[test]
fn a_bulk_edit_changes_only_its_fields_and_keeps_every_stamp() {
    let (_, mut b) = two_copies();
    crate::logbook::mark_self_filled(&mut b, "MY_GRIDSQUARE");
    let changes = checked(&[
        Change {
            field: Field::MyGridsquare,
            value: Some("em10".into()),
        },
        Change {
            field: Field::Operator,
            value: None,
        },
        Change {
            field: Field::MyPotaRef,
            value: Some("us-0001".into()),
        },
    ])
    .unwrap();
    let after = changed(&b, &changes, &no_country).expect("a change");
    let mut want = b.clone();
    want.my_grid = Some("em10".into());
    want.operator = None;
    crate::logbook::set_extra(&mut want, SELF_FILLED_FIELD, None);
    assert_eq!(
        after, want,
        "the summit side is SOTA's: the park is not put over it"
    );
    assert_eq!(after.upload, b.upload);
    assert_eq!(after.qsl_rcvd, b.qsl_rcvd);

    b.ota.my_program = None;
    b.ota.my_ref = None;
    let parked = changed(&b, &changes, &no_country).unwrap();
    assert_eq!(parked.ota.my_program.as_deref(), Some("POTA"));
    assert_eq!(parked.ota.my_ref.as_deref(), Some("US-0001"));

    assert!(
        changed(&parked, &changes, &no_country).is_none(),
        "already so"
    );
    assert!(checked(&[Change {
        field: Field::MyGridsquare,
        value: Some("ZZ99".into())
    }])
    .is_err());
}

/// A two-fer typed with spaces, lower case or a park twice is stored as the activation stamps
/// one: upper case, comma-separated, each park once.
#[test]
fn a_bulk_two_fer_is_spelled_as_the_activation_stamps_it() {
    let c = checked(&[Change {
        field: Field::MyPotaRef,
        value: Some(" us-0002 , US-0003,us-0002 ".into()),
    }])
    .unwrap();
    assert_eq!(c[0].value.as_deref(), Some("US-0002,US-0003"));
    assert!(checked(&[Change {
        field: Field::MyPotaRef,
        value: Some("US-0002, nonsense".into()),
    }])
    .is_err());
}

/// A two-fer typed with ';' between its parks, as some logs write one, is stored in the
/// activation's own form: comma-separated.
#[test]
fn a_bulk_two_fer_typed_with_semicolons_is_stored_with_commas() {
    for field in [Field::MyPotaRef, Field::PotaRef] {
        let c = checked(&[Change {
            field,
            value: Some("us-0002; US-0003;US-0002".into()),
        }])
        .unwrap();
        assert_eq!(c[0].value.as_deref(), Some("US-0002,US-0003"), "{field:?}");
    }
    assert!(checked(&[Change {
        field: Field::MyPotaRef,
        value: Some("US-0002;nonsense".into()),
    }])
    .is_err());
}

#[test]
fn the_formats_a_check_holds_references_to() {
    for g in ["FN", "FN31", "fn31pr", "FN31PR45"] {
        assert!(valid_grid(g), "{g}");
    }
    for g in ["", "F", "FN3", "SZ31", "FN31PZ", "FN31PR4", "FN31 "] {
        assert!(!valid_grid(g), "{g}");
    }
    assert!(valid_pota("US-1234") && valid_pota("K-0001,VE-12345"));
    assert!(!valid_pota("US1234") && !valid_pota("US-12") && !valid_pota("1234-0001"));
    assert!(valid_sota("W7W/LC-001") && valid_sota("G/LD-001"));
    assert!(!valid_sota("W7W/LC-1") && !valid_sota("W7WLC-001"));
    assert!(valid_iota("NA-001") && !valid_iota("XX-001") && !valid_iota("na-001"));
    assert_eq!(respelled("us1234", valid_pota).as_deref(), Some("US-1234"));
    assert_eq!(
        respelled(" us 1234 ", valid_pota).as_deref(),
        Some("US-1234")
    );
    assert_eq!(
        respelled("w7w/lc-001", valid_sota).as_deref(),
        Some("W7W/LC-001")
    );
    assert_eq!(respelled("park", valid_pota), None);
}

/// Two bare copies of `call`, a minute apart, neither holding anything but the identity.
fn pair(call_a: &str, call_b: &str) -> (QsoRecord, QsoRecord) {
    (contact(call_a, T0, 1), contact(call_b, T0 + 60, 2))
}

/// ★ A DIFFERENT CALL IS A DIFFERENT CONTACT. `W1AW/KH6` and `W1AW/KL7` are two DXCC entities and
/// `W1AW/4` and `W1AW/5` two call areas, though each pair shares a base call: the finder never
/// groups them, and a merge handed them anyway copies no confirmation or credit across and
/// names the call as a guarded conflict. The same call in another case is one.
#[test]
fn a_different_call_is_never_one_contact() {
    let rows = [
        contact("W1AW/KH6", T0, 1),
        contact("W1AW/KL7", T0 + 10, 2),
        contact("W1AW/4", T0 + 20, 3),
        contact("W1AW/5", T0 + 30, 4),
        contact("K1ABC", T0 + 40, 5),
        contact("k1abc ", T0 + 50, 6),
    ];
    for window in [
        crate::logbook::dedup::DEDUP_WINDOW_SECS,
        POSSIBLE_WINDOW_SECS,
    ] {
        assert_eq!(
            dupe_groups(rows.iter().map(key).collect(), window),
            vec![vec![rows[4].id.unwrap(), rows[5].id.unwrap()]],
            "only the one call, case and padding aside"
        );
    }

    let (a, mut b) = pair("W1AW/KH6", "W1AW/KL7");
    b.qsl_rcvd.lotw = true;
    b.confirmed = true;
    b.award_confirmed = true;
    b.credit_granted = vec!["DXCC".into()];
    let merged = merge(&[a.clone(), b], 0, &HashMap::new());
    assert_eq!(
        merged.record.qsl_rcvd, a.qsl_rcvd,
        "no confirmation crosses calls"
    );
    assert!(!merged.record.confirmed && !merged.record.award_confirmed);
    assert!(merged.record.credit_granted.is_empty());
    assert!(merged
        .conflicts
        .iter()
        .any(|c| c.tag == "CALL" && c.guarded));
}

/// ★ IDENTITY FIELDS. Copies holding different values for the station call, the operator, either
/// grid, the operator's park or summit, or the propagation mode or satellite are two contacts — a
/// club's two calls, a rover's grid corner, a satellite pass beside a simplex contact — and the
/// merge guards every such conflict, confirmed or not. A copy that holds none of a field is no
/// difference: it is the gap a merge fills.
#[test]
fn copies_differing_in_an_identity_field_are_two_contacts() {
    type Set = fn(&mut QsoRecord, &str);
    let fields: [(&str, Set, &str, &str); 7] = [
        (
            "STATION_CALLSIGN",
            |r, v| r.station_callsign = Some(v.into()),
            "W6R",
            "K2DEF",
        ),
        (
            "OPERATOR",
            |r, v| r.operator = Some(v.into()),
            "K2DEF",
            "N2XYZ",
        ),
        (
            "MY_GRIDSQUARE",
            |r, v| r.my_grid = Some(v.into()),
            "FN20",
            "FN21",
        ),
        ("GRIDSQUARE", |r, v| r.grid = Some(v.into()), "FN31", "FN32"),
        (
            "MY_SIG",
            |r, v| {
                r.ota.my_program = Some("POTA".into());
                r.ota.my_ref = Some(v.into());
            },
            "US-0001",
            "US-0002",
        ),
        (
            "PROP_MODE",
            |r, v| r.prop_mode = Some(v.into()),
            "SAT",
            "F2",
        ),
        (
            "PROP_MODE",
            |r, v| {
                r.prop_mode = Some("SAT".into());
                r.sat_name = Some(v.into());
            },
            "AO-91",
            "SO-50",
        ),
    ];
    for (tag, set, x, y) in fields {
        let (mut a, mut b) = pair("W1AW", "W1AW");
        set(&mut a, x);
        set(&mut b, y);
        assert_eq!(distinct(&a, &b, None), Some(tag), "{tag}: {x} / {y}");
        assert_eq!(
            split_distinct(vec![a.clone(), b.clone()], None),
            Vec::<Vec<QsoRecord>>::new(),
            "{tag}: no group"
        );
        let merged = merge(&[a.clone(), b.clone()], 0, &HashMap::new());
        assert!(
            merged.conflicts.iter().any(|c| c.tag == tag && c.guarded),
            "{tag}: guarded though no copy is confirmed"
        );
        // Held by one copy only: no difference, and it fills the kept one.
        let (bare, _) = pair("W1AW", "W1AW");
        assert_eq!(distinct(&bare, &b, None), None, "{tag}");
        let groups = split_distinct(vec![bare.clone(), b.clone()], None);
        assert_eq!(groups, vec![vec![bare.clone(), b.clone()]], "{tag}");
        // Beside two copies that are two contacts, it could be a copy of either: it joins neither.
        let groups = split_distinct(vec![bare.clone(), b.clone(), a.clone()], None);
        assert_eq!(groups, Vec::<Vec<QsoRecord>>::new(), "{tag}: ambiguous");
    }
}

/// A merge takes no upload stamp from a copy it does not keep — neither a stamp nor the
/// `LOTW_QSL_SENT` mark a stamp is read back from — so undoing it can never leave the kept contact
/// claiming an upload it never had. The kept copy's own stamps stand.
#[test]
fn a_merge_takes_no_upload_stamp_from_another_copy() {
    let (mut a, mut b) = pair("W1AW", "W1AW");
    a.upload.qrz = stamp(50);
    b.upload.lotw = stamp(100);
    b.upload.qrz = stamp(200);
    b.extra = vec![("LOTW_QSL_SENT".into(), "Y".into())];
    let merged = merge(&[a.clone(), b], 0, &HashMap::new());
    assert_eq!(merged.record.upload, a.upload);
    assert_eq!(extra_value(&merged.record, "LOTW_QSL_SENT"), None);
    assert!(merged.conflicts.is_empty(), "{:?}", merged.conflicts);
}

/// ★ A QSL-sent mark the operator cleared is a value of its own: a copy marked sent does not
/// bring the mark back by filling a gap. The two are a guarded conflict, and the kept copy's clear
/// stands until the operator chooses.
#[test]
fn a_cleared_qsl_sent_mark_is_never_brought_back_by_a_merge() {
    let (mut a, mut b) = pair("W1AW", "W1AW");
    a.qsl_sent = crate::logbook::QslSent {
        sent: false,
        via: None,
        date_unix: None,
        cleared_unix: Some(T0 + 1_000),
    };
    b.qsl_sent = crate::logbook::QslSent {
        sent: true,
        via: Some(QslVia::Bureau),
        date_unix: Some(T0 - T0 % 86_400),
        cleared_unix: None,
    };
    let merged = merge(&[a.clone(), b], 0, &HashMap::new());
    assert_eq!(merged.record.qsl_sent, a.qsl_sent);
    assert!(merged
        .conflicts
        .iter()
        .any(|c| c.tag == "QSL_SENT" && c.guarded));
    assert_eq!(merged.unchosen(&HashMap::new()), vec!["QSL_SENT"]);
}

/// Operator-written text the copies disagree on is guarded: kept by no default, so "Merge all"
/// cannot drop one copy's comment or notes unseen.
#[test]
fn a_text_the_copies_disagree_on_must_be_chosen() {
    for (tag, set) in [
        (
            "COMMENT",
            (|r: &mut QsoRecord, v: &str| r.comment = Some(v.into())) as fn(&mut QsoRecord, &str),
        ),
        ("NOTES", |r, v| r.notes = Some(v.into())),
    ] {
        let (mut a, mut b) = pair("W1AW", "W1AW");
        set(&mut a, "new antenna");
        set(&mut b, "QSL via bureau, requested");
        let merged = merge(&[a, b], 0, &HashMap::new());
        assert_eq!(merged.unchosen(&HashMap::new()), vec![tag]);
    }
}

/// ★ A GROUP IS BOUNDED. The window is measured from the group's FIRST contact, so a station
/// worked every 45 minutes all afternoon is not one "possible" group spanning hours: each group
/// spans no more than the window.
#[test]
fn a_group_spans_no_more_than_its_window() {
    let rows: Vec<QsoRecord> = (0..6)
        .map(|i| contact("W1AW", T0 + i * 2_700, i + 1))
        .collect();
    let at: HashMap<RecordId, u64> = rows.iter().map(|r| (r.id.unwrap(), r.when_unix)).collect();
    for window in [
        crate::logbook::dedup::DEDUP_WINDOW_SECS,
        POSSIBLE_WINDOW_SECS,
    ] {
        let groups = dupe_groups(rows.iter().map(key).collect(), window);
        for g in &groups {
            let span = at[g.last().unwrap()] - at[&g[0]];
            assert!(span <= window, "a group of {} spans {span} s", g.len());
        }
    }
    let possible = dupe_groups(rows.iter().map(key).collect(), POSSIBLE_WINDOW_SECS);
    assert_eq!(possible.len(), 3, "{possible:?}");
}

/// A side's passthrough references go with the side the merge took: the other copy's parked
/// `MY_POTA_REF` beside its WWFF reference is not written after the kept POTA park, where the
/// next read would take it for the activation.
#[test]
fn a_sides_left_over_references_follow_the_side_taken() {
    let (mut a, mut b) = pair("W1AW", "W1AW");
    a.ota.their_program = Some("POTA".into());
    a.ota.their_ref = Some("US-0001".into());
    b.ota.their_program = Some("WWFF".into());
    b.ota.their_ref = Some("KFF-0001".into());
    b.extra = vec![("POTA_REF".into(), "US-0002".into())];
    let choices: HashMap<String, RecordId> = [("SIG".to_string(), a.id.unwrap())].into();
    let merged = merge(&[a.clone(), b.clone()], 0, &choices);
    assert_eq!(merged.record.ota.their_ref.as_deref(), Some("US-0001"));
    assert_eq!(extra_value(&merged.record, "POTA_REF"), None);
    let reread = parse_adif(&adif_record_own_log(&merged.record)).remove(0);
    assert_eq!(reread.ota.their_ref.as_deref(), Some("US-0001"));

    // Taken from the other copy, its left-over reference comes with it.
    let choices: HashMap<String, RecordId> = [("SIG".to_string(), b.id.unwrap())].into();
    let merged = merge(&[a, b], 0, &choices);
    assert_eq!(merged.record.ota.their_ref.as_deref(), Some("KFF-0001"));
    assert_eq!(extra_value(&merged.record, "POTA_REF"), Some("US-0002"));
}

/// ★ A STATION CALL CHANGED IN BULK IS THE CALL THE UPLOAD IS SIGNED UNDER: the contact's upload
/// stamps (and the `LOTW_QSL_SENT` mark a stamp is read back from) are cleared, as a corrected
/// call's are, so the next upload sends it under the new call. The same call in another case is no
/// change, and every other bulk field keeps every stamp.
#[test]
fn a_bulk_station_call_change_clears_the_upload_stamps() {
    let (_, mut b) = two_copies();
    b.station_callsign = Some("K2DEF".into());
    b.upload.lotw = stamp(100);
    b.extra.push(("LOTW_QSL_SENT".into(), "Y".into()));
    let to = |v: Option<&str>| {
        checked(&[Change {
            field: Field::StationCallsign,
            value: v.map(str::to_string),
        }])
        .unwrap()
    };
    let after = changed(&b, &to(Some("w6r")), &no_country).unwrap();
    assert_eq!(after.station_callsign.as_deref(), Some("W6R"));
    assert_eq!(after.upload, crate::logbook::UploadState::default());
    assert_eq!(extra_value(&after, "LOTW_QSL_SENT"), None);
    assert_eq!(after.qsl_rcvd.card, b.qsl_rcvd.card, "the paper card stays");
    assert_eq!(
        after.credit_submitted, b.credit_submitted,
        "with the card's credit"
    );
    assert!(resends(&b, &after));

    // Cleared: no change of call (the next test), so the stamps stay.
    let cleared = changed(&b, &to(None), &no_country).unwrap();
    assert_eq!(cleared.upload, b.upload);
    assert!(!resends(&b, &cleared));

    assert!(changed(&b, &to(Some("k2def")), &no_country).is_none());
    let rig = changed(
        &b,
        &checked(&[Change {
            field: Field::MyRig,
            value: Some("FT-991A".into()),
        }])
        .unwrap(),
        &no_country,
    )
    .unwrap();
    assert_eq!(rig.upload, b.upload);
    assert!(!resends(&b, &rig));
}

/// ★ A STATION CALL CHANGED IN BULK TAKES OFF WHAT THE SERVICES MATCHED UNDER THE OLD ONE: LoTW's,
/// eQSL's and QRZ's confirmations, their received dates, eQSL's Authenticity-Guaranteed mark and
/// LoTW's DXCC. A paper card stays, with its date and route, and so do the credit codes, card or
/// none: they are an award desk's record and the operator's own. The count is of the
/// confirmations taken off. The same call, case and padding aside, takes off nothing, and
/// neither does any other field.
#[test]
fn a_bulk_station_call_change_clears_the_electronic_confirmations_and_keeps_the_card() {
    let mut r = contact("W1AW", T0, 1);
    r.station_callsign = Some("K2DEF".into());
    r.qsl_rcvd = QslRcvd {
        card: true,
        lotw: true,
        eqsl: true,
        qrz: true,
    };
    r.confirmed = true;
    r.award_confirmed = true;
    r.credit_granted = vec!["DXCC".into()];
    r.credit_submitted = vec!["WAS".into()];
    for (k, v) in [
        ("QSL_RCVD_VIA", "B"),
        ("QSLRDATE", "20260110"),
        ("LOTW_QSLRDATE", "20260105"),
        ("EQSL_QSLRDATE", "20260106"),
        (crate::logbook::EQSL_AG_FIELD, "Y"),
        (crate::logbook::LOTW_DXCC_FIELD, "1"),
        ("CQZ", "5"),
    ] {
        set_extra(&mut r, k, Some(v.into()));
    }
    let to = |v: Option<&str>| {
        checked(&[Change {
            field: Field::StationCallsign,
            value: v.map(str::to_string),
        }])
        .unwrap()
    };

    let after = changed(&r, &to(Some("w6r")), &no_country).unwrap();
    let mut want = r.clone();
    want.station_callsign = Some("W6R".into());
    want.qsl_rcvd = QslRcvd {
        card: true,
        ..QslRcvd::default()
    };
    for k in [
        "LOTW_QSLRDATE",
        "EQSL_QSLRDATE",
        crate::logbook::EQSL_AG_FIELD,
        crate::logbook::LOTW_DXCC_FIELD,
    ] {
        set_extra(&mut want, k, None);
    }
    assert_eq!(
        after, want,
        "the card, its date and route, and its credit stay"
    );
    assert!(
        after.confirmed && after.award_confirmed,
        "the card confirms it"
    );
    assert_eq!(confirmations_cleared(&r, &after), 3);

    // No card: nothing confirms it any more. The credit codes stay: they record what an award
    // desk did and what the operator submitted, not a service's match.
    let mut no_card = r.clone();
    no_card.qsl_rcvd.card = false;
    for k in ["QSL_RCVD_VIA", "QSLRDATE"] {
        set_extra(&mut no_card, k, None);
    }
    let cleared = changed(&no_card, &to(Some("W6R")), &no_country).unwrap();
    assert_eq!(cleared.qsl_rcvd, QslRcvd::default());
    assert!(!cleared.confirmed && !cleared.award_confirmed);
    assert_eq!(
        (&cleared.credit_granted, &cleared.credit_submitted),
        (&no_card.credit_granted, &no_card.credit_submitted),
        "the award desk's record stays"
    );
    assert_eq!(
        extra_value(&cleared, "CQZ"),
        Some("5"),
        "the zone is the log's own"
    );
    assert_eq!(confirmations_cleared(&no_card, &cleared), 3);

    // An eQSL alone never gave credit: a code it holds came from elsewhere, and stays.
    let mut eqsl_only = no_card.clone();
    eqsl_only.qsl_rcvd = QslRcvd {
        eqsl: true,
        ..QslRcvd::default()
    };
    eqsl_only.award_confirmed = false;
    let after = changed(&eqsl_only, &to(Some("W6R")), &no_country).unwrap();
    assert_eq!(after.credit_granted, eqsl_only.credit_granted);
    assert!(!after.confirmed);
    assert_eq!(confirmations_cleared(&eqsl_only, &after), 1);

    // The same call: nothing at all.
    assert!(changed(&r, &to(Some(" k2def ")), &no_country).is_none());
    // Another field: every confirmation stays.
    let rig = changed(
        &r,
        &checked(&[Change {
            field: Field::MyRig,
            value: Some("FT-991A".into()),
        }])
        .unwrap(),
        &no_country,
    )
    .unwrap();
    assert_eq!(
        (&rig.qsl_rcvd, &rig.extra, &rig.credit_granted),
        (&r.qsl_rcvd, &r.extra, &r.credit_granted)
    );
    assert_eq!(confirmations_cleared(&r, &rig), 0);
}

/// The revision a merge compares copies by covers what the edit key leaves out — the contest
/// exchange, the station call, the passthrough, a confirmation — and is blind to an upload stamp
/// landing, which changes nothing a merge decides on.
#[test]
fn a_copys_revision_moves_with_everything_but_a_stamp() {
    use crate::logbook::history::content_key;
    let (a, _) = two_copies();
    let was = content_key(&a);
    let mut stamped = a.clone();
    stamped.upload.clublog = stamp(300);
    assert_eq!(content_key(&stamped), was);
    let moves: [fn(&mut QsoRecord); 4] = [
        |r| {
            r.contest = Some(Box::new(ContestFields {
                contest_id: "CQ-WW-CW".into(),
                ..ContestFields::default()
            }))
        },
        |r| r.station_callsign = Some("W6R".into()),
        |r| r.extra.push(("APP_X_FOO".into(), "bar".into())),
        |r| r.qsl_rcvd.eqsl = true,
    ];
    for (i, change) in moves.into_iter().enumerate() {
        let mut r = a.clone();
        change(&mut r);
        assert_ne!(content_key(&r), was, "change {i}");
        assert_eq!(
            crate::logbook::QsoEdit::project(&r).key(),
            crate::logbook::QsoEdit::project(&a).key(),
            "the edit key is blind to change {i}"
        );
    }
}

/// ★ FILLING IN A BLANK STATION CALL, OR CLEARING ONE, IS NO CHANGE OF CALL, whatever the call in
/// Settings is: a contact naming no station call was uploaded and confirmed
/// under whatever call was in Settings then, which nothing records, and the fill records it. The
/// field changes and nothing else — every stamp, confirmation, date and credit stays — filled with
/// any call, or cleared. Only one call replaced by ANOTHER is a change, and they go.
#[test]
fn a_blank_station_call_filled_or_a_call_cleared_clears_nothing() {
    let mut r = contact("W1AW", T0, 1);
    r.qsl_rcvd = QslRcvd {
        lotw: true,
        eqsl: true,
        qrz: true,
        ..QslRcvd::default()
    };
    r.confirmed = true;
    r.award_confirmed = true;
    r.credit_granted = vec!["DXCC".into()];
    r.upload.lotw = stamp(100);
    r.upload.qrz = stamp(200);
    set_extra(&mut r, "LOTW_QSL_SENT", Some("Y".into()));
    set_extra(&mut r, "LOTW_QSLRDATE", Some("20260105".into()));
    let to = |v: Option<&str>| {
        checked(&[Change {
            field: Field::StationCallsign,
            value: v.map(str::to_string),
        }])
        .unwrap()
    };

    for call in ["KD9TAW", "KA1OLD"] {
        let filled = changed(&r, &to(Some(call)), &no_country).unwrap();
        let mut want = r.clone();
        want.station_callsign = Some(call.into());
        assert_eq!(filled, want, "{call}: the field, and nothing else");
        assert!(!resends(&r, &filled));
        assert_eq!(confirmations_cleared(&r, &filled), 0);

        let blanked = changed(&filled, &to(None), &no_country).unwrap();
        assert_eq!(blanked, r, "{call} cleared: nothing else");
        assert!(!resends(&filled, &blanked));
    }

    // CONTROL: one call replaced by another.
    let filled = changed(&r, &to(Some("KA1OLD")), &no_country).unwrap();
    let other = changed(&filled, &to(Some("W6R")), &no_country).unwrap();
    assert!(resends(&filled, &other));
    assert_eq!(other.upload, crate::logbook::UploadState::default());
    assert_eq!(confirmations_cleared(&filled, &other), 3);
}

/// ★ A PORTABLE /P IS THE SAME STATION CALL (operator ruling), for a change of station call and
/// for the duplicate finder: `KD9TAW/P` over `KD9TAW`, or back, changes the field and nothing else,
/// and copies under the two are one contact. Only /P: `/M`, `/QRP`, `/MM` and a prefix are other
/// calls, both ways.
#[test]
fn a_portable_p_is_the_same_station_call_and_nothing_else_is() {
    let mut r = contact("W1AW", T0, 1);
    r.station_callsign = Some("KD9TAW".into());
    r.qsl_rcvd.lotw = true;
    r.award_confirmed = true;
    r.confirmed = true;
    r.upload.lotw = stamp(100);
    let to = |v: &str| {
        checked(&[Change {
            field: Field::StationCallsign,
            value: Some(v.to_string()),
        }])
        .unwrap()
    };
    let portable = changed(&r, &to("kd9taw/p"), &no_country).unwrap();
    let mut want = r.clone();
    want.station_callsign = Some("KD9TAW/P".into());
    assert_eq!(portable, want, "the field, and nothing else");
    assert!(!resends(&r, &portable) && !resends(&portable, &r));

    let (mut a, mut b) = pair("W1AW", "W1AW");
    a.station_callsign = Some("KD9TAW".into());
    b.station_callsign = Some("KD9TAW/P".into());
    assert_eq!(distinct(&a, &b, None), None);
    assert_eq!(distinct(&b, &a, Some("W6R")), None);
    // A blank copy, under the Settings call: /P aside, the same call.
    b.station_callsign = None;
    assert_eq!(distinct(&a, &b, Some("KD9TAW/P")), None);

    for other in [
        "KD9TAW/M",
        "KD9TAW/QRP",
        "KD9TAW/MM",
        "W1/KD9TAW",
        "KD9TAW/9",
    ] {
        let moved = changed(&r, &to(other), &no_country).unwrap();
        assert!(resends(&r, &moved), "{other}");
        assert_eq!(
            moved.upload,
            crate::logbook::UploadState::default(),
            "{other}"
        );
        let mut b = a.clone();
        b.station_callsign = Some(other.into());
        assert_eq!(distinct(&a, &b, None), Some("STATION_CALLSIGN"), "{other}");
        assert_eq!(distinct(&b, &a, None), Some("STATION_CALLSIGN"), "{other}");
    }
}

/// ★ A MERGE NEVER DROPS A REFERENCE. Copies on the same side (both on one summit), one also
/// holding the park it parked beside it: whichever is kept, the merged contact holds the park.
/// Copies whose side differs (a park beside a summit) are a guarded conflict, so "Merge all"
/// cannot pick one silently; and a parked reference two copies hold differently is guarded too.
#[test]
fn a_merge_never_drops_a_parked_reference_and_guards_a_differing_side() {
    let summit = |r: &mut QsoRecord| {
        r.ota.my_program = Some("SOTA".into());
        r.ota.my_ref = Some("W7A/CN-01".into());
    };
    let (mut a, mut b) = pair("W1AW", "W1AW");
    summit(&mut a);
    summit(&mut b);
    a.extra = vec![
        ("MY_SIG".into(), "POTA".into()),
        ("MY_SIG_INFO".into(), "US-1234".into()),
    ];
    for keep in [0, 1] {
        let merged = merge(&[a.clone(), b.clone()], keep, &HashMap::new());
        assert_eq!(
            extra_value(&merged.record, "MY_SIG_INFO"),
            Some("US-1234"),
            "keep {keep}"
        );
        assert_eq!(extra_value(&merged.record, "MY_SIG"), Some("POTA"));
        assert_eq!(merged.record.ota.my_ref.as_deref(), Some("W7A/CN-01"));
        assert!(merged.conflicts.is_empty(), "{:?}", merged.conflicts);
    }

    // The same summit, two different parks beside it.
    b.extra = vec![
        ("MY_SIG".into(), "POTA".into()),
        ("MY_SIG_INFO".into(), "US-5678".into()),
    ];
    let merged = merge(&[a.clone(), b.clone()], 0, &HashMap::new());
    assert_eq!(merged.unchosen(&HashMap::new()), vec!["MY_SIG_INFO"]);

    // Two programmes neither of which a record can hold beside the other: the worked station's
    // side is a guarded choice.
    let (mut a, mut b) = pair("W1AW", "W1AW");
    a.ota.their_program = Some("POTA".into());
    a.ota.their_ref = Some("US-1234".into());
    b.ota.their_program = Some("WWFF".into());
    b.ota.their_ref = Some("KFF-1234".into());
    let merged = merge(&[a, b], 0, &HashMap::new());
    assert_eq!(merged.unchosen(&HashMap::new()), vec!["SIG"]);
}

/// ★ A HUNTER'S TWO-FER IS KEPT WHOLE. A summit on one copy and a park on the other are both
/// right: whichever copy is kept, and whichever side an older ask chose, the merged contact holds
/// the summit as its side and the park beside it — as a read of the record it writes holds them —
/// with nothing to choose. A park the summit's copy already parks beside it, and a different one
/// on the other copy, are a guarded choice.
#[test]
fn a_merge_of_a_hunters_two_fer_keeps_the_summit_and_the_park() {
    let (mut a, mut b) = pair("W1AW", "W1AW");
    a.ota.their_program = Some("SOTA".into());
    a.ota.their_ref = Some("W7A/CN-01".into());
    b.ota.their_program = Some("POTA".into());
    b.ota.their_ref = Some("US-1234".into());
    for keep in [0, 1] {
        for choices in [HashMap::new(), [("SIG".to_string(), b.id.unwrap())].into()] {
            let merged = merge(&[a.clone(), b.clone()], keep, &choices);
            let r = &merged.record;
            assert_eq!(
                (r.ota.their_program.as_deref(), r.ota.their_ref.as_deref()),
                (Some("SOTA"), Some("W7A/CN-01")),
                "keep {keep}"
            );
            assert_eq!(extra_value(r, "SIG"), Some("POTA"));
            assert_eq!(extra_value(r, "SIG_INFO"), Some("US-1234"));
            assert_eq!(extra_value(r, "POTA_REF"), Some("US-1234"));
            assert!(merged.conflicts.is_empty(), "{:?}", merged.conflicts);
            let reread = parse_adif(&adif_record_own_log(r)).remove(0);
            assert_eq!(reread.ota, r.ota, "the record reads back as merged");
            assert_eq!(extra_value(&reread, "SIG_INFO"), Some("US-1234"));
        }
    }

    // The summit's copy already parks another park: the two parks are the operator's to choose.
    a.extra = vec![
        ("SIG".into(), "POTA".into()),
        ("SIG_INFO".into(), "US-5678".into()),
    ];
    let merged = merge(&[a, b], 0, &HashMap::new());
    assert_eq!(merged.unchosen(&HashMap::new()), vec!["SIG_INFO"]);
}

/// Of copies otherwise equally full, the one a service already holds is the one kept by default:
/// a merge keeps only the kept copy's stamps, so keeping it sends nothing again. A failed upload is
/// no upload. Fuller still wins.
#[test]
fn of_equal_copies_the_one_sent_is_kept() {
    let (a, mut b) = pair("W1AW", "W1AW");
    assert_eq!(suggested_keep(&[a.clone(), b.clone()]), 0, "log order");
    b.upload.qrz = stamp(100);
    assert_eq!(suggested_keep(&[a.clone(), b.clone()]), 1);
    let mut failed = a.clone();
    failed.upload.lotw = Some(UploadStatus {
        outcome: UploadOutcome::Rejected,
        when_unix: 1,
        detail: None,
    });
    assert_eq!(suggested_keep(&[failed.clone(), b.clone()]), 1);
    let mut fuller = a;
    fuller.name = Some("Hiram".into());
    assert_eq!(
        suggested_keep(&[fuller, b]),
        0,
        "the fuller copy still wins"
    );
}

/// ★ A GRID IS ONE GRID AT ANY PRECISION. One logger writes the 4-character square, another the
/// 6-character subsquare: `FN31` and `FN31PR` (case aside) are one contact, either grid. Squares
/// that truly differ are still two. The merge still names the grid for the operator to choose.
#[test]
fn grids_differing_only_in_precision_are_one_contact() {
    for (tag, set) in [
        (
            "GRIDSQUARE",
            (|r: &mut QsoRecord, v: &str| r.grid = Some(v.into())) as fn(&mut QsoRecord, &str),
        ),
        ("MY_GRIDSQUARE", |r, v| r.my_grid = Some(v.into())),
    ] {
        for (x, y, two) in [
            ("FN31", "FN31PR", false),
            ("fn31pr", "FN31", false),
            ("EN52", "EN52WB12", false),
            ("FN31PR", "FN31PS", true),
            ("FN31", "FN32", true),
            ("FN3", "FN31", true),
        ] {
            let (mut a, mut b) = pair("W1AW", "W1AW");
            set(&mut a, x);
            set(&mut b, y);
            assert_eq!(
                distinct(&a, &b, None),
                two.then_some(tag),
                "{tag}: {x} / {y}"
            );
        }
        let (mut a, mut b) = pair("W1AW", "W1AW");
        set(&mut a, "FN31");
        set(&mut b, "FN31PR");
        let merged = merge(&[a, b], 0, &HashMap::new());
        assert_eq!(merged.unchosen(&HashMap::new()), vec![tag]);
    }
}

/// A copy naming no station call is under the call in Settings: beside a copy under another
/// call it is another contact, and beside one under the Settings call it is the same one.
#[test]
fn a_blank_station_call_is_the_settings_call() {
    let (a, mut b) = pair("W1AW", "W1AW");
    b.station_callsign = Some("W6R".into());
    assert_eq!(distinct(&a, &b, Some("KD9TAW")), Some("STATION_CALLSIGN"));
    assert_eq!(distinct(&a, &b, None), None, "no Settings call: a gap");
    b.station_callsign = Some("kd9taw".into());
    assert_eq!(distinct(&a, &b, Some("KD9TAW")), None);
}

/// An older record holding its confirmation only in `confirmed`/`award_confirmed`, with no
/// per-service flag, is written as a LoTW (award) or eQSL confirmation: a changed station call
/// takes that off too, and counts it.
#[test]
fn a_station_call_change_takes_off_an_older_records_confirmation() {
    let mut r = contact("W1AW", T0, 1);
    r.station_callsign = Some("K2DEF".into());
    r.confirmed = true;
    r.award_confirmed = true;
    let to_w6r = checked(&[Change {
        field: Field::StationCallsign,
        value: Some("W6R".into()),
    }])
    .unwrap();
    let after = changed(&r, &to_w6r, &no_country).unwrap();
    assert!(!after.confirmed && !after.award_confirmed);
    assert_eq!(confirmations_cleared(&r, &after), 1);
    r.award_confirmed = false;
    let after = changed(&r, &to_w6r, &no_country).unwrap();
    assert!(!after.confirmed);
    assert_eq!(confirmations_cleared(&r, &after), 1);
}

/// ★ A FILL OR A CLEAR CAN MOVE A CONTACT TO ANOTHER UPLOAD CALL. A contact naming no station call
/// is uploaded under the call in Settings, so filling one with another call, or clearing one that
/// is not the Settings call, changes the call it goes to a service under, though it changes no
/// stamp. Counted only while a service is still owed it; a /P aside, and a call replaced by another
/// (counted as a resend instead), are no move.
#[test]
fn a_fill_or_a_clear_onto_another_upload_call_is_a_move() {
    let to = |v: Option<&str>| {
        checked(&[Change {
            field: Field::StationCallsign,
            value: v.map(str::to_string),
        }])
        .unwrap()
    };
    let mut old = contact("W1AW", T0, 1);
    old.station_callsign = Some("KA1OLD".into());
    old.upload.lotw = stamp(100);
    let cleared = changed(&old, &to(None), &no_country).unwrap();
    assert!(moves(&old, &cleared, Some("KD9NEW")), "KA1OLD to KD9NEW");
    assert!(!moves(&old, &cleared, Some("ka1old")), "KA1OLD all along");

    let blank = contact("K1ABC", T0, 2);
    let filled = changed(&blank, &to(Some("KA1OLD")), &no_country).unwrap();
    assert!(moves(&blank, &filled, Some("KD9NEW")), "KD9NEW to KA1OLD");
    let own = changed(&blank, &to(Some("KD9NEW")), &no_country).unwrap();
    assert!(!moves(&blank, &own, Some("KD9NEW")), "the Settings call");
    assert!(
        !moves(&blank, &filled, None),
        "no Settings call: never sendable"
    );

    // A /P aside, the same call.
    let mut tw = contact("N0XYZ", T0, 3);
    tw.station_callsign = Some("KD9TAW/P".into());
    let cleared = changed(&tw, &to(None), &no_country).unwrap();
    assert!(!moves(&tw, &cleared, Some("KD9TAW")));

    // Sent to every service: nothing goes anywhere again.
    let mut sent = old.clone();
    sent.upload.eqsl = stamp(101);
    sent.upload.qrz = stamp(102);
    sent.upload.clublog = stamp(103);
    let cleared = changed(&sent, &to(None), &no_country).unwrap();
    assert!(!moves(&sent, &cleared, Some("KD9NEW")));

    // CONTROL: one call replaced by another is a resend, not a move.
    let other = changed(&old, &to(Some("W6R")), &no_country).unwrap();
    assert!(resends(&old, &other));
    assert!(!moves(&old, &other, Some("KD9NEW")));
}

/// ★ A MERGE OF COPIES A PORTABLE /P APART KEEPS THE SENT COPY'S STAMPS. KD9TAW and KD9TAW/P are
/// one station call to the clean-up tools but two records to LoTW: a stamp only the deleted copy
/// held comes to the merged contact, whichever spelling it keeps, so the next upload does not put
/// a second, undeletable QSO on LoTW. A stamp the kept copy holds stands. Any other call takes
/// none.
#[test]
fn a_merge_of_copies_a_portable_p_apart_keeps_the_sent_copys_stamps() {
    let (mut a, mut b) = pair("W1AW", "W1AW");
    a.station_callsign = Some("KD9TAW".into());
    a.upload.qrz = stamp(50);
    b.station_callsign = Some("kd9taw/p".into());
    b.upload.lotw = stamp(100);
    b.upload.qrz = stamp(200);
    b.upload.clublog = Some(UploadStatus {
        outcome: UploadOutcome::Rejected,
        when_unix: 300,
        detail: None,
    });
    for chosen in [a.id.unwrap(), b.id.unwrap()] {
        let choices = HashMap::from([("STATION_CALLSIGN".to_string(), chosen)]);
        let merged = merge(&[a.clone(), b.clone()], 0, &choices).record;
        assert_eq!(merged.upload.lotw, b.upload.lotw, "the sent copy's");
        assert_eq!(merged.upload.qrz, a.upload.qrz, "the kept copy's own");
        assert_eq!(merged.upload.clublog, None, "a failed upload is none");
        assert_eq!(merged.upload.eqsl, None);
    }

    // CONTROL: /M is another call.
    b.station_callsign = Some("KD9TAW/M".into());
    let merged = merge(&[a.clone(), b], 0, &HashMap::new()).record;
    assert_eq!(merged.upload, a.upload);
}

/// ★ THE CARRY FOLLOWS THE MERGED CONTACT'S SPELLING. Three copies: the kept one KD9TAW and unsent,
/// one KD9TAW/P and unsent, one KD9TAW and sent to LoTW. The operator picks the /P spelling: the
/// merged KD9TAW/P would sign a second LoTW QSO, so the plain copy's stamp comes with it. And a
/// kept copy naming no station call: the merged contact takes the /P spelling, the plain copy was
/// sent, and its stamp comes too.
#[test]
fn a_merge_carries_a_stamp_sent_under_another_spelling_than_the_merged_contacts() {
    let kept = {
        let mut r = contact("W1AW", T0, 1);
        r.station_callsign = Some("KD9TAW".into());
        r
    };
    let mut portable = contact("W1AW", T0 + 60, 2);
    portable.station_callsign = Some("KD9TAW/P".into());
    let mut sent = contact("W1AW", T0 + 120, 3);
    sent.station_callsign = Some("KD9TAW".into());
    sent.upload.lotw = stamp(100);
    let choices = HashMap::from([("STATION_CALLSIGN".to_string(), portable.id.unwrap())]);
    let merged = merge(&[kept, portable.clone(), sent.clone()], 0, &choices).record;
    assert_eq!(merged.station_callsign.as_deref(), Some("KD9TAW/P"));
    assert_eq!(
        merged.upload.lotw, sent.upload.lotw,
        "not owed to LoTW under /P"
    );
    assert_eq!(merged.upload.eqsl, None);

    // The kept copy names no station call (Settings KD9TAW): the /P spelling chosen.
    let blank = contact("W1AW", T0, 1);
    let choices = HashMap::from([("STATION_CALLSIGN".to_string(), portable.id.unwrap())]);
    let merged = merge(&[blank, sent.clone(), portable], 0, &choices).record;
    assert_eq!(merged.station_callsign.as_deref(), Some("KD9TAW/P"));
    assert_eq!(
        merged.upload.lotw, sent.upload.lotw,
        "not owed to LoTW under /P"
    );
}

/// ★ A STAMP UNDER THE SAME SPELLING, ANOTHER CALL OR ANOTHER WORKED STATION STAYS ITS OWN. Copies
/// all spelled KD9TAW keep the rule every merge has: the kept copy's stamps are its own, even
/// where another copy was sent. A copy under KD9TAW/M, another call, carries nothing, nor does a
/// /P copy of another worked call.
#[test]
fn a_merge_carries_no_stamp_of_the_same_spelling_another_call_or_another_worked_station() {
    let mut kept = contact("W1AW", T0, 1);
    kept.station_callsign = Some("KD9TAW".into());
    let mut same = contact("W1AW", T0 + 60, 2);
    same.station_callsign = Some("kd9taw".into());
    same.upload.lotw = stamp(100);
    let merged = merge(&[kept.clone(), same], 0, &HashMap::new()).record;
    assert_eq!(
        merged.upload.lotw, None,
        "the same spelling: the kept copy's own"
    );

    let mut mobile = contact("W1AW", T0 + 60, 2);
    mobile.station_callsign = Some("KD9TAW/M".into());
    mobile.upload.lotw = stamp(100);
    let choices = HashMap::from([("STATION_CALLSIGN".to_string(), mobile.id.unwrap())]);
    let merged = merge(&[kept.clone(), mobile], 0, &choices).record;
    assert_eq!(merged.station_callsign.as_deref(), Some("KD9TAW/M"));
    assert_eq!(merged.upload.lotw, None, "/M is another call");

    let mut other = contact("W1AW/4", T0 + 60, 2);
    other.station_callsign = Some("KD9TAW/P".into());
    other.upload.lotw = stamp(100);
    let merged = merge(&[kept, other], 0, &HashMap::new()).record;
    assert_eq!(merged.upload.lotw, None, "another worked call");
}

/// An award's status set from Bulk edit or the contact card moves that award's one Credit code
/// and nothing else: Granted takes it off Submitted, Submitted off Granted, a clear off both;
/// another award's code and the award's band credit stay, and so does every other field.
#[test]
fn an_award_status_moves_its_one_credit_code_and_nothing_else() {
    let mut r = contact("JA1ABC", T0, 1);
    r.credit_granted = vec!["DXCC_BAND".into(), "WAS".into()];
    r.credit_submitted = vec!["IOTA".into()];
    r.qsl_rcvd.lotw = true;
    r.award_confirmed = true;
    let set = |field, value: Option<&str>| {
        checked(&[Change {
            field,
            value: value.map(str::to_string),
        }])
        .unwrap()
    };

    let submitted = changed(&r, &set(Field::CreditDxcc, Some("submitted")), &no_country).unwrap();
    assert_eq!(submitted.credit_submitted, ["DXCC", "IOTA"]);
    assert_eq!(submitted.credit_granted, ["DXCC_BAND", "WAS"]);
    assert_eq!(
        value_of(&submitted, Field::CreditDxcc).as_deref(),
        Some("SUBMITTED")
    );
    // Every other field as it was.
    let mut back = submitted.clone();
    back.credit_submitted = r.credit_submitted.clone();
    assert_eq!(back, r);

    let granted = changed(
        &submitted,
        &set(Field::CreditDxcc, Some("GRANTED")),
        &no_country,
    )
    .unwrap();
    assert_eq!(granted.credit_submitted, ["IOTA"]);
    assert_eq!(granted.credit_granted, ["DXCC", "DXCC_BAND", "WAS"]);
    assert_eq!(
        value_of(&granted, Field::CreditDxcc).as_deref(),
        Some("GRANTED")
    );

    let cleared = changed(&granted, &set(Field::CreditDxcc, None), &no_country).unwrap();
    assert_eq!(
        cleared, r,
        "a clear takes the code off both lists, and only it"
    );
    assert_eq!(value_of(&cleared, Field::CreditDxcc), None);

    // Already so: no change.
    assert!(changed(
        &granted,
        &set(Field::CreditDxcc, Some("GRANTED")),
        &no_country
    )
    .is_none());
    // The other awards' fields write their own codes.
    let waz = changed(
        &r,
        &set(Field::CreditCqwazMixed, Some("GRANTED")),
        &no_country,
    )
    .unwrap();
    assert_eq!(waz.credit_granted, ["CQWAZ_MIXED", "DXCC_BAND", "WAS"]);
    let counties = changed(&r, &set(Field::CreditUsaca, Some("SUBMITTED")), &no_country).unwrap();
    assert_eq!(counties.credit_submitted, ["IOTA", "USACA"]);
    // A status that is not one is refused before any contact is read.
    assert!(checked(&[Change {
        field: Field::CreditIota,
        value: Some("yes".into()),
    }])
    .is_err());
    assert!(BULK_FIELDS.contains(&Field::CreditUsaca));
}

/// The award-status fields go over the wire by the award's Credit code.
#[test]
fn award_status_fields_are_named_by_their_credit_code() {
    for (field, name) in [
        (Field::CreditDxcc, "CREDIT_DXCC"),
        (Field::CreditWas, "CREDIT_WAS"),
        (Field::CreditCqwazMixed, "CREDIT_CQWAZ_MIXED"),
        (Field::CreditIota, "CREDIT_IOTA"),
        (Field::CreditUsaca, "CREDIT_USACA"),
    ] {
        assert_eq!(
            serde_json::to_string(&field).unwrap(),
            format!("\"{name}\"")
        );
        assert_eq!(credit_code(field), name.strip_prefix("CREDIT_"));
    }
}

/// The status written survives the log's own ADIF round trip.
#[test]
fn an_award_status_round_trips_through_adif() {
    let r = contact("W9ABC", T0, 2);
    let set = |field, v: &str| {
        checked(&[Change {
            field,
            value: Some(v.into()),
        }])
        .unwrap()
    };
    let a = changed(&r, &set(Field::CreditUsaca, "SUBMITTED"), &no_country).unwrap();
    let a = changed(&a, &set(Field::CreditDxcc, "GRANTED"), &no_country).unwrap();
    let back = parse_adif(&adif_record_own_log(&a));
    assert_eq!(back[0].credit_submitted, ["USACA"]);
    assert_eq!(back[0].credit_granted, ["DXCC"]);
}

/// ★ A merge keeps a grid for the map only only when every copy holding that grid held it so: one
/// copy that counted it (today's callbook mark alone, or none) keeps it counted, so a merge never
/// takes a grid out of anyone's totals. The mark is never merged as a value of its own.
#[test]
fn a_merge_keeps_a_grid_for_the_map_only_only_when_every_copy_held_it_so() {
    use crate::logbook::callbook::{is_map_only_grid, CALLBOOK_FILLED_FIELD, MAP_ONLY_GRID_FIELD};
    let marks = |r: &mut QsoRecord, map_only: bool| {
        r.grid = Some("FN31".into());
        set_extra(r, CALLBOOK_FILLED_FIELD, Some("GRIDSQUARE".into()));
        if map_only {
            set_extra(r, MAP_ONLY_GRID_FIELD, Some("Y".into()));
        }
    };
    let (mut a, mut b) = pair("W1AW", "W1AW");
    marks(&mut a, true);
    marks(&mut b, true);
    let merged = merge(&[a.clone(), b.clone()], 0, &HashMap::new());
    assert!(is_map_only_grid(&merged.record), "both map only");
    assert!(merged.conflicts.is_empty(), "{:?}", merged.conflicts);
    marks(&mut b, false);
    set_extra(&mut b, MAP_ONLY_GRID_FIELD, None);
    let merged = merge(&[a, b], 0, &HashMap::new());
    assert!(!is_map_only_grid(&merged.record), "one copy counted it");
    assert_eq!(crate::logbook::counted_grid(&merged.record), Some("FN31"));
}
