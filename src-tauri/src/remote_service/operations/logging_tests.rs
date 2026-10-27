//! The shack's own log commands find the row the view showed by its content.
use super::*;

// ---- The shack's own log commands find the row the view showed by its content ------------

const SEED: &str = "<CALL:4>W1AW<BAND:3>20m<MODE:3>FT8<FREQ:6>14.074<QSO_DATE:8>20260909<TIME_ON:6>010000<NAME:4>Joe <EOR>\n\
<CALL:5>K1ABC<BAND:3>40m<MODE:2>CW<FREQ:5>7.030<QSO_DATE:8>20260909<TIME_ON:6>020000<EOR>\n";

fn seed(f: &Fixture) {
    f.engine.lock().unwrap().import_adif(SEED);
    assert_eq!(f.engine.lock().unwrap().stored_log().len(), 2);
    // In the file, as 1.13's import had written it before it returned: on the 1.13 path the
    // file is written on its own lane since SPEC-2 v3 C19 (D1-A).
    let _ = written(f);
}

fn adif(f: &Fixture) -> Vec<u8> {
    std::fs::read(f.dir.join("contacts.adi")).unwrap()
}

/// ★ THE SHACK'S COMMANDS, SPLIT THE SAME WAY: the desktop's log commands find the row the view
/// showed in the store with the engine lock released (`find_seen`), and change it only while it
/// is still the version found (`change_found`: planned with the lock released, checked and made
/// under it). A row no contact holds any more is refused in the words the view always showed,
/// whether it changed before the search or between the search and the change; a store whose
/// writer is still behind is refused as unread, nothing changed. The control: once the write
/// lands, the row is found.
#[test]
fn the_shacks_commands_find_their_row_off_the_lock_and_check_it_under_it() {
    use crate::remote_service::log_tests::parse_one;
    use tempo_core::logbook::sqlite::WriteHold;
    let f = Fixture::with_store();
    seed(&f);
    let seen = |call: &str| {
        let e = f.engine.lock().unwrap();
        let row = e.stored_log().into_iter().find(|r| r.call == call);
        crate::logged_qso(&row.unwrap())
    };
    let card = |id| vec![tempo_core::logbook::LogOp::MarkQslCard { id, received: true }];
    let comment = |text: &str| {
        let mut e = f.engine.lock().unwrap();
        let mut r = e
            .stored_log()
            .iter()
            .find(|r| r.call == "K1ABC")
            .unwrap()
            .as_ref()
            .clone();
        r.comment = Some(text.into());
        assert!(e.update_qso(r.id.unwrap(), r));
    };

    let held = seen("K1ABC");
    let found = crate::find_seen(&f.engine, &held).expect("the row the view shows");
    assert_eq!(
        Some(found.id.clone()),
        held.id,
        "the contact the view shows"
    );
    let (changed, _) = crate::change_found(&f.engine, &found, "test", card);
    let changed = changed.expect("still that contact").expect("in the log");
    assert_eq!(changed.id.map(|id| id.to_string()), held.id);

    comment("changed before the search");
    // The change is in the store before the search: on a loaded machine its write can outlast
    // the search's own wait, which is then refused as unread.
    let _ = written(&f);
    assert_eq!(
        crate::find_seen(&f.engine, &held).unwrap_err(),
        crate::LOG_ROW_GONE
    );

    let held = seen("K1ABC");
    let found = crate::find_seen(&f.engine, &held).expect("the row as it is now");
    comment("changed between the search and the change");
    assert_eq!(
        crate::change_found(&f.engine, &found, "test", card)
            .0
            .unwrap_err(),
        crate::LOG_ROW_GONE
    );

    let held = seen("W1AW");
    let hold = WriteHold::take(&f.dir.join("contacts.sqlite3")).unwrap();
    f.engine.lock().unwrap().log_qso(parse_one(
        "<CALL:5>N3CCC<BAND:3>20m<MODE:3>SSB<QSO_DATE:8>20260911<TIME_ON:6>010000<EOR>",
    ));
    let unread = crate::find_seen(&f.engine, &held);
    drop(hold);
    assert_eq!(unread.unwrap_err(), crate::LOG_UNREAD);
    // "Once written" is the control's premise, so wait for the write: letting the hold go only
    // lets the writer start.
    let _ = written(&f);
    assert!(
        crate::find_seen(&f.engine, &held).is_ok(),
        "control: once written, found"
    );
    written(&f);
}

/// What the log's file holds once everything submitted is written — the store's writer and its
/// `log.adi` mirror flushed, or on the 1.13 path the lane that writes `log.adi` (SPEC-2 v3 C19,
/// D1-A) — so a comparison of the file is not a race with either, and the fixture's folder can
/// go.
fn written(f: &Fixture) -> Vec<u8> {
    f.engine
        .lock()
        .unwrap()
        .flush_log_store(std::time::Duration::from_secs(60))
        .expect("written");
    adif(f)
}
