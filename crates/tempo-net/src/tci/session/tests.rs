//! The session's rules on a manual clock: every window, deadline and readback decided by the
//! times given, with the server's messages typed in and everything the session writes recorded.
//! The simulator suites (`sim_tests`) run the same rules over a real WebSocket.

use super::*;
use crate::tci::frame::{decode, encode_block, BlockHeader, Channels, LengthMeaning, SampleFormat};
use crate::tci::model::Reading;
use crate::tci::{EXPERT_SDR2, THETIS_NATIVE};

fn trx(n: u32) -> Trx {
    Trx::new(n).expect("a receiver number")
}

/// What the session wrote.
#[derive(Debug, Clone, PartialEq)]
enum Out {
    Text(String),
    Binary(Vec<u8>),
    Ping,
    Close,
}

/// A recording transport; `broken` fails every write.
#[derive(Default)]
struct Wire {
    out: Vec<Out>,
    broken: bool,
}

impl Outbox for Wire {
    fn text(&mut self, text: &ClientText) -> bool {
        if self.broken {
            return false;
        }
        self.out.push(Out::Text(text.as_str().to_string()));
        true
    }

    fn binary(&mut self, frame: Vec<u8>) -> bool {
        if self.broken {
            return false;
        }
        self.out.push(Out::Binary(frame));
        true
    }

    fn ping(&mut self) -> bool {
        if self.broken {
            return false;
        }
        self.out.push(Out::Ping);
        true
    }

    fn close(&mut self) {
        self.out.push(Out::Close);
    }
}

impl Wire {
    /// Everything written since the last look.
    fn take(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    /// The text messages written since the last look (the rest is forgotten too).
    fn texts(&mut self) -> Vec<String> {
        self.take()
            .into_iter()
            .filter_map(|o| match o {
                Out::Text(t) => Some(t),
                _ => None,
            })
            .collect()
    }
}

/// Transmit audio at a constant level; counts what it is given and asked.
#[derive(Default)]
struct Level {
    transmits: usize,
    received: usize,
    iq: usize,
    /// The most samples it was ever asked for at once.
    longest: usize,
}

const LEVEL: f32 = 0.25;

impl Streams for Level {
    fn receive(&mut self, _block: &Block<'_>) {
        self.received += 1;
    }

    fn iq(&mut self, _block: &Block<'_>) {
        self.iq += 1;
    }

    fn transmit(&mut self, rate: u32, out: &mut [f32]) -> usize {
        assert_eq!(rate, 48_000);
        self.transmits += 1;
        self.longest = self.longest.max(out.len());
        out.fill(LEVEL);
        out.len()
    }
}

/// An ExpertSDR3 burst for a radio ready to transmit on receiver 0, up to `ready;`.
const BURST: &str = "protocol:ExpertSDR3,2.0;device:SunSDR2DX;receive_only:false;trx_count:2;\
                     tx_enable:0,true;trx:0,false;tune:0,false;drive:0,50;tune_drive:0,10;start;";

fn block(trx: u32, kind: BlockKind) -> Vec<u8> {
    let header = BlockHeader {
        trx: Trx::new(trx).expect("trx"),
        kind,
        sample_rate: 48_000,
        format: SampleFormat::Float32,
        channels: Channels::Two,
    };
    encode_block(&header, LengthMeaning::Values, &[0.0; 512]).expect("a block")
}

fn chrono(trx: u32) -> Vec<u8> {
    Chrono {
        trx: Trx::new(trx).expect("trx"),
        sample_rate: 48_000,
        format: SampleFormat::Float32,
        channels: Channels::Two,
        length: 2048,
    }
    .encode()
    .expect("a chrono")
}

struct Bench {
    s: Session,
    wire: Wire,
    streams: Level,
    now: u64,
}

impl Bench {
    fn new(config: Config) -> Bench {
        Bench {
            s: Session::new(config, 0),
            wire: Wire::default(),
            streams: Level::default(),
            now: 0,
        }
    }

    /// Ready on receiver 0, `audio_start` acknowledged and one receive block in, at 3 ms; the
    /// setup it wrote and its events forgotten.
    fn ready(config: Config) -> Bench {
        let mut b = Bench::new(config);
        b.at(1).text(BURST);
        b.text("ready;");
        b.at(2).text("audio_start:0;");
        b.at(3).rx();
        b.wire.take();
        b.events();
        b
    }

    fn at(&mut self, ms: u64) -> &mut Bench {
        self.now = ms;
        self
    }

    fn text(&mut self, text: &str) {
        self.s.on_text(&mut self.wire, text, self.now);
    }

    fn binary(&mut self, bytes: &[u8]) {
        self.s
            .on_binary(&mut self.wire, &mut self.streams, bytes, self.now);
    }

    fn rx(&mut self) {
        self.binary(&block(0, BlockKind::RxAudio));
    }

    fn poll(&mut self) {
        self.s.poll(&mut self.wire, self.now);
    }

    fn key(&mut self, start: TxStart) -> Result<(), Refusal> {
        self.s.key(&mut self.wire, start, self.now)
    }

    fn end_ours(&mut self) -> StopOutcome {
        self.s.end_ours(&mut self.wire, self.now)
    }

    fn close(&mut self) {
        self.s.close(&mut self.wire, self.now);
    }

    fn events(&mut self) -> Vec<Event> {
        self.s.take_events()
    }

