//! The session against the simulated SDR program (`tempo-tcisim`), over a real WebSocket on
//! loopback: each simulated dialect, and every fault the simulator injects that a session guard
//! exists for. Each fault is the positive control for its guard: the test is red when
//! the guard is removed.
//!
//! Waits are bounded and wait on the session's events or the simulator's log, never a bare sleep,
//! except where a test must let time pass to show that nothing happens. Where a window decides
//! the outcome, the window is set wide for the test, so the margin is the window's and not the
//! box's load; the exact windows are tested on the manual clock (`tests`).

use std::f64::consts::TAU;
use std::time::{Duration, Instant};

use tempo_tcisim::frame::Fit;
use tempo_tcisim::{
    Cause, Closer, Config as SimConfig, Cut, Dialect as SimDialect, Event as SimEvent, Fault,
    Keyer, Logged, Simulator, Source as SimSource,
};

use super::*;
use crate::tci::admission::Refusal;
use crate::tci::dialect::UNKNOWN;
use crate::tci::reconnect::{self, Ladder, Step};
use crate::tci::{DriveForm, EXPERT_SDR2};

/// The longest any one expected thing may take here.
const WAIT: Duration = Duration::from_secs(5);

fn trx0() -> Trx {
    Trx::new(0).expect("trx 0")
}

/// Transmit audio: a 1 kHz tone at half scale, so the simulator can be asked where it landed.
#[derive(Default)]
struct Tone {
    phase: f64,
}

const TONE_HZ: f64 = 1_000.0;

impl Streams for Tone {
    fn transmit(&mut self, rate: u32, out: &mut [f32]) -> usize {
        let step = TAU * TONE_HZ / f64::from(rate);
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

fn connect(sim: &Simulator, config: Config) -> Connection {
    Connection::connect(sim.addr(), config, Box::new(Tone::default())).expect("connected")
}

/// Wait for an event `pick` accepts; every event seen up to it, that one last.
fn expect(conn: &Connection, what: &str, pick: impl Fn(&Event) -> bool) -> Vec<Event> {
    let deadline = Instant::now() + WAIT;
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "no {what} within {WAIT:?}; saw {seen:?}");
        if let Some(event) = conn.next_event(left) {
            let hit = pick(&event);
            seen.push(event);
            if hit {
                return seen;
            }
        }
    }
}

/// Every event that comes within `quiet`.
fn drain(conn: &Connection, quiet: Duration) -> Vec<Event> {
    let mut seen = Vec::new();
    while let Some(event) = conn.next_event(quiet) {
        seen.push(event);
    }
    seen
}

/// Ready, and (with Nexus's audio on) the stream acknowledged and receive audio flowing. The
/// `ready` event.
fn ready(conn: &Connection, audio: bool) -> Event {
    let ready = expect(conn, "ready", |e| matches!(e, Event::Ready { .. }))
        .pop()
        .expect("the ready event");
    if audio {
        assert!(
            conn.wait_until(WAIT, |s| s.rx_audio_fresh
                && s.model.trx(trx0()).audio.is_true()),
            "receive audio never flowed: {:?}",
            conn.snapshot().model.trx(trx0())
        );
    }
    ready
}

fn wait_log(sim: &Simulator, what: &str, done: impl Fn(&[Logged]) -> bool) {
    assert!(
        sim.wait_for(WAIT, |log| done(log)),
        "the simulator never logged {what}"
    );
}

fn keyed_in_sim(log: &[Logged]) -> Vec<(Keyer, SimSource)> {
    log.iter()
        .filter_map(|l| match l.event {
            SimEvent::Keyed { by, source, .. } => Some((by, source)),
            _ => None,
        })
        .collect()
}

fn unkeyed_in_sim(log: &[Logged]) -> bool {
    log.iter()
        .any(|l| matches!(l.event, SimEvent::Unkeyed { .. }))
}

/// The time of the first command from `conn` in `log` that is exactly `text` (no `;`). Inside a
/// [`wait_log`] use the log it hands over: the simulator holds its log's lock while it asks.
fn command_in(log: &[Logged], conn: usize, text: &str) -> Option<Duration> {
    log.iter().find_map(|l| match &l.event {
        SimEvent::Command {
            conn: c, text: t, ..
        } if *c == conn && t == text => Some(l.at),
        _ => None,
    })
}

fn command_at(sim: &Simulator, conn: usize, text: &str) -> Option<Duration> {
    command_in(&sim.log(), conn, text)
}

/// The first command the simulator read from `conn`, once it has read one.
fn first_command(sim: &Simulator, conn: usize) -> String {
    wait_log(sim, "a command", |log| {
        log.iter()
            .any(|l| matches!(l.event, SimEvent::Command { conn: c, .. } if c == conn))
    });
    sim.commands_from(conn).remove(0)
}

/// A whole over of ours, keyed and ended, with the simulator's word on each half. An over of
/// Nexus's audio is ended only once the server has pulled some.
fn an_over(conn: &Connection, sim: &Simulator, start: TxStart) {
    conn.key(start).expect("admitted");
    expect(conn, "the echo", |e| *e == Event::KeyConfirmed(start));
    if start == TxStart::Audio {
        assert!(
            conn.wait_until(WAIT, |s| s.timing.first_chrono_ms.is_some()),
            "no TX_CHRONO came"
        );
    }
    assert!(matches!(conn.end_ours(), StopOutcome::Sent(_)));
    expect(conn, "the unkey's report", |e| *e == Event::Unkeyed);
    wait_log(sim, "the unkey", unkeyed_in_sim);
}

