//! ⛔ THE RADIO'S OWN CW KEYER IS NEVER HANDED A `;`. Hamlib sends the CAT keyer's text inside a
//! radio command (`send_morse`: `KY <text>;` to a Kenwood or an Elecraft, `KM1<text>;` to a newer
//! Yaesu), and those radios end a command at the first `;`: one in the text ended the keyer's
//! command early, and the radio read the rest as a command of its own. Hamlib's TS-2000 backend
//! wrote `KY TEST;` and then `FA00007000000      ;` for `b TEST;FA00007000000` (rigctl 4.5.5, a
//! fake radio on a pty): a retune to 7.000 MHz while the radio keys CW. The text reaches the
//! keyer from the operator's typing and macros, from Enter Sends Message, from a callbook
//! (`{HISNAME}`, `{HISSTATE}`) and from a computer driving the station. A message holding a
//! character the radio's keyer is never handed is refused whole before any of it keys, so Enter
//! Sends Message never logs a contact whose exchange stopped part way; the keyer's own check on
//! each word stays behind it. Each test runs the real `RadioLoop::step` against a rigctld that
//! records every line exactly as it was sent.

use super::cw_stop::{lines_since, step};
use super::*;
use std::io::{BufRead, BufReader, Write};

/// The second command the earlier measurement saw the radio read after the CW text's `;`: VFO A
/// to 7.000 MHz.
const RETUNE: &str = "FA00007000000";

/// The CW screen's words for a `;`, by value.
const SEMICOLON: &str = "CW not sent: \";\" is never handed to the radio's own keyer, because \
     Kenwood, Yaesu and Elecraft radios read it as the end of a command. Take it out of the \
     message.";

/// A rigctld that records each line it is sent exactly as sent (only the `\n` that ends it taken
/// off), and answers `f` 7.030 MHz, `m` CW, `t` not keyed and anything else `RPRT 0`.
fn recording_rigctld() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let log2 = Arc::clone(&log);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(match stream.try_clone() {
                Ok(r) => r,
                Err(_) => continue,
            });
            let mut bytes = Vec::new();
            loop {
                bytes.clear();
                match reader.read_until(b'\n', &mut bytes) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let line = String::from_utf8_lossy(&bytes);
                let line = line.strip_suffix('\n').unwrap_or(&line).to_string();
                let reply = match line.trim() {
                    "f" => "7030000\n",
                    "m" => "CW\n500\n",
                    "t" => "0\n",
                    _ => "RPRT 0\n",
                };
                log2.lock().unwrap().push(line);
                if stream.write_all(reply.as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    (addr, log)
}

/// A TS-2000 on the CAT keyer in the CW section on 40 m, at 20 WPM.
fn station() -> Arc<Mutex<Engine>> {
    station_with(|_| {})
}

/// [`station`], with `change` made to its settings first.
fn station_with(change: impl FnOnce(&mut tempo_app::settings::Settings)) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = 2014;
        change(&mut s);
        e.apply_settings(s);
        e.set_license_class("extra");
        e.set_cw_keyer("cat", 600.0);
        e.set_operating_mode("cw", false);
        e.set_frequency(7.03, "40m", "CW");
        e.set_cw_wpm(20);
    }
    engine
}

/// The radio loop, its rig on the recording rigctld, and the time of its next tick (ms).
struct Scene {
    engine: Arc<Mutex<Engine>>,
    log: Arc<Mutex<Vec<String>>>,
    rig: Rig,
    backend: MockBackend,
    state: RadioLoop,
    t: f64,
}

impl Scene {
    fn new(engine: Arc<Mutex<Engine>>) -> Scene {
        let (addr, log) = recording_rigctld();
        let state = loop_state_for(&engine);
        Scene {
            engine,
            log,
            rig: Rig::rigctld(&addr),
            backend: MockBackend::new(),
            state,
            t: 100.0,
        }
    }

    /// Run `ticks` ticks of the loop, 5 s apart: a tick takes the next word once the one before has
    /// keyed and its word space passed (under 5 s at 20 WPM for every word here but the long name,
    /// about 18 s).
    fn run(&mut self, ticks: usize) {
        for _ in 0..ticks {
            step(
                &mut self.state,
                &self.engine,
                &mut self.backend,
                &mut self.rig,
                self.t,
            );
            self.t += 5_000.0;
        }
    }

    /// Every line the rigctld was sent from `from` on.
    fn lines(&self, from: usize) -> Vec<String> {
        lines_since(&self.log, from)
    }

    /// How many lines the rigctld has been sent.
    fn sent(&self) -> usize {
        self.log.lock().unwrap().len()
    }

