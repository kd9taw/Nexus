//! An IOTA reference is OFFERED at log time, from the spot comment and the callbook, and written
//! only when the operator takes the offer (operator decision, 2026-10-27). Nothing stamps one by
//! itself: a callbook answer naming an island group no longer fills the contact on any path, and a
//! reference taken from an offer is held back from every upload until the operator touches it.
//!
//! The one exception (operator decisions, 2026-11-06): an FT contact logged with no popup takes
//! the reference a fresh spot on its band named, else the callbook answer already held for exactly
//! its call, held the same way.

use super::*;
use crate::engine::tests::dec_snr;
use crate::test_util::StoredLog;
use tempo_core::logbook::callbook::{is_callbook_filled, CallbookAnswer};
use tempo_core::logbook::offer::{is_offer_held, IotaOffer, OfferSource, SpotIota};

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
/// name as before, and its IOTA on no path a person logs through: the log forms, a plain `log_qso`
/// caller and Remote's synced log. (The FT contact with no popup is below.)
#[test]
fn a_callbook_answer_with_an_island_never_stamps_the_contact() {
    let mut e = station();
    for call in ["IA5FORM", "IA5PLAIN", "IA5SYNC"] {
        e.note_callbook_answer(call, answer(call));
    }
    let form = bare(&e, "IA5FORM");
    e.log_form_qso(form, StateSource::Operator, Vec::new());
    let plain = bare(&e, "IA5PLAIN");
    e.log_qso(plain);
    let sync = bare(&e, "IA5SYNC");
    let _ = e.log_qso_for_sync(sync);
    for call in ["IA5FORM", "IA5PLAIN", "IA5SYNC"] {
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

/// The log form: an offer the operator took there (`log_qso` writes it with `accept_iota_offer`
/// before `log_form_qso`) keeps its hold mark through to the stored contact and the upload queue.
#[test]
fn a_form_offer_taken_keeps_its_hold_through_to_the_upload_queue() {
    use tempo_core::logbook::offer::accept_iota_offer;
    let mut e = station();
    let _ = e.take_pending_uploads();
    let mut form = bare(&e, "IA5F");
    let offer = IotaOffer {
        reference: "EU-028".into(),
        source: OfferSource::Spot,
    };
    assert!(accept_iota_offer(&mut form, &offer));
    e.log_form_qso(form, StateSource::Operator, Vec::new());
    let r = logged(&e, "IA5F");
    assert_eq!(r.ota.iota.as_deref(), Some("EU-028"));
    assert!(is_offer_held(&r, "IOTA"), "held back until touched");
    let queued: Vec<PendingUpload> = e
        .take_pending_uploads()
        .into_iter()
        .filter(|u| u.rec.call == "IA5F")
        .collect();
    assert!(
        !queued.is_empty(),
        "premise: the contact is queued for upload"
    );
    for u in &queued {
        assert!(
            is_offer_held(&u.rec, "IOTA"),
            "the queued copy keeps the mark"
        );
        let sent = tempo_core::logbook::adif_record(&u.rec);
        assert!(!sent.contains("<IOTA:"), "{sent}");
    }
}

/// What the injected spot lookup was asked, so a test can name the call, band and time it read.
type Asked = std::sync::Arc<std::sync::Mutex<Vec<(String, String, u64)>>>;

/// A station whose spots say `spots` of every call, keeping what each lookup asked.
fn spotted(spots: SpotIota) -> (Engine, Asked) {
    let mut e = station();
    let asked = Asked::default();
    let keep = asked.clone();
    e.set_iota_at_log(
        move |call, band, when| {
            keep.lock()
                .unwrap()
                .push((call.to_string(), band.to_string(), when));
            spots.clone()
        },
        |_| true,
    );
    (e, asked)
}

fn names(group: &str) -> SpotIota {
    SpotIota::Names(group.to_string())
}

/// ★ THE NO-POPUP FT CONTACT: the spot's reference is in the log, held, and in no upload; the
/// lookup is asked for this contact's own call, band and start time, which is what a stale spot
/// or one on another band is refused by.
#[test]
fn an_ft_contact_with_no_popup_logs_the_spots_reference_held() {
    let (mut e, asked) = spotted(names("EU-028"));
    e.settings.auto_log = true;
    let _ = e.take_pending_uploads();
    work(&mut e, "IA5X", 1);
    let r = logged(&e, "IA5X");
    assert_eq!(r.ota.iota.as_deref(), Some("EU-028"));
    assert!(is_offer_held(&r, "IOTA"), "held back until touched");
    assert!(
        !is_callbook_filled(&r, "IOTA"),
        "the spot's, not a callbook's"
    );
    assert_eq!(
        *asked.lock().unwrap(),
        [("IA5X".to_string(), r.band.clone(), r.when_unix)]
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
}

/// Log QSO with Auto-log and the popup both off is the same contact with no popup.
#[test]
fn the_log_qso_button_with_no_popup_logs_the_spots_reference_held() {
    let (mut e, _) = spotted(names("EU-028"));
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    work(&mut e, "IA5X", 1);
    assert!(
        e.stored_records().iter().all(|r| r.call != "IA5X"),
        "premise"
    );
    assert!(e.log_current_qso());
    let r = logged(&e, "IA5X");
    assert_eq!(r.ota.iota.as_deref(), Some("EU-028"));
    assert!(is_offer_held(&r, "IOTA"));
}

/// The popup and the log forms keep their offer, with a spot and a callbook answer both there: the
/// spot lookup is never even asked.
#[test]
fn the_popup_and_the_log_forms_still_only_offer() {
    let (mut e, asked) = spotted(names("EU-028"));
    e.settings.prompt_to_log = true;
    for call in ["IA5P", "IA5F", "IA5L", "IA5S"] {
        e.note_callbook_answer(call, answer(call));
    }
    work(&mut e, "IA5P", 1);
    let key = e
        .pending_qso_log_key()
        .expect("premise: the popup holds it");
    let shown = e.pending_log().cloned().expect("held");
    assert_eq!(shown.ota.iota, None, "nothing written on the held contact");
    assert!(e.confirm_pending_log(&key, shown));
    e.settings.prompt_to_log = false;
    let form = bare(&e, "IA5F");
    e.log_form_qso(form, StateSource::Operator, Vec::new());
    let plain = bare(&e, "IA5L");
    e.log_qso(plain);
    let sync = bare(&e, "IA5S");
    let _ = e.log_qso_for_sync(sync);
    for call in ["IA5P", "IA5F", "IA5L", "IA5S"] {
        let r = logged(&e, call);
        assert_eq!(r.ota.iota, None, "{call}");
        assert!(!is_offer_held(&r, "IOTA"), "{call}");
    }
    assert!(asked.lock().unwrap().is_empty());
}

/// CALLBOOK ONLY: no spot, an answer already held for this call. Held and in no upload, and marked
/// as the callbook's, which is what the card's note says.
#[test]
fn a_callbook_answer_alone_logs_held_on_a_no_popup_contact() {
    let (mut e, _) = spotted(SpotIota::Silent);
    e.settings.auto_log = true;
    e.note_callbook_answer("IA5X", answer("IA5X"));
    let _ = e.take_pending_uploads();
    work(&mut e, "IA5X", 1);
    let r = logged(&e, "IA5X");
    assert_eq!(r.ota.iota.as_deref(), Some("EU-025"));
    assert!(is_offer_held(&r, "IOTA"));
    assert!(
        is_callbook_filled(&r, "IOTA"),
        "the card says from the callbook"
    );
    assert_eq!(
        r.name.as_deref(),
        Some("Marco"),
        "the rest of the answer still fills"
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
}

/// AGREE and DISAGREE: the spot's goes in with the spot's source either way.
#[test]
fn a_fresh_spot_wins_over_the_callbook_on_a_no_popup_contact() {
    for group in ["EU-025", "EU-028"] {
        let (mut e, _) = spotted(names(group));
        e.settings.auto_log = true;
        e.note_callbook_answer("IA5X", answer("IA5X"));
        work(&mut e, "IA5X", 1);
        let r = logged(&e, "IA5X");
        assert_eq!(r.ota.iota.as_deref(), Some(group));
        assert!(is_offer_held(&r, "IOTA"));
        assert!(!is_callbook_filled(&r, "IOTA"), "{group}: says from a spot");
    }
}

/// Spots that disagree, or place the station only on another band, write nothing from either.
#[test]
fn refused_spots_stop_the_callbook_on_a_no_popup_contact() {
    let (mut e, _) = spotted(SpotIota::Refused);
    e.settings.auto_log = true;
    e.note_callbook_answer("IA5X", answer("IA5X"));
    work(&mut e, "IA5X", 1);
    let r = logged(&e, "IA5X");
    assert_eq!(r.ota.iota, None);
    assert!(!is_offer_held(&r, "IOTA"));
}

/// NO ANSWER YET: the logging path never asks, so the contact logs with no reference, and an answer
/// arriving afterwards does not touch it.
#[test]
fn with_no_answer_yet_a_no_popup_contact_logs_no_reference() {
    let (mut e, _) = spotted(SpotIota::Silent);
    e.settings.auto_log = true;
    work(&mut e, "IA5X", 1);
    assert_eq!(logged(&e, "IA5X").ota.iota, None);
    e.note_callbook_answer("IA5X", answer("IA5X"));
    let r = logged(&e, "IA5X");
    assert_eq!(r.ota.iota, None);
    assert!(!is_offer_held(&r, "IOTA"));
}

/// The answer of another call, or a stroked call's, is not this contact's.
#[test]
fn the_no_popup_path_keeps_the_exact_call_rules() {
    let (mut e, _) = spotted(SpotIota::Silent);
    e.settings.auto_log = true;
    e.note_callbook_answer("IA5A", answer("IA5B"));
    work(&mut e, "IA5A", 1);
    assert_eq!(logged(&e, "IA5A").ota.iota, None, "another call's record");
    e.note_callbook_answer("IA5C/P", answer("IA5C/P"));
    work(&mut e, "IA5C/P", 11);
    assert_eq!(logged(&e, "IA5C/P").ota.iota, None, "stroked");
}
