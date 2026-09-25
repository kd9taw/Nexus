//! SPEC-2's C16 at the engine: a change to one contact is addressed by its id, never by where
//! the contact sits in the log, and the Logbook form's whole edit is one change that stores
//! exactly what the form's three commands stored.

use super::*;
use crate::dto::LoggedQso;
use crate::station::RowRefusal;
use crate::test_util::StoredLog;
use tempo_core::logbook::{OtaEdit, QslSent, QslVia, QsoEdit, RecordId, UploadOutcome};

/// A contact as a log holds it, under a fixed id, so two engines can hold the same one — with
/// every field an edit writes filled in, so an edit that forgets one cannot match by accident.
fn contact(n: u64, call: &str) -> QsoRecord {
    let field = |tag: &str, value: &str| format!("<{tag}:{}>{value}", value.len());
    let text = [
        field("CALL", call),
        field("BAND", "20m"),
        field("MODE", "FT8"),
        field("FREQ", "14.074"),
        field("GRIDSQUARE", "FN31"),
        field("STATE", "CT"),
        field("RST_SENT", "-10"),
        field("RST_RCVD", "-12"),
        field("NAME", "Hank"),
        field("QTH", "Newington"),
        field("COMMENT", "tnx"),
        field("NOTES", "dipole"),
        field("TX_PWR", "50"),
        field("MY_GRIDSQUARE", "EN52"),
        field("MY_RIG", "FT-710"),
        field("QSO_DATE", "20260910"),
        field("TIME_ON", &format!("12{:02}00", n % 60)),
        field("TIME_OFF", &format!("12{:02}30", n % 60)),
    ]
    .concat();
    let mut lb = tempo_core::logbook::Logbook::new();
    lb.import_adif(&(text + "<EOR>"));
    let mut r = QsoRecord::clone(&lb.records()[0]);
    r.id = Some(RecordId::Provisional {
        hash: n,
        ordinal: 0,
    });
    r
}

/// An engine whose log holds exactly `rows`, each under its own id — added by the append every
/// logged contact makes, which keeps the id a row brings and follows it into the hot index.
fn engine_holding(rows: &[QsoRecord]) -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.station.append(rows.to_vec(), false);
    let held: Vec<QsoRecord> = e.stored_log().iter().map(|r| QsoRecord::clone(r)).collect();
    assert_eq!(
        held, rows,
        "premise: the log holds exactly the contacts given"
    );
    e
}

/// What the Logbook form holds when the operator saves an edit: the typing over the stored
/// row, and the two park boxes as the form shows them (`""` is an empty box).
struct Draft {
    what: &'static str,
    stored: fn(&mut QsoRecord),
    typed: fn(&mut QsoEdit),
    their: &'static str,
    my: &'static str,
}

/// The `QsoEdit` a form sends for `draft`: its fields, and its park by the form's own rule — a
/// program only beside a filled ref (the stored one, or POTA), and no park at all when both
/// boxes are empty.
fn edit_of(stored: &QsoRecord, draft: &Draft) -> QsoEdit {
    let mut edit = QsoEdit::project(stored);
    (draft.typed)(&mut edit);
    let program = |filled: &str, stored: &Option<String>| {
        (!filled.is_empty()).then(|| {
            stored
                .clone()
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| "POTA".into())
        })
    };
    edit.ota = if draft.their.is_empty() && draft.my.is_empty() {
        OtaEdit::default()
    } else {
        OtaEdit {
            their_ref: Some(draft.their.into()),
            their_program: program(draft.their, &stored.ota.their_program),
            my_ref: Some(draft.my.into()),
            my_program: program(draft.my, &stored.ota.my_program),
        }
    };
    edit
}

