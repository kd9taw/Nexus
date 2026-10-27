//! Clicking a station always starts the contact fresh, as WSJT-X's double-click does, and the QSO
//! start time (TIME_ON) is set and reset where WSJT-X sets and resets `m_dateTimeQSOOn`.
//!
//! Field report (KR4FQG, 2026-10-26): a partial S&P QSO with 7P8 stalled; hours later a click on
//! 7P8 sent the hours-old roger with the hours-old report, 7P8 answered RR73, and the contact
//! logged under the start time of the first attempt. The cause was the re-click rule in
//! `call_station_ctx`: a click naming no message on the station still held by the stalled QSO
//! kept that QSO, its queued roger and its start time.
//!
//! WSJT-X (master `0f7d5928`): `handleDecodeSelection` resets the start time
//! (`set_dateTimeQSO(-1)`, mainwindow.cpp:9271) and `processMessage` picks the Tx message from the
//! clicked message alone (DecodedMessageReaction.cpp `planProcessMessage`): a CQ or a message to
//! someone else selects Tx1 (:696), a message to me maps by its type, and the report is re-read
//! from that decode (`SetReport`, :400). The start time is reset when Tx1 or a CQ is selected
//! (:9129, :9213) and after logging (:10683), and set, only if not already set, when Tx2 or Tx3 is
//! selected (:9168, :9178) to `now - (n-2)*T/R - (seconds mod T/R)` (`set_dateTimeQSO`,
//! :9087-9107). Pressing Enable Tx, or a Tx2/Tx3 button on a QSO whose start time is already set,
//! changes neither the message nor the start time.

use super::tests::dec_snr;
use super::*;
use crate::test_util::StoredLog;

const ME: &str = "KR4FQG";
const DX: &str = "7P8RU";
const HOURS: u64 = 3 * 3600;
const OLD_ROGER: &str = "7P8RU KR4FQG R-12";
const TX1: &str = "7P8RU KR4FQG EM85";

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

