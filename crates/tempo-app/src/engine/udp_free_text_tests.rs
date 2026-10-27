//! A logger's WSJT-X FreeText (JTAlert, GridTracker) goes into Tx 5, and with "send" out as Tx 5,
//! as WSJT-X 3.0.2 takes it (`widgets/mainwindow.cpp:692-709`): text, sent or not, is set into Tx 5
//! (`ui->tx5->setCurrentText`); an empty text without "send" clears it; "send" clicks Tx 5
//! (`on_txb5_clicked`, 8797-8803: the next over, and the rest of an over on the air,
//! `if(m_transmitting) m_restart=true`). Nexus kept a chat broadcast instead, which Operate's
//! sequencer never sent.
//!
//! What stays Nexus's own: the over goes out only while the operator has TX on, and this never arms
//! TX nor restarts the watchdog (`stop_hold_tests`); an over the encoder refuses is not queued; the
//! contest screen sends its own exchange; Chat sends a chat broadcast, on a chat tier only. Each
//! refusal is said with the text, which the cockpit shows.

use super::ft_refused_text_tests::{answering, next_over};
use super::*;
use crate::dto::{Tier, UdpFreeText};

/// Operate's FT8 or FT4 screen, listening, with TX on.
fn listening(tier: Tier) -> Engine {
    let mut e = Engine::new("K1ABC", "FN42", 0);
    e.set_tier(tier);
    e.set_mode("qso-monitor").expect("the FT screen");
    e.set_tx_enabled_by_operator(true);
    e
}

/// What Tx 5 took: `(seq, fill, refused)`.
fn tx5(e: &Engine) -> Option<(u32, Option<String>, Option<String>)> {
    e.snapshot()
        .udp_free_text
        .map(|UdpFreeText { seq, fill, refused }| (seq, fill, refused))
}

/// The "Now sending" readout.
fn tx_now(e: &Engine) -> Option<String> {
    e.snapshot().qso.and_then(|q| q.tx_now)
}

/// ⭐ "send": the text is in Tx 5 and is the next over, keyed as the text; an over on the air is
/// asked to switch to it. TX stays as the operator left it and the watchdog clock runs on.
#[test]
fn send_puts_the_text_in_tx5_and_makes_it_the_next_over() {
    for tier in [Tier::Ft8, Tier::Ft4] {
        let mut e = listening(tier);
        let started = now_unix_secs() - 60;
        e.tx_watchdog_start = Some(started);
        let _ = e.take_ft_restart();
        let r = e.udp_free_text(" tnx 73 ", true);
        assert_eq!(
            (r, tx5(&e), tx_now(&e), e.take_ft_restart()),
            (
                Ok(()),
                Some((1, Some("TNX 73".to_string()), None)),
                Some("TNX 73".to_string()),
                true
            ),
            "{tier:?} listening: (result, Tx 5, now sending, switch asked)"
        );
        assert_eq!(
            (e.tx_enabled(), e.tx_watchdog_start),
            (true, Some(started)),
            "{tier:?}: (TX on, the watchdog clock)"
        );
        assert_eq!(
            next_over(&mut e, 0),
            Some(("TNX 73".to_string(), true)),
            "{tier:?}: the over keyed"
        );

        // Working a station, a standard message goes out as the Tx 5 click sends it.
        let mut e = answering(tier, "CQ W9XYZ EN37");
        assert_eq!(
            (e.udp_free_text("W9XYZ K1ABC 73", true), tx_now(&e)),
            (Ok(()), Some("W9XYZ K1ABC 73".to_string())),
            "{tier:?} working W9XYZ: (result, now sending)"
        );
    }
}

/// Without "send" the text only fills Tx 5: nothing is queued and nothing switches. Each FreeText
/// is one more `seq`, so the cockpit takes each once.
#[test]
fn without_send_the_text_only_fills_tx5() {
    let mut e = listening(Tier::Ft8);
    let _ = e.take_ft_restart();
    assert_eq!(e.udp_free_text("TNX 73", false), Ok(()));
    assert_eq!(
        (tx5(&e), tx_now(&e), e.take_ft_restart()),
        (Some((1, Some("TNX 73".to_string()), None)), None, false),
        "(Tx 5, now sending, switch asked)"
    );
    assert_eq!(e.udp_free_text("GL", false), Ok(()));
    assert_eq!(tx5(&e), Some((2, Some("GL".to_string()), None)), "the next");
}

/// An empty text without "send" clears Tx 5. With "send" it asks for Tx 5 as it stands, which
/// Nexus does not know (the cockpit holds what was typed there): refused, Tx 5 left alone.
#[test]
fn an_empty_text_clears_tx5_and_sent_is_refused() {
    let mut e = listening(Tier::Ft8);
    assert_eq!(e.udp_free_text("", false), Ok(()));
    assert_eq!(tx5(&e), Some((1, Some(String::new()), None)), "cleared");
    let r = e.udp_free_text("  ", true);
    let (seq, fill, refused) = tx5(&e).expect("taken");
    assert_eq!(
        (r.is_err(), seq, fill, refused == r.err(), tx_now(&e)),
        (true, 2, None, true, None),
        "sent empty: (refused, seq, Tx 5, the reason shown, now sending)"
    );
}