#[test]
fn every_simulated_dialect_is_spoken_to_by_what_it_does() {
    for dialect in SimDialect::all() {
        let name = dialect.name.clone();
        let s = sim(dialect, []);
        let conn = connect(&s, Config::new(trx0()));
        let Event::Ready {
            dialect: pinned, ..
        } = ready(&conn, true)
        else {
            unreachable!()
        };
        if name.starts_with("ExpertSDR2") {
            // TCI 1.x: no source on a key, the drive global, the S-meter polled.
            assert_eq!(pinned, EXPERT_SDR2, "{name}");
            assert_eq!(pinned.drive, DriveForm::Global, "{name}");
            assert_eq!(
                conn.key(TxStart::Audio),
                Err(Some(Refusal::NoSourceArgument {
                    dialect: EXPERT_SDR2.name
                })),
                "{name}"
            );
            // The 1.x model (unverified): a key with no source takes the client's
            // stream while it runs. The session answers none of its chronos: nothing of Nexus's
            // goes out on a key it did not make for its audio.
            conn.key(TxStart::Microphone).expect("admitted");
            expect(&conn, "the echo", |e| {
                *e == Event::KeyConfirmed(TxStart::Microphone)
            });
            assert!(
                conn.wait_until(WAIT, |s| s.chronos_not_ours > 0),
                "{name}: the positive control: the server pulled"
            );
            conn.end_ours();
            expect(&conn, "the unkey's report", |e| *e == Event::Unkeyed);
            assert_eq!(
                keyed_in_sim(&s.log()),
                [(Keyer::Client(0), SimSource::Tci)],
                "{name}"
            );
            assert!(
                !s.log()
                    .iter()
                    .any(|l| matches!(l.event, SimEvent::FrameIn { conn: 0, .. })),
                "{name}: audio for a microphone key"
            );
            assert!(
                s.wait_for(WAIT, |log| log.iter().any(
                    |l| matches!(&l.event, SimEvent::Command { name, .. } if name == "rx_smeter")
                )),
                "{name}: the S-meter is polled"
            );
        } else {
            // Every other profile announces ExpertSDR3: a key names Nexus's audio.
            assert_eq!(pinned, EXPERT_SDR3, "{name}");
            an_over(&conn, &s, TxStart::Audio);
            assert_eq!(
                keyed_in_sim(&s.log()),
                [(Keyer::Client(0), SimSource::Tci)],
                "{name}"
            );
            let answers: Vec<(Option<Fit>, bool)> = s
                .log()
                .iter()
                .filter_map(|l| match l.event {
                    SimEvent::FrameIn {
                        conn: 0,
                        asked,
                        ours,
                        ..
                    } => Some((asked, ours)),
                    _ => None,
                })
                .collect();
            assert!(
                !answers.is_empty(),
                "{name}: no transmit audio reached the server"
            );
            for (asked, _) in &answers {
                assert!(
                    matches!(asked, Some(Fit::Values | Fit::Both)),
                    "{name}: an answer the size of no chrono: {asked:?}"
                );
            }
            let timing = conn.snapshot().timing;
            assert!(
                timing.echo_ms.is_some() && timing.first_chrono_ms.is_some(),
                "{name}: {timing:?}"
            );
        }
        assert!(!conn.ours_on_air(), "{name}");
        conn.close();
    }
}

#[test]
fn a_digital_over_carries_nexus_audio_and_only_while_it_is_ours() {
    let s = sim(SimDialect::expert_sdr3(), []);
    let conn = connect(&s, Config::new(trx0()));
    ready(&conn, true);
    conn.key(TxStart::Audio).expect("admitted");
    expect(&conn, "the echo", |e| {
        *e == Event::KeyConfirmed(TxStart::Audio)
    });
    // Let some audio go out.
    wait_log(&s, "a second of transmit audio", |log| {
        log.iter()
            .filter(|l| matches!(l.event, SimEvent::FrameIn { conn: 0, .. }))
            .count()
            >= 40
    });
    conn.end_ours();
    expect(&conn, "the unkey's report", |e| *e == Event::Unkeyed);
    let audio = s.tx_audio(0);
    let level = tempo_tcisim::synth::tone_level(&audio, 48_000, TONE_HZ);
    assert!(
        level > 0.4,
        "the tone at {level}, from {} samples",
        audio.len()
    );
    // One answer per chrono and never more: the server asks only while the key is up, so every
    // answer is to a chrono of our over. The last chrono before the unkey may cross our report of
    // it and go unanswered.
    std::thread::sleep(Duration::from_millis(300));
    let log = s.log();
    let chronos = log
        .iter()
        .filter(|l| {
            matches!(
                l.event,
                SimEvent::FrameOut {
                    conn: 0,
                    header: Some(h),
                    ..
                } if h.kind == tempo_tcisim::frame::kind::TX_CHRONO
            )
        })
        .count();
    let answers = log
        .iter()
        .filter(|l| matches!(l.event, SimEvent::FrameIn { conn: 0, .. }))
        .count();
    assert!(
        answers <= chronos && answers + 1 >= chronos,
        "{answers} answers to {chronos} chronos"
    );
    // Nothing once the server's word on the unkey had time to arrive.
    let unkeyed = log
        .iter()
        .find(|l| matches!(l.event, SimEvent::Unkeyed { .. }))
        .expect("unkeyed")
        .at;
    let late: Vec<Duration> = log
        .iter()
        .filter(|l| matches!(l.event, SimEvent::FrameIn { conn: 0, .. }) && l.at > unkeyed)
        .map(|l| l.at - unkeyed)
        .collect();
    assert!(late.len() <= 1, "answers after the unkey: {late:?}");
}

/// Nothing keyed, nothing of the microphone, no key sent.
fn no_key_reached(sim: &Simulator) {
    std::thread::sleep(Duration::from_millis(200));
    let log = sim.log();
    assert!(keyed_in_sim(&log).is_empty(), "keyed");
    assert!(
        !log.iter()
            .any(|l| matches!(l.event, SimEvent::MicFallback { .. })),
        "the microphone"
    );
    assert!(
        !sim.commands().iter().any(|c| c.starts_with("trx:0,true")),
        "{:?}",
        sim.commands()
    );
}