/// The first, unfinished attempt: double-click 7P8RU's CQ, they answer once with a report, we
/// roger it (R-12) three times, they are never heard again, the operator turns TX off. TIME_ON is
/// then back-dated by three hours. With `roll`, hours of other traffic roll the 240-row decode
/// history so nothing of 7P8RU is left in it; without, their old report to us is still there.
/// Either way 7P8RU then calls CQ again on this band, heard at -08.
fn stale_attempt(roll: bool) -> (Engine, u64) {
    let mut e = Engine::new(ME, "EM85", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;

    e.ingest_decodes_for_test(&[dec_snr("CQ 7P8RU KG30", -12)], 11);
    e.work_station(DX, None, Some("CQ 7P8RU KG30"), Some(-12), Some(1500.0))
        .expect("first attempt starts");
    assert_eq!(tx_now(&e), TX1, "premise: a fresh call opens with Tx1");
    key_once(&mut e, 12);
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU -15", -12)], 14);
    assert_eq!(tx_now(&e), OLD_ROGER, "premise: their report, our roger");
    let mut s = 15;
    for _ in 0..3 {
        key_once(&mut e, s);
        e.ingest_decodes_for_test(&[dec_snr("CQ N0ABC EM48", -3)], s + 2);
        s += 4;
    }
    e.set_tx_enabled(false); // the operator gives up
    let started = e
        .qso_start_unix
        .expect("premise: the first attempt has a start time");
    let backdated = started - HOURS;
    e.qso_start_unix = Some(backdated);

    if roll {
        for k in 0..150u64 {
            e.ingest_decodes_for_test(
                &[dec_snr("CQ N0ABC EM48", -3), dec_snr("CQ W1AW FN31", -9)],
                100 + 2 * k,
            );
        }
        assert!(
            e.latest_decode_slot_from(DX).is_none(),
            "premise: ring rolled"
        );
    } else {
        assert!(
            e.latest_reply_from(DX, ME).is_some(),
            "premise: their old report to us is still in the history"
        );
    }
    e.ingest_decodes_for_test(&[dec_snr("CQ 7P8RU KG30", -8)], 1001);
    (e, backdated)
}

/// Run the contact the click started to its log entry: our Tx1, their report (`their`) heard at
/// `heard`, our roger, their RR73, our 73. Returns every over that went out.
fn complete(e: &mut Engine, from: u64, their: i32, heard: i32) -> Vec<String> {
    let mut sent = vec![key_once(e, from)];
    e.ingest_decodes_for_test(
        &[dec_snr(&format!("KR4FQG 7P8RU {their:+03}"), heard)],
        from + 3,
    );
    sent.push(key_once(e, from + 4));
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU RR73", heard)], from + 7);
    sent.push(key_once(e, from + 8));
    e.ingest_decodes_for_test(&[], from + 11);
    sent
}

/// What a click started must be a fresh contact: Tx1 now, no start time until our roger is
/// selected, then a start time of NOW, and a log entry with the reports of THIS exchange.
fn assert_fresh_contact(mut e: Engine, backdated: u64, band: &str, route: &str) {
    assert_eq!(
        tx_now(&e),
        TX1,
        "{route}: the click must open with Tx1, never the old roger"
    );
    assert_eq!(
        e.qso_start_unix, None,
        "{route}: the click resets the start time and Tx1 keeps it reset (mainwindow.cpp:9271, :9129)"
    );
    let sent = complete(&mut e, 1200, -10, -6);
    assert!(
        !sent.iter().any(|t| t == OLD_ROGER),
        "{route}: the hours-old roger went out: {sent:?}"
    );
    assert_eq!(
        sent[1], "7P8RU KR4FQG R-06",
        "{route}: the report heard NOW"
    );
    let log = e.stored_log();
    assert_eq!(log.len(), 1, "{route}: the contact logged");
    let rec = &log[0];
    assert!(
        rec.when_unix > backdated + HOURS - 120,
        "{route}: TIME_ON is this exchange's, not the stalled attempt's: {} vs {backdated}",
        rec.when_unix
    );
    assert_eq!(rec.rst_sent.as_deref(), Some("-06"), "{route}: RST_SENT");
    assert_eq!(rec.rst_rcvd.as_deref(), Some("-10"), "{route}: RST_RCVD");
    assert_eq!(rec.band, band, "{route}: band");
}

/// A — the reported path: a click naming no message (Call Roster double-click, station card Work)
/// hours later.
#[test]
fn a_roster_click_hours_later_starts_fresh() {
    let (mut e, backdated) = stale_attempt(true);
    e.call_station(DX);
    assert_fresh_contact(e, backdated, "20m", "roster click");
}

/// A, the Needed board and alerts: the same click carrying the station's grid.
#[test]
fn a_needed_board_click_hours_later_starts_fresh() {
    let (mut e, backdated) = stale_attempt(true);
    e.call_station_with_grid(DX, Some("KG30"));
    assert_fresh_contact(e, backdated, "20m", "Needed click");
}

/// A, the Remote page and the cockpit's Work with an offset: `work_station` with no message.
#[test]
fn a_work_click_without_a_message_hours_later_starts_fresh() {
    let (mut e, backdated) = stale_attempt(true);
    e.work_station(DX, Some("KG30"), None, None, Some(1500.0))
        .expect("the click is taken");
    assert_fresh_contact(e, backdated, "20m", "work click");
}

/// A, with the old report to us STILL in the decode history: the station's latest transmission is
/// its new CQ, and a WSJT-X double-click on that CQ selects Tx1. Resuming from the older report
/// would send a roger for an exchange the station has since walked away from.
#[test]
fn a_click_answers_the_stations_latest_transmission_not_an_older_reply() {
    let (mut e, backdated) = stale_attempt(false);
    e.call_station(DX);
    assert_fresh_contact(e, backdated, "20m", "roster click, history not rolled");
}

/// The positive control for the rule above: when the station's latest transmission IS its report
/// to us, a click resumes from it as a double-click on it would (Tx3), with the report re-read
/// from that decode, and the start time restarts with the selection of Tx3.
#[test]
fn a_click_mid_qso_resumes_from_the_stations_latest_message_to_us() {
    let mut e = Engine::new(ME, "EM85", 0);
    e.set_tier(Tier::Ft8);
    e.ingest_decodes_for_test(&[dec_snr("CQ 7P8RU KG30", -12)], 11);
    e.call_station(DX);
    assert_eq!(tx_now(&e), TX1);
    key_once(&mut e, 12);
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU -15", -9)], 14);
    assert_eq!(tx_now(&e), "7P8RU KR4FQG R-09");
    e.qso_start_unix = Some(1); // mark the start time so a reset and a new set are both visible
    let before = now_unix_secs();
    e.call_station(DX);
    let after = now_unix_secs();
    assert_eq!(tx_now(&e), "7P8RU KR4FQG R-09", "Tx3, from their report");
    let on = e.qso_start_unix.expect("Tx3 selected sets the start time");
    assert!(
        (wsjtx_qso_on(before, 3, 15.0)..=wsjtx_qso_on(after, 3, 15.0)).contains(&on),
        "reset by the click, set by Tx3's selection: {on}"
    );
}

/// A logger's UDP Reply always names the message (WSJT-X `replyToCQ` is `processMessage`).
#[test]
fn a_udp_reply_hours_later_starts_fresh() {
    let (mut e, backdated) = stale_attempt(true);
    e.set_tx_enabled(true); // a UDP Reply is refused behind the operator's TX Off
    e.answer_udp_reply(DX, "CQ 7P8RU KG30", -8, Some(1500.0))
        .expect("the reply is taken");
    assert_fresh_contact(e, backdated, "20m", "UDP Reply");
}

/// B — control: a double-click on the CQ line.
#[test]
fn b_a_double_click_on_the_cq_line_starts_fresh() {
    let (mut e, backdated) = stale_attempt(true);
    e.work_station(DX, None, Some("CQ 7P8RU KG30"), Some(-8), Some(1500.0))
        .expect("the double-click is taken");
    assert_fresh_contact(e, backdated, "20m", "double-click");
}

/// C — a Tx2 button click: WSJT-X selects Tx2 and `set_dateTimeQSO(2)` returns at once because
/// the start time is already set (:9099). The message changes as clicked; the start time stays.
#[test]
fn c_a_tx2_button_keeps_the_start_time_wsjtx_already_has() {
    let (mut e, backdated) = stale_attempt(true);
    e.override_next_tx(DX, None, "7P8RU KR4FQG -08");
    assert_eq!(tx_now(&e), "7P8RU KR4FQG -08");
    assert_eq!(e.qso_start_unix, Some(backdated));
}

/// C — a Tx1 button click resets the start time (`on_txrb1_toggled`, :9129).
#[test]
fn c_a_tx1_button_resets_the_start_time() {
    let (mut e, _) = stale_attempt(true);
    e.override_next_tx(DX, None, TX1);
    assert_eq!(tx_now(&e), TX1);
    assert_eq!(e.qso_start_unix, None);
}

/// D — Enable Tx alone resumes what is queued, as WSJT-X's Enable Tx does: nothing changes.
#[test]
fn d_enable_tx_alone_resumes_unchanged() {
    let (mut e, backdated) = stale_attempt(true);
    e.set_tx_enabled(true);
    assert_eq!(key_once(&mut e, 1002), OLD_ROGER);
    assert_eq!(e.qso_start_unix, Some(backdated));
}

/// E — after a band change, once the station is heard on the new band: a roster click opens a new
/// contact there with Tx1 and a new start time, and the old band's roger never goes out.
#[test]
fn e_a_roster_click_on_the_new_band_starts_fresh_there() {
    let (mut e, backdated) = stale_attempt(false);
    e.set_frequency(21.074, "15m", "USB");
    e.ingest_decodes_for_test(&[dec_snr("CQ 7P8RU KG30", -6)], 1101);
    e.call_station(DX);
    assert_fresh_contact(e, backdated, "15m", "roster click on 15m");
}

/// F — control: a QSO already finished, and claimed by the auto-log as its fold does. (One
/// finished and not yet claimed is logged by the click itself, with Auto-log on:
/// `ft_log_offer_tests::a_click_right_after_our_rr73_logs_or_offers_the_finished_contact`.)
#[test]
fn f_a_finished_qso_reclicked_starts_fresh() {
    let (mut e, backdated) = stale_attempt(true);
    if let Mode::Qso { station, .. } = &mut e.mode {
        station.state = tempo_core::qso::State::Done;
        station.pending = None;
    }
    e.qso_logged = true;
    e.call_station(DX);
    assert_fresh_contact(e, backdated, "20m", "re-click after Done");
}

/// Answering a CQ-run caller's grid selects Tx2: the start time is the current period's start.
#[test]
fn a_cq_runs_first_answer_sets_the_start_time_at_tx2() {
    let mut e = Engine::new(ME, "EM85", 0);
    e.set_tier(Tier::Ft8);
    e.set_mode("qso-run").expect("CQ");
    assert_eq!(e.qso_start_unix, None, "a CQ keeps it reset");
    let before = now_unix_secs();
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 3);
    let after = now_unix_secs();
    assert_eq!(tx_now(&e), "W1AW KR4FQG -07", "premise: Tx2");
    let on = e.qso_start_unix.expect("Tx2 selected sets it");
    assert!((wsjtx_qso_on(before, 2, 15.0)..=wsjtx_qso_on(after, 2, 15.0)).contains(&on));
}

