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
    assert_eq!(key_once(e, slot + 9), "7P8RU KR4FQG 73", "premise: our 73");
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

/// An S&P contact with `call` from their CQ (decoded at `slot`) to our 73: reports both ways.
fn work_to_73(e: &mut Engine, call: &str, slot: u64) {
    e.ingest_decodes_for_test(&[dec_snr(&format!("CQ {call} FN31"), -12)], slot);
    e.call_station(call);
    key_once(e, slot + 1);
    e.ingest_decodes_for_test(&[dec_snr(&format!("KR4FQG {call} -15"), -9)], slot + 4);
    key_once(e, slot + 5);
    e.ingest_decodes_for_test(&[dec_snr(&format!("KR4FQG {call} RR73"), -9)], slot + 8);
    assert_eq!(
        key_once(e, slot + 9),
        format!("{call} KR4FQG 73"),
        "premise: our 73"
    );
    e.ingest_decodes_for_test(&[], slot + 12);
}

/// (3) The first-73 rule is WSJT-X's (`m_sentFirst73`, `widgets/mainwindow.cpp` 3.0.2 8105-8108):
/// our previous over to the station was already a 73, so this 73 is not offered. A contact with
/// another station completing in between changes nothing: it is not this station's.
#[test]
fn a_repeated_rr73_clicked_after_another_contact_is_still_not_offered() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    work_dx_to_73(&mut e, 11);
    work_to_73(&mut e, "W2XYZ", 31);
    assert_eq!(calls_logged(&e), [DX, "W2XYZ"], "premise: both logged");
    click_their_rr73_and_send_73(&mut e, 51);
    assert!(
        e.snapshot().pending_log.is_none(),
        "the repeat is not offered"
    );
    assert!(!e.log_current_qso(), "nor by the Log button");
    assert_eq!(e.stored_log().len(), 2);
}

/// (3) …and a 73 whose previous over to the station was NOT a 73 is a first 73, offered, even
/// with the station's earlier contact logged: here we called them again and they answered our
/// call with RR73.
#[test]
fn a_first_73_after_a_new_call_to_a_logged_station_is_offered() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    work_dx_to_73(&mut e, 11);
    e.ingest_decodes_for_test(&[dec_snr("CQ 7P8RU KG30", -12)], 41);
    e.call_station(DX);
    assert_eq!(
        key_once(&mut e, 42),
        "7P8RU KR4FQG EM85",
        "premise: a new call to them"
    );
    click_their_rr73_and_send_73(&mut e, 45);
    let shown = e.snapshot().pending_log.expect("a first 73: offered");
    assert_eq!(shown.call, DX);
    assert_eq!(e.stored_log().len(), 1, "not logged without the confirm");
}

/// (3) A first 73 to ANOTHER station right after one to this station is that station's first 73,
/// and is offered.
#[test]
fn a_first_73_to_another_station_right_after_one_is_offered() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = true;
    click_their_rr73_and_send_73(&mut e, 21);
    assert_eq!(e.pending_logs.len(), 1, "premise: offered");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W2XYZ RR73", -9)], 31);
    e.call_station("W2XYZ");
    assert_eq!(key_once(&mut e, 32), "W2XYZ KR4FQG 73", "premise: our 73");
    e.ingest_decodes_for_test(&[], 35);
    let calls: Vec<_> = e
        .pending_logs
        .iter()
        .map(|h| h.record.call.clone())
        .collect();
    assert_eq!(calls, [DX, "W2XYZ"], "both offered");
}

/// (3) A contact a band change set aside with auto-log and the prompt off, then confirmed from
/// Log QSO: a click on their repeated RR73 back on that band sends 73 again and offers nothing.
#[test]
fn a_repeat_after_a_set_aside_contact_was_logged_is_not_offered() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    click_their_rr73_send_73_and_change_band(&mut e);
    assert!(e.log_current_qso(), "premise: Log QSO offers it");
    let shown = e.snapshot().pending_log.expect("premise: in the popup");
    let key = e.pending_qso_log_key().unwrap_or_default();
    assert!(e.confirm_pending_log(&key, shown.into()));
    assert_eq!(calls_logged(&e), [DX], "premise: logged");
    e.set_frequency(14.074, "20m", "USB");
    click_their_rr73_and_send_73(&mut e, 41);
    assert!(!e.log_current_qso(), "the repeat is not offered");
    assert!(e.pending_logs.is_empty());
    assert_eq!(e.stored_log().len(), 1);
}

