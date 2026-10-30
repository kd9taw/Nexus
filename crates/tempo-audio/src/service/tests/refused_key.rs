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
//! nothing back, so the radio may be keying (`hamlibs_no_answer_*`). Their warning says the key was
//! not confirmed, never that it was not accepted. A rig that rejects the key (`RPRT -9`) is
//! refusing it.
//!
//! The TUNE is the exception: its carrier plays only into a key the radio confirmed. A key still
//! unanswered could reach the radio seconds later, after the tone, or after the operator let go of
//! Tune; so a tune whose key is not confirmed plays nothing, ends, and says so, and its release
//! goes out behind the key (`a_tune_whose_key_*`).
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

    /// The kind the snapshot gives that line, as the UI reads it off the wire (`Null` = none).
    fn kind(&self) -> serde_json::Value {
        let snapshot = self.engine.lock().unwrap().snapshot();
        serde_json::to_value(snapshot).unwrap()["radio"]["audioErrorKind"].clone()
    }
}

/// The voice keyer's words for a refused key.
const REFUSED: &str = "The rig didn't accept PTT — check your PTT method and CAT/port.";

/// The words for a key the radio did not answer in time.
const UNCONFIRMED: &str =
    "The rig did not answer the PTT key in time — check the CAT link to the radio.";

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
    // Named as a PTT problem, so the status lane says so instead of "the radio stopped".
    assert_eq!(s.kind(), "ptt");
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
    assert_eq!(
        s.kind(),
        serde_json::Value::Null,
        "control: no line, no kind"
    );
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

/// ⭐ THE SNAPSHOT SAYS A VOICE MESSAGE IS PLAYING FOR AS LONG AS THE LOOP PLAYS IT
/// (`radio.voicePlaying`, which the voice keyer reads to know its message ended), and no longer;
/// a message the radio refused never reads as playing once the loop has refused it.
#[test]
fn the_snapshot_says_a_voice_message_plays_until_its_over_ends() {
    let send = |e: &mut Engine| {
        phone(e);
        e.send_voice(vec![0.05; 12_000]).unwrap();
    };
    let playing = |s: &Scene| s.engine.lock().unwrap().snapshot().radio.voice_playing;
    let mut s = Scene::new(Key::Accepts, send);
    assert!(playing(&s), "queued");
    s.run_to(600.0);
    assert!(s.keys() >= 1, "premise: keyed");
    assert!(playing(&s), "mid-message");
    s.run_to(3_000.0);
    assert_eq!(s.backend.played.len(), 12_000, "premise: played whole");
    assert!(!playing(&s), "the over ended");

    let mut s = Scene::new(Key::Refuses, send);
    s.run_to(2_000.0);
    assert!(s.backend.played.is_empty(), "premise: refused");
    assert!(!playing(&s), "a refused message never ended");
}

