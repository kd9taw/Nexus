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

/// (3) As WSJT-X, literally: the rule reads the last over we SENT, whatever its band. A click on
/// the same station's RR73 on another band right after our 73 to them sends a 73 after a 73,
/// and is not offered.
#[test]
fn a_73_to_the_same_station_on_another_band_right_after_our_73_is_not_offered() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    work_dx_to_73(&mut e, 11);
    assert_eq!(calls_logged(&e), [DX], "premise: the 20 m contact logged");
    e.set_frequency(7.074, "40m", "USB");
    click_their_rr73_and_send_73(&mut e, 41);
    assert!(e.snapshot().pending_log.is_none(), "not offered on 40 m");
    assert!(!e.log_current_qso(), "nor by the Log button");
    assert_eq!(e.stored_log().len(), 1);
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
/// the last over we sent was a 73 (here, to another station), so this 73 is not offered.
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

/// (3) …and a 73 whose previous over was NOT a 73 is a first 73, offered, even with the
/// station's earlier contact logged: here we called them again and they answered our call with
/// RR73.
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

/// (3) As WSJT-X, literally: a click on ANOTHER station's RR73 right after our 73 to this one
/// sends a 73 after a 73, and is not offered.
#[test]
fn a_73_to_another_station_right_after_one_is_not_offered() {
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
    assert_eq!(calls, [DX], "only the first is offered");
    assert!(e.stored_log().is_empty(), "nothing logged");
}

/// (3) …and a repeat click on the same station after another over in between (our CQ) is a first
/// 73 again, offered, as WSJT-X prompts again.
#[test]
fn a_repeat_click_after_a_cq_in_between_is_offered() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    click_their_rr73_and_send_73(&mut e, 21);
    let shown = e.snapshot().pending_log.expect("premise: offered");
    let key = e.pending_qso_log_key().unwrap_or_default();
    assert!(e.confirm_pending_log(&key, shown.into()));
    assert_eq!(calls_logged(&e), [DX], "premise: logged once confirmed");
    e.set_mode("qso-run").expect("premise: the CQ run starts");
    assert!(
        key_once(&mut e, 31).starts_with("CQ KR4FQG"),
        "premise: our CQ goes out"
    );
    click_their_rr73_and_send_73(&mut e, 41);
    let shown = e.snapshot().pending_log.expect("offered again");
    assert_eq!(shown.call, DX);
    assert_eq!(e.stored_log().len(), 1, "not logged without the confirm");
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

/// An S&P contact with `DX` from their CQ (decoded at `slot`) up to their RR73, decoded at
/// `slot + 8`: reports both ways, and our 73 queued but not yet on the air.
fn work_dx_to_their_rr73(e: &mut Engine, slot: u64) {
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
}

/// How the operator re-works the station on its RR73 in the window before our 73 airs.
#[derive(Clone, Copy, Debug)]
enum ReClick {
    /// A double-click in Nexus.
    Click,
    /// A logger program's WSJT-X UDP Reply on that RR73 line.
    Reply,
}

fn re_click(e: &mut Engine, how: ReClick) {
    match how {
        ReClick::Click => e.call_station(DX),
        ReClick::Reply => e
            .answer_udp_reply(DX, "KR4FQG 7P8RU RR73", -9, None)
            .expect("premise: the Reply is taken"),
    }
}

/// A click or a logger's Reply on the DX's RR73 BEFORE our 73 to them airs is that same 73, as
/// in WSJT-X, where the log prompt comes as the 73 is sent: the contact logs once, with its
/// reports, and no report-less copy is offered, under every Auto-log / Prompt combination.
/// With both off, Log QSO logs the contact with its reports.
#[test]
fn a_reclick_on_their_rr73_before_our_73_airs_logs_the_contact_once() {
    for how in [ReClick::Click, ReClick::Reply] {
        for (auto_log, prompt) in [(true, false), (true, true), (false, true), (false, false)] {
            let case = format!("{how:?}, auto-log {auto_log}, prompt {prompt}");
            let mut e = ft8();
            e.settings.auto_log = auto_log;
            e.settings.prompt_to_log = prompt;
            work_dx_to_their_rr73(&mut e, 11);
            re_click(&mut e, how);
            assert_eq!(
                key_once(&mut e, 20),
                "7P8RU KR4FQG 73",
                "{case}: premise: one 73 goes out"
            );
            e.ingest_decodes_for_test(&[], 23);
            if !auto_log && !prompt {
                assert!(e.pending_logs.is_empty(), "{case}: nothing offered");
                assert!(e.log_current_qso(), "{case}: Log QSO logs it");
                assert!(!e.log_current_qso(), "{case}: once");
            }
            let mut rows: Vec<(String, Option<String>, Option<String>)> = e
                .stored_log()
                .iter()
                .map(|r| (r.call.clone(), r.rst_sent.clone(), r.rst_rcvd.clone()))
                .collect();
            rows.extend(e.pending_logs.iter().map(|h| {
                (
                    h.record.call.clone(),
                    h.record.rst_sent.clone(),
                    h.record.rst_rcvd.clone(),
                )
            }));
            assert_eq!(
                rows,
                [(DX.to_string(), Some("-09".into()), Some("-15".into()))],
                "{case}: one contact, with its reports, logged or held"
            );
            assert_eq!(e.stored_log().len(), usize::from(!prompt), "{case}: logged");
            assert!(
                e.pending_logs.iter().all(|h| !h.confirm_only),
                "{case}: no offer-only copy"
            );
        }
    }
}

