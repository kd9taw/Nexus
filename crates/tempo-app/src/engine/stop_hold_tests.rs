//! ⛔ A STOP HOLDS (the operator's ruling, 2026-10-11: "After Stop TX, Esc or the watchdog, TX
//! stays off until your own act: TX On, a CW F-key, the first PTT press or RTTY's TX button. Entry
//! still arms when nothing stopped TX."). Entering Phone, CW, RTTY or PSK arms TX like a rig's live
//! mic and key, but only while nothing holds it: after Stop TX, Esc, TX Off, an FT Halt or a
//! watchdog trip, TX stays off on every screen the operator visits until one of their own acts
//! turns it back on. And a halt ends an RTTY Auto run, as it ends JS8's schedule ("The run must be
//! restarted on purpose").
//!
//! The UI re-asserts the section on every view change, so `set_operating_mode(_, false)` below is
//! a trip to the Contest screen, the Map or the Logbook and back. The operator's TX On/Off button
//! (the first PTT press in Phone and RTTY's TX button reach the engine as the same command) is
//! [`Engine::set_tx_enabled_by_operator`].

use super::*;

/// A station with every HF privilege, at launch: TX off, nothing held.
fn station() -> Engine {
    let mut e = Engine::new("K1ABC", "EN50", 0);
    e.set_license_class("extra");
    e
}

/// The Illinois QSY Party running, as the `ilqp_engine` helper builds it: the Contest screen's own
/// entry is `set_mode("fieldday-sp")`.
fn run_the_party(e: &mut Engine) {
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_event = "ilqp".into();
    s.contest_qth_state = "IL".into();
    s.contest_qth_county = "MCHE".into();
    s.contest_entry_class = "UNLIMITED".into();
    s.fd_position_id = "A".into();
    e.apply_settings(s);
}

/// The rehearsal's own path: Phone, Stop TX, the Contest screen, back to Phone.
#[test]
fn stop_tx_then_the_contest_screen_and_back_leaves_tx_off() {
    let mut e = station();
    run_the_party(&mut e);
    e.set_mode("fieldday-sp").expect("the party runs");
    e.set_operating_mode("phone", true);
    e.set_frequency(14.150, "20m", "USB");
    assert!(
        e.tx_enabled(),
        "control: entering Phone with nothing stopped arms TX"
    );
    e.halt_tx(); // Stop TX in the TX strip
    let after_stop = e.tx_enabled();
    let _ = e.set_mode("fieldday-sp"); // the Contest screen's entry, at its worst
    let on_contest = e.tx_enabled();
    e.set_operating_mode("phone", false); // back to Phone: the view's re-assert
    assert_eq!(
        (after_stop, on_contest, e.tx_enabled()),
        (false, false, false),
        "(after Stop TX, on the Contest screen, back in Phone): TX came back on with no act"
    );
}

/// After Stop TX, entering any manual cockpit from any other (or re-entering the same one) leaves
/// TX off. Every pair is measured before anything is asserted, so a red names them all.
#[test]
fn entering_any_manual_cockpit_after_stop_tx_leaves_tx_off() {
    let cockpits = ["phone", "cw", "rtty", "keyboard"];
    let mut matrix = Vec::new();
    for from in cockpits {
        for to in cockpits {
            let mut e = station();
            e.set_operating_mode(from, true);
            let armed = e.tx_enabled();
            e.halt_tx();
            let off = !e.tx_enabled();
            e.set_operating_mode(to, from != to);
            matrix.push((from, to, armed, off, e.tx_enabled()));
        }
    }
    assert!(
        matrix.iter().all(|r| r.2 && r.3),
        "control: every cockpit armed on entry and Stop TX disarmed it: {matrix:?}"
    );
    let rearmed: Vec<_> = matrix.iter().filter(|r| r.4).map(|r| (r.0, r.1)).collect();
    assert!(
        rearmed.is_empty(),
        "entry turned TX back on after Stop TX: {rearmed:?}"
    );
}

