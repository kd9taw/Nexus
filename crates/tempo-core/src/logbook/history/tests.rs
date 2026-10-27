//! The history's proofs at the store: what each kind of change records, that a change and its
//! history land together or not at all, what an undo makes of a contact, and that a store from
//! before the history opens, gains it, and stays readable to the build that made it.

use super::*;
use crate::logbook::sqlite::{Batch, Expect, LogDb};
use crate::logbook::{adif_record_own_log, parse_adif, UploadOutcome, UploadState, UploadStatus};
use std::sync::Arc;

/// A plain contact with id `n`.
fn rec(call: &str, n: u64) -> QsoRecord {
    let mut r = parse_adif(&format!(
        "<CALL:{}>{call}<QSO_DATE:8>20260701<TIME_ON:6>010203<BAND:3>20m<MODE:3>FT8\
         <GRIDSQUARE:4>EN52<NAME:3>Bob<APP_OTHERLOG_X:5>kept!<EOR>",
        call.len()
    ))
    .remove(0);
    r.id = Some(RecordId::Provisional {
        hash: n,
        ordinal: 0,
    });
    r
}

fn note(run: &str, source: Source, edit: Kind) -> Note {
    Note {
        run: run.into(),
        at_ms: 1_700_000_000_000,
        source,
        edit,
        undoes: HashMap::new(),
    }
}

fn write(db: &mut LogDb, rows: &[QsoRecord], note: Option<&Note>) -> Result<()> {
    let upsert: Vec<RowWrite> = rows
        .iter()
        .map(|r| RowWrite::new(Arc::new(r.clone())))
        .collect();
    db.apply(Batch {
        upsert: &upsert,
        history: note,
        ..Batch::default()
    })
}

fn remove(db: &mut LogDb, ids: &[RecordId], note: Option<&Note>) -> Result<()> {
    db.apply(Batch {
        remove: ids,
        history: note,
        ..Batch::default()
    })
}

fn id(n: u64) -> RecordId {
    RecordId::Provisional {
        hash: n,
        ordinal: 0,
    }
}