/// `set_dateTimeQSO(n)`, mainwindow.cpp:9087-9107, by value.
#[test]
fn the_start_time_is_wsjtx_set_datetime_qso() {
    let minute = 28_000_000 * 60;
    // FT8, 37 s into the minute: the period began 7 s ago.
    assert_eq!(wsjtx_qso_on(minute + 37, 2, 15.0), minute + 30);
    assert_eq!(wsjtx_qso_on(minute + 37, 3, 15.0), minute + 15);
    // FT4: int(7.5) = 7 per period back, fmod(37, 7.5) = 7.
    assert_eq!(wsjtx_qso_on(minute + 37, 2, 7.5), minute + 30);
    assert_eq!(wsjtx_qso_on(minute + 37, 3, 7.5), minute + 23);
}

// --- Four more places a click and the DX call follow WSJT-X ---------------------------------------
//
// (1) a click on a station whose latest message to us is its RR73 or RRR sends our 73;
// (2) with Skip Tx1 on, a click on a CQ opens with the report;
// (3) clearing the DX call, or changing it, ends the contact and resets the start time;
// (4) a contact whose start time Tx2/Tx3 never set logs WSJT-X's estimate, not the time of logging.

/// A fresh FT8 engine for `ME`.
fn ft8() -> Engine {
    let mut e = Engine::new(ME, "EM85", 0);
    e.set_tier(Tier::Ft8);
    e
}