    /// A TX_CHRONO for `trx`, and the answer's samples (channels interleaved) if one went out.
    fn chrono(&mut self, trx: u32) -> Option<Vec<f32>> {
        self.binary(&chrono(trx));
        let answers: Vec<Vec<u8>> = self
            .wire
            .take()
            .into_iter()
            .filter_map(|o| match o {
                Out::Binary(b) => Some(b),
                _ => None,
            })
            .collect();
        match answers.as_slice() {
            [] => None,
            [bytes] => match decode(bytes).expect("our answer decodes") {
                Frame::Block(b) => {
                    assert_eq!(b.header.kind, BlockKind::TxAudio);
                    assert_eq!(b.header.trx, Trx::new(trx).expect("trx"));
                    assert_eq!(b.header.channels, Channels::Two);
                    assert_eq!(b.reading, Some(LengthMeaning::Values));
                    Some(b.samples().collect())
                }
                Frame::Chrono(_) => panic!("a chrono answered with a chrono"),
            },
            more => panic!("{} answers to one chrono", more.len()),
        }
    }
}

fn stop_trx0() -> StopOutcome {
    StopOutcome::Sent(vec![Stop::Trx(trx(0))])
}

fn closed(end: End, carried: Option<TxStart>) -> Event {
    Event::Closed {
        end,
        carried: carried.map(|kind| OurKey { trx: trx(0), kind }),
    }
}

#[test]
fn the_burst_reaches_ready_and_the_setup_goes_out_in_order() {
    let mut b = Bench::new(Config::new(trx(0)));
    b.at(1).text(BURST);
    assert!(b.wire.texts().is_empty(), "nothing is written before ready");
    assert_eq!(b.s.phase(), Phase::AwaitReady);
    b.text("ready;");
    assert_eq!(b.s.phase(), Phase::Ready);
    assert_eq!(
        b.wire.texts(),
        [
            "audio_start:0;",
            "rx_sensors_enable:true,200;",
            "tx_sensors_enable:true,200;"
        ]
    );
    assert_eq!(
        b.events(),
        [Event::Ready {
            dialect: EXPERT_SDR3,
            program: Some("ExpertSDR3".into()),
            version: Some("2.0".into()),
            device: Some("SunSDR2DX".into()),
        }]
    );
    // A second ready changes nothing.
    b.text("ready;");
    assert!(b.wire.texts().is_empty());
    assert!(b.events().is_empty());

    // Thetis announcing itself, Nexus's audio off, receiver 1: no stream, the S-meter polled.
    let mut b = Bench::new(Config {
        audio: false,
        ..Config::new(trx(1))
    });
    b.at(1).text("protocol:Thetis,2.10.3.14;ready;");
    assert!(b.wire.texts().is_empty());
    assert!(matches!(
        b.events().as_slice(),
        [Event::Ready { dialect, .. }] if *dialect == THETIS_NATIVE
    ));
    b.at(2).poll();
    assert_eq!(b.wire.texts(), ["rx_smeter:1,0;"]);
    b.at(501).poll();
    assert!(b.wire.texts().is_empty(), "not before METER_POLL_MS");
    b.at(502).poll();
    assert_eq!(b.wire.texts(), ["rx_smeter:1,0;"]);
}

#[test]
fn no_ready_in_time_ends_the_session_with_the_stops_first() {
    let mut b = Bench::new(Config::new(trx(0)));
    b.at(1).text(BURST);
    b.at(4_999).poll();
    assert_eq!(b.s.phase(), Phase::AwaitReady);
    b.wire.take();
    b.at(5_000).poll();
    assert_eq!(
        b.wire.take(),
        [Out::Text("trx:0,false;".into()), Out::Close]
    );
    assert_eq!(b.events(), [closed(End::NoReady, None)]);
    // Closed: nothing more is written, stops included.
    assert_eq!(b.end_ours(), StopOutcome::NotConnected);
    b.at(9_000).poll();
    assert!(b.wire.take().is_empty());
}

#[test]
fn a_digital_key_is_answered_chrono_by_chrono_and_only_while_ours() {
    let mut b = Bench::ready(Config::new(trx(0)));
    // No key of ours: a chrono gets nothing.
    assert_eq!(b.at(5).chrono(0), None);
    b.at(10).key(TxStart::Audio).expect("admitted");
    assert_eq!(b.wire.texts(), ["trx:0,true,tci;"]);
    assert!(b.s.ours_on_air() && b.s.audio_over_open());
    // Exactly the frames asked for (2048 values: 1024 stereo frames), Nexus's audio on both.
    let answer = b.at(12).chrono(0).expect("answered");
    assert_eq!(answer.len(), 2048);
    assert!(answer.iter().all(|s| (*s - LEVEL).abs() < 1e-6));
    // Another transceiver's chrono is not ours.
    assert_eq!(b.at(13).chrono(1), None);
    b.at(14).text("trx:0,true;");
    assert_eq!(b.events(), [Event::KeyConfirmed(TxStart::Audio)]);
    let snap = b.s.snapshot();
    assert!(snap.key_confirmed && snap.over_open);
    assert_eq!(
        snap.timing,
        Timing {
            echo_ms: Some(4),
            first_chrono_ms: Some(2)
        }
    );
    // Our stop: silence from now on, until the server reports the unkey.
    assert_eq!(b.at(50).end_ours(), stop_trx0());
    assert_eq!(b.wire.texts(), ["trx:0,false;"]);
    assert!(!b.s.audio_over_open());
    let after_stop = b.at(52).chrono(0).expect("answered while keyed");
    assert!(
        after_stop.iter().all(|s| *s == 0.0),
        "silence after our stop"
    );
    b.at(60).text("trx:0,false;");
    assert_eq!(b.events(), [Event::Unkeyed]);
    assert!(!b.s.ours_on_air());
    // Unkeyed: nothing, not even silence.
    assert_eq!(b.at(62).chrono(0), None);
    assert_eq!(
        b.streams.transmits, 1,
        "Nexus's audio only while the over was open"
    );
    assert_eq!(b.s.snapshot().chronos_not_ours, 3);
}

#[test]
fn a_report_that_can_predate_the_key_is_not_proof_of_its_end() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Microphone).expect("admitted");
    // Crossed in flight: the server's `false` from before it read our key.
    b.at(11).text("trx:0,false;");
    assert!(b.s.ours_on_air(), "a report before the echo proves nothing");
    assert!(b.events().is_empty());
    b.at(12).text("trx:0,true;");
    assert_eq!(b.events(), [Event::KeyConfirmed(TxStart::Microphone)]);
    // After the echo, before our stop: the server ended the over itself.
    b.at(20).text("trx:0,false;");
    assert_eq!(
        b.events(),
        [Event::Halted(Halt::ServerEnded), Event::Unkeyed]
    );
    assert!(!b.s.ours_on_air());
    assert!(
        b.wire.texts().iter().all(|t| t != "trx:0,false;"),
        "no stop needed"
    );
}

#[test]
fn the_chrono_window_ends_a_digital_key_the_server_does_not_pull() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(100).key(TxStart::Audio).expect("admitted");
    b.at(101).text("trx:0,true;");
    b.wire.take();
    b.events();
    b.at(399).poll();
    assert!(b.wire.texts().is_empty(), "inside the window");
    b.at(400).poll();
    assert_eq!(b.wire.texts(), ["trx:0,false;"]);
    assert_eq!(b.events(), [Event::Halted(Halt::NoChrono)]);
    assert!(b.s.ours_on_air(), "kept until the server reports the unkey");

    // A chrono inside the window keeps the over, for as long as it is pulled.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(100).key(TxStart::Audio).expect("admitted");
    b.at(101).text("trx:0,true;");
    assert!(b.at(399).chrono(0).is_some());
    b.events();
    for t in (400..1_400).step_by(100) {
        b.at(t).chrono(0);
        b.poll();
    }
    assert!(b.events().is_empty());
    assert!(b.s.audio_over_open());
    // A microphone key is not pulled: no chrono window.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(100).key(TxStart::Microphone).expect("admitted");
    b.at(101).text("trx:0,true;");
    b.events();
    b.at(1_000).poll();
    assert!(b.events().is_empty());
}