/// Controls: a QSY, a band change and entering Digital keep TX off after Stop TX, the operator's
/// own TX On arms, and Stop TX still stops.
#[test]
fn a_qsy_a_band_change_and_digital_keep_tx_off_after_stop_tx() {
    let mut e = station();
    e.set_operating_mode("phone", true);
    e.set_frequency(14.250, "20m", "USB");
    e.halt_tx();
    e.set_frequency(14.260, "20m", "USB"); // an in-band QSY
    assert!(!e.tx_enabled(), "an in-band QSY keeps TX off");
    e.set_frequency(7.200, "40m", "LSB"); // a band change
    assert!(!e.tx_enabled(), "a band change keeps TX off");
    e.set_operating_mode("digital", true); // Operate
    assert!(!e.tx_enabled(), "entering Digital keeps TX off");
    e.set_tx_enabled_by_operator(true); // the operator's TX On
    assert!(e.tx_enabled(), "the operator's TX On arms");
    e.set_operating_mode("cw", true);
    e.halt_tx();
    assert!(!e.tx_enabled(), "Stop TX still stops in CW");
}

/// An RTTY Auto CQ run with one CQ sent, then RTTY's Stop TX (`rttyStop` + `haltTx`), then (or not)
/// a trip to another screen and back, then the listen window: what keyed, and the run's state.
fn rtty_auto_cq_after_stop(round_trip: bool) -> (Option<String>, String) {
    let mut e = Engine::new("K1ABC", "EN61", 0);
    e.set_license_class("general");
    e.set_operating_mode("rtty", false);
    e.set_frequency(14.080, "20m", "LSB");
    let mut s = e.settings().clone();
    s.rtty_auto_listen_secs = 5; // the clamp's floor: the test waits 5 s, not 30
    e.apply_settings(s);
    e.set_tx_enabled_by_operator(true); // a no-op where entry already armed
    e.set_rtty_auto(true).unwrap();
    e.rtty_auto_cq().unwrap();
    e.set_rtty_sending(true);
    assert!(
        e.poll_rtty_one().is_some(),
        "control: the operator's CQ keys"
    );
    e.set_rtty_sending(false);
    e.rtty_auto_service(); // the over played out
    assert_eq!(
        e.rtty_state().seq_state,
        "calling_cq",
        "control: the run is on"
    );
    e.rtty_stop(); // RTTY's Stop TX: rttyStop()…
    e.halt_tx(); // …and haltTx()
    assert!(!e.tx_enabled(), "Stop TX shows TX Off");
    if round_trip {
        e.set_operating_mode("rtty", false); // RTTY → Contest/Map → RTTY
    }
    std::thread::sleep(std::time::Duration::from_millis(5_300));
    e.rtty_auto_service(); // the radio loop's tick after the listen window
    let keyed = e.poll_rtty_one();
    (keyed, e.rtty_state().seq_state)
}

#[test]
fn an_rtty_auto_run_keys_nothing_after_stop_tx_and_a_round_trip() {
    let (keyed, state) = rtty_auto_cq_after_stop(true);
    assert_eq!(
        (keyed, state.as_str()),
        (None, "idle"),
        "a CQ went to the keyer, or the run went on, with no act since Stop TX"
    );
}

/// Control: the same run, the same Stop TX, no screen trip.
#[test]
fn an_rtty_auto_run_keys_nothing_after_stop_tx_without_a_round_trip() {
    let (keyed, state) = rtty_auto_cq_after_stop(false);
    assert_eq!((keyed, state.as_str()), (None, "idle"));
}

/// A program on Nexus's CAT share (WSJT-X, N1MM) asks for PTT: refused after Stop TX, and still
/// refused after a screen round trip.
#[test]
fn the_cat_share_key_stays_refused_after_stop_tx_and_a_round_trip() {
    let mut e = station();
    e.settings.cat_broker_ptt = true;
    e.set_operating_mode("phone", true);
    e.set_frequency(14.250, "20m", "USB");
    assert!(e.broker_ptt(true), "control: with TX on the share keys");
    assert!(e.broker_ptt(false), "control: and unkeys");
    e.halt_tx();
    let after_stop = e.broker_ptt(true);
    e.set_operating_mode("phone", false);
    let after_round_trip = e.broker_ptt(true);
    assert_eq!(
        (after_stop, after_round_trip),
        (false, false),
        "(after Stop TX, after the round trip): the foreign key got through"
    );
}