/// INVARIANT (A) in that window: with Auto-log on and the popup off, the contact still logs
/// itself and no popup opens when the operator double-clicks their RR73 before our 73 airs.
#[test]
fn invariant_a_holds_when_their_rr73_is_clicked_before_our_73_airs() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    work_dx_to_their_rr73(&mut e, 11);
    e.call_station(DX);
    e.call_station(DX);
    assert_eq!(key_once(&mut e, 20), "7P8RU KR4FQG 73", "premise: our 73");
    e.ingest_decodes_for_test(&[], 23);
    assert_eq!(calls_logged(&e), [DX], "logged by itself, once");
    assert!(e.snapshot().pending_log.is_none(), "no popup");
    assert!(e.pending_logs.is_empty());
}

/// A double-click repeated on a roger before the 73 it queued airs is still that one first 73:
/// offered once.
#[test]
fn a_roger_clicked_twice_before_our_73_airs_is_offered_once() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU RR73", -9)], 21);
    e.call_station(DX);
    e.call_station(DX);
    assert_eq!(key_once(&mut e, 22), "7P8RU KR4FQG 73", "premise: our 73");
    e.ingest_decodes_for_test(&[], 25);
    assert_eq!(e.pending_logs.len(), 1, "offered once");
    assert!(e.pending_logs[0].confirm_only);
    assert!(e.stored_log().is_empty());
}

/// A logger program replying to the same station's RR73 again after another over (our CQ),
/// while the first offer still waits in the popup: the queue does not hold a second offer for
/// that station on that band.
#[test]
fn a_repeated_logger_reply_does_not_queue_a_second_offer_for_the_same_station_and_band() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = true;
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU RR73", -9)], 21);
    e.answer_udp_reply(DX, "KR4FQG 7P8RU RR73", -9, None)
        .expect("premise: the Reply is taken");
    assert_eq!(key_once(&mut e, 22), "7P8RU KR4FQG 73", "premise: our 73");
    e.ingest_decodes_for_test(&[], 25);
    assert_eq!(e.pending_logs.len(), 1, "premise: offered");
    e.set_mode("qso-run").expect("premise: the CQ run starts");
    assert!(
        key_once(&mut e, 31).starts_with("CQ KR4FQG"),
        "premise: our CQ goes out"
    );
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU RR73", -9)], 41);
    e.answer_udp_reply(DX, "KR4FQG 7P8RU RR73", -9, None)
        .expect("premise: the Reply is taken");
    assert_eq!(key_once(&mut e, 42), "7P8RU KR4FQG 73", "premise: our 73");
    e.ingest_decodes_for_test(&[], 45);
    let calls: Vec<_> = e
        .pending_logs
        .iter()
        .map(|h| h.record.call.clone())
        .collect();
    assert_eq!(calls, [DX], "one offer for that station and band");
    assert!(e.stored_log().is_empty());
}

/// A CQ run sends RR73 to W1AW, and before the next fold the operator double-clicks a station
/// decoded the slot before. With Auto-log on the contact with W1AW logs, with its reports; with
/// the popup on it waits there. It is never lost.
#[test]
fn a_click_right_after_our_rr73_logs_or_offers_the_finished_contact() {
    for (auto_log, prompt) in [(true, false), (true, true), (false, true)] {
        let case = format!("auto-log {auto_log}, prompt {prompt}");
        let mut e = ft8();
        e.settings.auto_log = auto_log;
        e.settings.prompt_to_log = prompt;
        e.set_mode("qso-run").expect("the run starts");
        assert!(key_once(&mut e, 0).starts_with("CQ KR4FQG"), "premise: CQ");
        e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 2);
        assert_eq!(key_once(&mut e, 2), "W1AW KR4FQG -07", "premise: report");
        e.ingest_decodes_for_test(
            &[
                dec_snr("KR4FQG W1AW R-10", -7),
                dec_snr("CQ K2ABC FN20", -11),
            ],
            4,
        );
        assert_eq!(key_once(&mut e, 4), "W1AW KR4FQG RR73", "premise: RR73");
        e.call_station("K2ABC");
        let mut rows: Vec<(String, Option<String>, Option<String>)> = e
            .stored_log()
            .iter()
            .map(|r| (r.call.clone(), r.rst_sent.clone(), r.rst_rcvd.clone()))
            .collect();
        rows.extend(e.pending_logs.iter().map(|h| {
            (
                h.record.call.clone(),
                h.record.rst_sent.clone(),
                h.record.rst_rcvd.clone(),
            )
        }));
        assert_eq!(
            rows,
            [("W1AW".to_string(), Some("-07".into()), Some("-10".into()))],
            "{case}: logged or held, with its reports"
        );
        assert_eq!(
            e.stored_log().len(),
            usize::from(!prompt),
            "{case}: logged by itself only with the popup off"
        );
        e.ingest_decodes_for_test(&[], 6);
        assert_eq!(
            e.stored_log().len() + e.pending_logs.len(),
            1,
            "{case}: once"
        );
    }
}