/// (1) The station's latest message to us is its RR73 (or RRR): a click sends our 73, as a WSJT-X
/// double-click on that line selects Tx5 (`rogerAck`, DecodedMessageReaction.cpp:590-598), and as
/// a double-click on it does here.
#[test]
fn a_click_after_their_rr73_or_rrr_sends_73() {
    for theirs in ["KR4FQG 7P8RU RR73", "KR4FQG 7P8RU RRR"] {
        let mut e = ft8();
        e.ingest_decodes_for_test(&[dec_snr(theirs, -9)], 21);
        e.call_station(DX);
        assert_eq!(tx_now(&e), "7P8RU KR4FQG 73", "a click after {theirs:?}");
        assert_eq!(
            key_once(&mut e, 22),
            "7P8RU KR4FQG 73",
            "{theirs:?}: it goes out"
        );

        let mut d = ft8();
        d.ingest_decodes_for_test(&[dec_snr(theirs, -9)], 21);
        d.work_station(DX, None, Some(theirs), Some(-9), None)
            .expect("the double-click is taken");
        assert_eq!(
            tx_now(&d),
            "7P8RU KR4FQG 73",
            "{theirs:?}: the same as a double-click"
        );
    }
}

/// (1) CONTROL: a 73 to us is a station that has signed off with us; a click calls it again (Tx1).
#[test]
fn a_click_after_their_73_calls_them_again() {
    let mut e = ft8();
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU 73", -9)], 21);
    e.call_station(DX);
    assert_eq!(tx_now(&e), TX1);
}

/// (2) Skip Tx1 on: a roster click on a CQ opens with the report heard on that CQ (Tx2), as a WSJT-X
/// double-click on the CQ with Tx1 disabled does (DecodedMessageReaction.cpp:696-702, `SetReport`
/// :400), and Tx2's selection sets the start time (:9168).
#[test]
fn skip_tx1_a_roster_click_on_a_cq_opens_with_the_report() {
    let mut e = ft8();
    e.set_skip_tx1(true);
    e.ingest_decodes_for_test(&[dec_snr("CQ 7P8RU KG30", -8)], 11);
    let before = now_unix_secs();
    e.call_station(DX);
    let after = now_unix_secs();
    assert_eq!(tx_now(&e), "7P8RU KR4FQG -08");
    let on = e.qso_start_unix.expect("Tx2 selected sets the start time");
    assert!((wsjtx_qso_on(before, 2, 15.0)..=wsjtx_qso_on(after, 2, 15.0)).contains(&on));
    assert_eq!(
        e.qso_report_sent,
        Some(-8),
        "RST_SENT is the report going out"
    );
}

/// A live search-and-pounce contact with TX on: our Tx1, their report, our roger queued.
fn live_contact() -> Engine {
    let mut e = ft8();
    e.ingest_decodes_for_test(&[dec_snr("CQ 7P8RU KG30", -12)], 11);
    e.call_station(DX);
    key_once(&mut e, 12);
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU -15", -9)], 14);
    assert_eq!(
        tx_now(&e),
        "7P8RU KR4FQG R-09",
        "premise: our roger is queued"
    );
    assert!(e.tx_enabled, "premise: TX on");
    assert!(
        e.qso_start_unix.is_some(),
        "premise: Tx3 set the start time"
    );
    e
}

/// Every over keyed in `slots`.
fn keyed(e: &mut Engine, slots: std::ops::Range<u64>) -> Vec<String> {
    let mut sent = Vec::new();
    for slot in slots {
        let text = tx_now(e);
        if !e.poll_tx(slot).is_empty() {
            sent.push(text);
        }
    }
    sent
}

/// (3) Clear DX (F4, the Clear button) mid-contact ends it, as WSJT-X's `clearDX` does
/// (mainwindow.cpp:10075-10104): the start time is reset, Enable Tx goes off, and the contact is
/// gone. Nothing is sent to the station afterwards: not the queued roger, not with TX on again,
/// and not an answer to its next report.
#[test]
fn clearing_the_dx_call_ends_the_contact_and_nothing_more_goes_to_the_station() {
    let mut e = live_contact();
    e.clear_dx_call();
    assert_eq!(e.qso_start_unix, None, "start time reset (:10077)");
    assert!(!e.tx_enabled, "Enable Tx off (:10079)");
    assert_eq!(e.qso_dxcall(), None, "the contact is over");
    assert_eq!(keyed(&mut e, 15..25), Vec::<String>::new(), "TX off");
    e.set_tx_enabled(true);
    assert_eq!(keyed(&mut e, 25..35), Vec::<String>::new(), "TX on again");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU -15", -9)], 35);
    assert_eq!(
        keyed(&mut e, 36..40),
        Vec::<String>::new(),
        "their report again is not answered"
    );
    assert!(e.stored_log().is_empty(), "and nothing logged");
}

/// (3) A call typed into the DX Call box after the clear is worked as a click works it: a fresh
/// contact, Tx1, the start time set by our roger, the log entry this exchange's.
#[test]
fn a_typed_call_after_the_clear_starts_fresh_like_a_click() {
    let (mut e, backdated) = stale_attempt(false);
    e.settings.double_click_sets_tx = true;
    e.clear_dx_call();
    e.override_next_tx(DX, None, TX1);
    assert_fresh_contact(e, backdated, "20m", "typed call after Clear DX");
}