/// ⛔ A program on Nexus's TCI server is no operator: after Stop TX its key, its microphone and
/// its tune are each refused, and the stop still holds at the next cockpit entry, until the
/// operator's own act.
#[test]
fn a_tci_program_after_stop_tx_arms_nothing_and_the_stop_still_holds() {
    use tempo_net::tci::server::{KeyKind, Mic, Refusal};
    let mut e = station();
    e.set_operating_mode("phone", true);
    e.set_frequency(14.250, "20m", "USB");
    e.settings.tci_server_tx = true;
    assert_eq!(
        e.tci_key(1, KeyKind::Mic(Mic::Selected), false),
        Ok(()),
        "control: with TX on a program keys"
    );
    e.halt_tx();
    let refused = [
        e.tci_key(1, KeyKind::Tci, true),
        e.tci_key(2, KeyKind::Mic(Mic::Selected), false),
        e.tci_tune_on(1),
    ];
    e.set_operating_mode("phone", false); // entry arms only where nothing holds
    assert_eq!(
        (refused, e.tx_enabled()),
        ([Err(Refusal::StopHold); 3], false),
        "(a program's over, its microphone and its tune; armed at Phone entry)"
    );
}

/// RTTY with a one-minute watchdog whose clock started 61 s ago, and a message queued: the next
/// poll trips it.
fn rtty_watchdog_tripped(e: &mut Engine) {
    e.set_operating_mode("rtty", false);
    e.set_frequency(14.080, "20m", "LSB");
    e.settings.tx_watchdog_min = 1;
    e.set_tx_enabled_by_operator(true); // a no-op where entry already armed
    e.rtty_send_text("TEST").unwrap();
    e.tx_watchdog_start = Some(now_unix_secs().saturating_sub(61));
    assert!(
        e.poll_rtty_one().is_none(),
        "control: the watchdog trips at the poll"
    );
    let radio = e.snapshot().radio;
    assert_eq!(
        (radio.tx_watchdog, radio.tx_enabled),
        (true, false),
        "control: the watchdog tripped and disarmed"
    );
}

#[test]
fn a_watchdog_trip_holds_tx_off_across_a_round_trip() {
    let mut e = Engine::new("K1ABC", "EN61", 0);
    e.set_license_class("general");
    rtty_watchdog_tripped(&mut e);
    e.set_operating_mode("rtty", false); // RTTY → Map → RTTY
    let radio = e.snapshot().radio;
    assert_eq!(
        (radio.tx_watchdog, radio.tx_enabled),
        (true, false),
        "(watchdog, TX) after the round trip: entry cleared the trip or re-armed"
    );
}

/// The Contest screen's entry (`set_mode`) restarts the watchdog clock, which takes the tripped
/// flag down with TX still off. The trip still holds.
#[test]
fn a_watchdog_trip_holds_tx_off_across_the_contest_screen() {
    let mut e = Engine::new("K1ABC", "EN61", 0);
    e.set_license_class("general");
    run_the_party(&mut e);
    rtty_watchdog_tripped(&mut e);
    e.set_mode("fieldday-sp").expect("the party runs");
    assert!(
        !e.snapshot().radio.tx_watchdog,
        "scene guard: the Contest screen's entry restarted the watchdog clock"
    );
    e.set_operating_mode("rtty", false); // back to RTTY
    assert!(
        !e.tx_enabled(),
        "back from the Contest screen, entry re-armed TX behind a watchdog trip"
    );
}

