//! The unkey that ends a slot over (FT8, FT4, JS8 and the other timed-slot modes), on a radio that
//! does not take it.
//!
//! WSJT-X 3.0.2 halts. A PTT off that Hamlib answers with any error code throws "… while setting
//! PTT off" (`Transceiver/HamlibTransceiver.cpp:1303`, `:277-284`), the transceiver goes offline
//! (`TransceiverBase.cpp:193-206`, `:360-373`), and the main window presses Halt Tx and reports the
//! rig failure (`widgets/mainwindow.cpp:12556-12564`). Nexus left TX on and said nothing, so the
//! next cycle keyed a radio that had not let go of the last over. Now TX halts and the status bar
//! says so (`crate::slot::slot_unkey_failure`), however the over ended: at its end, cut by Stop TX,
//! or cut by a logger's HaltTx. Every other over's unkey is left as it was (`a_voice_message_*`).
//!
//! What Nexus never stops doing is sending the unkey. WSJT-X sends PTT off once more as it closes
//! the rig (`TransceiverBase.cpp:245-247`), then nothing until the operator reopens it. Nexus's idle
//! self-heal sends it every tick until the radio takes it, and still does, with TX halted. Nothing
//! waits ahead of it either: this halt alone does not ask the radio to stop CW (`\stop_morse`), a
//! command a radio that has stopped answering would hold for a whole CAT deadline
//! (`a_failed_unkeys_halt_sends_no_cw_stop_*`).
//!
//! The rig is a rigctld that keys and answers the unkey (`T 0`) as each test says; the loop is the
//! real `RadioLoop::step`. The loop's clock runs on the real clock's timebase, a little ahead of it,
//! because an over's hold is measured on the real clock (`slot::slot_tx_phase`).

use super::*;
use crate::rig::remote_tests::{retuning_peer, Peer};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// What the rigctld answers to the unkey.
#[derive(Clone, Copy, PartialEq)]
enum Unkey {
    /// `RPRT 0`.
    Accepts,
    /// `RPRT <code>` to the first `n` unkeys, the radio staying keyed; `RPRT 0` after that.
    Fails { code: i32, n: usize },
    /// `RPRT 0` to the first unkey after [`LATE`], and at once after that: a slow radio.
    AnswersLate,
}

/// How many unkeys a failing radio refuses before it takes one: about two seconds of the loop's
/// 20 ms ticks, so the unkey is seen going out again and again, not once more.
const REFUSALS: usize = 100;

/// How long [`Unkey::AnswersLate`] takes to answer. The PTT deadline is 700 ms, but the rig reads
/// in 500 ms windows and looks at the deadline only between them, so an answer inside the first
/// second is still taken; this one comes well after that.
const LATE: Duration = Duration::from_millis(1_500);

/// The 20 m FT8 dial, the rigctld's as well as the engine's.
const FT8_DIAL: u64 = 14_074_000;

/// A station calling CQ in FT8 on that rigctld, its first over keyed at the start of an even slot
/// (this station's parity), or doing whatever else a test sets up ([`Station::with`]).
struct Station {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    peer: Peer,
    /// The radio's own PTT, as the rigctld holds it.
    on_air: Arc<AtomicBool>,
    /// While set, the rigctld refuses the key (`RPRT -1`).
    refuses_key: Arc<AtomicBool>,
    /// While set, the rigctld answers the CW stop (`\stop_morse`) only after [`LATE`]: a radio
    /// that has stopped answering it.
    slow_cw_stop: Arc<AtomicBool>,
    /// The start of the slot the first over is keyed in (ms).
    slot: f64,
    t: f64,
}

impl Station {
    fn new(unkey: Unkey) -> Station {
        Station::with(unkey, FT8_DIAL, "PKTUSB", |e| {
            e.set_tier(Tier::Ft8);
            e.set_frequency(14.074, "20m", "USB");
            e.start_cq(None).expect("Call CQ");
            // The snappy first over is drained, so the slot boundary keys it.
            let _ = e.take_immediate_tx();
        })
    }