/// With TX off the text still fills Tx 5, as in WSJT-X, and nothing is queued; the reason says so.
#[test]
fn with_tx_off_the_text_fills_tx5_and_nothing_is_queued() {
    let mut e = Engine::new("K1ABC", "FN42", 0);
    e.set_tier(Tier::Ft8);
    e.set_mode("qso-monitor").expect("the FT screen");
    let r = e.udp_free_text("TNX 73", true);
    assert_eq!(
        (r.clone(), tx5(&e), tx_now(&e), e.tx_enabled()),
        (
            Err(UDP_FREE_TEXT_TX_OFF.to_string()),
            Some((1, Some("TNX 73".to_string()), r.err())),
            None,
            false
        ),
        "(result, Tx 5, now sending, TX on)"
    );
}

/// ⛔ Text the encoder refuses fills Tx 5 and is not queued; the reason is the encoder's, as the
/// operator's own box says it. The QSO's next over stays, and nothing switches.
#[test]
fn text_the_encoder_refuses_fills_tx5_and_is_not_queued() {
    for (tier, cq, text) in [
        (Tier::Ft8, "CQ W9XYZ EN37", "W9XYZ K1ABC TNX 73"),
        (Tier::Ft4, "CQ W9XYZ EN37", "W9XYZ K1ABC TNX 73"),
        (Tier::Ft8, "CQ W9XYZ/M", "W9XYZ/M K1ABC TNX"),
    ] {
        let mut e = answering(tier, cq);
        let before = tx_now(&e);
        let _ = e.take_ft_restart();
        let r = e.udp_free_text(text, true);
        let why = e.ft_refusal(text);
        assert!(why.is_some(), "premise: {tier:?} refuses {text:?}");
        assert_eq!(
            (r, tx5(&e), tx_now(&e) == before, e.take_ft_restart()),
            (
                Err(why.clone().unwrap()),
                Some((1, Some(text.to_string()), why)),
                true,
                false
            ),
            "{tier:?} {text:?}: (result, Tx 5, next over unchanged, switch asked)"
        );
    }
}

/// The contest screen shows the text in Tx 5 and sends its own exchange: the run goes on.
#[test]
fn the_contest_screen_shows_the_text_and_sends_its_own_exchange() {
    let mut e = Engine::new("K1ABC", "FN42", 0);
    let mut s = e.settings().clone();
    s.fd_event = "arrlfd".into();
    s.fd_class = "2B".into();
    s.fd_section = "EMA".into();
    s.contest_qth_state = "MA".into();
    s.tx_even = true;
    s.fd_active = true;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.set_mode("fieldday-run").expect("premise: the run");
    let r = e.udp_free_text("TNX 73", true);
    let (_, fill, refused) = tx5(&e).expect("taken");
    assert_eq!(
        (r.is_err(), fill, refused == r.err()),
        (true, Some("TNX 73".to_string()), true),
        "(refused, Tx 5, the reason shown)"
    );
    assert!(
        matches!(e.mode, Mode::FieldDay { .. })
            && next_over(&mut e, 0).is_some_and(|(cq, keyed)| keyed && cq.starts_with("CQ ")),
        "still the contest, its CQ keyed"
    );
}

/// Chat has no Tx 5: on a chat tier the text goes out as a chat broadcast, as before, and on FT8
/// (whose packer cannot carry a chat chunk) it is refused and nothing is queued.
#[test]
fn chat_sends_a_chat_broadcast_on_a_chat_tier_only() {
    let mut chat = Engine::new("K1ABC", "FN42", 0);
    chat.set_mode("chat").expect("Chat");
    chat.set_tx_enabled_by_operator(true);
    assert_eq!(
        (
            chat.app.tier().is_chat(),
            chat.udp_free_text("TNX 73", true),
            chat.broadcast_queue.is_empty(),
            tx5(&chat)
        ),
        (true, Ok(()), false, None),
        "Chat: (a chat tier, result, queue empty, Tx 5)"
    );
    let mut ft8 = Engine::new("K1ABC", "FN42", 0);
    ft8.set_tier(Tier::Ft8);
    ft8.set_tx_enabled_by_operator(true);
    assert!(matches!(ft8.mode, Mode::Chat), "premise: Chat on FT8");
    assert_eq!(
        (
            ft8.udp_free_text("TNX 73", true).is_err(),
            ft8.broadcast_queue.is_empty()
        ),
        (true, true),
        "Chat on FT8: (refused, queue empty)"
    );
}

/// Text that is not plain printable ASCII, or longer than 128 characters, is refused whole.
#[test]
fn other_characters_or_too_long_a_text_are_refused_whole() {
    let mut e = listening(Tier::Ft8);
    for text in ["TNX \u{2713}", "TNX\t73", &"A".repeat(129)] {
        assert_eq!(
            (e.udp_free_text(text, true).is_err(), tx5(&e), tx_now(&e)),
            (true, None, None),
            "{text:?}: (refused, Tx 5, now sending)"
        );
    }
    assert_eq!(e.udp_free_text(&"A".repeat(128), false), Ok(()), "control");
}