/// With both off, a click that sets aside a finished contact while an older one is already set
/// aside for Log QSO does not drop the older one, and nothing opens by itself: both wait for Log
/// QSO, which logs each.
#[test]
fn a_click_set_aside_never_drops_an_older_set_aside_contact() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    let made_on = (e.settings.band.clone(), e.log_frequencies());
    e.qso_report_sent = Some(-3);
    e.stalled_qso = Some(e.set_aside("W9OLD".into(), None, Some(-4), made_on));
    e.qso_report_sent = None;
    work_dx_to_73(&mut e, 11);
    e.ingest_decodes_for_test(&[dec_snr("CQ W2XYZ FN31", -12)], 31);
    e.call_station("W2XYZ");
    assert!(
        e.pending_logs.is_empty(),
        "the popup does not open by itself"
    );
    assert!(e.stored_log().is_empty(), "nothing logged by itself");
    assert!(
        e.log_current_qso() && e.log_current_qso(),
        "Log QSO logs each"
    );
    let mut calls = calls_logged(&e);
    calls.sort();
    assert_eq!(calls, [DX, "W9OLD"], "neither contact is lost");
}

/// With both off, Log QSO pressed while a new station is shown logs the contact set aside
/// behind it, and names that station, so the toast can say which was logged.
#[test]
fn log_qso_names_the_set_aside_station_it_logged() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    work_dx_to_73(&mut e, 11);
    e.ingest_decodes_for_test(&[dec_snr("CQ W2XYZ FN31", -12)], 31);
    e.call_station("W2XYZ");
    let press = e.log_current_qso_press().expect("Log QSO logs it");
    assert_eq!((press.call.as_str(), press.already_waiting), (DX, false));
    assert_eq!(calls_logged(&e), [DX]);
}

/// Every contact in the log or in the popup, as (call, RST_SENT, RST_RCVD).
fn rows(e: &Engine) -> Vec<(String, Option<String>, Option<String>)> {
    let mut rows: Vec<(String, Option<String>, Option<String>)> = e
        .stored_log()
        .iter()
        .map(|r| (r.call.clone(), r.rst_sent.clone(), r.rst_rcvd.clone()))
        .collect();
    rows.extend(e.pending_logs.iter().map(|h| {
        (
            h.record.call.clone(),
            h.record.rst_sent.clone(),
            h.record.rst_rcvd.clone(),
        )
    }));
    rows
}

/// A CQ run up to our RR73 to W1AW, which goes on the air at slot 4 (reports -07 sent, -10
/// received). The next fold has not come yet.
fn run_to_our_rr73_to_w1aw(e: &mut Engine) {
    e.set_mode("qso-run").expect("the run starts");
    assert!(key_once(e, 0).starts_with("CQ KR4FQG"), "premise: CQ");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 2);
    assert_eq!(key_once(e, 2), "W1AW KR4FQG -07", "premise: report");
    e.ingest_decodes_for_test(
        &[
            dec_snr("KR4FQG W1AW R-10", -7),
            dec_snr("CQ K2ABC FN20", -11),
        ],
        4,
    );
    assert_eq!(key_once(e, 4), "W1AW KR4FQG RR73", "premise: RR73");
}

/// Two finished contacts with the same station on the same band, both set aside with Auto-log
/// and the popup off (the run gave up on 7P8RU, then 7P8RU came back and the contact completed,
/// then the operator clicked another station): both are kept. Nothing opens by itself, and Log
/// QSO logs each, with its own reports.
#[test]
fn two_contacts_with_one_station_set_aside_with_both_off_are_both_kept() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    let made_on = (e.settings.band.clone(), e.log_frequencies());
    e.qso_report_sent = Some(-3);
    e.qso_start_unix = Some(now_unix_secs() - 3600);
    e.stalled_qso = Some(e.set_aside(DX.into(), None, Some(-4), made_on));
    e.qso_report_sent = None;
    e.qso_start_unix = None;
    work_dx_to_73(&mut e, 11);
    e.ingest_decodes_for_test(&[dec_snr("CQ W2XYZ FN31", -12)], 31);
    e.call_station("W2XYZ");
    assert!(e.pending_logs.is_empty(), "nothing waits in the popup");
    assert!(e.snapshot().pending_log.is_none(), "the popup did not open");
    assert!(e.stored_log().is_empty(), "nothing logged by itself");
    assert!(e.log_current_qso(), "Log QSO logs one");
    assert!(e.log_current_qso(), "and then the other");
    assert!(!e.log_current_qso(), "and nothing more");
    let mut logged = rows(&e);
    logged.sort();
    assert_eq!(
        logged,
        [
            (DX.to_string(), Some("-03".into()), Some("-04".into())),
            (DX.to_string(), Some("-09".into()), Some("-15".into())),
        ],
        "both contacts, each with its own reports"
    );
}

