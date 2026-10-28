//! The clean-up tools through the engine and the real store: a merge, a bulk edit and the check's
//! fixes each change the log as one run of the history, the `log.adi` mirror follows, the run's
//! undo puts every contact back, nothing is queued to a service, and a large run waits while the
//! radio transmits.

use super::*;
use crate::loghistory::undo_run;
use crate::logstore::tests::{engine_on_store, flush, Dir};
use crate::logwrite;
use crate::test_util::StoredLog;
use std::cell::RefCell;
use tempo_core::logbook::history::content_key;
use tempo_core::logbook::{adif_header, QsoEdit, UploadOutcome, UploadService, UploadStatus};

thread_local! {
    // A writer racing the change planned on this thread: run after its plan's read, with no lock
    // held, before the change is made. (A plain comment: a doc comment cannot attach through
    // `thread_local!`.)
    static RACE: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

/// [`run_in_parts`]'s seam: the racing writer, if a test set one.
pub(super) fn race() {
    let hook = RACE.with(|r| r.borrow_mut().take());
    if let Some(mut hook) = hook {
        hook();
        RACE.with(|r| *r.borrow_mut() = Some(hook));
    }
}

fn racing<T>(hook: impl FnMut() + 'static, f: impl FnOnce() -> T) -> T {
    RACE.with(|r| *r.borrow_mut() = Some(Box::new(hook)));
    let out = f();
    RACE.with(|r| *r.borrow_mut() = None);
    out
}

/// One ADIF record: `call` on 20 m FT8 on 1 January 2026 at `time`, with `more` fields.
fn adif(call: &str, time: &str, more: &str) -> String {
    format!(
        "<CALL:{}>{call}<BAND:3>20m<FREQ:6>14.074<MODE:3>FT8<QSO_DATE:8>20260101<TIME_ON:6>{time}\
         {more}<EOR>\n",
        call.len()
    )
}

/// A shared engine on a store converted from a `log.adi` of `records`.
fn shared(d: &Dir, records: &[String]) -> Arc<Mutex<Engine>> {
    std::fs::write(d.log(), adif_header() + &records.concat()).unwrap();
    let e = engine_on_store(d);
    flush(&e);
    Arc::new(Mutex::new(e))
}

fn rows(engine: &Mutex<Engine>) -> Vec<QsoRecord> {
    engine
        .stored_log()
        .iter()
        .map(|r| QsoRecord::clone(r))
        .collect()
}

fn by_call(engine: &Mutex<Engine>, call: &str) -> Vec<QsoRecord> {
    rows(engine)
        .into_iter()
        .filter(|r| r.call == call)
        .collect()
}

fn mirror(d: &Dir) -> String {
    std::fs::read_to_string(d.log()).unwrap()
}

fn settle(engine: &Mutex<Engine>) {
    flush(&engine_lock(engine));
}

fn stamp(engine: &Mutex<Engine>, r: &QsoRecord, service: UploadService, when_unix: i64) {
    let (_, d) = logwrite::stamp_push(
        engine,
        r,
        service,
        UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix,
            detail: None,
        },
    );
    d.wait(DURABLE_WAIT).unwrap();
}

/// The merge the finder suggests for its group `g`, as the window sends it back.
fn ask(g: &DupeGroup, choices: &[(&str, usize)]) -> MergeAsk {
    MergeAsk {
        keep: g.copies[g.keep].id.unwrap(),
        copies: g
            .copies
            .iter()
            .map(|r| (r.id.unwrap(), content_key(r)))
            .collect(),
        choices: choices
            .iter()
            .map(|(tag, i)| (tag.to_string(), g.copies[*i].id.unwrap()))
            .collect(),
    }
}

/// Two copies of W1AW, two minutes apart: one LoTW-confirmed and sent to LoTW, one with a name,
/// a grid and a QRZ stamp — and a contact logged an hour later, which only the possible tier
/// groups with them.
fn dupes(d: &Dir) -> Arc<Mutex<Engine>> {
    let engine = shared(
        d,
        &[
            adif("W1AW", "120000", "<LOTW_QSL_RCVD:1>Y<RST_SENT:3>-10"),
            adif("K1ABC", "120100", ""),
            adif(
                "W1AW",
                "120200",
                "<NAME:5>Hiram<QTH:9>Newington<GRIDSQUARE:4>FN31<CQZ:1>5",
            ),
            adif("W1AW", "125900", "<COMMENT:5>later"),
        ],
    );
    let w = by_call(&engine, "W1AW");
    stamp(&engine, &w[0], UploadService::Lotw, 100);
    stamp(&engine, &w[1], UploadService::Qrz, 200);
    engine_lock(&engine).take_pending_uploads();
    engine
}

/// ★ THE MERGE, END TO END. The finder groups the two copies by the log's rule (the contact an
/// hour later only in the possible tier); the merge leaves ONE contact holding the union — the
/// record asserted whole against the rule's own answer on the copies as stored, with the kept
/// copy's own stamp and not the other's — deletes the other, and the mirror follows. It queues
/// nothing. Its run's undo puts both copies back WHOLE, stamps and all: the merge gave the kept
/// contact no stamp, so its undo leaves no upload claim behind.
#[test]
fn a_merge_keeps_the_union_and_its_undo_puts_both_copies_back() {
    let d = Dir::new("tools-merge");
    let engine = dupes(&d);
    let before = by_call(&engine, "W1AW");

    let possible = find_duplicates(&engine, DupeTier::Possible).unwrap();
    assert_eq!(possible.scanned, 4);
    assert_eq!(possible.groups.len(), 1);
    assert_eq!(possible.groups[0].copies.len(), 3);
    let scan = find_duplicates(&engine, DupeTier::Rule).unwrap();
    assert_eq!(scan.groups.len(), 1, "{:?}", scan.groups);
    let g = &scan.groups[0];
    assert_eq!(
        g.copies,
        before[..2].to_vec(),
        "the two copies, whole, in log order"
    );
    assert_eq!(g.keep, 1, "the fuller copy is suggested");

    let want = cleanup::merge(&g.copies, g.keep, &HashMap::new()).record;
    assert_eq!(
        want.upload.lotw, None,
        "the deleted copy's LoTW stamp is not the kept one's"
    );
    assert_eq!(want.upload.qrz.as_ref().map(|s| s.when_unix), Some(200));
    assert!(want.qsl_rcvd.lotw && want.name.as_deref() == Some("Hiram"));
    assert_eq!(want.rst_sent.as_deref(), Some("-10"));

    let (made, dur) = merge_duplicates(&engine, &[ask(g, &[])]);
    let made = made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(made.changed, 2, "one kept and changed, one deleted");
    assert!(made.refused.is_empty());
    let run = made.run.clone().expect("a run");
    let after = by_call(&engine, "W1AW");
    assert_eq!(
        after.len(),
        2,
        "the copy merged in is gone; the later contact stays"
    );
    assert_eq!(after[0], want, "the merged contact, whole");
    assert!(
        engine_lock(&engine).take_pending_uploads().is_empty(),
        "a merge queues nothing"
    );
    settle(&engine);
    assert_eq!(
        mirror(&d).matches("<CALL:4>W1AW").count(),
        2,
        "log.adi follows"
    );

    let (undone, dur) = undo_run(&engine, &run);
    assert_eq!(undone.unwrap().changed, 2);
    dur.wait(DURABLE_WAIT).unwrap();
    let back = by_call(&engine, "W1AW");
    assert_eq!(back.len(), 3);
    for was in &before {
        let now = back.iter().find(|r| r.id == was.id).expect("back by id");
        assert_eq!(now, was, "each copy whole, its stamps its own");
    }
    assert!(engine_lock(&engine).take_pending_uploads().is_empty());
    settle(&engine);
    assert_eq!(mirror(&d).matches("<CALL:4>W1AW").count(), 3);
}

/// A merge whose copies disagree on a field a confirmed copy holds waits for the operator's
/// choice, changing nothing; chosen, it is made with the value chosen.
#[test]
fn a_guarded_conflict_waits_for_a_choice() {
    let d = Dir::new("tools-merge-guarded");
    let engine = shared(
        &d,
        &[
            adif("W1AW", "120000", "<LOTW_QSL_RCVD:1>Y<NAME:4>Bill"),
            adif("W1AW", "120100", "<NAME:5>Hiram<QTH:9>Newington"),
        ],
    );
    let scan = find_duplicates(&engine, DupeTier::Rule).unwrap();
    let g = &scan.groups[0];
    let name = g.conflicts.iter().find(|c| c.tag == "NAME").unwrap();
    assert!(name.guarded);
    let keep_other = MergeAsk {
        keep: g.copies[1].id.unwrap(),
        ..ask(g, &[])
    };

    let (made, _) = merge_duplicates(&engine, std::slice::from_ref(&keep_other));
    let made = made.unwrap();
    assert_eq!(made.changed, 0);
    assert_eq!(made.run, None);
    assert_eq!(made.refused, vec![(keep_other.keep, "needsChoice")]);
    assert_eq!(rows(&engine).len(), 2, "nothing was merged");

    let chosen = MergeAsk {
        choices: [("NAME".to_string(), g.copies[0].id.unwrap())].into(),
        ..keep_other
    };
    let (made, dur) = merge_duplicates(&engine, &[chosen]);
    assert_eq!(made.unwrap().changed, 2);
    dur.wait(DURABLE_WAIT).unwrap();
    let left = rows(&engine);
    assert_eq!(left.len(), 1);
    assert_eq!(
        left[0].name.as_deref(),
        Some("Bill"),
        "the confirmed copy's, as chosen"
    );
    assert!(left[0].qsl_rcvd.lotw);
    assert_eq!(left[0].qth.as_deref(), Some("Newington"));
}

/// A copy another writer edits after the merge was shown — even between the plan's read and the
/// change — is never merged over: the merge plans again, finds the copy changed, and leaves the
/// group as it is.
#[test]
fn a_copy_changed_since_it_was_shown_is_not_merged_over() {
    let d = Dir::new("tools-merge-changed");
    let engine = dupes(&d);
    let scan = find_duplicates(&engine, DupeTier::Rule).unwrap();
    let the_ask = ask(&scan.groups[0], &[]);
    let target = scan.groups[0].copies[0].clone();
    let once = std::rc::Rc::new(std::cell::Cell::new(false));
    let hook = {
        let (engine, once) = (Arc::clone(&engine), once.clone());
        move || {
            if !once.replace(true) {
                let key = QsoEdit::project(&target).key();
                let mut edit = QsoEdit::project(&target);
                edit.comment = Some("theirs".into());
                let (made, d) = logwrite::edit_row(&engine, target.id.unwrap(), &key, &edit);
                made.unwrap().unwrap();
                d.wait(DURABLE_WAIT).unwrap();
            }
        }
    };
    let (made, _) = racing(hook, || {
        merge_duplicates(&engine, std::slice::from_ref(&the_ask))
    });
    let made = made.unwrap();
    assert_eq!(made.changed, 0);
    assert_eq!(made.refused, vec![(the_ask.keep, "changed")]);
    assert_eq!(by_call(&engine, "W1AW").len(), 3, "both copies stand");
    assert!(by_call(&engine, "W1AW")
        .iter()
        .any(|r| r.comment.as_deref() == Some("theirs")));
}

/// ★ BULK EDIT. The preview counts what the edit would change; the edit makes exactly that, as
/// one run, keeping every confirmation and upload stamp and queueing nothing — a call is not a
/// field it offers — and the run's undo puts every contact back whole.
#[test]
fn a_bulk_edit_previews_applies_and_undoes_and_keeps_every_stamp() {
    let d = Dir::new("tools-bulk");
    let engine = shared(
        &d,
        &[
            adif("W1AW", "120000", "<MY_GRIDSQUARE:4>FN20<LOTW_QSL_RCVD:1>Y"),
            adif("K1ABC", "120100", "<MY_GRIDSQUARE:4>EM10"),
            adif("N0XYZ", "120200", ""),
        ],
    );
    let before = rows(&engine);
    stamp(&engine, &before[1], UploadService::Qrz, 300);
    engine_lock(&engine).take_pending_uploads();
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();
    let changes = [
        Change {
            field: Field::MyGridsquare,
            value: Some("EM10".into()),
        },
        Change {
            field: Field::Operator,
            value: Some("k2def".into()),
        },
    ];

    let preview = bulk_preview(&engine, &ids, &changes).unwrap();
    assert_eq!(
        preview,
        BulkPreview {
            selected: 3,
            change: 3,
            unchanged: 0,
            missing: 0,
            recall: 0,
            resend: 0,
            unconfirm: 0,
            moves: 0,
            move_to: None,
            move_from: vec![],
            activation_changes: 0,
            activation_uploaded: 0,
            activation_exported: 0,
            activation_files: vec![],
        }
    );
    let only_grid = &changes[..1];
    let preview = bulk_preview(&engine, &ids, only_grid).unwrap();
    assert_eq!(
        (preview.change, preview.unchanged),
        (2, 1),
        "K1ABC is already EM10"
    );

    let (made, dur) = bulk_edit(&engine, &ids, ids.len(), &MovesAck::default(), &changes);
    let made = made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(made.changed, 3);
    let after = rows(&engine);
    for (was, now) in before.iter().zip(&after) {
        let mut want = was.clone();
        want.my_grid = Some("EM10".into());
        want.operator = Some("K2DEF".into());
        assert_eq!(
            *now, want,
            "only the two fields, every stamp and confirmation kept"
        );
    }
    assert!(
        engine_lock(&engine).take_pending_uploads().is_empty(),
        "a bulk edit queues nothing"
    );
    settle(&engine);
    assert_eq!(mirror(&d).matches("<OPERATOR:5>K2DEF").count(), 3);

    let call = [Change {
        field: Field::Gridsquare,
        value: Some("FN31".into()),
    }];
    assert!(
        bulk_edit(&engine, &ids, ids.len(), &MovesAck::default(), &call)
            .0
            .is_err(),
        "not a bulk field"
    );

    let (undone, dur) = undo_run(&engine, &made.run.unwrap());
    assert_eq!(undone.unwrap().changed, 3);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(rows(&engine), before, "every contact back, whole");
}