    fn keyer_error(&self) -> Option<String> {
        self.engine.lock().unwrap().cw_keyer_error()
    }
}

/// The lines that handed the radio's keyer a word (`b <word>`, Hamlib's send_morse), as sent.
fn keyed(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|l| l.starts_with("b "))
        .cloned()
        .collect()
}

/// After a refused message or word: the controls, each in the scene the refusal left. A new message
/// keys from the next tick: nothing waits on a word that was never sent, and none of the refused
/// message is left to key first. Ordinary text keys exactly as before, its one-character word
/// padded to two (`b K `, issue #86), and the CW screen's warning goes once a word keys; then the
/// punctuation and prosigns Nexus's table has (`?`, `=`, `+`, `/`, `(`, `)`) key too.
fn controls_key_as_before(s: &mut Scene, send: impl Fn(&mut Engine, &str)) {
    let from = s.sent();
    send(&mut s.engine.lock().unwrap(), "CQ TEST DE KD9TAW K");
    s.run(1);
    assert_eq!(
        keyed(&s.lines(from)),
        ["b CQ"],
        "control: a new message keys at once after a refusal"
    );
    s.run(5);
    assert_eq!(
        keyed(&s.lines(from)),
        ["b CQ", "b TEST", "b DE", "b KD9TAW", "b K "],
        "control: ordinary text keys exactly as before"
    );
    assert_eq!(s.keyer_error(), None, "control: a word that keys clears it");
    let from = s.sent();
    send(&mut s.engine.lock().unwrap(), "TU 5NN? = + R/ (KN)");
    s.run(7);
    assert_eq!(
        keyed(&s.lines(from)),
        ["b TU", "b 5NN?", "b = ", "b + ", "b R/", "b (KN)"],
        "control: the punctuation and prosigns Nexus's table has still key"
    );
    assert_eq!(s.keyer_error(), None);
}

/// ⛔ A `;` TYPED IN THE CW SCREEN (or in an F-key macro) NEVER REACHES THE RADIO'S KEYER, AND
/// NOTHING OF ITS MESSAGE KEYS. The message is refused whole before any word goes, so it is never
/// keyed part way and stopped, and the CW screen says why, once. CONTROLS: the same scene then keys
/// ordinary text and the punctuation Nexus's table has exactly as before.
#[test]
fn a_semicolon_typed_in_cw_never_reaches_the_radios_keyer_and_the_cw_screen_says_why() {
    let mut s = Scene::new(station());
    s.engine
        .lock()
        .unwrap()
        .send_cw(&format!("CQ TEST;{RETUNE} DE KD9TAW"));
    s.run(6);
    let lines = s.lines(0);
    assert!(
        !lines.iter().any(|l| l.contains(RETUNE)),
        "the word with the `;` reached the rigctld: {lines:?}"
    );
    assert_eq!(
        keyed(&lines),
        Vec::<String>::new(),
        "nothing of the message keys: it is refused whole, the word before the `;` too"
    );
    assert_eq!(
        s.keyer_error().as_deref(),
        Some(SEMICOLON),
        "the CW screen says the `;` was not sent"
    );
    controls_key_as_before(&mut s, |e, text| e.send_cw(text));
}

/// ⛔ ENTER SENDS MESSAGE KEYS NOTHING OF AN EXCHANGE THE RADIO'S KEYER WOULD NOT TAKE, AND ANSWERS
/// IT AS A REFUSAL, SO THE CONTEST STRIP LOGS NOTHING. The strip logs the contact only once Enter's
/// send has taken its message. Before, the send took it, the call and the class keyed, the keyer
/// refused the section (`IL;`, a slip of the finger beside `L` in Settings), and the contact was
/// already in the log with its exchange cut short. The strip's own line says why, so the CW
/// screen's warning line does not say it twice. CONTROL: the exchange as it should be keys in full.
#[test]
fn enter_sends_nothing_of_an_exchange_the_radios_keyer_would_not_take_so_nothing_is_logged() {
    let field_day = |section: &'static str| {
        station_with(move |s| {
            s.fd_active = true;
            s.fd_event = "arrlfd".into();
            s.fd_class = "3A".into();
            s.fd_section = section.into();
        })
    };
    let mut s = Scene::new(field_day("IL;"));
    let took = {
        let mut e = s.engine.lock().unwrap();
        e.select_peer("K9AAA");
        assert_eq!(
            e.preview_cw("! {EXCH}"),
            "K9AAA 3A IL;",
            "premise: the exchange holds the `;`"
        );
        e.send_cw_armed("! {EXCH}")
    };
    s.run(4);
    assert_eq!(
        (took, keyed(&s.lines(0)), s.keyer_error()),
        (Err(SEMICOLON.to_string()), vec![], None),
        "(Enter's send, what keyed, the CW screen's line): refused, so the strip logs nothing; \
         nothing of the message keys; said once, in the strip's own line"
    );
    // CONTROL: the section as it should be keys in full, and Enter's send takes it.
    let mut s = Scene::new(field_day("IL"));
    let took = {
        let mut e = s.engine.lock().unwrap();
        e.select_peer("K9AAA");
        e.send_cw_armed("! {EXCH}")
    };
    s.run(4);
    assert_eq!(took, Ok(()), "control: taken, so the strip logs it");
    assert_eq!(
        keyed(&s.lines(0)),
        ["b K9AAA", "b 3A", "b IL"],
        "control: the exchange keys in full"
    );
    assert_eq!(s.keyer_error(), None);
}

