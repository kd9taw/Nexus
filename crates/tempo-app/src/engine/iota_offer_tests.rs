//! An IOTA reference is OFFERED at log time, from the spot comment and the callbook, and written
//! only when the operator takes the offer (operator decision, 2026-10-27). Nothing stamps one by
//! itself: a callbook answer naming an island group no longer fills the contact on any path, an FT
//! contact auto-logged with no popup logs without one, and a reference taken from an offer is
//! held back from every upload until the operator touches it.

use super::*;
use crate::engine::tests::dec_snr;
use crate::test_util::StoredLog;
use tempo_core::logbook::callbook::{is_callbook_filled, CallbookAnswer};
use tempo_core::logbook::offer::{is_offer_held, IotaOffer, OfferSource};

/// The callbook entry of an island station: it names its group.
fn answer(call: &str) -> CallbookAnswer {
    CallbookAnswer {
        call: call.into(),
        name: Some("Marco".into()),
        iota: Some("EU-025".into()),
        ..CallbookAnswer::default()
    }
}

fn station() -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_tier(Tier::TempoFast);
    e
}

/// The stored contact with `call`.
fn logged(e: &Engine, call: &str) -> QsoRecord {
    let log = e.stored_records();
    let found: Vec<&QsoRecord> = log.iter().filter(|r| r.call == call).collect();
    assert_eq!(found.len(), 1, "one contact with {call}");
    found[0].clone()
}

fn bare(e: &Engine, call: &str) -> QsoRecord {
    let mut r = e.qso_record(call.into(), None, None);
    r.when_unix = now_unix_secs();
    r
}

/// Work `call` to its RR73: the contact completes (and auto-logs, or waits in the popup).
fn work(e: &mut Engine, call: &str, slot: u64) {
    e.call_station(call);
    e.ingest_decodes_for_test(&[dec_snr(&format!("K2DEF {call} -10"), -7)], slot);
    e.ingest_decodes_for_test(&[dec_snr(&format!("K2DEF {call} RR73"), -7)], slot + 2);
}

/// The IOTA field of every upload queued for `call`, as the services would be sent it.
fn uploaded_iota(e: &mut Engine, call: &str) -> Vec<bool> {
    e.take_pending_uploads()
        .iter()
        .filter(|u| u.rec.call == call)
        .map(|u| tempo_core::logbook::adif_record(&u.rec).contains("<IOTA:"))
        .collect()
}

/// ★ NOTHING IS STAMPED SILENTLY. A callbook answer naming the station's island group fills its
/// name as before, and its IOTA on no path: the FT auto-log, the log forms, a plain `log_qso`
/// caller and Remote's synced log.
#[test]
fn a_callbook_answer_with_an_island_never_stamps_the_contact() {
    let mut e = station();
    for call in ["IA5FT", "IA5FORM", "IA5PLAIN", "IA5SYNC"] {
        e.note_callbook_answer(call, answer(call));
    }
    work(&mut e, "IA5FT", 1);
    let form = bare(&e, "IA5FORM");
    e.log_form_qso(form, StateSource::Operator, Vec::new());
    let plain = bare(&e, "IA5PLAIN");
    e.log_qso(plain);
    let sync = bare(&e, "IA5SYNC");
    let _ = e.log_qso_for_sync(sync);
    for call in ["IA5FT", "IA5FORM", "IA5PLAIN", "IA5SYNC"] {
        let r = logged(&e, call);
        assert_eq!(r.name.as_deref(), Some("Marco"), "{call}: premise, filled");
        assert_eq!(
            r.ota.iota, None,
            "{call}: the island is offered, never stamped"
        );
        assert!(!is_callbook_filled(&r, "IOTA"), "{call}");
    }
}

/// The confirm-before-log popup: the offer the operator took is written and held back from every
/// upload; the same contact confirmed without taking it logs no reference.
#[test]
fn the_popup_writes_an_offer_taken_and_holds_it_back_from_uploads() {
    let mut e = station();
    e.settings.prompt_to_log = true;
    e.note_callbook_answer("IA5X", answer("IA5X"));
    e.note_callbook_answer("IA5Y", answer("IA5Y"));
    let _ = e.take_pending_uploads();

    work(&mut e, "IA5X", 1);
    let key = e
        .pending_qso_log_key()
        .expect("premise: the popup holds it");
    let shown = e.pending_log().cloned().expect("held");
    assert_eq!(
        shown.ota.iota, None,
        "premise: nothing stamped on the held contact"
    );
    let offer = IotaOffer {
        reference: "EU-025".into(),
        source: OfferSource::Callbook,
    };
    assert!(e.confirm_pending_log_with_offer(&key, shown, Some(&offer)));
    let r = logged(&e, "IA5X");
    assert_eq!(r.ota.iota.as_deref(), Some("EU-025"));
    assert!(is_offer_held(&r, "IOTA"), "held back until touched");
    assert!(
        is_callbook_filled(&r, "IOTA"),
        "and says it is the callbook's"
    );
    let sent = uploaded_iota(&mut e, "IA5X");
    assert!(
        !sent.is_empty(),
        "premise: the contact is queued for upload"
    );
    assert!(
        sent.iter().all(|has| !has),
        "no upload carries the reference"
    );

    work(&mut e, "IA5Y", 11);
    let key = e.pending_qso_log_key().expect("held");
    let shown = e.pending_log().cloned().expect("held");
    assert!(e.confirm_pending_log(&key, shown));
    assert_eq!(logged(&e, "IA5Y").ota.iota, None, "not taken: not logged");
}

/// A reference the contact already holds is never replaced by an offer, and is not held back.
#[test]
fn an_offer_never_replaces_a_reference_the_contact_holds() {
    let mut e = station();
    e.settings.prompt_to_log = true;
    let mut rec = bare(&e, "IA5Z");
    rec.ota.iota = Some("EU-001".into());
    e.load_pending_qso(rec);
    let key = e.pending_qso_log_key().expect("held");
    let shown = e.pending_log().cloned().expect("held");
    let offer = IotaOffer {
        reference: "EU-025".into(),
        source: OfferSource::Spot,
    };
    assert!(e.confirm_pending_log_with_offer(&key, shown, Some(&offer)));
    let r = logged(&e, "IA5Z");
    assert_eq!(r.ota.iota.as_deref(), Some("EU-001"));
    assert!(!is_offer_held(&r, "IOTA"));
}