/// ★ A PARK RE-TAG. Contacts logged without the activation running, and one under the wrong park,
/// take a two-fer as one run: the preview counts what changes, names the contact already uploaded
/// and the exported activation file the change puts out of date; the edit keeps every stamp and
/// queues nothing, the day's counter follows, and ONE undo puts back the log byte for byte.
#[test]
fn a_park_re_tag_previews_exports_applies_and_one_undo_restores_byte_identical() {
    let d = Dir::new("tools-bulk-retag");
    let me = "<STATION_CALLSIGN:6>KD9TAW";
    let engine = shared(
        &d,
        &[
            adif("W1AW", "120000", me),
            adif(
                "K1ABC",
                "120100",
                &format!("{me}<MY_SIG:4>POTA<MY_SIG_INFO:7>US-0001"),
            ),
            adif(
                "N0XYZ",
                "120200",
                &format!("{me}<MY_SIG:4>SOTA<MY_SOTA_REF:10>W7A/MN-001"),
            ),
        ],
    );
    let rows0 = rows(&engine);
    stamp(&engine, &rows0[1], UploadService::Lotw, 300);
    engine_lock(&engine).take_pending_uploads();
    // The wrong park's file for that day was already exported.
    let exports = d.log().with_file_name("pota_exports.json");
    engine_lock(&engine).set_pota_exports_path(exports);
    engine_lock(&engine).note_activation_exported("US-0001", rows0[1].when_unix, Some("KD9TAW"));
    settle(&engine);
    let before = rows(&engine);
    let mirror_before = mirror(&d);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();
    let retag = [Change {
        field: Field::MyPotaRef,
        value: Some("us-0002, US-0003".into()),
    }];

    let p = bulk_preview(&engine, &ids, &retag).unwrap();
    assert_eq!(
        (p.selected, p.change, p.unchanged),
        (3, 2, 1),
        "the summit of mine is SOTA's: no park goes over it"
    );
    assert_eq!(p.activation_changes, 2);
    assert_eq!(p.activation_uploaded, 1, "K1ABC was sent to LoTW");
    assert_eq!(
        p.activation_exported, 1,
        "K1ABC is in the exported US-0001 file"
    );
    assert_eq!(
        p.activation_files,
        vec!["US-0001 2026-01-01 KD9TAW".to_string()]
    );

    let (made, dur) = bulk_edit(&engine, &ids, ids.len(), &MovesAck::default(), &retag);
    let made = made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(made.changed, 2);
    let after = rows(&engine);
    for i in 0..2 {
        assert_eq!(after[i].ota.my_program.as_deref(), Some("POTA"));
        assert_eq!(after[i].ota.my_ref.as_deref(), Some("US-0002,US-0003"));
        assert_eq!(after[i].upload, before[i].upload, "every stamp kept");
    }
    assert_eq!(after[2], before[2]);
    assert!(
        engine_lock(&engine).take_pending_uploads().is_empty(),
        "a re-tag queues nothing"
    );
    let day = before[0].when_unix / 86_400;
    let count = |park: &str| {
        engine_lock(&engine)
            .station()
            .hot()
            .activation_day_count(park, day)
    };
    assert_eq!(
        (count("US-0001"), count("US-0002"), count("US-0003")),
        (0, 2, 2),
        "the day's counter follows the re-tag, at each park of the two-fer"
    );

    let (undone, dur) = undo_run(&engine, &made.run.unwrap());
    assert_eq!(undone.unwrap().changed, 2, "one undo takes back both");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(rows(&engine), before, "every contact back, whole");
    settle(&engine);
    assert_eq!(mirror(&d), mirror_before, "log.adi back byte for byte");
    assert_eq!(
        (count("US-0001"), count("US-0002"), count("US-0003")),
        (1, 0, 0)
    );
}

/// A bulk edit that changes contacts in an exported activation file without moving them out of
/// it — a grid or a power set on them — still names the file: its contents no longer match the
/// log. A contact in no exported file names none.
#[test]
fn a_bulk_edit_of_contacts_in_an_exported_file_names_it_though_none_moves() {
    let d = Dir::new("tools-bulk-exported-stay");
    let me = "<STATION_CALLSIGN:6>KD9TAW";
    let engine = shared(
        &d,
        &[
            adif("W1AW", "120000", me),
            adif(
                "K1ABC",
                "120100",
                &format!("{me}<MY_SIG:4>POTA<MY_SIG_INFO:7>US-0001"),
            ),
            adif(
                "N0XYZ",
                "120200",
                &format!("{me}<MY_SIG:4>POTA<MY_SIG_INFO:7>US-0001"),
            ),
        ],
    );
    let rows0 = rows(&engine);
    engine_lock(&engine).set_pota_exports_path(d.log().with_file_name("pota_exports.json"));
    engine_lock(&engine).note_activation_exported("US-0001", rows0[1].when_unix, Some("KD9TAW"));
    let ids: Vec<RecordId> = rows0.iter().map(|r| r.id.unwrap()).collect();
    for change in [
        Change {
            field: Field::MyGridsquare,
            value: Some("EN52".into()),
        },
        Change {
            field: Field::TxPwr,
            value: Some("5".into()),
        },
    ] {
        let p = bulk_preview(&engine, &ids, std::slice::from_ref(&change)).unwrap();
        assert_eq!(p.change, 3, "{:?}", change.field);
        assert_eq!(p.activation_changes, 0, "nothing moves: {:?}", change.field);
        assert_eq!(
            (p.activation_exported, p.activation_files.clone()),
            (2, vec!["US-0001 2026-01-01 KD9TAW".to_string()]),
            "K1ABC and N0XYZ are in the exported US-0001 file: {:?}",
            change.field
        );
    }
    // Only W1AW, in no file: nothing to name.
    let p = bulk_preview(
        &engine,
        &ids[..1],
        &[Change {
            field: Field::MyGridsquare,
            value: Some("EN52".into()),
        }],
    )
    .unwrap();
    assert_eq!((p.change, p.activation_exported), (1, 0));
    assert!(p.activation_files.is_empty());
}

/// A two-fer re-spelled in another order is the same activation and the same files (each file
/// names one park): the preview neither counts it as moved nor names an exported file. Another
/// park, as a positive control, does both.
#[test]
fn a_two_fer_re_ordered_is_no_activation_change() {
    let d = Dir::new("tools-bulk-two-fer-order");
    let engine = shared(
        &d,
        &[adif(
            "K1ABC",
            "120100",
            "<STATION_CALLSIGN:6>KD9TAW<MY_SIG:4>POTA<MY_SIG_INFO:15>US-0001,US-0002",
        )],
    );
    let rows0 = rows(&engine);
    engine_lock(&engine).set_pota_exports_path(d.log().with_file_name("pota_exports.json"));
    engine_lock(&engine).note_activation_exported("US-0001", rows0[0].when_unix, Some("KD9TAW"));
    let ids = [rows0[0].id.unwrap()];
    let retag = |v: &str| {
        let p = bulk_preview(
            &engine,
            &ids,
            &[Change {
                field: Field::MyPotaRef,
                value: Some(v.into()),
            }],
        )
        .unwrap();
        (p.change, p.activation_changes, p.activation_exported)
    };
    assert_eq!(retag("US-0002,US-0001"), (1, 0, 0), "the same two parks");
    assert_eq!(
        retag("US-0002,US-0003"),
        (1, 1, 1),
        "US-0001 left: its file named"
    );
}

/// ★ THE CHECK. Each kind of problem is found with its proposed fix; only the fixes ticked are
/// made, as one run; a fix whose field changed since the check is left out; the run's undo puts
/// the contacts back.
#[test]
fn the_check_finds_each_problem_and_makes_only_the_ticked_fixes() {
    let d = Dir::new("tools-check");
    let engine = shared(
        &d,
        &[
            adif("W1AW", "120000", "<GRIDSQUARE:7>FN31PRX<DXCC:3>291"),
            adif("K1ABC", "120100", "<MY_GRIDSQUARE:4>ZZ99"),
            "<CALL:5>N0XYZ<FREQ:6>14.074<MODE:3>FT8<QSO_DATE:8>20260101<TIME_ON:6>120200<EOR>\n"
                .into(),
            adif("VE3AA", "120300", "").replace("<BAND:3>20m", "<BAND:3>40m"),
            adif(
                "G4AAA",
                "235000",
                "<QSO_DATE_OFF:8>20260101<TIME_OFF:6>000500",
            ),
            adif("JA1AA", "120500", "<SIG:4>POTA<SIG_INFO:6>us1234"),
            adif("W2AA", "120600", "<MY_SIG:4>SOTA<MY_SIG_INFO:6>W2/GC1"),
            "<CALL:4>W3AA<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20990101<TIME_ON:6>120000<EOR>\n".into(),
        ],
    );
    // The country file's answer, as the command layer gives it: W1AW is 291, never 1.
    let mismatch = |call: &str, n: u32| (call == "W1AW" && n != 1).then_some(Some(1));
    let now = 1_790_000_000;
    let (checked, problems) = check(&engine, &mismatch, now).unwrap();
    assert_eq!(checked, 8);
    let found: Vec<(String, ProblemKind, Option<Option<String>>)> = problems
        .iter()
        .map(|p| {
            (
                p.call.clone(),
                p.kind,
                p.fix.as_ref().map(|f| f.value.clone()),
            )
        })
        .collect();
    let s = |v: &str| Some(Some(v.to_string()));
    assert_eq!(
        found,
        vec![
            ("W1AW".into(), ProblemKind::Grid, s("FN31PR")),
            ("W1AW".into(), ProblemKind::Dxcc, s("1")),
            ("K1ABC".into(), ProblemKind::MyGrid, Some(None)),
            ("N0XYZ".into(), ProblemKind::NoBand, s("20m")),
            ("VE3AA".into(), ProblemKind::BandFreq, s("20m")),
            ("G4AAA".into(), ProblemKind::EndBeforeStart, s("1767312300")),
            ("JA1AA".into(), ProblemKind::Pota, s("US-1234")),
            ("W2AA".into(), ProblemKind::Sota, None),
            ("W3AA".into(), ProblemKind::Future, None),
        ],
    );
    let before = rows(&engine);

    // Tick the grid, the band and the park — not the DXCC number. K1ABC's grid is edited in
    // between: its fix is left out.
    let fix = |p: &Problem| Fix {
        id: p.id,
        field: p.field.unwrap(),
        expect: p.current.clone(),
        value: p.fix.clone().unwrap().value,
    };
    let ticked: Vec<Fix> = problems
        .iter()
        .filter(|p| {
            matches!(
                p.kind,
                ProblemKind::Grid | ProblemKind::NoBand | ProblemKind::Pota | ProblemKind::MyGrid
            )
        })
        .map(fix)
        .collect();
    let k1 = before.iter().find(|r| r.call == "K1ABC").unwrap();
    let mut edit = QsoEdit::project(k1);
    edit.my_grid = Some("EM10".into());
    let (m, dd) = logwrite::edit_row(&engine, k1.id.unwrap(), &QsoEdit::project(k1).key(), &edit);
    m.unwrap().unwrap();
    dd.wait(DURABLE_WAIT).unwrap();
    let before = rows(&engine);

    let (made, dur) = apply_fixes(&engine, &ticked);
    let made = made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(made.changed, 3);
    assert_eq!(made.refused, vec![(k1.id.unwrap(), "changed")]);
    let after = rows(&engine);
    let of = |call: &str| after.iter().find(|r| r.call == call).unwrap().clone();
    assert_eq!(of("W1AW").grid.as_deref(), Some("FN31PR"));
    assert_eq!(of("W1AW").dxcc, Some(291), "not ticked: not made");
    assert_eq!(of("N0XYZ").band, "20m");
    assert_eq!(of("JA1AA").ota.their_ref.as_deref(), Some("US-1234"));
    assert_eq!(
        of("K1ABC").my_grid.as_deref(),
        Some("EM10"),
        "the operator's edit stands"
    );
    for (was, now) in before.iter().zip(&after) {
        if !["W1AW", "N0XYZ", "JA1AA"].contains(&was.call.as_str()) {
            assert_eq!(was, now, "{} untouched", was.call);
        }
    }
    assert!(engine_lock(&engine).take_pending_uploads().is_empty());

    let (undone, dur) = undo_run(&engine, &made.run.unwrap());
    assert_eq!(undone.unwrap().changed, 3);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(rows(&engine), before);
}