/// ⛔ THE RADIO'S KEYER KEEPS ITS OWN CHECK, FOR A WORD THAT REACHES IT ANOTHER WAY. Here a message
/// was queued for a WinKeyer, which puts no text inside a radio command and so takes it as typed,
/// and the keyer was switched to CAT on the CW screen before the radio loop took a word. The word
/// with the `;` is refused when its turn comes, the rest of the message is dropped with it, and the
/// CW screen says why, once. CONTROLS: ordinary text then keys exactly as before.
#[test]
fn a_message_queued_for_a_winkeyer_then_keyed_by_the_radio_never_hands_it_a_semicolon() {
    let engine = station_with(|s| s.winkeyer_port = "/dev/ttyUSB0".into());
    {
        let mut e = engine.lock().unwrap();
        e.set_cw_keyer("winkeyer", 600.0);
        let text = format!("CQ TEST;{RETUNE} DE KD9TAW");
        e.send_cw(&text);
        assert_eq!(
            (e.cw_sent().last(), e.cw_keyer_error()),
            (Some(&text), None),
            "premise: taken for the WinKeyer, as typed"
        );
        e.set_cw_keyer("cat", 600.0); // before the radio loop takes a word
    }
    let mut s = Scene::new(engine);
    s.run(6);
    let lines = s.lines(0);
    assert!(
        !lines.iter().any(|l| l.contains(RETUNE)),
        "the word with the `;` reached the rigctld: {lines:?}"
    );
    assert_eq!(
        keyed(&lines),
        ["b CQ"],
        "the word before it keys, and nothing after it: the rest of the message is dropped"
    );
    assert_eq!(
        s.keyer_error().as_deref(),
        Some(SEMICOLON),
        "the CW screen says the `;` was not sent"
    );
    controls_key_as_before(&mut s, |e, text| e.send_cw(text));
}

/// ⛔ A CALLBOOK NAME OR STATE WITH A `;` KEYS THE REST, NEVER THE `;`. `{HISNAME}` and `{HISSTATE}`
/// are the worked station's own callbook entry, put into Nexus's CW by an F-key macro. Each is kept
/// to what the keyers send where it enters, so a `;` is taken out, and the name and the state still
/// key around it: no word carries a `;`, so each is one command to the radio. CONTROL: a plain
/// name keys as it is.
#[test]
fn a_callbook_name_with_a_semicolon_keys_the_rest_of_the_name_and_never_the_semicolon() {
    let mut s = Scene::new(station());
    {
        let mut e = s.engine.lock().unwrap();
        e.select_peer("K2DEF");
        e.set_cw_peer_info("K2DEF".into(), format!("BOB;{RETUNE}"), "O;H".into());
        e.send_cw("TU {HISNAME} {HISSTATE} 73");
    }
    s.run(10);
    let lines = s.lines(0);
    assert!(
        !keyed(&lines).iter().any(|l| l.contains(';')),
        "a `;` reached the radio's keyer: {lines:?}"
    );
    assert_eq!(
        keyed(&lines),
        ["b TU", &format!("b BOB{RETUNE}"), "b OH", "b 73"],
        "the rest of the name, and the state, key"
    );
    assert_eq!(s.keyer_error(), None);
    // CONTROL: a plain name keys as it is.
    let from = s.sent();
    {
        let mut e = s.engine.lock().unwrap();
        e.set_cw_peer_info("K2DEF".into(), "Bob".into(), "OH".into());
        e.send_cw("TU {HISNAME} {HISSTATE}");
    }
    s.run(4);
    assert_eq!(
        keyed(&s.lines(from)),
        ["b TU", "b Bob", "b OH"],
        "control: a plain name keys as it is"
    );
}

