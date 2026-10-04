//! The Flex daemon against the SmartSDR simulator (`tempo-flexsim`), over real TCP on loopback:
//! the rigctld shim's verbs as a rigctld client sends them, the slice report and intents through
//! the engine, bring-up and teardown. Every keying test counts the keying commands on the wire.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use tempo_app::engine::slices::{SliceIntent, SliceRefusal};
use tempo_app::engine::Engine;
use tempo_flexsim::server::Event as SimEvent;
use tempo_flexsim::session::{Item, Pattern, Rule};
use tempo_flexsim::{Config as SimConfig, Fault, Foreign, Session as SimSession, Simulator};
use tempo_net::flex::encode::Station;
use tempo_net::flex::session;

use super::*;

/// The longest any single wait may take before a test fails.
const WAIT: Duration = Duration::from_secs(10);

/// Our handle in the bundled session (its first connection).
const OURS: u32 = 0x2B6E_1F40;

fn simulator(session: SimSession, faults: Vec<Fault>) -> Simulator {
    Simulator::start(
        session,
        SimConfig {
            faults,
            // The radio's own keepalive is not under test here.
            keepalive_timeout: Duration::from_secs(120),
            ..SimConfig::default()
        },
    )
    .expect("the simulator starts")
}

/// Production timing, except a shorter unkey deadline and teardown wait.
fn config(previous_handles: Vec<u32>) -> session::Config {
    let mut c = session::Config::new(Station::new(STATION).unwrap());
    c.unkey_deadline_ms = 1500;
    c.teardown_timeout_ms = 1000;
    c.previous_handles = previous_handles;
    c
}

fn daemon(sim: &Simulator) -> FlexDaemon {
    FlexDaemon::start_with(sim.tcp_addr(), 0, config(Vec::new())).expect("the daemon starts")
}

/// `rule` answers `command` with `code`, then sends `lines`.
fn rule(command: &str, code: &str, lines: &[&str]) -> (Pattern, Vec<Rule>) {
    (
        Pattern::Exact(command.to_string()),
        vec![Rule {
            code: code.to_string(),
            message: String::new(),
            items: lines.iter().map(|l| Item::Send(l.to_string())).collect(),
        }],
    )
}

/// The bundled session with `rules` added; an exact rule beats the bundled prefix rules.
fn with(mut s: SimSession, rules: Vec<(Pattern, Vec<Rule>)>) -> SimSession {
    for r in rules {
        s.rules.retain(|(p, _)| *p != r.0);
        s.rules.push(r);
    }
    s
}

/// The radio already has three slices when Nexus subscribes: our transmit slice A, our receive
/// slice B, and slice C of another client.
fn three_slices() -> SimSession {
    with(
        SimSession::v4_gui_client(),
        vec![rule(
            "sub slice all",
            "0",
            &[
                "S{h}|slice 0 in_use=1 RF_frequency=14.074000 mode=DIGU filter_lo=0 \
                 filter_hi=3000 tx=1 client_handle=0x{h} index_letter=A audio_level=50 \
                 audio_mute=0 agc_mode=med nb=0 nr=0 anf=0",
                "S{h}|slice 1 in_use=1 RF_frequency=7.074000 mode=DIGU filter_lo=0 \
                 filter_hi=3000 tx=0 client_handle=0x{h} index_letter=B audio_level=40 \
                 audio_mute=0 agc_mode=slow nb=0 nr=1 anf=0 rit_on=0",
                "S7A3C0001|slice 2 in_use=1 RF_frequency=3.573000 mode=USB tx=0 \
                 client_handle=0x7A3C0001 index_letter=C",
            ],
        )],
    )
}

/// A rigctld client on the shim, as Nexus's own `Rig` is one.
struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Client {
    fn connect(d: &FlexDaemon) -> Client {
        let writer = TcpStream::connect(d.local_addr()).expect("connect to the shim");
        writer.set_read_timeout(Some(WAIT)).unwrap();
        let reader = BufReader::new(writer.try_clone().unwrap());
        Client { reader, writer }
    }

    /// Send one verb and read `lines` lines of reply.
    fn ask(&mut self, verb: &str, lines: usize) -> String {
        writeln!(self.writer, "{verb}").unwrap();
        let mut out = String::new();
        for _ in 0..lines {
            self.reader.read_line(&mut out).expect("a reply line");
        }
        out
    }

    /// Send one verb and read reply lines until one starts with `last`.
    fn ask_until(&mut self, verb: &str, last: &str) -> String {
        writeln!(self.writer, "{verb}").unwrap();
        let mut out = String::new();
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).expect("a reply line");
            out.push_str(&line);
            if line.starts_with(last) || line.is_empty() {
                return out;
            }
        }
    }
}

