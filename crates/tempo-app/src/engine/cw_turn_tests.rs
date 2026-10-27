//! THE CW TURN CUE'S READ: whose turn it is, from the AI decoder's copy and our own keying
//! ([`Engine::cw_turn`], the model in `tempo_core::cw_turn`). Only while the AI decoder is on and
//! keeping up, and never in an unassisted entry, which gets nothing from a CW decoder. Our own
//! transmissions are marked on the decoder's clock, so the over after ours is read on its own;
//! a cleared screen, a logged contact and a QSY each forget the contact.

use super::*;
use tempo_core::cw_turn::{Role, State};

/// The CW screen on 20 m, TX armed, with the AI decoder on as it ships.
fn station() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_license_class("extra");
    e.set_operating_mode("cw", false);
    e.set_frequency(14.030, "20m", "CW");
    e
}

/// The AI-CW stream clock now (seconds of audio fed to the AI ring).
fn now(e: &Engine) -> f64 {
    e.ai_cw_fed as f64 / AI_CW_CLOCK_HZ
}

/// `secs` of received audio (silence: the copy below is the decoder's, pushed as a pass).
fn listen(e: &mut Engine, secs: f64) {
    let audio = vec![0.0f32; (secs * AI_CW_CLOCK_HZ) as usize];
    for chunk in audio.chunks(1200) {
        e.feed_rx_audio(chunk);
    }
}

/// One AI pass that heard `text` from stream time `from`, a letter every 0.15 s, and has
/// listened up to now.
fn copy(e: &mut Engine, text: &str, from: f64) {
    copy_every(e, text, from, 0.15);
}

fn copy_every(e: &mut Engine, text: &str, from: f64, step: f64) {
    let epoch = e.ai_cw_window().expect("the AI ring fills").2;
    let letters: Vec<(f64, char)> = text
        .chars()
        .enumerate()
        .map(|(i, c)| (from + i as f64 * step, c))
        .collect();
    e.push_ai_cw_pass(epoch, text, "");
    e.push_ai_cw_times(epoch, now(e), &letters, &[]);
}

/// An AI pass that heard nothing new.
fn pass(e: &mut Engine) {
    copy(e, "", 0.0);
}

/// W1ABC's CQ, heard and followed by the silence that ends it.
fn heard_a_cq(e: &mut Engine) {
    listen(e, 1.0);
    let from = now(e);
    listen(e, 4.2);
    copy(e, "CQ CQ DE W1ABC W1ABC K", from);
}

fn state(e: &Engine) -> Option<State> {
    e.cw_turn(Some("W1ABC")).map(|t| t.state)
}

#[test]
fn the_cue_reads_the_ai_decoders_copy_and_its_times() {
    let mut e = station();
    heard_a_cq(&mut e);
    let t = e
        .cw_turn(Some("W1ABC"))
        .expect("the AI decoder is on and keeping up");
    assert_eq!(
        (t.state, t.role, t.call.as_deref()),
        (State::YourTurn, Some(Role::Answer), Some("W1ABC"))
    );
}

#[test]
fn no_cue_without_the_ai_decoder() {
    let mut e = station();
    heard_a_cq(&mut e);
    assert!(state(&e).is_some(), "control: on, it reads");
    e.set_ai_cw_enabled(false);
    assert_eq!(state(&e), None);
    listen(&mut e, 1.0);
    assert_eq!(state(&e), None, "nor from audio heard with it off");
}

#[test]
fn no_cue_in_an_unassisted_entry_however_it_was_declared() {
    let mut e = station();
    heard_a_cq(&mut e);
    assert_eq!(state(&e), Some(State::YourTurn), "control: assisted");
    e.set_unassisted_mode(true);
    assert_eq!(state(&e), None, "declared by its switch");
    let mut e = station();
    heard_a_cq(&mut e);
    let mut s = e.settings().clone();
    s.unassisted_mode = true;
    e.apply_settings(s);
    assert_eq!(state(&e), None, "declared by a settings save");
}

