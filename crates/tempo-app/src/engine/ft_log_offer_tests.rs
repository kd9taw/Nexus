//! When a finished FT8/FT4 contact is logged by itself, offered in the confirm-before-log popup,
//! or left for Log QSO (operator rulings, 2026-10-27).
//!
//! A contact opened by a click on the station's RR73 or RRR exchanges no report: it is OFFERED,
//! never logged without the operator's confirm, on every path.

use super::tests::dec_snr;
use super::*;
use crate::test_util::StoredLog;

const ME: &str = "KR4FQG";
const DX: &str = "7P8RU";

/// A fresh FT8 engine for `ME` on 20 m.
fn ft8() -> Engine {
    let mut e = Engine::new(ME, "EM85", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e
}

fn tx_now(e: &Engine) -> String {
    e.snapshot()
        .qso
        .as_ref()
        .and_then(|q| q.tx_now.clone())
        .unwrap_or_default()
}

/// Transmit once on whichever of the two slots is ours; the text that went out.
fn key_once(e: &mut Engine, s: u64) -> String {
    let text = tx_now(e);
    for slot in [s, s + 1] {
        if !e.poll_tx(slot).is_empty() {
            return text;
        }
    }
    panic!("nothing keyed at slot {s} or {} (queued: {text:?})", s + 1)
}

/// Click the station's RR73 to us (decoded at `slot`), send our 73, and let the next period
/// fold. Returns nothing; the 73 going out is asserted.
fn click_their_rr73_and_send_73(e: &mut Engine, slot: u64) {
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU RR73", -9)], slot);
    e.call_station(DX);
    assert_eq!(
        key_once(e, slot + 1),
        "7P8RU KR4FQG 73",
        "premise: our 73 goes out"
    );
    e.ingest_decodes_for_test(&[], slot + 4);
}

/// An ordinary finished contact record for the queue, for `call`.
fn held(e: &Engine, call: &str) -> HeldQso {
    HeldQso::new(
        e.qso_record(call.into(), None, Some(-10)),
        GridSource::LookedUp,
    )
}

fn calls_logged(e: &Engine) -> Vec<String> {
    e.stored_log().iter().map(|r| r.call.clone()).collect()
}

/// (1) The fold offers the RR73-click contact OFFER-ONLY, and a full queue never logs it: the cap
/// takes the oldest waiting contact that is not offer-only instead, and the offer keeps waiting.
#[test]
fn a_full_queue_never_logs_an_offer_only_contact() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = true;
    e.hold_pending_log(held(&e, "W1AW")); // the popup is showing this one
    click_their_rr73_and_send_73(&mut e, 21);
    assert_eq!(e.pending_logs.len(), 2, "premise: offered behind the popup");
    assert!(
        e.pending_logs[1].confirm_only,
        "the fold marks it offer-only"
    );
    for i in 0..PENDING_LOG_QUEUE_CAP {
        let h = held(&e, &format!("K{i}AA"));
        e.hold_pending_log(h);
    }
    let logged = calls_logged(&e);
    assert!(
        !logged.iter().any(|c| c == DX),
        "never logged without the confirm: {logged:?}"
    );
    assert_eq!(
        logged.first().map(String::as_str),
        Some("K0AA"),
        "the cap took the oldest contact that is not offer-only"
    );
    assert!(
        e.pending_logs
            .iter()
            .any(|h| h.record.call == DX && h.confirm_only),
        "the offer keeps waiting"
    );
}

/// (1) When every waiting contact is offer-only the cap takes none, and nothing is lost.
#[test]
fn a_queue_of_offer_only_contacts_grows_rather_than_logging_one() {
    let mut e = ft8();
    e.settings.prompt_to_log = true;
    for i in 0..=PENDING_LOG_QUEUE_CAP {
        let mut h = held(&e, &format!("K{i}AA"));
        h.confirm_only = i > 0;
        e.hold_pending_log(h);
    }
    assert!(e.stored_log().is_empty(), "{:?}", calls_logged(&e));
    assert_eq!(e.pending_logs.len(), PENDING_LOG_QUEUE_CAP + 1);
}

