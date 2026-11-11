//! An IOTA reference a spot or the callbook names is LOGGED with the contact on every path, and goes
//! to every upload (operator rulings, 2026-11-10, replacing the 2026-10-27 offer-only rule and the
//! 2026-11-06 no-popup hold). Where a form shows the contact first (the log strip, the Logbook's
//! Add form, the confirm-before-log popup) the reference is put in its box, for the operator to
//! change or clear before saving; where none does (an FT contact with no popup, a plain `log_qso`,
//! Remote's synced log) the station fills it: a fresh spot on the contact's band, else the callbook
//! answer already held for exactly its call. Every logged contact with a reference the IOTA list
//! names gets `IOTA <ref> <group name>` after its COMMENT, once.

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
fn uploaded_iota(e: &mut Engine, call: &str) -> Vec<Option<String>> {
    e.take_pending_uploads()
        .iter()
        .filter(|u| u.rec.call == call)
        .map(|u| {
            let sent = tempo_core::logbook::adif_record(&u.rec);
            assert!(!sent.contains("APP_NEXUS_OFFER_HELD"), "{sent}");
            sent.split_once("<IOTA:6>").map(|(_, r)| r[..6].to_string())
        })
        .collect()
}

/// Every upload for `call` carries `want`, and there is at least one.
fn every_upload_carries(e: &mut Engine, call: &str, want: &str) {
    let sent = uploaded_iota(e, call);
    assert!(!sent.is_empty(), "premise: {call} is queued for upload");
    for s in &sent {
        assert_eq!(s.as_deref(), Some(want), "{call}");
    }
}

/// What the injected spot lookup was asked, so a test can name the call, band and time it read.
type Asked = std::sync::Arc<std::sync::Mutex<Vec<(String, String, u64)>>>;

/// The IOTA list's names, for the tests' groups.
fn group_name(r: &str) -> Option<String> {
    match r {
        "EU-005" => Some("Great Britain".into()),
        "EU-025" => Some("Lipari Islands".into()),
        "EU-028" => Some("Sicily".into()),
        _ => None,
    }
}

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
        group_name,
    );
    (e, asked)
}

fn names(group: &str) -> SpotIota {
    SpotIota::Names(group.to_string())
}

/// REPRO (operator report, 2026-11-10): an FT contact Auto-log wrote with no popup, with a fresh
/// spot on its band naming the station's island group, carries the reference into every upload
/// and the group's name into its COMMENT. Red before the fix: the released build logged no
/// reference at all on this path, and main wrote it held, sent nowhere, with no name.
#[test]
fn repro_an_auto_logged_ft_contact_logs_and_sends_the_spots_iota_with_its_name() {
    let (mut e, asked) = spotted(names("EU-005"));
    e.settings.auto_log = true;
    let _ = e.take_pending_uploads();
    work(&mut e, "IA5X", 1);
    let r = logged(&e, "IA5X");
    assert_eq!(r.ota.iota.as_deref(), Some("EU-005"));
    assert!(
        !is_callbook_filled(&r, "IOTA"),
        "the spot's, not a callbook's"
    );
    assert_eq!(r.comment.as_deref(), Some("IOTA EU-005 Great Britain"));
    assert_eq!(
        *asked.lock().unwrap(),
        [("IA5X".to_string(), r.band.clone(), r.when_unix)],
        "asked for this contact's own call, band and start time"
    );
    every_upload_carries(&mut e, "IA5X", "EU-005");
}

/// CALLBOOK ONLY on the same path: no spot, an answer already held for this call. Logged, marked
/// as the callbook's, in every upload, its name in the comment.
#[test]
fn an_auto_logged_ft_contact_takes_the_callbooks_iota() {
    let (mut e, _) = spotted(SpotIota::Silent);
    e.settings.auto_log = true;
    e.note_callbook_answer("IA5X", answer("IA5X"));
    let _ = e.take_pending_uploads();
    work(&mut e, "IA5X", 1);
    let r = logged(&e, "IA5X");
    assert_eq!(r.ota.iota.as_deref(), Some("EU-025"));
    assert!(is_callbook_filled(&r, "IOTA"), "says from the callbook");
    assert_eq!(r.name.as_deref(), Some("Marco"), "the rest still fills");
    assert_eq!(r.comment.as_deref(), Some("IOTA EU-025 Lipari Islands"));
    every_upload_carries(&mut e, "IA5X", "EU-025");
}