#[test]
fn the_echo_window_ends_a_key_the_server_does_not_echo() {
    for start in [TxStart::Microphone, TxStart::Tune, TxStart::Audio] {
        let mut b = Bench::ready(Config::new(trx(0)));
        b.at(100).key(start).expect("admitted");
        b.wire.take();
        for t in (200..1_600).step_by(100) {
            if start == TxStart::Audio {
                b.at(t).chrono(0);
            }
            b.at(t).rx();
            b.poll();
        }
        b.at(1_599).poll();
        assert!(b.events().is_empty(), "{start:?} inside the window");
        b.wire.take();
        b.at(1_600).poll();
        let stops = if start == TxStart::Tune {
            vec!["tune:0,false;", "trx:0,false;"]
        } else {
            vec!["trx:0,false;"]
        };
        assert_eq!(b.wire.texts(), stops, "{start:?}");
        assert_eq!(b.events(), [Event::Halted(Halt::NoEcho)], "{start:?}");
    }
}

#[test]
fn an_unkey_not_reported_in_time_is_sent_again_and_the_session_closes() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Audio).expect("admitted");
    b.at(11).chrono(0);
    b.text("trx:0,true;");
    b.at(100).end_ours();
    b.wire.take();
    b.events();
    for t in (200..2_100).step_by(100) {
        b.at(t).rx();
        b.poll();
    }
    assert_eq!(
        b.s.phase(),
        Phase::Ready,
        "the deadline is 2 s after the first stop"
    );
    b.wire.take();
    b.at(2_100).poll();
    assert_eq!(
        b.wire.take(),
        [Out::Text("trx:0,false;".into()), Out::Close],
        "the stops again, then the close"
    );
    assert_eq!(
        b.events(),
        [
            Event::UnkeyUnconfirmed,
            closed(End::UnkeyUnconfirmed, Some(TxStart::Audio))
        ]
    );
    assert!(b.s.ours_on_air(), "the belief goes to the next session");
}

#[test]
fn stops_go_out_in_every_phase_but_closed() {
    // Before ready, nothing of ours keyed: still sent.
    let mut b = Bench::new(Config::new(trx(0)));
    b.at(1).text(BURST);
    assert_eq!(b.end_ours(), stop_trx0());
    assert_eq!(
        b.s.stop(&mut b.wire, Stop::Tune(trx(0)), 2),
        StopOutcome::Sent(vec![Stop::Tune(trx(0))])
    );
    assert_eq!(
        b.s.stop(&mut b.wire, Stop::CwMacros, 3),
        StopOutcome::Sent(vec![Stop::CwMacros])
    );
    assert_eq!(
        b.wire.texts(),
        ["trx:0,false;", "tune:0,false;", "cw_macros_stop;"]
    );
    // Ready and idle, and while another program transmits.
    b.at(4).text("ready;");
    b.text("trx:0,true;");
    b.wire.take();
    assert_eq!(b.end_ours(), stop_trx0());
    assert_eq!(b.wire.texts(), ["trx:0,false;"]);
    // While closing.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Tune).expect("admitted");
    b.at(11).text("tune:0,true;");
    b.at(12).close();
    assert_eq!(b.s.phase(), Phase::Closing);
    b.wire.take();
    assert_eq!(
        b.end_ours(),
        StopOutcome::Sent(vec![Stop::Tune(trx(0)), Stop::Trx(trx(0))])
    );
    assert_eq!(b.wire.texts(), ["tune:0,false;", "trx:0,false;"]);
    // Closed: there is nothing to write to.
    b.s.transport_closed(20);
    assert_eq!(b.end_ours(), StopOutcome::NotConnected);
}

#[test]
fn a_lost_link_keeps_the_keyed_belief() {
    // On the manual clock.
    for start in [TxStart::Audio, TxStart::Microphone, TxStart::Tune] {
        let mut b = Bench::ready(Config::new(trx(0)));
        b.at(10).key(start).expect("admitted");
        b.at(11).text(if start == TxStart::Tune {
            "tune:0,true;"
        } else {
            "trx:0,true;"
        });
        b.events();
        b.s.transport_closed(12);
        assert_eq!(b.events(), [closed(End::Lost, Some(start))], "{start:?}");
        assert!(b.s.ours_on_air(), "{start:?}");
        assert_eq!(
            b.s.snapshot().keyed,
            Some(OurKey {
                trx: trx(0),
                kind: start
            })
        );
    }
}

#[test]
fn a_carried_key_is_stopped_first_and_ends_on_the_servers_word() {
    let carried = OurKey {
        trx: trx(0),
        kind: TxStart::Audio,
    };
    // The burst says still keyed: stops first after ready, the belief held until the report.
    let mut b = Bench::new(Config {
        carried: Some(carried),
        ..Config::new(trx(0))
    });
    assert!(b.s.ours_on_air());
    b.at(1).text(&BURST.replace("trx:0,false;", "trx:0,true;"));
    assert!(b.wire.texts().is_empty());
    b.text("ready;");
    assert_eq!(
        b.wire.texts()[0],
        "trx:0,false;",
        "the first command after ready"
    );
    assert!(b.s.ours_on_air());
    assert_eq!(
        b.key(TxStart::Microphone),
        Err(Refusal::AlreadyKeyed),
        "nothing new while it may be up"
    );
    b.at(5).text("trx:0,false;");
    assert!(b.events().contains(&Event::Unkeyed));
    assert!(!b.s.ours_on_air());

    // The burst says unkeyed: the server's word on a new link ends the belief, and the stops
    // still go first.
    let mut b = Bench::new(Config {
        carried: Some(carried),
        ..Config::new(trx(0))
    });
    b.at(1).text(BURST);
    assert!(!b.s.ours_on_air());
    b.text("ready;");
    assert_eq!(b.wire.texts()[0], "trx:0,false;");

    // A carried tune on another receiver is stopped where it is.
    let mut b = Bench::new(Config {
        carried: Some(OurKey {
            trx: trx(1),
            kind: TxStart::Tune,
        }),
        ..Config::new(trx(0))
    });
    b.at(1).text(&format!("{BURST}tune:1,true;ready;"));
    assert_eq!(b.wire.texts()[..2], ["tune:1,false;", "trx:1,false;"]);
    assert_eq!(
        b.end_ours(),
        StopOutcome::Sent(vec![
            Stop::Tune(trx(1)),
            Stop::Trx(trx(1)),
            Stop::Trx(trx(0)),
        ])
    );
    b.at(3).text("tune:1,false;");
    assert!(!b.s.ours_on_air());
    // Unconfirmed, it escalates like any stop.
    let mut b = Bench::new(Config {
        carried: Some(carried),
        ..Config::new(trx(0))
    });
    b.at(1).text(&BURST.replace("trx:0,false;", "trx:0,true;"));
    b.text("ready;");
    b.events();
    for t in (100..2_000).step_by(100) {
        b.at(t).rx();
        b.text("trx:0,true;");
        b.poll();
    }
    b.at(2_001).poll();
    assert_eq!(
        b.events(),
        [
            Event::UnkeyUnconfirmed,
            closed(End::UnkeyUnconfirmed, Some(TxStart::Audio))
        ]
    );
}

