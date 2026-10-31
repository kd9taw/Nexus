//! The TCI daemon against the simulated SDR program (`tempo-tcisim`), over a real WebSocket and a
//! real rigctld socket on loopback: the shim's verb map as a rigctld client sends it, which source
//! each key takes, the faults a guard here exists for, the daemon's life (`ready;`, the alarms, the
//! ladder, a lost link carried to the next session, teardown), and Nexus's transmit audio as the
//! server pulls it (the transmit route, `audio::TciTx`). Every keying test reads what keyed the
//! simulated radio, and every audio test what reached it, from the simulator's own log.
//!
//! The registry is process-wide and the simulators listen on ephemeral ports that a later test's
//! simulator may be given, so each test forgets its server's entry before its first daemon. Waits
//! are bounded and wait on the daemon's state or the simulator's log, never a bare sleep.

use std::f64::consts::TAU;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::atomic::AtomicU64;

use tempo_net::tci::session::NoStreams;
use tempo_tcisim::client::Peer;
use tempo_tcisim::frame::Fit;
use tempo_tcisim::{
    Cause, Closer, Config as SimConfig, Dialect as SimDialect, Event as SimEvent, Fault, Keyer,
    Logged, Meters, Mic, Simulator, Source as SimSource,
};

use super::*;
use crate::backend::TxTee;
use crate::capture_resample::CaptureResampler;

/// The longest any one expected thing may take here.
const WAIT: Duration = Duration::from_secs(10);

fn trx0() -> Trx {
    Trx::new(0).expect("trx 0")
}

/// Transmit audio: a 1 kHz tone at half scale.
#[derive(Default)]
struct Tone {
    phase: f64,
}

impl Streams for Tone {
    fn transmit(&mut self, rate: u32, out: &mut [f32]) -> usize {
        let step = TAU * 1_000.0 / f64::from(rate);
        for s in out.iter_mut() {
            *s = (0.5 * self.phase.sin()) as f32;
            self.phase = (self.phase + step) % TAU;
        }
        out.len()
    }
}

fn sim(dialect: SimDialect, faults: impl IntoIterator<Item = Fault>) -> Simulator {
    Simulator::start(SimConfig::dialect(dialect).with(faults)).expect("the simulator starts")
}

fn esdr3(faults: impl IntoIterator<Item = Fault>) -> Simulator {
    sim(SimDialect::expert_sdr3(), faults)
}

fn target(sim: &Simulator) -> Target {
    Target {
        host: "127.0.0.1".into(),
        port: sim.addr().port(),
        trx: 0,
        dialect: TciDialect::Auto,
        audio: true,
        rigctld_port: 0,
    }
}

fn key_of(sim: &Simulator) -> registry::Key {
    (sim.addr(), trx0())
}

fn start(target: &Target, options: Options) -> TciDaemon {
    TciDaemon::start_with(target, Box::new(Tone::default()), options).expect("the daemon starts")
}

/// A daemon with the production waits, on a fresh registry entry.
fn daemon(sim: &Simulator) -> TciDaemon {
    fresh(sim, Options::default())
}

/// A daemon with `options`, on a fresh registry entry.
fn fresh(sim: &Simulator, options: Options) -> TciDaemon {
    registry::forget(key_of(sim));
    start(&target(sim), options)
}

