//! ⛔ A CW MESSAGE THE RADIO'S OWN KEYER WOULD NOT TAKE IS REFUSED WHOLE, BEFORE ANYTHING KEYS.
//! With the CAT keyer, Hamlib hands the radio's keyer each word inside a radio command, so a word
//! holding a `;`, a control character or a character with no Morse is never handed to it
//! (`tempo_core::cw::sendable`). The keyer used to refuse that word only when its turn came, after
//! the words before it had keyed, and Enter Sends Message had logged the contact by then: a contact
//! in the log whose exchange stopped part way. Every word of a message is now asked before any is
//! queued: a refused message keys nothing, arms nothing and supersedes no stop, and Enter's send
//! answers it as a refusal, so the strip logs nothing. A keyer that puts no text inside a radio
//! command (a WinKeyer, the key line, the soundcard) sends its message as before.

use super::*;

/// The CW screen's words for a `;`: the radio's keyer's own refusal, by value.
const SEMICOLON: &str = "CW not sent: \";\" is never handed to the radio's own keyer, because \
     Kenwood, Yaesu and Elecraft radios read it as the end of a command. Take it out of the \
     message.";

/// The CW screen's words for a character with no Morse in Nexus's table, by value.
fn no_morse(c: &str) -> String {
    format!(
        "CW not sent: Nexus has no Morse for \"{c}\", so it is never handed to the radio's own \
         keyer. Take it out of the message."
    )
}

/// The second command an earlier measurement saw a TS-2000 read after a CW word's `;`: VFO A to
/// 7.000 MHz.
const RETUNE: &str = "FA00007000000";

/// An Extra on `keyer` in the CW section on 40 m CW, TX armed by the section's entry, working
/// K9AAA, with a WinKeyer port and a key-line port as given ("" for none).
fn station_on(keyer: &str, winkeyer_port: &str, key_port: &str) -> Engine {
    let mut s = Engine::new("KD9TAW", "EN52", 0).settings().clone();
    s.winkeyer_port = winkeyer_port.into();
    s.cw_key_port = key_port.into();
    let mut e = Engine::with_settings(s);
    e.set_license_class("extra");
    e.set_cw_keyer(keyer, 600.0);
    e.set_operating_mode("cw", false);
    e.set_frequency(7.03, "40m", "CW");
    e.select_peer("K9AAA");
    assert!(
        e.tx_enabled() && e.tx_allowed(),
        "premise: armed, inside the privileges"
    );
    e
}

/// The default keyer: the radio's own, over CAT.
fn cat_station() -> Engine {
    station_on("cat", "", "")
}

/// Field Day on the CAT keyer, class 3A in `section`: Enter's exchange (`! {EXCH}`) is the
/// strip's call, the class and the section, as Settings holds them.
fn field_day_station(section: &str) -> Engine {
    let mut s = Engine::new("KD9TAW", "EN52", 0).settings().clone();
    s.fd_active = true;
    s.fd_event = "arrlfd".into();
    s.fd_class = "3A".into();
    s.fd_section = section.into();
    let mut e = Engine::with_settings(s);
    e.set_license_class("extra");
    e.set_cw_keyer("cat", 600.0);
    e.set_operating_mode("cw", false);
    e.set_frequency(7.03, "40m", "CW");
    e.select_peer("K9AAA");
    assert!(
        e.tx_enabled() && e.tx_allowed(),
        "premise: armed, inside the privileges"
    );
    e
}

/// Every word still queued for the radio loop, in order.
fn queued(e: &Engine) -> Vec<String> {
    e.cw_queue.iter().cloned().collect()
}

/// What a refused send must leave exactly as it was: the latch, the stop's hold, a watchdog trip,
/// the queue and the SENT echo.
fn untouched(e: &Engine) -> (bool, bool, bool, Vec<String>, Vec<String>) {
    (
        e.tx_enabled(),
        e.tx_stop_hold,
        e.snapshot().radio.tx_watchdog,
        queued(e),
        e.cw_sent(),
    )
}