#[test]
fn the_sdr_stopping_mid_over_halts_it_and_refuses_keys() {
    // On the manual clock.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Audio).expect("admitted");
    b.at(11).chrono(0);
    b.text("trx:0,true;");
    b.events();
    b.at(20).text("stop;");
    assert_eq!(
        b.events(),
        [Event::Running(false), Event::Halted(Halt::SdrStopped)]
    );
    assert_eq!(b.wire.texts(), ["trx:0,false;"]);
    b.at(21).text("trx:0,false;");
    assert_eq!(b.events(), [Event::Unkeyed]);
    assert_eq!(
        b.key(TxStart::Microphone),
        Err(Refusal::SdrStopped { unreadable: false })
    );
    b.at(30).text("start;");
    assert_eq!(b.events(), [Event::Running(true)]);
    assert_eq!(b.key(TxStart::Microphone), Ok(()));
}

#[test]
fn silence_while_keyed_stops_the_over_and_a_stale_link_closes() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Audio).expect("admitted");
    b.at(11).chrono(0);
    b.text("trx:0,true;");
    b.events();
    b.wire.take();
    b.at(1_510).poll();
    assert!(b.events().is_empty(), "1499 ms of silence");
    b.at(1_511).poll();
    assert_eq!(b.events(), [Event::Halted(Halt::Silent)]);
    assert_eq!(b.wire.texts(), ["trx:0,false;"]);
    b.at(5_010).poll();
    assert_eq!(b.s.phase(), Phase::Ready);
    b.wire.take();
    b.at(5_011).poll();
    assert_eq!(
        b.wire.take(),
        [Out::Text("trx:0,false;".into()), Out::Close]
    );
    assert_eq!(
        b.events(),
        [closed(End::Stale, Some(TxStart::Audio))],
        "a half-open link never clears the belief"
    );
}

#[test]
fn pings_go_out_and_anything_heard_keeps_the_link() {
    let mut b = Bench::ready(Config::new(trx(0)));
    let pings = |b: &mut Bench| b.wire.take().iter().filter(|o| **o == Out::Ping).count();
    b.at(999).poll();
    assert_eq!(pings(&mut b), 0);
    b.at(1_000).poll();
    assert_eq!(pings(&mut b), 1);
    // A pong (or anything) every second keeps it for good.
    for t in (1_500..20_000).step_by(1_000) {
        b.s.heard(t);
        b.at(t).poll();
    }
    assert_eq!(b.s.phase(), Phase::Ready);
    // A ping that cannot be written ends the session.
    b.wire.broken = true;
    b.at(21_000).poll();
    assert!(b.s.is_closed());
    assert_eq!(b.events(), [closed(End::Lost, None)]);
}

#[test]
fn an_echo_naming_another_source_ends_a_digital_over() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Audio).expect("admitted");
    b.wire.take();
    b.at(11).text("trx:0,true,mic1;");
    assert_eq!(
        b.events(),
        [
            Event::KeyConfirmed(TxStart::Audio),
            Event::Halted(Halt::WrongSource(Source::Mic1))
        ]
    );
    assert_eq!(b.wire.texts(), ["trx:0,false;"]);
    // Naming Nexus's audio, or nothing, is fine; a microphone key may name a microphone.
    for (start, echo) in [
        (TxStart::Audio, "trx:0,true,tci;"),
        (TxStart::Audio, "trx:0,true;"),
        (TxStart::Microphone, "trx:0,true,mic2;"),
    ] {
        let mut b = Bench::ready(Config::new(trx(0)));
        b.at(10).key(start).expect("admitted");
        b.at(11).text(echo);
        assert_eq!(b.events(), [Event::KeyConfirmed(start)], "{echo}");
    }
}

#[test]
fn a_refused_readback_is_asked_again_at_most_once_a_second() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).text("trx:0,maybe;drive:0,-5;");
    assert_eq!(
        b.key(TxStart::Microphone),
        Err(Refusal::TransmitStateUnknown)
    );
    b.at(11).poll();
    assert_eq!(b.wire.texts(), ["trx:0;", "drive:0;"]);
    b.at(500).poll();
    assert!(b.wire.texts().is_empty());
    b.at(1_011).poll();
    assert_eq!(b.wire.texts(), ["trx:0;", "drive:0;"]);
    b.at(1_100).text("trx:0,false;drive:0,50;");
    b.at(2_200).poll();
    assert!(b.wire.texts().is_empty());
    b.rx();
    assert_eq!(b.key(TxStart::Microphone), Ok(()));
}

#[test]
fn a_command_cut_off_at_a_message_end_is_unknown() {
    let mut b = Bench::ready(Config::new(trx(0)));
    // `trx:0,false;` cut across two messages: neither half is read as anything.
    b.at(10).text("vfo:0,0,14074000;trx:0,");
    b.text("false;");
    assert!(b.s.model().trx(trx(0)).transmitting.is_unreadable());
    assert_eq!(
        b.s.model().trx(trx(0)).vfo[0].known().map(|h| h.get()),
        Some(14_074_000),
        "the command before the cut is read"
    );
    b.at(11).poll();
    assert_eq!(b.wire.texts(), ["trx:0;"]);
    // piHPSDR's unterminated burst lines name nothing the model holds.
    let mut m = Bench::new(Config::new(trx(0)));
    m.at(1).text(BURST);
    m.text("audio_stream_channels: 2");
    m.text("ready;");
    assert_eq!(m.s.phase(), Phase::Ready);
    assert_eq!(cut_name("a;b:1,2"), Some("b"));
    assert_eq!(cut_name(" trx :0,"), Some("trx"));
    assert_eq!(cut_name("ready;"), None);
    assert_eq!(cut_name("x;!!:1"), None);
}

#[test]
fn another_programs_key_is_reported_and_gets_no_audio() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).text("trx:0,true;");
    assert_eq!(b.events(), [Event::ForeignKey { on: true }]);
    assert_eq!(
        b.key(TxStart::Audio),
        Err(Refusal::Transmitting { tune: false })
    );
    assert_eq!(b.at(11).chrono(0), None);
    // Another receiver's key is not ours to report.
    b.at(12).text("trx:1,true;");
    assert!(b.events().is_empty());
    b.at(13).text("trx:0,false;");
    assert_eq!(b.events(), [Event::ForeignKey { on: false }]);
    b.rx();
    assert_eq!(b.key(TxStart::Audio), Ok(()));
    assert_eq!(b.streams.transmits, 0);
}