/// (3) In a CQ run, clearing the caller being worked ends that contact and goes back to the CQ,
/// behind TX off, as WSJT-X selects Tx6 with Enable Tx off (:10079, :10098-10103). The report
/// queued for the caller never goes out; Enable Tx calls CQ.
#[test]
fn clearing_a_cq_runs_caller_goes_back_to_cq_behind_tx_off() {
    let mut e = ft8();
    e.set_mode("qso-run").expect("CQ");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 3);
    assert_eq!(tx_now(&e), "W1AW KR4FQG -07", "premise: our report queued");
    e.clear_dx_call();
    assert!(!e.tx_enabled);
    assert_eq!(e.qso_start_unix, None);
    assert_eq!(keyed(&mut e, 4..10), Vec::<String>::new());
    e.set_tx_enabled(true);
    assert_eq!(key_once(&mut e, 10), "CQ KR4FQG EM85");
}

/// (3) The caller cleared in a CQ run usually calls again, since it never got our report. Until the
/// operator arms TX again the run does not answer it: no report is queued to it, and Enable Tx
/// sends the CQ. Its R-report heard while TX is off earns no RR73 and logs nothing.
#[test]
fn a_cleared_caller_calling_again_is_not_answered_before_tx_is_armed() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    e.set_mode("qso-run").expect("CQ");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 3);
    assert_eq!(tx_now(&e), "W1AW KR4FQG -07", "premise: our report queued");
    e.clear_dx_call();
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 5);
    assert_eq!(
        e.qso_dxcall(),
        None,
        "the run did not lock onto the cleared caller"
    );
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW R-10", -7)], 7);
    assert_eq!(e.qso_dxcall(), None);
    e.set_tx_enabled(true);
    let sent = keyed(&mut e, 8..10);
    assert_eq!(
        sent,
        vec!["CQ KR4FQG EM85".to_string()],
        "Enable Tx calls CQ"
    );
    assert!(
        e.stored_log().is_empty(),
        "nothing logged to the cleared caller"
    );
}

/// (3) CONTROL: another caller heard after the clear, TX still off, is answered as before.
#[test]
fn another_caller_after_the_clear_is_still_answered() {
    let mut e = ft8();
    e.set_mode("qso-run").expect("CQ");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 3);
    e.clear_dx_call();
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG K9XYZ EN52", -11)], 5);
    assert_eq!(e.qso_dxcall(), Some("K9XYZ"));
    assert_eq!(tx_now(&e), "K9XYZ KR4FQG -11");
}

/// (3) Arming TX again ends the hold: the cleared caller answering the next CQ is worked, and so
/// is a deliberate click on it right after the clear.
#[test]
fn the_cleared_caller_is_worked_again_once_the_operator_chooses_it() {
    let mut e = ft8();
    e.set_mode("qso-run").expect("CQ");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 3);
    e.clear_dx_call();
    e.set_tx_enabled(true);
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 5);
    assert_eq!(tx_now(&e), "W1AW KR4FQG -07", "after Enable Tx");

    let mut e = ft8();
    e.set_mode("qso-run").expect("CQ");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 3);
    e.clear_dx_call();
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 5);
    e.call_station("W1AW");
    assert_eq!(e.qso_dxcall(), Some("W1AW"));
    assert_eq!(tx_now(&e), "W1AW KR4FQG -07", "a click on it");
}

/// (3) Every caller cleared since the last arm stays held, not just the latest. In a pile-up the
/// operator clears W1AW, the TX-off run then locks onto K9XYZ, and the operator clears that one too;
/// W1AW calling again must still get nothing, and Enable Tx sends the CQ.
#[test]
fn a_second_clear_keeps_the_first_cleared_caller_held() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    e.set_mode("qso-run").expect("CQ");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 3);
    e.clear_dx_call();
    e.ingest_decodes_for_test(
        &[
            dec_snr("KR4FQG W1AW FN31", -7),
            dec_snr("KR4FQG K9XYZ EN52", -11),
        ],
        5,
    );
    assert_eq!(
        e.qso_dxcall(),
        Some("K9XYZ"),
        "premise: the next caller locked"
    );
    e.clear_dx_call();
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 7);
    assert_eq!(e.qso_dxcall(), None, "W1AW, cleared first, is still held");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG K9XYZ EN52", -11)], 9);
    assert_eq!(e.qso_dxcall(), None, "K9XYZ, cleared second, is held");
    e.set_tx_enabled(true);
    assert_eq!(keyed(&mut e, 10..12), vec!["CQ KR4FQG EM85".to_string()]);
    assert!(e.stored_log().is_empty());
}

