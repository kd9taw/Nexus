//! A key the radio refuses sends nothing: every over the radio loop keys for audio (or a keyline)
//! that Nexus sends. The tune, APRS, SSTV, the voice keyer, the soundcard CW keyer, RTTY, PSK and
//! the Remote microphone each played their over whatever the key's answer, so a refused key put
//! modem audio into a receiving radio (or sent nothing, said nothing, and lost the frame). Each
//! over is now not played, it is dropped rather than held, and the operator is told, in the
//! words the voice keyer gives a refused key or on the over's own warning line
//! (`RadioLoop::key_over`).
//!
//! The rig is a rigctld that refuses the key (`T 1`, and `T 3`, the DATA key) with `RPRT -1`; the
//! loop is the real `RadioLoop::step`. Each test has its control in the same scene: the same over
//! on a rigctld that accepts the key plays, as it always did.

use super::*;

/// What the rigctld answers to the key.
#[derive(Clone, Copy, PartialEq)]
enum Key {
    /// `RPRT 0`, always.
    Accepts,
    /// `RPRT -1`, always.
    Refuses,
    /// `RPRT -1` while a key of its own is still held (a key not yet followed by `T 0`), as Nexus's
    /// Flex client refuses a second key while one of ours is held (`AlreadyKeyed`).
    RefusesASecond,
}

