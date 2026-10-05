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
//!
//! A late answer is not a refusal. A slow radio reached through rigctld (a Xiegu, a vintage
//! Kenwood, any rig at 19200 baud or less) can key and answer after the loop's PTT deadline; its
//! over went out then, with the warning, and still does (`a_late_answer_*`). Nor is Hamlib's own
//! "the rig did not answer" (`RPRT -5`, and its I/O and bus kin): rigctld sent the key on and heard
//! nothing back, so the radio may be keying (`hamlibs_no_answer_*`). A rig that rejects the key
//! (`RPRT -9`) is refusing it.
//!
//! The slot overs (FT8, FT4, JS8 and the other timed-slot modes) key in `crate::slot` and follow
//! WSJT-X instead: a refusal, and Hamlib's own "the rig did not answer" with it, plays nothing and
//! halts TX, and only Nexus's own deadline passing still plays (the `*_ft8_*` tests at the end).

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
    /// `RPRT 0`, after [`LATE`]: a slow radio that keyed, answering past the PTT deadline.
    AnswersLate,
    /// `RPRT <code>`, always: the code Hamlib gives for the radio behind rigctld.
    Answers(i32),
}

/// Hamlib's own codes for "the rig did not answer" (`rig::rprt_is_link_fault`): a timeout, an I/O
/// error, a bus error, a busy bus.
const NO_ANSWER: [i32; 4] = [-5, -6, -13, -14];

/// How long [`Key::AnswersLate`] takes to answer the key. The PTT deadline is 700 ms, but the
/// rig reads in 500 ms windows and looks at the deadline only between them, so an answer inside
/// the first second is still taken; this one comes well after that.
const LATE: Duration = Duration::from_millis(1_500);

/// A rigctld answering the key as `key`, `f` with a 20 m dial and everything else `RPRT 0`,
/// logging every line it was sent.
fn rigctld(key: Key) -> (String, Arc<Mutex<Vec<String>>>) {
    rigctld_on(key, 14_250_000)
}