/// The three commands the form sent for `edit` until C16 (`Logbook.tsx`'s save), written from
/// the form's source, not from `QsoEdit`: the whole row as the form built it, then a QSL-sent
/// and a card mark where the form's own rules called for one.
fn three_commands(
    e: &mut Engine,
    id: RecordId,
    existing: &QsoRecord,
    edit: &QsoEdit,
    draft: &Draft,
) {
    let mut row = serde_json::json!({
        "call": edit.call, "grid": edit.grid, "band": edit.band, "freqMhz": edit.freq_mhz,
        "mode": edit.mode, "rstSent": edit.rst_sent, "rstRcvd": edit.rst_rcvd,
        "name": edit.name, "qth": edit.qth, "comment": edit.comment, "notes": edit.notes,
        "state": edit.state, "txPower": edit.tx_power, "whenUnix": edit.when_unix,
        "confirmed": existing.confirmed, "awardConfirmed": existing.award_confirmed,
        "upload": LoggedQso::from(existing.clone()).upload,
        "myGrid": edit.my_grid, "myRig": edit.my_rig,
    });
    // `timeOffUnix: endUnix ?? undefined` — an empty end box sends no key at all.
    if let Some(off) = edit.time_off_unix {
        row["timeOffUnix"] = off.into();
    }
    // `parkTheirRef || parkMyRef ? {…} : undefined`, the programs as `edit_of` fills them.
    if !(draft.their.is_empty() && draft.my.is_empty()) {
        row["ota"] = serde_json::json!({
            "theirRef": draft.their, "theirProgram": edit.ota.their_program,
            "myRef": draft.my, "myProgram": edit.ota.my_program, "iota": existing.ota.iota,
        });
    }
    let row: LoggedQso = serde_json::from_value(row).expect("the form's row");
    assert!(e.update_qso(id, row.into()), "the edit");
    // const origVia = existing.qslSent?.sent ? (existing.qslSent.via ?? 'SENT') : ''
    let orig = match (existing.qsl_sent.sent, existing.qsl_sent.via) {
        (false, _) => "",
        (true, Some(via)) => via.code(),
        (true, None) => "SENT",
    };
    let wanted = edit.qsl_sent_via.as_deref().unwrap_or("");
    if wanted != orig && wanted != "SENT" {
        assert!(
            e.mark_qsl_sent(id, QslVia::from_code(wanted)),
            "the sent mark"
        );
    }
    if edit.qsl_card != existing.qsl_rcvd.card {
        assert!(e.mark_qsl_card(id, edit.qsl_card), "the card mark");
    }
}

/// The QSL-sent mark's own clock reading taken from `old`, where the two lie within a few
/// seconds: each path stamps the wall clock when it marks, and nothing else may differ.
fn aligned(mut new: QsoRecord, old: &QsoRecord) -> QsoRecord {
    let near = |a: Option<u64>, b: Option<u64>| match (a, b) {
        (Some(a), Some(b)) if a.abs_diff(b) <= 5 => Some(b),
        _ => a,
    };
    new.qsl_sent.date_unix = near(new.qsl_sent.date_unix, old.qsl_sent.date_unix);
    new.qsl_sent.cleared_unix = near(new.qsl_sent.cleared_unix, old.qsl_sent.cleared_unix);
    new
}