fn count(db: &LogDb, table: &str) -> i64 {
    db.conn_for_tests()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

/// ★ Each kind of change is recorded, under the source and run its note names: a new contact,
/// an edit (its fields, before and after, and nothing else), a merge and a fill (their own kinds),
/// and a delete (the contact whole).
#[test]
fn each_kind_of_change_is_recorded_with_its_source() {
    let mut db = LogDb::open_in_memory().unwrap();
    let a = rec("K1ABC", 1);
    write(
        &mut db,
        std::slice::from_ref(&a),
        Some(&note("r1", Source::Operator, Kind::Edit)),
    )
    .unwrap();

    let mut edited = a.clone();
    edited.name = Some("Robert".into());
    edited.comment = Some("new".into());
    write(
        &mut db,
        &[edited.clone()],
        Some(&note("r2", Source::Operator, Kind::Edit)),
    )
    .unwrap();

    let mut merged = edited.clone();
    merged.qsl_rcvd.lotw = true;
    write(
        &mut db,
        &[merged.clone()],
        Some(&note("r3", Source::Lotw, Kind::Merge)),
    )
    .unwrap();

    let mut filled = merged.clone();
    filled.state = Some("WI".into());
    write(
        &mut db,
        &[filled.clone()],
        Some(&note("r4", Source::Fill, Kind::Fill)),
    )
    .unwrap();
    let held = db.load_all().unwrap().remove(0);

    remove(
        &mut db,
        &[id(1)],
        Some(&note("r5", Source::Import, Kind::Edit)),
    )
    .unwrap();

    let h = db.history_of(id(1)).unwrap();
    let seen: Vec<(Kind, &str, &str)> = h
        .iter()
        .map(|e| (e.kind, e.source.as_str(), e.run.as_str()))
        .collect();
    assert_eq!(
        seen,
        vec![
            (Kind::Create, "operator", "r1"),
            (Kind::Edit, "operator", "r2"),
            (Kind::Merge, "lotw", "r3"),
            (Kind::Fill, "fill", "r4"),
            (Kind::Delete, "import", "r5"),
        ]
    );
    assert_eq!(h[0].before, None, "a new contact stores the event alone");
    assert_eq!(
        h[1].changes(),
        vec![
            FieldChange {
                tag: "COMMENT".into(),
                old: None,
                new: Some("new".into()),
            },
            FieldChange {
                tag: "NAME".into(),
                old: Some("Bob".into()),
                new: Some("Robert".into()),
            },
        ],
        "an edit stores the fields it changed, and only those"
    );
    assert_eq!(
        h[3].changes(),
        vec![FieldChange {
            tag: "STATE".into(),
            old: None,
            new: Some("WI".into()),
        }]
    );
    let gone = h[4].deleted_contact().expect("a delete keeps the contact");
    assert_eq!(
        gone, held,
        "the contact as the store held it, every field — a foreign APP_ tag included"
    );
    assert_eq!(
        adif_record_own_log(&gone),
        adif_record_own_log(&filled),
        "and as the log writes it"
    );
    assert_eq!(h[0].at_ms, 1_700_000_000_000);
}

/// A change that moves only an upload stamp is not history: nothing is written for it, not even
/// its run. And a change with no note — the conversion of `log.adi` — writes none.
#[test]
fn a_stamp_and_a_change_without_a_note_write_no_history() {
    let mut db = LogDb::open_in_memory().unwrap();
    let a = rec("K1ABC", 1);
    write(&mut db, std::slice::from_ref(&a), None).unwrap();
    assert_eq!(count(&db, "qso_history"), 0, "no note, no history");
    let mut stamped = a.clone();
    stamped.upload.lotw = Some(UploadStatus {
        outcome: UploadOutcome::Accepted,
        when_unix: 5,
        detail: None,
    });
    write(
        &mut db,
        &[stamped],
        Some(&note("s", Source::Lotw, Kind::Edit)),
    )
    .unwrap();
    assert_eq!(count(&db, "qso_history"), 0, "a stamp is not history");
    assert_eq!(count(&db, "history_run"), 0, "nor is its run");
}

/// ★ A change and its history land together, or neither does: a batch the store refuses part-way
/// (a row with no id, after a good one) leaves no row and no history; one refused by its check
/// (another window changed a row) leaves none either.
#[test]
fn a_refused_batch_leaves_neither_rows_nor_history() {
    let mut db = LogDb::open_in_memory().unwrap();
    let good = rec("K1ABC", 1);
    let mut bad = rec("K2XYZ", 2);
    bad.id = None;
    assert!(write(
        &mut db,
        &[good.clone(), bad],
        Some(&note("r", Source::Import, Kind::Merge))
    )
    .is_err());
    assert_eq!(db.row_count().unwrap(), 0);
    assert_eq!(count(&db, "qso_history"), 0);
    assert_eq!(count(&db, "history_run"), 0);

    write(
        &mut db,
        std::slice::from_ref(&good),
        Some(&note("a", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    let mut theirs = good.clone();
    theirs.comment = Some("theirs".into());
    let mut ours = good.clone();
    ours.comment = Some("ours".into());
    write(&mut db, &[theirs], None).unwrap();
    let expect = HashMap::from([(id(1), Expect::Row(Arc::new(good.clone())))]);
    let upsert = [RowWrite::new(Arc::new(ours))];
    let refused = db.apply(Batch {
        upsert: &upsert,
        expect: Some(&expect),
        history: Some(&note("b", Source::Operator, Kind::Edit)),
        ..Batch::default()
    });
    assert!(refused.is_err(), "the check refuses it");
    assert_eq!(
        db.history_of(id(1)).unwrap().len(),
        1,
        "only the create: the refused edit wrote no history"
    );
}

/// A purge that names no row records a delete of every row it took out.
#[test]
fn a_purge_records_every_contact_it_took_out() {
    let mut db = LogDb::open_in_memory().unwrap();
    write(&mut db, &[rec("K1ABC", 1), rec("K2XYZ", 2)], None).unwrap();
    db.apply(Batch {
        clear: true,
        history: Some(&note("p", Source::Operator, Kind::Edit)),
        ..Batch::default()
    })
    .unwrap();
    let run = db.history_run("p").unwrap();
    assert_eq!(run.len(), 2);
    assert!(run
        .iter()
        .all(|e| e.kind == Kind::Delete && e.before.is_some()));
    assert_eq!(db.history_deleted(10).unwrap().len(), 2);
}

/// "Recently deleted" lists a contact once, by its latest delete, and stops listing it once it
/// is back in the log.
#[test]
fn recently_deleted_lists_what_is_gone_and_only_that() {
    let mut db = LogDb::open_in_memory().unwrap();
    write(&mut db, &[rec("K1ABC", 1), rec("K2XYZ", 2)], None).unwrap();
    remove(
        &mut db,
        &[id(1)],
        Some(&note("d1", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    write(
        &mut db,
        &[rec("K1ABC", 1)],
        Some(&note("back", Source::Undo, Kind::Edit)),
    )
    .unwrap();
    remove(
        &mut db,
        &[id(1)],
        Some(&note("d2", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    remove(
        &mut db,
        &[id(2)],
        Some(&note("d3", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    let gone: Vec<(String, RecordId)> = db
        .history_deleted(10)
        .unwrap()
        .into_iter()
        .map(|e| (e.run, e.qso_id))
        .collect();
    assert_eq!(gone, vec![("d3".into(), id(2)), ("d2".into(), id(1))]);
    write(
        &mut db,
        &[rec("K2XYZ", 2)],
        Some(&note("r", Source::Undo, Kind::Edit)),
    )
    .unwrap();
    assert_eq!(db.history_deleted(10).unwrap().len(), 1);
    let latest = db.history_latest_run(Some("operator")).unwrap().unwrap();
    assert_eq!((latest.run.as_str(), latest.contacts), ("d3", 1));
}

// ── undo, as a function of the entry and the contact as it now stands ─────────────────────────

fn entries_after(changes: &[(&QsoRecord, &QsoRecord)]) -> (LogDb, Vec<Entry>) {
    let mut db = LogDb::open_in_memory().unwrap();
    write(&mut db, &[changes[0].0.clone()], None).unwrap();
    for (i, (_, after)) in changes.iter().enumerate() {
        write(
            &mut db,
            &[(*after).clone()],
            Some(&note(&format!("e{i}"), Source::Operator, Kind::Edit)),
        )
        .unwrap();
    }
    let h = db.history_of(changes[0].0.id.unwrap()).unwrap();
    (db, h)
}

/// An edit's undo puts back each field it changed — and keeps a field changed again since.
#[test]
fn an_undo_puts_back_what_the_edit_changed_and_keeps_what_changed_since() {
    let a = rec("K1ABC", 1);
    let mut b = a.clone();
    b.name = Some("Robert".into());
    b.grid = Some("FN31".into());
    let mut c = b.clone();
    c.grid = Some("FN42".into());
    let (_, h) = entries_after(&[(&a, &b), (&b, &c)]);
    match undo(&h[0], Some(&c), None).unwrap() {
        Undo::Revert { row, kept, .. } => {
            assert_eq!(row.name.as_deref(), Some("Bob"), "the name goes back");
            assert_eq!(row.grid.as_deref(), Some("FN42"), "the later grid stays");
            assert_eq!(kept, vec!["GRIDSQUARE".to_string()]);
            assert_eq!(row.id, a.id);
            assert_eq!(row.extra, c.extra, "a foreign field survives the undo");
        }
        other => panic!("{other:?}"),
    }
    // With nothing changed since, the undo is the contact as it was, field for field.
    match undo(&h[1], Some(&c), None).unwrap() {
        Undo::Revert { row, kept, .. } => {
            assert!(kept.is_empty());
            assert_eq!(row, b, "the contact before the second edit, whole");
        }
        other => panic!("{other:?}"),
    }
    // Every field it changed has moved since: nothing to undo.
    let mut d = c.clone();
    d.grid = Some("FN53".into());
    assert_eq!(
        undo(&h[1], Some(&d), None).err(),
        Some(CannotUndo::ChangedSince)
    );
}

/// ⛔ An undo never puts back a blank station call that would put the contact under another call
/// than the one it names: with the call in Settings another call (or none), the call stays, with
/// its stamps, and the field is named as held. Every other field it changed still goes back. With
/// the Settings call the one it names (a portable /P aside), the blank goes back and keeps the
/// stamps. A named call put back over another is never held.
#[test]
fn an_undo_never_blanks_a_station_call_onto_another_call() {
    let stamp = UploadStatus {
        outcome: UploadOutcome::Accepted,
        when_unix: 9,
        detail: None,
    };
    let a = rec("K1ABC", 1);
    let mut b = a.clone();
    b.station_callsign = Some("KA1OLD".into());
    b.name = Some("Robert".into());
    let mut b_now = b.clone();
    b_now.upload.lotw = Some(stamp.clone());
    let (_, h) = entries_after(&[(&a, &b)]);

    for settings in [Some("KD9NEW"), None] {
        let Undo::Revert { row, kept, held } = undo(&h[0], Some(&b_now), settings).unwrap() else {
            panic!()
        };
        assert_eq!(
            row.station_callsign.as_deref(),
            Some("KA1OLD"),
            "{settings:?}"
        );
        assert_eq!(row.upload, b_now.upload, "{settings:?}: its stamps stay");
        assert_eq!(row.name.as_deref(), Some("Bob"), "the rest goes back");
        assert!(kept.is_empty());
        assert_eq!(held, vec!["STATION_CALLSIGN".to_string()]);
    }
    for settings in ["ka1old", "KA1OLD/P"] {
        let Undo::Revert { row, held, .. } = undo(&h[0], Some(&b_now), Some(settings)).unwrap()
        else {
            panic!()
        };
        assert_eq!(row.station_callsign, None, "{settings}");
        assert_eq!(
            row.upload, b_now.upload,
            "{settings}: the same call all along"
        );
        assert!(held.is_empty());
    }

    // A named call put back over another: never held, and owed again under it.
    let mut c = a.clone();
    c.station_callsign = Some("KD0AA".into());
    let mut d = c.clone();
    d.station_callsign = Some("W6R".into());
    let mut d_now = d.clone();
    d_now.upload.lotw = Some(stamp);
    let (_, h) = entries_after(&[(&c, &d)]);
    let Undo::Revert { row, held, .. } = undo(&h[0], Some(&d_now), Some("KD9NEW")).unwrap() else {
        panic!()
    };
    assert_eq!(row.station_callsign.as_deref(), Some("KD0AA"));
    assert_eq!(row.upload, UploadState::default());
    assert!(held.is_empty());
}

/// An undo keeps the stamps the contact has — unless it corrects the call, which owes the
/// contact to every service again, as an edit of the call does. It never sends anything.
#[test]
fn an_undo_keeps_the_stamps_unless_it_changes_the_call() {
    let stamp = UploadStatus {
        outcome: UploadOutcome::Accepted,
        when_unix: 9,
        detail: None,
    };
    let a = rec("K1ABC", 1);
    let mut b = a.clone();
    b.name = Some("Robert".into());
    let mut b_now = b.clone();
    b_now.upload.qrz = Some(stamp.clone());
    let (_, h) = entries_after(&[(&a, &b)]);
    let Undo::Revert { row, .. } = undo(&h[0], Some(&b_now), None).unwrap() else {
        panic!()
    };
    assert_eq!(
        row.upload.qrz,
        Some(stamp.clone()),
        "a stamp is never undone"
    );

    let mut corrected = a.clone();
    corrected.call = "K1ABD".into();
    let mut corrected_now = corrected.clone();
    corrected_now.upload.qrz = Some(stamp.clone());
    let (_, h) = entries_after(&[(&a, &corrected)]);
    let Undo::Revert { row, .. } = undo(&h[0], Some(&corrected_now), None).unwrap() else {
        panic!()
    };
    assert_eq!(row.call, "K1ABC");
    assert_eq!(
        row.upload,
        UploadState::default(),
        "a corrected call is owed again"
    );
}

/// A new contact's undo takes it out; a delete's puts it back whole; neither is made on a log
/// where it would mean nothing.
#[test]
fn a_create_undoes_to_a_removal_and_a_delete_to_a_restore() {
    let mut db = LogDb::open_in_memory().unwrap();
    let a = rec("K1ABC", 1);
    write(
        &mut db,
        std::slice::from_ref(&a),
        Some(&note("c", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    remove(
        &mut db,
        &[id(1)],
        Some(&note("d", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    let h = db.history_of(id(1)).unwrap();
    assert_eq!(undo(&h[0], Some(&a), None), Ok(Undo::Remove(id(1))));
    assert_eq!(undo(&h[0], None, None), Err(CannotUndo::Gone));
    match undo(&h[1], None, None).unwrap() {
        Undo::Restore(back) => {
            assert_eq!(back, a, "the contact as it was, field for field");
            assert_eq!(adif_record_own_log(&back), adif_record_own_log(&a));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(undo(&h[1], Some(&a), None), Err(CannotUndo::AlreadyThere));
}

// ── the schema: a store from before the history ─────────────────────────────────────────────

/// ★ A store written before the history existed opens, gains its two tables, keeps every
/// contact and its version; and the history it gains never stops the build that wrote it from
/// reading it — the version is unchanged, the old tables are as they were.
#[test]
fn a_store_from_before_the_history_gains_it_and_stays_readable() {
    let dir = std::env::temp_dir().join(format!(
        "nexus-history-schema-{}-{}",
        std::process::id(),
        line!()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("log.db");
    {
        let mut db = LogDb::open(&path).unwrap();
        write(&mut db, &[rec("K1ABC", 1), rec("K2XYZ", 2)], None).unwrap();
    }
    // Today's store: exactly this one without the history's objects.
    let old_schema = {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("DROP TABLE qso_history; DROP TABLE history_run;")
            .unwrap();
        schema(&conn)
    };
    assert!(old_schema.iter().all(|(_, name, _)| !ours(name)));

    let mut db = LogDb::open(&path).expect("today's store opens");
    assert_eq!(
        db.meta("schema_version").unwrap(),
        Some(1),
        "the version does not move"
    );
    assert_eq!(db.load_all().unwrap().len(), 2, "every contact is there");
    remove(
        &mut db,
        &[id(2)],
        Some(&note("d", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    drop(db);
    let conn = Connection::open(&path).unwrap();
    let now: Vec<_> = schema(&conn)
        .into_iter()
        .filter(|(_, name, _)| !ours(name))
        .collect();
    assert_eq!(
        now, old_schema,
        "the log's own tables and indexes are as they were"
    );
    // An older build's view: its version check passes and its tables read as before.
    let version: i64 = conn
        .query_row(
            "SELECT v FROM log_meta WHERE k = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version, crate::logbook::sqlite::SCHEMA_VERSION);
    // A contact an older build deletes leaves the history alone, and the restore still reads it.
    conn.execute("DELETE FROM qso WHERE id = ?1", [id(1).to_string()])
        .unwrap();
    drop(conn);
    let db = LogDb::open(&path).unwrap();
    let h = db.history_of(id(2)).unwrap();
    assert!(
        matches!(undo(&h[0], None, None), Ok(Undo::Restore(_))),
        "restorable after a restart"
    );
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The history's own tables and indexes.
fn ours(name: &str) -> bool {
    name.contains("history_run") || name.starts_with("qso_history")
}

fn schema(conn: &Connection) -> Vec<(String, String, Option<String>)> {
    let mut stmt = conn
        .prepare("SELECT type, name, sql FROM sqlite_master ORDER BY type, name")
        .unwrap();
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap();
    rows.map(|r| r.unwrap()).collect()
}

// ── the cost, measured ──────────────────────────────────────────────────────────────────────

/// What the history costs a big log: the same writes — a log of `HISTORY_BENCH_ROWS` contacts
/// (150,000 by default) put in 256-row chunks as the writer does, then 1,000 one-contact edits and
/// 1,000 one-contact deletes — made with no note and with one, timed, and the store's size on disk
/// after each; then the reads the UI makes. A measurement, not a check: run it in release with
/// `cargo test -p tempo-core --release --lib the_history_at_scale -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement: run in release with --ignored --nocapture"]
fn the_history_at_scale() {
    use std::time::Instant;
    let n: u64 = std::env::var("HISTORY_BENCH_ROWS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150_000);
    let base = rec("K1ABC", 0);
    let row = |i: u64| {
        let mut r = base.clone();
        r.call = format!("K{}AB{}", i % 10, i);
        r.when_unix += i;
        r.id = Some(id(i + 1));
        r
    };
    for noted in [false, true] {
        let dir = std::env::temp_dir().join(format!(
            "nexus-history-scale-{}-{noted}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.db");
        let mut db = LogDb::open(&path).unwrap();
        let mut k = 0u64;
        let mut note_for = |src: Source| {
            k += 1;
            Note {
                run: format!("run-{k}"),
                at_ms: 1_700_000_000_000,
                source: src,
                edit: Kind::Edit,
                undoes: HashMap::new(),
            }
        };
        let import = note_for(Source::Import);
        let started = Instant::now();
        let mut i = 0;
        while i < n {
            let rows: Vec<QsoRecord> = (i..(i + 256).min(n)).map(row).collect();
            write(&mut db, &rows, noted.then_some(&import)).unwrap();
            i += 256;
        }
        let load = started.elapsed();
        let started = Instant::now();
        for j in 0..1_000u64 {
            let mut r = row(j * 97 % n);
            r.comment = Some(format!("edit {j}"));
            let note = note_for(Source::Operator);
            write(&mut db, &[r], noted.then_some(&note)).unwrap();
        }
        let edits = started.elapsed();
        let started = Instant::now();
        for j in 0..1_000u64 {
            let note = note_for(Source::Operator);
            remove(&mut db, &[id(n - j)], noted.then_some(&note)).unwrap();
        }
        let deletes = started.elapsed();
        db.conn_for_tests()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
        let bytes = std::fs::metadata(&path).unwrap().len();
        println!(
            "HISTORY {} rows, history {}: load {:.2} s ({:.1} µs/row); 1,000 edits {:.1} ms \
             ({:.0} µs each); 1,000 deletes {:.1} ms ({:.0} µs each); store {:.1} MiB",
            n,
            if noted { "ON " } else { "OFF" },
            load.as_secs_f64(),
            load.as_secs_f64() * 1e6 / n as f64,
            edits.as_secs_f64() * 1e3,
            edits.as_secs_f64() * 1e3,
            deletes.as_secs_f64() * 1e3,
            deletes.as_secs_f64() * 1e3,
            bytes as f64 / 1_048_576.0
        );
        if noted {
            let count = |t: &str| count(&db, t);
            println!(
                "HISTORY entries {} in {} runs",
                count("qso_history"),
                count("history_run")
            );
            let started = Instant::now();
            let h = db.history_of(id(97)).unwrap();
            let one = started.elapsed();
            let started = Instant::now();
            let gone = db.history_deleted(200).unwrap();
            let deleted = started.elapsed();
            let started = Instant::now();
            let latest = db.history_latest_run(Some("import")).unwrap();
            let latest_t = started.elapsed();
            println!(
                "HISTORY reads: one contact's ({} entries) {:.3} ms; recently deleted ({}) {:.3} \
                 ms; latest import run ({} contacts) {:.3} ms",
                h.len(),
                one.as_secs_f64() * 1e3,
                gone.len(),
                deleted.as_secs_f64() * 1e3,
                latest.map_or(0, |r| r.contacts),
                latest_t.as_secs_f64() * 1e3
            );
        }
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ── what an undo must not take for something it is not ──────────────────────────────────────

/// An entry of a kind a later build wrote is shown, and never undone — not taken for an edit.
#[test]
fn an_entry_of_an_unknown_kind_is_never_undone() {
    let mut db = LogDb::open_in_memory().unwrap();
    let a = rec("K1ABC", 1);
    write(
        &mut db,
        std::slice::from_ref(&a),
        Some(&note("c", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    db.conn_for_tests()
        .execute(
            "INSERT INTO qso_history (run_id, qso_id, kind, before, after) \
             SELECT run_id, qso_id, 9, '<NAME:3>Bob', '<NAME:3>Ann' FROM qso_history",
            [],
        )
        .unwrap();
    let h = db.history_of(id(1)).unwrap();
    assert_eq!(h[1].kind, Kind::Unknown(9));
    assert_eq!(h[1].kind.word(), "unknown");
    let mut now = a.clone();
    now.name = Some("Ann".into());
    assert_eq!(
        undo(&h[1], Some(&now), None),
        Err(CannotUndo::UnknownKind),
        "an edit would have put Bob back"
    );
}

/// The contacts a run added that another run has edited or merged into since are named, with the
/// latest such change; the fill job's country and an undo are not changes of the operator's.
#[test]
fn the_contacts_a_run_added_and_another_changed_since_are_named() {
    let mut db = LogDb::open_in_memory().unwrap();
    let (a, b, c) = (rec("K1ABC", 1), rec("K2XYZ", 2), rec("K3QQQ", 3));
    write(
        &mut db,
        &[a.clone(), b.clone(), c.clone()],
        Some(&note("import", Source::Import, Kind::Merge)),
    )
    .unwrap();
    let mut a2 = a.clone();
    a2.comment = Some("by hand".into());
    write(
        &mut db,
        &[a2],
        Some(&note("edit", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    let mut b2 = b.clone();
    b2.confirmed = true;
    write(
        &mut db,
        &[b2],
        Some(&note("lotw", Source::Lotw, Kind::Merge)),
    )
    .unwrap();
    let mut c2 = c.clone();
    c2.country = Some("Filled".into());
    write(
        &mut db,
        &[c2],
        Some(&note("fill", Source::Fill, Kind::Fill)),
    )
    .unwrap();
    let mut c3 = c.clone();
    c3.country = None;
    write(
        &mut db,
        &[c3],
        Some(&note("undo", Source::Undo, Kind::Edit)),
    )
    .unwrap();
    let later = db.history_changed_after("import").unwrap();
    let seq_of = |id: RecordId| db.history_of(id).unwrap().last().unwrap().seq;
    assert_eq!(
        later,
        HashMap::from([(id(1), seq_of(id(1))), (id(2), seq_of(id(2)))])
    );
}

/// A change another run made since counts whatever its kind — a contact added again under the
/// same id, a kind a later build wrote — but a delete does not (the contact is gone, and an undo
/// finds nothing to take out), nor does a change made BEFORE the run added the contact.
#[test]
fn a_contact_added_again_or_changed_by_a_later_build_since_counts_and_a_delete_does_not() {
    let mut db = LogDb::open_in_memory().unwrap();
    let (a, b, c) = (rec("K1ABC", 1), rec("K2XYZ", 2), rec("K3QQQ", 3));
    // Before the import: C edited by the operator.
    write(
        &mut db,
        std::slice::from_ref(&c),
        Some(&note("before", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    remove(
        &mut db,
        &[id(3)],
        Some(&note("gone", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    write(
        &mut db,
        &[a.clone(), b.clone(), c.clone()],
        Some(&note("import", Source::Import, Kind::Merge)),
    )
    .unwrap();
    // A deleted, then added again by another import.
    remove(
        &mut db,
        &[id(1)],
        Some(&note("del", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    write(
        &mut db,
        std::slice::from_ref(&a),
        Some(&note("again", Source::Import, Kind::Merge)),
    )
    .unwrap();
    // B changed by a later build, in a kind this one does not know.
    db.conn_for_tests()
        .execute(
            "INSERT INTO qso_history (run_id, qso_id, kind) \
             SELECT (SELECT id FROM history_run WHERE run = 'del'), ?1, 9",
            [id(2).to_string()],
        )
        .unwrap();
    // C deleted since: not a change that keeps it.
    remove(
        &mut db,
        &[id(3)],
        Some(&note("del2", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    let later = db.history_changed_after("import").unwrap();
    let seq_of = |id: RecordId| db.history_of(id).unwrap().last().unwrap().seq;
    assert_eq!(
        later,
        HashMap::from([(id(1), seq_of(id(1))), (id(2), seq_of(id(2)))])
    );
}

/// The contacts added after a change, each with its latest add: a contact an undo put back is
/// not one of them, nor is one added before.
#[test]
fn the_contacts_added_after_a_change_leave_out_the_ones_put_back() {
    let mut db = LogDb::open_in_memory().unwrap();
    let (a, b, c) = (rec("K1ABC", 1), rec("K2XYZ", 2), rec("K3QQQ", 3));
    write(
        &mut db,
        &[a.clone(), b.clone()],
        Some(&note("first", Source::Import, Kind::Merge)),
    )
    .unwrap();
    remove(
        &mut db,
        &[id(1), id(2)],
        Some(&note("purge", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    let purge = db.history_run("purge").unwrap();
    let first_delete = purge.iter().map(|e| e.seq).min().unwrap();
    // A put back by an undo of its delete; C logged new.
    let mut undo = note("undo", Source::Undo, Kind::Edit);
    undo.undoes.insert(id(1), first_delete);
    write(&mut db, std::slice::from_ref(&a), Some(&undo)).unwrap();
    write(
        &mut db,
        std::slice::from_ref(&c),
        Some(&note("logged", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    let seq_of = |id: RecordId| db.history_of(id).unwrap().last().unwrap().seq;
    assert_eq!(
        db.history_created_after(first_delete).unwrap(),
        HashMap::from([(id(3), seq_of(id(3)))])
    );
    assert_eq!(
        db.history_created_after(0).unwrap().len(),
        3,
        "the control: from the start, the first import's two and C"
    );
}

/// A new contact's entry keeps the upload stamps it arrived with, so an undo can tell one
/// uploaded since; a contact that arrived with none keeps nothing.
#[test]
fn a_new_contact_keeps_the_stamps_it_arrived_with() {
    let mut db = LogDb::open_in_memory().unwrap();
    let a = rec("K1ABC", 1);
    let mut b = rec("K2XYZ", 2);
    b.upload.qrz = Some(UploadStatus {
        outcome: UploadOutcome::Accepted,
        when_unix: 1_900_000_000,
        detail: None,
    });
    write(
        &mut db,
        &[a.clone(), b.clone()],
        Some(&note("import", Source::Import, Kind::Merge)),
    )
    .unwrap();
    let (ea, eb) = (
        db.history_of(id(1)).unwrap().remove(0),
        db.history_of(id(2)).unwrap().remove(0),
    );
    assert_eq!(ea.after, None);
    assert!(eb
        .after
        .as_deref()
        .is_some_and(|f| f.contains("APP_TEMPO_UL_QRZ")));
    assert!(!ea.uploaded_since(&a) && !eb.uploaded_since(&b));
    let mut a2 = a.clone();
    a2.upload.lotw = b.upload.qrz.clone();
    assert!(ea.uploaded_since(&a2), "uploaded since");
    let mut b2 = b.clone();
    b2.upload.qrz = None;
    assert!(eb.uploaded_since(&b2), "its stamp cleared since");
}

// ── how long it is kept ──────────────────────────────────────────────────────────────────────

/// ★ The trim takes out a delete's whole contact once it is older than the cut-off, a step of at
/// most `limit` at a time, and the runs it leaves with nothing; an edit, a new contact and a
/// recent delete are never touched, however old.
#[test]
fn the_trim_takes_old_whole_contacts_out_and_never_an_edit() {
    let mut db = LogDb::open_in_memory().unwrap();
    let at = |run: &str, source: Source, at_ms: i64| Note {
        at_ms,
        ..note(run, source, Kind::Edit)
    };
    const DAY: i64 = 86_400_000;
    let old = 1_000 * DAY;
    let rows: Vec<QsoRecord> = (1..=5).map(|n| rec(&format!("K{n}AA"), n)).collect();
    write(&mut db, &rows, Some(&at("added", Source::Import, old))).unwrap();
    let mut edited = rows[0].clone();
    edited.name = Some("Robert".into());
    write(&mut db, &[edited], Some(&at("edit", Source::Operator, old))).unwrap();
    remove(
        &mut db,
        &[id(1), id(2), id(3)],
        Some(&at("purge", Source::Operator, old + DAY)),
    )
    .unwrap();
    remove(
        &mut db,
        &[id(4)],
        Some(&at("recent", Source::Operator, old + 200 * DAY)),
    )
    .unwrap();
    let before = old + 90 * DAY;

    assert_eq!(db.trim_history(before, 2).unwrap(), 2, "a step of two");
    assert_eq!(db.history_run("purge").unwrap().len(), 1, "one still to go");
    assert_eq!(db.trim_history(before, 2).unwrap(), 1, "the last, and done");
    assert_eq!(db.trim_history(before, 2).unwrap(), 0);
    assert!(db.history_run("purge").unwrap().is_empty());
    let runs: Vec<String> = {
        let mut stmt = db
            .conn_for_tests()
            .prepare("SELECT run FROM history_run ORDER BY id")
            .unwrap();
        let rows = stmt.query_map([], |r| r.get(0)).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    };
    assert_eq!(
        runs,
        vec!["added", "edit", "recent"],
        "a run left with nothing goes with it"
    );
    assert_eq!(
        db.history_run("added").unwrap().len(),
        5,
        "new contacts kept"
    );
    let h = db.history_of(id(1)).unwrap();
    assert_eq!(
        h.iter().map(|e| e.kind).collect::<Vec<_>>(),
        vec![Kind::Create, Kind::Edit],
        "the edit's fields kept, the old delete gone"
    );
    assert_eq!(h[1].changes().len(), 1);
    let gone: Vec<RecordId> = db
        .history_deleted(10)
        .unwrap()
        .iter()
        .map(|e| e.qso_id)
        .collect();
    assert_eq!(gone, vec![id(4)], "the recent delete stays restorable");
}

/// The writer trims in steps of its own, between the changes it is handed, and answers how many
/// whole contacts it took out once none is left.
#[test]
fn the_writer_trims_in_steps_and_answers_how_many() {
    use crate::logbook::writer::LogWriter;
    let dir = std::env::temp_dir().join(format!("nexus-history-trim-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("log.sqlite3");
    let n = 2_500u64;
    {
        let mut db = LogDb::open(&path).unwrap();
        let rows: Vec<QsoRecord> = (1..=n).map(|i| rec(&format!("K{i}AA"), i)).collect();
        write(&mut db, &rows, None).unwrap();
        let ids: Vec<RecordId> = (1..=n).map(id).collect();
        let purge = Note {
            at_ms: 1_000,
            ..note("purge", Source::Operator, Kind::Edit)
        };
        remove(&mut db, &ids, Some(&purge)).unwrap();
    }
    let writer = LogWriter::start(LogDb::open(&path).unwrap());
    let answer = writer.trim_history(2_000);
    assert_eq!(
        answer
            .recv_timeout(std::time::Duration::from_secs(60))
            .unwrap(),
        Ok(n as usize),
        "more than one step's worth, all of it"
    );
    drop(writer);
    let db = LogDb::open(&path).unwrap();
    assert!(db.history_run("purge").unwrap().is_empty());
    assert!(db.history_deleted(10).unwrap().is_empty());
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

/// ★ The purges and imports a whole-run undo can still be asked for once the toast has gone: a
/// purge while a contact it took out is still out, an import while a contact it added is still
/// in. A single delete, a companion program's one logged contact, an edit, a run older than asked
/// and a run already undone are not among them.
#[test]
fn the_runs_still_to_undo_are_the_purges_and_imports_not_yet_undone() {
    let mut db = LogDb::open_in_memory().unwrap();
    let (a, b, c, d, e) = (
        rec("K1ABC", 1),
        rec("K2XYZ", 2),
        rec("K3QQQ", 3),
        rec("K4ONE", 4),
        rec("K5IMP", 5),
    );
    write(
        &mut db,
        &[b.clone(), e.clone()],
        Some(&note("import", Source::Import, Kind::Merge)),
    )
    .unwrap();
    write(
        &mut db,
        std::slice::from_ref(&d),
        Some(&note("companion", Source::Import, Kind::Merge)),
    )
    .unwrap();
    write(
        &mut db,
        &[a.clone(), c.clone()],
        Some(&note("logged", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    remove(
        &mut db,
        &[id(3)],
        Some(&note("one-delete", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    remove(
        &mut db,
        &[id(1), id(4)],
        Some(&note("purge", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    let runs = |db: &LogDb, since: i64| -> Vec<(String, String, u64)> {
        db.history_undoable_runs(since, 10)
            .unwrap()
            .into_iter()
            .map(|r| (r.run, r.source, r.contacts))
            .collect()
    };
    assert_eq!(
        runs(&db, 0),
        vec![
            ("purge".to_string(), "operator".to_string(), 2),
            ("import".to_string(), "import".to_string(), 2),
        ],
        "newest first; B and E are still in the log, A and D are still out"
    );
    assert!(runs(&db, 1_700_000_000_001).is_empty(), "older than asked");

    // The purge put back a contact at a time: once both are back it is done.
    write(
        &mut db,
        std::slice::from_ref(&a),
        Some(&note("undo-a", Source::Undo, Kind::Edit)),
    )
    .unwrap();
    assert_eq!(runs(&db, 0).len(), 2, "D is still out");
    write(
        &mut db,
        std::slice::from_ref(&d),
        Some(&note("undo-d", Source::Undo, Kind::Edit)),
    )
    .unwrap();
    // The import undone: every contact it added is out of the log again. The import leaves the
    // list; its undo, a whole run of two, takes its place, to be redone.
    remove(
        &mut db,
        &[id(2), id(5)],
        Some(&note("undo-import", Source::Undo, Kind::Edit)),
    )
    .unwrap();
    assert_eq!(
        runs(&db, 0),
        vec![("undo-import".to_string(), "undo".to_string(), 2)],
        "the import is off the list, the one-contact restores never on it"
    );
}

/// ★ A whole run's undo is listed while it can still be redone: the lasting Redo once its toast has
/// gone. An undo that took contacts out, while one is still out (its redo puts them back), and one
/// that put contacts back, while one is still in (its redo takes them out: `removes`). An undo of
/// one contact is not listed; nor is an undo whose contacts are all where its redo would put them.
#[test]
fn a_whole_runs_undo_is_listed_to_redo_while_it_can_be() {
    let mut db = LogDb::open_in_memory().unwrap();
    let (a, b, c) = (rec("K1ABC", 1), rec("K2XYZ", 2), rec("K3QQQ", 3));
    write(
        &mut db,
        &[a.clone(), b.clone()],
        Some(&note("import", Source::Import, Kind::Merge)),
    )
    .unwrap();
    write(
        &mut db,
        std::slice::from_ref(&c),
        Some(&note("logged", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    let runs = |db: &LogDb| -> Vec<(String, String, u64, bool)> {
        db.history_undoable_runs(0, 10)
            .unwrap()
            .into_iter()
            .map(|r| (r.run, r.source, r.contacts, r.removes))
            .collect()
    };
    let row =
        |run: &str, source: &str, removes: bool| (run.to_string(), source.to_string(), 2, removes);
    assert_eq!(
        runs(&db),
        vec![row("import", "import", true)],
        "the control"
    );

    remove(
        &mut db,
        &[id(1), id(2)],
        Some(&note("undo-import", Source::Undo, Kind::Edit)),
    )
    .unwrap();
    remove(
        &mut db,
        &[id(3)],
        Some(&note("undo-one", Source::Undo, Kind::Edit)),
    )
    .unwrap();
    assert_eq!(
        runs(&db),
        vec![row("undo-import", "undo", false)],
        "its redo puts the two back; the one-contact undo is not listed"
    );

    // Redone: the two are back. The redo is listed, and undoing it takes them out again.
    write(
        &mut db,
        &[a, b],
        Some(&note("redo", Source::Undo, Kind::Edit)),
    )
    .unwrap();
    assert_eq!(
        runs(&db),
        vec![row("redo", "undo", true), row("import", "import", true)],
        "the undo is done with: both its contacts are back"
    );
}

// ── a merged contest contact: its exchange's directed columns ─────────────────────────────────

/// A contact as the contest merge writes it into the general log: what the other station sent
/// in the contest's directed columns — a CQ zone a service also filled beside it, the state it
/// sent beside the one looked up from the call, and the grid of a contact merged before the
/// merge moved the grid into the record's own field.
fn merged(n: u64) -> QsoRecord {
    let mut r = rec("W1AW", n);
    r.grid = None;
    r.state = Some("NJ".into());
    r.extra.push(("CQZ".into(), "5".into()));
    r.extra.sort();
    r.contest = Some(Box::new(crate::logbook::ContestFields {
        session: "s1".into(),
        contest_id: "ARRL-VHF-JAN".into(),
        srx: Some(12),
        rcvd: vec![("GRID".into(), "FN31".into())],
        adif: vec![
            ("CQZ".into(), "3".into()),
            ("GRIDSQUARE".into(), "FN31".into()),
            ("STATE".into(), "NY".into()),
            ("CNTY".into(), "NY,ALBANY".into()),
        ],
        ..Default::default()
    }));
    r
}

/// ★ An undo of an edit of a merged contest contact puts it back WHOLE: its contest columns in
/// their order, its own fields and its passthrough each as they were — not the columns read back
/// into the fields, as the log's own file would give them.
#[test]
fn an_undo_of_a_merged_contest_contacts_edit_puts_it_back_whole() {
    let a = merged(1);
    let mut b = a.clone();
    b.name = Some("Hiram".into());
    let (_, h) = entries_after(&[(&a, &b)]);
    assert_eq!(h.len(), 1);
    match undo(&h[0], Some(&b), None).unwrap() {
        Undo::Revert { row, kept, .. } => {
            assert!(kept.is_empty(), "{kept:?}");
            assert_eq!(row, a, "the contact before the edit, whole");
        }
        other => panic!("{other:?}"),
    }
}

/// ★ A value moved from a contest column into the record's own field — and a contest STATE
/// replaced beside the field's own — is a change the history records, and its undo puts every
/// part back where it was.
#[test]
fn a_value_moved_out_of_a_contest_column_is_recorded_and_undone_exactly() {
    let a = merged(1);
    let mut moved = a.clone();
    moved.grid = Some("FN31".into());
    let c = moved.contest.as_deref_mut().unwrap();
    c.adif.retain(|(t, _)| t != "GRIDSQUARE");
    for r in [&a, &moved] {
        let out = adif_record_own_log(r);
        assert_eq!(
            out.matches("<GRIDSQUARE:").count(),
            1,
            "premise: one grid: {out}"
        );
        assert!(
            out.contains("<GRIDSQUARE:4>FN31"),
            "premise: the same grid: {out}"
        );
    }
    let mut state = a.clone();
    state.state = Some("PA".into());
    let c = state.contest.as_deref_mut().unwrap();
    c.adif[2].1 = "PA".into();
    for after in [&moved, &state] {
        let (_, h) = entries_after(&[(&a, after)]);
        assert_eq!(h.len(), 1, "recorded");
        match undo(&h[0], Some(after), None).unwrap() {
            Undo::Revert { row, kept, .. } => {
                assert!(kept.is_empty(), "{kept:?}");
                assert_eq!(&row, &a, "put back whole");
            }
            other => panic!("{other:?}"),
        }
    }
}

/// ★ An entry an older build recorded — its contest columns written under their plain tags — is
/// never undone into a field the contact now holds a contest column for: that field is kept, as
/// one changed since. Where the contact holds no such column, the entry undoes as it always did.
#[test]
fn an_older_builds_entry_on_a_contest_held_tag_is_kept_not_reverted() {
    let older = Entry {
        seq: 1,
        qso_id: id(1),
        kind: Kind::Edit,
        run: "old".into(),
        at_ms: 1,
        source: "operator".into(),
        undoes: None,
        before: Some("<STATE:2>NJ".into()),
        after: Some("<STATE:2>NY".into()),
    };
    // The contact's own state happens to hold the entry's value: the old build's NY was the
    // exchange's, so a revert would put a value in the field that was perhaps never the field's.
    let mut now = merged(1);
    now.state = Some("NY".into());
    assert_eq!(
        undo(&older, Some(&now), None),
        Err(CannotUndo::ChangedSince)
    );
    // Control: the same entry, on a contact with no contest STATE, undoes.
    let mut plain = rec("W1AW", 1);
    plain.state = Some("NY".into());
    match undo(&older, Some(&plain), None).unwrap() {
        Undo::Revert { row, kept, .. } => {
            assert!(kept.is_empty());
            assert_eq!(row.state.as_deref(), Some("NJ"));
        }
        other => panic!("{other:?}"),
    }
}

/// ★ A merged contest contact deleted and put back is the contact as it was, contest columns and
/// all.
#[test]
fn a_deleted_merged_contest_contact_is_put_back_whole() {
    let mut db = LogDb::open_in_memory().unwrap();
    let a = merged(1);
    write(&mut db, std::slice::from_ref(&a), None).unwrap();
    remove(
        &mut db,
        &[id(1)],
        Some(&note("d", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    let h = db.history_of(id(1)).unwrap();
    match undo(&h[0], None, None).unwrap() {
        Undo::Restore(back) => assert_eq!(back, a, "whole"),
        other => panic!("{other:?}"),
    }
}

/// ★ The history-only tag is the history's alone: no record an undo or a restore hands back
/// carries it, so none of the log's writers — the upload, the operator's own file (the log's
/// mirror, its exports and backups are that text), the LoTW batch — ever writes it. The history's
/// own view carries it: the control that the check can see it.
#[test]
fn no_writer_ever_writes_the_history_only_tag() {
    let a = merged(1);
    let mut b = a.clone();
    b.name = Some("Hiram".into());
    b.grid = Some("FN31".into());
    let (mut db, h) = entries_after(&[(&a, &b)]);
    let Ok(Undo::Revert { row: undone, .. }) = undo(&h[0], Some(&b), None) else {
        panic!("undoes")
    };
    remove(
        &mut db,
        &[id(1)],
        Some(&note("d", Source::Operator, Kind::Edit)),
    )
    .unwrap();
    let h = db.history_of(id(1)).unwrap();
    let Ok(Undo::Restore(restored)) = undo(h.last().unwrap(), None, None) else {
        panic!("restores")
    };
    assert!(
        h.iter().any(|e| [&e.before, &e.after].iter().any(|f| f
            .as_deref()
            .is_some_and(|f| f.contains(HISTORY_ONLY_PREFIX)))),
        "control: the history holds the tag"
    );
    assert!(history_adif(&a).contains(HISTORY_ONLY_PREFIX), "control");
    for r in [&a, &b, &undone, &restored] {
        assert!(!r
            .extra
            .iter()
            .any(|(k, _)| k.starts_with(HISTORY_ONLY_PREFIX)));
        for out in [
            crate::logbook::adif_record(r),
            adif_record_own_log(r),
            crate::logbook::adif_record_with_station(r, "KD9TAW", "EN52"),
        ] {
            assert!(!out.contains(HISTORY_ONLY_PREFIX), "{out}");
        }
    }
}

/// An NAQP contact as the contest merge wrote it before the merge filled the record's own name:
/// the name the station sent only in the exchange, the state it sent beside the one looked up from
/// its call.
fn naqp(n: u64) -> QsoRecord {
    let mut r = rec("W1AW", n);
    r.name = None;
    r.state = Some("NJ".into());
    r.contest = Some(Box::new(crate::logbook::ContestFields {
        session: "naqp".into(),
        contest_id: "NAQP-CW".into(),
        adif: vec![("NAME".into(), "BOB".into()), ("STATE".into(), "NY".into())],
        ..Default::default()
    }));
    r
}

/// The operator's State edit as the Logbook makes it on a merged contact: the field and the
/// exchange's STATE alike.
fn state_edited(r: &QsoRecord, state: &str) -> QsoRecord {
    let mut s = r.clone();
    s.state = Some(state.into());
    for (t, v) in &mut s.contest.as_deref_mut().unwrap().adif {
        if t == "STATE" {
            *v = state.into();
        }
    }
    s
}

/// What a contact's record answers for its name and state, and its contest columns.
fn read_back(r: &QsoRecord) -> (Option<String>, Option<String>, Vec<(String, String)>) {
    (
        crate::logbook::record_field(r, "NAME").map(str::to_string),
        crate::logbook::record_field(r, "STATE").map(str::to_string),
        r.contest
            .as_deref()
            .map(|c| c.adif.clone())
            .unwrap_or_default(),
    )
}

/// ★ The one-time copy of a merged contact's received name, then a State edit, then an Undo of
/// the copy: the name is still the contact's (in its own field or its contest column, never in
/// neither), the State edit stays, and nothing is reported kept that the undo did not keep.
#[test]
fn undoing_the_contest_column_copy_after_a_state_edit_keeps_the_name() {
    let a = naqp(1);
    let copied = crate::logbook::with_contest_columns_copied(&a).expect("copies");
    let edited = state_edited(&copied, "PA");
    let (_, h) = entries_after(&[(&a, &copied), (&copied, &edited)]);
    match undo(&h[0], Some(&edited), None).unwrap() {
        Undo::Revert { row, kept, .. } => {
            assert!(kept.is_empty(), "{kept:?}");
            assert_eq!(
                read_back(&row),
                (
                    Some("BOB".into()),
                    Some("PA".into()),
                    vec![
                        ("NAME".to_string(), "BOB".to_string()),
                        ("STATE".to_string(), "PA".to_string())
                    ]
                )
            );
            assert_eq!(row.name, None, "the copy is undone");
            assert_eq!(row.state.as_deref(), Some("PA"), "the State edit stays");
        }
        other => panic!("{other:?}"),
    }
}

/// ★ An entry that took a column out (as an earlier build's move did), then a change of another
/// column, then its Undo: the column it took comes back, the later change stays — each column on
/// its own, never the contest columns as one.
#[test]
fn an_undo_reverts_the_contest_columns_one_tag_at_a_time() {
    let a = naqp(1);
    let mut moved = a.clone();
    moved.name = Some("BOB".into());
    moved
        .contest
        .as_deref_mut()
        .unwrap()
        .adif
        .retain(|(t, _)| t != "NAME");
    let edited = state_edited(&moved, "PA");
    let (_, h) = entries_after(&[(&a, &moved), (&moved, &edited)]);
    match undo(&h[0], Some(&edited), None).unwrap() {
        Undo::Revert { row, kept, .. } => {
            assert!(kept.is_empty(), "{kept:?}");
            assert_eq!(row.name, None);
            assert_eq!(
                read_back(&row),
                (
                    Some("BOB".into()),
                    Some("PA".into()),
                    vec![
                        ("NAME".to_string(), "BOB".to_string()),
                        ("STATE".to_string(), "PA".to_string())
                    ]
                ),
                "the name back in its column, in its place; the later State edit kept"
            );
        }
        other => panic!("{other:?}"),
    }
}

/// ★ A State edit, then a change of another contest column, then an Undo of the State edit:
/// both the record's own state and the exchange's go back, and the later column stays.
#[test]
fn undoing_a_state_edit_after_another_column_changed_reverts_both_states() {
    let a = naqp(1);
    let edited = state_edited(&a, "PA");
    let mut later = edited.clone();
    later
        .contest
        .as_deref_mut()
        .unwrap()
        .adif
        .push(("CQZ".into(), "5".into()));
    let (_, h) = entries_after(&[(&a, &edited), (&edited, &later)]);
    match undo(&h[0], Some(&later), None).unwrap() {
        Undo::Revert { row, kept, .. } => {
            assert!(kept.is_empty(), "{kept:?}");
            assert_eq!(row.state.as_deref(), Some("NJ"), "the field");
            assert_eq!(
                row.contest.as_deref().unwrap().adif,
                [
                    ("NAME".to_string(), "BOB".to_string()),
                    ("STATE".to_string(), "NY".to_string()),
                    ("CQZ".to_string(), "5".to_string()),
                ],
                "the exchange's state, and the later column"
            );
        }
        other => panic!("{other:?}"),
    }
}

/// ★ A contest column the entry changed and a later change moved again keeps the later value,
/// named as the History shows it, not by the history's own tag.
#[test]
fn a_contest_column_changed_since_is_kept_and_named() {
    let a = naqp(1);
    let edited = state_edited(&a, "PA");
    let mut later = edited.clone();
    later.contest.as_deref_mut().unwrap().adif[1].1 = "OH".into();
    let (_, h) = entries_after(&[(&a, &edited), (&edited, &later)]);
    match undo(&h[0], Some(&later), None).unwrap() {
        Undo::Revert { row, kept, .. } => {
            assert_eq!(kept, ["STATE (contest)".to_string()]);
            assert_eq!(row.state.as_deref(), Some("NJ"));
            assert_eq!(read_back(&row).1.as_deref(), Some("OH"));
        }
        other => panic!("{other:?}"),
    }
}

/// ★ The operator's own edit of a field a contest column also holds — a grid typed on a contact
/// that keeps the grid its exchange carried — undoes: this build's entry carries the unchanged
/// contest columns as its mark, so it is never taken for an older build's.
#[test]
fn undoing_the_operators_grid_edit_beside_a_contest_grid() {
    let a = merged(1);
    let mut b = a.clone();
    b.grid = Some("FN20".into());
    let (_, h) = entries_after(&[(&a, &b)]);
    match undo(&h[0], Some(&b), None).unwrap() {
        Undo::Revert { row, kept, .. } => {
            assert!(kept.is_empty(), "{kept:?}");
            assert_eq!(row, a, "the grid edit undone, the columns as they were");
        }
        other => panic!("{other:?}"),
    }
}

/// ★ A record an older build handed back with the history-only tag in its passthrough — a VHF
/// contact whose only contest column was its grid, put back or undone by a build that does not
/// know the tag: no writer ever writes the tag, and a read of the record (from ADIF, and from the
/// store) puts its columns back where they belong and drops the tag.
#[test]
fn a_stray_history_only_tag_is_never_written_and_heals_on_read() {
    let mut stray = rec("W1AW", 1);
    stray.grid = None;
    stray.contest = Some(Box::new(crate::logbook::ContestFields {
        session: "vhf".into(),
        contest_id: "ARRL-VHF-JAN".into(),
        ..Default::default()
    }));
    stray
        .extra
        .push((CONTEST_COLUMNS_TAG.into(), "GRIDSQUARE::FN31".into()));
    stray.extra.sort();
    for out in [
        crate::logbook::adif_record(&stray),
        adif_record_own_log(&stray),
        crate::logbook::adif_record_with_station(&stray, "KD9TAW", "EN52"),
    ] {
        assert!(!out.contains(HISTORY_ONLY_PREFIX), "{out}");
        assert!(
            out.contains("<APP_OTHERLOG_X:5>kept!"),
            "control: the passthrough is written: {out}"
        );
    }
    let healed = |r: &QsoRecord| {
        assert!(
            !r.extra
                .iter()
                .any(|(k, _)| k.starts_with(HISTORY_ONLY_PREFIX)),
            "{:?}",
            r.extra
        );
        assert!(r.extra.iter().any(|(k, _)| k == "APP_OTHERLOG_X"));
        assert_eq!(
            r.contest.as_deref().map(|c| c.adif.clone()),
            Some(vec![("GRIDSQUARE".to_string(), "FN31".to_string())])
        );
        assert_eq!(crate::logbook::record_field(r, "GRIDSQUARE"), Some("FN31"));
    };
    // As an older build's own log file holds it.
    let mut text = adif_record_own_log(&stray);
    let at = text.rfind("<EOR>").unwrap();
    text.insert_str(
        at,
        &crate::logbook::field(CONTEST_COLUMNS_TAG, "GRIDSQUARE::FN31"),
    );
    healed(&parse_adif(&text).remove(0));
    // As an older build stored it.
    let mut db = LogDb::open_in_memory().unwrap();
    write(&mut db, std::slice::from_ref(&stray), None).unwrap();
    let back = db.rows_by_ids(&[id(1)]).unwrap();
    healed(&back[0]);
}

/// ★ The one-time copy of contest columns into their fields is listed for a whole-run undo — even
/// over one contact — while a contact it changed is in the log and that change is not undone; an
/// ordinary fill run, of any size, never is.
#[test]
fn the_contest_column_copy_is_listed_until_it_is_undone_and_a_fill_never() {
    let mut db = LogDb::open_in_memory().unwrap();
    let (a, b, c) = (naqp(1), rec("K2XYZ", 2), rec("K3QQQ", 3));
    write(&mut db, &[a.clone(), b.clone(), c.clone()], None).unwrap();
    let copied = crate::logbook::with_contest_columns_copied(&a).unwrap();
    write(
        &mut db,
        std::slice::from_ref(&copied),
        Some(&note("copy", Source::ContestColumns, Kind::Fill)),
    )
    .unwrap();
    let (mut b2, mut c2) = (b.clone(), c.clone());
    b2.state = Some("NY".into());
    c2.state = Some("OH".into());
    write(
        &mut db,
        &[b2, c2],
        Some(&note("fill", Source::Fill, Kind::Fill)),
    )
    .unwrap();
    let runs = |db: &LogDb| -> Vec<(String, String, u64)> {
        db.history_undoable_runs(0, 10)
            .unwrap()
            .into_iter()
            .map(|r| (r.run, r.source, r.contacts))
            .collect()
    };
    assert_eq!(
        runs(&db),
        [("copy".to_string(), "contest-columns".to_string(), 1)]
    );
    // Undone: off the list.
    let entry = db.history_run("copy").unwrap().remove(0);
    let Ok(Undo::Revert { row, .. }) = undo(&entry, Some(&copied), None) else {
        panic!("undoes")
    };
    let mut undo_note = note("undo-copy", Source::Undo, Kind::Edit);
    undo_note.undoes.insert(id(1), entry.seq);
    write(&mut db, &[row], Some(&undo_note)).unwrap();
    assert!(runs(&db).is_empty(), "{:?}", runs(&db));
}