#[test]
fn a_digital_key_is_refused_while_the_server_ignores_audio_start() {
    // The server ignores `audio_start`: no acknowledgement and no frames, and a
    // `tci` key there takes the SDR's microphone.
    let s = sim(SimDialect::expert_sdr3(), [Fault::NoAudioAck]);
    let conn = connect(&s, Config::new(trx0()));
    ready(&conn, false);
    wait_log(&s, "the ignored audio_start", |log| {
        log.iter()
            .any(|l| matches!(&l.event, SimEvent::Ignored { text, .. } if text == "audio_start:0"))
    });
    assert_eq!(
        conn.key(TxStart::Audio),
        Err(Some(Refusal::AudioNotAcknowledged))
    );
    no_key_reached(&s);
}

#[test]
fn a_digital_key_is_refused_while_frames_flow_unacknowledged() {
    // Receive audio flows, so only the acknowledgement term can refuse the key.
    let quiet = SimDialect {
        audio_ack: false,
        ..SimDialect::expert_sdr3()
    };
    let s = sim(quiet, []);
    let conn = connect(&s, Config::new(trx0()));
    ready(&conn, false);
    assert!(
        conn.wait_until(WAIT, |s| s.rx_audio_fresh),
        "the positive control: receive audio flows"
    );
    assert_eq!(
        conn.key(TxStart::Audio),
        Err(Some(Refusal::AudioNotAcknowledged))
    );
    no_key_reached(&s);
}

#[test]
fn receive_audio_that_stops_refuses_a_digital_key() {
    let s = sim(
        SimDialect::expert_sdr3(),
        [Fault::RxAudioStall {
            after: Duration::from_millis(300),
        }],
    );
    let conn = connect(&s, Config::new(trx0()));
    ready(&conn, true);
    assert!(
        conn.wait_until(WAIT, |s| !s.rx_audio_fresh),
        "the stall never showed"
    );
    assert_eq!(conn.key(TxStart::Audio), Err(Some(Refusal::NoReceiveAudio)));
    assert!(keyed_in_sim(&s.log()).is_empty());
    // A microphone key does not need Nexus's audio.
    an_over(&conn, &s, TxStart::Microphone);
}

#[test]
fn a_digital_key_the_server_never_pulls_is_ended_by_the_chrono_window() {
    let s = sim(SimDialect::expert_sdr3(), [Fault::NoChrono]);
    let conn = connect(&s, Config::new(trx0()));
    ready(&conn, true);
    conn.key(TxStart::Audio).expect("admitted");
    let mut events = expect(&conn, "the halt", |e| *e == Event::Halted(Halt::NoChrono));
    events.extend(expect(&conn, "the unkey's report", |e| {
        *e == Event::Unkeyed
    }));
    assert!(
        events.contains(&Event::KeyConfirmed(TxStart::Audio)),
        "the server echoed the key: only the chrono was missing ({events:?})"
    );
    let key = command_at(&s, 0, "trx:0,true,tci").expect("our key");
    let stop = command_at(&s, 0, "trx:0,false").expect("our stop");
    assert!(
        stop - key < Duration::from_millis(1_500),
        "the stop came {:?} after the key",
        stop - key
    );
    assert!(s.events().iter().any(|e| matches!(
        e,
        SimEvent::Unkeyed {
            cause: Cause::Asked(Keyer::Client(0)),
            ..
        }
    )));
}

#[test]
fn a_late_first_chrono_inside_the_window_keeps_the_over_and_past_it_ends_it() {
    let window = Config {
        chrono_window_ms: 600,
        ..Config::new(trx0())
    };
    for (delay_ms, ends) in [(100, false), (1_500, true)] {
        let s = sim(
            SimDialect::expert_sdr3(),
            [Fault::ChronoLate {
                delay: Duration::from_millis(delay_ms),
            }],
        );
        let conn = connect(&s, window);
        ready(&conn, true);
        conn.key(TxStart::Audio).expect("admitted");
        if ends {
            expect(&conn, "the halt", |e| *e == Event::Halted(Halt::NoChrono));
        } else {
            wait_log(&s, "our answer to the late chrono", |log| {
                log.iter()
                    .any(|l| matches!(l.event, SimEvent::FrameIn { conn: 0, .. }))
            });
            std::thread::sleep(Duration::from_millis(700));
            let events = drain(&conn, Duration::from_millis(50));
            assert!(
                !events.iter().any(|e| matches!(e, Event::Halted(_))),
                "{delay_ms} ms: {events:?}"
            );
            assert!(conn.audio_over_open());
            let first = conn.snapshot().timing.first_chrono_ms.expect("measured");
            assert!(first >= delay_ms, "the first chrono after {first} ms");
            conn.end_ours();
            expect(&conn, "the unkey's report", |e| *e == Event::Unkeyed);
        }
    }
}

#[test]
fn a_link_lost_mid_over_keeps_the_belief_and_the_next_session_unkeys_first() {
    let s = sim(
        SimDialect::expert_sdr3(),
        [Fault::DisconnectMidOver {
            after: Duration::from_millis(200),
            radio_stays_keyed: true,
        }],
    );
    let mut ladder = Ladder::new();
    let conn = connect(&s, Config::new(trx0()));
    ready(&conn, true);
    ladder.ready();
    conn.key(TxStart::Audio).expect("admitted");
    let events = expect(&conn, "the close", |e| matches!(e, Event::Closed { .. }));
    let Some(Event::Closed { end, carried }) = events.last().cloned() else {
        unreachable!()
    };
    assert_eq!(end, End::Lost);
    let owed = OurKey {
        trx: trx0(),
        kind: TxStart::Audio,
    };
    assert_eq!(carried, Some(owed));
    assert!(conn.ours_on_air(), "a lost link never clears the belief");
    wait_log(&s, "the orphaned key", |log| {
        log.iter()
            .any(|l| matches!(l.event, SimEvent::Orphaned { .. }))
    });
    assert!(s.radio().trx[0].transmitting(), "the radio stays keyed");
    // The ladder: the belief handed on, and a retry at once.
    let steps = ladder.ended(&reconnect::End::Ended(end), carried);
    assert_eq!(steps, [Step::StillKeyed(owed), Step::Retry { after_ms: 0 }]);
    let next = connect(
        &s,
        Config {
            carried: Some(owed),
            ..Config::new(trx0())
        },
    );
    assert!(next.ours_on_air(), "the next session starts keyed");
    expect(&next, "ready", |e| matches!(e, Event::Ready { .. }));
    expect(&next, "the unkey's report", |e| *e == Event::Unkeyed);
    assert_eq!(
        first_command(&s, 1),
        "trx:0,false",
        "the first command after ready"
    );
    assert!(!next.ours_on_air());
    assert!(!s.radio().trx[0].transmitting());
}