/// ★ THE FORM'S EDIT AS ONE CHANGE STORES EXACTLY WHAT ITS THREE COMMANDS STORED. For every
/// kind of edit the form makes — fields, both QSL marks set and withdrawn, a mark with no
/// method, a corrected call on a confirmed contact, the park rule both ways, the end time —
/// `edit_qso` leaves the contact, and what goes back out to the connectors, exactly as the
/// form's edit + QSL-sent + card commands did.
#[test]
fn the_one_change_stores_what_the_form_s_three_commands_stored() {
    let drafts = [
        Draft {
            what: "an ordinary field edit",
            stored: |_| {},
            typed: |e| {
                e.name = Some("Henry".into());
                e.grid = Some("FN42".into());
                e.rst_sent = Some("-05".into());
                e.comment = Some("fixed".into());
            },
            their: "",
            my: "",
        },
        Draft {
            what: "a field edit, sent by bureau, and a card",
            stored: |_| {},
            typed: |e| {
                e.name = Some("Henry".into());
                e.qsl_sent_via = Some("B".into());
                e.qsl_card = true;
            },
            their: "",
            my: "",
        },
        Draft {
            what: "a direct mark and a card, both withdrawn",
            stored: |r| {
                r.qsl_sent = QslSent {
                    sent: true,
                    via: Some(QslVia::Direct),
                    date_unix: Some(1_788_000_000),
                    cleared_unix: None,
                };
                r.qsl_rcvd.card = true;
            },
            typed: |e| {
                e.qsl_sent_via = None;
                e.qsl_card = false;
            },
            their: "",
            my: "",
        },
        Draft {
            what: "a mark with no method, left as it was",
            stored: |r| {
                r.qsl_sent = QslSent {
                    sent: true,
                    via: None,
                    date_unix: Some(1_788_000_000),
                    cleared_unix: None,
                };
            },
            typed: |e| e.comment = Some("still sent".into()),
            their: "",
            my: "",
        },
        Draft {
            what: "a mark with no method given one",
            stored: |r| {
                r.qsl_sent = QslSent {
                    sent: true,
                    via: None,
                    date_unix: Some(1_788_000_000),
                    cleared_unix: None,
                };
            },
            typed: |e| e.qsl_sent_via = Some("E".into()),
            their: "",
            my: "",
        },
        Draft {
            what: "a corrected call on a confirmed, uploaded contact, with a card",
            stored: |r| {
                r.confirmed = true;
                r.award_confirmed = true;
                r.qsl_rcvd.lotw = true;
                r.credit_granted = vec!["DXCC".into()];
                r.upload.qrz = Some(tempo_core::logbook::UploadStatus {
                    outcome: UploadOutcome::Accepted,
                    when_unix: 1_788_000_300,
                    detail: None,
                });
                r.extra = vec![("LOTW_QSL_SENT".into(), "Y".into())];
            },
            typed: |e| {
                e.call = "W1AWX".into();
                e.qsl_card = true;
            },
            their: "",
            my: "",
        },
        Draft {
            what: "a park filled on their side only",
            stored: |_| {},
            typed: |_| {},
            their: "US-0001",
            my: "",
        },
        Draft {
            what: "both park boxes empty over a stored park",
            stored: |r| {
                r.ota.my_program = Some("SOTA".into());
                r.ota.my_ref = Some("W7W/NG-001".into());
                r.ota.iota = Some("NA-001".into());
            },
            typed: |e| e.name = Some("Henry".into()),
            their: "",
            my: "",
        },
        Draft {
            what: "a park changed beside a stored programme and IOTA",
            stored: |r| {
                r.ota.my_program = Some("WWFF".into());
                r.ota.my_ref = Some("KFF-0001".into());
                r.ota.iota = Some("NA-001".into());
            },
            typed: |_| {},
            their: "",
            my: "KFF-0002",
        },
        Draft {
            what: "the end time corrected",
            stored: |_| {},
            typed: |e| e.time_off_unix = e.time_off_unix.map(|t| t + 60),
            their: "",
            my: "",
        },
        Draft {
            what: "the end time left empty",
            stored: |_| {},
            typed: |e| e.time_off_unix = None,
            their: "",
            my: "",
        },
        Draft {
            what: "band and frequency moved",
            stored: |_| {},
            typed: |e| {
                e.band = "40m".into();
                e.freq_mhz = 7.074;
            },
            their: "",
            my: "",
        },
    ];
    for draft in &drafts {
        let mut stored = contact(7, "W1AW");
        (draft.stored)(&mut stored);
        let id = stored.id.expect("an id");
        let beside = contact(8, "K1ABC");
        let (mut old, mut new) = (
            engine_holding(&[stored.clone(), beside.clone()]),
            engine_holding(&[stored.clone(), beside.clone()]),
        );
        for e in [&mut old, &mut new] {
            e.take_pending_uploads();
        }
        let held = QsoRecord::clone(&new.logged_row(id).expect("held"));
        let edit = edit_of(&held, draft);

        three_commands(&mut old, id, &held, &edit, draft);
        let key = QsoEdit::project(&held).key();
        assert!(
            matches!(new.edit_qso(id, &key, &edit), Ok(Ok(_))),
            "{}",
            draft.what
        );

        let rows = |e: &Engine| -> Vec<QsoRecord> {
            e.stored_log().iter().map(|r| QsoRecord::clone(r)).collect()
        };
        let (was, now) = (rows(&old), rows(&new));
        let now: Vec<QsoRecord> = now
            .into_iter()
            .zip(&was)
            .map(|(n, o)| aligned(n, o))
            .collect();
        assert_eq!(now, was, "{}: the stored contacts differ", draft.what);
        let sent = |e: &mut Engine| -> Vec<(QsoRecord, u8)> {
            e.take_pending_uploads()
                .into_iter()
                .map(|p| (p.rec, p.legs))
                .collect()
        };
        let (was, now) = (sent(&mut old), sent(&mut new));
        assert_eq!(
            now, was,
            "{}: what goes to the connectors differs",
            draft.what
        );
        if draft.what.starts_with("a corrected call") {
            assert_eq!(now.len(), 1, "control: a corrected call IS sent again");
        }
    }
}