/// With auto-log and the prompt off, a contact run to 73 with reports both ways waits for Log
/// QSO. A click on their repeated RR73 then sends 73 again, and Log QSO still logs the COMPLETE
/// contact, reports and all: the click no longer swaps it for a report-less copy.
#[test]
fn a_reclick_keeps_the_complete_contact_for_log_qso() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    work_dx_to_73(&mut e, 11);
    assert!(e.stored_log().is_empty(), "premise: waits for Log QSO");
    click_their_rr73_and_send_73(&mut e, 41);
    assert!(e.log_current_qso(), "Log QSO takes the complete contact");
    assert!(e.pending_logs.is_empty(), "logged, not offered");
    let logged = e.stored_log();
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].call, DX);
    assert_eq!(
        (logged[0].rst_sent.as_deref(), logged[0].rst_rcvd.as_deref()),
        (Some("-09"), Some("-15")),
        "with the reports exchanged"
    );
    assert!(!e.log_current_qso(), "once");
}

/// …and our 73 to a roger the operator clicked, replaced by a click on another station before Log
/// QSO was pressed, stays for Log QSO OFFER-ONLY: it opens the popup and is not logged.
#[test]
fn a_click_elsewhere_keeps_the_roger_contact_offer_only() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    click_their_rr73_and_send_73(&mut e, 21);
    e.ingest_decodes_for_test(&[dec_snr("CQ W2XYZ FN31", -12)], 31);
    e.call_station("W2XYZ");
    assert!(e.log_current_qso(), "Log QSO takes it");
    assert!(e.stored_log().is_empty(), "not logged without the confirm");
    let shown = e.snapshot().pending_log.expect("offered in the popup");
    assert_eq!(shown.call, DX);
    assert!(e.pending_logs[0].confirm_only, "offer-only");
}

/// Click their RR73 (decoded at 21), send our 73 at 22, and change band to 40 m before the next
/// fold: the fold that would have offered the contact never comes for it.
fn click_their_rr73_send_73_and_change_band(e: &mut Engine) {
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU RR73", -9)], 21);
    e.call_station(DX);
    assert_eq!(key_once(e, 22), "7P8RU KR4FQG 73", "premise: our 73");
    e.set_frequency(7.074, "40m", "USB");
    e.ingest_decodes_for_test(&[], 25);
}

/// (2) A band change between our 73 and the fold: the contact is offered at the band change, on
/// the band it was made on, offer-only, and once.
#[test]
fn a_band_change_after_our_73_offers_the_contact_on_its_own_band() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    click_their_rr73_send_73_and_change_band(&mut e);
    assert!(e.stored_log().is_empty(), "never logged by itself");
    let shown = e
        .snapshot()
        .pending_log
        .expect("offered at the band change");
    assert_eq!((shown.call.as_str(), shown.band.as_str()), (DX, "20m"));
    assert!(
        (14.0..14.35).contains(&shown.freq_mhz),
        "{}",
        shown.freq_mhz
    );
    assert_eq!(e.pending_logs.len(), 1, "offered once");
    assert!(e.pending_logs[0].confirm_only, "offer-only");
}

/// (2) With auto-log and the prompt off nothing is offered by itself; the contact waits for Log
/// QSO, which offers it, on its own band, and does not log it.
#[test]
fn a_band_change_after_our_73_keeps_the_contact_for_log_qso() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    click_their_rr73_send_73_and_change_band(&mut e);
    assert!(e.pending_logs.is_empty(), "nothing offered by itself");
    assert!(e.log_current_qso(), "Log QSO takes it");
    assert!(e.stored_log().is_empty(), "not logged without the confirm");
    let shown = e.snapshot().pending_log.expect("offered in the popup");
    assert_eq!((shown.call.as_str(), shown.band.as_str()), (DX, "20m"));
    assert!(e.pending_logs[0].confirm_only, "offer-only");
}