#[test]
fn a_band_restore_above_the_ceiling_refuses_the_key_until_the_drive_is_written_again() {
    let s = sim(
        SimDialect::expert_sdr3(),
        [Fault::BandRestore {
            drive: 100,
            tune_drive: 100,
            modulation: None,
        }],
    );
    let conn = connect(&s, Config::new(trx0()));
    ready(&conn, true);
    let drive_is =
        |level: u32| conn.wait_until(WAIT, |s| s.model.drive(trx0()).known() == Some(&level));
    let write = || {
        conn.send(Command::Set(Setting::drive(trx0(), 30, &EXPERT_SDR3)))
            .expect("sent");
    };
    write();
    assert!(drive_is(30), "the drive's echo");
    an_over(&conn, &s, TxStart::Audio);
    // Retune to 40 m: the server restores that band's drive.
    conn.send(Command::Set(Setting::Vfo {
        trx: trx0(),
        channel: Channel::A,
        hz: Hz::new(7_074_000).expect("hz"),
    }))
    .expect("sent");
    assert!(drive_is(100), "the band's drive restored");
    let refused = Err(Some(Refusal::DriveAboveCeiling {
        drive: 100,
        ceiling: 30,
        tune: false,
    }));
    assert_eq!(conn.key(TxStart::Audio), refused);
    assert_eq!(conn.key(TxStart::Microphone), refused);
    write();
    assert!(drive_is(30));
    an_over(&conn, &s, TxStart::Audio);
    assert_eq!(keyed_in_sim(&s.log()).len(), 2, "two overs, none at 100");
}

#[test]
fn the_sdr_stopping_mid_over_halts_it_and_says_why() {
    let s = sim(
        SimDialect::expert_sdr3(),
        [Fault::ServerStop {
            after: Duration::from_millis(150),
        }],
    );
    let conn = connect(&s, Config::new(trx0()));
    ready(&conn, true);
    conn.key(TxStart::Audio).expect("admitted");
    let events = expect(&conn, "the halt", |e| *e == Event::Halted(Halt::SdrStopped));
    assert!(events.contains(&Event::Running(false)), "{events:?}");
    // No echo will come for a stopped SDR; our stop is answered with its state.
    expect(&conn, "the unkey's report", |e| *e == Event::Unkeyed);
    assert!(command_at(&s, 0, "trx:0,false").is_some());
    assert_eq!(
        conn.key(TxStart::Microphone),
        Err(Some(Refusal::SdrStopped { unreadable: false }))
    );
}

#[test]
fn stops_reach_the_server_in_every_state() {
    // A server that never says ready: stops still go out.
    let s = sim(SimDialect::expert_sdr3(), [Fault::NoReady]);
    let conn = connect(
        &s,
        Config {
            ready_timeout_ms: 60_000,
            ..Config::new(trx0())
        },
    );
    assert!(
        conn.wait_until(WAIT, |s| s.model.running.is_true()),
        "the burst"
    );
    assert_eq!(conn.snapshot().phase, Phase::AwaitReady);
    assert_eq!(conn.end_ours(), StopOutcome::Sent(vec![Stop::Trx(trx0())]));
    assert!(matches!(
        conn.stop(Stop::Tune(trx0())),
        StopOutcome::Sent(_)
    ));
    wait_log(&s, "both stops", |log| {
        command_in(log, 0, "trx:0,false").is_some() && command_in(log, 0, "tune:0,false").is_some()
    });
    // Ready and idle, nothing of ours keyed: still sent.
    let s2 = sim(SimDialect::expert_sdr3(), []);
    let conn2 = connect(&s2, Config::new(trx0()));
    ready(&conn2, true);
    assert_eq!(conn2.end_ours(), StopOutcome::Sent(vec![Stop::Trx(trx0())]));
    wait_log(&s2, "the stop", |log| {
        command_in(log, 0, "trx:0,false").is_some()
    });
    // And before every close.
    conn.close();
    wait_log(&s, "T 0's stop, then the close's own", |log| {
        log.iter()
            .filter(|l| matches!(&l.event, SimEvent::Command { text, .. } if text == "trx:0,false"))
            .count()
            == 2
    });
}

#[test]
fn no_key_goes_out_while_the_server_forbids_transmit() {
    for (fault, refusal) in [
        (
            Fault::TxEnableFalse,
            Refusal::TxNotEnabled { unreadable: false },
        ),
        (
            Fault::ReceiveOnly,
            Refusal::ReceiveOnly { unreadable: false },
        ),
    ] {
        let name = fault.name();
        let s = sim(SimDialect::expert_sdr3(), [fault]);
        let conn = connect(&s, Config::new(trx0()));
        ready(&conn, true);
        for start in [TxStart::Audio, TxStart::Microphone, TxStart::Tune] {
            assert_eq!(
                conn.key(start),
                Err(Some(refusal.clone())),
                "{name} {start:?}"
            );
        }
        std::thread::sleep(Duration::from_millis(100));
        let keys: Vec<String> = s
            .commands()
            .into_iter()
            .filter(|c| c.starts_with("trx:0,true") || c.starts_with("tune:0,true"))
            .collect();
        assert!(keys.is_empty(), "{name}: {keys:?}");
    }
}