/// ★ TWO CONTACTS ARE NEVER MERGED. Copies a minute apart under two calls sharing a base call
/// (`W1AW/KH6`, `W1AW/KL7` — two entities, one LoTW-confirmed), or under one call but two station
/// calls (a club's two calls), and one call worked two hours apart, are no group in either tier;
/// a merge of them asked by hand anyway is refused as `distinct`, even with every conflict
/// chosen, and changes nothing.
#[test]
fn copies_under_two_calls_or_two_station_calls_are_never_merged() {
    let d = Dir::new("tools-merge-distinct");
    let engine = shared(
        &d,
        &[
            adif("W1AW/KH6", "120000", ""),
            adif(
                "W1AW/KL7",
                "120100",
                "<LOTW_QSL_RCVD:1>Y<CREDIT_GRANTED:4>DXCC",
            ),
            adif("K1ABC", "130000", "<STATION_CALLSIGN:3>W6R"),
            adif("K1ABC", "130100", "<STATION_CALLSIGN:5>K2DEF<NAME:3>Bob"),
            adif("N0XYZ", "090000", ""),
            adif("N0XYZ", "110100", ""),
        ],
    );
    for tier in [DupeTier::Rule, DupeTier::Possible] {
        assert!(find_duplicates(&engine, tier).unwrap().groups.is_empty());
    }
    let before = rows(&engine);
    // Two calls; two station calls; and one call two hours apart — wider than any window.
    for pair in [&before[..2], &before[2..4], &before[4..]] {
        let copies: Vec<QsoRecord> = pair.to_vec();
        let conflicts = cleanup::merge(&copies, 0, &HashMap::new()).conflicts;
        let g = DupeGroup {
            copies,
            keep: 0,
            conflicts: conflicts.clone(),
        };
        let every: Vec<(&str, usize)> = conflicts.iter().map(|c| (c.tag.as_str(), 1)).collect();
        let the_ask = ask(&g, &every);
        let (made, _) = merge_duplicates(&engine, std::slice::from_ref(&the_ask));
        let made = made.unwrap();
        assert_eq!(made.changed, 0);
        assert_eq!(made.run, None);
        assert_eq!(made.refused, vec![(the_ask.keep, "distinct")]);
    }
    assert_eq!(rows(&engine), before, "nothing was merged");
}

/// A merge naming one contact twice — in one group, or in two — is refused whole, and a choice
/// naming a copy that holds no value of its conflict is refused as unchosen: neither ever reaches
/// the log.
#[test]
fn a_merge_of_a_malformed_ask_is_refused() {
    let d = Dir::new("tools-merge-shape");
    let engine = dupes(&d);
    let scan = find_duplicates(&engine, DupeTier::Rule).unwrap();
    let g = &scan.groups[0];
    let the_ask = ask(g, &[]);
    let before = rows(&engine);

    let mut twice = the_ask.clone();
    twice.copies.push(twice.copies[0].clone());
    assert!(merge_duplicates(&engine, &[twice]).0.is_err());
    assert!(
        merge_duplicates(&engine, &[the_ask.clone(), the_ask.clone()])
            .0
            .is_err()
    );

    let mut stray = the_ask.clone();
    stray
        .choices
        .insert("RST_SENT".into(), g.copies[g.keep].id.unwrap());
    let (made, _) = merge_duplicates(&engine, &[stray]);
    assert_eq!(made.unwrap().refused, vec![(the_ask.keep, "needsChoice")]);
    assert_eq!(rows(&engine), before);
}