/// NOTHING INVENTED: no spot and no answer, the contact logs no reference and its comment is
/// untouched.
#[test]
fn with_no_source_nothing_is_invented() {
    let (mut e, _) = spotted(SpotIota::Silent);
    e.settings.auto_log = true;
    e.settings.log_reports_to_comments = true;
    work(&mut e, "IA5X", 1);
    let r = logged(&e, "IA5X");
    assert_eq!(r.ota.iota, None);
    assert!(!is_offer_held(&r, "IOTA"));
    let c = r.comment.unwrap_or_default();
    assert!(!c.contains("IOTA"), "{c}");
    assert!(c.contains("Sent:"), "premise: the reports comment: {c}");

    // A station with no reader (no spots, no list) writes nothing either.
    let mut bare_station = station();
    bare_station.settings.auto_log = true;
    bare_station.note_callbook_answer("IA5Y", answer("IA5Y"));
    work(&mut bare_station, "IA5Y", 1);
    assert_eq!(logged(&bare_station, "IA5Y").ota.iota, None);
}

/// THE COMMENT IS KEPT: the group's name follows what is there, once. A contact whose comment
/// already names the group gets nothing more.
#[test]
fn an_existing_comment_is_kept_and_the_name_appended_once() {
    let (mut e, _) = spotted(names("EU-005"));
    e.settings.auto_log = true;
    e.settings.log_reports_to_comments = true;
    work(&mut e, "IA5X", 1);
    let c = logged(&e, "IA5X").comment.unwrap_or_default();
    assert!(
        c.ends_with("; IOTA EU-005 Great Britain"),
        "the reports comment, then the name: {c}"
    );
    assert_eq!(c.matches("Great Britain").count(), 1, "{c}");

    let mut form = bare(&e, "IA5N");
    form.ota.iota = Some("EU-005".into());
    form.comment = Some("op Ian, great britain again".into());
    e.log_form_qso(form, StateSource::Operator, Vec::new());
    assert_eq!(
        logged(&e, "IA5N").comment.as_deref(),
        Some("op Ian, great britain again"),
        "already named: left as it is"
    );
}

/// Log QSO with Auto-log and the popup both off is the same contact with no popup.
#[test]
fn the_log_qso_button_with_no_popup_logs_the_spots_reference() {
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
    assert!(is_offer_held(&r, "IOTA"), "marked as Nexus's fill");
}

/// EVERY PATH NO FORM SHOWS: a plain `log_qso` caller and Remote's synced log take the callbook's
/// reference too, and send it.
#[test]
fn a_plain_and_a_synced_log_fill_the_reference() {
    let (mut e, _) = spotted(SpotIota::Silent);
    for call in ["IA5PLAIN", "IA5SYNC"] {
        e.note_callbook_answer(call, answer(call));
    }
    let _ = e.take_pending_uploads();
    let plain = bare(&e, "IA5PLAIN");
    e.log_qso(plain);
    let sync = bare(&e, "IA5SYNC");
    let _ = e.log_qso_for_sync(sync);
    let queued = e.take_pending_uploads();
    for call in ["IA5PLAIN", "IA5SYNC"] {
        let r = logged(&e, call);
        assert_eq!(r.name.as_deref(), Some("Marco"), "{call}: premise, filled");
        assert_eq!(r.ota.iota.as_deref(), Some("EU-025"), "{call}");
        assert!(is_callbook_filled(&r, "IOTA"), "{call}");
        let sent: Vec<String> = queued
            .iter()
            .filter(|u| u.rec.call == call)
            .map(|u| tempo_core::logbook::adif_record(&u.rec))
            .collect();
        assert!(!sent.is_empty(), "premise: {call} is queued for upload");
        assert!(sent.iter().all(|a| a.contains("<IOTA:6>EU-025")), "{call}");
    }
}

/// THE LOG FORMS: the form put the reference in its box and logs what the box holds; the station
/// never fills one behind it, so a reference the operator cleared stays cleared.
#[test]
fn a_log_form_logs_what_its_box_holds_and_nothing_behind_it() {
    let (mut e, asked) = spotted(names("EU-028"));
    e.note_callbook_answer("IA5F", answer("IA5F"));
    let form = bare(&e, "IA5F");
    e.log_form_qso(form, StateSource::Operator, Vec::new());
    let r = logged(&e, "IA5F");
    assert_eq!(r.ota.iota, None, "cleared on the form: logged with none");
    assert!(asked.lock().unwrap().is_empty(), "the spots are not asked");
}