#[test]
fn a_server_that_never_echoes_is_unkeyed_escalated_and_believed_on_its_next_burst() {
    let fast = Config {
        key_echo_ms: 400,
        unkey_deadline_ms: 600,
        ..Config::new(trx0())
    };
    let s = sim(SimDialect::expert_sdr3(), [Fault::NoEcho]);
    let conn = connect(&s, fast);
    ready(&conn, true);
    conn.key(TxStart::Microphone).expect("admitted");
    expect(&conn, "the halt", |e| *e == Event::Halted(Halt::NoEcho));
    let events = expect(&conn, "the close", |e| matches!(e, Event::Closed { .. }));
    assert!(events.contains(&Event::UnkeyUnconfirmed), "{events:?}");
    let owed = OurKey {
        trx: trx0(),
        kind: TxStart::Microphone,
    };
    assert_eq!(
        events.last(),
        Some(&Event::Closed {
            end: End::UnkeyUnconfirmed,
            carried: Some(owed)
        })
    );
    // The radio did unkey (it only never says so): the next burst's `trx:0,false;` is its word.
    assert!(!s.radio().trx[0].transmitting());
    let next = connect(
        &s,
        Config {
            carried: Some(owed),
            ..fast
        },
    );
    expect(&next, "ready", |e| matches!(e, Event::Ready { .. }));
    assert!(!next.ours_on_air());
    assert_eq!(first_command(&s, 1), "trx:0,false");
}

#[test]
fn a_stuck_transmitter_is_escalated_and_stopped_again_by_every_session() {
    let fast = Config {
        unkey_deadline_ms: 500,
        ..Config::new(trx0())
    };
    let s = sim(SimDialect::expert_sdr3(), [Fault::StuckTrx]);
    let conn = connect(&s, fast);
    ready(&conn, true);
    conn.key(TxStart::Audio).expect("admitted");
    expect(&conn, "the echo", |e| {
        *e == Event::KeyConfirmed(TxStart::Audio)
    });
    conn.end_ours();
    let events = expect(&conn, "the close", |e| matches!(e, Event::Closed { .. }));
    assert!(events.contains(&Event::UnkeyUnconfirmed), "{events:?}");
    assert!(!events.contains(&Event::Unkeyed), "{events:?}");
    let owed = OurKey {
        trx: trx0(),
        kind: TxStart::Audio,
    };
    // Sent again before the close.
    wait_log(&s, "the stop sent again", |log| {
        log.iter()
            .filter(|l| {
                matches!(&l.event, SimEvent::Command { conn: 0, text, .. } if text == "trx:0,false")
            })
            .count()
            >= 2
    });
    // On the ladder, never at once: it has already escalated.
    let mut ladder = Ladder::new();
    ladder.ready();
    assert_eq!(
        ladder.ended(&reconnect::End::Ended(End::UnkeyUnconfirmed), Some(owed)),
        [Step::StillKeyed(owed), Step::Retry { after_ms: 1_000 }]
    );
    // The next session finds it still keyed, stops it first, and escalates again.
    let next = connect(
        &s,
        Config {
            carried: Some(owed),
            ..fast
        },
    );
    let events = expect(&next, "the close", |e| matches!(e, Event::Closed { .. }));
    assert!(events.contains(&Event::UnkeyUnconfirmed), "{events:?}");
    assert_eq!(first_command(&s, 1), "trx:0,false");
    assert!(next.ours_on_air());
}

#[test]
fn a_half_open_link_halts_the_over_and_ends_stale() {
    let fast = Config {
        keyed_silence_ms: 400,
        stale_ms: 1_500,
        ..Config::new(trx0())
    };
    let s = sim(
        SimDialect::expert_sdr3(),
        [Fault::HalfOpen {
            after: Duration::from_millis(1_000),
        }],
    );
    let conn = connect(&s, fast);
    ready(&conn, true);
    conn.key(TxStart::Audio).expect("admitted");
    expect(&conn, "the echo", |e| {
        *e == Event::KeyConfirmed(TxStart::Audio)
    });
    let events = expect(&conn, "the close", |e| matches!(e, Event::Closed { .. }));
    assert!(events.contains(&Event::Halted(Halt::Silent)), "{events:?}");
    assert!(!events.contains(&Event::UnkeyUnconfirmed), "{events:?}");
    assert_eq!(
        events.last(),
        Some(&Event::Closed {
            end: End::Stale,
            carried: Some(OurKey {
                trx: trx0(),
                kind: TxStart::Audio
            })
        })
    );
    // Idle, the same link just ends.
    let s2 = sim(
        SimDialect::expert_sdr3(),
        [Fault::HalfOpen {
            after: Duration::from_millis(200),
        }],
    );
    let conn2 = connect(&s2, fast);
    ready(&conn2, true);
    let events = expect(&conn2, "the close", |e| matches!(e, Event::Closed { .. }));
    assert_eq!(
        events.last(),
        Some(&Event::Closed {
            end: End::Stale,
            carried: None
        })
    );
}

#[test]
fn commands_split_and_concatenated_are_read_one_by_one() {
    // The burst in one message, nothing cut.
    let s = sim(
        SimDialect::expert_sdr3(),
        [Fault::SplitConcat { cut: None }],
    );
    let conn = connect(&s, Config::new(trx0()));
    ready(&conn, true);
    let snap = conn.snapshot();
    assert!(snap.model.trx(trx0()).tx_enable.is_true());
    assert_eq!(snap.text_refused, 0);
    an_over(&conn, &s, TxStart::Audio);
    // The first `trx` line cut across two messages: unknown, then asked again, never misread.
    let cut = Cut {
        starting: "trx:0".into(),
        at: 6,
    };
    let s2 = sim(
        SimDialect::expert_sdr3(),
        [Fault::SplitConcat { cut: Some(cut) }],
    );
    let conn2 = connect(&s2, Config::new(trx0()));
    ready(&conn2, true);
    wait_log(&s2, "the cut", |log| {
        log.iter()
            .any(|l| matches!(&l.event, SimEvent::TextOut { text, .. } if text.ends_with("trx:0,")))
    });
    wait_log(&s2, "the read", |log| command_in(log, 0, "trx:0").is_some());
    assert!(conn2.wait_until(WAIT, |s| s.model.trx(trx0()).transmitting.is_false()));
    assert!(conn2.snapshot().text_refused >= 2, "both halves refused");
    an_over(&conn2, &s2, TxStart::Audio);
}