/// ⛔ CW TYPED ON A COMPUTER DRIVING THE STATION TAKES THE SAME ROAD, AND THE SAME RULE. Remote's CW
/// reaches the station's own `send_cw` handler (src-tauri), which runs exactly this for a call from
/// afar: the text as that computer sent it into the engine's one CW queue, then the note that a
/// start from afar ran. The message with the `;` is refused whole there, so nothing of it reaches
/// the radio's keyer and the CW screen says why. CONTROLS: from afar, ordinary text and the table's
/// punctuation key exactly as before.
#[test]
fn cw_typed_from_afar_never_hands_the_radios_keyer_a_semicolon() {
    let from_afar = |e: &mut Engine, text: &str| {
        let under = e.remote_transmit_stand_down_generation();
        e.send_cw_as(text, false);
        e.note_started_from_afar(under);
    };
    let mut s = Scene::new(station());
    from_afar(
        &mut s.engine.lock().unwrap(),
        &format!("CQ TEST;{RETUNE} DE KD9TAW"),
    );
    // Two ticks, where `CQ` and the word with the `;` would go. The controls' message follows on
    // the very next tick, so its first word is the one keyed only if nothing of this message is
    // left.
    s.run(2);
    let lines = s.lines(0);
    assert!(
        !lines.iter().any(|l| l.contains(RETUNE)),
        "the word with the `;` from afar reached the rigctld: {lines:?}"
    );
    assert_eq!(
        keyed(&lines),
        Vec::<String>::new(),
        "nothing of the message from afar keys: it is refused whole"
    );
    assert_eq!(
        s.keyer_error().as_deref(),
        Some(SEMICOLON),
        "the CW screen says the `;` was not sent"
    );
    controls_key_as_before(&mut s, from_afar);
}

/// ⛔ A SPLIT PRESSED DURING A CAT-KEYER MESSAGE SETS VFO B'S MODE ONLY AFTER THE LAST WORD AND THE
/// RADIO'S HANG TIME. The loop's end of a word is an estimate (`cw_keying_until`), and the next
/// word is taken only once it passes, so a mode owed to VFO B could land between two words, or
/// as the radio's own keyer finished a word it ran late on: on a radio Hamlib reaches by a VFO
/// swap, mid-transmission. It waits for the queue to empty and `CW_HANG_MS` past the last word.
#[test]
fn a_split_pressed_during_a_cat_keyer_message_sets_vfo_bs_mode_after_its_hang_time() {
    let mut s = Scene::new(station_with(|s| {
        s.rig_model = 1042; // FTDX10
        s.split_mode = tempo_app::settings::SplitMode::None;
    }));
    s.run(1);
    {
        let mut e = s.engine.lock().unwrap();
        e.set_tx_enabled(true);
        e.send_cw("CQ CQ DE KD9TAW K");
    }
    let from = s.sent();
    let mut pressed = false;
    let (mut t_last_word, mut last_word_until, mut t_x) = (None, None, None);
    for _ in 0..400 {
        let before = s.sent();
        step(&mut s.state, &s.engine, &mut s.backend, &mut s.rig, s.t);
        let new = s.lines(before);
        if !pressed && !keyed(&new).is_empty() {
            // The first word is on the air: split pressed now.
            s.engine.lock().unwrap().request_split(Some(7.031));
            pressed = true;
        }
        if !keyed(&new).is_empty() {
            t_last_word = Some(s.t);
            last_word_until = Some(s.state.cw_keying_until);
        }
        if t_x.is_none() && new.iter().any(|l| l.starts_with("X ")) {
            t_x = Some(s.t);
        }
        s.t += 100.0;
    }
    let lines = s.lines(from);
    assert_eq!(
        keyed(&lines),
        ["b CQ", "b CQ", "b DE", "b KD9TAW", "b K "],
        "precondition: the whole message keyed"
    );
    let x: Vec<&String> = lines.iter().filter(|l| l.starts_with("X ")).collect();
    assert_eq!(x, ["X CW -1"], "one X: {lines:?}");
    let (t_x, t_last, until) = (t_x.unwrap(), t_last_word.unwrap(), last_word_until.unwrap());
    assert!(
        t_x > t_last,
        "the X after the last word ({t_x} ms, last word {t_last} ms)"
    );
    assert!(
        t_x >= until + CW_HANG_MS,
        "the X after the last word's end plus the hang time ({t_x} ms, end {until} ms)"
    );
}
