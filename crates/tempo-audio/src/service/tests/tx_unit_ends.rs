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
    let mut l = Loop::new(engine, None);
    l.run_to(until_ms);
    l.backend
}

/// The radio loop on a rigctld that keys, a 20 ms tick at a time from 100 ms.
struct Loop {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    t: f64,
}

impl Loop {
    /// `radio`: the dial (Hz) and mode the rigctld reads back; `None` for 40 m CW.
    fn new(engine: &Arc<Mutex<Engine>>, radio: Option<(u64, &'static str)>) -> Loop {
        let (addr, _log) = logging_rigctld(move |l| match (l, radio) {
            ("f", Some((hz, _))) => Some(format!("{hz}\n")),
            ("m", Some((_, mode))) => Some(format!("{mode}\n3000\n")),
            _ => None,
        });
        Loop {
            engine: Arc::clone(engine),
            state: loop_state_for(engine),
            backend: MockBackend::new(),
            rig: Rig::rigctld(&addr),
            t: 100.0,
        }
    }

    fn run_to(&mut self, until_ms: f64) {
        while self.t <= until_ms {
            step(
                &mut self.state,
                &self.engine,
                &mut self.backend,
                &mut self.rig,
                self.t,
            );
            self.t += 20.0;
        }
    }
}

/// The 20 m phone dial, read back from the radio.
const PHONE: Option<(u64, &str)> = Some((14_250_000, "USB"));

fn phone(e: &mut Engine) {
    e.set_operating_mode("phone", true);
    e.set_frequency(14.250, "20m", "USB");
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

/// ⭐ AN SSTV PICTURE IS ONE UNIT, MARKED ONCE, AT ITS END. It is fed a chunk at a time ahead of
/// playback, so the card running dry between chunks is the feed falling behind and must count:
/// only the last chunk is marked.
#[test]
fn an_sstv_picture_marks_only_its_end() {
    let picture = vec![0.05f32; 12_000 * 15];
    let engine = engine(|e| {
        phone(e);
        e.sstv_send(picture.clone(), "Scottie 1".to_string())
            .unwrap();
    });
    let mut l = Loop::new(&engine, PHONE);
    l.run_to(1_000.0);
    assert!(
        !l.backend.played.is_empty() && l.backend.played.len() < picture.len(),
        "premise: the picture is fed in chunks ({} of {})",
        l.backend.played.len(),
        picture.len()
    );
    assert_eq!(
        l.backend.unit_ends,
        Vec::<usize>::new(),
        "marked mid-picture"
    );
    l.run_to(17_000.0);
    assert_eq!(l.backend.played.len(), picture.len(), "premise: all sent");
    assert_eq!(l.backend.unit_ends, vec![picture.len()]);
}

/// A voice-keyer message is one unit, marked where it ends.
#[test]
fn a_voice_keyer_message_marks_its_end() {
    let engine = engine(|e| {
        phone(e);
        e.send_voice(vec![0.05; 12_000]).unwrap();
    });
    let mut l = Loop::new(&engine, PHONE);
    l.run_to(2_000.0);
    assert_eq!(
        l.backend.played.len(),
        12_000,
        "premise: the message played"
    );
    assert_eq!(l.backend.unit_ends, vec![12_000]);
}

/// An APRS frame is one unit, marked where it ends.
#[test]
fn an_aprs_frame_marks_its_end() {
    let engine = engine(|e| {
        phone(e);
        e.aprs_beacon(41.88, -87.63, '/', '>', "", &[])
            .expect("the beacon is queued");
    });
    let mut l = Loop::new(&engine, PHONE);
    l.run_to(3_000.0);
    assert!(!l.backend.played.is_empty(), "premise: the frame played");
    assert_eq!(l.backend.unit_ends, vec![l.backend.played.len()]);
}

/// An RTTY send is one unit, marked where it ends.
#[test]
fn an_rtty_send_marks_its_end() {
    let engine = engine(|e| {
        e.set_operating_mode("rtty", false);
        e.rtty_send_text("CQ TEST").unwrap();
    });
    let backend = run(&engine, 3_000.0);
    assert!(!backend.played.is_empty(), "premise: the over played");
    assert_eq!(backend.unit_ends, vec![backend.played.len()]);
}

/// ⭐ A KEYBOARD STREAM IS MARKED ONLY AT ITS CLOSE. RTTY and PSK continuous TX are fed a
/// character at a time while the operator types, so none of that is marked; when the operator
/// lets go and what was typed has gone out, the closing ramp ends the transmission and is.
#[test]
fn a_keyboard_stream_marks_only_its_close() {
    type Latch = fn(&mut Engine, bool) -> Result<(), String>;
    type Type = fn(&mut Engine, &str) -> Result<(), String>;
    let modes: [(&str, Latch, Type); 2] = [
        ("rtty", Engine::set_rtty_latched, Engine::rtty_type),
        ("keyboard", Engine::set_psk_latched, Engine::psk_type),
    ];
    for (mode, latch, type_text) in modes {
        let engine = engine(|e| {
            e.set_operating_mode(mode, false);
            latch(e, true).unwrap();
            type_text(e, "CQ CQ").unwrap();
        });
        let mut l = Loop::new(&engine, None);
        l.run_to(1_500.0);
        let streamed = l.backend.played.len();
        assert!(streamed > 0, "{mode}: premise: the stream is on the air");
        assert_eq!(
            l.backend.unit_ends,
            Vec::<usize>::new(),
            "{mode}: mid-stream"
        );
        latch(&mut engine.lock().unwrap(), false).unwrap();
        l.run_to(6_000.0);
        assert!(
            l.backend.played.len() > streamed,
            "{mode}: premise: the stream closed on the air"
        );
        assert_eq!(
            l.backend.unit_ends,
            vec![l.backend.played.len()],
            "{mode}: the close"
        );
    }
}

/// An over started late, inside the station's own slot (a Call CQ or a double-click), is one unit,
/// marked where it ends, as the boundary's overs are: FT8 on WSJT-X's slot clock, and FT2 played
/// from the key with any trailing pad stripped.
#[test]
fn a_late_start_over_marks_its_end() {
    for tier in [Tier::Ft8, Tier::Ft2] {
        let engine = engine(|e| e.set_tier(tier));
        let mut l = Loop::new(&engine, Some((14_074_000, "PKTUSB")));
        l.run_to(100.0);
        assert_eq!(
            l.state.last_slot,
            Some(0),
            "{tier:?}: scene guard: slot 0's boundary is consumed"
        );
        assert!(
            l.backend.played.is_empty(),
            "{tier:?}: scene guard: nothing keyed yet"
        );
        {
            let mut e = engine.lock().unwrap();
            e.set_frequency(14.074, "20m", "USB");
            e.broadcast("CQ KD9TAW EN52");
        }
        l.run_to(200.0);
        assert!(
            !l.backend.played.is_empty(),
            "{tier:?}: premise: the late start played"
        );
        assert_eq!(
            l.backend.unit_ends,
            vec![l.backend.played.len()],
            "{tier:?}"
        );
    }
}