/// (1) The mark is journalled with the queue: a restart restores the contact offer-only. A
/// journal of ordinary contacts carries no trace of it, so it reads and writes as before.
#[test]
fn an_offer_only_contact_stays_offer_only_across_a_restart() {
    let dir = std::env::temp_dir().join(format!(
        "nexus-offer-only-{}-{}",
        std::process::id(),
        now_unix_secs()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pending.json");
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    e.set_pending_qso_path(path.clone());
    e.hold_pending_log(held(&e, "W1AW"));
    let ordinary = std::fs::read_to_string(&path).unwrap();
    assert!(!ordinary.contains("confirmOnly"), "{ordinary}");
    click_their_rr73_and_send_73(&mut e, 21);
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("\"confirmOnly\":true").count(), 1, "{text}");

    let mut r = ft8();
    r.set_pending_qso_path(path.clone());
    r.restore_pending_qso_journal(now_unix_secs() as i64);
    let back: Vec<(String, bool)> = r
        .pending_logs
        .iter()
        .map(|h| (h.record.call.clone(), h.confirm_only))
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(back, [("W1AW".into(), false), (DX.into(), true)]);
}

/// (1) Remote's Log QSO journals the mark too: its hold is written by its own path.
#[test]
fn remotes_log_qso_journals_the_offer_only_mark() {
    let dir = std::env::temp_dir().join(format!(
        "nexus-offer-only-remote-{}-{}",
        std::process::id(),
        now_unix_secs()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pending.json");
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    e.set_pending_qso_path(path.clone());
    click_their_rr73_and_send_73(&mut e, 21);
    let remote_logging::CurrentQsoLogOutcome::Pending(write) = e.log_current_qso_for_sync() else {
        panic!("Remote's Log QSO holds it in the popup")
    };
    let prepared = write.prepare().unwrap();
    e.publish_pending_qso_journal(prepared)
        .unwrap()
        .sync()
        .unwrap();
    let mut r = ft8();
    r.set_pending_qso_path(path.clone());
    r.restore_pending_qso_journal(now_unix_secs() as i64);
    let back: Vec<(String, bool)> = r
        .pending_logs
        .iter()
        .map(|h| (h.record.call.clone(), h.confirm_only))
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(back, [(DX.into(), true)]);
}

/// An S&P contact with `DX` from their CQ (decoded at `slot`) to our 73: reports both ways.
fn work_dx_to_73(e: &mut Engine, slot: u64) {
    e.ingest_decodes_for_test(&[dec_snr("CQ 7P8RU KG30", -12)], slot);
    e.call_station(DX);
    key_once(e, slot + 1);
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU -15", -9)], slot + 4);
    assert_eq!(
        key_once(e, slot + 5),
        "7P8RU KR4FQG R-09",
        "premise: our roger"
    );
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU RR73", -9)], slot + 8);
    assert_eq!(
        key_once(e, slot + 9),
        "7P8RU KR4FQG 73",
        "premise: our 73"
    );
    e.ingest_decodes_for_test(&[], slot + 12);
}

/// (3) WSJT-X offers the contact on the FIRST 73 only. Their RR73 repeated after the offer was
/// confirmed and logged: a click sends 73 again, and nothing is offered, by the fold or the Log
/// button.
#[test]
fn a_repeated_rr73_clicked_after_the_offer_is_not_offered_again() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    click_their_rr73_and_send_73(&mut e, 21);
    let shown = e.snapshot().pending_log.expect("premise: offered");
    let key = e.pending_qso_log_key().unwrap_or_default();
    assert!(e.confirm_pending_log(&key, shown.into()));
    assert_eq!(e.stored_log().len(), 1, "premise: logged once confirmed");

    click_their_rr73_and_send_73(&mut e, 41);
    assert!(
        e.snapshot().pending_log.is_none(),
        "the repeat is not offered again"
    );
    assert!(!e.log_current_qso(), "nor by the Log button");
    assert_eq!(e.stored_log().len(), 1);
    assert!(e.pending_logs.is_empty());
}

/// (3) The same after an ordinary contact that logged itself: a click on their repeated RR73
/// sends 73 again, and no report-less copy is offered.
#[test]
fn a_repeated_rr73_clicked_after_a_logged_contact_is_not_offered() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    work_dx_to_73(&mut e, 11);
    assert_eq!(calls_logged(&e), [DX], "premise: the contact logged itself");
    click_their_rr73_and_send_73(&mut e, 41);
    assert!(e.snapshot().pending_log.is_none(), "nothing offered");
    assert!(!e.log_current_qso(), "nor by the Log button");
    assert_eq!(e.stored_log().len(), 1);
}

/// (3) CONTROL: the first 73 to that station on ANOTHER band is offered: the rule is per contact,
/// and a contact on another band is another contact.
#[test]
fn a_first_73_to_the_same_station_on_another_band_is_offered() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    work_dx_to_73(&mut e, 11);
    e.set_frequency(7.074, "40m", "USB");
    click_their_rr73_and_send_73(&mut e, 41);
    let shown = e.snapshot().pending_log.expect("offered on 40 m");
    assert_eq!((shown.call.as_str(), shown.band.as_str()), (DX, "40m"));
}