#[test]
fn a_close_on_purpose_sends_the_stops_and_waits_for_the_unkey() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Audio).expect("admitted");
    b.at(11).text("trx:0,true;");
    b.wire.take();
    b.events();
    b.at(20).close();
    assert_eq!(b.wire.take(), [Out::Text("trx:0,false;".into())]);
    assert_eq!(b.s.phase(), Phase::Closing);
    assert_eq!(
        b.s.send(&mut b.wire, &Command::SpotClear, 21),
        Err(SendError::NotReady)
    );
    b.at(30).text("trx:0,false;");
    assert_eq!(b.wire.take(), [Out::Close]);
    assert_eq!(b.events(), [Event::Unkeyed, closed(End::Closed, None)]);

    // No report within the wait: closed anyway, the belief handed on.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Audio).expect("admitted");
    b.at(11).text("trx:0,true;");
    b.events();
    b.at(20).close();
    b.at(1_019).poll();
    assert_eq!(b.s.phase(), Phase::Closing);
    b.at(1_020).poll();
    assert_eq!(b.events(), [closed(End::Closed, Some(TxStart::Audio))]);

    // Idle: the stop, then the close at once.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).close();
    assert_eq!(
        b.wire.take(),
        [Out::Text("trx:0,false;".into()), Out::Close]
    );
    assert_eq!(b.events(), [closed(End::Closed, None)]);
}

#[test]
fn a_failed_write_ends_the_session_and_keeps_the_belief() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.wire.broken = true;
    // The key's write fails: the belief was set before it.
    assert_eq!(b.at(10).key(TxStart::Audio), Ok(()));
    assert!(b.s.is_closed());
    assert_eq!(b.events(), [closed(End::Lost, Some(TxStart::Audio))]);
}

#[test]
fn the_drive_nexus_writes_is_the_ceiling_its_readback_is_held_to() {
    // On the manual clock.
    let mut b = Bench::ready(Config::new(trx(0)));
    let drive = |b: &mut Bench, level: u32, t: u64| {
        let write = Command::Set(Setting::drive(trx(0), level, &EXPERT_SDR3));
        b.s.send(&mut b.wire, &write, t).expect("sent");
    };
    drive(&mut b, 30, 10);
    assert_eq!(b.wire.texts(), ["drive:0,30;"]);
    assert_eq!(
        b.key(TxStart::Microphone),
        Err(Refusal::DriveAboveCeiling {
            drive: 50,
            ceiling: 30,
            tune: false
        }),
        "until the server echoes it"
    );
    b.at(11).text("drive:0,30;");
    assert_eq!(b.key(TxStart::Microphone), Ok(()));
    b.at(12).text("trx:0,true;");
    b.end_ours();
    b.at(13).text("trx:0,false;");
    // A band change restores the band's drive.
    b.at(20)
        .text("vfo:0,0,7074000;drive:0,100;tune_drive:0,100;");
    b.rx();
    assert_eq!(
        b.key(TxStart::Audio),
        Err(Refusal::DriveAboveCeiling {
            drive: 100,
            ceiling: 30,
            tune: false
        })
    );
    drive(&mut b, 30, 21);
    b.at(22).text("drive:0,30;");
    assert_eq!(b.key(TxStart::Audio), Ok(()));
    // A 1.x drive write is the ceiling too.
    let mut b = Bench::ready(Config::new(trx(0)));
    let write = Command::Set(Setting::drive(trx(0), 20, &EXPERT_SDR2));
    b.s.send(&mut b.wire, &write, 10).expect("sent");
    assert_eq!(b.s.snapshot().drive_ceiling, Some(20));
    // Another receiver's drive is not ours.
    let other = Command::Set(Setting::Drive {
        trx: Some(trx(1)),
        level: 5,
    });
    b.s.send(&mut b.wire, &other, 11).expect("sent");
    assert_eq!(b.s.snapshot().drive_ceiling, Some(20));
}

#[test]
fn cat_waits_for_ready_and_nothing_is_written_after_the_close() {
    let mut b = Bench::new(Config::new(trx(0)));
    let read = Command::Read(Query::Trx(trx(0)));
    assert_eq!(b.s.send(&mut b.wire, &read, 1), Err(SendError::NotReady));
    assert_eq!(b.key(TxStart::Microphone), Err(Refusal::NotReady));
    b.at(1).text(&format!("{BURST}ready;"));
    b.wire.take();
    assert_eq!(b.s.send(&mut b.wire, &read, 2), Ok(()));
    assert_eq!(b.wire.texts(), ["trx:0;"]);
    b.s.transport_closed(3);
    assert_eq!(b.s.send(&mut b.wire, &read, 4), Err(SendError::Closed));
    b.at(5).text("trx:0,true;");
    assert!(b.wire.take().is_empty());
}

