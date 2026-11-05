//! #163: the one-click corrected LoTW re-upload of past satellite QSOs, end to end — the real
//! `fix_sat_bands` and `lotw_upload_batch_with` with a stand-in TQSL, which is the body of the
//! `lotw_fix_sat_bands` command. Every way the upload can end without stamping a rewritten row
//! (the app closed mid-run, a later location's run that never starts, an edit mid-run, a rewrite
//! that stops part-way, no TQSL at all) leaves it owed to the ordinary Upload to LoTW, marked
//! "corrected, not sent yet"; a successful run stamps each row once; a LoTW-confirmed contact is
//! never touched. Adopted from the data-integrity review of the change.
use super::durable_command_tests::{by_command, engine_on_store};
use super::*;
use crate::remote_service::stored_log_tests::StoredLog;
use tempo_app::settings::StationProfile;
use tempo_core::logbook::{LogOp, QsoRecord, RecordId, UploadOutcome, UploadService, UploadStatus};

/// Old-shape satellite contacts (downlink band and FREQ, sent and accepted by LoTW), K1SAT
/// under K2DEF (Home), K2SAT under the Portable profile, K3SAT LoTW-confirmed.
fn station(tag: &str) -> (PathBuf, SharedEngine) {
    let (dir, engine) = engine_on_store(tag, 0);
    {
        let mut eng = engine_lock(&engine);
        let mut s = eng.settings().clone();
        s.lotw_station_location = "Home".into();
        eng.apply_settings(s);
        eng.save_station_profile(StationProfile {
            name: "Portable".into(),
            call: "K2DEF/P".into(),
            grid: "FN42".into(),
            lotw_location: "Portable".into(),
            ..StationProfile::default()
        })
        .expect("profile");
    }
    let mut adif = String::new();
    for (i, (call, station, grid, extra)) in [
        ("K1SAT", "K2DEF", "FN31", ""),
        ("K2SAT", "K2DEF/P", "FN42", ""),
        ("K3SAT", "K2DEF", "FN31", "<LOTW_QSL_RCVD:1>Y"),
    ]
    .into_iter()
    .enumerate()
    {
        adif.push_str(&format!(
            "<CALL:5>{call}<BAND:4>70cm<FREQ:7>436.795<MODE:2>FM<PROP_MODE:3>SAT\
             <SAT_NAME:5>SO-50<QSO_DATE:8>20261020<TIME_ON:6>12000{i}\
             <STATION_CALLSIGN:{}>{station}<MY_GRIDSQUARE:4>{grid}{extra}<EOR>\n",
            station.len()
        ));
    }
    tempo_app::logwrite::import_adif(&engine, &adif)
        .0
        .expect("imported");
    for call in ["K1SAT", "K2SAT", "K3SAT"] {
        let id = id_of(&engine, call);
        let (ok, d) = by_command(
            &engine,
            id,
            LogOp::Stamp {
                id,
                service: UploadService::Lotw,
                status: UploadStatus {
                    outcome: UploadOutcome::Accepted,
                    when_unix: 1_704_000_000,
                    detail: None,
                },
            },
        );
        assert!(ok);
        d.wait(tempo_app::logstore::DURABLE_WAIT).unwrap();
    }
    (dir, engine)
}