/// (3) The hold applies only while the run is calling CQ. With "Double-click sets Tx" off, a click
/// on a cleared caller starts its contact with TX still off and the hold still in place; the
/// contact must still advance on that station's roger, and another held caller must not take it.
#[test]
fn a_click_on_a_held_caller_with_double_click_sets_tx_off_advances_on_its_roger() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    e.settings.double_click_sets_tx = false;
    e.set_mode("qso-run").expect("CQ");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 3);
    e.clear_dx_call();
    e.ingest_decodes_for_test(
        &[
            dec_snr("KR4FQG W1AW FN31", -7),
            dec_snr("KR4FQG K9XYZ EN52", -11),
        ],
        5,
    );
    e.clear_dx_call();
    assert_eq!(e.cleared_dx_calls.len(), 2, "premise: two callers held");
    e.work_station(
        "W1AW",
        None,
        Some("KR4FQG W1AW FN31"),
        Some(-7),
        Some(1200.0),
    )
    .expect("click");
    assert_eq!(e.qso_dxcall(), Some("W1AW"));
    assert_eq!(tx_now(&e), "W1AW KR4FQG -07", "the click answers it");
    assert!(!e.tx_enabled, "premise: the click did not arm TX");
    e.ingest_decodes_for_test(
        &[
            dec_snr("KR4FQG W1AW R-10", -7),
            dec_snr("KR4FQG K9XYZ EN52", -11),
        ],
        7,
    );
    assert_eq!(
        e.qso_dxcall(),
        Some("W1AW"),
        "K9XYZ does not take the contact"
    );
    assert_eq!(
        tx_now(&e),
        "W1AW KR4FQG RR73",
        "the clicked contact advances"
    );
}

/// (3) The cleared caller's bare report (no grid) is the other answer a CQ run locks onto; it is
/// held too, in the portable and the hashed forms as well. Its RR73 and 73 lock nothing either.
#[test]
fn a_cleared_callers_report_is_not_answered_in_any_form() {
    let mut e = ft8();
    e.set_mode("qso-run").expect("CQ");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 3);
    e.clear_dx_call();
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW -10", -7)], 5);
    assert_eq!(e.qso_dxcall(), None, "bare report");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW/P FN31", -7)], 7);
    assert_eq!(e.qso_dxcall(), None, "portable form");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG <W1AW> -10", -7)], 9);
    assert_eq!(e.qso_dxcall(), None, "hashed form");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW RR73", -7)], 11);
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW 73", -7)], 13);
    assert_eq!(e.qso_dxcall(), None);
    e.set_tx_enabled(true);
    assert_eq!(keyed(&mut e, 14..16), vec!["CQ KR4FQG EM85".to_string()]);
}

/// (3) An arm that is refused (here the clock-repair hold) is not the operator choosing to transmit
/// again: both cleared callers stay held. The arm after the hold ends releases them.
#[test]
fn a_refused_arm_keeps_the_cleared_callers_held() {
    let mut e = ft8();
    e.set_mode("qso-run").expect("CQ");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 3);
    e.clear_dx_call();
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG K9XYZ EN52", -11)], 5);
    e.clear_dx_call();
    e.hold_tx_for_clock_repair(std::time::Instant::now() + std::time::Duration::from_secs(600));
    e.set_tx_enabled(true);
    assert!(!e.tx_enabled, "premise: the arm was refused");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 7);
    assert_eq!(e.qso_dxcall(), None, "W1AW still held");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG K9XYZ EN52", -11)], 9);
    assert_eq!(e.qso_dxcall(), None, "K9XYZ still held");
    e.end_clock_repair_hold();
    e.set_tx_enabled(true);
    assert!(e.tx_enabled);
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 11);
    assert_eq!(e.qso_dxcall(), Some("W1AW"), "released by the arm");
}

/// (3) Call CQ arms TX, so it releases the cleared callers like Enable Tx does.
#[test]
fn call_cq_releases_the_cleared_callers() {
    let mut e = ft8();
    e.set_mode("qso-run").expect("CQ");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 3);
    e.clear_dx_call();
    assert!(!e.tx_enabled);
    e.set_mode("qso-run").expect("CQ");
    assert!(e.tx_enabled, "premise: Call CQ arms TX");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 5);
    assert_eq!(e.qso_dxcall(), Some("W1AW"));
    assert_eq!(tx_now(&e), "W1AW KR4FQG -07");
}

/// (3) With a best-caller choice other than first heard, a held caller is set aside BEFORE the pick,
/// so it never hides another station answering in the same period: here W1AW is the strongest,
/// and K9XYZ is answered.
#[test]
fn a_held_caller_does_not_hide_another_from_the_best_caller_pick() {
    let mut e = ft8();
    e.settings.best_caller = "strongest".into();
    e.set_mode("qso-run").expect("CQ");
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -2)], 3);
    e.clear_dx_call();
    e.ingest_decodes_for_test(
        &[
            dec_snr("KR4FQG W1AW FN31", -2),
            dec_snr("KR4FQG K9XYZ EN52", -11),
        ],
        5,
    );
    assert_eq!(e.qso_dxcall(), Some("K9XYZ"));
    assert_eq!(tx_now(&e), "K9XYZ KR4FQG -11");
}

/// (3) CONTROL: calling CQ with nobody in the contact, Clear DX ends nothing and TX stays on
/// (`if (m_QSOProgress != CALLING) auto_tx_mode(false)`, :10078).
#[test]
fn clearing_while_calling_cq_leaves_tx_on() {
    let mut e = ft8();
    e.set_mode("qso-run").expect("CQ");
    e.clear_dx_call();
    assert!(e.tx_enabled);
    assert_eq!(key_once(&mut e, 2), "CQ KR4FQG EM85");
}