/// The spot clicks among `events`, as "CALL hz channel" ("-" for none named).
fn clicks(events: Vec<Event>) -> Vec<String> {
    events
        .into_iter()
        .map(|e| match e {
            Event::SpotClicked { call, hz, channel } => format!(
                "{} {} {}",
                call.as_str(),
                hz.get(),
                channel.map_or("-".to_string(), |c| c.to_string())
            ),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn spot_clicks_are_reported_and_receive_blocks_reach_the_streams() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).text(
        "clicked_on_spot:K1ABC,14025000;rx_clicked_on_spot:0,1,W1AW,7030000;\
         rx_clicked_on_spot:1,0,N0CALL,3530000;",
    );
    assert_eq!(
        clicks(b.events()),
        ["W1AW 7030000 1"],
        "ours, with its channel, at once; receiver 1's is not ours"
    );
    assert!(b.wire.take().is_empty(), "a click writes nothing");
    b.at(10 + CLICK_PAIR_MS).poll();
    assert_eq!(
        clicks(b.events()),
        ["K1ABC 14025000 -"],
        "the legacy click, with no twin, once its wait is over"
    );
    let before = b.streams.received;
    b.at(11 + CLICK_PAIR_MS)
        .binary(&block(0, BlockKind::RxAudio));
    b.binary(&block(1, BlockKind::RxAudio));
    b.binary(&block(0, BlockKind::Iq));
    assert_eq!(b.streams.received, before + 1);
    assert_eq!(b.streams.iq, 1);
    // A frame that does not decode is counted and dropped.
    b.binary(&[0u8; 10]);
    assert_eq!(b.s.snapshot().frames.refused, 1);
}

/// ONE CLICK IS ONE CLICK: a server that sends a click in both spellings (ExpertSDR3, Thetis,
/// AetherSDR) is heard once, with the channel the receiver's spelling names, in either order and
/// whether the two arrive in one message or two. A double click is two. CONTROL: the legacy
/// spelling alone is still a click, reported once its wait is over.
#[test]
fn a_click_in_both_spellings_is_reported_once_whichever_comes_first() {
    let mut b = Bench::ready(Config::new(trx(0)));
    let mut seen = Vec::new();
    // Legacy first, one message.
    b.at(10)
        .text("clicked_on_spot:K1ABC,14025000;rx_clicked_on_spot:0,0,K1ABC,14025000;");
    seen.push(("legacy, then receiver", clicks(b.events())));
    b.at(10 + CLICK_PAIR_MS).poll();
    seen.push(("legacy, then receiver: after the wait", clicks(b.events())));
    // Receiver first, two messages.
    b.at(300).text("rx_clicked_on_spot:0,0,W1AW,7030000;");
    b.at(305).text("clicked_on_spot:W1AW,7030000;");
    seen.push(("receiver, then legacy", clicks(b.events())));
    b.at(305 + CLICK_PAIR_MS).poll();
    seen.push(("receiver, then legacy: after the wait", clicks(b.events())));
    // A double click: both spellings, twice.
    b.at(600)
        .text("clicked_on_spot:N0CALL,3530000;rx_clicked_on_spot:0,0,N0CALL,3530000;");
    b.at(640)
        .text("clicked_on_spot:N0CALL,3530000;rx_clicked_on_spot:0,0,N0CALL,3530000;");
    b.at(640 + CLICK_PAIR_MS).poll();
    seen.push(("a double click", clicks(b.events())));
    // CONTROL: the legacy spelling alone (ExpertSDR2).
    b.at(1_000).text("clicked_on_spot:VP2V/K1ABC,7025000;");
    seen.push(("legacy alone, at once", clicks(b.events())));
    b.at(1_000 + CLICK_PAIR_MS).poll();
    seen.push(("legacy alone, after the wait", clicks(b.events())));
    let want: Vec<(&str, Vec<String>)> = vec![
        ("legacy, then receiver", vec!["K1ABC 14025000 0".into()]),
        ("legacy, then receiver: after the wait", vec![]),
        ("receiver, then legacy", vec!["W1AW 7030000 0".into()]),
        ("receiver, then legacy: after the wait", vec![]),
        (
            "a double click",
            vec!["N0CALL 3530000 0".into(), "N0CALL 3530000 0".into()],
        ),
        ("legacy alone, at once", vec![]),
        (
            "legacy alone, after the wait",
            vec!["VP2V/K1ABC 7025000 -".into()],
        ),
    ];
    assert_eq!(seen, want);
}

/// ⛔ A CLICK ON ANOTHER TRANSCEIVER'S PANORAMA IS NOT OURS IN EITHER SPELLING: its legacy twin,
/// which names no transceiver, is settled by the receiver's spelling, in either order. CONTROL:
/// the same clicks on our transceiver are reported.
#[test]
fn a_click_on_another_transceivers_panorama_is_never_ours_in_either_spelling() {
    let heard = |ours: u32| {
        let mut b = Bench::ready(Config::new(trx(ours)));
        b.at(10)
            .text("clicked_on_spot:K1ABC,14025000;rx_clicked_on_spot:1,0,K1ABC,14025000;");
        b.at(300).text("rx_clicked_on_spot:1,1,W1AW,7030000;");
        b.at(310).text("clicked_on_spot:W1AW,7030000;");
        b.at(310 + CLICK_PAIR_MS).poll();
        clicks(b.events())
    };
    assert_eq!(heard(0), Vec::<String>::new(), "transceiver 1's clicks");
    assert_eq!(
        heard(1),
        ["K1ABC 14025000 0", "W1AW 7030000 1"],
        "control: on transceiver 1, they are ours"
    );
}

/// A legacy click waits exactly its pairing window, and a burst of clicks is held only up to its
/// bound (the newest kept).
#[test]
fn a_legacy_click_waits_its_window_and_a_burst_is_bounded() {
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).text("clicked_on_spot:K1ABC,14025000;");
    b.at(10 + CLICK_PAIR_MS - 1).poll();
    assert!(clicks(b.events()).is_empty(), "still waiting for its twin");
    b.at(10 + CLICK_PAIR_MS).poll();
    assert_eq!(clicks(b.events()), ["K1ABC 14025000 -"]);
    let burst: String = (0..CLICKS_HELD + 12)
        .map(|n| format!("clicked_on_spot:K{n}ABC,14025000;"))
        .collect();
    b.at(1_000).text(&burst);
    b.at(1_000 + CLICK_PAIR_MS).poll();
    let got = clicks(b.events());
    assert_eq!(got.len(), CLICKS_HELD, "{got:?}");
    assert_eq!(
        got.last().map(String::as_str),
        Some(format!("K{}ABC 14025000 -", CLICKS_HELD + 11).as_str()),
        "the newest are kept"
    );
}

#[test]
fn receive_audio_must_be_fresh_for_a_digital_key() {
    let mut b = Bench::ready(Config::new(trx(0)));
    // The last block came at 3 ms.
    b.at(1_003).text("trx:0,false;");
    assert!(b.s.admits(TxStart::Audio, 1_003).is_ok());
    assert_eq!(
        b.s.admits(TxStart::Audio, 1_004),
        Err(Refusal::NoReceiveAudio)
    );
    assert_eq!(
        b.at(1_004).key(TxStart::Audio),
        Err(Refusal::NoReceiveAudio)
    );
    assert_eq!(b.key(TxStart::Microphone), Ok(()));
}

// ── The security review's three findings (2026-10-20) ──────────────────────────────────────────

#[test]
fn a_chrono_asking_for_more_than_a_frame_can_carry_is_dropped_before_audio_is_sized_by_it() {
    use crate::tci::frame::{MAX_CHRONO_LENGTH, MAX_PAYLOAD};
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Audio).expect("admitted");
    b.wire.take();
    // From the wire the decoder refuses a `length` past MAX_CHRONO_LENGTH: refused and counted.
    for length in [MAX_CHRONO_LENGTH + 1, u32::MAX] {
        let mut bytes = chrono(0);
        bytes[20..24].copy_from_slice(&length.to_le_bytes());
        b.at(11).binary(&bytes);
    }
    assert!(b.wire.take().is_empty());
    assert_eq!(b.s.snapshot().frames.refused, 2);
    // The session holds its own bound, whatever the decoder's: a chrono whose answer would not fit
    // one frame is dropped before anything is sized by it, and Nexus's audio is never asked for it.
    let most = MAX_PAYLOAD / 4;
    let too_big = Chrono {
        trx: trx(0),
        sample_rate: 48_000,
        format: SampleFormat::Float32,
        channels: Channels::One,
        length: u32::try_from(most + 1).expect("fits"),
    };
    b.s.on_chrono(&mut b.wire, &mut b.streams, too_big, 12);
    assert!(
        b.streams.longest <= most,
        "Nexus's audio was asked for {} samples",
        b.streams.longest
    );
    assert!(b.wire.take().is_empty(), "no answer");
    assert_eq!(b.s.snapshot().chronos_refused, 1, "counted");
    // The largest answer one frame carries still goes out.
    let fits = Chrono {
        length: u32::try_from(most).expect("fits"),
        ..too_big
    };
    b.s.on_chrono(&mut b.wire, &mut b.streams, fits, 13);
    assert_eq!(b.streams.longest, most);
    assert_eq!(b.wire.take().len(), 1, "answered");
}

