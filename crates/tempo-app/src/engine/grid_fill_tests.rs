//! A contact logged with no grid in the exchange (operator decisions, 2026-10-27).
//!
//! It takes the grid the station SENT ON THE AIR this session (its CQ, its call to anyone), on any
//! band and in any mode, and that grid counts as any logged grid. Otherwise, for a call without a
//! stroke and with no park, a callbook lookup already answered this session fills it for the
//! distance and the map only: marked as the callbook's, it counts toward no grid award, fills no
//! GRID need, and is sent to no service.

use super::tests::dec_snr;
use super::*;
use crate::test_util::StoredLog;
use tempo_core::logbook::callbook::{is_callbook_filled, CallbookAnswer};
use tempo_core::logbook::{adif_record, adif_record_own_log, counted_grid, extra_field};

const ME: &str = "KR4FQG";
const DX: &str = "7P8RU";

fn ft8() -> Engine {
    let mut e = Engine::new(ME, "EM85", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    e
}

fn tx_now(e: &Engine) -> String {
    e.snapshot()
        .qso
        .as_ref()
        .and_then(|q| q.tx_now.clone())
        .unwrap_or_default()
}

fn key_once(e: &mut Engine, s: u64) -> String {
    let text = tx_now(e);
    for slot in [s, s + 1] {
        if !e.poll_tx(slot).is_empty() {
            return text;
        }
    }
    panic!("nothing keyed at slot {s} or {} (queued: {text:?})", s + 1)
}

/// A CQ run on 17 m answered by `DX` with a bare report (no grid in the exchange), worked to our
/// 73, and logged by itself (Auto-log on).
fn run_and_work_a_gridless_caller(e: &mut Engine) -> QsoRecord {
    e.set_mode("qso-run").expect("the run starts");
    assert!(key_once(e, 100).starts_with("CQ KR4FQG"), "premise: CQ");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU -15", -9)], 102);
    assert_eq!(key_once(e, 102), "7P8RU KR4FQG R-09", "premise: roger");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU RR73", -9)], 104);
    assert_eq!(key_once(e, 104), "7P8RU KR4FQG 73", "premise: 73");
    e.ingest_decodes_for_test(&[], 106);
    let log = e.stored_log();
    assert_eq!(log.len(), 1, "premise: logged by itself");
    QsoRecord::clone(&log[0])
}

/// What 7P8RU's callbook entry says: a home QTH in KG30.
fn callbook(call: &str) -> CallbookAnswer {
    CallbookAnswer {
        call: call.into(),
        grid: Some("KG30ab".into()),
        ..CallbookAnswer::default()
    }
}

fn to_17m(e: &mut Engine) {
    e.set_frequency(18.100, "17m", "USB");
}

/// Heard calling CQ on 20 m with its grid, worked on 17 m with a bare report: the band change
/// cleared the roster, and the contact still logs the grid the station sent, counted as any.
#[test]
fn a_gridless_contact_takes_the_grid_the_station_sent_on_another_band() {
    let mut e = ft8();
    e.ingest_decodes_for_test(&[dec_snr("CQ 7P8RU KG31", -12)], 1);
    to_17m(&mut e);
    let rec = run_and_work_a_gridless_caller(&mut e);
    assert_eq!(rec.grid.as_deref(), Some("KG31"), "the grid it sent");
    assert!(
        !is_callbook_filled(&rec, "GRIDSQUARE"),
        "not the callbook's"
    );
    assert_eq!(counted_grid(&rec), Some("KG31"), "counted");
    assert!(adif_record(&rec).contains("<GRIDSQUARE:4>KG31"), "uploaded");
    assert!(
        e.station.hot().grid_worked_on("KG31", "17m"),
        "worked on 17 m"
    );
}

/// …and the grid it sent beats a callbook's.
#[test]
fn the_grid_the_station_sent_wins_over_the_callbook() {
    let mut e = ft8();
    e.note_callbook_answer(DX, callbook(DX));
    e.ingest_decodes_for_test(&[dec_snr("W1AW 7P8RU KG31", -12)], 1);
    to_17m(&mut e);
    let rec = run_and_work_a_gridless_caller(&mut e);
    assert_eq!(rec.grid.as_deref(), Some("KG31"));
    assert!(!is_callbook_filled(&rec, "GRIDSQUARE"));
}

/// Never heard sending a grid: the callbook's fills it, marked, for the distance and the map only.
/// It is not worked, not sent to any service (the ADIF a service gets, the WSJT-X logged message,
/// the N1MM broadcast), and the operator's own log keeps it.
#[test]
fn a_callbook_grid_is_for_distance_and_the_map_only() {
    let mut e = ft8();
    e.note_callbook_answer(DX, callbook(DX));
    to_17m(&mut e);
    let rec = run_and_work_a_gridless_caller(&mut e);
    assert_eq!(rec.grid.as_deref(), Some("KG30ab"), "filled for the map");
    assert!(is_callbook_filled(&rec, "GRIDSQUARE"), "marked");
    assert!(extra_field(&rec, "DISTANCE").is_some(), "the distance");
    assert_eq!(counted_grid(&rec), None, "counts toward nothing");
    assert!(
        !e.station.hot().grid_worked_on("KG30", "17m"),
        "not a worked grid"
    );
    assert!(
        !adif_record(&rec).contains("<GRIDSQUARE:"),
        "never sent: {}",
        adif_record(&rec)
    );
    assert!(adif_record_own_log(&rec).contains("<GRIDSQUARE:6>KG30ab"));
    assert_eq!(n1mm_contact_for(&rec, ME, 1).gridsquare, "");
}

/// A grid a callbook filled on an earlier contact is not taken again as a logged grid: the next
/// contact with that station, no lookup answered for it now, logs no grid.
#[test]
fn a_callbook_grid_is_never_copied_as_a_logged_grid() {
    let mut e = ft8();
    e.note_callbook_answer(DX, callbook(DX));
    to_17m(&mut e);
    let first = run_and_work_a_gridless_caller(&mut e);
    assert!(is_callbook_filled(&first, "GRIDSQUARE"), "premise");
    e.callbook_answers.clear();
    e.set_frequency(21.074, "15m", "USB");
    e.set_mode("qso-run").expect("the run starts");
    assert!(key_once(&mut e, 200).starts_with("CQ KR4FQG"));
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU -15", -9)], 202);
    key_once(&mut e, 202);
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU RR73", -9)], 204);
    key_once(&mut e, 204);
    e.ingest_decodes_for_test(&[], 206);
    let log = e.stored_log();
    assert_eq!(log.len(), 2, "premise: the second logged");
    let second = &log[1];
    assert_eq!(
        (second.grid.as_deref(), counted_grid(second)),
        (None, None),
        "no grid copied from the callbook's: {:?}",
        second.grid
    );
}

/// The station's sent grid survives a mode change (the roster is cleared there too).
#[test]
fn the_grid_the_station_sent_survives_a_mode_change() {
    let mut e = ft8();
    e.ingest_decodes_for_test(&[dec_snr("CQ 7P8RU KG31", -12)], 1);
    e.set_tier(Tier::Ft4);
    e.set_tier(Tier::Ft8);
    to_17m(&mut e);
    let rec = run_and_work_a_gridless_caller(&mut e);
    assert_eq!(rec.grid.as_deref(), Some("KG31"));
}