/// (3) After a run has moved on, the late answer it still owes its last partner (WSJT-X's
/// `m_hisCall`, which `clearDX` empties) is dropped by the clear: their late RR73 earns no 73.
/// CONTROL: without the clear it does.
#[test]
fn clearing_drops_the_late_answer_owed_to_the_last_partner() {
    for clear in [false, true] {
        let mut e = ft8();
        e.set_mode("qso-run").expect("CQ");
        e.recent_partner = Some(RecentPartner {
            call: "W1AW".into(),
            answer: LateAnswer::Bye73,
            confirm_with_rrr: false,
            until_slot: 100,
            owed: None,
        });
        if clear {
            e.clear_dx_call();
        }
        e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW RR73", -7)], 5);
        let sent = key_once(&mut e, 6);
        let late = e.own_tx.iter().any(|o| o.text == "W1AW KR4FQG 73");
        assert_eq!(late, !clear, "clear {clear}: sent {sent:?}");
    }
}

/// (4) A click on an R-report opens with Tx4; Tx2 and Tx3 are never selected, so WSJT-X sets the
/// start time when it logs, as its RR73 starts (mainwindow.cpp:8237-8249): now - (4-2)*T/R - (now
/// mod T/R) (mainwindow_slots.cpp:496-500). The contact logs that, not the time it was logged.
#[test]
fn a_contact_never_on_tx2_or_tx3_logs_wsjtx_start_estimate() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU R-15", -9)], 21);
    e.call_station(DX);
    assert_eq!(tx_now(&e), "7P8RU KR4FQG RR73", "premise: Tx4");
    assert_eq!(e.qso_start_unix, None, "Tx4 sets nothing (:9182)");
    let before = now_unix_secs();
    key_once(&mut e, 22);
    let after = now_unix_secs();
    let window = wsjtx_qso_on(before, 4, 15.0)..=wsjtx_qso_on(after, 4, 15.0);
    assert!(
        e.qso_start_unix.is_some_and(|on| window.contains(&on)),
        "estimated as the RR73 leaves: {:?}",
        e.qso_start_unix
    );
    e.ingest_decodes_for_test(&[], 25);
    let log = e.stored_log();
    assert_eq!(log.len(), 1, "the contact logged");
    assert!(
        window.contains(&log[0].when_unix),
        "TIME_ON {} is WSJT-X's estimate {window:?}",
        log[0].when_unix
    );
}

/// (4) With auto-log off, the Log button at Tx4 logs the same estimate, taken when it is pressed.
#[test]
fn the_log_button_on_a_contact_never_on_tx2_or_tx3_logs_wsjtx_start_estimate() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU R-15", -9)], 21);
    e.call_station(DX);
    key_once(&mut e, 22);
    e.ingest_decodes_for_test(&[], 25);
    assert_eq!(
        e.qso_start_unix, None,
        "auto-log off: nothing set as the RR73 left"
    );
    let before = now_unix_secs();
    assert!(e.log_current_qso(), "the Log button logs it");
    let after = now_unix_secs();
    let log = e.stored_log();
    assert_eq!(log.len(), 1);
    assert!(
        (wsjtx_qso_on(before, 4, 15.0)..=wsjtx_qso_on(after, 4, 15.0)).contains(&log[0].when_unix),
        "TIME_ON {}",
        log[0].when_unix
    );
}