/// What each press does after Stop TX, with or without a screen round trip after it.
fn presses(round_trip: bool) -> Vec<(&'static str, bool)> {
    let stopped = |mode: &str, mhz: f64, band: &str, side: &str| {
        let mut e = station();
        e.set_operating_mode(mode, true);
        e.set_frequency(mhz, band, side);
        e.halt_tx();
        if round_trip {
            e.set_operating_mode(mode, false);
        }
        e
    };
    let mut cw_enter = stopped("cw", 7.030, "40m", "CW");
    let mut cw_fkey = stopped("cw", 7.030, "40m", "CW");
    cw_fkey.send_cw("TEST");
    let mut rtty = stopped("rtty", 14.080, "20m", "LSB");
    let mut keyer = stopped("phone", 14.250, "20m", "USB");
    let mut ptt = stopped("phone", 14.250, "20m", "USB");
    ptt.set_ptt(true);
    vec![
        // ESM's Enter in CW (`send_cw_armed`, its rule 5)
        (
            "CW Enter with ESM sends",
            cw_enter.send_cw_armed("TEST").is_ok(),
        ),
        // an F-key is the operator's own act and keys (`cw_send_re_arms_tx_after_a_halt`)
        ("CW F-key keys", cw_fkey.poll_cw_one().is_some()),
        // an F-key macro or typed send in RTTY (and ESM's Enter there)
        ("RTTY F-key sends", rtty.rtty_send_text("CQ TEST").is_ok()),
        // the voice keyer (and ESM's Enter in Phone)
        (
            "Phone voice keyer plays",
            keyer.send_voice(vec![0.0; 4_800]).is_ok(),
        ),
        // the first PTT press
        ("Phone PTT keys", ptt.manual_ptt()),
    ]
}

#[test]
fn every_press_after_a_round_trip_does_what_it_did_after_stop_tx() {
    let after_stop = presses(false);
    assert_eq!(
        after_stop,
        vec![
            ("CW Enter with ESM sends", false),
            ("CW F-key keys", true),
            ("RTTY F-key sends", false),
            ("Phone voice keyer plays", false),
            ("Phone PTT keys", false),
        ],
        "control: what each press does straight after Stop TX"
    );
    assert_eq!(
        presses(true),
        after_stop,
        "a screen round trip changed what a press does after Stop TX"
    );
}

/// Each of the operator's own acts ends a stop's hold. The hold shows only at a section entry, so
/// each act is followed by a disarm that is NOT a stop (leaving a manual mode for Digital, or FT's
/// "Disable Tx after sending 73"), then Phone entry, which arms only when nothing holds.
#[test]
fn every_operator_arm_ends_the_hold() {
    type Act = fn(&mut Engine);
    let manual: [(&str, Act); 2] = [
        // TX On in any cockpit's strip; the first PTT press in Phone and RTTY's TX button send
        // the same command.
        ("TX On", |e| e.set_tx_enabled_by_operator(true)),
        ("a CW F-key", |e| e.send_cw("TEST")),
    ];
    for (name, act) in manual {
        let mut e = station();
        e.set_operating_mode("cw", true);
        e.set_frequency(7.030, "40m", "CW");
        e.halt_tx(); // Stop TX
        act(&mut e);
        assert!(e.tx_enabled(), "{name}: control: the act arms");
        e.set_operating_mode("digital", false); // leaving CW disarms, and is not a stop
        assert!(!e.tx_enabled(), "{name}: control: Digital disarmed");
        e.set_operating_mode("phone", false);
        assert!(e.tx_enabled(), "{name}: the hold outlived the act");
    }
    // A logger's UDP Reply is not on this list: another program never ends a stop (below).
    let ft: [(&str, Act); 3] = [
        ("Enable TX", |e| e.set_tx_enabled_by_operator(true)),
        ("Call CQ", |e| e.set_mode("qso-run").expect("Call CQ")),
        ("a double-click", |e| {
            e.call_station_ctx("K2ABC", Some("FN42"), None, None, None)
                .expect("the double-click works the station")
        }),
    ];
    for (name, act) in ft {
        let mut e = station();
        e.set_operating_mode("digital", true);
        e.set_tx_enabled_by_operator(true);
        e.halt_tx(); // the FT8 Halt
        act(&mut e);
        assert!(e.tx_enabled(), "{name}: control: the act arms");
        e.set_tx_enabled(false); // "Disable Tx after sending 73", from the radio loop
        e.set_operating_mode("phone", false);
        assert!(e.tx_enabled(), "{name}: the hold outlived the act");
    }
}