/// One command of every kind [`Command`] has, and every stream setting. The match names every
/// variant, so a new one does not compile until it is classified here too.
fn every_command(t: Trx) -> Vec<Command> {
    use crate::tci::frame::{AudioRate, IqRate};
    use crate::tci::wire::{Call, SpotColour, Text, Token};
    let hz = |v: i64| Hz::new(v).expect("hz");
    let call = || Call::new("K1ABC").expect("call");
    let mut list = vec![
        Command::RadioStart,
        Command::RadioStop,
        Command::Set(Setting::Vfo {
            trx: t,
            channel: Channel::A,
            hz: hz(14_074_000),
        }),
        Command::Set(Setting::drive(t, 30, &EXPERT_SDR3)),
        Command::Set(Setting::drive(t, 30, &EXPERT_SDR2)),
        Command::Set(Setting::TuneDrive { trx: t, level: 10 }),
        Command::Read(Query::Trx(t)),
        Command::Read(Query::Tune(t)),
        Command::Spot {
            call: call(),
            mode: Token::new("FT8").expect("mode"),
            hz: hz(14_074_000),
            colour: SpotColour::Decimal(0xFF00_FF00),
            text: Text::new("cq dx").expect("text"),
        },
        Command::Spot {
            call: call(),
            mode: Token::new("FT8").expect("mode"),
            hz: hz(14_074_000),
            colour: SpotColour::Hash(0xFF00_FF00),
            text: Text::new("cq dx").expect("text"),
        },
        Command::SpotDelete {
            call: call(),
            hz: None,
        },
        Command::SpotDelete {
            call: call(),
            hz: Some(hz(14_074_000)),
        },
        Command::SpotClear,
        Command::RxSensorsEnable {
            on: true,
            interval_ms: Some(200),
        },
        Command::TxSensorsEnable {
            on: false,
            interval_ms: None,
        },
        Command::CwMacrosSpeedUp(2),
        Command::CwMacrosSpeedDown(2),
        Command::SetInFocus,
    ];
    for stream in [
        StreamSetting::AudioStart(t),
        StreamSetting::AudioStop(t),
        StreamSetting::IqStart(t),
        StreamSetting::IqStop(t),
        StreamSetting::LineOutStart(t),
        StreamSetting::LineOutStop(t),
        StreamSetting::AudioSampleRate(AudioRate::new(24_000).expect("rate")),
        StreamSetting::IqSampleRate(IqRate::new(96_000).expect("rate")),
        StreamSetting::AudioSampleType(SampleFormat::Int16),
        StreamSetting::AudioChannels(Channels::One),
        StreamSetting::AudioSamples(1024),
        StreamSetting::TxAudioBuffering(100),
    ] {
        match stream {
            StreamSetting::AudioStart(_)
            | StreamSetting::AudioStop(_)
            | StreamSetting::IqStart(_)
            | StreamSetting::IqStop(_)
            | StreamSetting::LineOutStart(_)
            | StreamSetting::LineOutStop(_)
            | StreamSetting::AudioSampleRate(_)
            | StreamSetting::IqSampleRate(_)
            | StreamSetting::AudioSampleType(_)
            | StreamSetting::AudioChannels(_)
            | StreamSetting::AudioSamples(_)
            | StreamSetting::TxAudioBuffering(_) => list.push(Command::Stream(stream)),
        }
    }
    for c in &list {
        match c {
            Command::RadioStart
            | Command::RadioStop
            | Command::Set(_)
            | Command::Read(_)
            | Command::Stream(_)
            | Command::Spot { .. }
            | Command::SpotDelete { .. }
            | Command::SpotClear
            | Command::RxSensorsEnable { .. }
            | Command::TxSensorsEnable { .. }
            | Command::CwMacrosSpeedUp(_)
            | Command::CwMacrosSpeedDown(_)
            | Command::SetInFocus => {}
        }
    }
    list
}

#[test]
fn nothing_send_writes_is_read_by_a_server_as_a_key() {
    // The review's hypothesis: `send` could carry a CW macro, a CW message or a key past
    // admission. `Command` cannot express one (a key, a CW macro, a CW message, terminal mode and
    // a key-down are `Start`s, rendered only through admission); here every kind of command goes
    // through `send`, and every server dialect reads what went out.
    let mut b = Bench::ready(Config::new(trx(0)));
    for command in every_command(trx(0)) {
        assert_eq!(b.s.send(&mut b.wire, &command, 10), Ok(()), "{command:?}");
    }
    let texts = b.wire.texts();
    assert_eq!(texts.len(), every_command(trx(0)).len());
    for text in &texts {
        for dialect in crate::tci::DIALECTS {
            for read in wire::requests(text, &dialect) {
                assert!(
                    !matches!(read, Ok(wire::Request::Start(_))),
                    "{text} reads as a key in {}",
                    dialect.name
                );
            }
        }
    }
}

#[test]
fn nexus_audio_cannot_be_stopped_or_reshaped_under_an_over_of_ours() {
    // Taken from under a `tci` key, Nexus's audio stream gives the SDR its microphone (TCI 2.0
    // §4.2, TRX: "otherwise the microphone signal is taken").
    use crate::tci::frame::AudioRate;
    let reshape = [
        StreamSetting::AudioStop(trx(0)),
        StreamSetting::AudioSampleRate(AudioRate::new(24_000).expect("rate")),
        StreamSetting::AudioSampleType(SampleFormat::Int16),
        StreamSetting::AudioChannels(Channels::One),
        StreamSetting::AudioSamples(1024),
        StreamSetting::TxAudioBuffering(100),
    ];
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Audio).expect("admitted");
    b.wire.take();
    for stream in reshape {
        assert_eq!(
            b.s.send(&mut b.wire, &Command::Stream(stream), 11),
            Err(SendError::AudioInUse),
            "{stream:?} under an over of ours"
        );
    }
    assert!(b.wire.texts().is_empty(), "none of them went out");
    // IQ, and another receiver's stream, are not this over's audio.
    for stream in [
        StreamSetting::IqStart(trx(0)),
        StreamSetting::AudioStop(trx(1)),
    ] {
        assert_eq!(b.s.send(&mut b.wire, &Command::Stream(stream), 12), Ok(()));
    }
    // The over ended on the server's word: all of it may go out again.
    b.end_ours();
    b.at(13).text("trx:0,false;");
    for stream in reshape {
        assert_eq!(b.s.send(&mut b.wire, &Command::Stream(stream), 14), Ok(()));
    }
    // A microphone key does not use the stream.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Microphone).expect("admitted");
    let stop = Command::Stream(StreamSetting::AudioStop(trx(0)));
    assert_eq!(b.s.send(&mut b.wire, &stop, 11), Ok(()));
}