/// (2) …and on the band it was made on when the dial has already left it: the rig read back on
/// 40 m after a listen to WWV between.
#[test]
fn a_band_change_at_the_rig_after_our_73_offers_the_contact_on_the_band_left() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    e.observe_rig_freq(14_074_000);
    let made_on = e.log_frequencies().0;
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU RR73", -9)], 21);
    e.call_station(DX);
    assert_eq!(key_once(&mut e, 22), "7P8RU KR4FQG 73", "premise: our 73");
    e.observe_rig_freq(10_000_000);
    e.observe_rig_freq(7_074_000);
    let shown = e
        .snapshot()
        .pending_log
        .expect("offered at the band change");
    assert_eq!((shown.call.as_str(), shown.band.as_str()), (DX, "20m"));
    assert!(
        (shown.freq_mhz - made_on).abs() < 1e-9,
        "at {made_on}, not {}",
        shown.freq_mhz
    );
}

/// INVARIANT (A), as on origin/main: an ordinary contact, reports both ways, with Auto-log on and
/// the confirm popup off, logs itself as it completes, and no popup opens.
#[test]
fn invariant_a_auto_log_on_prompt_off_logs_the_contact_by_itself() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    work_dx_to_73(&mut e, 11);
    assert_eq!(calls_logged(&e), [DX], "logged by itself");
    let rec = e.stored_log()[0].clone();
    assert_eq!(
        (rec.rst_sent.as_deref(), rec.rst_rcvd.as_deref()),
        (Some("-09"), Some("-15")),
        "with the reports exchanged"
    );
    assert!(e.snapshot().pending_log.is_none(), "no popup");
    assert!(e.pending_logs.is_empty());
}

/// A CQ run: our CQ, W1AW answers with a grid, our report, their roger, our RR73, and one quiet
/// period. Returns what the run queues next.
fn run_one_contact(e: &mut Engine) -> String {
    e.set_mode("qso-run").expect("the run starts");
    assert!(key_once(e, 0).starts_with("CQ KR4FQG"), "premise: CQ");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 2);
    assert_eq!(key_once(e, 2), "W1AW KR4FQG -07", "premise: our report");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW R-10", -7)], 4);
    assert_eq!(key_once(e, 4), "W1AW KR4FQG RR73", "premise: our RR73");
    e.ingest_decodes_for_test(&[], 6);
    tx_now(e)
}

/// INVARIANT (B): a CQ run calls CQ again after a contact, whether it logged itself or waits in
/// the popup, the popup opened with Auto-log on or, now, off.
#[test]
fn invariant_b_a_cq_run_calls_cq_again_after_a_contact() {
    for (auto_log, prompt, logged, held) in
        [(true, false, 1, 0), (true, true, 0, 1), (false, true, 0, 1)]
    {
        let mut e = ft8();
        e.settings.auto_log = auto_log;
        e.settings.prompt_to_log = prompt;
        let next = run_one_contact(&mut e);
        let case = format!("auto-log {auto_log}, prompt {prompt}");
        assert!(
            next.starts_with("CQ KR4FQG"),
            "{case}: the run calls CQ again, queued {next:?}"
        );
        assert_eq!(e.stored_log().len(), logged, "{case}: logged");
        assert_eq!(e.pending_logs.len(), held, "{case}: held in the popup");
    }
}

/// CONTROL, unchanged from origin/main: with Auto-log and the popup both off nothing claims the
/// contact, so the run waits on Log QSO, which logs it and lets the run call CQ again.
#[test]
fn with_auto_log_and_the_popup_off_the_run_waits_for_log_qso() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    let next = run_one_contact(&mut e);
    assert!(!next.starts_with("CQ"), "waits: {next:?}");
    assert!(e.pending_logs.is_empty() && e.stored_log().is_empty());
    assert!(e.log_current_qso(), "Log QSO logs it");
    assert_eq!(e.stored_log().len(), 1);
    e.ingest_decodes_for_test(&[], 8);
    assert!(tx_now(&e).starts_with("CQ KR4FQG"), "{}", tx_now(&e));
}

/// (4) "Prompt me to log QSO" with Auto-log OFF: the popup opens by itself as the contact
/// completes, as WSJT-X's does, and the contact is offered once: Log QSO then has nothing more
/// to offer.
#[test]
fn the_popup_opens_by_itself_with_auto_log_off() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = true;
    work_dx_to_73(&mut e, 11);
    assert!(
        e.stored_log().is_empty(),
        "nothing logged before the confirm"
    );
    let shown = e
        .snapshot()
        .pending_log
        .expect("the popup opened by itself");
    assert_eq!(shown.call, DX);
    assert!(!e.log_current_qso(), "Log QSO does not offer it again");
    assert_eq!(e.pending_logs.len(), 1, "offered once");
    let key = e.pending_qso_log_key().unwrap_or_default();
    assert!(e.confirm_pending_log(&key, shown.into()));
    assert_eq!(calls_logged(&e), [DX]);
}