/// ⭐ A VOICE MESSAGE READS AS PLAYING UNTIL ITS OWN OVER ENDS, whatever else moves the loop's
/// shared PTT deadline: a later over that pushes that deadline past the message's end neither
/// ends the message early nor keeps it playing after its end.
#[test]
fn a_voice_message_plays_until_its_own_deadline_not_the_shared_one() {
    let mut s = Scene::new(Key::Accepts, |e| {
        phone(e);
        e.send_voice(vec![0.05; 12_000]).unwrap();
    });
    let playing = |s: &Scene| s.engine.lock().unwrap().snapshot().radio.voice_playing;
    s.run_to(600.0);
    assert!(playing(&s), "premise: mid-message");
    let own = s
        .state
        .voice_until_ms
        .expect("premise: the message's deadline");
    // A later over on the same key, ending well after the message.
    s.state.tx_until_ms = Some(own + 5_000.0);
    s.run_to(700.0);
    assert!(playing(&s), "a later over ended the message early");
    s.run_to(own + 40.0);
    assert!(
        s.state.tx_until_ms.is_some(),
        "premise: the later over is still on the air"
    );
    assert!(!playing(&s), "the message read as playing after its end");
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

/// ⛔ A TUNE WHOSE KEY IS NOT CONFIRMED PLAYS NO CARRIER. Its key answered after the PTT deadline,
/// and the tune used to put its carrier into the radio anyway. Behind a rigctld held up for
/// seconds the key was still on its way: the tone went into a receiving radio and the key reached
/// it long after, once after the operator had let go of Tune. Now nothing plays, the tune ends and
/// is not started again by the late answer (an over that late is stale), the line says the key
/// was not confirmed, and the release goes out after the key.
#[test]
fn a_tune_whose_key_is_answered_late_plays_no_carrier_and_ends() {
    let mut s = Scene::new(Key::AnswersLate, |e| {
        phone(e);
        e.set_tune(true);
    });
    s.run_to(300.0);
    assert!(s.keys() >= 1, "premise: the tune tried the key");
    assert!(
        s.backend.played.is_empty(),
        "a carrier went into an unconfirmed key"
    );
    assert!(!s.engine.lock().unwrap().tuning(), "the tune is still on");
    assert_eq!(s.banner().as_deref(), Some(UNCONFIRMED));
    assert_eq!(s.kind(), "pttUnconfirmed");
    s.run_to(3_000.0);
    assert!(
        s.backend.played.is_empty(),
        "the late answer started the carrier"
    );
    assert_eq!(s.keys(), 1, "the tune keyed again");
    let log = s.log.lock().unwrap().clone();
    let key = log.iter().position(|l| l == "T 1").unwrap();
    assert!(
        log[key..].iter().any(|l| l == "T 0"),
        "the release did not follow the key: {log:?}"
    );
}

/// The control: a tune whose key the radio confirms at once plays its carrier on that same tick.
#[test]
fn a_tune_keyed_at_once_still_plays_on_the_first_tick() {
    let mut s = Scene::new(Key::Accepts, |e| {
        phone(e);
        e.set_tune(true);
    });
    s.run_to(s.t + 20.0);
    assert_eq!(s.keys(), 1, "premise: the tune keyed");
    assert!(
        !s.backend.played.is_empty(),
        "no carrier on the keying tick"
    );
    assert_eq!(s.banner(), None);
}

/// The operator's own PTT names a key with no answer in time as not confirmed, and a refusal as
/// not accepted. Nothing of Nexus's plays under it either way: the voice is the operator's, at
/// the radio's microphone.
#[test]
fn a_phone_ptt_not_answered_in_time_reads_not_confirmed_and_a_refusal_not_accepted() {
    for (key, words, kind) in [
        (Key::AnswersLate, UNCONFIRMED, "pttUnconfirmed"),
        (Key::Answers(-5), UNCONFIRMED, "pttUnconfirmed"),
        (Key::Refuses, REFUSED, "ptt"),
    ] {
        let mut s = Scene::new(key, |e| {
            phone(e);
            e.set_ptt(true);
        });
        s.run_to(300.0);
        assert!(s.keys() >= 1, "premise: the PTT tried the key");
        assert_eq!(s.banner().as_deref(), Some(words));
        assert_eq!(s.kind(), kind);
        assert!(s.backend.played.is_empty(), "Nexus played under the mic");
    }
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
    assert_eq!(s.banner().as_deref(), Some(UNCONFIRMED));
    assert_eq!(s.kind(), "pttUnconfirmed");
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
    assert_eq!(s.banner().as_deref(), Some(UNCONFIRMED));
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

/// ⛔ HAMLIB'S OWN "THE RIG DID NOT ANSWER" TO THE TUNE'S KEY IS NOT A CONFIRMED KEY. A radio too
/// slow for Hamlib makes rigctld answer the key with its timeout (`RPRT -5`), as Nexus's own
/// deadline would. It is not a refusal, so the line says the key was not confirmed; and as for
/// any key not confirmed, the tune plays no carrier and ends. A rejection (`RPRT -9`) drops it
/// too, as a refusal.
#[test]
fn a_tune_whose_key_hamlib_could_not_get_answered_plays_no_carrier() {
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
        assert_eq!(s.banner().as_deref(), Some(UNCONFIRMED), "RPRT {code}");
        assert_eq!(s.kind(), "pttUnconfirmed", "RPRT {code}");
        assert!(
            s.backend.played.is_empty(),
            "RPRT {code}: a carrier went into an unconfirmed key"
        );
        assert!(
            !s.engine.lock().unwrap().tuning(),
            "RPRT {code}: the tune is still on"
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
        assert_eq!(s.banner().as_deref(), Some(UNCONFIRMED), "RPRT {code}");
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
        // Asked before the late start: its key records the slot as keyed too, as the boundary's does.
        assert!(
            !s.state.boundary_keyed.is_some_and(|k| k.tx_this_slot),
            "scene guard: the boundary did not key"
        );
        assert_eq!(
            s.keys(),
            0,
            "scene guard: nothing keyed before the late start"
        );
        ft8_cq(&mut s.engine.lock().unwrap());
        s.run_to(200.0);
        assert_eq!(s.keys(), 1, "premise: the late start tried the key, once");
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

// ── No transmit audio: a slot over never keys the radio with nothing to send ────────────────────
// The sound card's output used to be opened by the first over's `play`, after its key, and an open
// that failed was latched for the session with nothing on screen: every later over keyed the radio
// with no audio. Slot TX now opens the output when TX is armed, and an output that will not open
// keys nothing, halts TX and says why, on the refused key's own line, until TX is on again.

/// What [`MockBackend::tx_open_error`] stands in for: a playback device another program holds.
const NO_OUTPUT: &str = "playback stream: Device or resource busy";

impl Scene {
    /// The sound card's words, when the refusal on screen is for transmit audio that would not
    /// open.
    fn refused_for_tx_audio(&self) -> Option<String> {
        self.slot_key_refused()
            .filter(|r| r.tx_audio_failed)
            .map(|r| r.why)
    }
}

/// ⭐ AN FT8 OVER WHOSE AUDIO OUTPUT WILL NOT OPEN NEVER KEYS THE RADIO: TX STOPS WITH THE REASON,
/// AND THE NEXT GOOD OPEN KEYS NORMALLY. The open used to happen inside the first over, after the
/// key, and its failure keyed every over of the session with no audio and said nothing. Now
/// nothing goes on the wire, nothing is played, TX is off and the line says why in the sound
/// card's words; once the device is back and the operator turns TX on again, the next over keys
/// and plays as any over does.
#[test]
fn an_ft8_over_with_no_audio_output_never_keys_and_halts_tx_until_a_good_open() {
    let mut s = Scene::on(Key::Accepts, FT8_DIAL, ft8_cq_at_the_boundary);
    s.backend.tx_open_error = Some(NO_OUTPUT.to_string());
    assert!(s.armed(), "premise: Call CQ armed TX");
    s.run_to(300.0);
    let sent = s.log.lock().unwrap().clone();
    assert_eq!(
        s.keys(),
        0,
        "the radio was keyed with no audio to send: {sent:?}"
    );
    assert!(s.backend.played.is_empty(), "an over went to a dead output");
    assert!(
        !s.armed(),
        "TX is still on: the next over would key the radio with no audio"
    );
    assert_eq!(
        s.refused_for_tx_audio().as_deref(),
        Some(NO_OUTPUT),
        "nothing says why TX stopped, in the sound card's own words: {:?}",
        s.slot_key_refused()
    );
    // …and it stays stopped: this station's next boundary (slot 2, 30 s) keys nothing.
    s.run_to(30_100.0);
    assert_eq!(s.keys(), 0, "a later boundary keyed the radio");
    assert!(s.backend.played.is_empty());

    // The device is back (the other program let go of it), and the operator turns TX on again.
    s.backend.tx_open_error = None;
    ft8_cq_at_the_boundary(&mut s.engine.lock().unwrap());
    assert_eq!(s.slot_key_refused(), None, "turning TX on again clears it");
    // Slot 4 (60 s) is the next of this station's boundaries.
    s.run_to(59_990.0);
    assert_eq!(s.keys(), 0, "premise: keyed before its slot");
    s.run_to(60_100.0);
    assert_eq!(s.keys(), 1, "the next good open does not key normally");
    assert!(!s.backend.played.is_empty(), "the good over was not played");
    assert!(s.armed(), "a good over turned TX off");
    assert_eq!(s.slot_key_refused(), None);
}

/// ⭐ THE OUTPUT OPENS WHEN SLOT TX IS ARMED, NOT AT THE FIRST OVER, AND NEVER FOR A STATION THAT
/// ONLY LISTENS (#139: a codec that cannot play and capture at once loses its decodes to an idle
/// playback stream). An FT8 station with TX off opens nothing over a whole T/R cycle; Phone, whose
/// section entry arms TX for the mic, still opens nothing until its own key-down; an FT8 station
/// armed mid-slot has its output opened on the next tick, before any over is due.
#[test]
fn slot_tx_opens_its_audio_output_when_armed_and_a_listening_station_opens_none() {
    let mut s = Scene::on(Key::Accepts, FT8_DIAL, |e| e.set_tier(Tier::Ft8));
    s.run_to(31_000.0);
    assert!(!s.armed(), "premise: TX is off");
    assert_eq!(
        s.backend.prepare_tx_calls, 0,
        "a station that only listens opened its transmit audio"
    );

    let mut p = Scene::new(Key::Accepts, phone);
    assert!(p.armed(), "premise: Phone's section entry armed TX");
    p.run_to(1_000.0);
    assert_eq!(
        p.backend.prepare_tx_calls, 0,
        "Phone opened the slot path's transmit audio without a key-down"
    );

    // Armed in slot 2, this station's own, after its boundary: the CQ waits for slot 4.
    ft8_cq_at_the_boundary(&mut s.engine.lock().unwrap());
    s.run_to(31_040.0);
    assert!(
        s.backend.prepare_tx_calls >= 1,
        "arming TX did not open the transmit audio"
    );
    assert_eq!(s.keys(), 0, "premise: no over was due yet");
    assert!(s.armed() && s.slot_key_refused().is_none());
}

// ── A key a CAT-broker client holds: an armed FT run waits for it ──────────────────────────────
// With "Other programs may key transmit" on, a program on Nexus's CAT broker (WSJT-X, VarAC) keys
// the radio through this loop as a held key (`Engine::broker_ptt`). An armed FT run planned its
// over all the same, and the slot keyed a second time and played the whole over under the
// client's key: one transmitter, two programs' signals. The slot planner now plans no over while
// the client holds its key (`Engine::plan_tx`), as for a program on Nexus's TCI server, and the
// run keys its next free slot once the client lets go.

/// "Other programs may key transmit" on.
fn share_the_key(e: &mut Engine) {
    let mut settings = e.settings().clone();
    settings.cat_broker_ptt = true;
    e.apply_settings(settings);
}

/// One FT8 over played from the first tick of its slot: the 0.5 s lead-in and 79 symbols of
/// 1,920 samples, at 12 kHz.
const FT8_OVER: usize = 157_680;

impl Scene {
    /// The over on the air, or the last one sent: the WSJT-X Status's Tx Message.
    fn sent(&self) -> String {
        let e = self.engine.lock().unwrap();
        e.wsjtx_tx_status().tx_message.trim().to_string()
    }

    /// Run to 1 s into each FT8 slot of `slots`: the message each one sent, `None` where nothing
    /// played.
    fn overs(&mut self, slots: &[u64]) -> Vec<Option<String>> {
        slots
            .iter()
            .map(|&slot| {
                let played = self.backend.played.len();
                self.run_to(slot as f64 * 15_000.0 + 1_000.0);
                (self.backend.played.len() > played).then(|| self.sent())
            })
            .collect()
    }
}

/// The CQ as this station sends it with no client keying, from slot 0: the same scene, the
/// client's key apart. It is longer than one free-text frame, so it goes out in three overs.
fn the_cq_with_no_client() -> Vec<Option<String>> {
    let mut c = Scene::on(Key::Accepts, FT8_DIAL, |e| {
        share_the_key(e);
        ft8_cq_at_the_boundary(e);
    });
    let overs = c.overs(&[0, 2, 4, 6]);
    assert!(
        matches!(overs.as_slice(), [Some(_), Some(_), Some(_), None]),
        "control: the CQ is not three overs: {overs:?}"
    );
    overs
}

/// ⛔ AN ARMED FT8 CQ SENDS NOTHING WHILE A CAT-BROKER CLIENT HOLDS ITS KEY, AND FROM THE NEXT FREE
/// SLOT AFTER THE RELEASE IT GOES OUT AS IT DOES WITH NO CLIENT: every over once, in order, one T/R
/// cycle later. The client keys between the overs and holds its key through this station's slot,
/// as WSJT-X does for an over of its own. The boundary used to key again and play the CQ under the
/// client's key.
#[test]
fn an_armed_ft8_cq_sends_nothing_under_a_broker_clients_key_and_its_next_slot_sends_it() {
    let control = the_cq_with_no_client();
    let mut s = Scene::on(Key::Accepts, FT8_DIAL, |e| {
        share_the_key(e);
        ft8_cq_at_the_boundary(e);
        assert!(
            e.broker_ptt(true),
            "premise: the client keys between the overs"
        );
    });
    assert!(s.armed(), "premise: Call CQ armed TX");
    // Slot 0 is this station's: the client holds its key through it.
    s.run_to(12_000.0);
    assert!(
        s.state.manual_ptt_applied,
        "premise: the loop keyed the client's key"
    );
    assert_eq!(
        (s.keys(), s.backend.played.len()),
        (1, 0),
        "the slot keyed again and played its over under the client's key"
    );
    assert!(s.armed(), "the CQ was disarmed");
    // The client lets go: the radio unkeys, and nothing keys before slot 2 (30 s).
    assert!(
        s.engine.lock().unwrap().broker_ptt(false),
        "premise: the release"
    );
    s.run_to(29_990.0);
    assert!(
        s.unkeyed_after_the_key(),
        "the client's release did not unkey the radio"
    );
    assert_eq!(
        (s.keys(), s.backend.played.len()),
        (1, 0),
        "the CQ keyed before this station's next slot"
    );
    assert_eq!(
        s.overs(&[2, 4, 6, 8]),
        control,
        "from the next free slot the CQ did not go out as with no client, every over once"
    );
    assert_eq!(
        s.backend.played.len(),
        3 * FT8_OVER,
        "an over that waited was not sent whole"
    );
    assert!(s.armed() && s.slot_key_refused().is_none());
}

/// ⛔ A LATE START (a Call CQ inside this station's own slot) UNDER A CAT-BROKER CLIENT'S KEY SENDS
/// NOTHING EITHER, AND THE CQ GOES OUT FROM THE NEXT FREE SLOT AFTER THE RELEASE, every over once.
/// The late start keyed again and played the CQ under the client's key.
#[test]
fn a_late_start_under_a_broker_clients_key_sends_nothing_and_the_next_slot_sends_it() {
    let control = the_cq_with_no_client();
    let mut s = Scene::on(Key::Accepts, FT8_DIAL, |e| {
        share_the_key(e);
        e.set_tier(Tier::Ft8);
    });
    s.run_to(100.0);
    assert_eq!(
        s.state.last_slot,
        Some(0),
        "scene guard: slot 0's boundary is consumed"
    );
    {
        let mut e = s.engine.lock().unwrap();
        ft8_cq(&mut e);
        assert!(
            e.broker_ptt(true),
            "premise: the client keys as the CQ arms"
        );
    }
    s.run_to(5_000.0);
    assert!(
        s.state.manual_ptt_applied,
        "premise: the loop keyed the client's key"
    );
    assert_eq!(
        (s.keys(), s.backend.played.len()),
        (1, 0),
        "the late start keyed again and played its over under the client's key"
    );
    // The client lets go early in the slot: the CQ waits for slot 2 all the same.
    assert!(
        s.engine.lock().unwrap().broker_ptt(false),
        "premise: the release"
    );
    s.run_to(29_990.0);
    assert_eq!(
        (s.keys(), s.backend.played.len()),
        (1, 0),
        "the CQ keyed before this station's next slot"
    );
    assert_eq!(
        s.overs(&[2, 4, 6, 8]),
        control,
        "from the next free slot the CQ did not go out as with no client, every over once"
    );
    assert!(s.armed() && s.slot_key_refused().is_none());
}