/// ⛔ TYPED TEXT OR AN F-KEY MACRO WITH A WORD THE RADIO'S KEYER REFUSES QUEUES NOTHING, AND THE CW
/// SCREEN SAYS WHICH CHARACTER. The `;` is in the second word, so the word before it would have
/// keyed; a character with no Morse in the last word, or a macro token typed wrong (left in the
/// text as it was written), is refused the same way: every word is asked, not only the first.
/// CONTROL: a clean message is queued word by word, as before.
#[test]
fn a_typed_message_with_a_word_the_radios_keyer_refuses_queues_nothing_and_says_why() {
    let mut e = cat_station();
    let before = untouched(&e);
    e.send_cw(&format!("CQ TEST;{RETUNE} DE KD9TAW"));
    assert_eq!(
        untouched(&e),
        before,
        "nothing queued, echoed or armed: nothing keys"
    );
    assert_eq!(
        e.cw_keyer_error().as_deref(),
        Some(SEMICOLON),
        "the CW screen says which character"
    );
    assert_eq!(e.poll_cw_one(), None, "nothing keys");
    for (text, c) in [
        ("CQ CQ DE O'BRIEN", "'"),
        ("TU {HISNAM} 73", "{"),
        ("TNX JOS\u{c9} 73", "\u{c9}"),
    ] {
        e.send_cw(text);
        assert_eq!(untouched(&e), before, "{text:?}: nothing queued");
        assert_eq!(
            e.cw_keyer_error(),
            Some(no_morse(c)),
            "{text:?}: the CW screen says which character"
        );
    }
    // CONTROL: ordinary text, and the punctuation Nexus's table has, are queued as before.
    e.send_cw("CQ TEST DE KD9TAW K");
    assert_eq!(queued(&e), ["CQ", "TEST", "DE", "KD9TAW", "K"]);
    e.cw_queue.clear();
    e.send_cw("TU 5NN? = + R/ (KN)");
    assert_eq!(queued(&e), ["TU", "5NN?", "=", "+", "R/", "(KN)"]);
}

/// ⛔ A REFUSED MESSAGE ARMS NOTHING AND SUPERSEDES NO STOP. An F-key is the operator's transmit
/// act, so a send that keys turns TX back on, ends a stop's hold and a watchdog trip, and
/// supersedes a stop the radio loop has not consumed yet. A send that keys nothing must do none of
/// that: after Stop TX the latch stays down, the hold stands, and the CW abort and the mid-over cut
/// still reach the loop; after a watchdog trip, the trip stands. CONTROL: a clean F-key does all
/// of it, as it always has.
#[test]
fn a_refused_message_leaves_the_latch_a_stop_and_a_watchdog_trip_as_they_were() {
    // Stop TX before the radio loop has ticked.
    let mut e = cat_station();
    e.send_cw("CQ TEST");
    e.halt_tx();
    let stopped = untouched(&e);
    e.send_cw("TU;73");
    assert_eq!(
        untouched(&e),
        stopped,
        "TX stays off, the hold stands, nothing queued"
    );
    assert!(
        e.take_cw_abort(),
        "the pending CW abort still reaches the loop"
    );
    assert!(e.take_slot_tx_abort(), "and so does the mid-over cut");
    // CONTROL: a clean F-key turns TX on, ends the hold and supersedes a pending stop.
    e.halt_tx();
    e.send_cw("TU 73");
    assert!(e.tx_enabled() && !e.tx_stop_hold, "control: an F-key arms");
    assert!(
        !e.take_cw_abort() && !e.take_slot_tx_abort(),
        "control: and supersedes the pending stop"
    );
    assert_eq!(queued(&e), ["TU", "73"]);

    // A watchdog trip.
    let mut e = cat_station();
    e.trip_tx_watchdog_for_test();
    let tripped = untouched(&e);
    assert!(tripped.2 && !tripped.0, "premise: tripped, TX off");
    e.send_cw("TU;73");
    assert_eq!(untouched(&e), tripped, "the trip stands, TX stays off");
    // CONTROL: a clean F-key ends the trip, as the operator's act.
    e.send_cw("TU 73");
    assert_eq!(
        (e.tx_enabled(), e.snapshot().radio.tx_watchdog),
        (true, false),
        "control: an F-key ends the trip and arms"
    );
}