#[test]
fn the_server_stopping_nexus_audio_mid_over_halts_it() {
    for report in ["audio_stop:0;", "audio_stop:x;"] {
        let mut b = Bench::ready(Config::new(trx(0)));
        b.at(10).key(TxStart::Audio).expect("admitted");
        b.at(11).chrono(0);
        b.text("trx:0,true;");
        b.events();
        b.wire.take();
        b.at(20).text(report);
        assert_eq!(
            b.events(),
            [Event::Halted(Halt::AudioStopped)],
            "{report}: not halted"
        );
        assert_eq!(b.wire.texts(), ["trx:0,false;"], "{report}");
    }
    // A microphone key does not use the stream.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).key(TxStart::Microphone).expect("admitted");
    b.at(11).text("trx:0,true;");
    b.events();
    b.at(20).text("audio_stop:0;");
    assert!(b.events().is_empty());
}

#[test]
fn a_refused_drive_report_is_unknown_on_the_receiver_it_names_or_on_every_one() {
    let drives = |b: &Bench| {
        (
            b.s.model().drive(trx(0)).clone(),
            b.s.model().drive(trx(1)).clone(),
        )
    };
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).text("drive:1,40;");
    // The receiver reads: only its drive is unknown.
    b.text("drive:1,x;");
    assert_eq!(
        drives(&b),
        (Reading::Known(50), Reading::Unreadable),
        "receiver 1's report"
    );
    // No receiver that reads: every receiver's drive is unknown.
    for report in ["drive:x;", "drive:7x,30;", "drive:99,30;", "drive:-1,30;"] {
        b.text("drive:0,50;drive:1,40;");
        b.text(report);
        assert_eq!(
            drives(&b),
            (Reading::Unreadable, Reading::Unreadable),
            "{report}"
        );
    }
    // A drive report cut off at a message's end: every receiver too.
    b.text("drive:0,50;drive:1,40;");
    b.text("drive:1,");
    assert_eq!(
        drives(&b),
        (Reading::Unreadable, Reading::Unreadable),
        "cut"
    );
}

/// What the panadapter's tap was handed: each block's frames and the centre it came with.
#[derive(Default)]
struct IqRecorder {
    got: std::sync::Mutex<Vec<(usize, Option<i64>)>>,
}

impl IqTap for IqRecorder {
    fn iq(&self, block: &Block<'_>, centre: Option<Hz>) {
        self.got
            .lock()
            .unwrap()
            .push((block.frames(), centre.map(Hz::get)));
    }
}

impl IqRecorder {
    fn take(&self) -> Vec<(usize, Option<i64>)> {
        std::mem::take(&mut *self.got.lock().unwrap())
    }
}

/// ⭐ THE PANADAPTER'S TAP GETS THIS TRANSCEIVER'S IQ, WITH THE CENTRE IT WAS SENT UNDER: the centre
/// is the server's last `dds` report before the block, in stream order, so a block sent before a
/// retune is placed where it was taken. Another transceiver's IQ, receive audio and a frame that
/// does not decode never reach it; the owner's streams still get every IQ block of ours.
#[test]
fn the_panadapter_tap_gets_this_transceivers_iq_with_the_centre_it_was_sent_under() {
    let mut b = Bench::ready(Config::new(trx(0)));
    let tap = Arc::new(IqRecorder::default());
    b.s.set_iq_tap(Some(tap.clone()));
    b.at(10).binary(&block(0, BlockKind::Iq));
    assert_eq!(tap.take(), [(256, None)], "no centre reported yet");
    b.text("dds:0,14080000;");
    b.binary(&block(0, BlockKind::Iq));
    b.text("dds:0,14090000;");
    b.binary(&block(0, BlockKind::Iq));
    assert_eq!(
        tap.take(),
        [(256, Some(14_080_000)), (256, Some(14_090_000))],
        "each block with the centre before it"
    );
    b.binary(&block(1, BlockKind::Iq));
    b.text("dds:1,7074000;");
    b.binary(&block(0, BlockKind::RxAudio));
    b.binary(&[0u8; 10]);
    assert!(tap.take().is_empty(), "not ours, not IQ, or not a frame");
    assert_eq!(b.streams.iq, 3, "the owner's streams get our IQ too");
    b.s.set_iq_tap(None);
    b.binary(&block(0, BlockKind::Iq));
    assert!(tap.take().is_empty(), "no tap: nothing handed");
    assert_eq!(b.streams.iq, 4);
}

/// ⭐ A CLOSE STOPS THE IQ STREAM THIS SESSION STARTED, AFTER THE STOPS: `iq_stop` follows the
/// transmitter's stops (it keys nothing) and comes before the close frame, also while a close waits
/// for an unkey. An IQ stream already stopped, or another transceiver's, is not stopped again.
#[test]
fn a_close_stops_the_iq_stream_this_session_started_after_the_stops() {
    let iq = |n: u32, on: bool| {
        Command::Stream(if on {
            StreamSetting::IqStart(trx(n))
        } else {
            StreamSetting::IqStop(trx(n))
        })
    };
    let mut b = Bench::ready(Config::new(trx(0)));
    b.s.send(&mut b.wire, &iq(0, true), 10).expect("sent");
    assert_eq!(b.wire.texts(), ["iq_start:0;"]);
    b.at(20).close();
    assert_eq!(
        b.wire.take(),
        [
            Out::Text("trx:0,false;".into()),
            Out::Text("iq_stop:0;".into()),
            Out::Close
        ]
    );

    // Keyed: the stops, `iq_stop`, then the wait for the unkey before the close frame.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.s.send(&mut b.wire, &iq(0, true), 10).expect("sent");
    b.at(11).key(TxStart::Audio).expect("admitted");
    b.at(12).text("trx:0,true;");
    b.wire.take();
    b.at(20).close();
    assert_eq!(
        b.wire.texts(),
        ["trx:0,false;", "iq_stop:0;"],
        "before the wait"
    );
    b.at(30).text("trx:0,false;");
    assert_eq!(b.wire.take(), [Out::Close]);

    // Started and stopped: the close does not stop it again. Another transceiver's is not ours.
    let mut b = Bench::ready(Config::new(trx(0)));
    for command in [iq(0, true), iq(0, false), iq(1, true)] {
        b.s.send(&mut b.wire, &command, 10).expect("sent");
    }
    b.wire.take();
    b.at(20).close();
    assert_eq!(
        b.wire.take(),
        [Out::Text("trx:0,false;".into()), Out::Close]
    );

    // Never started: nothing.
    let mut b = Bench::ready(Config::new(trx(0)));
    b.at(10).close();
    assert_eq!(
        b.wire.take(),
        [Out::Text("trx:0,false;".into()), Out::Close]
    );
}