/// [`rigctld`], its dial at `dial_hz`.
fn rigctld_on(key: Key, dial_hz: u64) -> (String, Arc<Mutex<Vec<String>>>) {
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
                // The Hamlib code the line is answered with.
                let code = match l.as_str() {
                    "T 1" | "T 3" => {
                        if key == Key::AnswersLate {
                            std::thread::sleep(LATE);
                        }
                        let code = match key {
                            Key::Accepts | Key::AnswersLate => 0,
                            Key::Refuses => -1,
                            Key::RefusesASecond if held => -1,
                            Key::RefusesASecond => 0,
                            Key::Answers(code) => code,
                        };
                        held |= code == 0;
                        code
                    }
                    "T 0" => {
                        held = false;
                        0
                    }
                    _ => 0,
                };
                let reply = if l == "f" {
                    format!("{dial_hz}\n")
                } else {
                    format!("RPRT {code}\n")
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
        Scene::on(key, 14_250_000, setup)
    }

    /// [`Scene::new`], the rigctld's dial at `dial_hz`.
    fn on(key: Key, dial_hz: u64, setup: impl FnOnce(&mut Engine)) -> Scene {
        let (addr, log) = rigctld_on(key, dial_hz);
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

/// ⭐ A LATE ANSWER TO THE TUNE'S KEY IS NOT A REFUSAL. A slow radio keyed and answered after the
/// PTT deadline, and the tune put its carrier into it, as it always did. Only a radio that answers
/// no drops the tune; this one keeps it, and the line says the key was not confirmed.
#[test]
fn a_late_answer_to_the_tune_key_still_plays_the_carrier() {
    let mut s = Scene::new(Key::AnswersLate, |e| {
        phone(e);
        e.set_tune(true);
    });
    s.run_to(300.0);
    assert!(s.keys() >= 1, "premise: the tune tried the key");
    // The words of a key that did not come back accepted: the premise that this one was late.
    assert_eq!(s.banner().as_deref(), Some(REFUSED));
    assert!(!s.backend.played.is_empty(), "the carrier was dropped");
    assert!(s.engine.lock().unwrap().tuning(), "the tune ended");
}

/// ⭐ A VOICE MESSAGE WHOSE KEY IS ANSWERED LATE IS PLAYED, WHOLE, WITH THE WARNING.
#[test]
fn a_late_answer_to_the_voice_keyers_key_still_plays_the_message() {
    let mut s = Scene::new(Key::AnswersLate, |e| {
        phone(e);
        e.send_voice(vec![0.05; 12_000]).unwrap();
    });
    s.run_to(2_000.0);
    assert!(s.keys() >= 1, "premise: the keyer tried the key");
    assert_eq!(s.banner().as_deref(), Some(REFUSED));
    assert_eq!(s.backend.played.len(), 12_000, "the message was not played");
}

/// ⭐ AN SSTV PICTURE WHOSE KEY IS ANSWERED LATE IS SENT.
#[test]
fn a_late_answer_to_the_sstv_key_still_sends_the_picture() {
    let mut s = Scene::new(Key::AnswersLate, |e| {
        phone(e);
        e.sstv_send(vec![0.05; 12_000], "Scottie 1".to_string())
            .unwrap();
    });
    s.run_to(2_000.0);
    assert!(s.keys() >= 1, "premise: the picture tried the key");
    assert_eq!(s.banner().as_deref(), Some(REFUSED));
    assert!(!s.backend.played.is_empty(), "the picture was not sent");
}

/// ⭐ AN RTTY OVER WHOSE KEY IS ANSWERED LATE IS PLAYED AND ECHOED AS SENT, with the keyer's
/// warning.
#[test]
fn a_late_answer_to_the_rtty_key_still_plays_and_echoes_the_over() {
    let mut s = Scene::new(Key::AnswersLate, |e| {
        e.set_operating_mode("rtty", false);
        e.rtty_send_text("CQ TEST").unwrap();
    });
    s.run_to(3_000.0);
    assert!(s.keys() >= 1, "premise: the over tried the key");
    let st = s.engine.lock().unwrap().rtty_state();
    assert!(
        st.keyer_error
            .as_deref()
            .is_some_and(|e| e.starts_with("AFSK keyer: the rig didn't accept PTT")),
        "{:?}",
        st.keyer_error
    );
    assert!(!s.backend.played.is_empty(), "the over was not played");
    assert_eq!(rtty_sent(&s.engine), "CQ TEST", "the over was not echoed");
}

/// ⭐ A SOUNDCARD CW SEND WHOSE KEYS ARE ANSWERED LATE PLAYS WHOLE. A refused word drops the rest of
/// the send; a late one must not, or every macro on a slow radio would stop at its first word.
#[test]
fn a_late_answer_to_the_cw_keyers_key_still_plays_the_whole_send() {
    let send = |e: &mut Engine| {
        cw(e);
        e.send_cw("CQ TEST");
    };
    let mut whole = Scene::new(Key::Accepts, send);
    whole.run_to(6_000.0);
    let mut s = Scene::new(Key::AnswersLate, send);
    s.run_to(6_000.0);
    assert!(s.keys() >= 1, "premise: the keyer tried the key");
    let error = s.engine.lock().unwrap().cw_keyer_error();
    assert!(
        error
            .as_deref()
            .is_some_and(|e| e.starts_with("Soundcard keyer: the rig didn't accept PTT")),
        "{error:?}"
    );
    assert!(!whole.backend.played.is_empty(), "control: nothing played");
    assert_eq!(
        s.backend.played.len(),
        whole.backend.played.len(),
        "the send was cut"
    );
}

/// ⭐ AN APRS FRAME WHOSE KEY IS ANSWERED LATE IS PLAYED, and the APRS line does not say it was not
/// sent.
#[test]
fn a_late_answer_to_the_aprs_key_still_plays_the_frame() {
    let mut s = Scene::new(Key::AnswersLate, |e| {
        phone(e);
        e.aprs_beacon(41.88, -87.63, '/', '>', "", &[])
            .expect("the beacon is queued");
    });
    s.run_to(3_000.0);
    assert_eq!(s.keys(), 1, "premise: the beacon tried the key, once");
    assert!(!s.backend.played.is_empty(), "the frame was not played");
    assert_eq!(s.engine.lock().unwrap().aprs_tx_notice(), None);
}

/// ⭐ HAMLIB'S OWN "THE RIG DID NOT ANSWER" TO THE TUNE'S KEY IS NOT A REFUSAL. A radio too slow
/// for Hamlib makes rigctld answer the key with its timeout (`RPRT -5`), as Nexus's own deadline
/// would; the tune keeps its carrier, with the warning, as it always did. A rejection (`RPRT -9`)
/// still drops it.
#[test]
fn hamlibs_no_answer_to_the_tune_key_still_plays_the_carrier() {
    let tune = |e: &mut Engine| {
        phone(e);
        e.set_tune(true);
    };
    for code in NO_ANSWER {
        let mut s = Scene::new(Key::Answers(code), tune);
        s.run_to(300.0);
        assert!(
            s.keys() >= 1,
            "premise: RPRT {code}: the tune tried the key"
        );
        assert_eq!(s.banner().as_deref(), Some(REFUSED), "RPRT {code}");
        assert!(
            !s.backend.played.is_empty(),
            "RPRT {code}: the carrier was dropped"
        );
        assert!(
            s.engine.lock().unwrap().tuning(),
            "RPRT {code}: the tune ended"
        );
    }

    let mut s = Scene::new(Key::Answers(-9), tune);
    s.run_to(300.0);
    assert!(s.keys() >= 1, "premise: RPRT -9: the tune tried the key");
    assert_eq!(s.banner().as_deref(), Some(REFUSED), "RPRT -9");
    assert!(
        s.backend.played.is_empty(),
        "RPRT -9: a carrier went into the rig"
    );
    assert!(
        !s.engine.lock().unwrap().tuning(),
        "RPRT -9: the tune is still on"
    );
}

/// ⭐ A VOICE MESSAGE WHOSE KEY HAMLIB COULD NOT GET ANSWERED IS PLAYED, WHOLE, WITH THE WARNING. A
/// rejection (`RPRT -9`) still plays nothing.
#[test]
fn hamlibs_no_answer_to_the_voice_keyers_key_still_plays_the_message() {
    let send = |e: &mut Engine| {
        phone(e);
        e.send_voice(vec![0.05; 12_000]).unwrap();
    };
    for code in NO_ANSWER {
        let mut s = Scene::new(Key::Answers(code), send);
        s.run_to(2_000.0);
        assert!(
            s.keys() >= 1,
            "premise: RPRT {code}: the keyer tried the key"
        );
        assert_eq!(s.banner().as_deref(), Some(REFUSED), "RPRT {code}");
        assert_eq!(
            s.backend.played.len(),
            12_000,
            "RPRT {code}: the message was not played"
        );
    }

    let mut s = Scene::new(Key::Answers(-9), send);
    s.run_to(2_000.0);
    assert!(s.keys() >= 1, "premise: RPRT -9: the keyer tried the key");
    assert_eq!(s.banner().as_deref(), Some(REFUSED), "RPRT -9");
    assert!(
        s.backend.played.is_empty(),
        "RPRT -9: the message was played"
    );
}

/// ⭐ AN RTTY OVER WHOSE KEY HAMLIB COULD NOT GET ANSWERED IS PLAYED AND ECHOED AS SENT, with the
/// keyer's warning. A rejection (`RPRT -9`) is still neither played nor echoed.
#[test]
fn hamlibs_no_answer_to_the_rtty_key_still_plays_and_echoes_the_over() {
    let send = |e: &mut Engine| {
        e.set_operating_mode("rtty", false);
        e.rtty_send_text("CQ TEST").unwrap();
    };
    let warned = |s: &Scene| {
        let st = s.engine.lock().unwrap().rtty_state();
        st.keyer_error
            .is_some_and(|e| e.starts_with("AFSK keyer: the rig didn't accept PTT"))
    };
    for code in NO_ANSWER {
        let mut s = Scene::new(Key::Answers(code), send);
        s.run_to(3_000.0);
        assert!(
            s.keys() >= 1,
            "premise: RPRT {code}: the over tried the key"
        );
        assert!(warned(&s), "RPRT {code}: no keyer warning");
        assert!(
            !s.backend.played.is_empty(),
            "RPRT {code}: the over was not played"
        );
        assert_eq!(
            rtty_sent(&s.engine),
            "CQ TEST",
            "RPRT {code}: the over was not echoed"
        );
    }

    let mut s = Scene::new(Key::Answers(-9), send);
    s.run_to(3_000.0);
    assert!(s.keys() >= 1, "premise: RPRT -9: the over tried the key");
    assert!(warned(&s), "RPRT -9: no keyer warning");
    assert!(s.backend.played.is_empty(), "RPRT -9: the over was played");
    assert_eq!(
        rtty_sent(&s.engine),
        "",
        "RPRT -9: an over that never keyed was echoed"
    );
}

// ── The slot overs: FT8, FT4, JS8 and the other timed-slot modes follow WSJT-X ──────────────
// WSJT-X 3.0.2 plays an over's audio only once the rig reports PTT on (mainwindow.cpp:12440-12448)
// and halts on a PTT Hamlib answers with any error code, its own timeouts included
// (HamlibTransceiver.cpp:277-284, mainwindow.cpp:12556-12564). It sets no deadline of its own, so
// a slow rig keys late and plays. `crate::slot::slot_key_failure` is that rule here.

/// The 20 m FT8 dial, the rigctld's as well as the engine's: a radio reading elsewhere is followed
/// there, and FT8 is not keyed in the phone segment.
const FT8_DIAL: u64 = 14_074_000;

/// An FT8 CQ, armed in the Digital section the way Call CQ arms it.
fn ft8_cq(e: &mut Engine) {
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.broadcast("CQ KD9TAW EN52");
}

/// [`ft8_cq`], for the slot boundary to key: the snappy first over is drained, so the loop's
/// first tick, on slot 0 (this station's parity), keys it at the boundary.
fn ft8_cq_at_the_boundary(e: &mut Engine) {
    ft8_cq(e);
    let _ = e.take_immediate_tx();
}

impl Scene {
    /// Whether transmit is armed (the FT cockpit's TX On).
    fn armed(&self) -> bool {
        self.engine.lock().unwrap().tx_enabled()
    }

    /// What the operator is told about a refused slot key.
    fn slot_key_refused(&self) -> Option<tempo_app::dto::SlotKeyRefused> {
        self.engine
            .lock()
            .unwrap()
            .snapshot()
            .radio
            .slot_key_refused
    }

    /// Whether an unkey followed the first key on the wire.
    fn unkeyed_after_the_key(&self) -> bool {
        let log = self.log.lock().unwrap();
        log.iter()
            .position(|l| l == "T 1")
            .is_some_and(|key| log[key..].iter().any(|l| l == "T 0"))
    }
}

/// ⭐ AN FT8 OVER THE RADIO REFUSES TO KEY IS NOT PLAYED, AND TX HALTS, AS WSJT-X HALTS. The over
/// was played into the receiving radio, TX stayed armed, and the next cycle tried again. Now
/// nothing is played, the radio is unkeyed, TX is off, and the operator is told why.
#[test]
fn an_ft8_over_the_radio_refuses_to_key_is_not_played_and_halts_tx() {
    for key in [Key::Refuses, Key::Answers(-9)] {
        let mut s = Scene::on(key, FT8_DIAL, ft8_cq_at_the_boundary);
        assert!(s.armed(), "premise: Call CQ armed TX");
        s.run_to(300.0);
        assert!(s.keys() >= 1, "premise: the boundary tried the key");
        assert!(
            s.backend.played.is_empty(),
            "the over was played into a radio that did not key"
        );
        assert!(
            !s.armed(),
            "TX is still on (WSJT-X's Halt Tx unticks Enable Tx)"
        );
        assert!(
            s.slot_key_refused().is_some(),
            "nothing says why TX stopped"
        );
        assert!(s.unkeyed_after_the_key(), "the radio was not unkeyed");
    }

    // The control: a radio that keys gets the over, and TX stays on.
    let mut s = Scene::on(Key::Accepts, FT8_DIAL, ft8_cq_at_the_boundary);
    s.run_to(300.0);
    assert_eq!(s.keys(), 1, "control: the boundary keyed once");
    assert!(!s.backend.played.is_empty(), "control: no over");
    assert!(s.armed(), "control: TX went off");
    assert_eq!(s.slot_key_refused(), None, "control");
}

/// ⭐ HAMLIB'S OWN "THE RIG DID NOT ANSWER" TO AN FT8 KEY HALTS TX, AS WSJT-X HALTS ON IT. This is
/// where a slot over differs from the voice keyer, RTTY, Tune and the rest, which play on it with
/// their warning: WSJT-X makes no exception for Hamlib's timeout codes.
#[test]
fn hamlibs_own_no_answer_to_an_ft8_key_halts_tx() {
    for code in NO_ANSWER {
        let mut s = Scene::on(Key::Answers(code), FT8_DIAL, ft8_cq_at_the_boundary);
        assert!(s.armed(), "premise: RPRT {code}: Call CQ armed TX");
        s.run_to(300.0);
        assert!(
            s.keys() >= 1,
            "premise: RPRT {code}: the boundary tried the key"
        );
        assert!(
            s.backend.played.is_empty(),
            "RPRT {code}: the over was played into a radio that did not key"
        );
        assert!(!s.armed(), "RPRT {code}: TX is still on");
        assert!(
            s.slot_key_refused().is_some(),
            "RPRT {code}: nothing says why"
        );
        assert!(
            s.unkeyed_after_the_key(),
            "RPRT {code}: the radio was not unkeyed"
        );
    }
}

/// ⭐ A LATE ANSWER TO AN FT8 KEY STILL PLAYS THE OVER. Nexus's own PTT deadline has no counterpart
/// in WSJT-X, which waits on Hamlib: a slow radio keys late and its over goes out, as it always did
/// here.
#[test]
fn a_late_answer_to_an_ft8_key_still_plays_the_over() {
    let mut s = Scene::on(Key::AnswersLate, FT8_DIAL, ft8_cq_at_the_boundary);
    s.run_to(300.0);
    assert_eq!(s.keys(), 1, "premise: the boundary tried the key, once");
    assert!(!s.backend.played.is_empty(), "the over was not played");
    assert!(s.armed(), "TX went off");
    assert_eq!(s.slot_key_refused(), None);
}

/// ⭐ THE LATE START (a Call CQ or a double-click inside our own slot) IS A SLOT KEY TOO: a refused
/// one plays nothing and halts TX. The loop is settled first (its FT8 clock built, slot 0's
/// boundary consumed), as a running station is, so the over keys mid-slot.
#[test]
fn a_refused_late_start_ft8_key_is_not_played_and_halts_tx() {
    for key in [Key::Refuses, Key::Accepts] {
        let mut s = Scene::on(key, FT8_DIAL, |e| e.set_tier(Tier::Ft8));
        s.run_to(100.0);
        assert_eq!(
            s.state.last_slot,
            Some(0),
            "scene guard: slot 0's boundary is consumed"
        );
        ft8_cq(&mut s.engine.lock().unwrap());
        s.run_to(200.0);
        assert_eq!(s.keys(), 1, "premise: the late start tried the key, once");
        assert!(
            !s.state.boundary_keyed.is_some_and(|k| k.tx_this_slot),
            "scene guard: the boundary did not key"
        );
        if key == Key::Refuses {
            assert!(s.backend.played.is_empty(), "the over was played");
            assert!(!s.armed(), "TX is still on");
            assert!(s.slot_key_refused().is_some(), "nothing says why");
            assert!(s.unkeyed_after_the_key(), "the radio was not unkeyed");
        } else {
            assert!(!s.backend.played.is_empty(), "control: no over");
            assert!(s.armed(), "control: TX went off");
            assert_eq!(s.slot_key_refused(), None, "control");
        }
    }
}

/// ⏱ A KEY THE RADIO ACCEPTS GOES OUT ON THE FIRST TICK OF ITS SLOT, AS IT ALWAYS DID. The CQ is
/// armed in slot 0 after its boundary passed, so it waits for slot 2 (slot 1 is the other
/// station's): no key before 30 s, and the key and the audio on the tick that crosses it.
#[test]
fn a_keyed_ft8_over_keys_on_the_first_tick_of_its_slot() {
    let mut s = Scene::on(Key::Accepts, FT8_DIAL, |e| e.set_tier(Tier::Ft8));
    s.run_to(100.0);
    ft8_cq_at_the_boundary(&mut s.engine.lock().unwrap());
    s.run_to(29_990.0);
    assert_eq!(s.keys(), 0, "keyed before its slot");
    assert!(s.backend.played.is_empty(), "played before its slot");
    s.run_to(30_000.0);
    assert_eq!(s.keys(), 1, "not keyed on the first tick of its slot");
    assert!(
        !s.backend.played.is_empty(),
        "no audio on the first tick of its slot"
    );
    assert!(s.armed() && s.slot_key_refused().is_none());
}