/// ★ The revision a merge was shown is the copy's whole content: a field the edit form does not
/// carry (here the operator, set by a bulk edit) changed after the scan leaves the group as it is.
#[test]
fn a_copy_changed_outside_the_edit_form_is_not_merged_over() {
    let d = Dir::new("tools-merge-revision");
    let engine = dupes(&d);
    let scan = find_duplicates(&engine, DupeTier::Rule).unwrap();
    let the_ask = ask(&scan.groups[0], &[]);
    let other = scan.groups[0].copies[0].id.unwrap();
    let (made, dur) = bulk_edit(
        &engine,
        &[other],
        1,
        &MovesAck::default(),
        &[Change {
            field: Field::Operator,
            value: Some("N2XYZ".into()),
        }],
    );
    assert_eq!(made.unwrap().changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    let before = rows(&engine);
    let (made, _) = merge_duplicates(&engine, std::slice::from_ref(&the_ask));
    assert_eq!(made.unwrap().refused, vec![(the_ask.keep, "changed")]);
    assert_eq!(rows(&engine), before);
}

/// ★ A bulk edit acts only on the count the operator confirmed: handed a different number of
/// contacts it changes nothing.
#[test]
fn a_bulk_edit_acts_only_on_the_count_confirmed() {
    let d = Dir::new("tools-bulk-count");
    let engine = shared(
        &d,
        &[adif("W1AW", "120000", ""), adif("K1ABC", "120100", "")],
    );
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();
    let rig = [Change {
        field: Field::MyRig,
        value: Some("IC-7300".into()),
    }];
    for expect in [1, 3] {
        let (made, _) = bulk_edit(&engine, &ids, expect, &MovesAck::default(), &rig);
        assert_eq!(made, Err(NOT_AS_PREVIEWED.to_string()));
    }
    assert_eq!(rows(&engine), before);
}

/// ★ AN AWARD'S STATUS, SET IN BULK (the Awards view's "Mark submitted", the contact card's
/// choice). Refused, changing nothing, on any count but the one the operator confirmed; made, it
/// moves that award's one Credit code — every other field, the other awards' codes and the band
/// credit stay — the store writes it to the ADIF mirror, and the run's undo puts every contact back.
#[test]
fn an_award_status_is_set_only_on_the_count_confirmed_and_undoes_whole() {
    let d = Dir::new("tools-bulk-credit");
    let engine = shared(
        &d,
        &[
            adif(
                "JA1ABC",
                "120000",
                "<LOTW_QSL_RCVD:1>Y<CREDIT_GRANTED:9>DXCC_BAND<CREDIT_SUBMITTED:4>IOTA",
            ),
            adif("DL1ABC", "120100", "<QSL_RCVD:1>Y"),
        ],
    );
    engine_lock(&engine).take_pending_uploads();
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();
    let submit = [Change {
        field: Field::CreditDxcc,
        value: Some("SUBMITTED".into()),
    }];
    let preview = bulk_preview(&engine, &ids, &submit).unwrap();
    assert_eq!((preview.change, preview.moves, preview.recall), (2, 0, 0));
    for expect in [1, 3] {
        let (made, _) = bulk_edit(&engine, &ids, expect, &MovesAck::default(), &submit);
        assert_eq!(made, Err(NOT_AS_PREVIEWED.to_string()));
    }
    assert_eq!(
        rows(&engine),
        before,
        "an unconfirmed count changes nothing"
    );

    let (made, dur) = bulk_edit(
        &engine,
        &ids,
        preview.selected,
        &MovesAck::default(),
        &submit,
    );
    let made = made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(made.changed, 2);
    let after = rows(&engine);
    for (was, now) in before.iter().zip(&after) {
        let mut want = was.clone();
        want.credit_submitted.push("DXCC".into());
        want.credit_submitted.sort();
        assert_eq!(*now, want, "only DXCC's code moves");
    }
    assert_eq!(after[0].credit_granted, ["DXCC_BAND"]);
    settle(&engine);
    assert_eq!(mirror(&d).matches("<CREDIT_SUBMITTED:4>DXCC").count(), 1);
    assert_eq!(
        mirror(&d).matches("<CREDIT_SUBMITTED:9>DXCC,IOTA").count(),
        1
    );
    assert!(
        engine_lock(&engine).take_pending_uploads().is_empty(),
        "nothing is sent"
    );

    let (undone, dur) = undo_run(&engine, &made.run.unwrap());
    assert_eq!(undone.unwrap().changed, 2);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(rows(&engine), before, "every contact back, whole");
}

/// ★ A STATION CALL CHANGED IN BULK. The preview counts the contacts uploaded under another call;
/// the edit clears their upload stamps — the next upload sends them under the new call — and
/// queues nothing itself. A contact already under the new call is left as it is, stamps and all.
#[test]
fn a_bulk_station_call_change_owes_the_contact_to_every_service_again() {
    let d = Dir::new("tools-bulk-station");
    let engine = shared(
        &d,
        &[
            adif(
                "W1AW",
                "120000",
                "<STATION_CALLSIGN:5>KD0AA<LOTW_QSL_RCVD:1>Y",
            ),
            adif("K1ABC", "120100", "<STATION_CALLSIGN:3>W6R"),
            adif("N0XYZ", "120200", "<STATION_CALLSIGN:5>KD0AA"),
        ],
    );
    for r in rows(&engine) {
        stamp(&engine, &r, UploadService::Lotw, 100);
    }
    engine_lock(&engine).take_pending_uploads();
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();
    let w6r = [Change {
        field: Field::StationCallsign,
        value: Some("w6r".into()),
    }];
    let preview = bulk_preview(&engine, &ids, &w6r).unwrap();
    assert_eq!((preview.change, preview.resend), (2, 2));

    let (made, dur) = bulk_edit(&engine, &ids, 3, &MovesAck::default(), &w6r);
    assert_eq!(made.unwrap().changed, 2);
    dur.wait(DURABLE_WAIT).unwrap();
    for (was, now) in before.iter().zip(rows(&engine)) {
        assert_eq!(now.station_callsign.as_deref(), Some("W6R"));
        if was.call == "K1ABC" {
            assert_eq!(now, *was, "already W6R: untouched, stamp and all");
        } else {
            assert_eq!(now.upload, Default::default(), "{}: owed again", was.call);
        }
    }
    assert!(
        engine_lock(&engine).take_pending_uploads().is_empty(),
        "nothing queued by the tool"
    );
}

/// ★ A STATION CALL CHANGED IN BULK TAKES OFF THE SERVICES' CONFIRMATIONS, THROUGH THE REAL STORE.
/// The preview counts the LoTW, eQSL and QRZ confirmations it would take off; the edit takes them
/// off with their dates, eQSL's AG mark and LoTW's DXCC, keeps the paper card with its date and
/// route, keeps every credit code, card or none, and queues nothing. The
/// mirror follows. The run's undo puts every contact back exactly, confirmations and all. An
/// unchanged call (case aside) takes nothing off.
#[test]
fn a_bulk_station_call_change_clears_the_services_confirmations_and_undo_restores_them() {
    let d = Dir::new("tools-bulk-unconfirm");
    let engine = shared(
        &d,
        &[
            adif(
                "W1AW",
                "120000",
                "<STATION_CALLSIGN:5>KD0AA<QSL_RCVD:1>Y<QSL_RCVD_VIA:1>B<QSLRDATE:8>20260110\
                 <LOTW_QSL_RCVD:1>Y<LOTW_QSLRDATE:8>20260105<EQSL_QSL_RCVD:1>Y\
                 <EQSL_QSLRDATE:8>20260106<APP_EQSL_AG:1>Y<APP_QRZLOG_STATUS:1>C\
                 <CREDIT_GRANTED:4>DXCC",
            ),
            adif(
                "K1ABC",
                "120100",
                "<STATION_CALLSIGN:5>KD0AA<DXCC:3>291<LOTW_QSL_RCVD:1>Y\
                 <LOTW_QSLRDATE:8>20260105<APP_NEXUS_LOTW_DXCC:1>1<CREDIT_GRANTED:4>DXCC",
            ),
            adif(
                "N0XYZ",
                "120200",
                "<STATION_CALLSIGN:5>kd0aa<LOTW_QSL_RCVD:1>Y<EQSL_QSL_RCVD:1>Y",
            ),
        ],
    );
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();
    let to = |call: &str| {
        [Change {
            field: Field::StationCallsign,
            value: Some(call.into()),
        }]
    };

    let same = bulk_preview(&engine, &ids, &to("KD0AA")).unwrap();
    assert_eq!(
        (same.change, same.unconfirm),
        (0, 0),
        "the same call: nothing"
    );
    let (made, _) = bulk_edit(&engine, &ids, 3, &MovesAck::default(), &to("KD0AA"));
    assert_eq!(made.unwrap().changed, 0);
    assert_eq!(rows(&engine), before, "an unchanged call clears nothing");

    let preview = bulk_preview(&engine, &ids, &to("W6R")).unwrap();
    assert_eq!((preview.change, preview.unconfirm), (3, 6));

    let (made, dur) = bulk_edit(&engine, &ids, 3, &MovesAck::default(), &to("W6R"));
    let made = made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(made.changed, 3);
    let after = rows(&engine);
    let extra =
        |r: &QsoRecord, k: &str| r.extra.iter().find(|(t, _)| t == k).map(|(_, v)| v.clone());
    for r in &after {
        assert_eq!(r.station_callsign.as_deref(), Some("W6R"));
        assert!(
            !r.qsl_rcvd.lotw && !r.qsl_rcvd.eqsl && !r.qsl_rcvd.qrz,
            "{}",
            r.call
        );
        for k in [
            "LOTW_QSLRDATE",
            "EQSL_QSLRDATE",
            "APP_EQSL_AG",
            "APP_NEXUS_LOTW_DXCC",
        ] {
            assert_eq!(extra(r, k), None, "{}: {k}", r.call);
        }
    }
    let (card, lotw, plain) = (&after[0], &after[1], &after[2]);
    assert!(card.qsl_rcvd.card && card.confirmed && card.award_confirmed);
    assert_eq!(extra(card, "QSLRDATE").as_deref(), Some("20260110"));
    assert_eq!(extra(card, "QSL_RCVD_VIA").as_deref(), Some("B"));
    assert_eq!(card.credit_granted, ["DXCC"], "the card stands behind it");
    assert!(!lotw.confirmed && !lotw.award_confirmed);
    assert_eq!(
        lotw.credit_granted,
        ["DXCC"],
        "the award desk's record stays"
    );
    assert_eq!(lotw.dxcc, Some(291), "the log's own DXCC stays");
    assert!(!plain.confirmed);
    assert!(
        engine_lock(&engine).take_pending_uploads().is_empty(),
        "nothing queued by the tool"
    );
    settle(&engine);
    let text = mirror(&d);
    assert!(!text.contains("LOTW_QSL_RCVD") && !text.contains("EQSL_QSL_RCVD"));
    assert!(!text.contains("APP_QRZLOG_STATUS"));
    assert_eq!(text.matches("<QSL_RCVD:1>Y").count(), 1);

    let (undone, dur) = undo_run(&engine, &made.run.unwrap());
    assert_eq!(undone.unwrap().changed, 3);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(rows(&engine), before, "every confirmation back, exactly");
}

/// The check proposes no fix it cannot stand behind: a confirmed contact's DXCC and band — by a
/// card, LoTW or granted credit — are listed with none, and so is a DXCC the call's prefix cannot
/// decide, a grid holding two locators or a 10-character one is not cut, and an end
/// before the start that is no midnight wrap is not cleared.
#[test]
fn the_check_lists_what_only_the_operator_can_settle() {
    let mut r = tempo_core::logbook::parse_adif(&adif(
        "W1AW",
        "120000",
        "<GRIDSQUARE:9>FN31,FN32<DXCC:1>1<QSO_DATE_OFF:8>20251231<TIME_OFF:6>060000",
    ))
    .remove(0);
    r.id = Some("~0000000000000001".parse().unwrap());
    r.band = "40m".into();
    r.freq_mhz = 14.074;
    // Number 1 is fixed to 291; number 110 is a mismatch the prefix cannot decide.
    let mismatch = |_: &str, n: u32| match n {
        1 => Some(Some(291)),
        110 => Some(None),
        _ => None,
    };
    let fixes = |r: &QsoRecord| -> Vec<(ProblemKind, Option<Option<String>>)> {
        problems_of(r, &mismatch, 1_790_000_000)
            .into_iter()
            .map(|p| (p.kind, p.fix.map(|f| f.value)))
            .collect()
    };
    let s = |v: &str| Some(Some(v.to_string()));
    assert_eq!(
        fixes(&r),
        vec![
            (ProblemKind::Grid, None),
            (ProblemKind::Dxcc, s("291")),
            (ProblemKind::BandFreq, s("20m")),
            (ProblemKind::EndBeforeStart, None),
        ]
    );
    r.grid = Some("FN31PR45AB".into());
    r.qsl_rcvd.lotw = true;
    assert_eq!(
        fixes(&r),
        vec![
            (ProblemKind::Grid, None),
            (ProblemKind::Dxcc, None),
            (ProblemKind::BandFreq, None),
            (ProblemKind::EndBeforeStart, None),
        ],
        "LoTW-confirmed: listed, never fixed"
    );
    r.qsl_rcvd.lotw = false;
    r.credit_granted = vec!["DXCC".into()];
    assert_eq!(fixes(&r)[1], (ProblemKind::Dxcc, None), "credit granted");
    r.credit_granted.clear();
    r.qsl_rcvd.card = true;
    assert_eq!(
        fixes(&r)[1..3],
        [(ProblemKind::Dxcc, None), (ProblemKind::BandFreq, None)],
        "a paper card confirms the entity and band"
    );
    r.qsl_rcvd.card = false;
    r.dxcc = Some(110);
    assert_eq!(
        fixes(&r)[1],
        (ProblemKind::Dxcc, None),
        "undecided by the prefix: listed, never fixed"
    );
}

/// ⛔ A run of more than the writer's chunk is refused while the FT sequencer is armed and changes
/// nothing — as a large undo is — while a small one is made; disarmed, the large one is made.
#[test]
fn a_large_run_waits_until_transmit_is_off() {
    let n = tempo_core::logbook::writer::CHUNK_ROWS + 20;
    let d = Dir::new("tools-large-tx");
    let records: Vec<String> = (0..n)
        .map(|i| {
            adif(
                &format!("K{i}ABC"),
                &format!("12{:02}{:02}", i / 60 % 60, i % 60),
                "",
            )
        })
        .collect();
    let engine = shared(&d, &records);
    let ids: Vec<RecordId> = rows(&engine).iter().map(|r| r.id.unwrap()).collect();
    let changes = [Change {
        field: Field::MyRig,
        value: Some("IC-7300".into()),
    }];
    engine_lock(&engine).set_tx_enabled(true);
    assert!(
        engine_lock(&engine).transmit_needs_the_lock(),
        "premise: armed for FT"
    );

    let (refused, _) = bulk_edit(&engine, &ids, ids.len(), &MovesAck::default(), &changes);
    assert_eq!(refused, Err(MUST_WAIT_FOR_TX.to_string()));
    assert!(
        rows(&engine).iter().all(|r| r.my_rig.is_none()),
        "nothing was made"
    );
    let (small, d1) = bulk_edit(&engine, &ids[..3], 3, &MovesAck::default(), &changes);
    assert_eq!(small.unwrap().changed, 3, "a small run is made");
    d1.wait(DURABLE_WAIT).unwrap();

    engine_lock(&engine).set_tx_enabled(false);
    let (made, dur) = bulk_edit(&engine, &ids, ids.len(), &MovesAck::default(), &changes);
    assert_eq!(made.unwrap().changed, n - 3);
    dur.wait(DURABLE_WAIT).unwrap();
    assert!(rows(&engine)
        .iter()
        .all(|r| r.my_rig.as_deref() == Some("IC-7300")));
}

/// ★ ONE RUN ACROSS PARTS. A bulk edit of more contacts than one change carries ([`CHUNK`]) is
/// made in several changes, and they are one run: its undo puts every contact back, the ones of
/// the first change and of the last alike.
#[test]
fn a_run_made_in_several_changes_is_undone_whole() {
    let n = CHUNK + 6;
    let d = Dir::new("tools-parts");
    let records: Vec<String> = (0..n)
        .map(|i| {
            adif(
                &format!("K{i}ABC"),
                &format!("{:02}{:02}{:02}", i / 3600, i / 60 % 60, i % 60),
                "",
            )
        })
        .collect();
    let engine = shared(&d, &records);
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();
    let (made, dur) = bulk_edit(
        &engine,
        &ids,
        ids.len(),
        &MovesAck::default(),
        &[Change {
            field: Field::StationCallsign,
            value: Some("K2DEF".into()),
        }],
    );
    let made = made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(made.changed, n);
    assert!(rows(&engine)
        .iter()
        .all(|r| r.station_callsign.as_deref() == Some("K2DEF")));
    let (undone, dur) = undo_run(&engine, &made.run.unwrap());
    assert_eq!(undone.unwrap().changed, n, "both changes' contacts");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(rows(&engine), before);
}

/// The bench: at 150,000 contacts, the duplicate scan's time, and the longest hold of the Engine
/// lock one chunk of a bulk edit of every contact takes. Run in release:
/// `cargo test -p tempo-app --release --lib logtools -- --ignored --nocapture`.
#[test]
#[ignore = "the 150k bench: run in release"]
fn bench_150k() {
    let n = 150_000;
    let d = Dir::new("tools-bench");
    let mut s = adif_header();
    for i in 0..n {
        let call = format!("K{}A{}", i % 10, i / 10);
        let (day, sec) = (1 + i / 86_400 % 28, i % 86_400);
        s.push_str(&format!(
            "<CALL:{}>{call}<BAND:3>20m<FREQ:6>14.074<MODE:3>FT8<QSO_DATE:8>202601{day:02}\
             <TIME_ON:6>{:02}{:02}{:02}<EOR>\n",
            call.len(),
            sec / 3600,
            sec / 60 % 60,
            sec % 60,
        ));
        // Every hundredth contact logged twice, a minute later.
        if i % 100 == 0 {
            let sec = sec.min(86_340) + 60;
            s.push_str(&format!(
                "<CALL:{}>{call}<BAND:3>20m<FREQ:6>14.074<MODE:3>FT8<QSO_DATE:8>202601{day:02}\
                 <TIME_ON:6>{:02}{:02}{:02}<EOR>\n",
                call.len(),
                sec / 3600,
                sec / 60 % 60,
                sec % 60,
            ));
        }
    }
    std::fs::write(d.log(), s).unwrap();
    let e = engine_on_store(&d);
    flush(&e);
    let engine = Arc::new(Mutex::new(e));

    let t = Instant::now();
    let scan = find_duplicates(&engine, DupeTier::Rule).unwrap();
    let scan_time = t.elapsed();
    assert_eq!(scan.scanned, n + n / 100);
    assert_eq!(scan.groups.len(), n / 100);
    let t = Instant::now();
    let possible = find_duplicates(&engine, DupeTier::Possible).unwrap();
    let possible_time = t.elapsed();

    let ids: Vec<RecordId> = rows(&engine).iter().map(|r| r.id.unwrap()).collect();
    let t = Instant::now();
    let (made, dur) = bulk_edit(
        &engine,
        &ids,
        ids.len(),
        &MovesAck::default(),
        &[Change {
            field: Field::MyRig,
            value: Some("IC-7300".into()),
        }],
    );
    let made = made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    let edit_time = t.elapsed();

    let asks: Vec<MergeAsk> = scan.groups.iter().map(|g| ask(g, &[])).collect();
    let fresh = find_duplicates(&engine, DupeTier::Rule).unwrap();
    let asks_now: Vec<MergeAsk> = fresh.groups.iter().map(|g| ask(g, &[])).collect();
    assert_eq!(asks.len(), asks_now.len());
    let t = Instant::now();
    let (merged, dur) = merge_duplicates(&engine, &asks_now);
    let merged = merged.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    let merge_time = t.elapsed();

    println!(
        "150k bench: {} contacts; rule scan {:?} ({} groups); possible scan {:?} ({} groups); \
         bulk edit of all {:?}, longest lock hold {:?}; merge of {} groups {:?}, longest lock \
         hold {:?}",
        scan.scanned,
        scan_time,
        scan.groups.len(),
        possible_time,
        possible.groups.len(),
        edit_time,
        made.longest_hold,
        asks_now.len(),
        merge_time,
        merged.longest_hold,
    );
    assert_eq!(made.changed, ids.len());
    // Each pair's copies differ only in their time, so the kept one already holds the union:
    // the merge deletes the other and changes nothing else.
    assert_eq!(merged.changed, asks_now.len());
    assert!(scan_time < Duration::from_secs(10), "{scan_time:?}");
    assert!(
        made.longest_hold < Duration::from_millis(50),
        "{:?}",
        made.longest_hold
    );
}

/// The call in Settings set to `call`, as the operator sets it.
/// The operator's tick on a preview's `moves` contacts sent under `to`.
fn ack(moves: usize, to: &str) -> MovesAck {
    MovesAck {
        moves,
        move_to: Some(to.into()),
    }
}

fn settings_call_is(engine: &Mutex<Engine>, call: &str) {
    let mut e = engine_lock(engine);
    let mut s = e.settings().clone();
    s.mycall = call.into();
    e.apply_settings(s);
}

/// ★ FILLING IN THE STATION CALL A CONTACT WAS ALREADY UPLOADED UNDER. A contact naming no station
/// call is uploaded and confirmed under the call in Settings; a bulk edit writing that call into
/// it changes the field and nothing else: the preview counts nothing to resend and nothing to
/// take off, and every stamp and confirmation stays, through the real store.
#[test]
fn filling_in_the_settings_call_keeps_every_stamp_and_confirmation() {
    let d = Dir::new("tools-bulk-fill-own");
    let engine = shared(
        &d,
        &[adif(
            "W1AW",
            "120000",
            "<LOTW_QSL_RCVD:1>Y<LOTW_QSLRDATE:8>20260105<EQSL_QSL_RCVD:1>Y<CREDIT_GRANTED:4>DXCC",
        )],
    );
    settings_call_is(&engine, "KD9TAW");
    let w = rows(&engine);
    stamp(&engine, &w[0], UploadService::Lotw, 100);
    stamp(&engine, &w[0], UploadService::Qrz, 200);
    engine_lock(&engine).take_pending_uploads();
    let before = rows(&engine);
    let ids = [before[0].id.unwrap()];
    let fill = [Change {
        field: Field::StationCallsign,
        value: Some("kd9taw".into()),
    }];
    let preview = bulk_preview(&engine, &ids, &fill).unwrap();
    assert_eq!(
        (preview.change, preview.resend, preview.unconfirm),
        (1, 0, 0)
    );
    let (made, dur) = bulk_edit(&engine, &ids, 1, &MovesAck::default(), &fill);
    assert_eq!(made.unwrap().changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    let mut want = before[0].clone();
    want.station_callsign = Some("KD9TAW".into());
    assert_eq!(rows(&engine), vec![want], "the field, and nothing else");
}

/// ★ UNDOING A STATION CALL CHANGE AFTER AN UPLOAD UNDER THE NEW CALL. The change clears the
/// stamps; an upload under the new call stamps the contact again; the undo puts the old call back.
/// Those stamps are of an upload under the call the contact no longer carries, so the undo clears
/// them (and `LOTW_QSL_SENT`), and the next upload sends it under the old call again: the service
/// already holding that QSO answers it as a duplicate. The stamps the change cleared are not put
/// back — a stamp is never history — so nothing is claimed sent that the log cannot show. Every
/// other field, confirmations included, is back exactly. An undo that leaves the contact under the
/// call it was uploaded under (a blank call filled with the Settings call) keeps its stamps.
#[test]
fn undoing_a_station_call_change_never_keeps_the_new_calls_stamps() {
    let d = Dir::new("tools-bulk-undo-stamps");
    let engine = shared(
        &d,
        &[
            adif(
                "W1AW",
                "120000",
                "<STATION_CALLSIGN:5>KD0AA<LOTW_QSL_RCVD:1>Y<LOTW_QSL_SENT:1>Y",
            ),
            adif("K1ABC", "120100", ""),
        ],
    );
    settings_call_is(&engine, "KD9TAW");
    for r in rows(&engine) {
        stamp(&engine, &r, UploadService::Lotw, 100);
    }
    engine_lock(&engine).take_pending_uploads();
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();
    let to = |call: &str| {
        [Change {
            field: Field::StationCallsign,
            value: Some(call.into()),
        }]
    };

    let (made, dur) = bulk_edit(&engine, &ids[..1], 1, &MovesAck::default(), &to("W6R"));
    let run = made.unwrap().run.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    let moved = rows(&engine);
    assert_eq!(moved[0].upload, Default::default());
    // Uploaded under W6R.
    stamp(&engine, &moved[0], UploadService::Lotw, 300);
    stamp(&engine, &moved[0], UploadService::Qrz, 301);
    assert!(rows(&engine)[0].upload.lotw.is_some());

    let (undone, dur) = undo_run(&engine, &run);
    assert_eq!(undone.unwrap().changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    let now = rows(&engine);
    let mut want = before[0].clone();
    want.upload = Default::default();
    want.extra.retain(|(k, _)| k != "LOTW_QSL_SENT");
    assert_eq!(now[0], want, "back under KD0AA, owed to every service");
    settle(&engine);
    let reread = tempo_core::logbook::parse_adif(&mirror(&d));
    let w1aw = reread.iter().find(|r| r.call == "W1AW").unwrap();
    assert_eq!(w1aw.upload, Default::default(), "the mirror agrees");

    // A blank call filled with the Settings call, uploaded, then undone: the same call all along.
    let (made, dur) = bulk_edit(&engine, &ids[1..], 1, &MovesAck::default(), &to("KD9TAW"));
    let run = made.unwrap().run.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    stamp(&engine, &rows(&engine)[1], UploadService::Qrz, 400);
    let stamped = rows(&engine)[1].upload.clone();
    let (undone, dur) = undo_run(&engine, &run);
    assert_eq!(undone.unwrap().changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    let k1abc = rows(&engine).remove(1);
    assert_eq!(k1abc.station_callsign, None);
    assert_eq!(
        k1abc.upload, stamped,
        "uploaded under the call it is still under"
    );
}

/// ★ AN UPLOAD IN FLIGHT WHILE THE STATION CALL CHANGES. A QRZ, eQSL or Club Log push sent under
/// the old call answers after a bulk edit gave the contact another: its stamp is not made, so the
/// contact stays owed under the call it now carries — by id or by the newest-match fallback. A
/// push of a contact naming no call, answered after the Settings call was filled in, was sent
/// under that call and is stamped.
#[test]
fn an_upload_answered_after_the_station_call_changed_is_not_stamped() {
    let d = Dir::new("tools-bulk-inflight");
    let engine = shared(
        &d,
        &[
            adif("W1AW", "120000", "<STATION_CALLSIGN:5>KD0AA"),
            adif("K1ABC", "120100", ""),
        ],
    );
    settings_call_is(&engine, "KD9TAW");
    let pushed = rows(&engine);
    let ids: Vec<RecordId> = pushed.iter().map(|r| r.id.unwrap()).collect();
    for (id, call) in [(ids[0], "W6R"), (ids[1], "KD9TAW")] {
        let (made, dur) = bulk_edit(
            &engine,
            &[id],
            1,
            &MovesAck::default(),
            &[Change {
                field: Field::StationCallsign,
                value: Some(call.into()),
            }],
        );
        made.unwrap();
        dur.wait(DURABLE_WAIT).unwrap();
    }
    for service in [
        UploadService::Qrz,
        UploadService::Eqsl,
        UploadService::Clublog,
    ] {
        let mut unnamed = pushed[0].clone();
        unnamed.id = None;
        for p in [&pushed[0], &unnamed] {
            let (made, _) = logwrite::stamp_push(
                &engine,
                p,
                service,
                UploadStatus {
                    outcome: UploadOutcome::Accepted,
                    when_unix: 500,
                    detail: None,
                },
            );
            assert!(!made, "{service:?}: sent under KD0AA, now W6R");
        }
    }
    assert_eq!(rows(&engine)[0].upload, Default::default());
    stamp(&engine, &pushed[1], UploadService::Qrz, 600);
    assert!(
        rows(&engine)[1].upload.qrz.is_some(),
        "the same call all along"
    );
}

/// One change setting STATION_CALLSIGN to `call`, or clearing it.
fn station_to(call: Option<&str>) -> [Change; 1] {
    [Change {
        field: Field::StationCallsign,
        value: call.map(str::to_string),
    }]
}

/// The contacts the default LoTW batch would send now.
fn owed_to_lotw(engine: &Mutex<Engine>) -> Vec<RecordId> {
    let rows = engine_lock(engine).log_rows();
    station::lotw_unsent_ids(&rows).unwrap()
}

/// ★ FILLING IN A BLANK STATION CALL IS NEVER A CHANGE OF CALL, whatever the call in Settings is
/// now. The contact was uploaded and confirmed under whatever call it was; the fill records it.
/// Old contacts, uploaded under KA1OLD while it was the Settings call, filled with KA1OLD after
/// Settings moved to KD9NEW: the preview counts nothing to resend or take off, and every stamp and
/// confirmation stays. Clearing a station call is no change of call either. Replacing one call
/// with another still is: its stamps and confirmations go.
#[test]
fn filling_a_blank_station_call_clears_nothing_whatever_the_settings_call() {
    let d = Dir::new("tools-bulk-fill-old");
    let engine = shared(
        &d,
        &[
            adif("W1AW", "120000", "<LOTW_QSL_RCVD:1>Y<EQSL_QSL_RCVD:1>Y"),
            adif(
                "K1ABC",
                "120100",
                "<STATION_CALLSIGN:5>KD0AA<LOTW_QSL_RCVD:1>Y",
            ),
            adif(
                "N0XYZ",
                "120200",
                "<STATION_CALLSIGN:5>KD0AA<LOTW_QSL_RCVD:1>Y",
            ),
        ],
    );
    settings_call_is(&engine, "KA1OLD");
    for r in rows(&engine) {
        stamp(&engine, &r, UploadService::Lotw, 100);
    }
    settings_call_is(&engine, "KD9NEW");
    engine_lock(&engine).take_pending_uploads();
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();

    let fill = station_to(Some("ka1old"));
    let preview = bulk_preview(&engine, &ids[..1], &fill).unwrap();
    assert_eq!(
        (preview.change, preview.resend, preview.unconfirm),
        (1, 0, 0)
    );
    let (made, dur) = bulk_edit(&engine, &ids[..1], 1, &ack(1, "KA1OLD"), &fill);
    assert_eq!(made.unwrap().changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    let mut want = before[0].clone();
    want.station_callsign = Some("KA1OLD".into());
    assert_eq!(rows(&engine)[0], want, "the field, and nothing else");

    let clear = station_to(None);
    let preview = bulk_preview(&engine, &ids[1..2], &clear).unwrap();
    assert_eq!(
        (preview.change, preview.resend, preview.unconfirm),
        (1, 0, 0)
    );
    let (made, dur) = bulk_edit(&engine, &ids[1..2], 1, &ack(1, "KD9NEW"), &clear);
    assert_eq!(made.unwrap().changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    let mut want = before[1].clone();
    want.station_callsign = None;
    assert_eq!(
        rows(&engine)[1],
        want,
        "cleared: the field, and nothing else"
    );

    // CONTROL: one call replaced by another is a change.
    let other = station_to(Some("W6R"));
    let preview = bulk_preview(&engine, &ids[2..], &other).unwrap();
    assert_eq!(
        (
            preview.change,
            preview.recall,
            preview.resend,
            preview.unconfirm
        ),
        (1, 1, 1, 1)
    );
    let (made, dur) = bulk_edit(&engine, &ids[2..], 1, &MovesAck::default(), &other);
    assert_eq!(made.unwrap().changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    let n0xyz = rows(&engine).remove(2);
    assert_eq!(n0xyz.upload, Default::default());
    assert!(!n0xyz.qsl_rcvd.lotw);
    assert!(engine_lock(&engine).take_pending_uploads().is_empty());
}

/// ★ AN UNDO NEVER PUTS A CONTACT UNDER A CALL IT WAS NOT MADE UNDER. A contact naming no station
/// call goes out under the call in Settings NOW. Old contacts uploaded under KA1OLD, filled with
/// KA1OLD — after Settings moved to KD9NEW, or before it did — and the fill undone: putting the
/// blank back would owe them to every service under KD9NEW, and the next LoTW batch would sign
/// them as KD9NEW's. The undo keeps KA1OLD in the field, and its stamps, so nothing is owed. With
/// the Settings call still the filled one, the blank goes back and moves nothing.
#[test]
fn an_undo_never_puts_a_contact_under_a_call_it_was_not_made_under() {
    let d = Dir::new("tools-bulk-undo-blank");
    let engine = shared(
        &d,
        &[
            adif("W1AW", "120000", "<LOTW_QSL_RCVD:1>Y"),
            adif("K1ABC", "120100", "<LOTW_QSL_RCVD:1>Y"),
        ],
    );
    settings_call_is(&engine, "KA1OLD");
    for r in rows(&engine) {
        stamp(&engine, &r, UploadService::Lotw, 100);
        stamp(&engine, &r, UploadService::Qrz, 101);
    }
    engine_lock(&engine).take_pending_uploads();
    let ids: Vec<RecordId> = rows(&engine).iter().map(|r| r.id.unwrap()).collect();
    let fill = station_to(Some("KA1OLD"));

    // K1ABC filled while KA1OLD is still the Settings call.
    let (made, dur) = bulk_edit(&engine, &ids[1..], 1, &MovesAck::default(), &fill);
    let early = made.unwrap().run.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    // The new call.
    settings_call_is(&engine, "KD9NEW");
    // W1AW filled after it.
    let (made, dur) = bulk_edit(&engine, &ids[..1], 1, &ack(1, "KA1OLD"), &fill);
    let late = made.unwrap().run.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    let filled = rows(&engine);
    assert!(filled.iter().all(|r| r.upload.lotw.is_some()));

    for (run, id) in [(&late, ids[0]), (&early, ids[1])] {
        let (undone, dur) = undo_run(&engine, run);
        let undone = undone.unwrap();
        dur.wait(DURABLE_WAIT).unwrap();
        assert_eq!(undone.held, vec![(id, "STATION_CALLSIGN".to_string())]);
        assert_eq!(
            undone.held_under.as_deref(),
            Some("KD9NEW"),
            "the call a blank would go under"
        );
        assert_eq!(undone.changed, 0, "nothing else to put back");
    }
    assert_eq!(rows(&engine), filled, "under KA1OLD, stamps and all");
    assert!(
        owed_to_lotw(&engine).is_empty(),
        "nothing owed under KD9NEW"
    );
    settle(&engine);
    let reread = tempo_core::logbook::parse_adif(&mirror(&d));
    assert!(reread
        .iter()
        .all(|r| r.station_callsign.as_deref() == Some("KA1OLD") && r.upload.lotw.is_some()));

    // No call in Settings: still held, and no call named for it.
    settings_call_is(&engine, " ");
    let (undone, dur) = undo_run(&engine, &late);
    let undone = undone.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(undone.held, vec![(ids[0], "STATION_CALLSIGN".to_string())]);
    assert_eq!(undone.held_under, None, "no call in Settings");

    // CONTROL: the Settings call is the filled one again — the blank goes back, nothing moves.
    settings_call_is(&engine, "KA1OLD");
    let (undone, dur) = undo_run(&engine, &early);
    assert_eq!(undone.unwrap().changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    let k1abc = rows(&engine).remove(1);
    assert_eq!(k1abc.station_callsign, None);
    assert_eq!(k1abc.upload, filled[1].upload);
}

/// ★ A PORTABLE /P IS THE SAME STATION CALL for the clean-up tools: KD9TAW/P written over KD9TAW
/// (or back) changes the field and nothing else. Only /P: KD9TAW/M is another call, and its stamps
/// and confirmations go.
#[test]
fn a_portable_p_is_the_same_station_call() {
    let d = Dir::new("tools-bulk-portable");
    let engine = shared(
        &d,
        &[
            adif(
                "W1AW",
                "120000",
                "<STATION_CALLSIGN:6>KD9TAW<LOTW_QSL_RCVD:1>Y",
            ),
            adif(
                "K1ABC",
                "120100",
                "<STATION_CALLSIGN:8>KD9TAW/P<LOTW_QSL_RCVD:1>Y",
            ),
            adif(
                "N0XYZ",
                "120200",
                "<STATION_CALLSIGN:6>KD9TAW<LOTW_QSL_RCVD:1>Y",
            ),
        ],
    );
    for r in rows(&engine) {
        stamp(&engine, &r, UploadService::Lotw, 100);
    }
    engine_lock(&engine).take_pending_uploads();
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();

    for (id, to) in [(ids[0], "kd9taw/p"), (ids[1], "KD9TAW")] {
        let preview = bulk_preview(&engine, &[id], &station_to(Some(to))).unwrap();
        assert_eq!(
            (
                preview.change,
                preview.recall,
                preview.resend,
                preview.unconfirm
            ),
            (1, 0, 0, 0),
            "{to}"
        );
        let (made, dur) = bulk_edit(
            &engine,
            &[id],
            1,
            &MovesAck::default(),
            &station_to(Some(to)),
        );
        assert_eq!(made.unwrap().changed, 1);
        dur.wait(DURABLE_WAIT).unwrap();
    }
    let now = rows(&engine);
    for (i, call) in [(0, "KD9TAW/P"), (1, "KD9TAW")] {
        let mut want = before[i].clone();
        want.station_callsign = Some(call.into());
        assert_eq!(now[i], want, "{call}: the field, and nothing else");
    }

    // CONTROL: /M is another call.
    let preview = bulk_preview(&engine, &ids[2..], &station_to(Some("KD9TAW/M"))).unwrap();
    assert_eq!(
        (
            preview.change,
            preview.recall,
            preview.resend,
            preview.unconfirm
        ),
        (1, 1, 1, 1)
    );
}

/// A push naming no row, of a contact unchanged since, under a station call other than the
/// Settings call, is stamped on it: the newest-match fallback reads the station call with the row.
#[test]
fn an_unnamed_push_under_another_station_call_is_stamped() {
    let d = Dir::new("tools-push-unnamed-station");
    let engine = shared(&d, &[adif("W1AW", "120000", "<STATION_CALLSIGN:3>W6R")]);
    settings_call_is(&engine, "KD9TAW");
    let mut unnamed = rows(&engine).remove(0);
    unnamed.id = None;
    let (made, dur) = logwrite::stamp_push(
        &engine,
        &unnamed,
        UploadService::Qrz,
        UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 700,
            detail: None,
        },
    );
    assert!(made, "the contact pushed, under W6R");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(
        rows(&engine)[0].upload.qrz.as_ref().map(|s| s.when_unix),
        Some(700)
    );
}

/// ★ A FILL OR A CLEAR THAT MOVES A CONTACT TO ANOTHER UPLOAD CALL IS COUNTED. Clearing KA1OLD
/// while Settings says KD9NEW clears no stamp, but every service not yet sent the contact gets it
/// as KD9NEW: the preview counts it, with both calls. Filling a blank with KA1OLD moves it the other
/// way. Filling it with the Settings call, or a contact sent everywhere, moves nothing. The edit
/// itself still clears nothing.
#[test]
fn a_fill_or_a_clear_that_moves_the_upload_call_is_counted() {
    let d = Dir::new("tools-bulk-moves");
    let engine = shared(
        &d,
        &[
            adif(
                "W1AW",
                "120000",
                "<STATION_CALLSIGN:6>KA1OLD<LOTW_QSL_RCVD:1>Y",
            ),
            adif("K1ABC", "120100", ""),
            adif("N0XYZ", "120200", "<STATION_CALLSIGN:6>KA1OLD"),
        ],
    );
    settings_call_is(&engine, "KD9NEW");
    let w = rows(&engine);
    stamp(&engine, &w[0], UploadService::Lotw, 100);
    for service in [
        UploadService::Lotw,
        UploadService::Eqsl,
        UploadService::Qrz,
        UploadService::Clublog,
    ] {
        stamp(&engine, &w[2], service, 100);
    }
    engine_lock(&engine).take_pending_uploads();
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();
    let counted = |p: &BulkPreview| (p.change, p.recall, p.resend, p.unconfirm, p.moves);

    let clear = station_to(None);
    let preview = bulk_preview(&engine, &ids[..1], &clear).unwrap();
    assert_eq!(counted(&preview), (1, 0, 0, 0, 1));
    assert_eq!(preview.move_to.as_deref(), Some("KD9NEW"));
    assert_eq!(preview.move_from, vec!["KA1OLD".to_string()]);
    let (made, dur) = bulk_edit(&engine, &ids[..1], 1, &ack(1, "KD9NEW"), &clear);
    assert_eq!(made.unwrap().changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    let mut want = before[0].clone();
    want.station_callsign = None;
    assert_eq!(rows(&engine)[0], want, "stamps and confirmation stay");

    let preview = bulk_preview(&engine, &ids[1..2], &station_to(Some("ka1old"))).unwrap();
    assert_eq!(counted(&preview), (1, 0, 0, 0, 1));
    assert_eq!(preview.move_to.as_deref(), Some("KA1OLD"));
    assert_eq!(preview.move_from, vec!["KD9NEW".to_string()]);

    let preview = bulk_preview(&engine, &ids[1..2], &station_to(Some("KD9NEW"))).unwrap();
    assert_eq!(counted(&preview), (1, 0, 0, 0, 0), "the Settings call");
    assert_eq!((preview.move_to, preview.move_from), (None, vec![]));

    let preview = bulk_preview(&engine, &ids[2..], &clear).unwrap();
    assert_eq!(counted(&preview), (1, 0, 0, 0, 0), "sent everywhere");

    // Named as the upload sends it: KD9TAW/P, which LoTW holds apart from KD9TAW.
    let preview = bulk_preview(&engine, &ids[1..2], &station_to(Some("kd9taw/p"))).unwrap();
    assert_eq!(counted(&preview), (1, 0, 0, 0, 1));
    assert_eq!(preview.move_to.as_deref(), Some("KD9TAW/P"));
    assert_eq!(preview.move_from, vec!["KD9NEW".to_string()]);
    settings_call_is(&engine, "KD9NEW/P");
    let preview = bulk_preview(&engine, &ids[1..2], &station_to(Some("KA1OLD"))).unwrap();
    assert_eq!(preview.move_from, vec!["KD9NEW/P".to_string()]);
}

/// ★ A BULK EDIT NEVER SENDS CONTACTS UNDER ANOTHER CALL THE OPERATOR DID NOT TICK FOR. The station
/// reads the moves again at the apply: with none acknowledged, with another count, or with another
/// call — the call in Settings changed between the preview and the confirm — it refuses and
/// changes nothing. As acknowledged, it is made.
#[test]
fn a_bulk_edit_refuses_moves_the_operator_did_not_acknowledge() {
    let d = Dir::new("tools-bulk-moves-ack");
    let engine = shared(
        &d,
        &[adif(
            "W1AW",
            "120000",
            "<STATION_CALLSIGN:6>KA1OLD<LOTW_QSL_RCVD:1>Y",
        )],
    );
    settings_call_is(&engine, "KA1OLD");
    let before = rows(&engine);
    let ids = [before[0].id.unwrap()];
    let clear = station_to(None);
    // The preview: KA1OLD is the Settings call, nothing moves.
    let preview = bulk_preview(&engine, &ids, &clear).unwrap();
    assert_eq!((preview.moves, preview.move_to.as_deref()), (0, None));
    // Settings changes before the confirm: the clear would now send it under KD9NEW.
    settings_call_is(&engine, "KD9NEW");
    for (acknowledged, what) in [
        (MovesAck::default(), "none acknowledged"),
        (ack(2, "KD9NEW"), "another count"),
        (ack(1, "KA1OLD"), "another call"),
    ] {
        let (made, _) = bulk_edit(&engine, &ids, 1, &acknowledged, &clear);
        assert_eq!(made.unwrap_err(), MOVES_NOT_AS_ACKNOWLEDGED, "{what}");
        assert_eq!(rows(&engine), before, "{what}: nothing changed");
    }
    let (made, dur) = bulk_edit(&engine, &ids, 1, &ack(1, "KD9NEW"), &clear);
    assert_eq!(made.unwrap().changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(rows(&engine)[0].station_callsign, None);
}

/// An upload answered after a bulk edit added /P to the station call it was sent under is stamped:
/// a portable /P is the same station call, as the edit treated it. Another suffix is another call.
#[test]
fn an_answer_after_a_portable_p_edit_is_stamped() {
    let d = Dir::new("tools-push-portable");
    let engine = shared(
        &d,
        &[
            adif("W1AW", "120000", "<STATION_CALLSIGN:6>KD9TAW"),
            adif("K1ABC", "120100", "<STATION_CALLSIGN:6>KD9TAW"),
        ],
    );
    let pushed = rows(&engine);
    for (r, to) in [(&pushed[0], "KD9TAW/P"), (&pushed[1], "KD9TAW/M")] {
        let (made, dur) = bulk_edit(
            &engine,
            &[r.id.unwrap()],
            1,
            &MovesAck::default(),
            &station_to(Some(to)),
        );
        made.unwrap();
        dur.wait(DURABLE_WAIT).unwrap();
    }
    for p in &pushed {
        let (_, dur) = logwrite::stamp_push(
            &engine,
            p,
            UploadService::Qrz,
            UploadStatus {
                outcome: UploadOutcome::Accepted,
                when_unix: 800,
                detail: None,
            },
        );
        dur.wait(DURABLE_WAIT).unwrap();
    }
    let now = rows(&engine);
    assert_eq!(
        now[0].upload.qrz.as_ref().map(|s| s.when_unix),
        Some(800),
        "KD9TAW/P is KD9TAW"
    );
    assert_eq!(now[1].upload.qrz, None, "KD9TAW/M is another call");
}

/// ★ A MERGE OF COPIES A PORTABLE /P APART SENDS NOTHING AGAIN. The copy sent to LoTW under
/// KD9TAW/P is deleted and the unsent KD9TAW copy kept: its stamp comes along, so no second LoTW
/// QSO is signed under KD9TAW.
#[test]
fn a_merge_of_copies_a_portable_p_apart_sends_nothing_again() {
    let d = Dir::new("tools-merge-portable");
    let engine = shared(
        &d,
        &[
            adif(
                "W1AW",
                "120000",
                "<STATION_CALLSIGN:6>KD9TAW<NAME:5>Hiram<QTH:9>Newington",
            ),
            adif("W1AW", "120100", "<STATION_CALLSIGN:8>KD9TAW/P"),
        ],
    );
    let w = rows(&engine);
    stamp(&engine, &w[1], UploadService::Lotw, 100);
    engine_lock(&engine).take_pending_uploads();
    assert_eq!(owed_to_lotw(&engine), vec![w[0].id.unwrap()]);
    let scan = find_duplicates(&engine, DupeTier::Rule).unwrap();
    let g = &scan.groups[0];
    assert_eq!(g.keep, 0, "the fuller, unsent copy");
    let (made, dur) = merge_duplicates(&engine, &[ask(g, &[("STATION_CALLSIGN", 0)])]);
    assert!(made.unwrap().refused.is_empty());
    dur.wait(DURABLE_WAIT).unwrap();
    let kept = rows(&engine);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].station_callsign.as_deref(), Some("KD9TAW"));
    assert_eq!(kept[0].upload.lotw.as_ref().map(|s| s.when_unix), Some(100));
    assert!(owed_to_lotw(&engine).is_empty(), "nothing owed to LoTW");
}

// ── update from the callbook ───────────────────────────────────────────────────────────────────

/// W1AW's callbook answer: its own record, with a name, a place and zones.
fn qrz_answer(call: &str) -> CallbookAnswer {
    CallbookAnswer {
        call: call.into(),
        name: Some("Hiram".into()),
        qth: Some("Newington".into()),
        grid: Some("FN31pr".into()),
        state: Some("CT".into()),
        county: Some("Hartford".into()),
        cq_zone: Some(5),
        itu_zone: Some(8),
        ..CallbookAnswer::default()
    }
}

/// 2 January 2026, 00:00 UTC: a day after the `adif` contacts, well inside the 12 months the
/// update gives a contact the station's location.
const JAN_2026: u64 = 1_767_225_600 + 86_400;

fn answers(calls: &[&str]) -> HashMap<String, CallbookAnswer> {
    calls
        .iter()
        .map(|c| (c.to_string(), qrz_answer(c)))
        .collect()
}

/// [`callbook_fill`] that never waits for the radio, answered as the other tools answer.
fn fill(
    engine: &Mutex<Engine>,
    ids: &[RecordId],
    expect: usize,
    answers: &HashMap<String, CallbookAnswer>,
    now_unix: u64,
) -> (Result<ToolRun, String>, Durability) {
    let (made, why, dur) = callbook_fill(engine, ids, expect, answers, now_unix, &mut || false);
    (why.map_or(Ok(made), Err), dur)
}

/// The fields a fill could touch, as the stored contact holds them.
fn callbook_fields(r: &QsoRecord) -> Vec<(&'static str, Option<String>)> {
    let extra = |t: &str| r.extra.iter().find(|(k, _)| k == t).map(|(_, v)| v.clone());
    vec![
        ("NAME", r.name.clone()),
        ("QTH", r.qth.clone()),
        ("GRIDSQUARE", r.grid.clone()),
        ("STATE", r.state.clone()),
        ("CNTY", extra("CNTY")),
        ("CQZ", extra("CQZ")),
        ("ITUZ", extra("ITUZ")),
    ]
}

/// ★ THE UPDATE, END TO END. Only EMPTY fields are filled from the answer — a field the contact
/// holds keeps its value, however the callbook disagrees; the call, the country, the DXCC number,
/// every upload stamp and every confirmation stay as they were; nothing is queued to any service;
/// a contact whose call has no answer is not touched. The whole change is ONE run, and a single
/// undo of it puts every contact back exactly.
#[test]
fn an_update_fills_only_empty_fields_keeps_stamps_and_undoes_whole() {
    let d = Dir::new("tools-callbook");
    let engine = shared(
        &d,
        &[
            // Empty: every field fills.
            adif("W1AW", "120000", "<COUNTRY:13>United States<DXCC:3>291"),
            // Holds a name, a grid and a zone of its own, and is confirmed and uploaded.
            adif(
                "W1AW",
                "130000",
                "<COUNTRY:13>United States<DXCC:3>291<NAME:3>Bob<GRIDSQUARE:4>EM10\
                 <CQZ:1>4<LOTW_QSL_RCVD:1>Y<QSL_RCVD:1>Y",
            ),
            // A call the callbook did not know: no answer, left alone.
            adif("Q9QQQ", "140000", ""),
        ],
    );
    let w = by_call(&engine, "W1AW");
    stamp(&engine, &w[1], UploadService::Qrz, 300);
    stamp(&engine, &w[1], UploadService::Lotw, 400);
    engine_lock(&engine).take_pending_uploads();
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();

    let (made, dur) = fill(&engine, &ids, ids.len(), &answers(&["W1AW"]), JAN_2026);
    let made = made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(made.changed, 2, "both W1AW contacts gained a field");
    let after = rows(&engine);

    let empty = &after[0];
    assert_eq!(
        callbook_fields(empty),
        vec![
            ("NAME", Some("Hiram".into())),
            ("QTH", Some("Newington".into())),
            ("GRIDSQUARE", Some("FN31pr".into())),
            ("STATE", Some("CT".into())),
            ("CNTY", Some("CT,Hartford".into())),
            ("CQZ", Some("5".into())),
            ("ITUZ", Some("8".into())),
        ]
    );
    let held = &after[1];
    assert_eq!(
        callbook_fields(held),
        vec![
            ("NAME", Some("Bob".into())),
            ("QTH", Some("Newington".into())),
            ("GRIDSQUARE", Some("EM10".into())),
            ("STATE", Some("CT".into())),
            ("CNTY", Some("CT,Hartford".into())),
            ("CQZ", Some("4".into())),
            ("ITUZ", Some("8".into())),
        ],
        "a held field keeps its value; only the empty ones fill"
    );
    for (was, now) in before.iter().zip(&after) {
        assert_eq!(
            (&now.call, &now.country, now.dxcc),
            (&was.call, &was.country, was.dxcc),
            "never the call, the country or the DXCC number"
        );
        assert_eq!(
            now.upload, was.upload,
            "{}: every upload stamp kept",
            now.call
        );
        assert_eq!(
            now.qsl_rcvd, was.qsl_rcvd,
            "{}: every confirmation kept",
            now.call
        );
        assert_eq!(now.confirmed, was.confirmed);
        assert_eq!(now.award_confirmed, was.award_confirmed);
    }
    assert_eq!(after[2], before[2], "the unknown call is not touched");
    assert!(
        engine_lock(&engine).take_pending_uploads().is_empty(),
        "an update queues nothing"
    );

    // Run again: every field it could fill is filled, so nothing changes and no run is made.
    let (again, _) = fill(&engine, &ids, ids.len(), &answers(&["W1AW"]), JAN_2026);
    let again = again.unwrap();
    assert_eq!((again.changed, again.run), (0, None));

    let (undone, dur) = undo_run(&engine, &made.run.unwrap());
    assert_eq!(undone.unwrap().changed, 2);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(
        rows(&engine),
        before,
        "one undo puts every contact back, whole"
    );
}

/// A contest contact's exchange is never replaced: the zone and state it carries stay the
/// exchange's, whatever the callbook says; an empty field beside them still fills.
#[test]
fn an_update_never_overrides_a_contest_exchange() {
    let d = Dir::new("tools-callbook-contest");
    let engine = shared(
        &d,
        &[adif(
            "W1AW",
            "120000",
            "<COUNTRY:13>United States<CONTEST_ID:6>CQ-WPX<SRX_STRING:1>4<CQZ:1>4<STATE:2>MA",
        )],
    );
    let before = rows(&engine);
    let ids = [before[0].id.unwrap()];
    let (made, dur) = fill(&engine, &ids, 1, &answers(&["W1AW"]), JAN_2026);
    assert_eq!(made.unwrap().changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    let r = &rows(&engine)[0];
    let fields = callbook_fields(r);
    assert_eq!(fields[5], ("CQZ", Some("4".into())));
    assert_eq!(fields[3], ("STATE", Some("MA".into())));
    assert_eq!(fields[4], ("CNTY", None), "no Connecticut county under MA");
    assert_eq!(fields[0], ("NAME", Some("Hiram".into())));
    assert_eq!(r.contest, before[0].contest, "the exchange is as it was");
}

/// The preview: the distinct calls once each, those the cache answers, and per field how many
/// contacts the fill could give it — exact for a kept answer, at most for a call still to look
/// up. It changes nothing.
#[test]
fn the_update_preview_counts_calls_and_fields() {
    let d = Dir::new("tools-callbook-preview");
    let engine = shared(
        &d,
        &[
            adif("W1AW", "120000", "<COUNTRY:13>United States"),
            adif("w1aw", "121000", "<COUNTRY:13>United States<NAME:3>Bob"),
            adif("K1ABC", "122000", "<COUNTRY:13>United States"),
        ],
    );
    let before = rows(&engine);
    let mut ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();
    ids.push(ids[0]);
    let mut asked = Vec::new();
    let p = callbook_preview(&engine, &ids, JAN_2026, &mut |call| {
        asked.push(call.to_string());
        (call == "W1AW").then(|| qrz_answer("W1AW"))
    })
    .unwrap();
    assert_eq!(
        asked,
        ["W1AW", "K1ABC"],
        "each call asked of the cache once"
    );
    assert_eq!(p.selected, 3);
    assert_eq!(p.missing, 0);
    assert_eq!(p.calls, ["W1AW", "K1ABC"]);
    assert_eq!(p.kept, 1);
    assert_eq!(p.could_change, 3);
    // W1AW's answer: NAME on one contact (the other holds Bob); the rest on both. K1ABC, still to
    // be looked up: at most every field the rule fills.
    assert_eq!(p.fields.get("NAME"), Some(&2));
    assert_eq!(p.fields.get("GRIDSQUARE"), Some(&3));
    assert_eq!(p.fields.get("CQZ"), Some(&3));
    assert_eq!(p.fields.get("EMAIL"), Some(&1), "W1AW's answer has none");
    assert_eq!(rows(&engine), before, "a preview changes nothing");
}

/// The update is made only on the contacts the operator confirmed.
#[test]
fn an_update_acts_only_on_the_count_confirmed() {
    let d = Dir::new("tools-callbook-count");
    let engine = shared(&d, &[adif("W1AW", "120000", "")]);
    let before = rows(&engine);
    let ids = [before[0].id.unwrap()];
    let (refused, _) = fill(&engine, &ids, 2, &answers(&["W1AW"]), JAN_2026);
    assert_eq!(refused, Err(NOT_AS_PREVIEWED.to_string()));
    assert_eq!(rows(&engine), before);
}

/// One ADIF record like [`adif`]'s, on `date` (`YYYYMMDD`).
fn adif_on(call: &str, date: &str, more: &str) -> String {
    format!(
        "<CALL:{}>{call}<BAND:3>20m<FREQ:6>14.074<MODE:3>FT8<QSO_DATE:8>{date}<TIME_ON:6>120000\
         {more}<EOR>\n",
        call.len()
    )
}

/// ★ THE 12-MONTH LOCATION GATE (operator ruling). A callbook says where a station is NOW: the
/// bulk update gives a contact from the last 12 months the whole answer, and an older one only who
/// the operator is: the name (and e-mail, QSL route, habits), never the grid, QTH, state, county,
/// zones, position or UTC offset. By value, on the stored contacts.
#[test]
fn an_update_gives_the_location_only_to_contacts_from_the_last_12_months() {
    let d = Dir::new("tools-callbook-age");
    let engine = shared(
        &d,
        &[
            // Two years before "now": the name only.
            adif_on("W1AW", "20240101", "<COUNTRY:13>United States"),
            // Three months before "now": everything.
            adif_on("W1AW", "20251001", "<COUNTRY:13>United States"),
        ],
    );
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();
    let mut full = qrz_answer("W1AW");
    full.position = Some((41.7, -72.7));
    full.utc_offset = Some(-5.0);
    full.iota = Some("NA-001".into());
    full.email = Some("w1aw@example.org".into());
    let answers = HashMap::from([("W1AW".to_string(), full)]);

    // The preview first: the old contact is counted as left without its location.
    let p = callbook_preview(&engine, &ids, JAN_2026, &mut |_| {
        Some(answers["W1AW"].clone())
    })
    .unwrap();
    assert_eq!(p.location_too_old, 1);
    assert_eq!(p.fields.get("NAME"), Some(&2));
    assert_eq!(
        p.fields.get("GRIDSQUARE"),
        Some(&1),
        "the recent contact only"
    );
    assert_eq!(p.fields.get("EMAIL"), Some(&2));

    let (made, dur) = fill(&engine, &ids, ids.len(), &answers, JAN_2026);
    assert_eq!(made.unwrap().changed, 2);
    dur.wait(DURABLE_WAIT).unwrap();
    let after = rows(&engine);
    let extra =
        |r: &QsoRecord, t: &str| r.extra.iter().find(|(k, _)| k == t).map(|(_, v)| v.clone());

    let old = &after[0];
    assert_eq!(
        old.when_unix,
        1_704_067_200 + 12 * 3600,
        "premise: the 2024 contact"
    );
    assert_eq!(
        callbook_fields(old),
        vec![
            ("NAME", Some("Hiram".into())),
            ("QTH", None),
            ("GRIDSQUARE", None),
            ("STATE", None),
            ("CNTY", None),
            ("CQZ", None),
            ("ITUZ", None),
        ]
    );
    assert_eq!(extra(old, "EMAIL").as_deref(), Some("w1aw@example.org"));
    for tag in ["LAT", "LON", callbook::UTC_OFFSET_FIELD] {
        assert_eq!(
            extra(old, tag),
            None,
            "{tag}: no location on a 2-year-old contact"
        );
    }

    let recent = &after[1];
    assert_eq!(
        callbook_fields(recent),
        vec![
            ("NAME", Some("Hiram".into())),
            ("QTH", Some("Newington".into())),
            ("GRIDSQUARE", Some("FN31pr".into())),
            ("STATE", Some("CT".into())),
            ("CNTY", Some("CT,Hartford".into())),
            ("CQZ", Some("5".into())),
            ("ITUZ", Some("8".into())),
        ]
    );
    assert_eq!(extra(recent, "EMAIL").as_deref(), Some("w1aw@example.org"));
    for tag in ["LAT", "LON", callbook::UTC_OFFSET_FIELD] {
        assert!(
            extra(recent, tag).is_some(),
            "{tag}: a 3-month-old contact takes it"
        );
    }

    // Every location field, by its ADIF name: none on the old contact, each on the recent one.
    let location = |r: &QsoRecord, t: &str| match t {
        "GRIDSQUARE" => r.grid.clone(),
        "QTH" => r.qth.clone(),
        "STATE" => r.state.clone(),
        "IOTA" => r.ota.iota.clone(),
        _ => extra(r, t),
    };
    assert_eq!(recent.ota.iota.as_deref(), Some("NA-001"));
    for tag in LOCATION_FIELDS {
        assert_eq!(
            location(old, tag),
            None,
            "{tag}: not on the 2-year-old contact"
        );
        assert!(
            location(recent, tag).is_some(),
            "{tag}: on the recent contact"
        );
    }
}

/// A log of `n` contacts, each its own call, and the answer for every call.
fn callbook_log(d: &Dir, n: usize) -> (Arc<Mutex<Engine>>, HashMap<String, CallbookAnswer>) {
    let records: Vec<String> = (0..n)
        .map(|i| {
            adif(
                &format!("K{i}ABC"),
                &format!("{:02}{:02}{:02}", i / 3600, i / 60 % 60, i % 60),
                "",
            )
        })
        .collect();
    let answers = (0..n)
        .map(|i| {
            let call = format!("K{i}ABC");
            (call.clone(), qrz_answer(&call))
        })
        .collect();
    (shared(d, &records), answers)
}

/// ★ A LARGE UPDATE WAITS FOR THE RADIO. The FT sequencer is armed after the first part is made:
/// the next part is refused, the fill waits (with the Engine lock released) until transmit is
/// off, then makes the rest — every contact, in ONE run that one undo takes back.
#[test]
fn a_large_update_waits_for_transmit_to_end_and_is_one_run() {
    let n = CHUNK + 6;
    let d = Dir::new("tools-callbook-tx-wait");
    let (engine, answers) = callbook_log(&d, n);
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();
    let arm = Arc::clone(&engine);
    let mut plans = 0;
    let mut waits = 0;
    let (made, why, dur) = racing(
        move || {
            plans += 1;
            if plans == 2 {
                engine_lock(&arm).set_tx_enabled(true);
            }
        },
        || {
            callbook_fill(&engine, &ids, n, &answers, JAN_2026, &mut || {
                waits += 1;
                let mut e = engine_lock(&engine);
                assert!(e.transmit_needs_the_lock(), "premise: armed for FT");
                e.set_tx_enabled(false);
                true
            })
        },
    );
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(why, None);
    assert_eq!(waits, 1);
    assert_eq!(
        made.changed, n,
        "every contact, the parts after the wait too"
    );
    assert!(rows(&engine)
        .iter()
        .all(|r| r.name.as_deref() == Some("Hiram")));
    let (undone, dur) = undo_run(&engine, &made.run.unwrap());
    assert_eq!(undone.unwrap().changed, n, "one run: both parts");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(rows(&engine), before);
}

/// ★ STOPPED PART-WAY, STILL UNDONE WHOLE. The radio transmits from the second part on and the
/// operator stops the wait: the first part stands, and the fill answers it WITH its run — the
/// toast's Undo takes it back — and says how far it got.
#[test]
fn an_update_stopped_while_waiting_for_transmit_answers_its_run() {
    let n = CHUNK + 6;
    let d = Dir::new("tools-callbook-tx-stop");
    let (engine, answers) = callbook_log(&d, n);
    let before = rows(&engine);
    let ids: Vec<RecordId> = before.iter().map(|r| r.id.unwrap()).collect();
    let arm = Arc::clone(&engine);
    let mut plans = 0;
    let (made, why, dur) = racing(
        move || {
            plans += 1;
            if plans == 2 {
                engine_lock(&arm).set_tx_enabled(true);
            }
        },
        || callbook_fill(&engine, &ids, n, &answers, JAN_2026, &mut || false),
    );
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(made.changed, CHUNK, "the first part stands");
    assert_eq!(why, Some(stopped(&made, MUST_WAIT_FOR_TX)));
    assert!(why
        .unwrap()
        .starts_with(&format!("{CHUNK} contacts were changed")));
    // A large undo waits for transmit too.
    engine_lock(&engine).set_tx_enabled(false);
    let (undone, dur) = undo_run(&engine, &made.run.expect("the run of the part made"));
    assert_eq!(undone.unwrap().changed, CHUNK);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(rows(&engine), before);
}

/// The control: the single-contact rule the callsign card and the log time use
/// ([`callbook::fill_from_callbook`]) has no age gate. The same 2-year-old contact, filled by it,
/// takes the whole answer.
#[test]
fn the_single_contact_fill_has_no_age_gate() {
    let d = Dir::new("tools-callbook-age-control");
    let engine = shared(
        &d,
        &[adif_on("W1AW", "20240101", "<COUNTRY:13>United States")],
    );
    let mut r = QsoRecord::clone(&rows(&engine)[0]);
    callbook::fill_from_callbook(&mut r, &qrz_answer("W1AW"));
    assert_eq!(
        callbook_fields(&r),
        vec![
            ("NAME", Some("Hiram".into())),
            ("QTH", Some("Newington".into())),
            ("GRIDSQUARE", Some("FN31pr".into())),
            ("STATE", Some("CT".into())),
            ("CNTY", Some("CT,Hartford".into())),
            ("CQZ", Some("5".into())),
            ("ITUZ", Some("8".into())),
        ]
    );
}

// ── blank grids ─────────────────────────────────────────────────────────────────────────────────

/// ★ The blank-grid offer: each contact with no grid for which a grid is known — the one the
/// station sent on the air first (the grid heard nearest the contact's time), else the callbook's
/// for a call with no stroke and no park from the last 12 months — and nothing for a contact with a
/// grid, a stroked call, a park's contact or an old one with only a callbook answer. Only the
/// ticked are filled, as one run its undo takes back: the heard grid as the contact's own, the
/// callbook's with the callbook's mark (counted toward nothing, sent nowhere). An uploaded contact
/// keeps its stamps, and nothing is queued to a service. A contact given a grid since is left out.
#[test]
fn blank_grids_are_offered_and_only_the_ticked_are_filled() {
    let d = Dir::new("tools-grids");
    let engine = shared(
        &d,
        &[
            adif("W1AW", "120000", ""),
            adif("K1ABC", "120100", ""),
            adif("K1ABC/P", "120200", ""),
            adif("N0XYZ", "120300", "<SIG:4>POTA<SIG_INFO:7>US-0001"),
            adif("VE3AA", "120400", "<GRIDSQUARE:4>FN03"),
            adif("G4AAA", "120500", ""),
            adif("JA1AA", "120600", ""),
        ],
    );
    let now = 1_767_270_000; // 1 January 2026, after the contacts.
    let noon = 1_767_268_800; // 1 January 2026 12:00:00, the first contact.
                              // The decode log heard W1AW send FN31 an hour before (and nothing else asked of it); this
                              // session, JA1AA PM95 an hour after its contact.
    let mut heard = |calls: &HashSet<String>| {
        assert!(
            calls.contains("W1AW") && !calls.contains("VE3AA"),
            "the blank ones asked"
        );
        HashMap::from([(
            "W1AW".to_string(),
            vec![(noon - 3600, noon - 3600, "FN31".to_string())],
        )])
    };
    let mut session = |calls: &HashSet<String>| {
        assert!(calls.contains("JA1AA"), "the blank ones asked");
        HashMap::from([("JA1AA".to_string(), (noon + 360 + 3600, "PM95".to_string()))])
    };
    let answer = |call: &str, grid: &str| CallbookAnswer {
        call: call.into(),
        grid: Some(grid.into()),
        ..CallbookAnswer::default()
    };
    let asked = RefCell::new(Vec::new());
    let mut kept = |key: &str| {
        asked.borrow_mut().push(key.to_string());
        match key {
            "W1AW" => Some(answer("W1AW", "AA00")),
            "K1ABC" => Some(answer("K1ABC", "FN42ab")),
            // A callbook record of the stroked call itself: still no grid for it.
            "K1ABC/P" => Some(answer("K1ABC/P", "FN42ab")),
            "N0XYZ" => Some(answer("N0XYZ", "EN34")),
            "G4AAA" => Some(answer("G4AAA", "IO91")),
            _ => None,
        }
    };
    let (blank, offers) = grid_offers(&engine, &mut heard, &mut session, &mut kept, now).unwrap();
    assert_eq!(blank, 6, "every contact with no grid");
    let seen: Vec<(&str, &str, GridFrom)> = offers
        .iter()
        .map(|o| (o.call.as_str(), o.grid.as_str(), o.source))
        .collect();
    assert_eq!(
        seen,
        [
            ("W1AW", "FN31", GridFrom::Heard),
            ("K1ABC", "FN42ab", GridFrom::Callbook),
            ("G4AAA", "IO91", GridFrom::Callbook),
            ("JA1AA", "PM95", GridFrom::Heard),
        ],
        "heard first; the callbook only with no stroke and no park"
    );
    assert!(
        !asked.borrow().contains(&"W1AW".to_string()),
        "heard: no callbook asked"
    );
    // A year on, the callbook says where a station is now, not then: only the heard grid.
    let (_, later) = grid_offers(
        &engine,
        &mut heard,
        &mut session,
        &mut kept,
        now + 400 * 86_400,
    )
    .unwrap();
    assert_eq!(later.len(), 2);

    let w1aw = by_call(&engine, "W1AW")[0].clone();
    stamp(&engine, &w1aw, UploadService::Qrz, 1_767_270_100);
    let (_, offers) = grid_offers(&engine, &mut heard, &mut session, &mut kept, now).unwrap();
    assert!(
        offers[0].uploaded && !offers[1].uploaded,
        "the uploaded one says so"
    );
    let w1aw = by_call(&engine, "W1AW")[0].clone();
    // JA1AA is given a grid by the operator before the fill: left out.
    let ja = by_call(&engine, "JA1AA")[0].clone();
    let mut edit = QsoEdit::project(&ja);
    edit.grid = Some("PM96".into());
    let (m, dd) = logwrite::edit_row(&engine, ja.id.unwrap(), &QsoEdit::project(&ja).key(), &edit);
    m.unwrap().unwrap();
    dd.wait(DURABLE_WAIT).unwrap();
    let pick = |o: &GridOffer| GridPick {
        id: o.id,
        grid: o.grid.clone(),
        source: o.source,
    };
    let before = rows(&engine);
    let ticked: Vec<GridPick> = offers.iter().map(pick).collect();
    let (made, dur) = fill_grids(&engine, &ticked);
    let made = made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(made.changed, 3);
    assert_eq!(made.refused, vec![(ja.id.unwrap(), "changed")]);
    let after_w1 = by_call(&engine, "W1AW")[0].clone();
    assert_eq!(after_w1.grid.as_deref(), Some("FN31"));
    assert_eq!(
        tempo_core::logbook::counted_grid(&after_w1),
        Some("FN31"),
        "heard: counted"
    );
    assert_eq!(after_w1.upload, w1aw.upload, "its upload stamps stay");
    let k1 = by_call(&engine, "K1ABC")[0].clone();
    assert_eq!(k1.grid.as_deref(), Some("FN42ab"));
    assert!(
        callbook::is_callbook_filled(&k1, "GRIDSQUARE"),
        "the callbook's mark"
    );
    assert!(callbook::is_map_only_grid(&k1), "for the map only");
    assert!(
        !callbook::is_map_only_grid(&after_w1),
        "a heard grid is not map only"
    );
    assert_eq!(
        tempo_core::logbook::counted_grid(&k1),
        None,
        "counted toward nothing"
    );
    assert!(
        !tempo_core::logbook::adif_record(&k1).contains("<GRIDSQUARE:"),
        "sent nowhere"
    );
    assert_eq!(
        by_call(&engine, "JA1AA")[0].grid.as_deref(),
        Some("PM96"),
        "the edit stands"
    );
    assert!(
        engine_lock(&engine).take_pending_uploads().is_empty(),
        "nothing queued to a service"
    );
    // One run: its undo puts every contact back.
    let (undone, dur) = undo_run(&engine, made.run.as_deref().unwrap());
    undone.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    settle(&engine);
    assert_eq!(rows(&engine), before, "the undo takes the whole fill back");
    // A grid that is no locator is refused whole.
    let bad = vec![GridPick {
        id: w1aw.id.unwrap(),
        grid: "NOPE".into(),
        source: GridFrom::Heard,
    }];
    assert!(fill_grids(&engine, &bad).0.is_err());
    assert_eq!(rows(&engine), before);
}

/// ALL.TXT's lines as WSJT-X writes them: each CQ, or call to anyone, that carries the sender's
/// grid, as a run of one grid (first heard, last heard, grid); transmissions, reports, free text
/// and other lines passed over. A report and DT run together (`-12-10.5`) move the message a word
/// left, and it is still read. The grid in effect at a time is the last run begun at or before
/// it, else the first after it.
#[test]
fn the_grids_heard_are_read_from_all_txt() {
    let text = "\
260101_120000    14.074 Rx FT8    -10  0.2 1500 CQ W1AW FN31
260101_120015    14.074 Rx FT8    -10  0.2 1500 CQ W1AW FN31
260101_120030    14.074 Tx FT8      0  0.0 1500 CQ K1ABC FN42
260101_120045    14.074 Rx FT8    -12  0.1  900 KD9TAW W1AW -07
260101_120100    14.074 Rx FT8    -12  0.1  900 K2DEF W1AW FN32
260101_120115    14.074 Rx FT8    -12  0.1  900 TNX FER QSO
garbage line
260102_120000     7.074 Rx FT4    -10  0.2 1500 CQ DX G4AAA IO91
260102_120030     7.074 Rx FT4    -12-10.5 1500 CQ JA1AA PM95
";
    let wanted: HashSet<String> = ["W1AW", "G4AAA", "K1ABC", "JA1AA"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let got = heard_grids_in(text.as_bytes(), &wanted);
    let w1 = &got["W1AW"];
    assert_eq!(
        w1,
        &vec![
            (1_767_268_800, 1_767_268_815, "FN31".to_string()),
            (1_767_268_860, 1_767_268_860, "FN32".to_string())
        ]
    );
    assert_eq!(
        got["G4AAA"],
        vec![(1_767_355_200, 1_767_355_200, "IO91".to_string())]
    );
    assert_eq!(
        got["JA1AA"],
        vec![(1_767_355_230, 1_767_355_230, "PM95".to_string())],
        "a report and DT run together"
    );
    assert!(
        !got.contains_key("K1ABC"),
        "our own transmission is not heard"
    );
    assert_eq!(heard_at(w1, 1_767_268_850), Some("FN31"));
    assert_eq!(heard_at(w1, 1_767_268_900), Some("FN32"));
    assert_eq!(
        heard_at(w1, 1_767_268_800 - 3600),
        Some("FN31"),
        "an hour before any: the first"
    );
}

/// ★ A grid heard more than 24 hours from the contact, before or after, is not offered as heard:
/// the station may have been somewhere else then. The contact falls to the callbook's grid (for
/// the map only) where there is one, else to nothing; this session's grid is held to the same
/// rule (operator decision, 2026-10-27). Within the day, a run the station kept sending is in
/// effect from its first decode to its last.
#[test]
fn a_grid_heard_far_from_the_contact_is_not_offered_as_heard() {
    let noon = 1_767_268_800; // 1 January 2026 12:00:00.
    let day = 86_400;
    let w1 = vec![(noon + 400 * day, noon + 401 * day, "FN31".to_string())];
    assert_eq!(heard_at(&w1, noon), None, "heard 400 days later");
    let w1 = vec![(noon - 3 * day, noon - day - 1, "FN31".to_string())];
    assert_eq!(
        heard_at(&w1, noon),
        None,
        "last heard a day and a second before"
    );
    let w1 = vec![(noon - 3 * day, noon - day, "FN31".to_string())];
    assert_eq!(heard_at(&w1, noon), Some("FN31"), "sent until a day before");
    let w1 = vec![(noon + day, noon + 2 * day, "FN31".to_string())];
    assert_eq!(heard_at(&w1, noon), Some("FN31"), "first heard a day after");

    let d = Dir::new("tools-grids-far");
    let engine = shared(
        &d,
        &[
            adif("W1AW", "120000", "<MY_GRIDSQUARE:4>EN52"),
            adif("G4AAA", "120500", "<MY_GRIDSQUARE:4>EN52"),
            adif("JA1AA", "120600", ""),
        ],
    );
    let mut heard = |_: &HashSet<String>| {
        HashMap::from([
            (
                "W1AW".to_string(),
                vec![(noon + 400 * day, noon + 400 * day, "FN31".to_string())],
            ),
            (
                "G4AAA".to_string(),
                vec![(noon - 2 * day, noon - 2 * day, "IO92".to_string())],
            ),
        ])
    };
    let mut session = |_: &HashSet<String>| {
        HashMap::from([("JA1AA".to_string(), (noon + 3 * day, "PM95".to_string()))])
    };
    let mut kept = |key: &str| {
        (key == "G4AAA").then(|| CallbookAnswer {
            call: "G4AAA".into(),
            grid: Some("IO91".into()),
            ..CallbookAnswer::default()
        })
    };
    let (blank, offers) =
        grid_offers(&engine, &mut heard, &mut session, &mut kept, noon + day).unwrap();
    assert_eq!(blank, 3);
    let seen: Vec<(&str, &str, GridFrom)> = offers
        .iter()
        .map(|o| (o.call.as_str(), o.grid.as_str(), o.source))
        .collect();
    assert_eq!(
        seen,
        [("G4AAA", "IO91", GridFrom::Callbook)],
        "none heard near enough: only the callbook's, for the map only"
    );
    // The fill writes the distance from the contact's own grid, as the log funnel does.
    let picks: Vec<GridPick> = offers
        .iter()
        .map(|o| GridPick {
            id: o.id,
            grid: o.grid.clone(),
            source: o.source,
        })
        .collect();
    let (made, dur) = fill_grids(&engine, &picks);
    made.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    let g4 = by_call(&engine, "G4AAA")[0].clone();
    assert!(callbook::is_map_only_grid(&g4), "premise: map only");
    assert_eq!(
        tempo_core::logbook::extra_field(&g4, "DISTANCE"),
        crate::engine::distance_km(&g4)
            .map(|km| km.to_string())
            .as_deref(),
        "the distance, as the log funnel fills it"
    );
    assert!(
        tempo_core::logbook::extra_field(&g4, "DISTANCE").is_some(),
        "premise: a distance"
    );
}