/// A re-click on W1AW's R-report right after our RR73 to them is the same over answered again:
/// our RR73 goes out again as before, and the contact logs once, with its reports, under every
/// Auto-log / Prompt combination, however long the contact ran (TIME_ON 10 minutes back, so the
/// log's 300 s duplicate guard cannot be what stops a second row).
#[test]
fn a_reclick_on_their_r_report_after_our_rr73_logs_the_contact_once() {
    for (auto_log, prompt) in [(true, false), (true, true), (false, true), (false, false)] {
        let case = format!("auto-log {auto_log}, prompt {prompt}");
        let mut e = ft8();
        e.settings.auto_log = auto_log;
        e.settings.prompt_to_log = prompt;
        run_to_our_rr73_to_w1aw(&mut e);
        e.qso_start_unix = Some(now_unix_secs() - 600);
        e.call_station_ctx("W1AW", None, Some("KR4FQG W1AW R-10"), Some(-7), None)
            .expect("premise: the click is taken");
        assert_eq!(
            key_once(&mut e, 6),
            "W1AW KR4FQG RR73",
            "{case}: our RR73 goes out again"
        );
        e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW 73", -7)], 8);
        if !auto_log && !prompt {
            assert!(e.pending_logs.is_empty(), "{case}: nothing opens by itself");
            assert!(e.log_current_qso(), "{case}: Log QSO logs it");
            assert!(!e.log_current_qso(), "{case}: once");
        }
        assert_eq!(
            rows(&e),
            [("W1AW".to_string(), Some("-07".into()), Some("-10".into()))],
            "{case}: one contact, with its reports"
        );
        assert_eq!(e.stored_log().len(), usize::from(!prompt), "{case}: logged");
    }
}

/// A roger offer for 7P8RU on 20 m FT8 waiting in the popup does not suppress one on 20 m FT4:
/// that is another contact.
#[test]
fn a_roger_offer_on_ft4_is_kept_beside_a_waiting_ft8_one() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = true;
    click_their_rr73_and_send_73(&mut e, 21);
    assert_eq!(e.pending_logs.len(), 1, "premise: the FT8 offer waits");
    e.set_tier(Tier::Ft4);
    e.set_mode("qso-run").expect("premise: the CQ run starts");
    assert!(key_once(&mut e, 40).starts_with("CQ KR4FQG"), "premise: CQ");
    click_their_rr73_and_send_73(&mut e, 51);
    let offers: Vec<(String, String, bool)> = e
        .pending_logs
        .iter()
        .map(|h| (h.record.call.clone(), h.record.mode.clone(), h.confirm_only))
        .collect();
    assert_eq!(
        offers,
        [
            (DX.to_string(), "FT8".to_string(), true),
            (DX.to_string(), "FT4".to_string(), true)
        ]
    );
}