    /// The station on a rigctld whose dial reads `dial_hz` in `mode`, for an engine `setup`
    /// armed and gave something to send.
    fn with(unkey: Unkey, dial_hz: u64, mode: &str, setup: impl FnOnce(&mut Engine)) -> Station {
        let on_air = Arc::new(AtomicBool::new(false));
        let ptt = Arc::clone(&on_air);
        let refuses_key = Arc::new(AtomicBool::new(false));
        let refuse = Arc::clone(&refuses_key);
        let slow_cw_stop = Arc::new(AtomicBool::new(false));
        let slow = Arc::clone(&slow_cw_stop);
        let answered = AtomicUsize::new(0);
        let peer = retuning_peer(dial_hz, mode, move |line, radio| {
            match line {
                "T 1" | "T 3" if refuse.load(Ordering::SeqCst) => {
                    return Some("RPRT -1\n".into());
                }
                "T 1" | "T 3" => radio.keyed = true,
                "T 0" => {
                    let i = answered.fetch_add(1, Ordering::SeqCst);
                    match unkey {
                        Unkey::Fails { code, n } if i < n => {
                            return Some(format!("RPRT {code}\n"));
                        }
                        Unkey::AnswersLate if i == 0 => std::thread::sleep(LATE),
                        _ => {}
                    }
                    radio.keyed = false;
                }
                "\\stop_morse" => {
                    if slow.load(Ordering::SeqCst) {
                        std::thread::sleep(LATE);
                    }
                    return Some("RPRT 0\n".into());
                }
                _ => return None,
            }
            ptt.store(radio.keyed, Ordering::SeqCst);
            Some("RPRT 0\n".into())
        });
        let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
        {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            setup(&mut e);
        }
        // An even FT8 slot 10 to 40 s ahead of the real clock, so the loop's clock stays ahead
        // of it for the whole test, and an over's hold is measured on the loop's.
        let slot = ((now_unix_ms() + 10_000.0) / 30_000.0).ceil() * 30_000.0;
        Station {
            engine,
            state: loop_state(),
            backend: MockBackend::new(),
            rig: Rig::rigctld(&peer.address),
            peer,
            on_air,
            refuses_key,
            slow_cw_stop,
            slot,
            t: slot + 100.0,
        }
    }

    /// Run the loop, a 20 ms tick at a time, to `until` ms.
    fn run_to(&mut self, until: f64) {
        self.run_with(None, until);
    }

