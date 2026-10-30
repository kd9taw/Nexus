//! The grading of logged contacts' paths held to its two contracts: a new contact gains its grade
//! and heading from what this module already holds, and grading the existing log never writes a
//! contact, never takes the Engine lock, and resumes where it stopped.

use super::*;
use crate::SharedEngine;
use propagation::PathPredictor;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::time::Duration;
use tempo_app::engine::{engine_lock, Engine};
use tempo_app::test_util::StoredLog;
use tempo_core::logbook::{adif_header, adif_record_own_log, parse_adif};

fn serial() -> MutexGuard<'static, ()> {
    let g = lock(&TEST_SERIAL);
    forget_grades();
    lock(&OUTLOOKS).clear();
    g
}

struct Dir(PathBuf);
impl Dir {
    fn new(tag: &str) -> Dir {
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "nexus-pathgrades-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("scratch");
        Dir(p)
    }
    fn log(&self) -> PathBuf {
        self.0.join("log.adi")
    }
    fn grades(&self) -> PathBuf {
        self.0.join("path_grades.jsonl")
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const NOW: i64 = 1_778_150_000; // 2026-05-07 ~10:33Z

fn adif_date(t: i64) -> (String, String) {
    // Days since 1970-01-01 to a civil date (proleptic Gregorian).
    let z = t.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let s = t.rem_euclid(86_400);
    (
        format!("{y:04}{m:02}{d:02}"),
        format!("{:02}{:02}{:02}", s / 3600, s % 3600 / 60, s % 60),
    )
}

fn tag(name: &str, v: &str) -> String {
    format!("<{name}:{}>{v}", v.len())
}

/// A contact from FN42 to `grid` on `band` at `t`, with the flux it logged, if any.
fn qso(call: &str, grid: &str, band: &str, t: i64, sfi: Option<&str>) -> QsoRecord {
    let (d, tm) = adif_date(t);
    let mut s = String::from("<EOH>");
    s += &tag("CALL", call);
    s += &tag("QSO_DATE", &d);
    s += &tag("TIME_ON", &tm);
    s += &tag("BAND", band);
    s += &tag("MODE", "FT8");
    s += &tag("GRIDSQUARE", grid);
    s += &tag("MY_GRIDSQUARE", "FN42");
    if let Some(f) = sfi {
        s += &tag("SFI", f);
    }
    s += "<EOR>";
    parse_adif(&s).remove(0)
}

/// A varied log: logged flux (any age), recent without flux, old without flux (ungradable),
/// no grid (ungradable), a VHF band (ungradable), and repeats of one cell.
fn log_records() -> Vec<QsoRecord> {
    let year = 365 * 86_400;
    vec![
        qso("VK2AA", "QF56", "20m", NOW - 3600, Some("150")),
        qso("VK2AB", "QF56", "20m", NOW - 3600 + 60, Some("150")),
        qso("JA1AA", "PM95", "20m", NOW - 86_400, None),
        qso("G4AAA", "IO91", "40m", NOW - 2 * 86_400, None),
        qso("ZS6AA", "KG44", "15m", NOW - 5 * year, Some("90")),
        qso("PY2AA", "GG66", "10m", NOW - 6 * year, None),
        qso("W1AW", "FN31", "2m", NOW - 3600, Some("150")),
        {
            let mut r = qso("K1NOG", "FN31", "20m", NOW - 3600, Some("150"));
            r.grid = None;
            r
        },
    ]
}

fn on_store(d: &Dir, records: &[QsoRecord]) -> SharedEngine {
    let mut adif = adif_header();
    for r in records {
        adif.push_str(&adif_record_own_log(r));
    }
    std::fs::write(d.log(), adif).expect("log.adi");
    let opened = tempo_app::logstore::open(
        &d.log(),
        Arc::new(|_| tempo_core::logbook::sqlite::Resolved::default()),
        None,
    )
    .expect("the store opens");
    let mut e = Engine::new("K2DEF", "FN42", 0);
    e.attach_log_store(opened);
    Arc::new(Mutex::new(e))
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// Every contact the store holds, each as the operator's own ADIF (every field, the passthrough
/// and the id), plus every file in the log's folder.
fn fingerprint(d: &Dir, engine: &SharedEngine) -> (Vec<String>, Vec<Vec<u8>>) {
    let mut rows: Vec<String> = engine
        .stored_records()
        .iter()
        .map(|r| format!("{:?}|{}", r.id, adif_record_own_log(r)))
        .collect();
    rows.sort();
    // Every file in the log's folder but the grades' own: the database, its WAL and shared
    // memory, `log.adi` and any backup.
    let mut paths: Vec<PathBuf> = walk(&d.0)
        .into_iter()
        .filter(|p| p.file_name().is_some_and(|n| n != "path_grades.jsonl"))
        .collect();
    paths.sort();
    let files = paths
        .iter()
        .map(|p| {
            let mut b = p.to_string_lossy().into_owned().into_bytes();
            b.extend(std::fs::read(p).unwrap_or_default());
            b
        })
        .collect();
    (rows, files)
}

/// Let the store's own work after opening (the writer, the `log.adi` mirror's first picture and
/// its backup) finish: flushed, then the fingerprint the same four reads running. Without it the
/// store's own writes can land during a pass and read as the pass's.
fn settle(d: &Dir, engine: &SharedEngine) {
    let _ = engine_lock(engine).flush_log_store(Duration::from_secs(60));
    let (mut last, mut same) = (None, 0);
    for _ in 0..120 {
        let now = fingerprint(d, engine);
        if last.as_ref() == Some(&now) {
            same += 1;
            if same >= 4 {
                return;
            }
        } else {
            (last, same) = (Some(now), 0);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    panic!("the store never settled");
}

#[test]
fn grading_the_existing_log_never_writes_a_contact() {
    let _s = serial();
    let d = Dir::new("nowrite");
    let engine = on_store(&d, &log_records());
    settle(&d, &engine);
    let before = fingerprint(&d, &engine);
    let job = capture(&engine, NOW, Some(80.0));
    let status = run_pass(job, &d.grades(), &AtomicBool::new(false)).expect("a pass");
    let after = fingerprint(&d, &engine);
    assert_eq!(before.0.len(), 8);
    assert_eq!(before, after, "the pass changed the log");
    // It did grade: the five gradable contacts, in four cells (two share one).
    assert_eq!(
        (status.contacts, status.graded, status.ungradable),
        (8, 5, 3)
    );
    assert_eq!(status.cells_total, 4);
    assert!(std::fs::metadata(d.grades()).is_ok_and(|m| m.len() > 0));
    // POSITIVE CONTROL: the fingerprint sees a change to one contact's passthrough.
    {
        let mut eng = engine_lock(&engine);
        let r = qso("ON4AA", "JO20", "20m", NOW - 60, None);
        eng.log_qso(r);
    }
    assert_ne!(
        fingerprint(&d, &engine).0,
        before.0,
        "the fingerprint is blind"
    );
}

#[test]
fn the_pass_runs_with_the_engine_lock_held_elsewhere() {
    let _s = serial();
    let d = Dir::new("nolock");
    let engine = on_store(&d, &log_records());
    let job = capture(&engine, NOW, Some(80.0));
    // Hold the Engine lock for the whole pass: a pass that needed it would never finish.
    let held = engine_lock(&engine);
    let path = d.grades();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(run_pass(job, &path, &AtomicBool::new(false)).map(|s| s.cells_done));
    });
    let done = rx
        .recv_timeout(Duration::from_secs(120))
        .expect("the pass waited for the Engine lock");
    drop(held);
    assert_eq!(done.expect("a pass"), 4);
}

#[test]
fn a_pass_resumes_and_never_grades_a_cell_twice() {
    let _s = serial();
    let d = Dir::new("resume");
    let engine = on_store(&d, &log_records());
    // Stopped before its first chunk: nothing graded, nothing kept.
    let s = run_pass(
        capture(&engine, NOW, Some(80.0)),
        &d.grades(),
        &AtomicBool::new(true),
    )
    .unwrap();
    assert_eq!((s.cells_total, s.cells_done), (4, 0));
    // A full pass, then a restart (memory gone) and a torn line left by a kill mid-write.
    let s = run_pass(
        capture(&engine, NOW, Some(80.0)),
        &d.grades(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(s.cells_done, 4);
    let kept = std::fs::read_to_string(d.grades()).unwrap();
    assert_eq!(kept.lines().count(), 5, "a header and four cells");
    std::fs::write(d.grades(), format!("{kept}{{\"k\":\"torn")).unwrap();
    forget_grades();
    let s = run_pass(
        capture(&engine, NOW, Some(80.0)),
        &d.grades(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(s.cells_total, 0, "every cell came back from the file");
    // Other inputs (the station's power) make the file's cells another station's: graded again.
    let mut job = capture(&engine, NOW, Some(80.0));
    job.power_w = Some(5.0);
    let s = run_pass(job, &d.grades(), &AtomicBool::new(false)).unwrap();
    assert_eq!(s.cells_total, 4);
    assert!(load_file(&d.grades(), "p533|None|0").is_none());
}

#[test]
fn the_logbook_shows_the_stamp_made_for_the_contact_as_it_is_else_the_model() {
    let _s = serial();
    let d = Dir::new("view");
    let engine = on_store(&d, &log_records());
    run_pass(
        capture(&engine, NOW, Some(80.0)),
        &d.grades(),
        &AtomicBool::new(false),
    )
    .unwrap();
    let base = GradeAsk::of(&log_records()[0]);
    let modelled = view_for(&base, "FN42", NOW, Some(80.0)).expect("graded");
    assert_eq!(modelled.tier, GradeTier::Modelled);
    assert_eq!(modelled.ssn_source, Some(SsnSource::Logged));
    // Stamped for this grid and band: the stamp is shown.
    let mut stamped = base.clone();
    stamped.extra.extend([
        (path_stamp::PATH_FIELD.to_string(), "S".to_string()),
        (
            path_stamp::PATH_CONF_FIELD.to_string(),
            "LIKELY".to_string(),
        ),
        (
            path_stamp::PATH_WHY_FIELD.to_string(),
            "MARGIN_S,HEARD_S:2;QF56/20m".to_string(),
        ),
    ]);
    let v = view_for(&stamped, "FN42", NOW, Some(80.0)).unwrap();
    assert_eq!(
        (v.tier, v.verdict, v.confidence),
        (
            GradeTier::Logged,
            RouteVerdict::Short,
            Some(RouteConfidence::Likely)
        )
    );
    assert_eq!(v.why, ["MARGIN_S", "HEARD_S:2"]);
    // Stamped for another band (the contact was edited since): the model's, said as such.
    let mut stale = stamped.clone();
    stale.band = "40m".into();
    let v = view_for(&stale, "FN42", NOW, Some(80.0));
    // 40 m was never graded here, so nothing — and never the stale stamp.
    assert!(v.is_none() || v.as_ref().is_some_and(|v| v.tier == GradeTier::Modelled));
    stale.band = "20m".into();
    stale.grid = Some("QF57".into());
    stale.extra.retain(|(k, _)| k != path_stamp::PATH_WHY_FIELD);
    stale.extra.push((
        path_stamp::PATH_WHY_FIELD.into(),
        "MARGIN_S;QF56/20m".into(),
    ));
    assert!(view_for(&stale, "FN42", NOW, Some(80.0)).is_none_or(|v| v.tier == GradeTier::Modelled));
    // A contact the pass could not grade shows nothing.
    let old = GradeAsk::of(&log_records()[5]);
    assert_eq!(view_for(&old, "FN42", NOW, Some(80.0)), None);
}

fn sydney_card(me: &str, at: i64) -> PathPrediction {
    let me = latlon(me).unwrap();
    let dx = latlon("QF56").unwrap();
    let eng = propagation::p533::engine::P533Engine::new(Some(me));
    let wx = propagation::SpaceWx {
        ssn: Some(80.0),
        ..Default::default()
    };
    let mut sp = eng.predict(dx, at, &wx);
    let mut lp = eng.predict_route(dx, Route::Long, at, &wx).unwrap();
    sp.geometry = Some(propagation::path_geometry_route(
        me,
        dx,
        Route::Short,
        at,
        None,
        None,
        None,
    ));
    lp.geometry = Some(propagation::path_geometry_route(
        me,
        dx,
        Route::Long,
        at,
        None,
        None,
        None,
    ));
    sp.long_path = Some(Box::new(lp));
    sp
}

#[test]
fn a_new_contact_is_graded_from_the_card_kept_for_its_station() {
    let _s = serial();
    let live = Mutex::new(propagation::LiveSpots::default());
    let region = Mutex::new(propagation::LiveSpots::default());
    let rec = qso("VK2AA", "QF56", "20m", NOW, None);
    // No card for the station: no grade.
    assert_eq!(at_log(&rec, "", &live, &region, &[]), None);
    let card = sydney_card("FN42", NOW);
    keep_outlook("FN42aa", "QF56", NOW, &card);
    let got = at_log(&rec, "", &live, &region, &[]).expect("graded");
    let hour = (NOW.rem_euclid(86_400) / 3600) as usize;
    let want = propagation::compare_routes_at(&card, card.long_path.as_deref().unwrap(), hour)
        .into_iter()
        .find(|c| c.band == "20m")
        .unwrap();
    assert_eq!(got, stamp_of(&want, "QF56/20m".into()));
    // A card from more than six hours away, or for another station, is not this contact's.
    let late = qso("VK2AA", "QF56", "20m", NOW + OUTLOOK_FRESH_SECS + 60, None);
    assert_eq!(at_log(&late, "", &live, &region, &[]), None);
    let other = qso("VK4AA", "QG62", "20m", NOW, None);
    assert_eq!(at_log(&other, "", &live, &region, &[]), None);
}

#[test]
fn the_grade_reasons_are_tokens_the_logbook_can_read() {
    use propagation::RouteReason as R;
    assert_eq!(reason_token(&R::OneClosed { open: Route::Long }), "OPEN_L");
    assert_eq!(
        reason_token(&R::Margin {
            better: Route::Short
        }),
        "MARGIN_S"
    );
    assert_eq!(
        reason_token(&R::Reliability {
            better: Route::Long
        }),
        "RELIAB_L"
    );
    assert_eq!(reason_token(&R::WithinMargin), "WITHIN");
    assert_eq!(reason_token(&R::BothClosed), "CLOSED");
    assert_eq!(
        reason_token(&R::GreyLine {
            route: Route::Short
        }),
        "GREY_S"
    );
    assert_eq!(
        reason_token(&R::HeardAlong {
            route: Route::Short,
            receivers: 3
        }),
        "HEARD_S:3"
    );
    assert_eq!(
        reason_token(&R::LateDt {
            ms: 60,
            decodes: 12
        }),
        "LATEDT:60/12"
    );
}

#[test]
fn a_contacts_day_is_modelled_at_its_own_flux_and_hour() {
    let _s = serial();
    let r = qso("ZS6AA", "KG44", "15m", NOW - 5 * 365 * 86_400, Some("90"));
    let day = contact_day(&GradeAsk::of(&r), "FN42", None, 0.0, NOW, Some(10.0)).expect("a day");
    assert_eq!(day.ssn_source, SsnSource::Logged);
    let pd = day.pred.day.as_ref().expect("the strip's day");
    assert_eq!(
        pd.now_hour as i64,
        (r.when_unix as i64).rem_euclid(86_400) / 3600
    );
    assert!(day.pred.long_path.is_some());
    // An old contact with no flux logged has no day to show.
    let old = qso("PY2AA", "GG66", "10m", NOW - 6 * 365 * 86_400, None);
    assert!(contact_day(&GradeAsk::of(&old), "FN42", None, 0.0, NOW, Some(80.0)).is_none());
}

/// ★ Every service is sent a graded contact exactly as an ungraded one: the grade never leaves
/// the machine, and Nexus adds no antenna field. An antenna path the operator set on the contact
/// card is theirs and goes out as any field they type. Each body is built the way its push
/// builds it: the row the Logbook holds (`upload_record`), its outbound ADIF, then the
/// service's own body.
#[test]
fn every_service_gets_the_contact_unchanged_by_its_grade() {
    let plain = qso("JA1AA", "PM95", "20m", NOW, Some("150"));
    let mut graded = plain.clone();
    path_stamp::stamp_path(
        &mut graded,
        &PathStamp {
            route: StampRoute::Short,
            conf: Some(StampConf::Likely),
            why: vec!["MARGIN_S".into(), "HEARD_S:2".into()],
            key: "PM95/20m".into(),
        },
    );
    assert_ne!(
        graded.extra, plain.extra,
        "control: the contact carries its grade"
    );
    let sent = |r: &QsoRecord| {
        let dto = tempo_app::dto::LoggedQso::from(r.clone());
        tempo_core::logbook::adif_record(&crate::upload_record(dto))
    };
    let bodies = |adif: &str| {
        vec![
            (
                "QRZ",
                tempo_core::qrz::build_insert_body("key", adif, false),
            ),
            (
                "Club Log",
                tempo_core::clublog::build_realtime_body(&tempo_core::clublog::ClubLogQuery {
                    email: "e".into(),
                    password: "p".into(),
                    callsign: "K2DEF".into(),
                    api_key: "k".into(),
                    adif: adif.to_string(),
                }),
            ),
            (
                "HRDLog",
                tempo_core::hrdlog::build_upload_body(&tempo_core::hrdlog::HrdLogQuery {
                    callsign: "K2DEF".into(),
                    code: "c".into(),
                    app: "Nexus".into(),
                    adif: adif.to_string(),
                }),
            ),
            (
                "eQSL",
                tempo_core::eqsl::build_upload_body("u", "p", adif, Some("Home"), "1"),
            ),
            (
                "Cloudlog",
                propagation::live::cloudlog::build_body("k", "1", adif),
            ),
        ]
    };
    let lotw = |r: &QsoRecord| {
        tempo_app::station::lotw_batch_adif(std::slice::from_ref(r), false, "K2DEF", "FN42")
    };
    assert_eq!(sent(&graded), sent(&plain));
    for ((name, g), (_, w)) in bodies(&sent(&graded))
        .into_iter()
        .zip(bodies(&sent(&plain)))
    {
        assert_eq!(g, w, "{name}");
        assert!(
            !g.contains("APP_NEXUS_PATH") && !g.contains("APP%5FNEXUS%5FPATH"),
            "{name}"
        );
        assert!(!g.contains("ANT_"), "{name}");
    }
    assert_eq!(lotw(&graded), lotw(&plain));
    // The operator's own antenna path, set on the card, goes out as theirs.
    let mut set = graded.clone();
    tempo_core::logbook::fill_extra(&mut set, "ANT_PATH", "L".into());
    let with_path = sent(&plain).replace("<SFI:3>150", "<ANT_PATH:1>L<SFI:3>150");
    assert_eq!(sent(&set), with_path);
    for ((name, g), (_, w)) in bodies(&sent(&set)).into_iter().zip(bodies(&with_path)) {
        assert_eq!(g, w, "{name}");
    }
    // The operator's own export reads back as the same contact, grade included.
    let own = adif_record_own_log(&graded);
    let back = parse_adif(&format!("<EOH>{own}")).remove(0);
    assert_eq!(back.extra, graded.extra);
}

#[test]
fn receivers_along_one_route_are_said_and_raise_nothing() {
    let _s = serial();
    let live = Mutex::new(propagation::LiveSpots::default());
    let region = Mutex::new(propagation::LiveSpots::default());
    keep_outlook("FN42", "QF56", NOW, &sydney_card("FN42", NOW));
    let rec = qso("VK2AA", "QF56", "20m", NOW, None);
    let plain = at_log(&rec, "", &live, &region, &[]).unwrap();
    // A receiver half-way round the long path heard the worked station within the quarter hour.
    let (me, dx) = (latlon("FN42").unwrap(), latlon("QF56").unwrap());
    let brg = propagation::geo::bearing_deg(me, dx);
    let lp_km = 40_030.0 - propagation::geo::haversine_km(me, dx);
    let mid = propagation::geo::destination_point(me, brg + 180.0, lp_km / 2.0);
    let spot = |tx: &str, at: i64| propagation::PathSpot {
        time: at,
        tx_call: tx.into(),
        tx_grid: Some("QF56".into()),
        rx_call: "ZS6RX".into(),
        rx_grid: Some(propagation::geo::latlon_to_maidenhead(mid.0, mid.1)),
        band: propagation::model::Band::B20,
        mode: Some("FT8".into()),
        snr: Some(-12.0),
        freq_mhz: None,
    };
    lock(&region).push(spot("VK2AA", NOW + 120));
    // Another station's spot, and one from an hour before or after: none counts.
    lock(&region).push(spot("VK3ZZ", NOW + 120));
    lock(&region).push(spot("VK2AA", NOW - 3600));
    lock(&region).push(spot("VK2AA", NOW + 3600));
    let heard = at_log(&rec, "", &live, &region, &[]).unwrap();
    assert_eq!(heard.why.last().map(String::as_str), Some("HEARD_L:1"));
    assert_eq!((heard.route, heard.conf), (plain.route, plain.conf));
}

#[test]
fn a_contact_that_did_not_go_by_the_ionosphere_is_never_graded() {
    let _s = serial();
    assert!(ionospheric(None) && ionospheric(Some("F2")) && ionospheric(Some("ion")));
    assert!(!ionospheric(Some("SAT")) && !ionospheric(Some("EME")) && !ionospheric(Some("RPT")));
    let live = Mutex::new(propagation::LiveSpots::default());
    let region = Mutex::new(propagation::LiveSpots::default());
    keep_outlook("FN42", "QF56", NOW, &sydney_card("FN42", NOW));
    let ion = qso("VK2AA", "QF56", "20m", NOW, Some("150"));
    let mut sat = ion.clone();
    sat.prop_mode = Some("SAT".into());
    // Control: the same contact by the ionosphere is graded at log time and for display.
    assert!(at_log(&ion, "", &live, &region, &[]).is_some());
    assert_eq!(at_log(&sat, "", &live, &region, &[]), None);
    let d = Dir::new("propmode");
    let engine = on_store(&d, &[ion.clone(), sat.clone()]);
    let s = run_pass(
        capture(&engine, NOW, Some(80.0)),
        &d.grades(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!((s.graded, s.ungradable), (1, 1));
    assert!(view_for(&GradeAsk::of(&ion), "FN42", NOW, Some(80.0)).is_some());
    assert_eq!(view_for(&GradeAsk::of(&sat), "FN42", NOW, Some(80.0)), None);
    // A stamp on a contact later marked as a satellite one is not shown either.
    let mut stamped = GradeAsk::of(&sat);
    stamped
        .extra
        .push((path_stamp::PATH_FIELD.into(), "S".into()));
    stamped.extra.push((
        path_stamp::PATH_WHY_FIELD.into(),
        "MARGIN_S;QF56/20m".into(),
    ));
    assert_eq!(view_for(&stamped, "FN42", NOW, Some(80.0)), None);
}

/// More cells than one chunk: every chunk is kept in the file, header first, and a restart reads
/// them all back.
#[test]
fn a_pass_of_many_chunks_keeps_every_chunk() {
    let _s = serial();
    let d = Dir::new("chunks");
    // Distinct hours and bands to one square: one cell each, more than a chunk's worth.
    let mut recs = Vec::new();
    for (k, band) in ["10m", "15m", "20m", "40m"].into_iter().enumerate() {
        for h in 0..24 {
            recs.push(qso(
                &format!("VK{k}A{h}"),
                "QF56",
                band,
                NOW - 3600 * (h + 1),
                Some("150"),
            ));
        }
    }
    let engine = on_store(&d, &recs);
    let s = run_pass(
        capture(&engine, NOW, Some(80.0)),
        &d.grades(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(s.cells_total as usize > CHUNK, "{} cells", s.cells_total);
    let held = load_file(&d.grades(), "p533|None|0").expect("a readable file");
    assert_eq!(
        held.len(),
        s.cells_total as usize,
        "every chunk in the file"
    );
    assert_eq!(held.len(), lock(&GRADES).cells.len());
}
