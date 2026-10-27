//! A callbook lookup already answered this session reaches the contact logged with that call, on
//! every path into the log: the FT auto-log, the log forms, the plain `log_qso` callers (RTTY,
//! JS8, the companion) and Remote's synced log. Empty fields only, and never a lookup of its own.

use super::*;
use crate::engine::tests::dec_snr;
use crate::test_util::StoredLog;
use tempo_core::logbook::callbook::{
    callbook_filled, CallbookAnswer, CALLBOOK_FILLED_FIELD, LOTW_USER_FIELD,
};
use tempo_core::logbook::extra_field;

/// What W9's callbook entry says: a station in Hartford County, Connecticut.
fn answer(call: &str) -> CallbookAnswer {
    CallbookAnswer {
        call: call.into(),
        name: Some("Dave".into()),
        qth: Some("Newington".into()),
        grid: Some("FN31pr".into()),
        state: Some("CT".into()),
        county: Some("Hartford".into()),
        iota: None,
        cq_zone: Some(5),
        itu_zone: Some(8),
        position: Some((-41.7147, -72.7272)),
        email: Some("dave@example.org".into()),
        web: None,
        qsl_via: Some("W1MGR".into()),
        lotw: Some(true),
        eqsl: None,
        paper_qsl: None,
        utc_offset: Some(-5.0),
    }
}

/// An FT station on 20 m, every call it logs in the United States.
fn station() -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_tier(Tier::TempoFast);
    e.set_dxcc_resolver(|_| Some("United States".to_string()));
    e
}

/// The fields a callbook fill writes, as the stored contact holds them.
fn filled(r: &QsoRecord) -> Vec<(&'static str, Option<String>)> {
    let x = |k: &str| extra_field(r, k).map(str::to_string);
    vec![
        ("NAME", r.name.clone()),
        ("QTH", r.qth.clone()),
        ("GRIDSQUARE", r.grid.clone()),
        ("STATE", r.state.clone()),
        ("CNTY", x("CNTY")),
        ("CQZ", x("CQZ")),
        ("ITUZ", x("ITUZ")),
        ("LAT", x("LAT")),
        ("LON", x("LON")),
        ("EMAIL", x("EMAIL")),
        ("QSL_VIA", x("QSL_VIA")),
        (LOTW_USER_FIELD, x(LOTW_USER_FIELD)),
    ]
}

fn all_filled() -> Vec<(&'static str, Option<String>)> {
    [
        ("NAME", "Dave"),
        ("QTH", "Newington"),
        ("GRIDSQUARE", "FN31pr"),
        ("STATE", "CT"),
        ("CNTY", "CT,Hartford"),
        ("CQZ", "5"),
        ("ITUZ", "8"),
        ("LAT", "S041 42.882"),
        ("LON", "W072 43.632"),
        ("EMAIL", "dave@example.org"),
        ("QSL_VIA", "W1MGR"),
        (LOTW_USER_FIELD, "Y"),
    ]
    .into_iter()
    .map(|(k, v)| (k, Some(v.to_string())))
    .collect()
}

/// The stored contact with `call`.
fn logged(e: &Engine, call: &str) -> QsoRecord {
    let log = e.stored_records();
    let found: Vec<&QsoRecord> = log.iter().filter(|r| r.call == call).collect();
    assert_eq!(found.len(), 1, "one contact with {call}");
    found[0].clone()
}

/// A contact with `call` as a log form or another program hands it in: nothing filled.
fn bare(e: &Engine, call: &str) -> QsoRecord {
    let mut r = e.qso_record(call.into(), None, None);
    r.when_unix = now_unix_secs();
    r
}

/// ★ Every path into the log takes the answer into the contact: the FT auto-log, the log form,
/// a plain `log_qso` caller and Remote's synced log. Each field lands in ADIF's own spelling and
/// is named in the mark.
#[test]
fn every_log_path_takes_the_callbook_answer() {
    let mut e = station();
    for call in ["W9FT", "W9FORM", "W9PLAIN", "W9SYNC"] {
        e.note_callbook_answer(call, answer(call));
    }

    // The FT sequencer's auto-log.
    e.call_station("W9FT");
    e.ingest_decodes_for_test(&[dec_snr("K2DEF W9FT -10", -7)], 1);
    e.ingest_decodes_for_test(&[dec_snr("K2DEF W9FT RR73", -7)], 3);
    // The log forms (the log strip and the Logbook's own add).
    let form = bare(&e, "W9FORM");
    e.log_form_qso(form, StateSource::Operator, Vec::new());
    // RTTY, JS8, the companion: `log_qso`.
    let plain = bare(&e, "W9PLAIN");
    e.log_qso(plain);
    // Remote's log form.
    let sync = bare(&e, "W9SYNC");
    let _ = e.log_qso_for_sync(sync);

    for call in ["W9FT", "W9FORM", "W9PLAIN", "W9SYNC"] {
        let r = logged(&e, call);
        assert_eq!(filled(&r), all_filled(), "{call}");
        assert!(
            callbook_filled(&r).contains(&"CQZ"),
            "{call}: the zone is marked as the callbook's"
        );
        assert_eq!(r.dxcc, None, "{call}: never the callbook's DXCC number");
    }
}