/// Call CQ, a mode change (Monitor, or FT8 to FT4), or a click on a CQ that a contest takes, in
/// the period right after our RR73: the finished contact is logged (Auto-log on) or offered (the
/// popup on), as a click on another station now does, as the FT8 contact it was; with both off it
/// is kept for Log QSO. Never lost, and never logged twice.
#[test]
fn call_cq_a_mode_change_or_a_contest_cq_click_after_our_rr73_never_loses_the_contact() {
    #[derive(Clone, Copy, Debug)]
    enum Act {
        CallCq,
        ModeChange,
        TierChange,
        ContestCq,
    }
    for act in [
        Act::CallCq,
        Act::ModeChange,
        Act::TierChange,
        Act::ContestCq,
    ] {
        for (auto_log, prompt) in [(true, false), (true, true), (false, true), (false, false)] {
            let case = format!("{act:?}, auto-log {auto_log}, prompt {prompt}");
            let mut e = ft8();
            e.settings.auto_log = auto_log;
            e.settings.prompt_to_log = prompt;
            if matches!(act, Act::ContestCq) {
                let mut s = e.settings().clone();
                s.fd_event = "txqp".into();
                s.fd_class = "2B".into();
                s.fd_section = "EMA".into();
                s.contest_qth_state = "MA".into();
                s.contest_cq_zone = 5;
                s.contest_itu_zone = 8;
                s.contest_check = "74".into();
                s.contest_category_operator = "SINGLE-OP".into();
                s.contest_category_power = "LOW".into();
                s.contest_category_assisted = "NON-ASSISTED".into();
                s.fd_active = true;
                e.apply_settings(s);
            }
            run_to_our_rr73_to_w1aw(&mut e);
            match act {
                Act::CallCq => e.set_mode("qso-run").expect("Call CQ"),
                Act::ModeChange => e.set_mode("qso-monitor").expect("the mode change"),
                Act::TierChange => e.set_tier(Tier::Ft4),
                Act::ContestCq => {
                    e.ingest_decodes_for_test(&[dec_snr("CQ W9XYZ EN37", -12)], 5);
                    e.work_station("W9XYZ", None, Some("CQ W9XYZ EN37"), Some(-12), None)
                        .expect("premise: the contest takes the click");
                    assert!(
                        matches!(e.mode, Mode::FieldDay { .. }),
                        "{case}: premise: in the contest's S&P"
                    );
                }
            }
            if !auto_log && !prompt {
                assert!(e.pending_logs.is_empty(), "{case}: nothing opens by itself");
                assert!(
                    e.stored_log().is_empty(),
                    "{case}: nothing logged by itself"
                );
                assert!(e.log_current_qso(), "{case}: Log QSO logs it");
            }
            assert_eq!(
                rows(&e),
                [("W1AW".to_string(), Some("-07".into()), Some("-10".into()))],
                "{case}: logged or held, with its reports"
            );
            e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW 73", -7)], 20);
            assert!(!e.log_current_qso(), "{case}: Log QSO has nothing more");
            assert_eq!(rows(&e).len(), 1, "{case}: once");
            assert_eq!(e.stored_log().len(), usize::from(!prompt), "{case}: logged");
            let modes: Vec<String> = e
                .stored_log()
                .iter()
                .map(|r| r.mode.clone())
                .chain(e.pending_logs.iter().map(|h| h.record.mode.clone()))
                .collect();
            assert_eq!(modes, ["FT8"], "{case}: the mode it was made in");
        }
    }
}

/// Log QSO on a roger contact whose offer for the same station, band and mode already waits in
/// the popup: nothing new is held, and the press says it is already waiting, not "Logged".
#[test]
fn log_qso_says_when_the_offer_already_waits() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    click_their_rr73_and_send_73(&mut e, 21);
    let first = e.log_current_qso_press().expect("premise: offered");
    assert!(!first.already_waiting, "the first press holds it");
    assert_eq!(e.pending_logs.len(), 1, "premise: in the popup");
    e.set_mode("qso-run").expect("premise: the CQ run starts");
    assert!(key_once(&mut e, 31).starts_with("CQ KR4FQG"), "premise: CQ");
    click_their_rr73_and_send_73(&mut e, 41);
    let press = e.log_current_qso_press().expect("the press is answered");
    assert_eq!(press.call, DX);
    assert!(press.already_waiting, "it says the offer already waits");
    assert_eq!(e.pending_logs.len(), 1, "nothing new held");
    assert!(e.stored_log().is_empty());
}

/// The no-second-offer rule is for report-less offers only: a contact that carries a report is
/// held beside a waiting offer for the same station, band and mode, never dropped.
#[test]
fn a_contact_with_reports_is_held_beside_a_waiting_offer_for_the_same_station() {
    let mut e = ft8();
    e.settings.prompt_to_log = true;
    let offer = HeldQso {
        confirm_only: true,
        ..HeldQso::new(e.qso_record(DX.into(), None, None), GridSource::LookedUp)
    };
    assert!(
        e.hold_pending_log(offer.clone()),
        "premise: the offer waits"
    );
    assert!(
        !e.hold_pending_log(offer),
        "premise: a second offer is not held"
    );
    e.qso_report_sent = Some(-9);
    let reported = HeldQso {
        confirm_only: true,
        ..HeldQso::new(
            e.qso_record(DX.into(), None, Some(-15)),
            GridSource::LookedUp,
        )
    };
    assert!(
        e.hold_pending_log(reported),
        "a contact with reports is held"
    );
    assert_eq!(e.pending_logs.len(), 2);
}

/// With both off, contacts that clicks replace one after another each wait for Log QSO, and a
/// log of one does not end the others.
#[test]
fn contacts_replaced_one_after_another_with_both_off_each_wait_for_log_qso() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    work_dx_to_73(&mut e, 11);
    e.ingest_decodes_for_test(&[dec_snr("CQ W2XYZ FN31", -12)], 31);
    e.call_station("W2XYZ");
    key_once(&mut e, 32);
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W2XYZ -11", -12)], 35);
    key_once(&mut e, 36);
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W2XYZ RR73", -12)], 39);
    key_once(&mut e, 40);
    e.ingest_decodes_for_test(&[dec_snr("CQ K2ABC FN20", -11)], 43);
    e.call_station("K2ABC");
    assert!(e.pending_logs.is_empty(), "nothing opens by itself");
    assert!(e.log_current_qso(), "Log QSO logs the first");
    assert!(e.log_current_qso(), "and the second");
    assert!(!e.log_current_qso(), "and nothing more");
    assert_eq!(calls_logged(&e), [DX, "W2XYZ"]);
}