/// ⛔ ENTER SENDS MESSAGE: AN EXCHANGE THE RADIO'S KEYER WOULD NOT TAKE IS REFUSED, SO THE STRIP
/// LOGS NOTHING. The strip logs a contact only once Enter's send has taken its message, and shows a
/// refusal in its own line, so the CW screen's warning line does not say it twice. The `;` (a slip
/// of the finger beside `L`) is in the exchange's last word. CONTROL: the exchange as it should be
/// is taken and queued word by word.
#[test]
fn an_enter_send_whose_exchange_the_radios_keyer_refuses_takes_nothing() {
    let mut e = field_day_station("IL;");
    assert_eq!(
        e.preview_cw("! {EXCH}"),
        "K9AAA 3A IL;",
        "premise: the exchange holds the `;`"
    );
    let before = untouched(&e);
    assert_eq!(
        e.send_cw_armed("! {EXCH}"),
        Err(SEMICOLON.to_string()),
        "Enter's send is refused, with the character"
    );
    assert_eq!(untouched(&e), before, "nothing queued, echoed or armed");
    assert_eq!(
        e.cw_keyer_error(),
        None,
        "said once, in the strip's own line"
    );
    assert_eq!(e.poll_cw_one(), None, "nothing keys");
    // CONTROL: the section as it should be.
    let mut e = field_day_station("IL");
    assert_eq!(e.send_cw_armed("! {EXCH}"), Ok(()));
    assert_eq!(queued(&e), ["K9AAA", "3A", "IL"]);
}

/// THE RULE IS THE RADIO'S OWN KEYER'S, AND THE KEYER IS THE ONE THE RADIO LOOP HANDS THE WORDS TO.
/// A WinKeyer or a key line chosen with no port set keys on no keyer at all (#434): its message is
/// refused whole too, for the missing port. A WinKeyer, a key line or the soundcard
/// that will key the message puts no text inside a radio command: its message is queued exactly as
/// before, typed and by Enter alike.
#[test]
fn only_a_message_the_radios_own_keyer_will_key_is_refused_before_it_is_queued() {
    let text = format!("CQ TEST;{RETUNE} DE KD9TAW");
    let as_typed = ["CQ", &format!("TEST;{RETUNE}"), "DE", "KD9TAW"];
    for (keyer, winkeyer_port, key_port, refused) in [
        ("cat", "", "", Some(SEMICOLON)),
        ("winkeyer", "", "", Some(CW_NO_WINKEYER_PORT)),
        ("serial", "", "", Some(CW_NO_KEYLINE_PORT)),
        ("winkeyer", "/dev/ttyUSB0", "", None),
        ("serial", "", "/dev/ttyUSB1", None),
        ("soundcard", "", "", None),
    ] {
        let scene = format!("{keyer}, WinKeyer port {winkeyer_port:?}, key line {key_port:?}");
        let mut typed = station_on(keyer, winkeyer_port, key_port);
        typed.send_cw(&text);
        let mut enter = station_on(keyer, winkeyer_port, key_port);
        let took = enter.send_cw_armed(&text);
        if let Some(why) = refused {
            assert_eq!(
                (queued(&typed), typed.cw_keyer_error().as_deref()),
                (vec![], Some(why)),
                "{scene}: typed, refused whole"
            );
            assert_eq!(
                (took, queued(&enter)),
                (Err(why.to_string()), vec![]),
                "{scene}: Enter, refused"
            );
        } else {
            assert_eq!(
                (queued(&typed), typed.cw_keyer_error()),
                (as_typed.map(String::from).to_vec(), None),
                "{scene}: typed, queued as before"
            );
            assert_eq!(
                (took, queued(&enter)),
                (Ok(()), as_typed.map(String::from).to_vec()),
                "{scene}: Enter, queued as before"
            );
        }
    }
}