/// Entry arms exactly as before wherever nothing stopped TX.
#[test]
fn entry_arms_tx_when_nothing_stopped_it() {
    for cockpit in ["phone", "cw", "rtty", "keyboard"] {
        let mut e = station();
        e.set_operating_mode(cockpit, true);
        assert!(e.tx_enabled(), "{cockpit}: entry at launch did not arm");
    }
    // Phone, then FT8 (the disarm on leaving a manual mode), then Phone again.
    let mut e = station();
    e.set_operating_mode("phone", true);
    e.set_operating_mode("digital", true);
    assert!(!e.tx_enabled(), "control: entering Digital disarmed");
    e.set_operating_mode("phone", true);
    assert!(e.tx_enabled(), "Phone → FT8 → Phone did not arm");
    // FT8 with a contact worked and TX turned off after its 73, but no Halt.
    let mut e = station();
    e.set_operating_mode("digital", true);
    e.set_mode("qso-run").expect("Call CQ");
    e.set_tx_enabled(false); // "Disable Tx after sending 73", from the radio loop
    e.set_operating_mode("phone", true);
    assert!(e.tx_enabled(), "FT8 with no Halt → Phone did not arm");
}

/// TX Off is a stop: it holds. FT's "Disable Tx after sending 73" lowers the same latch and is
/// not one (the control).
#[test]
fn tx_off_holds_and_the_after_73_disable_does_not() {
    let mut e = station();
    e.set_operating_mode("rtty", true);
    e.set_tx_enabled_by_operator(false); // TX Off in RTTY's strip
    e.set_operating_mode("rtty", false);
    assert!(!e.tx_enabled(), "TX Off, then a round trip: entry re-armed");
    e.set_operating_mode("digital", true);
    e.set_operating_mode("phone", true);
    assert!(
        !e.tx_enabled(),
        "TX Off, then FT8 and Phone: entry re-armed"
    );

    let mut e = station();
    e.set_operating_mode("digital", true);
    e.set_tx_enabled_by_operator(true); // Enable TX
    e.set_tx_enabled(false); // "Disable Tx after sending 73", from the radio loop
    e.set_operating_mode("phone", true);
    assert!(
        e.tx_enabled(),
        "control: the after-73 disable held TX off like a stop"
    );
}

/// After an FT8 Halt, Phone opens with TX off. The first PTT press does not key: Phone turns TX
/// back on and says so (`PhoneCockpit.txoff.test.tsx`, state 2), and the next press keys.
#[test]
fn after_an_ft8_halt_phone_entry_leaves_tx_off_and_the_first_ptt_arms() {
    let mut e = station();
    e.set_operating_mode("digital", true);
    e.set_tx_enabled_by_operator(true); // Enable TX
    e.halt_tx(); // the FT8 Halt (Stop TX, Esc, or WSJT-X's HaltTx)
    e.set_operating_mode("phone", true);
    e.set_frequency(14.250, "20m", "USB");
    assert!(!e.tx_enabled(), "Phone entry armed TX after an FT8 Halt");
    e.set_ptt(true); // the first press reaches the engine…
    assert!(!e.manual_ptt(), "the first PTT press keyed with TX off");
    e.set_ptt(false);
    e.set_tx_enabled_by_operator(true); // …and Phone turns TX back on (the toast)
    e.set_ptt(true); // the next press
    assert!(e.manual_ptt(), "control: the next PTT press keys");
}

/// Stop TX ends an RTTY Auto run at once; Auto itself stays on, and the run restarts only on
/// purpose.
#[test]
fn stop_tx_ends_an_rtty_auto_run() {
    let mut e = Engine::new("K1ABC", "EN61", 0);
    e.set_license_class("general");
    e.set_operating_mode("rtty", false);
    e.set_frequency(14.080, "20m", "LSB");
    e.set_rtty_auto(true).unwrap();
    e.rtty_auto_cq().unwrap();
    e.set_rtty_sending(true);
    assert!(
        e.poll_rtty_one().is_some(),
        "control: the operator's CQ keys"
    );
    e.set_rtty_sending(false);
    e.rtty_auto_service();
    assert_eq!(
        e.rtty_state().seq_state,
        "calling_cq",
        "control: the run is on"
    );
    e.rtty_stop(); // RTTY's Stop TX: rttyStop()…
    e.halt_tx(); // …and haltTx()
    let state = e.rtty_state();
    assert_eq!(
        (state.seq_state.as_str(), state.auto),
        ("idle", true),
        "(the run, Auto) after Stop TX"
    );
    e.set_tx_enabled_by_operator(true);
    e.rtty_auto_cq().expect("the operator restarts the run");
    assert!(
        e.poll_rtty_one().is_some(),
        "control: a run restarted on purpose keys its CQ"
    );
}