/// With Auto-log and the popup both off, a CQ run waiting for Log QSO after a contact keeps
/// waiting through a switch from FT8 to FT4, as before the switch did anything for the log: no
/// CQ is planned or keyed, whether the switch moves the dial (Nexus driving the radio) or not
/// (Companion mode), until the operator presses Log QSO. The press logs the contact as the FT8
/// contact it was, on its FT8 frequency, and the run then calls CQ again.
#[test]
fn a_waiting_cq_run_keeps_waiting_through_an_ft8_to_ft4_switch_with_both_off() {
    for source in [SourceKind::Native, SourceKind::Companion] {
        let case = format!("{source:?}");
        let mut e = ft8();
        e.settings.auto_log = false;
        e.settings.prompt_to_log = false;
        e.source_kind = source;
        run_to_our_rr73_to_w1aw(&mut e);
        e.ingest_decodes_for_test(&[], 6);
        assert!(
            !tx_now(&e).starts_with("CQ"),
            "{case}: premise: the run waits"
        );
        let made_on = e.log_frequencies();
        e.set_tier(Tier::Ft4);
        for slot in 10..22 {
            e.ingest_decodes_for_test(&[], slot);
            let planned = tx_now(&e);
            assert!(
                !planned.starts_with("CQ"),
                "{case}: slot {slot}: CQ planned: {planned:?}"
            );
            if !e.poll_tx(slot).is_empty() {
                assert!(!planned.starts_with("CQ"), "{case}: slot {slot}: CQ keyed");
            }
        }
        assert!(
            e.stored_log().is_empty() && e.pending_logs.is_empty(),
            "{case}: nothing by itself"
        );
        assert!(e.log_current_qso(), "{case}: Log QSO logs it");
        let rec = e.stored_log()[0].clone();
        assert_eq!(
            (rec.call.as_str(), rec.mode.as_str(), rec.band.as_str()),
            ("W1AW", "FT8", "20m"),
            "{case}: as the FT8 contact it was"
        );
        assert_eq!(
            (rec.freq_mhz, rec.freq_rx_mhz),
            made_on,
            "{case}: on its FT8 frequency"
        );
        assert_eq!(
            (rec.rst_sent.as_deref(), rec.rst_rcvd.as_deref()),
            (Some("-07"), Some("-10")),
            "{case}: with its reports"
        );
        assert!(!e.log_current_qso(), "{case}: once");
        e.ingest_decodes_for_test(&[], 22);
        assert!(
            tx_now(&e).starts_with("CQ KR4FQG"),
            "{case}: then the run calls CQ: {}",
            tx_now(&e)
        );
    }
}

/// With Auto-log on, Log QSO pressed on a contact that is already logged never reaches an
/// unfinished contact a band change set aside: that press is for the contact just made, and says
/// there is nothing more to log. The set-aside contact still waits for a press with no contact
/// shown. (A run's give-up needs no case: any log ends it, as before.)
#[test]
fn log_qso_on_a_logged_contact_never_reaches_an_unfinished_set_aside_contact() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    e.qso_report_sent = Some(-3);
    e.band_change_qso = Some(e.set_aside(
        "W2XYZ".into(),
        None,
        Some(-4),
        (String::from("40m"), (7.075_5, None)),
    ));
    e.qso_report_sent = None;
    work_dx_to_73(&mut e, 11);
    assert_eq!(calls_logged(&e), [DX], "premise: the contact logged itself");
    assert!(!e.log_current_qso(), "nothing more to log");
    assert_eq!(
        calls_logged(&e),
        [DX],
        "the unfinished contact is not logged"
    );
    e.set_mode("qso-monitor")
        .expect("premise: no contact shown");
    assert!(
        e.log_current_qso(),
        "a press with no contact shown still takes it"
    );
    assert_eq!(calls_logged(&e), [DX, "W2XYZ"]);
}

/// The contacts waiting for Log QSO with both off are counted in the snapshot, and the count
/// follows each press.
#[test]
fn the_contacts_waiting_for_log_qso_are_counted() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    assert_eq!(e.snapshot().log_qso_waiting, 0);
    work_dx_to_73(&mut e, 11);
    assert_eq!(
        e.snapshot().log_qso_waiting,
        0,
        "the contact shown is not counted"
    );
    e.set_mode("qso-run").expect("Call CQ");
    assert_eq!(e.snapshot().log_qso_waiting, 1, "set aside by Call CQ");
    assert!(e.log_current_qso(), "the press takes it");
    assert_eq!(e.snapshot().log_qso_waiting, 0);
}