/// (4) …and for the contact after a click on their RR73: offered by itself, offer-only.
#[test]
fn the_popup_opens_by_itself_for_an_rr73_click_with_auto_log_off() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = true;
    click_their_rr73_and_send_73(&mut e, 21);
    assert!(e.stored_log().is_empty());
    assert_eq!(
        e.snapshot().pending_log.map(|r| r.call).as_deref(),
        Some(DX),
        "the popup opened by itself"
    );
    assert!(e.pending_logs[0].confirm_only);
    assert!(!e.log_current_qso(), "offered once");
    assert_eq!(e.pending_logs.len(), 1);
}

/// (4) Unchanged: with both off nothing happens by itself; Log QSO logs the contact.
#[test]
fn with_both_off_nothing_logs_until_log_qso() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    work_dx_to_73(&mut e, 11);
    assert!(e.stored_log().is_empty() && e.pending_logs.is_empty());
    assert!(e.log_current_qso(), "Log QSO logs it");
    assert_eq!(calls_logged(&e), [DX]);
    assert!(e.pending_logs.is_empty(), "with no popup");
}

/// (4) The start time with the popup alone is the one Auto-log takes: estimated as our closing
/// over leaves, as WSJT-X does with either setting on (`widgets/mainwindow.cpp` 3.0.2 8113).
#[test]
fn the_popup_alone_takes_the_start_time_as_the_closing_over_leaves() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = true;
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU R-15", -9)], 21);
    e.call_station(DX);
    let before = now_unix_secs();
    key_once(&mut e, 22);
    let after = now_unix_secs();
    let window = wsjtx_qso_on(before, 4, 15.0)..=wsjtx_qso_on(after, 4, 15.0);
    assert!(
        e.qso_start_unix.is_some_and(|on| window.contains(&on)),
        "estimated as the RR73 leaves: {:?}",
        e.qso_start_unix
    );
}

/// (4) Remote's Log QSO follows the same rules: the contact the popup opened for by itself is
/// not offered a second time from Remote, before the confirm or after it.
#[test]
fn remotes_log_qso_does_not_offer_a_contact_the_popup_already_has() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = true;
    work_dx_to_73(&mut e, 11);
    assert!(matches!(
        e.log_current_qso_for_sync(),
        remote_logging::CurrentQsoLogOutcome::PendingExists
    ));
    let shown = e.snapshot().pending_log.expect("premise: the popup opened");
    let key = e.pending_qso_log_key().unwrap_or_default();
    assert!(e.confirm_pending_log(&key, shown.into()));
    assert!(matches!(
        e.log_current_qso_for_sync(),
        remote_logging::CurrentQsoLogOutcome::NoEligibleContact
    ));
    assert_eq!(calls_logged(&e), [DX], "logged once");
}

/// (4) …and at a band change: a contact complete when the band changes opens the popup there,
/// on the band it was made on, with Auto-log off, as the fold would have; so does our 73 after a
/// click on their RR73.
#[test]
fn with_auto_log_off_the_popup_opens_at_a_band_change() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = true;
    e.set_mode("qso-run").expect("the run starts");
    key_once(&mut e, 0);
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 2);
    key_once(&mut e, 2);
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW R-10", -7)], 4);
    assert_eq!(key_once(&mut e, 4), "W1AW KR4FQG RR73", "premise: our RR73");
    e.set_frequency(7.074, "40m", "USB");
    assert!(e.stored_log().is_empty());
    let shown = e.snapshot().pending_log.expect("the popup opened");
    assert_eq!((shown.call.as_str(), shown.band.as_str()), ("W1AW", "20m"));

    let mut c = ft8();
    c.settings.auto_log = false;
    c.settings.prompt_to_log = true;
    click_their_rr73_send_73_and_change_band(&mut c);
    let shown = c
        .snapshot()
        .pending_log
        .expect("the RR73-click contact too");
    assert_eq!((shown.call.as_str(), shown.band.as_str()), (DX, "20m"));
    assert!(c.pending_logs[0].confirm_only);
}