/// A band change halts and puts the operator's arm switch back where it was: it is not a stop, so
/// it neither holds TX off nor lets go of a hold.
#[test]
fn a_band_change_neither_holds_nor_releases_tx() {
    // Not a stop: FT8 armed, a band change halts it, and Phone still arms.
    let mut e = station();
    e.set_operating_mode("digital", true);
    e.set_tx_enabled_by_operator(true); // Enable TX
    e.set_frequency(7.074, "40m", "USB"); // a band change: halted and left off in FT8
    assert!(!e.tx_enabled(), "scene guard: the band change halted FT8");
    e.set_operating_mode("phone", true);
    assert!(e.tx_enabled(), "a band change held TX off like a stop");
    // Not a release: after Stop TX, a band change and a return to Phone leave TX off.
    let mut e = station();
    e.set_operating_mode("phone", true);
    e.set_frequency(14.250, "20m", "USB");
    e.halt_tx();
    e.set_frequency(7.200, "40m", "LSB");
    e.set_operating_mode("phone", false);
    assert!(!e.tx_enabled(), "a band change let go of Stop TX's hold");
}

/// ⛔ After a watchdog trip, a program on Nexus's TCI server is refused and the trip stands.
#[test]
fn a_tci_program_after_a_watchdog_trip_arms_nothing_and_leaves_the_trip() {
    use tempo_net::tci::server::Refusal;
    let mut e = Engine::new("K1ABC", "EN61", 0);
    e.set_license_class("general");
    rtty_watchdog_tripped(&mut e);
    e.settings.tci_server_tx = true;
    let r = e.tci_tune_on(1);
    let radio = e.snapshot().radio;
    assert_eq!(
        (r, radio.tx_watchdog, radio.tx_enabled),
        (Err(Refusal::WatchdogTrip), true, false),
        "(a program's tune, watchdog tripped, TX armed)"
    );
}

/// FT8 with K2ABC's CQ decoded, "double-click sets Tx" off: a logger's Reply arms TX whatever it
/// says.
fn ft8_with_a_cq(e: &mut Engine) {
    e.settings.double_click_sets_tx = false;
    e.set_operating_mode("digital", true);
    e.ingest_decodes_for_test(&[super::tests::dec_snr("CQ K2ABC FN42", -10)], 5);
}

/// A logger's WSJT-X UDP Reply to that CQ, as the radio loop hands it over.
fn udp_reply(e: &mut Engine) -> Result<(), String> {
    e.answer_udp_reply("K2ABC", "CQ K2ABC FN42", -10, Some(1500.0))
}

/// Control: with nothing stopped, a logger's Reply works the station and arms TX, as before.
#[test]
fn a_udp_reply_with_nothing_stopped_works_the_station_and_arms() {
    let mut e = station();
    ft8_with_a_cq(&mut e);
    assert!(
        !e.tx_enabled(),
        "premise: TX is off, and nothing stopped it"
    );
    let r = udp_reply(&mut e);
    assert_eq!(
        (r, e.qso_dxcall().map(str::to_string), e.tx_enabled()),
        (Ok(()), Some("K2ABC".to_string()), true),
        "(answer, station worked, TX armed)"
    );
}