/// ONE CLICK ON THE SDR PROGRAM'S PANORAMA IS HEARD ONCE, ON EVERY DIALECT THAT SENDS CLICKS:
/// ExpertSDR3 and AetherSDR send each click in both spellings, back to back, and are heard once,
/// with the channel; their click on another transceiver's panorama is not ours. ExpertSDR2 sends
/// only the legacy spelling, which names no transceiver: each of its clicks is heard once, after
/// its wait. A click writes nothing and keys nothing.
#[test]
fn a_click_on_the_sdr_panorama_is_heard_once_on_every_dialect() {
    let cases = [
        (SimDialect::expert_sdr3(), vec!["K1ABC 14025000 Some(0)"]),
        (SimDialect::aether_sdr(), vec!["K1ABC 14025000 Some(0)"]),
        (
            SimDialect::expert_sdr2(),
            vec!["W1AW 7030000 None", "K1ABC 14025000 None"],
        ),
    ];
    for (dialect, want) in cases {
        let name = dialect.name.clone();
        let s = sim(dialect, []);
        let conn = connect(
            &s,
            Config {
                audio: false,
                ..Config::new(trx0())
            },
        );
        ready(&conn, false);
        s.click_spot_on(1, 0, "W1AW", 7_030_000);
        s.click_spot("K1ABC", 14_025_000);
        let mut seen = expect(
            &conn,
            "the click on our panorama",
            |e| matches!(e, Event::SpotClicked { call, .. } if call.as_str() == "K1ABC"),
        );
        seen.extend(drain(&conn, Duration::from_millis(2 * CLICK_PAIR_MS)));
        let heard: Vec<String> = seen
            .iter()
            .filter_map(|e| match e {
                Event::SpotClicked { call, hz, channel } => Some(format!(
                    "{} {} {:?}",
                    call.as_str(),
                    hz.get(),
                    channel.map(Channel::get)
                )),
                _ => None,
            })
            .collect();
        assert_eq!(heard, want, "{name}");
        assert!(
            keyed_in_sim(&s.log()).is_empty(),
            "{name}: a click keys nothing"
        );
    }
}

#[test]
fn oversized_and_garbage_messages_are_refused_and_the_session_goes_on() {
    for fault in [
        Fault::Oversized {
            bytes: 200_000,
            binary: false,
        },
        Fault::Oversized {
            bytes: 200_000,
            binary: true,
        },
        Fault::Garbage,
    ] {
        let name = format!("{fault:?}");
        let s = sim(SimDialect::expert_sdr3(), [fault]);
        let conn = connect(&s, Config::new(trx0()));
        ready(&conn, true);
        assert!(
            conn.wait_until(WAIT, |s| s.text_refused + s.frames.refused > 0),
            "{name}: nothing was refused"
        );
        // Garbage leaves `trx` and `drive` unknown until they are read again.
        assert!(
            conn.wait_until(WAIT, |s| s.model.trx(trx0()).transmitting.is_false()
                && s.model.drive(trx0()).known().is_some()),
            "{name}: {:?}",
            conn.snapshot().model.trx(trx0())
        );
        an_over(&conn, &s, TxStart::Audio);
        assert_eq!(conn.snapshot().phase, Phase::Ready, "{name}");
    }
    // Past what the WebSocket layer holds: the session ends, with its stops, and nothing panics.
    let s = sim(
        SimDialect::expert_sdr3(),
        [Fault::Oversized {
            bytes: 2 * MAX_WS_MESSAGE,
            binary: false,
        }],
    );
    let conn = connect(&s, Config::new(trx0()));
    let events = expect(&conn, "the close", |e| matches!(e, Event::Closed { .. }));
    assert!(
        matches!(
            events.last(),
            Some(Event::Closed {
                end: End::ProtocolError(_),
                carried: None
            })
        ),
        "{events:?}"
    );
}

#[test]
fn an_unknown_server_is_spoken_to_by_what_it_does() {
    let mystery = Fault::UnknownVersion {
        program: "MysterySDR".into(),
        version: "9.9".into(),
    };
    let s = sim(SimDialect::expert_sdr3(), [mystery.clone()]);
    let conn = connect(&s, Config::new(trx0()));
    let ready_event = ready(&conn, true);
    assert!(
        matches!(
            &ready_event,
            Event::Ready { dialect, program: Some(p), .. }
                if *dialect == UNKNOWN && p == "MysterySDR"
        ),
        "{ready_event:?}"
    );
    assert_eq!(
        conn.key(TxStart::Audio),
        Err(Some(Refusal::NoSourceArgument {
            dialect: UNKNOWN.name
        }))
    );
    an_over(&conn, &s, TxStart::Microphone);
    // The operator names the server: its keys take Nexus's audio.
    let s2 = sim(SimDialect::expert_sdr3(), [mystery]);
    let conn2 = connect(
        &s2,
        Config {
            preference: Preference::ExpertSdr3,
            ..Config::new(trx0())
        },
    );
    ready(&conn2, true);
    an_over(&conn2, &s2, TxStart::Audio);
}

#[test]
fn a_slow_server_is_waited_for_inside_the_windows() {
    let s = sim(
        SimDialect::expert_sdr3(),
        [Fault::Slow {
            delay: Duration::from_millis(300),
        }],
    );
    let conn = connect(&s, Config::new(trx0()));
    ready(&conn, true);
    an_over(&conn, &s, TxStart::Audio);
    let echo = conn.snapshot().timing.echo_ms.expect("measured");
    assert!(echo >= 300, "the echo after {echo} ms");
    let events = drain(&conn, Duration::from_millis(100));
    assert!(
        !events.iter().any(|e| matches!(e, Event::Halted(_))),
        "{events:?}"
    );
}