/// ★ A value the contact already carries is never replaced: what the operator typed in the form,
/// and a grid the station sent on the air.
#[test]
fn a_value_the_contact_carries_is_never_replaced() {
    let mut e = station();
    e.note_callbook_answer("W9FORM", answer("W9FORM"));
    e.note_callbook_answer("W9FT", answer("W9FT"));

    let mut form = bare(&e, "W9FORM");
    form.name = Some("Bob".into());
    form.grid = Some("EN52".into());
    form.qth = Some("Elgin".into());
    tempo_core::logbook::fill_extra(&mut form, "CQZ", "4".into());
    e.log_form_qso(form, StateSource::Operator, Vec::new());
    let r = logged(&e, "W9FORM");
    assert_eq!(r.name.as_deref(), Some("Bob"));
    assert_eq!(r.grid.as_deref(), Some("EN52"));
    assert_eq!(r.qth.as_deref(), Some("Elgin"));
    assert_eq!(extra_field(&r, "CQZ"), Some("4"));
    let marked = callbook_filled(&r);
    for tag in ["NAME", "GRIDSQUARE", "QTH", "CQZ"] {
        assert!(
            !marked.contains(&tag),
            "{tag} is the operator's: {marked:?}"
        );
    }
    assert_eq!(
        extra_field(&r, "ITUZ"),
        Some("8"),
        "an empty field still fills"
    );

    // The station sent its grid: the decode's grid is logged, not the callbook's.
    e.call_station("W9FT");
    e.ingest_decodes_for_test(&[dec_snr("K2DEF W9FT EN61", -7)], 1);
    e.ingest_decodes_for_test(&[dec_snr("K2DEF W9FT RR73", -7)], 3);
    assert_eq!(logged(&e, "W9FT").grid.as_deref(), Some("EN61"));
}

/// ★ THE FT AUTO-LOG NEVER WAITS FOR A LOOKUP. The engine has no callbook client: the fill reads
/// only answers a lookup already returned. A contact whose call nobody looked up logs at once,
/// in the same call that completed it, with nothing filled; an answer that arrives later does
/// not reach back into the logged contact; and another call's answer is not borrowed.
#[test]
fn the_ft_auto_log_never_waits_for_a_lookup() {
    let mut e = station();
    e.note_callbook_answer("K9AAA", answer("K9AAA"));
    e.call_station("W9XYZ");
    e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ -10", -7)], 1);
    e.ingest_decodes_for_test(&[dec_snr("K2DEF W9XYZ RR73", -7)], 3);
    // Logged by the ingest that completed it — nothing pending, nothing waiting.
    let r = logged(&e, "W9XYZ");
    assert!(
        filled(&r).iter().all(|(_, v)| v.is_none()),
        "no answer for this call: nothing filled, {:?}",
        filled(&r)
    );
    assert_eq!(extra_field(&r, CALLBOOK_FILLED_FIELD), None);

    e.note_callbook_answer("W9XYZ", answer("W9XYZ"));
    assert_eq!(
        logged(&e, "W9XYZ"),
        r,
        "a later answer leaves the logged contact alone"
    );
}

/// A lookup of a stroked call that came back with the base call's record names the operator,
/// but does not place the contact: the base call's entry is where the licensee lives.
#[test]
fn a_base_call_answer_does_not_place_a_portable_contact() {
    let mut e = station();
    e.note_callbook_answer("W9XYZ/P", answer("W9XYZ"));
    let rec = bare(&e, "W9XYZ/P");
    e.log_qso(rec);
    let r = logged(&e, "W9XYZ/P");
    assert_eq!(r.name.as_deref(), Some("Dave"));
    assert_eq!(extra_field(&r, "EMAIL"), Some("dave@example.org"));
    for (tag, v) in filled(&r) {
        if !matches!(tag, "NAME" | "EMAIL" | "QSL_VIA" | LOTW_USER_FIELD) {
            assert_eq!(v, None, "{tag}: not where /P was");
        }
    }
}