/// A rigctld client on the shim, as Nexus's own `Rig` is one.
struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Client {
    fn connect(d: &TciDaemon) -> Client {
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

/// Ask `verb` (a `lines`-line answer) until it answers `want`, or fail.
fn ask_until_answer(c: &mut Client, verb: &str, lines: usize, want: &str) {
    let deadline = Instant::now() + WAIT;
    loop {
        let got = c.ask(verb, lines);
        if got == want {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{verb} never answered {want:?}; last {got:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The session's own traffic, which is not a verb's: the stream and meter setup at `ready;` and
/// its meter polls.
fn session_traffic(command: &str) -> bool {
    [
        "audio_start:",
        "rx_sensors_enable:",
        "tx_sensors_enable:",
        "rx_smeter:",
    ]
    .iter()
    .any(|p| command.starts_with(p))
}

/// What the server read from connection `conn`, the session's own traffic left out.
fn verbs_from(sim: &Simulator, conn: usize) -> Vec<String> {
    sim.commands_from(conn)
        .into_iter()
        .filter(|c| !session_traffic(c))
        .collect()
}

fn wait_log(sim: &Simulator, what: &str, done: impl Fn(&[Logged]) -> bool) {
    assert!(
        sim.wait_for(WAIT, |log| done(log)),
        "the simulator never logged {what}"
    );
}

/// What keyed the radio, in order.
fn keys(log: &[Logged]) -> Vec<(Keyer, SimSource)> {
    log.iter()
        .filter_map(|l| match l.event {
            SimEvent::Keyed { by, source, .. } => Some((by, source)),
            _ => None,
        })
        .collect()
}

fn unkeys(log: &[Logged]) -> usize {
    log.iter()
        .filter(|l| matches!(l.event, SimEvent::Unkeyed { .. }))
        .count()
}

/// When the server read `text` from `conn`, first.
fn command_at(log: &[Logged], conn: usize, text: &str) -> Option<Duration> {
    log.iter().find_map(|l| match &l.event {
        SimEvent::Command {
            conn: c, text: t, ..
        } if *c == conn && t == text => Some(l.at),
        _ => None,
    })
}

/// How many times the server read `text` from `conn`. Inside a [`wait_log`] it takes the log the
/// simulator hands over: the simulator holds its log's lock while it asks.
fn count_in(log: &[Logged], conn: usize, text: &str) -> usize {
    log.iter()
        .filter(|l| {
            matches!(&l.event, SimEvent::Command { conn: c, text: t, .. } if *c == conn && t == text)
        })
        .count()
}

/// When `conn` closed, and who closed it.
fn closed_at(log: &[Logged], conn: usize) -> Option<(Duration, Closer)> {
    log.iter().find_map(|l| match l.event {
        SimEvent::Closed { conn: c, by } if c == conn => Some((l.at, by)),
        _ => None,
    })
}

/// Receive audio acknowledged and flowing: what admission needs for a key of Nexus's audio.
fn wait_audio(d: &TciDaemon) {
    assert!(
        d.session().wait_until(WAIT, |s| s.rx_audio_fresh
            && s.model.trx(trx0()).audio.is_true()),
        "receive audio never flowed"
    );
}

fn wait_dead(d: &TciDaemon) {
    let deadline = Instant::now() + WAIT;
    while d.is_alive() {
        assert!(Instant::now() < deadline, "the session never ended");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_unkeyed(d: &TciDaemon) {
    assert!(
        d.session().wait_until(WAIT, |s| s.keyed.is_none()),
        "the server never reported our unkey"
    );
}

/// The halt the daemon keeps for the radio loop, once there is one.
fn halt(d: &TciDaemon) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(words) = d.take_halt() {
            return words;
        }
        assert!(Instant::now() < deadline, "no halt");
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ── Bring-up and reads ──────────────────────────────────────────────────────────────────────

/// ⭐ Once the server has said `ready;` the shim serves the transceiver: the dial, the mode in
/// rigctld words with its passband, PTT, the one VFO, split, the drive, RIT and XIT, and the
/// authored capabilities with the server's own range. One session per transceiver.
#[test]
fn the_daemon_serves_the_transceiver_once_the_server_is_ready() {
    let sim = esdr3([]);
    let d = daemon(&sim);
    assert!(d.is_alive());
    assert_eq!(
        registry::claim(d.key(), now_ms()).unwrap_err(),
        registry::Refusal::Live,
        "one session per transceiver"
    );
    let mut c = Client::connect(&d);
    assert_eq!(c.ask("f", 1), "14074000\n");
    assert_eq!(c.ask("m", 2), "USB\n2700\n");
    assert_eq!(c.ask("t", 1), "0\n");
    assert_eq!(c.ask("v", 1), "VFOA\n");
    assert_eq!(c.ask("s", 2), "0\nVFOA\n");
    assert_eq!(c.ask("l RFPOWER", 1), "0.500000\n");
    assert_eq!(c.ask("u RIT", 1), "0\n");
    assert_eq!(c.ask("u XIT", 1), "0\n");
    let caps = crate::baud_ladder::parse_caps(&c.ask_until("\\dump_caps", "Get level:"));
    assert_eq!(caps, shim::authored_caps(vec![(10_000, 30_000_000)]));
    assert_eq!(
        caps.split_detect,
        crate::baud_ladder::SplitDetect::Absent,
        "split is not read for the privilege gate until a bench shows VFO B transmits"
    );
}

/// A verb the shim does not implement answers what the shared encoder answers any backend that
/// lacks it, byte for byte: CW is the SDR program's keyer's, a later change. A read between them.
#[test]
fn the_rest_answers_as_the_broker_answers_a_backend_without_it() {
    let sim = esdr3([]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    let before = verbs_from(&sim, 0);
    let mock = crate::rigctld_server::tests::MockRig::default();
    for verb in [
        "b CQ",
        "\\stop_morse",
        "L KEYSPD 25",
        "X USB 0",
        "U NB 1",
        "R +",
    ] {
        let shared = match crate::rigctld_server::handle_command(verb, &mock) {
            crate::rigctld_server::Handled::Reply(r) => r,
            crate::rigctld_server::Handled::Close => unreachable!(),
        };
        assert_eq!(c.ask(verb, 1), shared, "{verb}");
        assert_eq!(c.ask("f", 1), "14074000\n");
    }
    assert_eq!(verbs_from(&sim, 0), before, "nothing sent for them");
}

// ── The verb map ────────────────────────────────────────────────────────────────────────────

/// ⭐ Each write is ONE typed command on the wire, answered `RPRT 0` once the server reports it,
/// and read back from the server's word: the dial, the mode (`PKTUSB` is `digu`) with its width
/// placed on its side, the drive, split with VFO B, RIT and XIT (XIT must reach the radio: the
/// engine offers it for the TCI model, `rig_has_xit(7)`).
#[test]
fn each_write_is_one_command_done_once_the_server_reports_it() {
    let sim = esdr3([]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    let before = verbs_from(&sim, 0).len();
    for verb in [
        "F 7074000",
        "M PKTUSB 3000",
        "L RFPOWER 0.25",
        "S 1 VFOB",
        "I 7076000",
        "U RIT 1",
        "J -200",
        "U XIT 1",
        "Z 50",
        "S 0 VFOA",
    ] {
        assert_eq!(c.ask(verb, 1), "RPRT 0\n", "{verb}");
    }
    assert_eq!(
        verbs_from(&sim, 0)[before..],
        [
            "vfo:0,0,7074000",
            "modulation:0,digu",
            "rx_filter_band:0,200,3200",
            "drive:0,25",
            "split_enable:0,true",
            "vfo:0,1,7076000",
            "rit_enable:0,true",
            "rit_offset:0,-200",
            "xit_enable:0,true",
            "xit_offset:0,50",
            "split_enable:0,false",
        ]
    );
    assert_eq!(c.ask("f", 1), "7074000\n");
    assert_eq!(c.ask("m", 2), "PKTUSB\n3000\n");
    assert_eq!(c.ask("l RFPOWER", 1), "0.250000\n");
    assert_eq!(c.ask("u RIT", 1), "1\n");
    assert_eq!(c.ask("u XIT", 1), "1\n");
    assert_eq!(c.ask("s", 2), "0\nVFOA\n");
    let radio = sim.radio();
    let t = &radio.trx[0];
    assert_eq!(t.vfo, [7_074_000, 7_076_000]);
    assert_eq!(t.modulation, "DIGU");
    assert_eq!((t.drive, t.split), (25, false));
    assert_eq!(
        (t.rit, t.rit_offset, t.xit, t.xit_offset),
        (true, -200, true, 50)
    );
}

/// A write the server could not take is refused before anything is sent: a dial outside its
/// `vfo_limits`, a mode it does not offer (ExpertSDR3 has no reverse CW), a width that is not
/// one, a level past full scale.
#[test]
fn a_write_the_server_could_not_take_is_refused_and_nothing_is_sent() {
    let sim = esdr3([]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    let before = verbs_from(&sim, 0);
    for verb in [
        "F 40000000",
        "I 5",
        "M PKTFM 0",
        "M CWR 0",
        "M RTTY 0",
        "M USB 200000",
        "L RFPOWER 1.5",
    ] {
        assert_eq!(c.ask(verb, 1), "RPRT -1\n", "{verb}");
        assert_eq!(c.ask("f", 1), "14074000\n");
    }
    assert_eq!(verbs_from(&sim, 0), before, "nothing sent");
}

/// Every dialect the simulator speaks takes the dial, the mode and the drive, in its own form:
/// TCI 1.x writes its drive with no transceiver.
#[test]
fn every_dialect_takes_the_dial_the_mode_and_the_drive() {
    for dialect in SimDialect::all() {
        let name = dialect.name.clone();
        let global = name.starts_with("ExpertSDR2");
        let s = sim(dialect, []);
        let d = daemon(&s);
        let mut c = Client::connect(&d);
        for verb in ["F 14080000", "M PKTUSB 0", "L RFPOWER 0.25"] {
            assert_eq!(c.ask(verb, 1), "RPRT 0\n", "{name}: {verb}");
        }
        assert_eq!(c.ask("f", 1), "14080000\n", "{name}");
        assert!(c.ask("m", 2).starts_with("PKTUSB\n"), "{name}");
        assert_eq!(c.ask("l RFPOWER", 1), "0.250000\n", "{name}");
        let drive = if global { "drive:25" } else { "drive:0,25" };
        assert!(
            verbs_from(&s, 0).iter().any(|v| v == drive),
            "{name}: {:?}",
            verbs_from(&s, 0)
        );
    }
}

/// ⭐ A write the server holds back (another program's change inside its 200 ms hold, §3.5) is
/// never taken for done: no report, so `RPRT -5`, and the dial reads what the server has. The same
/// write once the hold is over is taken.
#[test]
fn a_write_the_server_holds_back_is_not_taken_for_done() {
    let sim = esdr3([Fault::Hold200ms {
        window: Duration::from_secs(3),
    }]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    let mut other = Peer::connect(sim.addr(), WAIT).expect("another program connects");
    other
        .send_text("vfo:0,0,7100000;")
        .expect("the other program's write");
    ask_until_answer(&mut c, "f", 1, "7100000\n");
    assert_eq!(
        c.ask("F 14080000", 1),
        "RPRT -5\n",
        "held: the server never reported it"
    );
    assert_eq!(c.ask("f", 1), "7100000\n");
    assert!(sim
        .events()
        .iter()
        .any(|e| matches!(e, SimEvent::Held { conn: 0, text } if text == "vfo:0,0,14080000")));
    ask_until_answer(&mut c, "F 14080000", 1, "RPRT 0\n");
    assert_eq!(c.ask("f", 1), "14080000\n");
}

/// A slow server's late report of an earlier write is not a later write's answer: each waits for
/// its own value.
#[test]
fn a_late_report_of_an_earlier_write_is_not_the_next_ones_answer() {
    let sim = esdr3([Fault::Slow {
        delay: Duration::from_millis(1_500),
    }]);
    let d = fresh(
        &sim,
        Options {
            readback: Duration::from_millis(1_000),
            ..Options::default()
        },
    );
    let mut c = Client::connect(&d);
    assert_eq!(c.ask("F 14080000", 1), "RPRT -5\n", "its report is late");
    assert_eq!(
        c.ask("F 14090000", 1),
        "RPRT -5\n",
        "the late report of 14.080 MHz comes inside this wait, and is not its answer"
    );
    ask_until_answer(&mut c, "f", 1, "14090000\n");
}

// ── Which source a key takes ────────────────────────────────────────────────────────────────

/// ⭐ The Flex rule on TCI: a digital over takes Nexus's audio (`trx:0,true,tci`, pulled with
/// TX_CHRONO and answered with it), Phone at the shack the SDR's microphone (`trx:0,true`), and the
/// voice keyer's message in a voice mode Nexus's audio again.
#[test]
fn a_digital_over_takes_nexus_audio_and_phone_the_sdr_microphone() {
    let sim = esdr3([]);
    let d = daemon(&sim);
    d.set_tx_route(true);
    let mut c = Client::connect(&d);
    wait_audio(&d);
    let over = |c: &mut Client, n: usize, source: SimSource| {
        assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
        wait_log(&sim, "the key", |log| keys(log).len() == n);
        assert_eq!(keys(&sim.log())[n - 1], (Keyer::Client(0), source));
        if source == SimSource::Tci {
            wait_log(&sim, "our audio", |log| {
                log.iter().any(|l| {
                    matches!(
                        l.event,
                        SimEvent::FrameIn {
                            conn: 0,
                            ours: true,
                            ..
                        }
                    )
                })
            });
        }
        assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
        wait_log(&sim, "the unkey", |log| unkeys(log) == n);
        wait_unkeyed(&d);
    };
    assert_eq!(c.ask("M PKTUSB 0", 1), "RPRT 0\n");
    over(&mut c, 1, SimSource::Tci);
    assert_eq!(c.ask("M USB 0", 1), "RPRT 0\n");
    over(&mut c, 2, SimSource::Mic(Mic::Selected));
    d.set_voice_key(true);
    over(&mut c, 3, SimSource::Tci);
    d.set_voice_key(false);
    assert!(sim.commands_from(0).iter().any(|v| v == "trx:0,true,tci"));
    assert!(sim.commands_from(0).iter().any(|v| v == "trx:0,true"));
}

/// With the radio's TCI audio off, the operator routes Nexus's audio some other way: every over
/// keys the SDR's selected microphone, and no stream is asked for.
#[test]
fn with_tci_audio_off_every_over_takes_the_sdr_microphone() {
    let sim = esdr3([]);
    registry::forget(key_of(&sim));
    let d = start(
        &Target {
            audio: false,
            ..target(&sim)
        },
        Options::default(),
    );
    let mut c = Client::connect(&d);
    assert_eq!(c.ask("M PKTUSB 0", 1), "RPRT 0\n");
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    wait_log(&sim, "the key", |log| !keys(log).is_empty());
    assert_eq!(
        keys(&sim.log())[0],
        (Keyer::Client(0), SimSource::Mic(Mic::Selected))
    );
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    assert!(!sim
        .commands_from(0)
        .iter()
        .any(|v| v.starts_with("audio_start")));
}

/// ⭐ Never a silent over: a key that takes Nexus's audio is refused, and says why, until its route
/// into the session is up. Nothing reaches the wire.
#[test]
fn nexus_audio_is_keyed_only_while_its_route_is_up() {
    let sim = esdr3([]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    wait_audio(&d);
    assert_eq!(c.ask("M PKTUSB 0", 1), "RPRT 0\n");
    let asked = Instant::now();
    assert_eq!(c.ask("T 1", 1), "RPRT -1\n");
    let why = d.key_refused_since(asked).expect("why");
    assert!(why.contains("transmit audio is not connected"), "{why}");
    assert!(!sim
        .commands_from(0)
        .iter()
        .any(|v| v.starts_with("trx:0,true")));
    d.set_tx_route(true);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
}

/// A key in CW (the SDR program's keyer is a later change) or in a mode Nexus does not transmit
/// in is refused, says why, and sends nothing.
#[test]
fn cw_and_modes_nexus_does_not_transmit_in_never_key() {
    let sim = esdr3([]);
    let d = daemon(&sim);
    d.set_tx_route(true);
    let mut c = Client::connect(&d);
    assert_eq!(c.ask("M CW 0", 1), "RPRT 0\n");
    let asked = Instant::now();
    assert_eq!(c.ask("T 1", 1), "RPRT -1\n");
    let why = d.key_refused_since(asked).expect("why");
    assert!(why.contains("not keyed in CW"), "{why}");
    // A fresh session that never asked for a mode, on a mode the SDR's operator chose.
    drop(c);
    drop(d);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    sim.operator("modulation:0,drm;");
    ask_until_answer(&mut c, "m", 2, "DRM\n2700\n");
    let asked = Instant::now();
    assert_eq!(c.ask("T 1", 1), "RPRT -1\n");
    let why = d.key_refused_since(asked).expect("why");
    assert!(
        why.contains("DRM, which Nexus does not transmit in"),
        "{why}"
    );
    assert!(keys(&sim.log()).is_empty(), "nothing keyed");
}

/// ⭐⭐ A band change makes the server restore the band's own mode and drive (§3.5). The over is
/// still keyed as the mode the radio loop asked for, and both are put back first: a digital over
/// on the restored phone mode would go out on the SDR's microphone, at the band's drive.
#[test]
fn a_band_restore_is_put_back_before_the_key() {
    let sim = esdr3([Fault::BandRestore {
        drive: 100,
        tune_drive: 100,
        modulation: Some("USB".into()),
    }]);
    let d = daemon(&sim);
    d.set_tx_route(true);
    let mut c = Client::connect(&d);
    wait_audio(&d);
    assert_eq!(c.ask("M PKTUSB 0", 1), "RPRT 0\n");
    assert_eq!(c.ask("L RFPOWER 0.3", 1), "RPRT 0\n");
    assert_eq!(c.ask("F 7074000", 1), "RPRT 0\n");
    // The restore has reached Nexus.
    ask_until_answer(&mut c, "l RFPOWER", 1, "1.000000\n");
    assert!(c.ask("m", 2).starts_with("USB\n"));
    let before = verbs_from(&sim, 0).len();
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    wait_log(&sim, "the key", |log| !keys(log).is_empty());
    assert_eq!(
        keys(&sim.log())[0],
        (Keyer::Client(0), SimSource::Tci),
        "the digital over takes Nexus's audio, never the SDR's microphone"
    );
    let sent: Vec<String> = verbs_from(&sim, 0)
        .into_iter()
        .skip(before)
        .take(3)
        .collect();
    assert_eq!(
        sent,
        ["modulation:0,digu", "drive:0,30", "trx:0,true,tci"],
        "the mode, then the drive, put back before the key"
    );
    let radio = sim.radio();
    assert_eq!(
        (radio.trx[0].modulation.as_str(), radio.trx[0].drive),
        ("DIGU", 30)
    );
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
}

// ── Stops and another program's key ─────────────────────────────────────────────────────────

/// ⭐ `T 0` is never gated: it goes out with nothing of ours on the air, and during another
/// program's over, which a server with no owner (ExpertSDR3) then ends: the safe direction.
#[test]
fn t_0_is_sent_whatever_the_session_believes() {
    let sim = esdr3([]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    let before = count_in(&sim.log(), 0, "trx:0,false");
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n", "nothing of ours on the air");
    wait_log(&sim, "the stop", |log| {
        count_in(log, 0, "trx:0,false") > before
    });
    sim.operator("trx:0,true;");
    ask_until_answer(&mut c, "t", 1, "1\n");
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    wait_log(&sim, "the other key ended by our stop", |log| {
        log.iter().any(|l| {
            matches!(
                l.event,
                SimEvent::Unkeyed {
                    by: Keyer::Operator,
                    cause: Cause::Asked(Keyer::Client(0)),
                    ..
                }
            )
        })
    });
    ask_until_answer(&mut c, "t", 1, "0\n");
}

/// ⭐ Another program's key, or the SDR operator's: `t` shows it, ours is refused meanwhile and
/// says why, and none of it is ours to read as our own.
#[test]
fn another_programs_key_shows_and_ours_is_refused_meanwhile() {
    let sim = esdr3([]);
    let d = daemon(&sim);
    let mut c = Client::connect(&d);
    assert_eq!(c.ask("M USB 0", 1), "RPRT 0\n");
    sim.operator("trx:0,true;");
    ask_until_answer(&mut c, "t", 1, "1\n");
    assert!(!d.keyed(), "not ours");
    let asked = Instant::now();
    assert_eq!(c.ask("T 1", 1), "RPRT -1\n");
    let why = d.key_refused_since(asked).expect("why");
    assert!(why.contains("transmitting for someone else"), "{why}");
    assert!(!sim
        .commands_from(0)
        .iter()
        .any(|v| v.starts_with("trx:0,true")));
    sim.operator("trx:0,false;");
    ask_until_answer(&mut c, "t", 1, "0\n");
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
}

/// No key goes out while the server forbids transmitting (at the shim): refused, with the
/// server's reason in the operator's words, and nothing keyed.
#[test]
fn no_key_goes_out_while_the_server_forbids_transmitting() {
    for (fault, words) in [
        (Fault::TxEnableFalse, "does not allow transmitting here"),
        (Fault::ReceiveOnly, "receive-only"),
    ] {
        let sim = esdr3([fault]);
        let d = daemon(&sim);
        let mut c = Client::connect(&d);
        assert_eq!(c.ask("M USB 0", 1), "RPRT 0\n");
        let asked = Instant::now();
        assert_eq!(c.ask("T 1", 1), "RPRT -1\n", "{words}");
        let why = d.key_refused_since(asked).expect("why");
        assert!(why.contains(words), "{why}");
        assert!(keys(&sim.log()).is_empty(), "{words}: nothing keyed");
    }
}

// ── The meters ──────────────────────────────────────────────────────────────────────────────

/// The S-meter is the server's dBm as dB from S9 at the dial; while keyed, SWR and the peak watts
/// come from its `tx_sensors`.
#[test]
fn the_meters_read_the_servers_sensors() {
    let sim = esdr3([]);
    sim.set_meters(Meters {
        smeter_dbm: -53.0,
        mic_dbm: -30.0,
        max_power_w: 100.0,
        swr: 1.5,
    });
    let d = daemon(&sim);
    // A meter not reported yet answers as a verb not implemented (`RPRT -11`): wait for the
    // server's first push of each.
    assert!(
        d.session().wait_until(WAIT, |s| {
            let t = s.model.trx(trx0());
            t.rx_dbm[0].known().is_some() && t.tx_sensors.known().is_some()
        }),
        "the server never pushed its meters"
    );
    let mut c = Client::connect(&d);
    ask_until_answer(&mut c, "l STRENGTH", 1, "20\n");
    assert_eq!(c.ask("M USB 0", 1), "RPRT 0\n");
    assert_eq!(c.ask("L RFPOWER 0.5", 1), "RPRT 0\n");
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    ask_until_answer(&mut c, "l SWR", 1, "1.50\n");
    ask_until_answer(&mut c, "l RFPOWER_METER_WATTS", 1, "50.0\n");
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
}

// ── Tune ────────────────────────────────────────────────────────────────────────────────────

/// ⭐ Tune is the SDR program's own carrier (`tune:0,true`), ended by the daemon itself past the
/// hold it was given if the radio loop does not end it; `tune_off` ends one at once.
#[test]
fn tune_is_the_sdr_programs_carrier_and_ends_by_itself_past_its_hold() {
    let sim = esdr3([]);
    let d = fresh(
        &sim,
        Options {
            tune_margin: Duration::from_millis(100),
            ..Options::default()
        },
    );
    assert!(d.tunes_natively());
    d.tune_on(300).expect("tune keyed");
    wait_log(&sim, "the carrier", |log| {
        keys(log) == [(Keyer::Client(0), SimSource::Tune)]
    });
    wait_log(&sim, "the carrier ended", |log| unkeys(log) == 1);
    assert!(sim.commands_from(0).iter().any(|v| v == "tune:0,false"));
    assert_eq!(
        halt(&d),
        "the tune carrier outlasted its hold, so Nexus ended it"
    );
    wait_unkeyed(&d);
    d.tune_on(60_000).expect("tune keyed");
    wait_log(&sim, "the second carrier", |log| keys(log).len() == 2);
    assert!(d.tune_off());
    wait_log(&sim, "the second carrier ended", |log| unkeys(log) == 2);
    wait_unkeyed(&d);
    assert_eq!(d.take_halt(), None, "ended by the radio loop: no halt");
}

// ── What the radio loop is told ─────────────────────────────────────────────────────────────

/// An over the session ends by itself (the SDR program stopped) is the halt the radio loop
/// reads, in the operator's words.
#[test]
fn an_over_the_session_ends_by_itself_is_the_halt_the_radio_loop_reads() {
    let sim = esdr3([Fault::ServerStop {
        after: Duration::from_millis(200),
    }]);
    let d = fresh(
        &sim,
        Options {
            // Keep the session past the stop the server never confirms, for the reading.
            adjust: |c| c.unkey_deadline_ms = 30_000,
            ..Options::default()
        },
    );
    let mut c = Client::connect(&d);
    assert_eq!(c.ask("M USB 0", 1), "RPRT 0\n");
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    assert_eq!(halt(&d), "the SDR program stopped, so Nexus ended the over");
}

/// An unkey the server never confirms is the alarm the radio loop reads, and the key is carried
/// to the next session.
#[test]
fn an_unkey_the_server_never_confirms_is_the_alarm_and_is_carried() {
    let sim = esdr3([Fault::StuckTrx]);
    let d = fresh(
        &sim,
        Options {
            adjust: |c| c.unkey_deadline_ms = 600,
            ..Options::default()
        },
    );
    let key = d.key();
    let mut c = Client::connect(&d);
    assert_eq!(c.ask("M USB 0", 1), "RPRT 0\n");
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    ask_until_answer(&mut c, "t", 1, "1\n");
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    wait_dead(&d);
    let alarm = d.alarm();
    assert_eq!(alarm.as_deref(), Some(UNKEY_ALARM));
    assert_eq!(
        registry::carried(key),
        Some(OurKey {
            trx: trx0(),
            kind: TxStart::Microphone
        })
    );
}

/// ⭐ The link is LOST during an over and the radio stays keyed (at the daemon): the belief
/// stays, the operator is told the radio may still be transmitting, the key is carried, and the
/// radio loop's next daemon sends `trx:0,false` as its first command after `ready;`, whose report
/// ends the belief.
#[test]
fn a_link_lost_mid_over_is_carried_to_the_next_session_which_stops_first() {
    let sim = esdr3([Fault::DisconnectMidOver {
        after: Duration::from_millis(200),
        radio_stays_keyed: true,
    }]);
    let d = daemon(&sim);
    let key = d.key();
    let mut c = Client::connect(&d);
    assert_eq!(c.ask("M USB 0", 1), "RPRT 0\n");
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    wait_dead(&d);
    assert!(d.keyed(), "a lost link never clears the belief");
    let alarm = d.alarm();
    assert!(
        alarm.as_deref().is_some_and(|a| a.starts_with(
            "the connection to the SDR program was lost during a transmission — it may still be \
             transmitting"
        )),
        "{alarm:?}"
    );
    let ours = OurKey {
        trx: trx0(),
        kind: TxStart::Microphone,
    };
    assert_eq!(registry::carried(key), Some(ours));
    assert_eq!(registry::retry_in(key, now_ms()), None, "retried at once");
    drop(c);
    drop(d);
    // The radio loop's next daemon, for the same server and transceiver.
    let d = start(&target(&sim), Options::default());
    wait_log(&sim, "the next session's first command", |log| {
        log.iter()
            .any(|l| matches!(l.event, SimEvent::Command { conn: 1, .. }))
    });
    assert_eq!(sim.commands_from(1)[0], "trx:0,false", "the stop first");
    wait_unkeyed(&d);
    assert!(d.is_alive());
    assert_eq!(registry::carried(key), None);
}

/// ⭐ Teardown while keyed, with a rigctld client still connected (Nexus's own `Rig`): the stop goes
/// out before the close, the server confirms it, the session closes inside teardown, and nothing is
/// left carried.
#[test]
fn teardown_while_keyed_stops_first_and_closes_with_nothing_carried() {
    let sim = esdr3([]);
    let d = daemon(&sim);
    let key = d.key();
    d.set_tx_route(true);
    let mut c = Client::connect(&d);
    wait_audio(&d);
    assert_eq!(c.ask("M PKTUSB 0", 1), "RPRT 0\n");
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    wait_log(&sim, "the key", |log| !keys(log).is_empty());
    drop(d);
    wait_log(&sim, "the close", |log| closed_at(log, 0).is_some());
    let log = sim.log();
    let stop = command_at(&log, 0, "trx:0,false").expect("our stop");
    let (closed, by) = closed_at(&log, 0).expect("the close");
    assert!(stop < closed, "the stop before the close");
    // Nexus's side ended it with its close frame, and the program read it: the session reads on
    // after the frame until the program's answer, so the socket does not go while the program is
    // still writing audio to it.
    assert_eq!(by, Closer::Client, "the close frame was never read");
    assert_eq!(unkeys(&log), 1, "the radio unkeyed");
    assert_eq!(registry::carried(key), None, "confirmed: nothing carried");
    drop(c);
}

/// The ladder: a server that never says `ready;` is given up on and asked again on the ladder's
/// wait, and after three in a row Nexus waits for the operator; nothing is dialled meanwhile.
#[test]
fn a_server_that_never_says_ready_is_given_up_on_and_then_waits_for_the_operator() {
    let sim = esdr3([Fault::NoReady]);
    let key = key_of(&sim);
    registry::forget(key);
    let now = Arc::new(AtomicU64::new(1_000_000));
    let clock: Clock = {
        let now = now.clone();
        Arc::new(move || now.load(Ordering::SeqCst))
    };
    let options = Options {
        adjust: |c| c.ready_timeout_ms = 300,
        clock,
        ..Options::default()
    };
    let attempt =
        || match TciDaemon::start_with(&target(&sim), Box::new(NoStreams), options.clone()) {
            Ok(_) => panic!("a server that never says ready was served"),
            Err(e) => e.status,
        };
    let connections = || {
        sim.events()
            .iter()
            .filter(|e| matches!(e, SimEvent::Connected { .. }))
            .count()
    };
    for n in 1..=3 {
        let status = attempt();
        assert!(status.contains("did not finish connecting"), "{status}");
        assert_eq!(connections(), n);
        if n < 3 {
            let wait = registry::retry_in(key, now.load(Ordering::SeqCst)).expect("a wait");
            assert_eq!(
                attempt(),
                registry::Refusal::Wait { remaining_ms: wait }.text()
            );
            now.fetch_add(wait, Ordering::SeqCst);
        }
    }
    assert!(registry::held(key));
    let status = attempt();
    assert!(
        status.contains("Nexus will not reconnect by itself"),
        "{status}"
    );
    assert_eq!(
        connections(),
        3,
        "nothing dialled while waiting for the operator"
    );
    registry::operator_acted(key);
    assert!(attempt().contains("did not finish connecting"));
    assert_eq!(connections(), 4);
    registry::forget(key);
}

// ── Transmit audio ──────────────────────────────────────────────────────────────────────────

/// One TX_CHRONO's ask at the simulated ExpertSDR3's stream (48 kHz, stereo, 2048 values): 1024
/// frames, 21.3 ms. Every answer of ours is exactly that, so a sample's place in what the server
/// read says which answer carried it.
const FRAMES: usize = 1_024;

/// A daemon with Nexus's own audio paths, as the radio loop starts one, on a fresh registry entry:
/// the SDR program has taken Nexus's audio stream, the route is marked up as the radio loop marks it
/// once its tee is in, and the shim is in a data mode. With the client that keys it.
fn audio_daemon(sim: &Simulator, options: Options) -> (TciDaemon, Client) {
    registry::forget(key_of(sim));
    let d = TciDaemon::start_receiving(&target(sim), options).expect("the daemon starts");
    wait_audio(&d);
    assert!(d.tx_route_ready(), "premise: the transmit route is ready");
    d.set_tx_route(true);
    let mut c = Client::connect(&d);
    assert_eq!(c.ask("M PKTUSB 0", 1), "RPRT 0\n");
    (d, c)
}

/// The daemon's transmit route.
fn route(d: &TciDaemon) -> Arc<audio::TciTx> {
    d.tx().expect("the transmit route").clone()
}

/// Wait until `done`, or fail saying `what`.
fn until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "never: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A tone at the modem rate.
fn tone_at(seconds: f32, hz: f32, amplitude: f32) -> Vec<f32> {
    (0..(seconds * 12_000.0) as usize)
        .map(|i| amplitude * (std::f32::consts::TAU * hz * i as f32 / 12_000.0).sin())
        .collect()
}

/// What the route makes of `over` for the server: resampled to 48 kHz from its start, the TX level
/// applied as it leaves.
fn at_48k(over: &[f32], level: f32) -> Vec<f32> {
    CaptureResampler::new(12_000, 48_000)
        .process(over)
        .into_iter()
        .map(|s| (s * level).clamp(-1.0, 1.0))
        .collect()
}

/// Where `a` and `b` first differ by more than float noise, or `None`.
fn first_difference(a: &[f32], b: &[f32]) -> Option<usize> {
    a.iter().zip(b).position(|(x, y)| (x - y).abs() > 1e-6)
}

/// Where `want` starts in `sent`, whole: at an answer's start, since audio handed over is first taken
/// by the next chrono.
fn start_of(sent: &[f32], want: &[f32]) -> Option<usize> {
    let last = sent.len().checked_sub(want.len())?;
    (0..=last)
        .step_by(FRAMES)
        .find(|&k| first_difference(&sent[k..k + want.len()], want).is_none())
}

/// The first answer at or after `from` (in answers) that carries anything but silence, as an index
/// into what the server read.
fn first_audio(sent: &[f32], from: usize) -> Option<usize> {
    (from * FRAMES..sent.len()).step_by(FRAMES).find(|&k| {
        sent[k..(k + FRAMES).min(sent.len())]
            .iter()
            .any(|s| *s != 0.0)
    })
}

/// The transmit frames the server read from `conn`, in order: when, whether it held our key then,
/// how each fit the TX_CHRONO it answered, and its size in bytes.
fn frames_in(log: &[Logged], conn: usize) -> Vec<(Duration, bool, Option<Fit>, usize)> {
    log.iter()
        .filter_map(|l| match &l.event {
            SimEvent::FrameIn {
                conn: c,
                ours,
                asked,
                bytes,
                ..
            } if *c == conn => Some((l.at, *ours, *asked, *bytes)),
            _ => None,
        })
        .collect()
}

/// How many TX_CHRONOs the server sent `conn`.
fn chronos_to(log: &[Logged], conn: usize) -> usize {
    log.iter()
        .filter(|l| {
            matches!(&l.event, SimEvent::FrameOut { conn: c, header: Some(h), .. }
                if *c == conn && h.kind == tempo_tcisim::frame::kind::TX_CHRONO)
        })
        .count()
}

/// How many answers the server has read from the scene's client.
fn answers(sim: &Simulator) -> usize {
    frames_in(&sim.log(), 0).len()
}

/// ⭐ NEXUS'S AUDIO GOES OUT AS THE SDR PROGRAM PULLS IT. Each TX_CHRONO of our key is answered at
/// once with exactly the frames it asks for: the over resampled to the server's rate from its start,
/// whole and in order, with the operator's TX level applied as it leaves, and silence while the key
/// is ours and nothing is queued, before the over and after it. The same when the chronos come in
/// bursts at the same average rate. Each chrono is answered once, never more.
#[test]
fn nexus_audio_goes_out_as_the_sdr_program_pulls_it_with_silence_around_it() {
    for faults in [vec![], vec![Fault::ChronoBurst { burst: 4 }]] {
        let what = format!("{faults:?}");
        let sim = esdr3(faults);
        let (d, mut c) = audio_daemon(&sim, Options::default());
        let tx = route(&d);
        tx.set_level(0.8);
        assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
        let over = tone_at(1.0, 1_000.0, 0.5);
        tx.feed(&over);
        until("the over pulled", || tx.queued() == 0);
        let pulled = sim.tx_audio(0).len();
        until("ten answers after it", || {
            sim.tx_audio(0).len() >= pulled + 10 * FRAMES
        });
        assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
        wait_unkeyed(&d);
        // Each chrono answered once, never more (the last answers may still be on their way).
        assert!(
            sim.wait_for(WAIT, |log| frames_in(log, 0).len() == chronos_to(log, 0)),
            "{what}: each chrono answered once"
        );
        let log = sim.log();
        let sent = sim.tx_audio(0);
        let want = at_48k(&over, 0.8);
        let k = start_of(&sent, &want)
            .unwrap_or_else(|| panic!("{what}: the over, whole and in order, at the TX level"));
        assert!(
            sent[..k].iter().all(|s| *s == 0.0),
            "{what}: silence before it"
        );
        assert!(
            sent[k + want.len()..].iter().all(|s| *s == 0.0),
            "{what}: silence after it"
        );
        let frames = frames_in(&log, 0);
        assert!(
            frames
                .iter()
                .all(|(.., bytes)| *bytes == tempo_tcisim::frame::HEADER_LEN + 2 * FRAMES * 4),
            "{what}: every answer exactly its chrono's ask, 2048 float32 values: {frames:?}"
        );
        assert_eq!(
            sent.len(),
            frames.len() * FRAMES,
            "{what}: channel 0 of each"
        );
        let stop = command_at(&log, 0, "trx:0,false").expect("our stop");
        assert!(
            frames
                .iter()
                .filter(|(at, ..)| *at < stop)
                .all(|(_, ours, fit, _)| *ours && *fit == Some(Fit::Values)),
            "{what}: each answer for our key, in the chrono's own size: {frames:?}"
        );
    }
}

/// ⛔ AUDIO ONLY FOR OUR KEY. Audio handed to the route while nothing of ours
/// is keyed is dropped, never kept for a later key; another program's over, whose chronos reach
/// every client, gets none of ours; and our next key carries silence until its own over is handed
/// over, then that over alone.
#[test]
fn nexus_audio_goes_to_no_key_but_ours_and_waits_for_none() {
    let sim = esdr3([Fault::ForeignKey {
        after: Duration::from_millis(500),
        hold: Duration::from_millis(600),
        chrono_to_all: true,
    }]);
    let (d, mut c) = audio_daemon(&sim, Options::default());
    let tx = route(&d);
    tx.feed(&tone_at(0.5, 1_000.0, 0.5));
    assert_eq!(tx.queued(), 0, "nothing of ours keyed: dropped");
    wait_log(&sim, "the other program's key", |log| {
        keys(log).contains(&(Keyer::Operator, SimSource::Tci))
    });
    tx.feed(&tone_at(0.5, 1_000.0, 0.5));
    assert_eq!(tx.queued(), 0, "another program's key: dropped");
    wait_log(&sim, "its end", |log| unkeys(log) == 1);
    assert!(
        d.snapshot().chronos_not_ours > 0,
        "premise: the server asked us for audio during it"
    );
    assert_eq!(answers(&sim), 0, "none of ours went to it");
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    until("ten answers of our key", || answers(&sim) >= 10);
    let over = tone_at(0.3, 1_500.0, 0.5);
    tx.feed(&over);
    until("our over pulled", || tx.queued() == 0);
    let pulled = sim.tx_audio(0).len();
    until("two answers after it", || {
        sim.tx_audio(0).len() >= pulled + 2 * FRAMES
    });
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    wait_unkeyed(&d);
    let sent = sim.tx_audio(0);
    let k = start_of(&sent, &at_48k(&over, 1.0)).expect("our over, whole");
    assert!(
        sent[..k].iter().all(|s| *s == 0.0),
        "silence until our over: nothing kept from before our key"
    );
}

/// ⛔ STOP TX WINS AND FLUSHES. The radio loop's hard stop: the unkey, then the flush. The rest
/// of the over goes where it waits, with what the resampler had made ahead, so nothing of it reaches
/// the server after the stop: the session answers silence until the server reports the unkey, and
/// the next key carries silence until its own over is handed over, then that over from its first
/// sample.
#[test]
fn stop_tx_drops_the_rest_of_the_over_and_nothing_of_it_leaves_after() {
    let sim = esdr3([]);
    let (d, mut c) = audio_daemon(&sim, Options::default());
    let tx = route(&d);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    let n = answers(&sim);
    tx.feed(&tone_at(2.0, 1_000.0, 0.5));
    until("a third of a second of it", || answers(&sim) >= n + 16);
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    let dropped = tx.flush();
    assert!(
        dropped > 12_000,
        "premise: most of the over queued: {dropped}"
    );
    assert_eq!(tx.queued(), 0, "nothing left to send");
    wait_unkeyed(&d);
    let log = sim.log();
    let stop = command_at(&log, 0, "trx:0,false").expect("our stop");
    let before = frames_in(&log, 0)
        .iter()
        .filter(|(at, ..)| *at < stop)
        .count();
    let sent = sim.tx_audio(0);
    assert!(
        sent[before * FRAMES..].iter().all(|s| *s == 0.0),
        "audio after the stop"
    );
    // The next over.
    let first = sent.len();
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    until("two answers of the next key", || {
        sim.tx_audio(0).len() >= first + 2 * FRAMES
    });
    let second = tone_at(0.5, 1_500.0, 0.5);
    tx.feed(&second);
    until("the next over pulled", || tx.queued() == 0);
    let pulled = sim.tx_audio(0).len();
    until("two answers after it", || {
        sim.tx_audio(0).len() >= pulled + 2 * FRAMES
    });
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    wait_unkeyed(&d);
    let sent = &sim.tx_audio(0)[first..];
    let k = start_of(sent, &at_48k(&second, 1.0)).expect("the next over, from its first sample");
    assert!(
        sent[..k].iter().all(|s| *s == 0.0),
        "nothing of the stopped over at the next key"
    );
}

/// ⭐ A MID-OVER SWITCH ON TCI (WSJT-X's `m_restart`) REPLACES ONLY WHAT THE SERVER HAS NOT PULLED.
/// The server pulls the over in real time, so the rest of it waits in Nexus: what was sent is the
/// first message to the sample, what follows is the second, spliced at one place as if the over had
/// been built that way, and the over is as long as it was. The rebuild is made while the session
/// goes on answering the server (it is built slowly here, on purpose).
#[test]
fn a_switch_mid_over_on_tci_replaces_only_what_the_server_has_not_pulled() {
    let sim = esdr3([]);
    let (d, mut c) = audio_daemon(&sim, Options::default());
    let tx = route(&d);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    let first = tone_at(1.5, 1_000.0, 0.5);
    let second = tone_at(1.5, 1_500.0, 0.5);
    let fed_at = answers(&sim);
    tx.feed(&first);
    until("a third of a second of it", || answers(&sim) >= fed_at + 16);
    let before_switch = answers(&sim);
    let mut asked = None;
    let mut after_build = 0;
    let switched = tx.retune(&mut |left, before| {
        asked = Some((left, before));
        std::thread::sleep(Duration::from_millis(200));
        after_build = answers(&sim);
        second[second.len() - left - before..].to_vec()
    });
    assert!(switched, "the TCI route switches");
    let (left, before) = asked.expect("the rest was asked for");
    assert_eq!(before, 0, "no look-back: the resampler runs on");
    let during = after_build - before_switch;
    assert!(
        during >= 5,
        "the session went on answering while the rest was built: {during} answers in 200 ms"
    );
    until("the whole over", || tx.queued() == 0);
    let pulled = sim.tx_audio(0).len();
    until("two answers after it", || {
        sim.tx_audio(0).len() >= pulled + 2 * FRAMES
    });
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    wait_unkeyed(&d);
    let sent = sim.tx_audio(0);
    let k = first_audio(&sent, fed_at).expect("the over");
    let unswitched = at_48k(&first, 1.0);
    let over = &sent[k..k + unswitched.len()];
    assert!(
        sent[k + unswitched.len()..].iter().all(|s| *s == 0.0),
        "the over is as long as it was"
    );
    let pulled_before = (before_switch - k / FRAMES) * FRAMES / 4;
    assert!(
        left + pulled_before <= first.len(),
        "only what was not pulled is asked for: {left} left, {pulled_before} pulled"
    );
    let splice = first_difference(over, &unswitched).expect("the over was switched");
    assert!(
        splice >= (after_build - k / FRAMES) * FRAMES,
        "what was pulled before and during the build is untouched: first change at {splice}, {} \
         pulled",
        (after_build - k / FRAMES) * FRAMES
    );
    // One splice point, in the modem-rate over, explains every sample sent.
    let at = (splice / 4).saturating_sub(64)..splice / 4 + 160;
    let j = at
        .clone()
        .find(|&j| {
            let mut spliced = first[..j].to_vec();
            spliced.extend_from_slice(&second[j..]);
            first_difference(over, &at_48k(&spliced, 1.0)).is_none()
        })
        .unwrap_or_else(|| panic!("no single splice in {at:?} makes what was sent"));
    assert!(
        j >= pulled_before,
        "spliced after what was pulled: {j} < {pulled_before}"
    );
}

/// ⭐ Stop TX wins over a switch: a Stop that lands while the rest is being built leaves the queue
/// empty and the switch refused (so nothing is booked as sent), and a Stop just after a switch cuts
/// the new message as it cuts any over: silence from the next answer on.
#[test]
fn stop_tx_cuts_a_tci_over_during_and_after_a_switch() {
    let sim = esdr3([]);
    let (d, mut c) = audio_daemon(&sim, Options::default());
    let tx = route(&d);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    let second = tone_at(1.5, 1_500.0, 0.5);
    for stop_during in [true, false] {
        let n = answers(&sim);
        tx.feed(&tone_at(1.5, 1_000.0, 0.5));
        until("the over on the air", || answers(&sim) >= n + 10);
        let switched = tx.retune(&mut |left, before| {
            if stop_during {
                tx.flush();
            }
            second[second.len() - left - before..].to_vec()
        });
        assert_eq!(
            switched, !stop_during,
            "stop during the build: {stop_during}"
        );
        if !stop_during {
            tx.flush();
        }
        assert_eq!(
            tx.queued(),
            0,
            "stop during the build: {stop_during}: nothing left to send"
        );
        let cut = answers(&sim);
        until("ten answers after the stop", || answers(&sim) >= cut + 10);
        let sent = sim.tx_audio(0);
        assert!(
            sent[(cut + 2) * FRAMES..].iter().all(|s| *s == 0.0),
            "stop during the build: {stop_during}: audio after the stop"
        );
    }
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    wait_unkeyed(&d);
}

/// Nothing to replace, nothing asked: an empty queue refuses without building anything; a queue
/// that grew while the rest was built is left as it is (where its over ends is not known); and an
/// over no longer open (our stop sent, the flush still to come) is not switched, so the engine
/// books no message as sent that never was.
#[test]
fn a_tci_switch_with_nothing_left_a_grown_queue_or_no_over_open_changes_nothing() {
    let sim = esdr3([]);
    let (d, mut c) = audio_daemon(&sim, Options::default());
    let tx = route(&d);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    let mut asked = false;
    assert!(!tx.retune(&mut |_, _| {
        asked = true;
        Vec::new()
    }));
    assert!(!asked, "nothing queued: nothing built");
    let first = tone_at(1.0, 1_000.0, 0.5);
    tx.feed(&first);
    until("the over on the air", || tx.queued() < first.len());
    let extra = vec![0.1f32; 6_000];
    let mut queued_after_feed = 0;
    let switched = tx.retune(&mut |left, _| {
        tx.feed(&extra);
        queued_after_feed = tx.queued();
        vec![0.0; left]
    });
    assert!(!switched, "a grown queue is not replaced");
    assert!(
        tx.queued() + 3_000 >= queued_after_feed,
        "the queue left as it was: {} of {queued_after_feed}",
        tx.queued()
    );
    until("the over and what was added", || tx.queued() == 0);
    let pulled = sim.tx_audio(0).len();
    until("two answers after it", || {
        sim.tx_audio(0).len() >= pulled + 2 * FRAMES
    });
    let mut whole = first.clone();
    whole.extend_from_slice(&extra);
    assert!(
        start_of(&sim.tx_audio(0), &at_48k(&whole, 1.0)).is_some(),
        "the first over unchanged, what was added after it"
    );
    // Our stop sent, the rest still queued: the radio loop flushes it in the same tick.
    tx.feed(&tone_at(1.0, 1_000.0, 0.5));
    assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
    assert!(tx.queued() > 0, "premise: audio still queued");
    let mut asked = false;
    assert!(
        !tx.retune(&mut |left, _| {
            asked = true;
            vec![0.0; left]
        }),
        "no over open: not switched"
    );
    assert!(!asked, "nothing built");
    tx.flush();
    wait_unkeyed(&d);
}

/// ⭐⭐ NEVER A SILENT OVER, PART WAY THROUGH EITHER. The SDR program stops
/// pulling Nexus's audio part way through an over and stays keyed. The daemon ends the over: our
/// stop goes out once the last pull is older than the chrono window, and the radio loop reads why,
/// as it reads the session's own halts.
#[test]
fn an_over_whose_audio_the_sdr_program_stops_pulling_is_ended_and_says_why() {
    let sim = esdr3([Fault::ChronoStall {
        after: Duration::from_millis(400),
    }]);
    let (d, mut c) = audio_daemon(&sim, Options::default());
    let tx = route(&d);
    assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
    tx.feed(&tone_at(3.0, 1_000.0, 0.5));
    assert_eq!(
        halt(&d),
        "the SDR program stopped taking Nexus's audio part way through the over, so Nexus ended it"
    );
    wait_log(&sim, "our stop", |log| count_in(log, 0, "trx:0,false") >= 1);
    let log = sim.log();
    let stalled = log
        .iter()
        .find_map(|l| {
            matches!(
                l.event,
                SimEvent::Fault {
                    name: "ChronoStall",
                    ..
                }
            )
            .then_some(l.at)
        })
        .expect("the stall");
    let stop = command_at(&log, 0, "trx:0,false").expect("our stop");
    let after = stop.saturating_sub(stalled);
    assert!(
        after <= Duration::from_millis(1_500),
        "our stop {after:?} after the stall"
    );
    assert!(tx.queued() > 0, "premise: the over still had audio to send");
    wait_unkeyed(&d);
    assert_eq!(d.take_halt(), None, "one halt");
}

/// The watch on the last pull ends nothing a healthy server does: an over pulled to its end with
/// silence after it, chronos in bursts, a first chrono that comes late after an earlier over's last
/// pull (the session's own window rules until the first pull), and a server that stops pulling only
/// once the over's audio has all gone out (the over was whole; what is left is its tail).
#[test]
fn the_pull_watch_ends_nothing_a_healthy_server_does() {
    let trials: [(&str, Vec<Fault>, usize); 4] = [
        ("healthy", vec![], 1),
        ("bursts", vec![Fault::ChronoBurst { burst: 4 }], 1),
        (
            "late first chrono",
            vec![Fault::ChronoLate {
                delay: Duration::from_millis(150),
            }],
            2,
        ),
        (
            "stall in the tail",
            vec![Fault::ChronoStall {
                after: Duration::from_millis(1_500),
            }],
            1,
        ),
    ];
    for (what, faults, overs) in trials {
        let sim = esdr3(faults);
        let (d, mut c) = audio_daemon(&sim, Options::default());
        let tx = route(&d);
        for n in 0..overs {
            if n > 0 {
                // Well past the window since the last over's last pull.
                assert!(
                    !d.session()
                        .wait_until(Duration::from_millis(600), |s| s.keyed.is_some()),
                    "{what}: premise: nothing of ours keyed between the overs"
                );
                assert!(
                    tx.last_pull()
                        .is_some_and(|at| at.elapsed() >= Duration::from_millis(600)),
                    "{what}: premise: the last over's last pull is older than the window"
                );
            }
            assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
            tx.feed(&tone_at(1.0, 1_000.0, 0.5));
            // The over, then past the window and its poll, and past a stall in the tail: the
            // watch has had its chance.
            assert!(
                !d.session()
                    .wait_until(Duration::from_millis(2_500), |s| !s.over_open),
                "{what}: over {n} ended before our stop"
            );
            assert_eq!(tx.queued(), 0, "{what}: premise: the over pulled");
            assert_eq!(c.ask("T 0", 1), "RPRT 0\n");
            wait_unkeyed(&d);
        }
        assert_eq!(d.take_halt(), None, "{what}: ended by the watch");
    }
}

/// The transmit route is ready only once the SDR program has taken Nexus's audio stream: never on a
/// server that does not acknowledge `audio_start` (where a `tci` key would take its microphone),
/// never with Audio over TCI off, never for a daemon without Nexus's own audio paths; and ready on a
/// server that does.
#[test]
fn the_transmit_route_is_ready_only_once_the_sdr_program_takes_nexus_audio() {
    let sim = esdr3([Fault::NoAudioAck]);
    registry::forget(key_of(&sim));
    let d = TciDaemon::start_receiving(&target(&sim), Options::default()).expect("starts");
    wait_log(&sim, "the stream asked for", |log| {
        count_in(log, 0, "audio_start:0") == 1
    });
    assert!(
        !d.session().wait_until(Duration::from_millis(500), |s| s
            .model
            .trx(trx0())
            .audio
            .is_true()),
        "premise: never acknowledged"
    );
    assert!(!d.tx_route_ready(), "unacknowledged");
    drop(d);
    let sim = esdr3([]);
    registry::forget(key_of(&sim));
    let d = TciDaemon::start_receiving(
        &Target {
            audio: false,
            ..target(&sim)
        },
        Options::default(),
    )
    .expect("starts");
    assert!(!d.tx_route_ready(), "Audio over TCI off");
    drop(d);
    let d = fresh(&sim, Options::default());
    wait_audio(&d);
    assert!(
        d.tx_tee().is_none() && !d.tx_route_ready(),
        "no audio paths"
    );
    drop(d);
    registry::forget(key_of(&sim));
    let d = TciDaemon::start_receiving(&target(&sim), Options::default()).expect("starts");
    wait_audio(&d);
    assert!(d.tx_route_ready(), "control: acknowledged and flowing");
    assert!(d.tx_tee().is_some());
}

/// Streams that kill the session's driver once tripped, as a bug in the session would: the next
/// block handed to them panics.
struct Tripwire(Arc<AtomicBool>);

impl Tripwire {
    fn check(&self) {
        assert!(!self.0.load(Ordering::Relaxed), "the session's driver dies");
    }
}

impl Streams for Tripwire {
    fn receive(&mut self, _block: &tempo_net::tci::frame::Block<'_>) {
        self.check();
    }

    fn transmit(&mut self, _rate: u32, _out: &mut [f32]) -> usize {
        self.check();
        0
    }
}

/// ⭐ A SESSION WHOSE DRIVER DIES ENDS ITS WATCHER, AND A DEATH MID-OVER IS AN ALARM. A driver that
/// dies without reaching `Closed` drops the watcher's event channel, which then answers at once,
/// with nothing: a watcher that took that for a quiet server spun a whole core for the rest of the
/// session. The session must read closed (so the radio loop sees the daemon gone), the watcher must
/// end, the stops a close sends must still go out, and an over that was on the air must be an
/// alarm. The control: a driver that dies idle raises no alarm, and nothing keys either way.
#[test]
fn a_session_whose_driver_dies_ends_its_watcher() {
    for keyed in [false, true] {
        let sim = esdr3([]);
        registry::forget(key_of(&sim));
        let trip = Arc::new(AtomicBool::new(false));
        let d = TciDaemon::start_with(
            &target(&sim),
            Box::new(Tripwire(trip.clone())),
            Options::default(),
        )
        .expect("the daemon starts");
        d.set_tx_route(true);
        let mut c = Client::connect(&d);
        wait_audio(&d);
        if keyed {
            assert_eq!(c.ask("M PKTUSB 0", 1), "RPRT 0\n");
            assert_eq!(c.ask("T 1", 1), "RPRT 0\n");
            wait_log(&sim, "the key", |log| keys(log).len() == 1);
        }
        let stops_before = count_in(&sim.log(), 0, "trx:0,false");
        trip.store(true, Ordering::Relaxed);
        wait_dead(&d);
        let deadline = Instant::now() + WAIT;
        while !d.watch_thread.as_ref().is_some_and(|t| t.is_finished()) {
            assert!(
                Instant::now() < deadline,
                "keyed {keyed}: the watcher never ended"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        wait_log(&sim, "the stop", |log| {
            count_in(log, 0, "trx:0,false") > stops_before
        });
        let alarm = d.alarm();
        if keyed {
            let alarm = alarm.expect("an alarm");
            assert!(alarm.contains("may still be transmitting"), "{alarm}");
        } else {
            assert_eq!(alarm, None);
        }
        assert_eq!(keys(&sim.log()).len(), usize::from(keyed));
        drop(c);
        drop(d);
    }
}