fn id_of(engine: &SharedEngine, call: &str) -> RecordId {
    engine
        .stored_log()
        .iter()
        .find(|r| r.call == call)
        .and_then(|r| r.id)
        .expect("logged")
}
fn row(engine: &SharedEngine, call: &str) -> QsoRecord {
    QsoRecord::clone(
        engine
            .stored_log()
            .iter()
            .find(|r| r.call == call)
            .expect("logged"),
    )
}
/// The row as the database holds it, on a connection of its own (what a restart reads).
fn on_disk(dir: &Path, call: &str) -> QsoRecord {
    let db = tempo_core::logbook::migrate::database_path(&dir.join("log.adi"));
    tempo_core::logbook::sqlite::LogDb::open(&db)
        .and_then(|d| d.load_all())
        .expect("read the store")
        .into_iter()
        .find(|r| r.call == call)
        .expect("on disk")
}
fn offered(engine: &SharedEngine) -> Vec<RecordId> {
    let rows = engine_lock(engine).log_rows();
    tempo_app::station::lotw_sat_band_fixes(&rows).unwrap()
}
fn owed(engine: &SharedEngine) -> Vec<RecordId> {
    let rows = engine_lock(engine).log_rows();
    tempo_app::station::lotw_unsent_ids(&rows).unwrap()
}
/// `lotw_fix_sat_bands`'s body, with TQSL named.
fn fix_and_upload(
    engine: &SharedEngine,
    ids: &[RecordId],
    tqsl: impl FnMut(&str, &[String]) -> Result<(i32, String), String>,
) -> Result<UploadReportDto, String> {
    let (fixed, durability) = tempo_app::logwrite::fix_sat_bands(engine, ids, now_unix());
    let fixed = fixed?;
    durability
        .wait(tempo_app::logstore::DURABLE_WAIT)
        .map_err(durability_failed)?;
    lotw_upload_batch_with(engine, LotwPick::Ids(fixed), true, tqsl)
}
fn batch_text(args: &[String]) -> String {
    std::fs::read_to_string(args.last().unwrap()).unwrap()
}
fn outcome(r: &QsoRecord) -> Option<UploadOutcome> {
    r.upload.lotw.as_ref().map(|s| s.outcome)
}