/// ⛔ A POSITION NEVER NAMES A CONTACT. A contact above the target is deleted between reading
/// the target and changing it; every change by id still lands on the target, and on nothing
/// else. By position, each would have landed on the contact that slid into its place.
#[test]
fn a_change_by_id_lands_on_its_contact_after_a_delete_above_it() {
    let rows = [
        contact(1, "W1AAA"),
        contact(2, "W2BBB"),
        contact(3, "W3CCC"),
    ];
    let mut e = engine_holding(&rows);
    let target = rows[2].id.expect("an id");
    let key = QsoEdit::project(&e.logged_row(target).expect("held")).key();

    assert!(e.delete_qso(rows[0].id.expect("an id")), "a delete above");
    assert_eq!(
        e.stored_log()[1].id,
        Some(target),
        "control: the target moved up — its old position now names nothing"
    );
    let mut edit = QsoEdit::project(&e.logged_row(target).expect("held"));
    edit.name = Some("Edited".into());
    assert!(matches!(e.edit_qso(target, &key, &edit), Ok(Ok(_))));
    assert!(e.mark_qsl_sent(target, Some(QslVia::Bureau)));
    assert!(e.mark_qsl_card(target, true));
    assert!(e.set_sat_tag(target, Some("AO-91")));
    let mut fixed = QsoRecord::clone(&e.logged_row(target).expect("held"));
    fixed.comment = Some("fixed".into());
    assert!(e.update_qso(target, fixed));

    let log = e.stored_log();
    let now: Vec<_> = log
        .iter()
        .map(|r| {
            (
                r.call.as_str(),
                r.name.as_deref(),
                r.qsl_sent.sent,
                r.qsl_rcvd.card,
                r.sat_name.as_deref(),
                r.comment.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        now,
        [
            ("W2BBB", Some("Hank"), false, false, None, Some("tnx")),
            (
                "W3CCC",
                Some("Edited"),
                true,
                true,
                Some("AO-91"),
                Some("fixed")
            ),
        ],
        "every change is on W3CCC, and W2BBB — at its old place — is untouched"
    );
    assert!(e.delete_qso(target));
    assert_eq!(e.stored_log().len(), 1);
    assert_eq!(
        e.stored_log()[0].call,
        "W2BBB",
        "the delete took the target"
    );
}

/// A change addressed to a contact that is gone, or that changed since the caller read it, is
/// refused and changes nothing — and the refusal of a changed one carries it as it now stands.
/// A background stamp is not a change the key sees, so it never refuses an edit.
#[test]
fn a_change_to_a_gone_or_changed_contact_is_refused_and_changes_nothing() {
    let rows = [contact(1, "W1AAA"), contact(2, "W2BBB")];
    let mut e = engine_holding(&rows);
    let id = rows[1].id.expect("an id");
    let held = QsoRecord::clone(&e.logged_row(id).expect("held"));
    let key = QsoEdit::project(&held).key();
    let mut mine = QsoEdit::project(&held);
    mine.name = Some("Mine".into());

    // A stamp lands between the read and the edit: not refused.
    assert!(e.stamp_qrz_upload(&held, UploadOutcome::Accepted, 1_788_000_000, None));
    assert!(
        matches!(e.edit_qso(id, &key, &mine), Ok(Ok(_))),
        "a stamp refuses nothing"
    );

    // Another writer changes what an edit writes: refused, with the contact as it stands.
    let key = QsoEdit::project(&e.logged_row(id).expect("held")).key();
    let mut theirs = QsoRecord::clone(&e.logged_row(id).expect("held"));
    theirs.grid = Some("FN42".into());
    assert!(e.update_qso(id, theirs));
    let (before, tick) = (e.stored_log(), e.snapshot().log_tick);
    match e.edit_qso(id, &key, &mine) {
        Ok(Err(RowRefusal::Changed(now))) => {
            assert_eq!(now.grid.as_deref(), Some("FN42"), "as it now stands")
        }
        other => panic!("a changed contact must be refused: {other:?}"),
    }
    assert_eq!(e.stored_log(), before, "the refusal changed nothing");
    assert_eq!(e.snapshot().log_tick, tick, "nothing to reload");

    // A QSL-sent code the form does not offer: refused before anything is looked at.
    let key = QsoEdit::project(&e.logged_row(id).expect("held")).key();
    let mut odd = QsoEdit::project(&e.logged_row(id).expect("held"));
    odd.qsl_sent_via = Some("X".into());
    assert!(e.edit_qso(id, &key, &odd).is_err());
    assert_eq!(e.stored_log(), before);

    // Gone: deleted, and never in this log.
    assert!(e.delete_qso(id));
    assert_eq!(e.edit_qso(id, &key, &mine), Ok(Err(RowRefusal::Gone)));
    assert_eq!(
        e.fresh_log_row(
            RecordId::Provisional {
                hash: 99,
                ordinal: 0
            },
            &key
        ),
        Err(RowRefusal::Gone)
    );
    assert!(!e.mark_qsl_card(id, true), "a mark finds no contact either");
    assert_eq!(e.stored_log().len(), 1);
}

/// The LoTW verbs by id: a stamp lands on exactly the contacts it names, wherever they now sit;
/// "already uploaded" stamps exactly the unsent ones; and a batch chosen by hand keeps, in the
/// order it names them, only the contacts still held with a known time of day — LoTW matches
/// on time, so one without can never confirm.
#[test]
fn the_lotw_verbs_by_id_touch_exactly_the_contacts_they_name() {
    let mut confirmed = contact(4, "W4DDD");
    confirmed.award_confirmed = true;
    let mut timeless = contact(5, "W5EEE");
    timeless.time_known = false;
    let rows = [
        contact(1, "W1AAA"),
        contact(2, "W2BBB"),
        contact(3, "W3CCC"),
        confirmed,
        timeless,
        contact(6, "W6FFF"),
    ];
    let id = |i: usize| rows[i].id.expect("an id");
    let mut e = engine_holding(&rows);
    assert_eq!(
        e.lotw_unsent_ids(),
        [id(0), id(1), id(2), id(5)],
        "the default batch: not the confirmed one, not the one with no time of day"
    );
    // A batch by hand, as the upload reads it (C15): the contacts named, from the log's rows,
    // and only those with a time of day.
    let by_hand: Vec<RecordId> = crate::station::rows_named(
        &e.log_rows(),
        &[
            id(5),
            id(4),
            id(3),
            RecordId::Provisional {
                hash: 99,
                ordinal: 0,
            },
            id(0),
        ],
    )
    .expect("the log reads")
    .into_iter()
    .filter(|r| r.time_known)
    .filter_map(|r| r.id)
    .collect();
    assert_eq!(
        by_hand,
        [id(5), id(3), id(0)],
        "a batch by hand: in its own order, only what is held with a time of day"
    );
    assert_eq!(
        e.ids_at_positions(&[2, 0, 99]),
        [id(2), id(0)],
        "the Awards buckets' positions, as they stand"
    );

    assert!(e.delete_qso(id(0)), "a delete above both");
    e.stamp_lotw_upload(&[id(2), id(1)], UploadOutcome::Pending, 1_788_000_000, None);
    let stamped = |e: &Engine| -> Vec<(String, Option<UploadOutcome>)> {
        e.stored_log()
            .iter()
            .map(|r| (r.call.clone(), r.upload.lotw.as_ref().map(|s| s.outcome)))
            .collect()
    };
    let p = Some(UploadOutcome::Pending);
    assert_eq!(
        stamped(&e),
        [
            ("W2BBB".into(), p),
            ("W3CCC".into(), p),
            ("W4DDD".into(), None),
            ("W5EEE".into(), None),
            ("W6FFF".into(), None),
        ],
        "exactly the two named"
    );
    assert_eq!(e.mark_lotw_uploaded_all(), 1, "only W6FFF was still unsent");
    let a = Some(UploadOutcome::Accepted);
    assert_eq!(
        stamped(&e),
        [
            ("W2BBB".into(), p),
            ("W3CCC".into(), p),
            ("W4DDD".into(), None),
            ("W5EEE".into(), None),
            ("W6FFF".into(), a),
        ]
    );
}