    /// [`Self::run_to`], with a logger's WSJT-X UDP link.
    fn run_with(&mut self, wsjtx: Option<&WsjtxServer>, until: f64) {
        let sinks = Sinks {
            wsjtx,
            psk: None,
            cfg_dial_hz: FT8_DIAL,
        };
        let (mut ra, mut rr) = (mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        while self.t <= until {
            self.state
                .step(
                    &self.engine,
                    &mut self.backend,
                    &mut self.rig,
                    &sinks,
                    self.t,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
            self.t += 20.0;
        }
    }

    /// How many times the rigctld was sent `line`.
    fn sent(&self, line: &str) -> usize {
        let lines = self.peer.lines.lock().unwrap();
        lines.iter().filter(|l| *l == line).count()
    }

    /// The keys the rigctld was sent.
    fn keys(&self) -> usize {
        let lines = self.peer.lines.lock().unwrap();
        lines.iter().filter(|l| *l == "T 1" || *l == "T 3").count()
    }

    /// The unkeys the rigctld was sent after the first key.
    fn unkeys(&self) -> usize {
        let lines = self.peer.lines.lock().unwrap();
        let Some(key) = lines.iter().position(|l| l == "T 1" || l == "T 3") else {
            return 0;
        };
        lines[key..].iter().filter(|l| *l == "T 0").count()
    }

    /// Whether the radio is transmitting, by its own PTT.
    fn on_air(&self) -> bool {
        self.on_air.load(Ordering::SeqCst)
    }

    /// Whether transmit is armed (the FT cockpit's TX On).
    fn armed(&self) -> bool {
        self.engine.lock().unwrap().tx_enabled()
    }

    /// What the operator is told about a slot over's unkey the radio did not take.
    fn unkey_failed(&self) -> Option<tempo_app::dto::SlotUnkeyFailed> {
        self.engine
            .lock()
            .unwrap()
            .snapshot()
            .radio
            .slot_unkey_failed
    }

    /// The radio's own PTT, read back over CAT: the cockpit's TX badge.
    fn badge(&self) -> bool {
        self.engine.lock().unwrap().snapshot().radio.rig_keyed
    }

    /// Key the first over at the start of its slot, and say when its PTT hold ends.
    fn key_the_over(&mut self) -> f64 {
        self.run_to(self.slot + 300.0);
        assert_eq!(self.keys(), 1, "premise: the slot boundary keyed the over");
        assert!(!self.backend.played.is_empty(), "premise: the over plays");
        assert!(self.armed(), "premise: TX is on");
        self.state.tx_until_ms.expect("premise: the over holds PTT")
    }

    /// From a tick whose unkey the radio refused: the unkey goes out again every tick until the
    /// radio takes the one after its [`REFUSALS`], and then no more; the TX badge shows the radio
    /// keyed until then. `from` is that tick and `what` names the case.
    fn sends_the_unkey_until_the_radio_takes_it(&mut self, from: f64, what: &str) {
        assert!(self.on_air(), "{what}: premise: the radio is still keyed");
        assert!(self.rig.keyed, "{what}: the loop forgot the radio is keyed");
        let sent = self.unkeys();
        self.run_to(from + 1_000.0);
        assert!(
            self.on_air(),
            "{what}: premise: the radio still refuses the unkey"
        );
        assert!(
            self.unkeys() >= sent + 40,
            "{what}: the unkey was not sent again every tick: {sent}, then {} a second later",
            self.unkeys()
        );
        assert!(
            self.badge(),
            "{what}: the TX badge says the radio is not keyed"
        );
        self.run_to(from + 3_000.0);
        assert!(!self.on_air(), "{what}: the radio was left keyed");
        assert!(
            !self.rig.keyed,
            "{what}: the loop believes it is still keyed"
        );
        assert!(!self.badge(), "{what}: the TX badge still says keyed");
        let sent = self.unkeys();
        assert!(sent > REFUSALS, "{what}: {sent} unkeys");
        self.run_to(from + 4_000.0);
        assert_eq!(
            self.unkeys(),
            sent,
            "{what}: the unkey went on being sent after the radio took it"
        );
    }
}

/// ⭐ AN FT8 OVER WHOSE UNKEY THE RADIO REFUSES HALTS TX AND SAYS SO, AND THE UNKEY IS SENT UNTIL THE
/// RADIO TAKES IT. TX stayed on, nothing was said, and the next cycle keyed the radio again. Now TX
/// is off, as WSJT-X's Halt Tx leaves it, the status bar says why, the loop sends the unkey every
/// tick until the radio takes it, and the next cycle keys nothing.
#[test]
fn an_ft8_over_whose_unkey_the_radio_refuses_halts_tx_says_so_and_the_unkey_goes_on() {
    for code in [-1, -9] {
        let what = format!("RPRT {code}");
        let mut s = Station::new(Unkey::Fails { code, n: REFUSALS });
        let end = s.key_the_over();
        s.run_to(end + 20.0);
        assert!(
            s.unkeys() >= 1,
            "{what}: premise: the over's end sent the unkey"
        );
        assert!(
            !s.armed(),
            "{what}: TX is still on (WSJT-X's Halt Tx unticks Enable Tx)"
        );
        let failed = s
            .unkey_failed()
            .unwrap_or_else(|| panic!("{what}: nothing says why TX stopped"));
        assert!(failed.why.contains(&what), "{what}: {failed:?}");
        s.sends_the_unkey_until_the_radio_takes_it(end, &what);

        // The halt holds: the next cycle keys nothing.
        s.run_to(s.slot + 30_300.0);
        assert_eq!(s.keys(), 1, "{what}: the next cycle keyed the radio");
        assert!(s.unkey_failed().is_some(), "{what}: the reason went away");
    }
}

/// ⏱ THE CONTROL: AN FT8 OVER THE RADIO UNKEYS ENDS AS IT ALWAYS DID. One unkey at its end, TX stays
/// on, nothing is said, and the CQ run's next over keys on the first tick of its slot, 30 s on.
#[test]
fn an_ft8_over_the_radio_unkeys_ends_as_it_always_did() {
    let mut s = Station::new(Unkey::Accepts);
    let end = s.key_the_over();
    s.run_to(end - 20.0);
    assert_eq!(s.unkeys(), 0, "unkeyed before the over's end");
    s.run_to(end + 20.0);
    assert_eq!(s.unkeys(), 1, "the over's end sent one unkey");
    assert!(!s.on_air() && !s.rig.keyed, "the radio was not unkeyed");
    assert!(s.armed(), "TX went off");
    assert_eq!(s.unkey_failed(), None);

    let next = s.slot + 30_000.0;
    s.run_to(next - 20.0);
    assert_eq!(s.keys(), 1, "keyed before its slot");
    assert_eq!(s.unkeys(), 1, "the unkey was sent again");
    s.run_to(next);
    assert_eq!(
        s.keys(),
        2,
        "the next over did not key on the first tick of its slot"
    );
    assert!(s.armed() && s.unkey_failed().is_none());
}

/// ⭐ HAMLIB'S OWN "THE RIG DID NOT ANSWER" TO AN FT8 UNKEY HALTS TX, AS WSJT-X HALTS ON IT: its
/// `error_check` excepts nothing but `RIG_OK`. The radio takes the next unkey, and is unkeyed.
#[test]
fn hamlibs_own_no_answer_to_an_ft8_unkey_halts_tx() {
    for code in [-5, -6, -13, -14] {
        let what = format!("RPRT {code}");
        let mut s = Station::new(Unkey::Fails { code, n: 1 });
        let end = s.key_the_over();
        s.run_to(end + 20.0);
        assert!(
            s.unkeys() >= 2,
            "{what}: premise: the unkey was sent, and again"
        );
        assert!(!s.armed(), "{what}: TX is still on");
        assert!(
            s.unkey_failed().is_some_and(|f| f.why.contains(&what)),
            "{what}: nothing says why: {:?}",
            s.unkey_failed()
        );
        assert!(!s.on_air(), "{what}: the radio was left keyed");
    }
}

/// ⭐ A LATE ANSWER TO AN FT8 UNKEY HALTS NOTHING. Nexus's own PTT deadline has no counterpart in
/// WSJT-X, which waits on Hamlib: a slow radio unkeys late there and nothing halts. The loop sends
/// the unkey again, the radio takes it, and the run goes on.
#[test]
fn a_late_answer_to_an_ft8_unkey_halts_nothing() {
    let mut s = Station::new(Unkey::AnswersLate);
    let end = s.key_the_over();
    s.run_to(end + 20.0);
    assert!(s.unkeys() >= 1, "premise: the over's end sent the unkey");
    assert!(s.armed(), "TX went off");
    assert_eq!(s.unkey_failed(), None);
    s.run_to(end + 1_000.0);
    assert!(!s.on_air() && !s.rig.keyed, "the radio was left keyed");
    assert!(s.armed() && s.unkey_failed().is_none());
}

/// ⏱ THE RULE IS THE SLOT OVERS' ALONE. A voice message whose unkey the radio refuses is left as it
/// was: TX stays on, and the slot overs' line says nothing. The unkey is sent until the radio takes
/// it, as it always was.
#[test]
fn a_voice_message_whose_unkey_the_radio_refuses_is_left_as_it_was() {
    let refuses = Unkey::Fails {
        code: -1,
        n: REFUSALS,
    };
    let mut s = Station::with(refuses, 14_250_000, "USB", |e| {
        e.set_operating_mode("phone", true);
        e.set_frequency(14.250, "20m", "USB");
        e.send_voice(vec![0.05; 12_000]).unwrap();
    });
    s.run_to(s.t + 100.0);
    assert_eq!(s.keys(), 1, "premise: the message keyed");
    let end = s.state.tx_until_ms.expect("premise: the message holds PTT");
    s.run_to(end + 20.0);
    assert!(s.unkeys() >= 1, "premise: its end sent the unkey");
    assert!(s.armed(), "TX went off");
    assert_eq!(
        s.unkey_failed(),
        None,
        "a voice message's unkey is not a slot over's"
    );
    s.sends_the_unkey_until_the_radio_takes_it(end, "voice");
}

/// ⭐ STOP TX WHOSE UNKEY THE RADIO REFUSES SAYS SO, AND THE UNKEY IS SENT UNTIL THE RADIO TAKES IT.
/// WSJT-X's Halt Tx ends the over through the same PTT off as its end does, and a failure there is
/// the same rig failure. TX is already off; now the status bar says the unkey failed. The control:
/// Stop TX on a radio that unkeys sends one unkey and says nothing. Either way Stop TX asks the
/// radio to stop CW once, as it always did, and the failed unkey adds no second ask.
#[test]
fn stop_tx_whose_unkey_the_radio_refuses_says_so_and_the_unkey_goes_on() {
    for unkey in [
        Unkey::Fails {
            code: -1,
            n: REFUSALS,
        },
        Unkey::Accepts,
    ] {
        let mut s = Station::new(unkey);
        s.key_the_over();
        let cut = s.slot + 5_000.0;
        s.run_to(cut - 20.0);
        assert_eq!(s.unkeys(), 0, "premise: the over is on the air");
        let cw_stops = s.sent("\\stop_morse");
        s.engine.lock().unwrap().halt_tx(); // Stop TX
        s.run_to(cut);
        assert_eq!(s.state.tx_until_ms, None, "premise: Stop TX cut the over");
        assert!(s.unkeys() >= 1, "premise: Stop TX sent the unkey");
        assert!(!s.armed(), "premise: Stop TX turned TX off");
        if unkey == Unkey::Accepts {
            assert_eq!(s.unkeys(), 1, "control: one unkey");
            assert!(!s.on_air() && !s.rig.keyed, "control: still keyed");
            assert_eq!(s.unkey_failed(), None, "control");
            assert_eq!(
                s.sent("\\stop_morse") - cw_stops,
                1,
                "control: Stop TX no longer asks the radio to stop CW"
            );
        } else {
            assert!(
                s.unkey_failed().is_some_and(|f| f.why.contains("RPRT -1")),
                "nothing says the unkey failed: {:?}",
                s.unkey_failed()
            );
            s.sends_the_unkey_until_the_radio_takes_it(cut, "Stop TX");
            assert_eq!(
                s.sent("\\stop_morse") - cw_stops,
                1,
                "Stop TX's own CW stop, and no second one for the unkey that failed"
            );
        }
    }
}

/// A logger's HaltTx (`WSJT-X` UDP message type 8), as JTAlert sends it.
fn halt_tx_datagram() -> Vec<u8> {
    let mut w = tempo_net::qds::QdsWriter::new();
    w.put_u32(tempo_net::wsjtx::MAGIC)
        .put_u32(tempo_net::wsjtx::SCHEMA)
        .put_u32(tempo_net::wsjtx::msg_type::HALT_TX)
        .put_utf8(Some("JTAlert"))
        .put_bool(false);
    w.into_bytes()
}

/// ⭐ …AND A LOGGER'S HALTTX THE SAME. WSJT-X's UDP HaltTx presses the same Halt Tx. The control: a
/// HaltTx on a radio that unkeys sends one unkey and says nothing.
#[test]
fn a_loggers_halt_tx_whose_unkey_the_radio_refuses_says_so_and_the_unkey_goes_on() {
    for unkey in [
        Unkey::Fails {
            code: -1,
            n: REFUSALS,
        },
        Unkey::Accepts,
    ] {
        let logger = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let server =
            WsjtxServer::new("127.0.0.1:0".parse().unwrap(), logger.local_addr().unwrap()).unwrap();
        let mut s = Station::new(unkey);
        s.key_the_over();
        let cut = s.slot + 5_000.0;
        s.run_with(Some(&server), cut - 20.0);
        assert_eq!(s.unkeys(), 0, "premise: the over is on the air");
        let cw_stops = s.sent("\\stop_morse");
        logger
            .send_to(&halt_tx_datagram(), server.local_addr().unwrap())
            .unwrap();
        s.run_with(Some(&server), cut);
        assert_eq!(
            s.state.tx_until_ms, None,
            "premise: the HaltTx cut the over"
        );
        assert!(s.unkeys() >= 1, "premise: the HaltTx sent the unkey");
        assert!(!s.armed(), "premise: the HaltTx turned TX off");
        if unkey == Unkey::Accepts {
            assert_eq!(s.unkeys(), 1, "control: one unkey");
            assert!(!s.on_air() && !s.rig.keyed, "control: still keyed");
            assert_eq!(s.unkey_failed(), None, "control");
            // The HaltTx's CW stop goes out on the tick after it.
            s.run_with(Some(&server), cut + 20.0);
            assert_eq!(
                s.sent("\\stop_morse") - cw_stops,
                1,
                "control: the HaltTx no longer asks the radio to stop CW"
            );
        } else {
            assert!(
                s.unkey_failed().is_some_and(|f| f.why.contains("RPRT -1")),
                "nothing says the unkey failed: {:?}",
                s.unkey_failed()
            );
            s.sends_the_unkey_until_the_radio_takes_it(cut, "HaltTx");
            assert_eq!(
                s.sent("\\stop_morse") - cw_stops,
                1,
                "the HaltTx's own CW stop, and no second one"
            );
        }
    }
}

// ── The CW stop after a failed unkey: skipped (operator, 2026-10-05: "Skip it when an unkey
// failed") ──────────────────────────────────────────────────────────────────────────────────
// Every halt asks the radio to stop CW (`\stop_morse`) on the next tick, before that tick's
// unkey. Nothing keys CW while a slot over ends, and a radio that has stopped answering holds the
// command for the whole CAT deadline, the unkey waiting behind it. Stop TX, a logger's HaltTx
// (above) and the refused key's halt (below) still ask.

/// ⭐ A FAILED UNKEY'S HALT SENDS NO CW STOP, SO NOTHING HOLDS UP THE UNKEY AFTER IT. This radio
/// answers the CW stop late, as one that has stopped answering does: the tick after the failure
/// sends its unkey at once.
#[test]
fn a_failed_unkeys_halt_sends_no_cw_stop_so_nothing_holds_up_the_unkey_after_it() {
    let mut s = Station::new(Unkey::Fails {
        code: -1,
        n: REFUSALS,
    });
    s.slow_cw_stop.store(true, Ordering::SeqCst);
    let end = s.key_the_over();
    let cw_stops = s.sent("\\stop_morse");
    s.run_to(end + 20.0);
    assert!(
        !s.armed() && s.unkey_failed().is_some(),
        "premise: the failed unkey halted TX"
    );
    let sent = s.unkeys();
    let tick = std::time::Instant::now();
    s.run_to(s.t); // the next tick, alone
    let took = tick.elapsed();
    assert_eq!(
        s.sent("\\stop_morse") - cw_stops,
        0,
        "the failed unkey's halt asked the radio to stop CW (the next tick took {took:?})"
    );
    assert!(
        s.unkeys() > sent,
        "the next tick sent no unkey: {sent}, then {}",
        s.unkeys()
    );
    assert!(
        took < Duration::from_millis(400),
        "the next tick's unkey waited {took:?}"
    );
}

/// ⏱ THE CONTROL: THE REFUSED KEY'S HALT STILL ASKS THE RADIO TO STOP CW, AS STOP TX DOES.
#[test]
fn a_refused_keys_halt_still_sends_the_cw_stop() {
    let mut s = Station::new(Unkey::Accepts);
    s.refuses_key.store(true, Ordering::SeqCst);
    s.run_to(s.t); // the first tick: the slot boundary's key, refused
    let cw_stops = s.sent("\\stop_morse");
    assert!(s.keys() >= 1, "premise: the slot boundary tried the key");
    assert!(s.backend.played.is_empty(), "premise: nothing was played");
    assert!(!s.armed(), "premise: the refused key halted TX");
    assert!(
        s.engine
            .lock()
            .unwrap()
            .snapshot()
            .radio
            .slot_key_refused
            .is_some(),
        "premise: it was the refused key's halt"
    );
    s.run_to(s.slot + 300.0);
    assert_eq!(
        s.sent("\\stop_morse") - cw_stops,
        1,
        "the refused key's halt no longer asks the radio to stop CW"
    );
}