/// ⛔ After Stop TX, a logger's Reply arms nothing and the stop still holds: TX stays off through
/// a screen round trip, until the operator's own act.
#[test]
fn a_udp_reply_after_stop_tx_arms_nothing_and_the_stop_still_holds() {
    let mut e = station();
    ft8_with_a_cq(&mut e);
    e.set_tx_enabled_by_operator(true);
    e.halt_tx(); // the FT8 Halt
    let r = udp_reply(&mut e);
    let armed = e.tx_enabled();
    e.set_operating_mode("phone", false); // entry arms only where nothing holds
    assert_eq!(
        (r.is_err(), armed, e.qso_dxcall(), e.tx_enabled()),
        (true, false, None, false),
        "(refused, armed by the Reply, station worked, armed at Phone entry)"
    );
}

/// ⛔ After a watchdog trip, a logger's Reply arms nothing and leaves the trip standing.
#[test]
fn a_udp_reply_after_a_watchdog_trip_arms_nothing_and_leaves_the_trip() {
    let mut e = Engine::new("K1ABC", "EN61", 0);
    e.set_license_class("general");
    rtty_watchdog_tripped(&mut e);
    ft8_with_a_cq(&mut e);
    let radio = e.snapshot().radio;
    assert_eq!(
        (radio.tx_watchdog, radio.tx_enabled, e.tx_stop_hold),
        (true, false, false),
        "scene guard: the trip stands on FT8 by its own flag, with no stop's hold"
    );
    let r = udp_reply(&mut e);
    let radio = e.snapshot().radio;
    assert_eq!(
        (
            r.is_err(),
            radio.tx_watchdog,
            radio.tx_enabled,
            e.qso_dxcall()
        ),
        (true, true, false, None),
        "(refused, watchdog tripped, TX armed, station worked)"
    );
}

/// ⛔ A logger's FreeText with "send" never arms TX, never restarts the watchdog clock and never
/// clears a stop or a trip. It is queued as the next over (Tx 5) only while the operator has TX
/// armed (WSJT-X's Tx5 goes out only with Enable Tx on).
#[test]
fn a_udp_free_text_never_arms_and_goes_out_only_while_armed() {
    let free_text = |e: &mut Engine| {
        let r = e.udp_free_text("TNX 73", true);
        let queued = e.snapshot().qso.and_then(|q| q.tx_now).as_deref() == Some("TNX 73");
        (r.is_ok(), e.tx_enabled(), queued)
    };
    // Operate's FT8 screen.
    let operate = |e: &mut Engine| e.set_mode("qso-monitor").expect("the FT screen");
    // TX off, nothing stopped.
    let mut off = station();
    operate(&mut off);
    off.set_operating_mode("digital", true);
    assert_eq!(
        free_text(&mut off),
        (false, false, false),
        "TX off: (sent, armed, queued)"
    );
    // After Stop TX: refused, and the stop still holds at Phone entry.
    let mut stopped = station();
    operate(&mut stopped);
    stopped.set_operating_mode("digital", true);
    stopped.set_tx_enabled_by_operator(true);
    stopped.halt_tx();
    let after_stop = free_text(&mut stopped);
    stopped.set_operating_mode("phone", false);
    assert_eq!(
        (after_stop, stopped.tx_enabled()),
        ((false, false, false), false),
        "after Stop TX: ((sent, armed, queued), armed at Phone entry)"
    );
    // After a watchdog trip: refused, and the trip stands.
    let mut tripped = Engine::new("K1ABC", "EN61", 0);
    tripped.set_license_class("general");
    operate(&mut tripped);
    rtty_watchdog_tripped(&mut tripped);
    tripped.set_operating_mode("digital", false);
    let after_trip = free_text(&mut tripped);
    assert_eq!(
        (after_trip, tripped.snapshot().radio.tx_watchdog),
        ((false, false, false), true),
        "after a watchdog trip: ((sent, armed, queued), watchdog tripped)"
    );
    // Control: armed by the operator, the text is queued and the watchdog clock keeps running.
    let mut armed = station();
    operate(&mut armed);
    armed.set_operating_mode("digital", true);
    armed.set_tx_enabled_by_operator(true);
    armed.tx_watchdog_start = Some(1_000);
    assert_eq!(
        free_text(&mut armed),
        (true, true, true),
        "armed: (sent, armed, queued)"
    );
    assert_eq!(
        armed.tx_watchdog_start,
        Some(1_000),
        "the free text restarted the watchdog clock"
    );
}