/// The commands on the wire, pings left out.
fn wire(sim: &Simulator) -> Vec<String> {
    sim.commands().into_iter().filter(|c| c != "ping").collect()
}

fn count(sim: &Simulator, text: &str) -> usize {
    wire(sim).iter().filter(|c| *c == text).count()
}

/// Wait for `done` on the daemon's session.
fn wait_session(d: &FlexDaemon, what: &str, done: impl Fn(&session::Snapshot) -> bool) {
    assert!(
        d.session().wait_until(WAIT, done),
        "timed out waiting for {what}"
    );
}

/// Ask `verb` until it answers `want`, or fail.
fn ask_until_answer(c: &mut Client, verb: &str, want: &str) {
    let deadline = Instant::now() + WAIT;
    loop {
        if c.ask(verb, 1) == want {
            return;
        }
        assert!(Instant::now() < deadline, "{verb} never answered {want:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ── Bring-up and reads ──────────────────────────────────────────────────────────────────────

/// ⭐ A first connect finds no slice of ours, makes one the way GUI clients do — a panadapter,
/// then a slice on it — and the shim serves it: the dial, the mode in rigctld words, the
/// passband, PTT and the only VFO.
#[test]
fn a_first_connect_brings_up_a_slice_and_the_shim_serves_it() {
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let d = daemon(&sim);
    let sent = wire(&sim);
    let at = |prefix: &str| {
        sent.iter()
            .position(|c| c.starts_with(prefix))
            .unwrap_or_else(|| panic!("no {prefix:?} in {sent:?}"))
    };
    assert!(at("client gui") < at("display panafall create x=1024 y=480"));
    assert!(at("display panafall create") < at("slice create pan=0x40000000 freq=14.100000"));
    let mut c = Client::connect(&d);
    assert_eq!(c.ask("f", 1), "14074000\n");
    assert_eq!(c.ask("m", 2), "PKTUSB\n3000\n");
    assert_eq!(c.ask("t", 1), "0\n");
    assert_eq!(c.ask("v", 1), "VFOA\n");
    assert_eq!(c.ask("s", 2), "0\nVFOA\n");
    assert_eq!(c.ask("V VFOB", 1), "RPRT -1\n", "there is no second VFO");
    assert_eq!(c.ask("l RFPOWER", 1), "0.500000\n");
    assert_eq!(c.ask("l AF", 1), "0.500000\n");
    assert_eq!(
        c.ask("u NB", 1),
        "RPRT -11\n",
        "the slice reported no nb yet"
    );
    assert!(d.is_alive());
}

/// The authored capabilities reach the radio loop through its own reader, and a verb the shim
/// does not implement answers what the shared encoder answers any backend that lacks it.
#[test]
fn the_shim_authors_its_caps_and_answers_the_rest_as_the_broker_does() {
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    let caps = crate::baud_ladder::parse_caps(&c.ask_until("\\dump_caps", "Get level:"));
    assert_eq!(caps, shim::authored_caps());
    assert_eq!(caps.rfpower_floor_milli, Some(0));
    assert!(caps.vfo_read_native);
    assert_eq!(caps.split_detect, crate::baud_ladder::SplitDetect::Absent);
    // Byte for byte the encoder's answers for a backend without the verb. A read between them,
    // because three unrecognised verbs in a row cost a client its connection, by design.
    let mock = crate::rigctld_server::tests::MockRig::default();
    for verb in ["l STRENGTH", "J 100", "Z 100", "I 14075000", "R +"] {
        let shared = match crate::rigctld_server::handle_command(verb, &mock) {
            crate::rigctld_server::Handled::Reply(r) => r,
            crate::rigctld_server::Handled::Close => unreachable!(),
        };
        assert_eq!(c.ask(verb, 1), shared, "{verb}");
        assert_eq!(c.ask("f", 1), "14074000\n");
    }
}

// ── The verb map ────────────────────────────────────────────────────────────────────────────

/// ⭐ Each shim write is ONE typed command on the wire, aimed at the served slice: the dial, the
/// mode (`PKTUSB` is `DIGU`), a width placed on the mode's side, RF power, AF and the three
/// noise functions.
#[test]
fn each_shim_write_is_one_typed_command_on_the_wire() {
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    let before = wire(&sim).len();
    for verb in [
        "F 7074000",
        "M USB 0",
        "M PKTUSB 3000",
        "L RFPOWER 0.25",
        "L AF 0.5",
        "U NB 1",
        "U NR 0",
        "U ANF 1",
    ] {
        assert_eq!(c.ask(verb, 1), "RPRT 0\n", "{verb}");
    }
    assert_eq!(
        wire(&sim)[before..],
        [
            "slice tune 0 7.074000",
            "slice set 0 mode=USB",
            "slice set 0 mode=DIGU",
            "filt 0 0 3000",
            "transmit set rfpower=25",
            "slice set 0 audio_level=50",
            "slice set 0 nb=1",
            "slice set 0 nr=0",
            "slice set 0 anf=1",
        ]
    );
}

/// What cannot be expressed is refused before anything is sent: a mode the radio does not
/// offer, a width the mode has no side for, a level out of range.
#[test]
fn a_write_the_radio_cannot_take_is_refused_before_anything_is_sent() {
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    let before = wire(&sim).len();
    assert_eq!(c.ask("M CWR 0", 1), "RPRT -1\n");
    assert_eq!(c.ask("M RTTY 500", 1), "RPRT -1\n");
    assert_eq!(c.ask("L RFPOWER 1.5", 1), "RPRT -1\n");
    assert_eq!(c.ask("L AF -0.1", 1), "RPRT -1\n");
    assert_eq!(c.ask("F 0", 1), "RPRT -1\n");
    assert!(
        wire(&sim)[before..].is_empty(),
        "{:?}",
        &wire(&sim)[before..]
    );
}

// ── Transmit ────────────────────────────────────────────────────────────────────────────────

/// ⭐ `T 1` keys through the core's admission (`xmit 1`), `t` follows the radio's interlock, and
/// `T 0` unkeys: keyed clears only on the interlock readback, never on the reply to `xmit 0`.
#[test]
fn t_keys_through_admission_and_the_readback_ends_it() {
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    assert_eq!(count(&sim, "xmit 1"), 1);
    ask_until_answer(&mut c, "t", "1\n");
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    wait_session(&d, "the unkey confirmed", |s| !s.keyed);
    ask_until_answer(&mut c, "t", "0\n");
    assert_eq!(count(&sim, "xmit 1"), 1, "one key, the one asked for");
    assert!(count(&sim, "xmit 0") >= 1);
}

/// `t` follows the radio's interlock, not the session's proof of the unkey: once the radio reports
/// READY after our `xmit 0`, `t` reads 0 although the readback has not finished. The radio loop
/// reads `t` only while Nexus is idle, to see SOMEONE ELSE keying, and the tail of our own over
/// is not that. The window is widened to three seconds here so the read lands inside it.
#[test]
fn t_reads_the_interlock_not_the_tail_of_our_own_readback() {
    let mut session = SimSession::v4_gui_client();
    for (pattern, rules) in &mut session.rules {
        if *pattern == Pattern::Exact("xmit 0".into()) {
            for item in rules.iter_mut().flat_map(|r| r.items.iter_mut()) {
                if *item == Item::Wait(430) {
                    *item = Item::Wait(3000);
                }
            }
        }
    }
    let sim = simulator(session, vec![]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    ask_until_answer(&mut c, "t", "1\n");
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    wait_session(&d, "the radio READY with the readback unfinished", |s| {
        s.keyed
            && s.model
                .interlock
                .sample
                .as_ref()
                .is_some_and(|i| i.state == "READY")
    });
    assert_eq!(c.ask("t", 1), "0\n");
    assert!(
        d.session().snapshot().keyed,
        "the read landed inside the readback window"
    );
    wait_session(&d, "the unkey confirmed", |s| !s.keyed);
}

/// ⭐ The starts admission cannot prove the end of are refused, and NOTHING keying reaches the
/// wire: the ATU, the CW keyer. Stops are never gated, and with nothing of ours on the air they
/// send nothing at all: Nexus never stops another client's transmission.
#[test]
fn refused_starts_never_reach_the_wire_and_stops_send_only_for_ours() {
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("U TUNER 2", 1), "RPRT -1\n");
    assert_eq!(c.ask("b CQ TEST", 1), "RPRT -1\n");
    assert_eq!(c.ask("\\stop_morse", 1), "RPRT 0\n");
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    let sent = wire(&sim);
    for keying in ["atu start", "cwx send", "xmit 1", "xmit 0", "cwx clear"] {
        assert!(
            !sent.iter().any(|c| c.starts_with(keying)),
            "{keying} on the wire: {sent:?}"
        );
    }
    assert_eq!(c.ask("U TUNER 0", 1), "RPRT -11\n", "no sender, no meaning");
}

/// ⭐ Another client's transmit slice is never keyed: our own slice is served receive-only and
/// `T 1` is refused, with no `xmit 1` on the wire. The control is
/// [`t_keys_through_admission_and_the_readback_ends_it`], where the same verb keys.
#[test]
fn another_clients_transmit_slice_is_never_keyed() {
    let sim = simulator(
        SimSession::v4_gui_client(),
        vec![Fault::ForeignClient(Foreign::default())],
    );
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    assert_eq!(c.ask("f", 1), "14074000\n", "our own slice, never theirs");
    assert_eq!(c.ask("T 1", 1), "RPRT -1\n");
    assert_eq!(count(&sim, "xmit 1"), 0);
    let foreign = Foreign::default();
    assert!(
        !wire(&sim).iter().any(|c| foreign.is_named_by(c)),
        "nothing aimed at the other client's objects: {:?}",
        wire(&sim)
    );
}

// ── The receiver set and the intents ────────────────────────────────────────────────────────

/// A Flex station's engine: model 2036, the client's report observed.
fn flex_engine(d: &FlexDaemon) -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    let mut s = e.settings().clone();
    s.rig_model = 2036;
    e.apply_settings(s);
    e.observe_flex_slices(Some(d.slices()));
    e
}

fn receiver_set(e: &Engine) -> serde_json::Value {
    serde_json::to_value(e.snapshot()).unwrap()["radio"]["receivers"]["set"].clone()
}

/// ⭐ THE ROUND TRIP. The radio's slices reach the snapshot as the indexed receiver set, each
/// slice keyed by the radio's index with its owner and transmit flag; an intent for our other
/// slice goes engine → loop → client → radio as one typed command, and the radio's own report
/// brings the result back to the snapshot.
#[test]
fn the_receiver_set_round_trips_from_the_radio_to_the_snapshot_and_back() {
    let echo = |cmd: &str, status: &str| rule(cmd, "0", &[status]);
    let session = with(
        three_slices(),
        vec![
            echo(
                "slice tune 1 7.035500",
                "S{h}|slice 1 RF_frequency=7.035500",
            ),
            echo(
                "slice set 1 mode=USB",
                "S{h}|slice 1 mode=USB filter_lo=100 filter_hi=2900",
            ),
            echo(
                "filt 1 200 2700",
                "S{h}|slice 1 filter_lo=200 filter_hi=2700",
            ),
            echo("slice set 1 agc_mode=fast", "S{h}|slice 1 agc_mode=fast"),
            echo("slice set 1 audio_level=25", "S{h}|slice 1 audio_level=25"),
            echo("slice set 1 audio_mute=1", "S{h}|slice 1 audio_mute=1"),
            echo("slice set 1 nb=1", "S{h}|slice 1 nb=1"),
            echo("slice set 1 nr=0", "S{h}|slice 1 nr=0"),
            echo("slice set 1 anf=1", "S{h}|slice 1 anf=1"),
        ],
    );
    let sim = simulator(session, vec![]);
    let d = daemon(&sim);
    assert!(
        !wire(&sim).iter().any(|c| c.starts_with("slice create")),
        "our transmit slice was there already: no bring-up"
    );
    let mut e = flex_engine(&d);
    let set = receiver_set(&e);
    assert_eq!(set.as_array().map(Vec::len), Some(3), "{set}");
    assert_eq!(
        (
            &set[0]["id"],
            &set[0]["index"],
            &set[0]["transmits"],
            &set[0]["owner"]
        ),
        (&"slice".into(), &0.into(), &true.into(), &"ours".into())
    );
    assert_eq!(set[1]["letter"], "B");
    assert_eq!(set[1]["dialMhz"], 7.074);
    assert_eq!(set[1]["band"], "40m");
    assert_eq!(set[1]["rigMode"], "PKTUSB");
    assert_eq!(set[1]["agc"], "slow");
    assert_eq!(set[1]["afGain"], 0.4_f32 as f64);
    assert_eq!((&set[1]["nr"], &set[1]["ritHz"]), (&true.into(), &0.into()));
    assert_eq!(set[2]["owner"], "foreign");
    assert_eq!(set[2]["letter"], "C");

    // Engine → loop → client → radio: one typed command per intent, in order.
    let before = wire(&sim).len();
    for intent in [
        SliceIntent::Tune { mhz: 7.0355 },
        SliceIntent::Mode { mode: "USB".into() },
        SliceIntent::Filter {
            low_hz: 200,
            high_hz: 2700,
        },
        SliceIntent::Agc {
            speed: "fast".into(),
        },
        SliceIntent::AfGain { gain: 0.25 },
        SliceIntent::Mute { muted: true },
        SliceIntent::NoiseBlanker { on: true },
        SliceIntent::NoiseReduction { on: false },
        SliceIntent::AutoNotch { on: true },
    ] {
        e.request_slice(1, intent.clone())
            .unwrap_or_else(|r| panic!("{intent:?}: {r}"));
        for w in e.take_slice_writes() {
            let outcome = d.apply(w.index, &w.intent);
            assert_eq!(outcome, Ok(()), "{:?}", w.intent);
            e.observe_slice_write(&w, outcome);
        }
    }
    assert_eq!(
        wire(&sim)[before..],
        [
            "slice tune 1 7.035500",
            "slice set 1 mode=USB",
            "filt 1 200 2700",
            "slice set 1 agc_mode=fast",
            "slice set 1 audio_level=25",
            "slice set 1 audio_mute=1",
            "slice set 1 nb=1",
            "slice set 1 nr=0",
            "slice set 1 anf=1",
        ]
    );

    // The radio → the snapshot: its report is the readback.
    wait_session(&d, "the radio's report of every change", |s| {
        s.model.slices.get(&1).is_some_and(|b| b.anf == Some(true))
    });
    e.observe_flex_slices(Some(d.slices()));
    let b = &receiver_set(&e)[1];
    assert_eq!(b["dialMhz"], 7.0355);
    assert_eq!(b["rigMode"], "USB");
    assert_eq!(b["filterWidthHz"], 2500);
    assert_eq!(b["agc"], "fast");
    assert_eq!(b["afGain"], 0.25);
    assert_eq!(
        (&b["muted"], &b["nb"], &b["nr"], &b["notch"]),
        (&true.into(), &true.into(), &false.into(), &true.into())
    );
    assert_eq!(e.last_slice_refusal(), None);
}

/// ⭐ The two doors. The engine refuses intents for the transmit slice and another client's
/// slice; the client refuses them again at the wire, against the radio's latest report. Nothing
/// reaches the radio either way.
#[test]
fn the_transmit_slice_and_another_clients_slice_are_refused_at_both_doors() {
    let sim = simulator(three_slices(), vec![]);
    let d = daemon(&sim);
    let mut e = flex_engine(&d);
    assert_eq!(
        e.request_slice(0, SliceIntent::Tune { mhz: 14.2 }),
        Err(SliceRefusal::TransmitSlice(0))
    );
    assert_eq!(
        e.request_slice(2, SliceIntent::Mute { muted: true }),
        Err(SliceRefusal::NotOurs(2))
    );
    let before = wire(&sim).len();
    assert!(d.apply(0, &SliceIntent::Tune { mhz: 14.2 }).is_err());
    assert!(d.apply(2, &SliceIntent::Mute { muted: true }).is_err());
    assert!(d.apply(7, &SliceIntent::Mute { muted: true }).is_err());
    assert!(
        wire(&sim)[before..].is_empty(),
        "{:?}",
        &wire(&sim)[before..]
    );
}

// ── Start and teardown ──────────────────────────────────────────────────────────────────────

/// The radio refuses Nexus as a GUI client: the start fails with the reason, so the radio loop
/// falls back to SmartSDR CAT, and nothing was made on the radio.
#[test]
fn a_refused_registration_fails_the_start_with_the_reason() {
    let session = with(
        SimSession::v4_gui_client(),
        vec![rule("client gui", "F3000001", &[])],
    );
    let sim = simulator(session, vec![]);
    let err = FlexDaemon::start_with(sim.tcp_addr(), 0, config(Vec::new()))
        .err()
        .expect("the radio refused registration");
    assert!(err.to_string().contains("refused Nexus"), "{err}");
    assert!(
        !wire(&sim)
            .iter()
            .any(|c| c.starts_with("display panafall create") || c.starts_with("slice create")),
        "{:?}",
        wire(&sim)
    );
}

/// ⭐ Teardown unkeys FIRST, then removes the slice and panadapter it made (the waterfall too),
/// then closes.
#[test]
fn dropping_the_daemon_unkeys_first_then_removes_what_it_made() {
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    let keyed_at = wire(&sim).len();
    drop(d);
    let after = wire(&sim)[keyed_at..].to_vec();
    let at = |text: &str| {
        after
            .iter()
            .position(|c| c == text)
            .unwrap_or_else(|| panic!("no {text:?} in {after:?}"))
    };
    assert!(at("xmit 0") < at("slice remove 0"), "{after:?}");
    assert!(at("slice remove 0") < at("display pan remove 0x40000000"));
    assert!(at("display pan remove 0x40000000") < at("display panafall remove 0x42000000"));
    assert!(
        sim.wait_for(WAIT, |log| log.iter().any(|l| matches!(
            l.event,
            SimEvent::Closed {
                by: tempo_flexsim::Closer::Client,
                ..
            }
        ))),
        "the session closed"
    );
}

/// ⭐ A daemon that replaces a lost one knows the lost session's handle: when that session's
/// transmitter is still keyed after a disconnect mid-over, the new one will not key under it, says
/// so, and can stop it — which it could not if the handle were forgotten (a stop never ends
/// another client's transmission).
#[test]
fn a_replacement_daemon_can_stop_the_lost_sessions_stuck_transmitter() {
    // The radio reports itself idle when Nexus subscribes, and the new slice reports no
    // interlock: the simulator remembers who holds the transmitter only in its answer to
    // `sub tx all`, which it ends with that holder, so a scripted idle line after it would
    // describe a radio that is not idle.
    let mut session = SimSession::v4_gui_client();
    for (pattern, rules) in &mut session.rules {
        let ready = "S0|interlock tx_client_handle=0x00000000 state=READY reason= source= \
                     tx_allowed=1 amplifier=";
        match pattern {
            Pattern::Prefix(p) if p == "slice create " => {
                for r in rules.iter_mut() {
                    r.items
                        .retain(|i| !matches!(i, Item::Send(l) if l.contains("|interlock ")));
                }
            }
            Pattern::Exact(p) if p == "sub tx all" => {
                for r in rules.iter_mut() {
                    for item in r.items.iter_mut() {
                        if matches!(item, Item::Send(l) if l.contains("state=RECEIVE")) {
                            *item = Item::Send(ready.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let sim = simulator(
        session,
        vec![Fault::DisconnectMidOver {
            after: Duration::from_millis(100),
            radio_stays_keyed: true,
        }],
    );
    let first = FlexDaemon::start(sim.tcp_addr(), 0).expect("the first daemon starts");
    let mut c = Client::connect(&first);
    wait_session(&first, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    wait_session(&first, "the session lost mid-over", |s| {
        s.phase == Phase::Closed
    });
    assert!(!first.is_alive());
    drop(c);
    drop(first);
    // What the radio loop does with a dead daemon: start another.
    let second = FlexDaemon::start(sim.tcp_addr(), 0).expect("the second daemon starts");
    let mut c = Client::connect(&second);
    assert_eq!(
        c.ask("T 1", 1),
        "RPRT -1\n",
        "never key under the lost transmitter"
    );
    let deadline = Instant::now() + WAIT;
    while second.alarm().is_none() {
        assert!(Instant::now() < deadline, "the operator was never told");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(second.alarm().unwrap().contains(&format!("{OURS:08X}")));
    let unkeys = count(&sim, "xmit 0");
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    assert_eq!(
        count(&sim, "xmit 0"),
        unkeys + 1,
        "the stop reached the lost session's transmitter"
    );
    assert_eq!(
        count(&sim, "xmit 1"),
        1,
        "one key in all: the first session's"
    );
}
