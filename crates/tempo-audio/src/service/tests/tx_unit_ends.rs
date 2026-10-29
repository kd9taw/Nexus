//! The loop marks where each complete unit of transmit audio ends, and only there
//! (`AudioBackend::mark_unit_end`). The sound card counts a block that runs dry anywhere else as an
//! underrun, so a missing mark reads as an underrun at the end of every word or message, and a
//! mark on audio that is fed as it is made (Tune) would hide a stalled loop. Each test runs the
//! real `RadioLoop::step` and reads what the mock card was handed.

use super::cw_stop::{logging_rigctld, step};
use super::*;

/// A licensed engine, set up by `setup`.
fn engine(setup: impl FnOnce(&mut Engine)) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    {
        let mut e = engine.lock().unwrap();
        e.set_license_class("extra");
        setup(&mut e);
    }
    engine
}

/// Run the loop from 100 ms to `until_ms`, 20 ms a tick, against a rigctld that keys.
fn run(engine: &Arc<Mutex<Engine>>, until_ms: f64) -> MockBackend {
    let (addr, _log) = logging_rigctld(|_| None);
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(engine);
    let mut t = 100.0;
    while t <= until_ms {
        step(&mut state, engine, &mut backend, &mut rig, t);
        t += 20.0;
    }
    backend
}

/// ⭐ A SOUNDCARD CW MACRO ENDS A UNIT AT EVERY WORD. The keyer plays one word at a time and the
/// next only after the word space, so the card runs dry between words by design; each word is
/// marked where it ends, and the last mark is the end of everything played.
#[test]
fn a_soundcard_cw_macro_marks_the_end_of_every_word() {
    let engine = engine(|e| {
        e.set_cw_keyer("soundcard", 600.0);
        e.set_operating_mode("cw", false);
        e.set_frequency(7.03, "40m", "CW");
        e.set_cw_wpm(25);
        e.send_cw("CQ TEST DE");
    });
    let backend = run(&engine, 8_000.0);
    assert!(!backend.played.is_empty(), "premise: the macro was played");
    assert_eq!(
        backend.unit_ends.len(),
        3,
        "one mark per word: {:?}",
        backend.unit_ends
    );
    assert!(
        backend.unit_ends.windows(2).all(|w| w[0] < w[1]),
        "each word is a unit of its own: {:?}",
        backend.unit_ends
    );
    assert_eq!(backend.unit_ends.last(), Some(&backend.played.len()));
}

/// A queued PSK message is one unit, marked where it ends.
#[test]
fn a_queued_psk_message_marks_its_end() {
    let engine = engine(|e| {
        e.set_operating_mode("keyboard", false);
        e.psk_send_text("CQ TEST").unwrap();
    });
    let backend = run(&engine, 3_000.0);
    assert!(
        !backend.played.is_empty(),
        "premise: the message was played"
    );
    assert_eq!(backend.unit_ends, vec![backend.played.len()]);
}

/// ⭐ TUNE IS NEVER MARKED. Its carrier is topped up a little at a time for as long as the operator
/// holds it, so the card running dry under it is the loop falling behind, whatever the length of
/// the stall, and must be counted.
#[test]
fn a_tune_carrier_is_never_marked() {
    let engine = engine(|e| {
        e.set_operating_mode("phone", true);
        e.set_frequency(14.250, "20m", "USB");
        e.set_tune(true);
    });
    let backend = run(&engine, 1_000.0);
    assert!(
        backend.played.len() > 6_000,
        "premise: the carrier was topped up across the run ({} samples)",
        backend.played.len()
    );
    assert_eq!(backend.unit_ends, Vec::<usize>::new());
}
