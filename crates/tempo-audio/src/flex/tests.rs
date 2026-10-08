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

/// [`count`], once the radio has logged `text` at least `n` times. The shim answers a key or a
/// stop once the command is written to the session's socket, which can be before the radio has
/// read it: a count taken straight after the answer can miss it.
fn wait_count(sim: &Simulator, text: &str, n: usize) -> usize {
    sim.wait_for(WAIT, |log| {
        log.iter()
            .filter(|l| matches!(&l.event, SimEvent::Command { text: t, .. } if t == text))
            .count()
            >= n
    });
    count(sim, text)
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
    assert_eq!(wait_count(&sim, "xmit 1", 1), 1);
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
/// is not that. The window is widened to three seconds here so the read lands inside it, and the
/// session's unkey deadline to eight, so the widened window is still a confirmed unkey.
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
    let mut slow = config(Vec::new());
    slow.unkey_deadline_ms = 8000;
    let d = FlexDaemon::start_with(sim.tcp_addr(), 0, slow).expect("the daemon starts");
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

/// Look at the daemon as the radio loop does, once a tick, until it reads dead.
fn wait_dead(d: &FlexDaemon) {
    let deadline = Instant::now() + WAIT;
    while d.is_alive() {
        assert!(Instant::now() < deadline, "the session never ended");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// ⭐ The radio never confirms the unkey: past the deadline the session sends `xmit 0` again and
/// closes, and the operator is told the radio may still be transmitting. The radio loop reads the
/// alarm once, when it first sees the daemon dead (`crate::service`, `daemon_died`), and the
/// session publishes its closed state before it sends the events that carry the alarm, so that one
/// read must already find it.
#[test]
fn an_unconfirmed_unkey_is_the_alarm_the_radio_loop_reads() {
    let sim = simulator(SimSession::v4_gui_client(), vec![Fault::StuckTransmit]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    ask_until_answer(&mut c, "t", "1\n");
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    wait_dead(&d);
    let alarm = d.alarm();
    assert!(
        alarm
            .as_deref()
            .is_some_and(|a| a.contains("did not confirm the unkey")),
        "{alarm:?}"
    );
}

/// The control: a confirmed unkey raises no alarm, past the unkey deadline or once the session
/// has ended.
#[test]
fn a_confirmed_unkey_raises_no_alarm() {
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    ask_until_answer(&mut c, "t", "1\n");
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    wait_session(&d, "the unkey confirmed", |s| !s.keyed);
    // A whole deadline after the confirmation is past the deadline the stop started.
    std::thread::sleep(Duration::from_millis(config(Vec::new()).unkey_deadline_ms));
    assert!(d.is_alive());
    assert_eq!(d.alarm(), None);
    // The radio goes away.
    drop(sim);
    wait_dead(&d);
    assert_eq!(d.alarm(), None);
}

/// ⭐ The session is LOST during an over (the radio drops the connection mid-over and stays keyed):
/// nothing confirmed the unkey, so the radio may still be transmitting, and the operator is told so,
/// in the words of what happened. The end of the session alone was only logged.
#[test]
fn a_session_lost_mid_over_is_the_alarm_the_radio_loop_reads() {
    let sim = simulator(
        SimSession::v4_gui_client(),
        vec![Fault::DisconnectMidOver {
            after: Duration::from_millis(100),
            radio_stays_keyed: true,
        }],
    );
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    wait_dead(&d);
    let alarm = d.alarm();
    assert!(
        alarm.as_deref().is_some_and(|a| a.starts_with(
            "the connection to the radio was lost during a transmission — it may still be \
             transmitting"
        )),
        "{alarm:?}"
    );
}

/// …and the radio STOPS ANSWERING during an over. The first missed ping unkeys at once and says so,
/// but the radio never answers again, and the fifth miss ends the session four seconds later: inside
/// the production five-second unkey deadline, so the unkey is still unconfirmed and the session never
/// escalates. The missed-ping notice alone says Nexus sent the unkey, which reads as handled; the
/// operator must read that the radio may still be transmitting.
#[test]
fn a_session_that_stops_answering_mid_over_is_the_alarm_the_radio_loop_reads() {
    let sim = simulator(
        SimSession::v4_gui_client(),
        vec![
            // Pings 1 and 2 answered; none from the third on. The key goes in before the first miss.
            Fault::DropPings {
                first: 3,
                count: 100,
            },
            // The radio acknowledges the unkey and never shows it: the readback cannot prove it.
            Fault::StuckTransmit,
        ],
    );
    let mut config = session::Config::new(Station::new(STATION).unwrap());
    config.teardown_timeout_ms = 1000;
    let d = FlexDaemon::start_with(sim.tcp_addr(), 0, config).expect("the daemon starts");
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    wait_dead(&d);
    // These words come only from a session that the keepalive ended while keyed: an escalation
    // would have ended it as an unconfirmed unkey, in that alarm's words.
    let alarm = d.alarm();
    assert!(
        alarm.as_deref().is_some_and(|a| a.starts_with(
            "the radio stopped answering during a transmission — it may still be transmitting"
        )),
        "{alarm:?}"
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
        wait_count(&sim, "xmit 0", unkeys + 1),
        unkeys + 1,
        "the stop reached the lost session's transmitter"
    );
    assert_eq!(
        count(&sim, "xmit 1"),
        1,
        "one key in all: the first session's"
    );
}

// ── Audio: DAX on the one session ────────────────────────────────────────────────────────────

use std::sync::Arc as AudioArc;

use tempo_flexsim::{vita as sim_vita, Content, ForeignDax, Start, Stream};

/// The bundled session's DAX receive and transmit stream ids.
const DAX_RX: u32 = 0x0400_0001;
const DAX_TX: u32 = 0x8400_0000;

/// In production the daemon's UDP goes to the radio's own ports: DAX TX to its VITA-49 port 4991
/// (not 4993, where the older native path sent it), the registration datagram to 4992.
#[test]
fn a_radios_dax_goes_to_its_vita_port() {
    let radio: std::net::SocketAddr = "192.0.2.20:4992".parse().unwrap();
    let o = Options::for_radio(radio);
    assert_eq!(o.vita, "192.0.2.20:4991".parse().unwrap());
    assert_eq!(o.registration, "192.0.2.20:4992".parse().unwrap());
}

/// A daemon whose UDP goes to the simulator, with a memory of its own.
fn audio_daemon(sim: &Simulator, memory: AudioArc<routing::FileMemory>) -> FlexDaemon {
    FlexDaemon::start_full(
        sim.tcp_addr(),
        0,
        config(Vec::new()),
        Options {
            vita: sim.udp_addr(),
            registration: sim.udp_addr(),
            memory,
        },
    )
    .expect("the daemon starts")
}

fn memory() -> AudioArc<routing::FileMemory> {
    AudioArc::new(routing::FileMemory::new(None))
}

/// A simulator that streams a DAX receive tone once a `dax_rx` stream is created.
fn with_rx_tone(session: SimSession, faults: Vec<Fault>) -> Simulator {
    Simulator::start(
        session,
        SimConfig {
            faults,
            streams: vec![Stream {
                stream_id: DAX_RX,
                content: Content::DaxAudio {
                    class: sim_vita::class::AUDIO_F32_STEREO,
                    tone_hz: 1000.0,
                    amplitude: 0.5,
                },
                period: Stream::dax_period(),
                ticks: None,
                start: Start::After("stream create type=dax_rx".into()),
                until: Some("stream remove 0x04000001".into()),
            }],
            keepalive_timeout: Duration::from_secs(120),
        },
    )
    .expect("the simulator starts")
}

/// Poll `f` until it answers, or fail.
fn eventually<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Run the routing at a quiet point, as the radio loop does at the end of a quiet tick, until
/// the wire shows `want`.
fn route_until(d: &FlexDaemon, sim: &Simulator, browser_voice: bool, want: &str, times: usize) {
    eventually(&format!("{want} x{times}"), || {
        d.sync_tx_routing(browser_voice);
        (count(sim, want) >= times).then_some(())
    });
}

/// Run the routing at quiet points for `ms`, writing whatever it wants to.
fn route_for(d: &FlexDaemon, browser_voice: bool, ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        d.sync_tx_routing(browser_voice);
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The DAX TX datagrams the daemon sent the radio (the registration byte left out), with when
/// they arrived.
fn dax_tx_packets(sim: &Simulator) -> Vec<(Duration, Vec<u8>)> {
    sim.log()
        .into_iter()
        .filter_map(|l| match l.event {
            SimEvent::UdpIn { bytes, .. } if bytes.len() > 1 => Some((l.at, bytes)),
            _ => None,
        })
        .collect()
}

/// The left channel of each DAX TX packet, in order, after checking the packet whole.
fn left_samples(packets: &[(Duration, Vec<u8>)]) -> Vec<f32> {
    let mut out = Vec::new();
    for (_, bytes) in packets {
        let v = tempo_net::flexvita::parse_vita(bytes).expect("a VITA-49 packet");
        let stereo = tempo_net::flex::streams::decode_dax_audio(v.packet_class.unwrap(), v.payload)
            .expect("whole float32 stereo frames");
        for lr in stereo.chunks_exact(2) {
            assert_eq!(lr[0], lr[1], "the same audio on both sides");
            out.push(lr[0]);
        }
    }
    out
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// A tone at the modem rate.
fn tone(seconds: f32, amplitude: f32) -> Vec<f32> {
    (0..(seconds * 12_000.0) as usize)
        .map(|i| amplitude * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 12_000.0).sin())
        .collect()
}

/// Native audio on, the DAX source written for the slice's digital mode, and the transmit route
/// up: what a station on DAX transmit looks like before its first over.
fn dax_tx_ready(sim: &Simulator, d: &FlexDaemon) {
    d.set_native_audio(true);
    route_until(d, sim, false, "transmit set dax=1", 1);
    eventually("the transmit route", || d.tx_route_ready().then_some(()));
}

/// ⭐ Native audio rides the client's ONE session: it registers a UDP port, sends the one-byte
/// registration datagram, gives our slice a DAX channel, creates that channel's receive stream,
/// and the served slice's audio arrives at 12 kHz. Turned off, the stream goes after the broker's
/// grace window, not before.
#[test]
fn native_audio_streams_the_served_slice_on_the_one_session() {
    let sim = with_rx_tone(SimSession::v4_gui_client(), vec![]);
    let d = audio_daemon(&sim, memory());
    let port = eventually("client udpport", || {
        wire(&sim)
            .iter()
            .find_map(|c| c.strip_prefix("client udpport ").map(str::to_string))
    });
    assert!(port.parse::<u16>().is_ok_and(|p| p != 0));
    // The daemon has sent the datagram by the time it is up, but the simulator logs it on its own
    // UDP thread, which may not have run yet: wait for it, as `wait_count` does for a command.
    assert!(
        sim.wait_for(WAIT, |log| log.iter().any(
            |l| matches!(&l.event, SimEvent::UdpIn { bytes, from } if *bytes == [0u8] && from.port().to_string() == port)
        )),
        "the registration datagram, from the registered port"
    );
    d.set_native_audio(true);
    let mut audio = Vec::new();
    eventually("half a second of receive audio", || {
        audio.extend(d.take_audio());
        (audio.len() >= 6_000).then_some(())
    });
    let sent = wire(&sim);
    let at = |text: &str| {
        sent.iter()
            .position(|c| c == text)
            .unwrap_or_else(|| panic!("no {text:?} in {sent:?}"))
    };
    assert!(at("slice set 0 dax=1") < at("stream create type=dax_rx dax_channel=1"));
    assert_eq!(d.rx_streams(), [(DAX_RX, 1u8)].into_iter().collect());
    // The tone, at its level: 0.5 peak (allow the resampler's settling at the start).
    let steady = &audio[1_000..];
    assert!((peak(steady) - 0.5).abs() < 0.05, "peak {}", peak(steady));
    let connections = sim
        .events()
        .iter()
        .filter(|e| matches!(e, SimEvent::Connected { .. }))
        .count();
    assert_eq!(connections, 1, "one session per radio");
    // Off: the stream goes, after the grace window.
    let off = Instant::now();
    d.set_native_audio(false);
    eventually("stream remove", || {
        (count(&sim, "stream remove 0x04000001") == 1).then_some(())
    });
    assert!(
        off.elapsed() >= Duration::from_millis(tempo_net::flex::streams::REMOVAL_GRACE_MS - 100),
        "removed after {:?}, inside the grace window",
        off.elapsed()
    );
}

/// ⭐ THE REGISTRATION DATAGRAM GOES AFTER REGISTRATION, JUST BEFORE `client udpport`, where
/// upstream sends it (port plan §4.5, step 7). The radio learns our UDP endpoint from its source,
/// which firmware that answers `client udpport` with "not supported" needs. The client sent it
/// after its whole bring-up, about 46 ms after the command, and then right after the bind, before
/// the session even connected, to a radio with no client of ours yet.
///
/// The simulator logs datagrams on its own UDP thread, beside each connection's reading thread,
/// so the test waits on the log, never reads it once, and asserts only the orders the log can
/// carry: a radio that never registers Nexus is sent nothing, and a radio that does is sent the
/// datagram after the `client gui` command, which Nexus can only answer once the radio has. Its
/// place among the commands, after `mic list` and before `client udpport`, is the session's own
/// order, pinned in the session's tests
/// (`the_registration_datagram_goes_after_registration_just_before_client_udpport`).
#[test]
fn the_registration_datagram_follows_the_registration() {
    fn udp_in(l: &tempo_flexsim::server::Logged, payload: &[u8]) -> bool {
        matches!(&l.event, SimEvent::UdpIn { bytes, .. } if bytes.as_slice() == payload)
    }
    fn command(l: &tempo_flexsim::server::Logged, prefix: &str) -> bool {
        matches!(&l.event, SimEvent::Command { text, .. } if text.starts_with(prefix))
    }
    let datagram = |l: &tempo_flexsim::server::Logged| udp_in(l, &[0]);

    // A radio that takes the connection and hangs up without a word: Nexus never registered.
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    silent.set_nonblocking(true).unwrap();
    let radio = silent.local_addr().unwrap();
    let options = Options {
        vita: sim.udp_addr(),
        registration: sim.udp_addr(),
        memory: memory(),
    };
    let start = std::thread::spawn(move || {
        FlexDaemon::start_full(radio, 0, config(Vec::new()), options).is_ok()
    });
    drop(eventually("Nexus connecting", || silent.accept().ok()));
    assert!(
        !start.join().unwrap(),
        "premise: a radio that never answered registered Nexus"
    );
    // The simulator reads its UDP port in order: once a datagram sent now is logged, any the start
    // sent is logged before it.
    let after = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    after.send_to(&[0xFF, 0xFF], sim.udp_addr()).unwrap();
    assert!(
        sim.wait_for(WAIT, |log| log.iter().any(|l| udp_in(l, &[0xFF, 0xFF]))),
        "premise: the simulator logs what reaches its UDP port"
    );
    assert!(
        !sim.log().iter().any(datagram),
        "the registration datagram went to a radio that never registered Nexus"
    );

    // A radio that registers Nexus.
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let _d = audio_daemon(&sim, memory());
    assert!(
        sim.wait_for(WAIT, |log| log.iter().any(datagram)
            && log.iter().any(|l| command(l, "client udpport "))),
        "the registration datagram never came"
    );
    let log = sim.log();
    let gui = log
        .iter()
        .position(|l| command(l, "client gui"))
        .expect("premise: Nexus registered");
    let sent = log.iter().position(datagram).unwrap();
    assert!(
        gui < sent,
        "the registration datagram went before the radio registered Nexus"
    );
}

/// The control for the test above: with native audio off, nothing about DAX reaches the wire.
#[test]
fn without_native_audio_no_dax_command_is_sent() {
    let sim = with_rx_tone(SimSession::v4_gui_client(), vec![]);
    let d = audio_daemon(&sim, memory());
    route_for(&d, false, 800);
    assert!(d.take_audio().is_empty());
    let sent = wire(&sim);
    assert!(
        !sent.iter().any(|c| c.starts_with("stream create")
            || c.starts_with("transmit set dax")
            || (c.starts_with("slice set") && c.contains(" dax="))),
        "a DAX command with native audio off: {sent:?}"
    );
    assert!(!d.tx_route_ready());
}

/// No idle streams: each slice of OURS in use gets a channel no other slice uses, and a stream for
/// that channel; another client's slice is never touched.
#[test]
fn each_slice_of_ours_in_use_gets_its_own_channel_and_no_other_does() {
    let session = with(
        three_slices(),
        vec![
            rule("slice set 1 dax=2", "0", &["S{h}|slice 1 dax=2"]),
            (
                Pattern::Exact("stream create type=dax_rx dax_channel=2".into()),
                vec![Rule {
                    code: "0".into(),
                    message: "0x04000002".into(),
                    items: vec![Item::Send(
                        "S{h}|stream 0x04000002 type=dax_rx dax_channel=2 slice=1 \
                         client_handle=0x{h} ip={peer}"
                            .into(),
                    )],
                }],
            ),
        ],
    );
    let sim = simulator(session, vec![]);
    let d = audio_daemon(&sim, memory());
    d.set_native_audio(true);
    eventually("two receive streams", || {
        (d.rx_streams().len() == 2).then_some(())
    });
    assert_eq!(
        d.rx_streams(),
        [(0x0400_0001, 1u8), (0x0400_0002, 2u8)]
            .into_iter()
            .collect()
    );
    let sent = wire(&sim);
    assert_eq!(count(&sim, "slice set 0 dax=1"), 1, "{sent:?}");
    assert_eq!(count(&sim, "slice set 1 dax=2"), 1, "{sent:?}");
    assert_eq!(
        count(&sim, "slice set 1 dax=1"),
        0,
        "two slices offered one channel: {sent:?}"
    );
    assert!(
        !sent.iter().any(|c| c.starts_with("slice set 2")),
        "another client's slice was touched: {sent:?}"
    );
    assert!(
        !sent
            .iter()
            .any(|c| c.starts_with("stream create type=dax_rx dax_channel=")
                && !c.ends_with('1')
                && !c.ends_with('2')),
        "a stream for a channel no slice uses: {sent:?}"
    );
}

/// The broker's recreate: a receive stream the radio drops while our slice still holds its
/// channel comes back, after the recreate delay and not before.
#[test]
fn a_receive_stream_the_radio_drops_comes_back() {
    let sim = with_rx_tone(
        SimSession::v4_gui_client(),
        vec![Fault::DropDaxRx {
            stream_id: DAX_RX,
            after: Duration::from_millis(300),
        }],
    );
    let d = audio_daemon(&sim, memory());
    d.set_native_audio(true);
    eventually("a second create", || {
        (count(&sim, "stream create type=dax_rx dax_channel=1") == 2).then_some(())
    });
    let log = sim.log();
    let removed = log
        .iter()
        .find(|l| matches!(&l.event, SimEvent::Sent { line, .. } if line == "S0|stream 0x04000001 removed"))
        .expect("the radio's removal")
        .at;
    let again = log
        .iter()
        .filter(|l| matches!(&l.event, SimEvent::Command { text, .. } if text == "stream create type=dax_rx dax_channel=1"))
        .nth(1)
        .expect("the recreate")
        .at;
    assert!(
        again >= removed + Duration::from_millis(tempo_net::flex::streams::RECREATE_DELAY_MS - 50),
        "recreated {:?} after the removal",
        again - removed
    );
    // And the audio comes back on it.
    let _ = d.take_audio();
    eventually("audio again", || (!d.take_audio().is_empty()).then_some(()));
}

/// ⭐ DAX TX in the TESTED format, to the configured VITA-49 destination (the radio's 4991 in
/// production: `Options::for_radio`), at the operator's TX level, paced in real time, and only
/// while our key is held.
#[test]
fn dax_tx_is_the_tested_format_at_the_operators_level_and_paced() {
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let d = audio_daemon(&sim, memory());
    dax_tx_ready(&sim, &d);
    let tee = d.tx_tee().expect("the route");
    tee.set_level(0.25);
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    tee.feed(&tone(0.5, 0.8));
    let packets = eventually("half a second of DAX TX", || {
        let p = dax_tx_packets(&sim);
        (p.len() >= 90).then_some(p)
    });
    for (i, (_, bytes)) in packets.iter().enumerate() {
        assert_eq!(bytes.len(), (7 + 256) * 4, "128 stereo float32 frames");
        let w0 = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        assert_eq!(w0 >> 28, 1, "IF data with a stream id");
        assert_eq!((w0 >> 22) & 3, 3, "TSI 3");
        assert_eq!((w0 >> 20) & 3, 1, "TSF 1");
        assert_eq!(
            (w0 >> 16) & 0xF,
            (i % 16) as u32,
            "the 4-bit count, in order"
        );
        let v = tempo_net::flexvita::parse_vita(bytes).unwrap();
        assert_eq!(v.stream_id, Some(DAX_TX), "our own transmit stream");
        assert_eq!(
            v.packet_class,
            Some(0x03E3),
            "float32 stereo, not int16 mono"
        );
    }
    let left = left_samples(&packets);
    let steady = &left[2_000..left.len() - 2_000];
    assert!(
        (peak(steady) - 0.8 * 0.25).abs() < 0.01,
        "the TX level applied as the audio leaves: peak {}",
        peak(steady)
    );
    // Paced: ninety packets are 0.48 s of audio, and they took about that long to arrive.
    let spread = packets[89].0 - packets[0].0;
    assert!(
        spread >= Duration::from_millis(400),
        "a burst, not a stream: {spread:?}"
    );
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
}

/// The level is applied as each packet leaves, so the Pwr slider reaches audio already queued.
#[test]
fn the_tx_level_reaches_audio_already_queued() {
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let d = audio_daemon(&sim, memory());
    dax_tx_ready(&sim, &d);
    let tee = d.tx_tee().unwrap();
    tee.set_level(1.0);
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    tee.feed(&tone(1.5, 0.5));
    eventually("the first packets", || {
        (dax_tx_packets(&sim).len() >= 20).then_some(())
    });
    tee.set_level(0.5);
    let start = dax_tx_packets(&sim).len() + 5;
    let packets = eventually("the rest", || {
        let p = dax_tx_packets(&sim);
        (p.len() >= start + 60).then_some(p)
    });
    assert!((peak(&left_samples(&packets[2..15])) - 0.5).abs() < 0.02);
    assert!((peak(&left_samples(&packets[start..start + 60])) - 0.25).abs() < 0.01);
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
}

/// ⭐ No key, no packets: audio handed to the route while nothing of ours is keyed never leaves,
/// and is dropped rather than kept for a later key.
#[test]
fn nothing_leaves_without_our_key() {
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let d = audio_daemon(&sim, memory());
    dax_tx_ready(&sim, &d);
    let tee = d.tx_tee().unwrap();
    tee.feed(&tone(0.3, 0.5));
    std::thread::sleep(Duration::from_millis(500));
    assert!(dax_tx_packets(&sim).is_empty(), "DAX TX without a key");
    let tx = d.audio.as_ref().unwrap().tx();
    assert_eq!(audio::queued(&tx), 0, "dropped, not kept for the next key");
    // Control: keyed, the same audio leaves.
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    tee.feed(&tone(0.3, 0.5));
    eventually("packets once keyed", || {
        (!dax_tx_packets(&sim).is_empty()).then_some(())
    });
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
}

/// The bundled session, with the slice's mode reported back for the modes these tests set.
fn with_mode_echo() -> SimSession {
    with(
        SimSession::v4_gui_client(),
        ["USB", "DIGU", "CW"]
            .iter()
            .map(|m| {
                let echo = format!("S{{h}}|slice 0 mode={m}");
                rule(&format!("slice set 0 mode={m}"), "0", &[echo.as_str()])
            })
            .collect(),
    )
}

/// ⭐ The operator's ruling of 2026-10-03, "follow the slice's mode": DAX for a digital TX slice (after Nexus's own transmit
/// stream exists), the mic for Phone at the shack, DAX for the stream's browser voice, and CW
/// left alone. Each write once per change.
#[test]
fn the_dax_source_follows_the_tx_slices_mode() {
    let sim = simulator(with_mode_echo(), vec![]);
    let mem = memory();
    let d = audio_daemon(&sim, mem.clone());
    d.set_native_audio(true);
    route_until(&d, &sim, false, "transmit set dax=1", 1);
    let sent = wire(&sim);
    let at = |text: &str| sent.iter().position(|c| c == text).unwrap();
    assert!(
        at("stream create type=dax_tx") < at("transmit set dax=1"),
        "{sent:?}"
    );
    assert_eq!(
        mem.recall(&sim.tcp_addr().to_string()),
        Some(false),
        "the operator's own setting is kept while Nexus has it changed"
    );
    let mut c = Client::connect(&d);
    assert_eq!(c.ask("M USB 0", 1), "RPRT 0\n");
    route_until(&d, &sim, false, "transmit set dax=0", 1);
    assert_eq!(
        mem.recall(&sim.tcp_addr().to_string()),
        None,
        "back on the operator's"
    );
    // The stream's browser voice.
    route_until(&d, &sim, true, "transmit set dax=1", 2);
    route_until(&d, &sim, false, "transmit set dax=0", 2);
    // Once per change: quiet points with nothing changed write nothing.
    route_for(&d, false, 400);
    assert_eq!(count(&sim, "transmit set dax=0"), 2);
    assert_eq!(count(&sim, "transmit set dax=1"), 2);
    // CW: left alone.
    assert_eq!(c.ask("M CW 0", 1), "RPRT 0\n");
    route_for(&d, true, 500);
    assert_eq!(count(&sim, "transmit set dax=0"), 2);
    assert_eq!(count(&sim, "transmit set dax=1"), 2);
}

/// ⭐ The same ruling, "when SmartSDR's DAX is also connected, never write the flag": beside another
/// program's DAX transmit stream Nexus writes no DAX source, creates no transmit stream of its
/// own and sends no DAX TX, and a key is not refused for the audio route (SmartSDR's DAX carries
/// Nexus's audio from the sound card, as before).
#[test]
fn never_beside_smartsdrs_dax() {
    let sim = simulator(
        SimSession::v4_gui_client(),
        vec![Fault::ForeignDaxTx(ForeignDax::default())],
    );
    let d = audio_daemon(&sim, memory());
    d.set_native_audio(true);
    eventually("the other program seen", || {
        d.other_dax_feeder().then_some(())
    });
    route_for(&d, false, 600);
    route_for(&d, true, 300);
    let sent = wire(&sim);
    assert!(
        !sent
            .iter()
            .any(|c| c.starts_with("transmit set dax") || c == "stream create type=dax_tx"),
        "beside SmartSDR's DAX: {sent:?}"
    );
    assert!(!d.tx_route_ready());
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_eq!(
        c.ask("T 1", 1),
        "RPRT 0\n",
        "the sound card route keys as before"
    );
    if let Some(tee) = d.tx_tee() {
        tee.feed(&tone(0.2, 0.5));
    }
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        dax_tx_packets(&sim).is_empty(),
        "DAX TX beside another program's"
    );
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
}

/// ⭐ Never while keyed: a mode change while our transmitter is keyed is written only after the
/// radio has proved the unkey (its READY naming no transmitter).
#[test]
fn the_dax_source_is_never_written_while_keyed() {
    let sim = simulator(with_mode_echo(), vec![]);
    let d = audio_daemon(&sim, memory());
    dax_tx_ready(&sim, &d);
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    ask_until_answer(&mut c, "t", "1\n");
    assert_eq!(c.ask("M USB 0", 1), "RPRT 0\n");
    route_for(&d, false, 400);
    assert_eq!(count(&sim, "transmit set dax=0"), 0, "written while keyed");
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    route_until(&d, &sim, false, "transmit set dax=0", 1);
    let log = sim.events();
    let unkey = log
        .iter()
        .position(|e| matches!(e, SimEvent::Command { text, .. } if text == "xmit 0"))
        .unwrap();
    let released = unkey
        + log[unkey..]
            .iter()
            .position(|e| matches!(e, SimEvent::Sent { line, .. } if line.contains("tx_client_handle=0x00000000 state=READY")))
            .unwrap();
    let written = log
        .iter()
        .position(|e| matches!(e, SimEvent::Command { text, .. } if text == "transmit set dax=0"))
        .unwrap();
    assert!(
        written > released,
        "the flag went before the unkey was proven"
    );
}

/// ⭐ Restore on disconnect: the operator's own setting goes back as the client ends, after the
/// unkey and before Nexus's slices go, and the memory is cleared.
#[test]
fn the_operators_setting_goes_back_at_disconnect() {
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let mem = memory();
    let d = audio_daemon(&sim, mem.clone());
    dax_tx_ready(&sim, &d);
    drop(d);
    let sent = wire(&sim);
    let at = |text: &str| sent.iter().rposition(|c| c == text);
    let restored = at("transmit set dax=0").expect("the operator's mic back");
    assert!(restored > at("transmit set dax=1").unwrap());
    assert!(restored < at("slice remove 0").expect("our slice removed"));
    assert_eq!(mem.recall(&sim.tcp_addr().to_string()), None);
}

/// ⭐ Restore at the next connect: a session lost mid-over cannot put the setting back, so the
/// next connect to the same radio does, first.
#[test]
fn a_session_lost_mid_over_is_put_back_at_the_next_connect() {
    let sim = simulator(
        SimSession::v4_gui_client(),
        vec![Fault::DisconnectMidOver {
            after: Duration::from_millis(200),
            radio_stays_keyed: false,
        }],
    );
    let mem = memory();
    let a = audio_daemon(&sim, mem.clone());
    dax_tx_ready(&sim, &a);
    let mut c = Client::connect(&a);
    wait_session(&a, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    eventually("the lost session", || (!a.is_alive()).then_some(()));
    drop(c);
    drop(a);
    assert_eq!(
        count(&sim, "transmit set dax=0"),
        0,
        "nothing could be put back"
    );
    let key = sim.tcp_addr().to_string();
    assert_eq!(mem.recall(&key), Some(false), "kept for the next connect");
    // The next connect: the radio still takes DAX, and the operator's mic comes back first.
    let b = audio_daemon(&sim, mem.clone());
    route_until(&b, &sim, false, "transmit set dax=0", 1);
    assert_eq!(mem.recall(&key), None);
    let log = sim.events();
    assert!(log.iter().any(
        |e| matches!(e, SimEvent::Command { conn: 1, text, .. } if text == "transmit set dax=0")
    ));
}

/// ⭐ A digital over never goes out on the radio's mic: with native audio on, `T 1` is refused
/// until the radio takes its audio from Nexus's DAX. The controls: once it does, the same verb
/// keys; with native audio off, it keys as before.
#[test]
fn a_digital_over_is_refused_on_the_mic() {
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let d = audio_daemon(&sim, memory());
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    d.set_native_audio(true);
    assert_ne!(c.ask("T 1", 1), "RPRT 0\n");
    assert_eq!(count(&sim, "xmit 1"), 0, "keyed on the mic");
    route_until(&d, &sim, false, "transmit set dax=1", 1);
    eventually("the echo", || {
        (d.session().snapshot().model.transmit.dax == Some(true)).then_some(())
    });
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    assert_eq!(wait_count(&sim, "xmit 1", 1), 1);
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    wait_session(&d, "the unkey confirmed", |s| !s.keyed);
    // Native audio off: the radio's routing is the operator's business, as before.
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let d = audio_daemon(&sim, memory());
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
}

/// ⭐ NEVER A SILENT OVER: native audio turned off while the radio still takes its transmit audio
/// from the DAX Nexus set, so nothing feeds it. `T 1` is refused, in the shim's words, until the
/// operator's own setting (the mic) is back. The control: once it is, the same verb keys.
#[test]
fn a_key_is_refused_while_nothing_feeds_the_dax_nexus_set() {
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let d = audio_daemon(&sim, memory());
    dax_tx_ready(&sim, &d);
    eventually("the echo", || {
        (d.session().snapshot().model.transmit.dax == Some(true)).then_some(())
    });
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    d.set_native_audio(false);
    let asked = Instant::now();
    let answer = c.ask("T 1", 1);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        format!(
            "answer={answer:?} xmit1={} refused={:?}",
            count(&sim, "xmit 1"),
            d.key_refused_since(asked)
        ),
        "answer=\"RPRT -1\\n\" xmit1=0 refused=Some((\"not keying a DIGU over: native audio is \
         off, and the radio still takes its transmit audio from the DAX Nexus set, which nothing \
         feeds until its mic input is back\", FlexAudioRefusal { mode: \"DIGU\", cause: DaxUnfed \
         }))"
    );
    route_until(&d, &sim, false, "transmit set dax=0", 1);
    eventually("the mic", || {
        (d.session().snapshot().model.transmit.dax == Some(false)).then_some(())
    });
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    assert_eq!(wait_count(&sim, "xmit 1", 1), 1);
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
}

/// The mode this shim just commanded counts before the radio reports it: right after `M PKTUSB`,
/// with the radio still reporting USB and the mic as the source, `T 1` is refused.
#[test]
fn a_mode_just_commanded_counts_before_the_radio_reports_it() {
    // USB and its mic are reported back; the switch back to DIGU is not.
    let session = with(
        SimSession::v4_gui_client(),
        vec![rule(
            "slice set 0 mode=USB",
            "0",
            &["S{h}|slice 0 mode=USB"],
        )],
    );
    let sim = simulator(session, vec![]);
    let d = audio_daemon(&sim, memory());
    dax_tx_ready(&sim, &d);
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_eq!(c.ask("M USB 0", 1), "RPRT 0\n");
    route_until(&d, &sim, false, "transmit set dax=0", 1);
    eventually("the mic", || {
        (d.session().snapshot().model.transmit.dax == Some(false)).then_some(())
    });
    // Control: Phone on the mic keys.
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    wait_session(&d, "the unkey confirmed", |s| !s.keyed);
    let keys = count(&sim, "xmit 1");
    assert_eq!(c.ask("M PKTUSB 0", 1), "RPRT 0\n");
    assert_eq!(
        d.session().snapshot().model.slices[&0].mode.as_deref(),
        Some("USB"),
        "premise: the radio has not reported the digital mode"
    );
    assert_ne!(c.ask("T 1", 1), "RPRT 0\n");
    assert_eq!(
        count(&sim, "xmit 1"),
        keys,
        "keyed a digital over on the mic"
    );
}

/// A refused DAX transmit stream routes nothing: no `transmit set dax=1`, no route, and a digital
/// key refused rather than sent on the mic. The create is asked again, not in a storm.
#[test]
fn a_refused_dax_transmit_stream_routes_nothing() {
    let sim = simulator(SimSession::v4_gui_client(), vec![Fault::DaxTxRefused]);
    let d = audio_daemon(&sim, memory());
    d.set_native_audio(true);
    route_for(&d, false, 2_600);
    let creates = count(&sim, "stream create type=dax_tx");
    assert!((1..=2).contains(&creates), "{creates} creates in 2.6 s");
    assert_eq!(count(&sim, "transmit set dax=1"), 0);
    assert!(!d.tx_route_ready());
    let mut c = Client::connect(&d);
    wait_session(&d, "the readback idle", |s| s.transmit_ready);
    assert_ne!(c.ask("T 1", 1), "RPRT 0\n");
    assert_eq!(count(&sim, "xmit 1"), 0);
}