#[test]
fn another_programs_key_gets_none_of_our_audio() {
    let s = sim(
        SimDialect::expert_sdr3(),
        [Fault::ForeignKey {
            after: Duration::from_millis(200),
            hold: Duration::from_millis(800),
            chrono_to_all: true,
        }],
    );
    let conn = connect(&s, Config::new(trx0()));
    ready(&conn, true);
    expect(&conn, "the foreign key", |e| {
        *e == Event::ForeignKey { on: true }
    });
    assert_eq!(
        conn.key(TxStart::Audio),
        Err(Some(Refusal::Transmitting { tune: false }))
    );
    expect(&conn, "its end", |e| *e == Event::ForeignKey { on: false });
    assert!(
        conn.snapshot().chronos_not_ours > 0,
        "the positive control: its chronos reached us"
    );
    let log = s.log();
    assert!(
        !log.iter()
            .any(|l| matches!(l.event, SimEvent::FrameIn { conn: 0, .. })),
        "none of our audio during another program's over"
    );
    an_over(&conn, &s, TxStart::Audio);
}

#[test]
fn frames_the_codec_refuses_are_counted_and_receive_audio_counts_as_absent() {
    let s = sim(SimDialect::expert_sdr3(), [Fault::FormatCode4]);
    let conn = connect(&s, Config::new(trx0()));
    ready(&conn, false);
    assert!(
        conn.wait_until(WAIT, |s| s.frames.refused >= 10),
        "the positive control: frames arrived"
    );
    let snap = conn.snapshot();
    assert_eq!(snap.frames.accepted, 0);
    assert!(!snap.rx_audio_fresh);
    assert_eq!(conn.key(TxStart::Audio), Err(Some(Refusal::NoReceiveAudio)));
}

#[test]
fn length_as_frames_is_read_and_recorded_as_evidence() {
    let s = sim(SimDialect::expert_sdr3(), [Fault::LengthAsFrames]);
    let conn = connect(&s, Config::new(trx0()));
    ready(&conn, true);
    let snap = conn.snapshot();
    assert_eq!(snap.frames.refused, 0);
    assert_eq!(
        snap.evidence.stereo_reading,
        Some(crate::tci::frame::LengthMeaning::Frames)
    );
    assert_eq!(
        snap.dialect.chrono_length,
        crate::tci::frame::LengthMeaning::Values,
        "evidence, never acted on"
    );
}

#[test]
fn a_close_on_purpose_mid_over_stops_before_the_close_frame() {
    let s = sim(SimDialect::expert_sdr3(), []);
    let conn = connect(&s, Config::new(trx0()));
    ready(&conn, true);
    conn.key(TxStart::Audio).expect("admitted");
    expect(&conn, "the echo", |e| {
        *e == Event::KeyConfirmed(TxStart::Audio)
    });
    conn.close();
    wait_log(&s, "the close", |log| {
        log.iter()
            .any(|l| matches!(l.event, SimEvent::Closed { .. }))
    });
    let log = s.log();
    let stop = log
        .iter()
        .position(|l| matches!(&l.event, SimEvent::Command { text, .. } if text == "trx:0,false"))
        .expect("our stop");
    let closed = log
        .iter()
        .position(|l| {
            matches!(
                l.event,
                SimEvent::Closed {
                    by: Closer::Client,
                    ..
                }
            )
        })
        .expect("a clean close");
    assert!(stop < closed);
    assert!(!s.radio().trx[0].transmitting());
}

/// How many closes each close test below makes: what they guard against is a race, so one close
/// proves little either way.
const CLOSES: usize = 6;

/// How connection 0 ended, as the simulator logged it.
fn closer(log: &[Logged]) -> Option<Closer> {
    log.iter().find_map(|l| match l.event {
        SimEvent::Closed { conn: 0, by } => Some(by),
        _ => None,
    })
}

/// Where in `log` the first command from connection 0 that is exactly `text` (no `;`) is.
fn command_index(log: &[Logged], text: &str) -> Option<usize> {
    log.iter()
        .position(|l| matches!(&l.event, SimEvent::Command { conn: 0, text: t, .. } if t == text))
}

/// A session with the program's receive audio and IQ both arriving, as a close finds it while the
/// panadapter draws.
fn streaming(s: &Simulator, config: Config) -> Connection {
    let conn = connect(s, config);
    ready(&conn, true);
    conn.send(Command::Stream(StreamSetting::IqStart(trx0())))
        .expect("sent");
    wait_log(s, "the program's IQ", |log| {
        log.iter()
            .filter(|l| {
                matches!(
                    l.event,
                    SimEvent::FrameOut {
                        conn: 0,
                        header: Some(h),
                        ..
                    } if h.kind == tempo_tcisim::frame::kind::IQ
                )
            })
            .count()
            >= 3
    });
    conn
}

/// ⭐ A CLOSE REACHES A PROGRAM THAT IS STREAMING, TO ITS LAST LINE: with the program's receive
/// audio and IQ arriving, the close's stop, its `iq_stop` and its close frame are all read, and the
/// connection ends on the close frame. A socket let go at once would hold the program's frames
/// unread and end in a reset, and the simulator, which writes before it reads, would then drop the
/// connection without reading the last lines sent.
#[test]
fn a_close_while_the_program_streams_is_read_to_its_last_line() {
    for close in 0..CLOSES {
        let s = sim(SimDialect::expert_sdr3(), []);
        streaming(&s, Config::new(trx0())).close();
        wait_log(&s, "the end of the connection", |log| closer(log).is_some());
        let log = s.log();
        assert!(
            command_index(&log, "trx:0,false").is_some(),
            "close {close}: the stop never reached the program"
        );
        assert!(
            command_index(&log, "iq_stop:0").is_some(),
            "close {close}: iq_stop never reached the program"
        );
        assert_eq!(
            closer(&log),
            Some(Closer::Client),
            "close {close}: the close frame was never read"
        );
    }
}