/// A rigctld answering the key as `key`, `f` with a 20 m dial and everything else `RPRT 0`,
/// logging every line it was sent.
fn rigctld(key: Key) -> (String, Arc<Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let log2 = Arc::clone(&log);
    std::thread::spawn(move || {
        let mut held = false;
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(match stream.try_clone() {
                Ok(r) => r,
                Err(_) => continue,
            });
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let l = line.trim().to_string();
                log2.lock().unwrap().push(l.clone());
                let refused = match l.as_str() {
                    "T 1" | "T 3" => {
                        let refused = match key {
                            Key::Accepts => false,
                            Key::Refuses => true,
                            Key::RefusesASecond => held,
                        };
                        held |= !refused;
                        refused
                    }
                    "T 0" => {
                        held = false;
                        false
                    }
                    _ => false,
                };
                let reply = if l == "f" {
                    "14250000\n"
                } else if refused {
                    "RPRT -1\n"
                } else {
                    "RPRT 0\n"
                };
                if stream.write_all(reply.as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    (addr, log)
}

/// The radio loop on that rigctld, for an engine `setup` armed and gave an over to send.
struct Scene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    log: Arc<Mutex<Vec<String>>>,
    t: f64,
}

impl Scene {
    fn new(key: Key, setup: impl FnOnce(&mut Engine)) -> Scene {
        let (addr, log) = rigctld(key);
        let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
        {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            setup(&mut e);
        }
        Scene {
            engine,
            state: loop_state(),
            backend: MockBackend::new(),
            rig: Rig::rigctld(&addr),
            log,
            t: 100.0,
        }
    }

    /// Run the loop, a 20 ms tick at a time, to `until` ms.
    fn run_to(&mut self, until: f64) {
        step_to(
            &self.engine,
            &mut self.state,
            &mut self.backend,
            &mut self.rig,
            &mut self.t,
            until,
        );
    }

    /// The keys the rigctld was sent.
    fn keys(&self) -> usize {
        let log = self.log.lock().unwrap();
        log.iter().filter(|l| *l == "T 1" || *l == "T 3").count()
    }

    /// The line the voice keyer gives a refused key (the shared audio-error banner).
    fn banner(&self) -> Option<String> {
        self.engine.lock().unwrap().snapshot().radio.audio_error
    }
}

/// The voice keyer's words for a refused key.
const REFUSED: &str = "The rig didn't accept PTT — check your PTT method and CAT/port.";

fn phone(e: &mut Engine) {
    e.set_operating_mode("phone", true);
    e.set_frequency(14.250, "20m", "USB");
}

/// ⭐ A TUNE THE RADIO REFUSES PUTS NO CARRIER INTO IT. The tune keyed and played its carrier
/// without looking at the key's answer, and said nothing. Now the carrier never plays, the tune
/// ends (the operator's Tune comes back up), the release unkeys as for any tune, and the line says
/// the rig did not accept PTT.
#[test]
fn a_refused_tune_plays_no_carrier_and_ends() {
    let mut s = Scene::new(Key::Refuses, |e| {
        phone(e);
        e.set_tune(true);
    });
    s.run_to(300.0);
    assert!(s.keys() >= 1, "premise: the tune tried the key");
    assert!(s.backend.played.is_empty(), "a carrier went into the rig");
    assert!(!s.engine.lock().unwrap().tuning(), "the tune is still on");
    assert_eq!(s.banner().as_deref(), Some(REFUSED));
    let log = s.log.lock().unwrap().clone();
    let key = log.iter().position(|l| l == "T 1").unwrap();
    assert!(
        log[key..].iter().any(|l| l == "T 0"),
        "the release did not unkey: {log:?}"
    );

    // The control: a rig that keys gets the carrier, and the tune stays on.
    let mut s = Scene::new(Key::Accepts, |e| {
        phone(e);
        e.set_tune(true);
    });
    s.run_to(300.0);
    assert!(!s.backend.played.is_empty(), "control: no carrier");
    assert!(s.engine.lock().unwrap().tuning(), "control: the tune ended");
    assert_eq!(s.banner(), None);
}

/// ⭐ AN APRS FRAME THE RADIO REFUSES IS NOT PLAYED. It was played into the receiving radio and
/// lost. The APRS status line says the key was refused, and the frame is not sent later.
#[test]
fn a_refused_aprs_frame_is_not_played() {
    let beacon = |e: &mut Engine| {
        phone(e);
        e.aprs_beacon(41.88, -87.63, '/', '>', "", &[])
            .expect("the beacon is queued");
    };
    let mut s = Scene::new(Key::Refuses, beacon);
    s.run_to(3_000.0);
    assert_eq!(s.keys(), 1, "premise: the beacon tried the key, once");
    assert!(s.backend.played.is_empty(), "the frame was played");
    let notice = s
        .engine
        .lock()
        .unwrap()
        .aprs_tx_notice()
        .map(str::to_string);
    assert!(
        notice
            .as_deref()
            .is_some_and(|n| n.starts_with("APRS not sent: the radio did not accept the key")),
        "{notice:?}"
    );

    let mut s = Scene::new(Key::Accepts, beacon);
    s.run_to(3_000.0);
    assert!(
        !s.backend.played.is_empty(),
        "control: the frame was not played"
    );
    assert_eq!(s.engine.lock().unwrap().aprs_tx_notice(), None);
}

/// ⭐ A VOICE MESSAGE THE RADIO REFUSES IS NOT PLAYED. The keyer said the rig did not accept PTT
/// and played the message into the receiving radio anyway.
#[test]
fn a_refused_voice_message_is_not_played() {
    let send = |e: &mut Engine| {
        phone(e);
        e.send_voice(vec![0.05; 12_000]).unwrap();
    };
    let mut s = Scene::new(Key::Refuses, send);
    s.run_to(2_000.0);
    assert!(s.keys() >= 1, "premise: the keyer tried the key");
    assert!(s.backend.played.is_empty(), "the message was played");
    assert_eq!(s.banner().as_deref(), Some(REFUSED));

    let mut s = Scene::new(Key::Accepts, send);
    s.run_to(2_000.0);
    assert_eq!(s.backend.played.len(), 12_000, "control: the message");
    assert_eq!(s.banner(), None);
}

/// ⭐ AN SSTV PICTURE THE RADIO REFUSES IS NOT SENT. It was fed into the receiving radio for its
/// whole length.
#[test]
fn a_refused_sstv_picture_is_not_sent() {
    let send = |e: &mut Engine| {
        phone(e);
        e.sstv_send(vec![0.05; 12_000], "Scottie 1".to_string())
            .unwrap();
    };
    let mut s = Scene::new(Key::Refuses, send);
    s.run_to(2_000.0);
    assert!(s.keys() >= 1, "premise: the picture tried the key");
    assert!(s.backend.played.is_empty(), "the picture was sent");
    assert!(!s.engine.lock().unwrap().sstv_sending(), "still sending");
    assert_eq!(s.banner().as_deref(), Some(REFUSED));

    let mut s = Scene::new(Key::Accepts, send);
    s.run_to(2_000.0);
    assert!(!s.backend.played.is_empty(), "control: no picture");
}

fn cw(e: &mut Engine) {
    e.set_cw_keyer("soundcard", 600.0);
    e.set_operating_mode("cw", false);
    e.set_frequency(14.03, "20m", "CW");
}

/// ⭐ A CW SEND THE RADIO REFUSES IS NOT PLAYED, AND DOES NOT RESUME. The soundcard keyer played
/// each word whatever the key's answer. Now the refused word is not played, and the rest of the
/// send goes with it, so a later word the radio does key cannot put half a message on the air.
#[test]
fn a_refused_cw_send_is_not_played_and_goes() {
    let send = |e: &mut Engine| {
        cw(e);
        e.send_cw("CQ TEST DE KD9TAW");
    };
    let mut s = Scene::new(Key::Refuses, send);
    s.run_to(6_000.0);
    assert_eq!(s.keys(), 1, "a word after the refused one tried the key");
    assert!(s.backend.played.is_empty(), "a word was played");
    let error = s.engine.lock().unwrap().cw_keyer_error();
    assert!(
        error
            .as_deref()
            .is_some_and(|e| e.starts_with("Soundcard keyer: the rig didn't accept PTT")),
        "{error:?}"
    );

    let mut s = Scene::new(Key::Accepts, send);
    s.run_to(6_000.0);
    assert!(s.keys() >= 4, "control: a key per word");
    assert!(!s.backend.played.is_empty(), "control: nothing played");
    assert_eq!(s.engine.lock().unwrap().cw_keyer_error(), None);
}

/// The control for the rule's one exception: a radio that refuses a SECOND key while the first is
/// held (Nexus's Flex client does) is still keyed by the first, so the soundcard keyer's later
/// words go out on it, as they always did. Every word of the macro plays.
#[test]
fn a_cw_macro_plays_whole_on_a_radio_that_refuses_a_second_key_while_keyed() {
    let send = |e: &mut Engine| {
        cw(e);
        e.send_cw("CQ CQ CQ");
    };
    let mut whole = Scene::new(Key::Accepts, send);
    whole.run_to(6_000.0);
    let mut s = Scene::new(Key::RefusesASecond, send);
    s.run_to(6_000.0);
    assert_eq!(
        s.backend.played.len(),
        whole.backend.played.len(),
        "a word keyed under the first key's hold was not played"
    );
    assert_eq!(
        s.keys(),
        3,
        "the macro stopped keying at a refused second key"
    );
}

/// ⭐ AN RTTY OVER THE RADIO REFUSES IS NOT PLAYED, AND NOT ECHOED AS SENT. The AFSK keyer played
/// the message and the transcript echoed it as sent.
#[test]
fn a_refused_rtty_over_is_not_played_or_echoed() {
    let send = |e: &mut Engine| {
        e.set_operating_mode("rtty", false);
        e.rtty_send_text("CQ TEST").unwrap();
    };
    let mut s = Scene::new(Key::Refuses, send);
    s.run_to(3_000.0);
    assert!(s.keys() >= 1, "premise: the over tried the key");
    assert!(s.backend.played.is_empty(), "the over was played");
    let st = s.engine.lock().unwrap().rtty_state();
    assert!(
        st.keyer_error
            .as_deref()
            .is_some_and(|e| e.starts_with("AFSK keyer: the rig didn't accept PTT")),
        "{:?}",
        st.keyer_error
    );
    assert!(!st.sending, "the cockpit reads an over on the air");
    assert_eq!(
        rtty_sent(&s.engine),
        "",
        "an over that never keyed was echoed"
    );

    let mut s = Scene::new(Key::Accepts, send);
    s.run_to(3_000.0);
    assert!(!s.backend.played.is_empty(), "control: nothing played");
    assert_eq!(rtty_sent(&s.engine), "CQ TEST", "control: no echo");
}

/// ⭐ CONTINUOUS RTTY THE RADIO REFUSES DROPS ITS LATCH. The stream played its idle into the
/// receiving radio for as long as the latch stayed up.
#[test]
fn a_refused_rtty_stream_drops_its_latch() {
    let latch = |e: &mut Engine| {
        e.set_operating_mode("rtty", false);
        e.set_rtty_latched(true).unwrap();
    };
    let mut s = Scene::new(Key::Refuses, latch);
    s.run_to(1_000.0);
    assert!(s.keys() >= 1, "premise: the stream tried the key");
    assert!(s.backend.played.is_empty(), "the stream was played");
    assert!(!s.engine.lock().unwrap().rtty_state().latched, "latched");

    let mut s = Scene::new(Key::Accepts, latch);
    s.run_to(1_000.0);
    assert!(!s.backend.played.is_empty(), "control: nothing played");
    assert!(s.engine.lock().unwrap().rtty_state().latched, "control");
}

/// ⭐ A PSK OVER THE RADIO REFUSES IS NOT PLAYED.
#[test]
fn a_refused_psk_over_is_not_played() {
    let send = |e: &mut Engine| {
        e.set_operating_mode("keyboard", false);
        e.psk_send_text("CQ TEST").unwrap();
    };
    let mut s = Scene::new(Key::Refuses, send);
    s.run_to(3_000.0);
    assert!(s.keys() >= 1, "premise: the over tried the key");
    assert!(s.backend.played.is_empty(), "the over was played");
    let st = s.engine.lock().unwrap().psk_state();
    assert!(
        st.keyer_error
            .as_deref()
            .is_some_and(|e| e.starts_with("PSK keyer: the rig didn't accept PTT")),
        "{:?}",
        st.keyer_error
    );
    assert!(!st.sending, "the cockpit reads an over on the air");

    let mut s = Scene::new(Key::Accepts, send);
    s.run_to(3_000.0);
    assert!(!s.backend.played.is_empty(), "control: nothing played");
}

/// ⭐ CONTINUOUS PSK THE RADIO REFUSES DROPS ITS LATCH.
#[test]
fn a_refused_psk_stream_drops_its_latch() {
    let latch = |e: &mut Engine| {
        e.set_operating_mode("keyboard", false);
        e.set_psk_latched(true).unwrap();
    };
    let mut s = Scene::new(Key::Refuses, latch);
    s.run_to(1_000.0);
    assert!(s.keys() >= 1, "premise: the stream tried the key");
    assert!(s.backend.played.is_empty(), "the stream was played");
    assert!(!s.engine.lock().unwrap().psk_state().latched, "latched");

    let mut s = Scene::new(Key::Accepts, latch);
    s.run_to(1_000.0);
    assert!(!s.backend.played.is_empty(), "control: nothing played");
    assert!(s.engine.lock().unwrap().psk_state().latched, "control");
}

/// ⭐ THE REMOTE MICROPHONE'S VOICE IS NOT PLAYED INTO A RADIO THAT REFUSED THE KEY. Its first
/// frame keys the rig; the loop said the rig did not accept PTT and played the operator's voice
/// anyway. The over ends instead.
#[test]
fn a_refused_remote_microphone_over_plays_nothing_and_ends() {
    for key in [Key::Refuses, Key::Accepts] {
        let (addr, log) = rigctld(key);
        let mut s = MicScene::new();
        s.rig = Rig::rigctld(&addr);
        s.hold.hold(MIC_PRESS, s.mono(0.0));
        s.step(0.0);
        let mut t = 20.0;
        while t <= 1_000.0 {
            s.tick(t, true);
            t += 20.0;
        }
        let banner = s.engine.lock().unwrap().snapshot().radio.audio_error;
        let keys = log.lock().unwrap().iter().filter(|l| *l == "T 1").count();
        assert!(keys >= 1, "premise: the first frame tried the key");
        if key == Key::Refuses {
            assert!(s.backend.played.is_empty(), "the voice was played");
            assert!(
                !s.engine.lock().unwrap().mic_armed(),
                "the over did not end"
            );
            assert_eq!(banner.as_deref(), Some(REFUSED));
        } else {
            assert!(!s.backend.played.is_empty(), "control: no voice");
            assert_eq!(banner, None);
        }
    }
}