/// Contacts waiting for Log QSO are kept across a restart, in the popup's journal, and come back
/// waiting for Log QSO: the popup does not open by itself, and each press logs one.
#[test]
fn contacts_waiting_for_log_qso_are_kept_across_a_restart() {
    let path = std::env::temp_dir().join(format!(
        "nexus-log-qso-waiting-{}-{}.json",
        std::process::id(),
        line!()
    ));
    let _ = std::fs::remove_file(&path);
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    e.set_pending_qso_path(path.clone());
    work_dx_to_73(&mut e, 11);
    e.set_mode("qso-run").expect("Call CQ");
    assert_eq!(e.snapshot().log_qso_waiting, 1, "premise: one waits");
    assert!(path.exists(), "journaled");
    let mut r = ft8();
    r.settings.auto_log = false;
    r.settings.prompt_to_log = false;
    r.set_pending_qso_path(path.clone());
    r.restore_pending_qso_journal(now_unix_secs() as i64);
    let snap = r.snapshot();
    assert_eq!(snap.log_qso_waiting, 1, "it waits again after the restart");
    assert!(
        snap.pending_log.is_none(),
        "the popup does not open by itself"
    );
    assert!(r.log_current_qso(), "Log QSO logs it");
    assert_eq!(
        rows(&r),
        [(DX.to_string(), Some("-09".into()), Some("-15".into()))]
    );
    assert!(!path.exists(), "and the journal is cleared");
    let _ = std::fs::remove_file(&path);
}

/// A contact held in the popup and one waiting for Log QSO share the journal: answering the popup
/// keeps the waiting one in it.
#[test]
fn answering_the_popup_keeps_a_contact_waiting_for_log_qso_in_the_journal() {
    let path = std::env::temp_dir().join(format!(
        "nexus-log-qso-waiting-{}-{}.json",
        std::process::id(),
        line!()
    ));
    let _ = std::fs::remove_file(&path);
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    e.set_pending_qso_path(path.clone());
    work_dx_to_73(&mut e, 11);
    e.set_mode("qso-run").expect("Call CQ");
    assert!(
        e.hold_pending_log(held(&e, "K2ABC")),
        "premise: the popup holds one"
    );
    let key = e.pending_qso_log_key().unwrap_or_default();
    assert!(e.discard_pending_log(&key));
    let mut r = ft8();
    r.set_pending_qso_path(path.clone());
    r.restore_pending_qso_journal(now_unix_secs() as i64);
    assert_eq!(
        r.snapshot().log_qso_waiting,
        1,
        "the waiting contact is still journaled"
    );
    let _ = std::fs::remove_file(&path);
}

/// Log QSO in Chat (or a contest) has no contact of its own: it reaches a finished contact waiting
/// for Log QSO, never an unfinished one a band change set aside, which waits for a press outside.
#[test]
fn log_qso_in_chat_reaches_only_a_contact_waiting_for_log_qso() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    e.qso_report_sent = Some(-3);
    e.band_change_qso = Some(e.set_aside(
        "W2XYZ".into(),
        None,
        Some(-4),
        (String::from("40m"), (7.075_5, None)),
    ));
    e.qso_report_sent = None;
    e.set_mode("chat").expect("premise: Chat");
    assert!(
        !matches!(e.mode, Mode::Qso { .. }),
        "premise: no FT contact"
    );
    assert!(
        !e.log_current_qso(),
        "the unfinished contact is not logged here"
    );
    assert!(e.stored_log().is_empty());
    e.set_mode("qso-monitor").expect("premise: back to FT");
    assert!(e.log_current_qso(), "a press there takes it");
    assert_eq!(calls_logged(&e), ["W2XYZ"]);
}

/// With both off, a FINISHED contact a band change ends waits for Log QSO with the others:
/// counted beside Log, journaled across a restart, and logged by a press with no contact shown,
/// on the band and frequency it was made on.
#[test]
fn a_finished_contact_a_band_change_ends_with_both_off_is_counted_kept_and_logged() {
    let path = std::env::temp_dir().join(format!(
        "nexus-log-qso-waiting-{}-{}.json",
        std::process::id(),
        line!()
    ));
    let _ = std::fs::remove_file(&path);
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    e.set_pending_qso_path(path.clone());
    let made_on = e.log_frequencies();
    work_dx_to_73(&mut e, 11);
    e.set_frequency(7.074, "40m", "USB");
    assert_eq!(e.snapshot().log_qso_waiting, 1, "counted beside Log");
    assert!(path.exists(), "journaled");
    assert!(e.stored_log().is_empty() && e.pending_logs.is_empty());
    let mut r = ft8();
    r.settings.auto_log = false;
    r.settings.prompt_to_log = false;
    r.set_pending_qso_path(path.clone());
    r.set_frequency(7.074, "40m", "USB");
    r.restore_pending_qso_journal(now_unix_secs() as i64);
    assert_eq!(r.snapshot().log_qso_waiting, 1, "kept across a restart");
    assert!(
        r.snapshot().pending_log.is_none(),
        "nothing opens by itself"
    );
    assert!(r.log_current_qso(), "a press with no contact shown logs it");
    let rec = r.stored_log()[0].clone();
    assert_eq!((rec.call.as_str(), rec.band.as_str()), (DX, "20m"));
    assert_eq!(
        (rec.freq_mhz, rec.freq_rx_mhz),
        made_on,
        "on its own frequency"
    );
    assert_eq!(
        (rec.rst_sent.as_deref(), rec.rst_rcvd.as_deref()),
        (Some("-09"), Some("-15"))
    );
    assert_eq!(r.snapshot().log_qso_waiting, 0);
    assert!(!r.log_current_qso(), "once");
    let _ = std::fs::remove_file(&path);
}