/// The log form's box filled from the spot and logged as it was (`log_qso` passes the offer with
/// the record): marked as the spot's, in every upload.
#[test]
fn a_form_logged_with_its_filled_reference_sends_it() {
    use tempo_core::logbook::offer::accept_iota_offer;
    let (mut e, _) = spotted(SpotIota::Silent);
    let _ = e.take_pending_uploads();
    let mut form = bare(&e, "IA5F");
    form.ota.iota = Some("EU-028".into());
    let offer = IotaOffer {
        reference: "EU-028".into(),
        source: OfferSource::Spot,
    };
    assert!(accept_iota_offer(&mut form, &offer));
    e.log_form_qso(form, StateSource::Operator, Vec::new());
    let r = logged(&e, "IA5F");
    assert_eq!(r.ota.iota.as_deref(), Some("EU-028"));
    assert!(is_offer_held(&r, "IOTA"));
    assert_eq!(r.comment.as_deref(), Some("IOTA EU-028 Sicily"));
    every_upload_carries(&mut e, "IA5F", "EU-028");
}

/// THE POPUP SHOWS IT: the held contact carries the reference before the operator sees it, and
/// confirmed as shown it is logged and sent.
#[test]
fn the_popup_is_filled_and_logs_the_reference_as_shown() {
    let (mut e, _) = spotted(SpotIota::Silent);
    e.settings.prompt_to_log = true;
    e.note_callbook_answer("IA5X", answer("IA5X"));
    let _ = e.take_pending_uploads();
    work(&mut e, "IA5X", 1);
    let key = e
        .pending_qso_log_key()
        .expect("premise: the popup holds it");
    let shown = e.pending_log().cloned().expect("held");
    assert_eq!(
        shown.ota.iota.as_deref(),
        Some("EU-025"),
        "the popup opens with it"
    );
    assert!(e.confirm_pending_log(&key, shown));
    let r = logged(&e, "IA5X");
    assert_eq!(r.ota.iota.as_deref(), Some("EU-025"));
    assert!(is_callbook_filled(&r, "IOTA"));
    assert_eq!(r.comment.as_deref(), Some("IOTA EU-025 Lipari Islands"));
    every_upload_carries(&mut e, "IA5X", "EU-025");
}

/// The operator changes the popup's reference, or clears it: what they left is logged, a changed
/// one is theirs (no fill marks), a cleared one logs none.
#[test]
fn the_popup_reference_can_be_changed_or_cleared() {
    let (mut e, _) = spotted(names("EU-028"));
    e.settings.prompt_to_log = true;
    work(&mut e, "IA5X", 1);
    let key = e.pending_qso_log_key().expect("held");
    let mut shown = e.pending_log().cloned().expect("held");
    assert_eq!(shown.ota.iota.as_deref(), Some("EU-028"), "premise");
    shown.ota.iota = Some("eu-025".into());
    assert!(e.confirm_pending_log(&key, shown));
    let r = logged(&e, "IA5X");
    assert_eq!(r.ota.iota.as_deref(), Some("EU-025"));
    assert!(!is_offer_held(&r, "IOTA"), "the operator's now");
    assert!(!is_callbook_filled(&r, "IOTA"));

    work(&mut e, "IA5Y", 11);
    let key = e.pending_qso_log_key().expect("held");
    let mut shown = e.pending_log().cloned().expect("held");
    assert_eq!(shown.ota.iota.as_deref(), Some("EU-028"), "premise");
    shown.ota.iota = None;
    assert!(e.confirm_pending_log(&key, shown));
    let r = logged(&e, "IA5Y");
    assert_eq!(r.ota.iota, None, "cleared: logged with none");
    assert!(!is_offer_held(&r, "IOTA"));
    assert!(!r.comment.unwrap_or_default().contains("IOTA"));
}

/// The popup's box filled by the UI's own ask (a lookup that answered after the hold) and logged as
/// it was: marked as that source's. A reference the contact holds is never replaced by an offer.
#[test]
fn the_popup_marks_a_reference_the_ui_filled_and_never_replaces_one() {
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

    let rec = bare(&e, "IA5W");
    e.load_pending_qso(rec);
    let key = e.pending_qso_log_key().expect("held");
    let mut shown = e.pending_log().cloned().expect("held");
    shown.ota.iota = Some("EU-025".into());
    assert!(e.confirm_pending_log_with_offer(&key, shown, Some(&offer)));
    let r = logged(&e, "IA5W");
    assert_eq!(r.ota.iota.as_deref(), Some("EU-025"));
    assert!(is_offer_held(&r, "IOTA"), "the spot's fill");
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
        assert!(!is_callbook_filled(&r, "IOTA"), "{group}: says from a spot");
    }
}