/// Success: one TQSL run per location, each corrected row on its uplink band, stamped Pending
/// once; never offered again, never in the next upload; the confirmed row untouched.
#[test]
fn success_stamps_each_once_and_confirmed_untouched() {
    let (dir, engine) = station("satfix-ok");
    let ids = offered(&engine);
    assert_eq!(
        ids,
        vec![id_of(&engine, "K1SAT"), id_of(&engine, "K2SAT")],
        "the confirmed one is not offered"
    );
    let confirmed_before = row(&engine, "K3SAT");
    let mut runs = Vec::new();
    let report = fix_and_upload(&engine, &ids, |_, args| {
        let b = batch_text(args);
        // During the run the rows are already owed (stamped not sent).
        for c in ["K1SAT", "K2SAT"].into_iter().filter(|c| b.contains(c)) {
            assert!(tempo_app::station::owed_to_lotw(&row(&engine, c)));
        }
        assert!(
            b.contains("<BAND:2>2m") && b.contains("<BAND_RX:4>70cm"),
            "{b}"
        );
        assert!(!b.contains("K3SAT"));
        runs.push(b);
        Ok((0, String::new()))
    })
    .expect("ran");
    assert_eq!(runs.len(), 2, "Portable and Home");
    assert_eq!((report.dispatched, report.outcome.as_str()), (2, "pending"));
    for c in ["K1SAT", "K2SAT"] {
        assert_eq!(
            outcome(&row(&engine, c)),
            Some(UploadOutcome::Pending),
            "{c}"
        );
        assert_eq!(
            outcome(&on_disk(&dir, c)),
            Some(UploadOutcome::Pending),
            "{c} disk"
        );
        assert!((row(&engine, c).freq_mhz - 145.85).abs() < 1e-9);
    }
    assert!(offered(&engine).is_empty(), "not offered again");
    assert!(owed(&engine).is_empty(), "not in the next upload");
    let next = lotw_upload_batch_with(&engine, LotwPick::Unsent, true, |_, _| {
        panic!("nothing owed")
    })
    .unwrap();
    assert_eq!(next.outcome, "none");
    assert_eq!(
        row(&engine, "K3SAT"),
        confirmed_before,
        "confirmed row untouched"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The app closed after the rewrite, before TQSL stamped: what a restart reads is rewritten
/// AND owed, so the ordinary upload sends the corrected copy.
#[test]
fn crash_between_rewrite_and_stamp_leaves_it_owed() {
    let (dir, engine) = station("satfix-crash");
    let ids = offered(&engine);
    let (fixed, d) = tempo_app::logwrite::fix_sat_bands(&engine, &ids, now_unix());
    assert_eq!(fixed.unwrap(), ids);
    d.wait(tempo_app::logstore::DURABLE_WAIT).unwrap();
    // "crash": no upload ever runs. Read the disk as a restart would.
    for c in ["K1SAT", "K2SAT"] {
        let r = on_disk(&dir, c);
        assert_eq!(r.freq_rx_mhz, Some(436.795), "{c} rewritten on disk");
        assert!(
            tempo_app::station::owed_to_lotw(&r),
            "{c} owed on disk: {:?}",
            r.upload.lotw
        );
        assert!(tempo_core::lotw_sat_fix::sat_band_fix(&r).is_none());
    }
    let mut sent = String::new();
    let report = lotw_upload_batch_with(&engine, LotwPick::Unsent, true, |_, args| {
        sent.push_str(&batch_text(args));
        Ok((0, String::new()))
    })
    .unwrap();
    assert_eq!(report.dispatched, 2);
    assert!(sent.contains("K1SAT") && sent.contains("K2SAT") && sent.contains("<BAND:2>2m"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Multi-location: the second run cannot start. Its row stays owed (and the log line saying
/// so is true); the first is Pending.
#[test]
fn later_run_that_never_starts_leaves_its_row_owed() {
    let (dir, engine) = station("satfix-later");
    let ids = offered(&engine);
    let mut ran = 0;
    let report = fix_and_upload(&engine, &ids, |_, _| {
        ran += 1;
        if ran == 2 {
            Err("TQSL did not answer in time".into())
        } else {
            Ok((0, String::new()))
        }
    })
    .expect("the earlier run's report");
    assert_eq!(report.unsent_after_error, 1);
    let outs: Vec<_> = ["K1SAT", "K2SAT"]
        .iter()
        .map(|c| outcome(&row(&engine, c)))
        .collect();
    assert!(outs.contains(&Some(UploadOutcome::Pending)), "{outs:?}");
    assert_eq!(
        owed(&engine).len(),
        1,
        "the unstarted run's row is owed: {outs:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The operator edits a row while TQSL runs: the batch skips stamping it; it stays owed and
/// the next upload sends it (in its corrected shape).
#[test]
fn edit_mid_run_leaves_it_owed() {
    let (dir, engine) = station("satfix-edit");
    let k1 = id_of(&engine, "K1SAT");
    let report = fix_and_upload(&engine, &[k1], |_, _| {
        let mut r = row(&engine, "K1SAT");
        r.comment = Some("nice pass".into());
        assert!(
            by_command(
                &engine,
                k1,
                LogOp::Edit {
                    id: k1,
                    rec: Box::new(r)
                }
            )
            .0
        );
        Ok((0, String::new()))
    })
    .unwrap();
    assert_eq!(report.skipped_edited, 1);
    let r = row(&engine, "K1SAT");
    assert!(tempo_app::station::owed_to_lotw(&r), "{:?}", r.upload.lotw);
    assert_eq!(owed(&engine), vec![k1]);
    assert!(offered(&engine).len() == 1, "only K2SAT is still offered");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The rewrite stops part-way (the command returns its Err and never uploads): the rows
/// already rewritten are owed; the rest are untouched and still offered.
#[test]
fn partial_rewrite_leaves_the_prefix_owed_and_the_rest_offered() {
    let (dir, engine) = station("satfix-partial");
    let ids = offered(&engine);
    // The loop's state after it returns early at ids[1]: ids[0] alone made.
    let (fixed, d) = tempo_app::logwrite::fix_sat_bands(&engine, &ids[..1], now_unix());
    assert_eq!(fixed.unwrap(), vec![ids[0]]);
    d.wait(tempo_app::logstore::DURABLE_WAIT).unwrap();
    assert_eq!(owed(&engine), vec![ids[0]]);
    assert_eq!(offered(&engine), vec![ids[1]]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// No TQSL at all: the first run is the error; both rows are owed.
#[test]
fn no_tqsl_leaves_all_owed() {
    let (dir, engine) = station("satfix-notqsl");
    let ids = offered(&engine);
    let e = fix_and_upload(&engine, &ids, |_, _| Err("TQSL is not installed".into()));
    assert!(e.is_err());
    assert_eq!(owed(&engine).len(), 2);
    assert!(offered(&engine).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