/// (3) Field Day: WSJT-X's `clearDX` is the same in its Field Day activity, so the clear ends the
/// contact there too. The sequencer goes back to listening for its next station, as after a
/// contact, Enable Tx goes off, and nothing more goes to the station.
#[test]
fn clearing_the_dx_call_ends_a_field_day_contact() {
    let mut e = Engine::new("K1ABC", "FN42", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_event = "arrlfd".into();
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    s.fd_position_id = "aaaa0001".into();
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.set_mode("fieldday-sp").expect("Field Day runs");
    let cq = "CQ FD W9XYZ EN37";
    e.ingest_decodes_for_test(&[dec_snr(cq, -12)], 1);
    e.work_station("W9XYZ", Some("EN37"), Some(cq), Some(-12), None)
        .expect("the double-click is taken");
    let partner = |e: &Engine| e.snapshot().field_day.and_then(|f| f.dxcall);
    assert_eq!(
        partner(&e).as_deref(),
        Some("W9XYZ"),
        "premise: in the contact"
    );
    assert!(e.tx_enabled, "premise: TX on");
    e.clear_dx_call();
    assert_eq!(partner(&e), None, "the contact is over");
    assert!(!e.tx_enabled, "Enable Tx off");
    let before = e.own_tx.len();
    e.set_tx_enabled(true);
    for slot in 2..8 {
        let _ = e.poll_tx(slot);
    }
    let sent: Vec<&str> = e
        .own_tx
        .iter()
        .skip(before)
        .map(|o| o.text.as_str())
        .collect();
    assert!(
        !sent.iter().any(|t| t.contains("W9XYZ")),
        "nothing to the station: {sent:?}"
    );
}

/// (3) Clear DX drops an earlier abandoned contact kept for the Log button only when it is the
/// cleared station's. CONTROL: one with another station stays loggable.
#[test]
fn clearing_drops_an_abandoned_contact_with_the_same_station_only() {
    for (stalled, kept) in [("W1AW", false), ("K9XYZ", true)] {
        let mut e = ft8();
        e.set_mode("qso-run").expect("CQ");
        e.ingest_decodes_for_test(&[dec_snr("KR4FQG W1AW FN31", -7)], 3);
        e.stalled_qso = Some(StalledQso {
            dxcall: stalled.into(),
            dxgrid: None,
            rx_report: Some(-5),
            tx_report: Some(-7),
            start_unix: None,
            band: "20m".into(),
            freqs: (14.074, None),
            mode: "FT8".into(),
            end_unix: 0,
            confirm_only: false,
        });
        e.clear_dx_call();
        assert_eq!(
            e.stalled_qso.is_some(),
            kept,
            "abandoned contact with {stalled}"
        );
    }
}

/// Click the station's RR73 (or RRR) to us, send our 73, then fold the next period.
fn click_their_roger_and_send_73(e: &mut Engine, theirs: &str) {
    e.ingest_decodes_for_test(&[dec_snr(theirs, -9)], 21);
    e.call_station(DX);
    assert_eq!(
        key_once(e, 22),
        "7P8RU KR4FQG 73",
        "premise: our 73 goes out"
    );
    e.ingest_decodes_for_test(&[], 25);
}

/// Operator ruling, 2026-10-27: after a click on the station's RR73 or RRR, our 73 is offered for
/// the log, as WSJT-X opens its Log QSO dialog as the 73 starts (mainwindow.cpp:8237-8249). No
/// report was exchanged in this contact, so it is never logged silently: with auto-log on it goes
/// to the confirm-before-log popup even with that setting off, and the operator decides.
#[test]
fn a_73_sent_from_a_click_on_their_roger_is_offered_for_the_log() {
    for theirs in ["KR4FQG 7P8RU RR73", "KR4FQG 7P8RU RRR"] {
        let mut e = ft8();
        e.settings.auto_log = true;
        e.settings.prompt_to_log = false;
        click_their_roger_and_send_73(&mut e, theirs);
        assert!(
            e.stored_log().is_empty(),
            "{theirs:?}: never logged silently"
        );
        let shown = e
            .snapshot()
            .pending_log
            .unwrap_or_else(|| panic!("{theirs:?}: offered in the popup"));
        assert_eq!(shown.call, DX);
        e.ingest_decodes_for_test(&[], 27);
        assert_eq!(e.pending_logs.len(), 1, "{theirs:?}: offered once");
        let key = e.pending_qso_log_key().unwrap_or_default();
        assert!(e.confirm_pending_log(&key, shown.into()));
        assert_eq!(e.stored_log().len(), 1, "{theirs:?}: logged once confirmed");
    }
}

/// Operator ruling, 2026-10-27: with auto-log off, the Log button on that contact opens the
/// popup too (WSJT-X's Log QSO button opens its dialog), whatever the confirm setting says.
#[test]
fn the_log_button_after_a_click_on_their_roger_opens_the_popup() {
    let mut e = ft8();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    click_their_roger_and_send_73(&mut e, "KR4FQG 7P8RU RR73");
    assert!(
        e.snapshot().pending_log.is_none(),
        "auto-log off: nothing offered by itself"
    );
    assert!(e.log_current_qso(), "the Log button takes it");
    assert!(e.stored_log().is_empty(), "not logged without the confirm");
    assert_eq!(
        e.snapshot().pending_log.map(|r| r.call).as_deref(),
        Some(DX),
        "offered in the popup"
    );
}

/// Operator ruling, 2026-10-27: a logger program's UDP Reply on the station's RR73 or RRR is a
/// click made there, as WSJT-X treats it (`processMessage`), so our 73 is offered exactly as after
/// a click in Nexus: in the popup, never logged without the confirm.
#[test]
fn a_logger_reply_on_their_roger_is_offered_like_a_click() {
    let mut e = ft8();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    e.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU RR73", -9)], 21);
    e.answer_udp_reply(DX, "KR4FQG 7P8RU RR73", -9, None)
        .expect("the Reply is taken");
    assert_eq!(
        key_once(&mut e, 22),
        "7P8RU KR4FQG 73",
        "premise: our 73 goes out"
    );
    e.ingest_decodes_for_test(&[], 25);
    assert!(e.stored_log().is_empty(), "never logged silently");
    assert_eq!(
        e.snapshot().pending_log.map(|r| r.call).as_deref(),
        Some(DX),
        "offered in the popup"
    );
}

/// CONTROL: the phantom-contact guard still holds before our 73 has gone out: a click's contact
/// is not offered, nor taken by the Log button.
#[test]
fn the_phantom_guard_still_holds_off_the_roger_click() {
    let mut c = ft8();
    c.settings.auto_log = false;
    c.ingest_decodes_for_test(&[dec_snr("KR4FQG 7P8RU RR73", -9)], 21);
    c.call_station(DX);
    assert!(
        !c.log_current_qso(),
        "before our 73 is on the air: nothing to log"
    );
    assert!(c.pending_logs.is_empty());
}