/// Spots only on another band and a held callbook answer that name the same group write it,
/// marked as the callbook's. Naming another group, or with no answer, they write nothing.
#[test]
fn a_spot_on_another_band_needs_a_callbook_answer_naming_the_same_group() {
    let group = answer("IA5X").iota.expect("the fixture names a group");
    let (mut e, _) = spotted(SpotIota::OtherBand(group.clone()));
    e.settings.auto_log = true;
    e.note_callbook_answer("IA5X", answer("IA5X"));
    work(&mut e, "IA5X", 1);
    let r = logged(&e, "IA5X");
    assert_eq!(r.ota.iota, Some(group));
    assert!(is_callbook_filled(&r, "IOTA"), "says from the callbook");

    let (mut e, _) = spotted(SpotIota::OtherBand("AS-001".into()));
    e.settings.auto_log = true;
    e.note_callbook_answer("IA5X", answer("IA5X"));
    work(&mut e, "IA5X", 1);
    assert_eq!(logged(&e, "IA5X").ota.iota, None, "they disagree");

    let (mut e, _) = spotted(SpotIota::OtherBand("AS-001".into()));
    e.settings.auto_log = true;
    work(&mut e, "IA5X", 1);
    assert_eq!(logged(&e, "IA5X").ota.iota, None, "no answer in hand");
}

/// Spots that disagree with each other write nothing from either.
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

/// REVIEW REPRO: a call corrected in the popup must not keep the busted call's filled reference.
/// The hold filled IA5X's callbook group; corrected to IA5Y, the box still holding that fill, the
/// contact logs IA5Y's own fill (its callbook answer) or none, never IA5X's, and no fill mark of
/// IA5X's travels with it.
#[test]
fn a_call_corrected_in_the_popup_drops_the_busted_calls_fill_and_takes_its_own() {
    use tempo_core::logbook::callbook::CALLBOOK_FILLED_FIELD;
    use tempo_core::logbook::offer::OFFER_HELD_FIELD;
    for (new_answer, want) in [(None, None), (Some("EU-001"), Some("EU-001"))] {
        let (mut e, _) = spotted(SpotIota::Silent);
        e.settings.prompt_to_log = true;
        e.note_callbook_answer("IA5X", answer("IA5X"));
        if let Some(group) = new_answer {
            let mut a = answer("IA5Y");
            a.iota = Some(group.into());
            e.note_callbook_answer("IA5Y", a);
        }
        work(&mut e, "IA5X", 1);
        let key = e.pending_qso_log_key().expect("held");
        let mut shown = e.pending_log().cloned().expect("held");
        assert_eq!(shown.ota.iota.as_deref(), Some("EU-025"), "premise: filled");
        shown.call = "IA5Y".into();
        assert!(e.confirm_pending_log(&key, shown));
        let r = logged(&e, "IA5Y");
        assert_eq!(r.ota.iota.as_deref(), want, "{new_answer:?}");
        let marks: Vec<_> = r
            .extra
            .iter()
            .filter(|(k, v)| {
                (k == OFFER_HELD_FIELD || k == CALLBOOK_FILLED_FIELD) && v.contains("IOTA")
            })
            .collect();
        assert_eq!(
            marks.is_empty(),
            want.is_none(),
            "{new_answer:?}: {marks:?}"
        );
        assert!(
            !r.comment.unwrap_or_default().contains("EU-025"),
            "no trace of the other station's group"
        );
    }
}

/// …and a reference the operator put in the box, or one they typed onto the contact, stays when
/// the call is corrected: only our own fill is re-derived.
#[test]
fn a_call_correction_keeps_a_reference_the_operator_chose() {
    let (mut e, _) = spotted(SpotIota::Silent);
    e.settings.prompt_to_log = true;
    e.note_callbook_answer("IA5X", answer("IA5X"));
    work(&mut e, "IA5X", 1);
    let key = e.pending_qso_log_key().expect("held");
    let mut shown = e.pending_log().cloned().expect("held");
    shown.call = "IA5Y".into();
    shown.ota.iota = Some("EU-031".into());
    assert!(e.confirm_pending_log(&key, shown));
    let r = logged(&e, "IA5Y");
    assert_eq!(
        r.ota.iota.as_deref(),
        Some("EU-031"),
        "changed in the box: theirs"
    );
    assert!(!is_offer_held(&r, "IOTA"));

    let mut typed = bare(&e, "IA5Z");
    typed.ota.iota = Some("EU-001".into());
    e.load_pending_qso(typed);
    let key = e.pending_qso_log_key().expect("held");
    let mut shown = e.pending_log().cloned().expect("held");
    shown.call = "IA5W".into();
    assert!(e.confirm_pending_log(&key, shown));
    assert_eq!(
        logged(&e, "IA5W").ota.iota.as_deref(),
        Some("EU-001"),
        "typed: kept"
    );
}