/// A close with our key up, the program streaming and slow to report: the stop, then the close
/// frame only once the program has reported the unkey, and every line of it read.
#[test]
fn a_close_with_our_key_up_waits_for_the_unkey_and_reaches_a_streaming_program_whole() {
    // Every text from the program comes 300 ms late, its report of the unkey too; its frames keep
    // their cadence.
    let slow = Fault::Slow {
        delay: Duration::from_millis(300),
    };
    for close in 0..CLOSES {
        let s = sim(SimDialect::expert_sdr3(), [slow.clone()]);
        let conn = streaming(&s, Config::new(trx0()));
        conn.key(TxStart::Audio).expect("admitted");
        expect(&conn, "the echo", |e| {
            *e == Event::KeyConfirmed(TxStart::Audio)
        });
        conn.close();
        wait_log(&s, "the end of the connection", |log| closer(log).is_some());
        let log = s.log();
        let found = |what: &str, at: Option<usize>| {
            at.unwrap_or_else(|| panic!("close {close}: {what}: {:?}", closer(&log)))
        };
        let stop = found(
            "the stop never reached the program",
            command_index(&log, "trx:0,false"),
        );
        found(
            "iq_stop never reached the program",
            command_index(&log, "iq_stop:0"),
        );
        let report = found(
            "the program never reported the unkey",
            log.iter().skip(stop).position(|l| {
                matches!(&l.event, SimEvent::TextOut { conn: 0, text } if text.contains("trx:0,false"))
            }),
        );
        let closed = found(
            "the close frame was never read",
            log.iter().position(|l| {
                matches!(
                    l.event,
                    SimEvent::Closed {
                        conn: 0,
                        by: Closer::Client
                    }
                )
            }),
        );
        assert!(
            stop + report < closed,
            "close {close}: the close frame came before the program reported the unkey"
        );
        assert!(!s.radio().trx[0].transmitting());
    }
}

/// The escalation's stop, sent again before its close, reaches a program that is streaming: a
/// transmitter stuck on the air, our stop never confirmed, then the stops again, `iq_stop` and the
/// close frame, every line read.
#[test]
fn the_stop_the_escalation_sends_again_reaches_a_streaming_program() {
    let fast = Config {
        unkey_deadline_ms: 500,
        ..Config::new(trx0())
    };
    for close in 0..CLOSES {
        let s = sim(SimDialect::expert_sdr3(), [Fault::StuckTrx]);
        let conn = streaming(&s, fast);
        conn.key(TxStart::Audio).expect("admitted");
        expect(&conn, "the echo", |e| {
            *e == Event::KeyConfirmed(TxStart::Audio)
        });
        conn.end_ours();
        let events = expect(&conn, "the close", |e| matches!(e, Event::Closed { .. }));
        assert!(
            events.contains(&Event::UnkeyUnconfirmed),
            "close {close}: {events:?}"
        );
        wait_log(&s, "the end of the connection", |log| closer(log).is_some());
        let log = s.log();
        let stops = log
            .iter()
            .filter(|l| {
                matches!(&l.event, SimEvent::Command { conn: 0, text, .. } if text == "trx:0,false")
            })
            .count();
        assert!(
            stops >= 2,
            "close {close}: the stop sent again never reached the program ({stops} read)"
        );
        assert!(
            command_index(&log, "iq_stop:0").is_some(),
            "close {close}: iq_stop never reached the program"
        );
        assert_eq!(
            closer(&log),
            Some(Closer::Client),
            "close {close}: the close frame was never read"
        );
    }
}

/// One block as the panadapter's tap was handed it: its rate, channels and frames, and its centre.
type Handed = (u32, crate::tci::frame::Channels, usize, Option<i64>);

/// What the panadapter's tap was handed.
#[derive(Default)]
struct IqBlocks {
    got: std::sync::Mutex<Vec<Handed>>,
}

impl IqTap for IqBlocks {
    fn iq(&self, block: &Block<'_>, centre: Option<Hz>) {
        self.got.lock().unwrap().push((
            block.header.sample_rate,
            block.header.channels,
            block.frames(),
            centre.map(Hz::get),
        ));
    }
}

/// ⭐ THE SDR PROGRAM'S IQ REACHES THE PANADAPTER'S TAP, CENTRED WHERE THE PROGRAM SAYS: after
/// `iq_start` the simulated program's IQ blocks for our transceiver arrive with two channels at its
/// IQ rate and the centre its burst reported.
///
/// The close's `iq_stop` is pinned as written on the manual clock
/// (`a_close_stops_the_iq_stream_this_session_started_after_the_stops`), and as the program reads
/// it in `a_close_while_the_program_streams_is_read_to_its_last_line`.
#[test]
fn the_programs_iq_reaches_the_tap_centred_where_the_program_says() {
    let s = sim(SimDialect::expert_sdr3(), []);
    let conn = connect(&s, Config::new(trx0()));
    ready(&conn, true);
    let tap = std::sync::Arc::new(IqBlocks::default());
    assert!(conn.set_iq_tap(Some(tap.clone())));
    conn.send(Command::Stream(StreamSetting::IqStart(trx0())))
        .expect("sent");
    let deadline = Instant::now() + WAIT;
    while tap.got.lock().unwrap().len() < 5 {
        assert!(Instant::now() < deadline, "no IQ reached the tap");
        std::thread::sleep(Duration::from_millis(10));
    }
    let dds = i64::try_from(s.radio().trx[0].dds).expect("a frequency");
    for (rate, channels, frames, centre) in tap.got.lock().unwrap().iter() {
        assert_eq!(
            (*rate, *channels, *centre),
            (48_000, crate::tci::frame::Channels::Two, Some(dds))
        );
        assert!(*frames > 0);
    }
    assert!(conn.set_iq_tap(None));
    let handed = tap.got.lock().unwrap().len();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        tap.got.lock().unwrap().len(),
        handed,
        "no tap: nothing handed"
    );
}