/// …and our 73 to a roger the operator clicked, set aside by a band change with both off, is
/// counted too, and still only offered.
#[test]
fn a_roger_contact_a_band_change_ends_with_both_off_is_counted_and_offered() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    click_their_rr73_send_73_and_change_band(&mut e);
    assert_eq!(e.snapshot().log_qso_waiting, 1, "counted");
    assert!(e.log_current_qso(), "Log QSO takes it");
    assert!(e.stored_log().is_empty(), "offered, not logged");
    assert!(e.pending_logs[0].confirm_only, "offer-only");
    assert_eq!(e.snapshot().log_qso_waiting, 0);
}

/// A press with no contact shown takes a counted contact before an unfinished one a band change
/// set aside, which is not counted: the count it invites names what it logs. The run's give-up
/// still comes first (any log ends it, so it would be lost behind another), and none is lost.
#[test]
fn a_press_with_no_contact_shown_takes_a_counted_contact_before_a_band_change_one() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    work_dx_to_73(&mut e, 11);
    e.set_mode("qso-run").expect("Call CQ");
    assert_eq!(e.snapshot().log_qso_waiting, 1, "premise: one counted");
    e.qso_report_sent = Some(-3);
    e.band_change_qso = Some(e.set_aside(
        "W2XYZ".into(),
        None,
        Some(-4),
        (String::from("40m"), (7.075_5, None)),
    ));
    e.qso_report_sent = None;
    e.set_mode("qso-monitor")
        .expect("premise: no contact shown");
    assert!(e.log_current_qso());
    assert_eq!(calls_logged(&e), [DX], "the counted contact first");
    assert_eq!(e.snapshot().log_qso_waiting, 0);
    assert!(e.log_current_qso());
    assert_eq!(calls_logged(&e), [DX, "W2XYZ"], "then the band change's");
    // A give-up and a counted contact: the give-up first, and then the counted one, not lost.
    let made_on = (e.settings.band.clone(), e.log_frequencies());
    work_to_73(&mut e, "K1JT", 41);
    e.set_mode("qso-run").expect("Call CQ");
    e.qso_report_sent = Some(-3);
    e.stalled_qso = Some(e.set_aside("K2ABC".into(), None, Some(-5), made_on));
    e.qso_report_sent = None;
    e.set_mode("qso-monitor")
        .expect("premise: no contact shown");
    assert!(e.log_current_qso() && e.log_current_qso());
    assert_eq!(
        calls_logged(&e),
        [DX, "W2XYZ", "K2ABC", "K1JT"],
        "neither lost"
    );
    assert!(!e.log_current_qso());
}

/// With both off, a tier change that ends the contact (away from a tier whose overs run long,
/// with TX on) sets the finished contact aside for Log QSO before the station is replaced: TX is
/// halted there anyway, and the contact is logged as the one it was.
#[test]
fn a_tier_change_that_ends_a_finished_contact_with_both_off_keeps_it_for_log_qso() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    e.set_tier(Tier::Q65);
    let mode = e.adif_mode_for_tier().to_string();
    work_dx_to_73(&mut e, 11);
    assert!(e.tx_enabled, "premise: TX on");
    e.set_tier(Tier::Ft8);
    assert!(!e.tx_enabled, "premise: TX halted by the switch, as before");
    assert_eq!(e.snapshot().log_qso_waiting, 1, "kept, counted");
    assert!(e.log_current_qso(), "Log QSO logs it");
    let rec = e.stored_log()[0].clone();
    assert_eq!((rec.call.as_str(), rec.mode.as_str()), (DX, mode.as_str()));
    assert_eq!(
        (rec.rst_sent.as_deref(), rec.rst_rcvd.as_deref()),
        (Some("-09"), Some("-15"))
    );
}

/// …and on the band and frequency it was made on when the rig is read back on another band
/// (a listen to WWV between): the dial has already left them when the band change is seen.
#[test]
fn a_finished_contact_kept_at_a_band_change_at_the_rig_logs_on_the_band_left() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    e.observe_rig_freq(14_074_000);
    let made_on = e.log_frequencies();
    work_dx_to_73(&mut e, 11);
    e.observe_rig_freq(10_000_000);
    e.observe_rig_freq(7_074_000);
    assert_eq!(e.snapshot().log_qso_waiting, 1, "counted");
    assert!(e.log_current_qso());
    let rec = e.stored_log()[0].clone();
    assert_eq!((rec.call.as_str(), rec.band.as_str()), (DX, "20m"));
    assert_eq!((rec.freq_mhz, rec.freq_rx_mhz), made_on);
}