#[test]
fn a_decoder_that_has_stopped_listening_shows_no_turn() {
    let mut e = station();
    heard_a_cq(&mut e);
    assert!(state(&e).is_some(), "control");
    // Eleven seconds of audio with no pass: the decoder has stopped or cannot keep up.
    listen(&mut e, 11.0);
    assert_eq!(state(&e), None);
    pass(&mut e);
    assert!(state(&e).is_some(), "a pass again, a read again");
}

#[test]
fn our_keying_is_marked_on_the_decoders_clock() {
    let mut e = station();
    listen(&mut e, 1.0);
    pass(&mut e);
    e.send_cw("CQ CQ DE {MYCALL} {MYCALL} K");
    listen(&mut e, 0.5);
    assert_eq!(state(&e), Some(State::YouSend));
    // The over ends: nothing queued, and the rig's last word is done.
    e.cw_queue.clear();
    e.observe_cw_keying(None);
    listen(&mut e, 1.0);
    pass(&mut e);
    let t = e.cw_turn(None).expect("a read");
    assert_eq!(t.state, State::Listen, "we listen for a reply to our CQ");
    assert!(
        t.window.is_some_and(|w| w.total == 8.0 && w.left < 8.0),
        "{t:?}"
    );
}

#[test]
fn our_own_sidetone_in_the_copy_is_not_their_over() {
    let mut e = station();
    listen(&mut e, 1.0);
    pass(&mut e);
    let start = now(&e);
    e.send_cw("W1ABC DE {MYCALL} KN");
    listen(&mut e, 2.0);
    e.cw_queue.clear();
    e.observe_cw_keying(None);
    listen(&mut e, 1.0);
    // The receiver heard our own over back: its 18 letters inside our two seconds of keying.
    copy_every(&mut e, "W1ABC DE KD9TAW KN", start + 0.15, 0.1);
    assert_eq!(state(&e), Some(State::Listen));
}

#[test]
fn the_next_stations_cq_after_our_73_is_an_answer_never_73() {
    let mut e = station();
    listen(&mut e, 1.0);
    let from = now(&e);
    listen(&mut e, 4.3);
    copy(&mut e, "KD9TAW DE W1ABC TU 73 SK", from);
    assert_eq!(state(&e), Some(State::Closing), "control: their closing");
    e.send_cw("! DE {MYCALL} TU 73 SK");
    listen(&mut e, 2.0);
    e.cw_queue.clear();
    e.observe_cw_keying(None);
    listen(&mut e, 2.0);
    let from = now(&e);
    listen(&mut e, 4.2);
    copy(&mut e, "CQ CQ DE K2XYZ K2XYZ K", from);
    let t = e.cw_turn(Some("K2XYZ")).expect("a read");
    assert_eq!(
        (t.state, t.role, t.call.as_deref()),
        (State::YourTurn, Some(Role::Answer), Some("K2XYZ"))
    );
}

#[test]
fn clear_a_logged_contact_or_a_qsy_forgets_the_contact() {
    let mut e = station();
    heard_a_cq(&mut e);
    assert_eq!(state(&e), Some(State::YourTurn), "control");
    e.cw_clear();
    assert_eq!(state(&e), Some(State::Idle), "Clear");
    heard_a_cq(&mut e);
    assert_eq!(state(&e), Some(State::YourTurn), "control");
    e.log_qso(contact("W1ABC"));
    assert_eq!(state(&e), Some(State::Idle), "a logged contact");
    heard_a_cq(&mut e);
    assert_eq!(state(&e), Some(State::YourTurn), "control");
    e.set_frequency(7.030, "40m", "CW");
    assert_eq!(
        state(&e),
        None,
        "a QSY: a new stream, nothing heard on it yet"
    );
}

fn contact(call: &str) -> QsoRecord {
    let adif = format!(
        "<CALL:{}>{call}<BAND:3>20m<MODE:2>CW<FREQ:6>14.030<QSO_DATE:8>20261021\
         <TIME_ON:6>120000<RST_SENT:3>599<RST_RCVD:3>599<EOR>",
        call.len()
    );
    tempo_core::logbook::parse_adif(&adif)
        .pop()
        .expect("one record")
}
