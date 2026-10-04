//! The session against the SmartSDR simulator (`tempo-flexsim`), over real TCP on loopback.
//!
//! Each of the simulator's faults is run against the guard it exists to trip, and each test also
//! runs its control, the same exchange without the fault, so a guard that passes cannot be
//! passing because the fault never happened. Waits are on events and state, never fixed sleeps,
//! except where a test shows that something keeps not happening. Every test ends by counting the
//! keying commands on the wire against the starts the test itself asked for.
//!
//! | Fault | Guard | Test |
//! |---|---|---|
//! | `SplitLine` | line assembly never returns a partial line as a value | [`a_split_line_is_never_a_value`] |
//! | `ReorderReply` | replies matched by sequence number | [`replies_are_matched_by_sequence_number`] |
//! | `DropPings` | keepalive: four misses survive, five end the session; a miss during an over unkeys | [`keepalive_survives_four_missed_replies_and_ends_on_five`], [`a_missed_ping_during_an_over_unkeys_at_once`] |
//! | `StuckTransmit` | the unkey readback; escalation; the next session will not key under the old handle | [`a_stuck_transmitter_escalates_and_the_next_session_will_not_key`] |
//! | `ForeignClient` | ownership by client handle; never key a transmitter that is not ours | [`another_clients_objects_are_shown_never_touched`], [`another_clients_transmitter_is_never_taken`] |
//! | `DisconnectMidOver` | unkey locally first; never swap under a keyed transmitter | [`a_session_lost_mid_over_unkeys_first_and_reconnects_without_a_swap`] |
//! | `Vita` | receive continuity | not here: nothing in this module reads VITA-49 yet; the simulator's own tests trip the shipped `flexvita` guards |

use std::time::{Duration, Instant};

use tempo_flexsim::session::{Item, Pattern};
use tempo_flexsim::{
    Closer, Config as SimConfig, Event as SimEvent, Fault, Foreign, Session as SimSession,
    Simulator,
};

use super::admission::Refusal;
use super::encode::{Command, Station, TxStart, TxStop};
use super::model::{ObjectRef, Owner};
use super::reconnect::{End, Ladder, Step};
use super::session::{Config, ConnError, Connection, Event, SendError, StopOutcome};
use super::wire::split_status;

/// The longest any single wait may take before a test fails.
const WAIT: Duration = Duration::from_secs(10);

const IDLE: &str =
    "S0|interlock tx_client_handle=0x00000000 state=READY reason= source= tx_allowed=1 amplifier=";

fn simulator(session: SimSession, faults: Vec<Fault>) -> Simulator {
    Simulator::start(
        session,
        SimConfig {
            faults,
            // The radio's own keepalive is not under test here; the client's is.
            keepalive_timeout: Duration::from_secs(120),
            ..SimConfig::default()
        },
    )
    .expect("the simulator starts")
}

/// Production timing, except a shorter unkey deadline and teardown wait. Tests of keepalive set
/// their own ping interval; the rest keep one ping a second, so a loaded machine cannot fake a
/// missed ping in a test that is about something else.
fn config(previous_handles: Vec<u32>) -> Config {
    let mut c = Config::new(Station::new("Nexus").unwrap());
    c.unkey_deadline_ms = 1500;
    c.teardown_timeout_ms = 1000;
    c.previous_handles = previous_handles;
    c
}

/// The bundled session with the idle interlock moved from `slice create` to `info`, so a test
/// decides, connection by connection, whether the radio reports itself idle. (The simulator
/// remembers who holds the transmitter only for `sub tx all`; its scripted `slice create` would
/// otherwise report an idle radio even while an old handle holds the transmitter.)
fn idle_on_info() -> SimSession {
    let mut s = SimSession::v4_gui_client();
    for (pattern, rules) in &mut s.rules {
        match pattern {
            Pattern::Prefix(p) if p == "slice create " => {
                for rule in rules.iter_mut() {
                    rule.items
                        .retain(|i| !matches!(i, Item::Send(l) if l.contains("|interlock ")));
                }
            }
            Pattern::Exact(p) if p == "info" => {
                for rule in rules.iter_mut() {
                    rule.items.push(Item::Send(IDLE.to_string()));
                }
            }
            _ => {}
        }
    }
    s
}

/// A client: the connection, and every event it has reported so far.
struct Client {
    conn: Connection,
    seen: Vec<Event>,
}

impl Client {
    fn connect(sim: &Simulator, config: Config) -> Client {
        Client {
            conn: Connection::connect(sim.tcp_addr(), config).expect("connect"),
            seen: Vec::new(),
        }
    }

    /// Connected, registered and subscribed.
    fn ready(sim: &Simulator, config: Config) -> Client {
        let mut c = Client::connect(sim, config);
        c.wait("Ready", |e| matches!(e, Event::Ready { .. }));
        c
    }

    /// The first event, seen or to come, that satisfies `pred`.
    fn wait(&mut self, what: &str, pred: impl Fn(&Event) -> bool) -> Event {
        if let Some(e) = self.seen.iter().find(|e| pred(e)) {
            return e.clone();
        }
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.conn.next_event(left) {
                Some(e) => {
                    self.seen.push(e.clone());
                    if pred(&e) {
                        return e;
                    }
                }
                None => panic!("no {what} within {WAIT:?}; seen {:#?}", self.seen),
            }
        }
    }

    /// Take in every event already reported.
    fn drain(&mut self) {
        while let Some(e) = self.conn.next_event(Duration::ZERO) {
            self.seen.push(e);
        }
    }

    fn handle(&self) -> u32 {
        self.conn.snapshot().handle.expect("a client handle")
    }

    /// `slice create`: the simulator's radio makes slice 0, ours, the transmit slice.
    fn create_slice(&mut self) {
        let reply = self
            .conn
            .request(
                Command::SliceCreate {
                    pan: None,
                    freq_hz: 14_074_000.0,
                    mode: None,
                },
                WAIT,
            )
            .expect("slice create answered");
        assert!(reply.is_success(), "{reply:?}");
        assert!(
            self.conn
                .wait_until(WAIT, |s| s.model.slices.contains_key(&0)),
            "slice 0 reported"
        );
    }

    /// `info`, which under [`idle_on_info`] makes the radio report itself idle.
    fn info(&mut self) {
        let reply = self
            .conn
            .request(Command::Info, WAIT)
            .expect("info answered");
        assert!(reply.is_success());
    }

    fn wait_transmit_ready(&self) {
        assert!(
            self.conn.wait_until(WAIT, |s| s.transmit_ready
                && s.model.tx_slices() == [0]
                && s.model
                    .interlock
                    .sample
                    .as_ref()
                    .is_some_and(|x| x.state == "READY")),
            "transmit ready: {:?}",
            self.conn.snapshot()
        );
    }

    fn wait_interlock(&self, state: &str) {
        assert!(
            self.conn.wait_until(WAIT, |s| s
                .model
                .interlock
                .sample
                .as_ref()
                .is_some_and(|x| x.state == state)),
            "interlock {state}: {:?}",
            self.conn.snapshot().model.interlock
        );
    }

    fn closed(&mut self) -> (End, bool, Option<u32>) {
        match self.wait("Closed", |e| matches!(e, Event::Closed { .. })) {
            Event::Closed {
                end,
                was_keyed,
                handle,
            } => (end, was_keyed, handle),
            _ => unreachable!(),
        }
    }
}

/// Commands the simulator received on one connection, pings left out.
fn commands(sim: &Simulator, conn: usize) -> Vec<String> {
    sim.events()
        .into_iter()
        .filter_map(|e| match e {
            SimEvent::Command { conn: c, text, .. } if c == conn && text != "ping" => Some(text),
            _ => None,
        })
        .collect()
}

/// Every keying command the simulator received, on any connection, by an independent reading.
fn keys_on_the_wire(sim: &Simulator) -> usize {
    sim.commands()
        .iter()
        .filter(|c| c.as_str() == "xmit 1")
        .count()
}

/// Exactly `n` keying commands reached the simulator. The simulator logs what it reads on its own
/// thread, so wait for the log to reach `n` before checking that it is not more.
fn assert_keys(sim: &Simulator, n: usize) {
    let keys = |log: &[tempo_flexsim::Logged]| {
        log.iter()
            .filter(|l| matches!(&l.event, SimEvent::Command { text, .. } if text == "xmit 1"))
            .count()
    };
    assert!(sim.wait_for(WAIT, |log| keys(log) >= n), "{n} keys logged");
    assert_eq!(keys_on_the_wire(sim), n);
}

fn position(sim: &Simulator, pred: impl Fn(&SimEvent) -> bool) -> Option<usize> {
    sim.events().iter().position(pred)
}

#[test]
fn the_connect_sequence_reaches_the_radio_in_order() {
    let sim = simulator(SimSession::v4_gui_client(), vec![]);
    let mut c = Client::ready(&sim, config(vec![]));
    assert_eq!(
        commands(&sim, 0),
        [
            "client program Nexus",
            "client gui",
            "client station Nexus",
            "client set send_reduced_bw_dax=1",
            "client set enforce_network_mtu=1 network_mtu=1450",
            "keepalive enable",
            "sub slice all",
            "sub pan all",
            "sub tx all",
            "sub atu all",
            "sub meter all",
            "sub audio all",
            "sub gps all",
            "sub client all",
            "sub radio all",
            "sub xvtr all",
            "mic list",
            "slice list",
        ]
    );
    c.wait("Registered", |e| {
        matches!(e, Event::Registered { client_id: Some(id) }
            if id.as_str() == "6F1C2A3B-0000-4000-8000-00000000B001")
    });
    assert!(c.seen.contains(&Event::Ready {
        slices: Some(vec![])
    }));
    assert!(c.conn.wait_until(WAIT, |s| s.model.meters.len() == 3
        && s.model.radio.model.as_deref() == Some("FLEX-6400")
        && s.model
            .interlock
            .sample
            .as_ref()
            .is_some_and(|x| x.state == "RECEIVE")));
    let snapshot = c.conn.snapshot();
    assert!(snapshot.transmit_protocol);
    assert_eq!(snapshot.handle, Some(0x2B6E_1F40));
    c.conn.close();
    assert!(sim.wait_for(WAIT, |log| log.iter().any(|l| matches!(
        l.event,
        SimEvent::Closed {
            conn: 0,
            by: Closer::Client
        }
    ))));
    assert_eq!(keys_on_the_wire(&sim), 0);
}

#[test]
fn a_split_line_is_never_a_value() {
    // Fault::SplitLine → line assembly. The slice status is cut inside its frequency, and the
    // rest is held until the client's next command (its next ping).
    let line = "S2B6E1F40|slice 0 in_use=1 RF_frequency=14.074000";
    let cut = line.find("14.07").unwrap() + "14.07".len();
    for faulted in [false, true] {
        let faults = if faulted {
            vec![Fault::SplitLine {
                containing: "RF_frequency=".into(),
                cuts: vec![cut],
                max_hold: Duration::from_secs(5),
            }]
        } else {
            vec![]
        };
        let sim = simulator(SimSession::v4_gui_client(), faults);
        let mut c = Client::ready(&sim, config(vec![]));
        c.create_slice();
        assert!(c.conn.wait_until(WAIT, |s| s
            .model
            .slices
            .get(&0)
            .and_then(|d| d.frequency_mhz)
            == Some(14.074)));
        let pieces = sim.events().into_iter().find_map(|e| match e {
            SimEvent::Split { pieces, .. } => Some(pieces),
            _ => None,
        });
        assert_eq!(
            pieces.is_some(),
            faulted,
            "the fault fired (or not, in the control)"
        );
        if let Some(pieces) = pieces {
            // The positive control: a reader that takes each read as whole lines would have read
            // the first piece as a value, and that value is wrong.
            let first = pieces[0].trim_end();
            let (_, kvs) = split_status(first.split_once('|').unwrap().1);
            assert_eq!(kvs.get("RF_frequency"), Some("14.07"), "{pieces:?}");
        }
        c.drain();
        assert!(
            !c.seen
                .iter()
                .any(|e| matches!(e, Event::LineRejected { .. })),
            "{:?}",
            c.seen
        );
        assert_eq!(keys_on_the_wire(&sim), 0);
    }
}

#[test]
fn replies_are_matched_by_sequence_number() {
    // Fault::ReorderReply → sequence matching. The `mic list` reply is held until after the
    // `slice list` reply.
    for faulted in [false, true] {
        let faults = if faulted {
            vec![Fault::ReorderReply {
                command: "mic list".into(),
            }]
        } else {
            vec![]
        };
        let sim = simulator(SimSession::v4_gui_client(), faults);
        let c = Client::ready(&sim, config(vec![]));
        assert!(c.seen.contains(&Event::Ready {
            slices: Some(vec![])
        }));
        assert!(c.conn.wait_until(WAIT, |s| s.model.mic_inputs.is_some()));
        assert_eq!(
            c.conn.snapshot().model.mic_inputs.unwrap(),
            ["MIC", "BAL", "LINE", "ACC"]
        );
        let seq = |text: &str| {
            sim.events()
                .into_iter()
                .find_map(|e| match e {
                    SimEvent::Command { seq, text: t, .. } if t == text => Some(seq),
                    _ => None,
                })
                .unwrap()
        };
        let (mic, slices) = (seq("mic list"), seq("slice list"));
        let sent = |s: u32| {
            position(
                &sim,
                |e| matches!(e, SimEvent::Sent { line, .. } if line.starts_with(&format!("R{s}|"))),
            )
            .unwrap()
        };
        assert_eq!(
            sent(slices) < sent(mic),
            faulted,
            "the replies arrived out of order (or in order, in the control)"
        );
        assert_eq!(
            sim.events()
                .iter()
                .any(|e| matches!(e, SimEvent::ReplyHeld { .. })),
            faulted
        );
    }
}

#[test]
fn keepalive_survives_four_missed_replies_and_ends_on_five() {
    // Fault::DropPings → keepalive.
    let mut cfg = config(vec![]);
    cfg.ping_interval_ms = 100;
    // Four lost: pings 3–6. The session is still up after pings 7 and 8 are answered.
    let sim = simulator(
        SimSession::v4_gui_client(),
        vec![Fault::DropPings { first: 3, count: 4 }],
    );
    let mut c = Client::ready(&sim, cfg.clone());
    assert!(sim.wait_for(WAIT, |log| log
        .iter()
        .filter(|l| matches!(&l.event, SimEvent::Command { text, .. } if text == "ping"))
        .count()
        >= 9));
    let dropped = sim
        .events()
        .iter()
        .filter(|e| matches!(e, SimEvent::PingDropped { .. }))
        .count();
    assert_eq!(dropped, 4, "the fault fired");
    c.drain();
    assert!(
        !c.seen.iter().any(|e| matches!(e, Event::Closed { .. })),
        "{:?}",
        c.seen
    );
    drop(c);

    // Five lost: the session ends, by the client, after exactly five.
    let sim = simulator(
        SimSession::v4_gui_client(),
        vec![Fault::DropPings {
            first: 3,
            count: 100,
        }],
    );
    let mut c = Client::ready(&sim, cfg);
    let (end, was_keyed, _) = c.closed();
    assert_eq!(end, End::KeepaliveLost);
    assert!(!was_keyed);
    assert!(sim.wait_for(WAIT, |log| log.iter().any(|l| matches!(
        l.event,
        SimEvent::Closed {
            conn: 0,
            by: Closer::Client
        }
    ))));
    let dropped = sim
        .events()
        .iter()
        .filter(|e| matches!(e, SimEvent::PingDropped { .. }))
        .count();
    assert_eq!(dropped, 5);
    assert_eq!(keys_on_the_wire(&sim), 0);
}

#[test]
fn a_missed_ping_during_an_over_unkeys_at_once() {
    // Fault::DropPings during an over → the unkey goes out at the first miss, before the session
    // ends on the fifth.
    for faulted in [false, true] {
        let mut cfg = config(vec![]);
        cfg.ping_interval_ms = 300;
        // Ping 10 is about 2.7 s after registration: the over has long started by then.
        let faults = if faulted {
            vec![Fault::DropPings {
                first: 10,
                count: 100,
            }]
        } else {
            vec![]
        };
        let sim = simulator(SimSession::v4_gui_client(), faults);
        let mut c = Client::ready(&sim, cfg);
        c.create_slice();
        c.wait_transmit_ready();
        c.conn.start(TxStart::Key).expect("keyed");
        c.wait_interlock("TRANSMITTING");
        if faulted {
            match c.wait("the unkey", |e| {
                matches!(e, Event::UnkeyedOnMissedPing { .. })
            }) {
                Event::UnkeyedOnMissedPing { misses } => assert_eq!(misses, 1),
                _ => unreachable!(),
            }
            // The simulator logs what it reads on its own thread: wait for the unkey to land.
            assert!(sim.wait_for(WAIT, |log| log.iter().any(|l| matches!(
                &l.event,
                SimEvent::Command { text, .. } if text == "xmit 0"
            ))));
            let key = position(
                &sim,
                |e| matches!(e, SimEvent::Command { text, .. } if text == "xmit 1"),
            )
            .unwrap();
            let first_drop = position(&sim, |e| matches!(e, SimEvent::PingDropped { .. })).unwrap();
            let unkey = position(
                &sim,
                |e| matches!(e, SimEvent::Command { text, .. } if text == "xmit 0"),
            )
            .expect("xmit 0 on the wire");
            assert!(
                key < first_drop,
                "the over started before the first lost ping"
            );
            assert!(first_drop < unkey);
            // The readback still arrives and confirms, and the keepalive then ends the session.
            c.wait("UnkeyConfirmed", |e| *e == Event::UnkeyConfirmed);
            let (end, _, _) = c.closed();
            assert_eq!(end, End::KeepaliveLost);
            assert!(sim.wait_for(WAIT, |log| log
                .iter()
                .any(|l| matches!(l.event, SimEvent::Closed { conn: 0, .. }))));
            let closed =
                position(&sim, |e| matches!(e, SimEvent::Closed { conn: 0, .. })).expect("closed");
            assert!(
                unkey < closed,
                "the unkey went before the end of the session"
            );
        } else {
            // The control: an over with every ping answered runs on with no unkey.
            std::thread::sleep(Duration::from_millis(1800));
            assert!(!commands(&sim, 0).contains(&"xmit 0".to_string()));
            assert!(c.conn.snapshot().keyed);
            assert!(matches!(
                c.conn.stop(TxStop::Unkey),
                StopOutcome::Sent { .. }
            ));
            c.wait("UnkeyConfirmed", |e| *e == Event::UnkeyConfirmed);
        }
        assert_keys(&sim, 1);
    }
}

#[test]
fn a_stuck_transmitter_escalates_and_the_next_session_will_not_key() {
    // Fault::StuckTransmit → the unkey readback. `xmit 0` is answered with success and the
    // interlock never moves.
    for faulted in [false, true] {
        let faults = if faulted {
            vec![Fault::StuckTransmit]
        } else {
            vec![]
        };
        let sim = simulator(idle_on_info(), faults);
        let mut ladder = Ladder::new();
        let mut c = Client::ready(&sim, config(ladder.previous_handles()));
        c.create_slice();
        c.info();
        c.wait_transmit_ready();
        let first_handle = c.handle();
        c.conn.start(TxStart::Key).expect("keyed");
        c.wait_interlock("TRANSMITTING");
        let StopOutcome::Sent { seq } = c.conn.stop(TxStop::Unkey) else {
            panic!("the unkey was not sent")
        };
        c.wait(
            "the xmit 0 reply",
            |e| matches!(e, Event::Reply { seq: s, code: 0, .. } if *s == seq),
        );
        if !faulted {
            c.wait("UnkeyConfirmed", |e| *e == Event::UnkeyConfirmed);
            assert!(!c.conn.snapshot().keyed);
            assert_eq!(c.conn.snapshot().phase, super::session::Phase::Ready);
            assert_eq!(
                commands(&sim, 0).iter().filter(|t| *t == "xmit 0").count(),
                1
            );
            assert_keys(&sim, 1);
            continue;
        }
        // The reply came, and keyed did not clear: a reply is not RF cessation.
        assert!(c.conn.snapshot().keyed);
        c.wait("UnkeyUnconfirmed", |e| *e == Event::UnkeyUnconfirmed);
        let (end, was_keyed, handle) = c.closed();
        assert_eq!((end.clone(), was_keyed), (End::UnkeyUnconfirmed, true));
        assert!(sim.wait_for(WAIT, |log| log
            .iter()
            .any(|l| matches!(l.event, SimEvent::Closed { conn: 0, .. }))));
        assert_eq!(
            commands(&sim, 0).iter().filter(|t| *t == "xmit 0").count(),
            2,
            "the unkey went out again before the close"
        );
        assert!(sim
            .events()
            .iter()
            .any(|e| matches!(e, SimEvent::StatusWithheld { .. })));
        // The ladder: unkey locally first, then retry.
        let steps = ladder.ended(&end, was_keyed, handle);
        assert_eq!(steps[0], Step::UnkeyLocally);
        let Step::Retry { after_ms } = steps[1] else {
            panic!("{steps:?}")
        };
        drop(c);
        std::thread::sleep(Duration::from_millis(after_ms));
        // The next session sees the old handle still keyed: it reports it, refuses to key under
        // it, and sends the unkey for it.
        let mut next = Client::ready(&sim, config(ladder.previous_handles()));
        next.wait("PreviousSessionHoldsTransmitter", |e| {
            *e == Event::PreviousSessionHoldsTransmitter {
                handle: first_handle,
            }
        });
        next.create_slice();
        assert_eq!(
            next.conn.start(TxStart::Key),
            Err(Some(Refusal::TransmitterHeld { by: first_handle }))
        );
        assert!(matches!(
            next.conn.stop(TxStop::Unkey),
            StopOutcome::Sent { .. }
        ));
        assert!(sim.wait_for(WAIT, |log| log.iter().any(|l| matches!(
            &l.event,
            SimEvent::Command { conn: 1, text, .. } if text == "xmit 0"
        ))));
        assert!(!commands(&sim, 1).contains(&"xmit 1".to_string()));
        assert_keys(&sim, 1);
    }
}

#[test]
fn another_clients_objects_are_shown_never_touched() {
    // Fault::ForeignClient → ownership by client handle. Another GUI client's slice is the
    // transmit slice, and its pan and waterfall are on the radio.
    for faulted in [false, true] {
        let foreign = Foreign::default();
        let faults = if faulted {
            vec![Fault::ForeignClient(foreign.clone())]
        } else {
            vec![]
        };
        let sim = simulator(SimSession::v4_gui_client(), faults);
        let mut c = Client::ready(&sim, config(vec![]));
        c.create_slice();
        assert!(c.conn.wait_until(WAIT, |s| s
            .model
            .interlock
            .sample
            .as_ref()
            .is_some_and(|x| x.state == "READY")));
        let snap = c.conn.snapshot();
        if !faulted {
            c.wait_transmit_ready();
            c.conn
                .start(TxStart::Key)
                .expect("our own transmit slice keys");
            c.wait_interlock("TRANSMITTING");
            c.conn.stop(TxStop::Unkey);
            c.wait("UnkeyConfirmed", |e| *e == Event::UnkeyConfirmed);
            assert_keys(&sim, 1);
            continue;
        }
        let theirs = Owner::Foreign(foreign.handle);
        for object in [
            ObjectRef::Slice(foreign.slice),
            ObjectRef::Pan(foreign.pan),
            ObjectRef::Waterfall(foreign.waterfall),
        ] {
            assert_eq!(snap.model.owner(object, snap.handle), theirs, "{object:?}");
        }
        assert!(
            snap.model.clients.contains_key(&foreign.handle),
            "shown as another client"
        );
        assert_eq!(
            snap.model.slices[&0].tx,
            Some(false),
            "ours is not the transmit slice"
        );
        assert_eq!(
            c.conn.start(TxStart::Key),
            Err(Some(Refusal::TxSliceNotOurs {
                slice: foreign.slice,
                owner: theirs
            }))
        );
        for command in [
            Command::SliceTune {
                slice: foreign.slice,
                freq_hz: 7_100_000.0,
                keep_pan: false,
            },
            Command::SliceRemove {
                slice: foreign.slice,
            },
            Command::PanCenter {
                pan: foreign.pan,
                freq_hz: 7_150_000.0,
            },
            Command::PanRemove { pan: foreign.pan },
            Command::WaterfallRemove {
                waterfall: foreign.waterfall,
            },
            Command::SliceCreate {
                pan: Some(foreign.pan),
                freq_hz: 7_100_000.0,
                mode: None,
            },
        ] {
            assert!(
                matches!(
                    c.conn.request(command.clone(), WAIT),
                    Err(ConnError::Refused(SendError::NotOurs { .. }))
                ),
                "{command:?}"
            );
        }
        // The transcript: nothing named their objects, and nothing keyed.
        let named: Vec<String> = sim
            .commands()
            .into_iter()
            .filter(|t| foreign.is_named_by(t))
            .collect();
        assert!(named.is_empty(), "{named:?}");
        assert_eq!(keys_on_the_wire(&sim), 0);
    }
}

#[test]
fn another_clients_transmitter_is_never_taken() {
    // Fault::ForeignClient with the other client transmitting: our slice is the transmit slice,
    // but the interlock names their handle.
    for faulted in [false, true] {
        let foreign = Foreign {
            tx_slice: false,
            transmitting: true,
            ..Foreign::default()
        };
        let faults = if faulted {
            vec![Fault::ForeignClient(foreign.clone())]
        } else {
            vec![]
        };
        let sim = simulator(idle_on_info(), faults);
        let mut c = Client::ready(&sim, config(vec![]));
        c.create_slice();
        if faulted {
            c.wait_interlock("TRANSMITTING");
            assert_eq!(
                c.conn.start(TxStart::Key),
                Err(Some(Refusal::TransmitterHeld { by: foreign.handle }))
            );
            assert_eq!(
                c.conn.stop(TxStop::Unkey),
                StopOutcome::NothingOfOurs,
                "their transmission is not ours to stop"
            );
            assert!(!sim.commands().iter().any(|t| t.starts_with("xmit")));
        } else {
            c.info();
            c.wait_transmit_ready();
            c.conn.start(TxStart::Key).expect("keyed");
            assert_keys(&sim, 1);
            c.conn.stop(TxStop::Unkey);
            c.wait("UnkeyConfirmed", |e| *e == Event::UnkeyConfirmed);
        }
    }
}

#[test]
fn a_session_lost_mid_over_unkeys_first_and_reconnects_without_a_swap() {
    // Fault::DisconnectMidOver → losing the session while keyed. Whether a radio unkeys a client
    // whose connection drops is not established; `radio_stays_keyed` plays the worse answer.
    for stays_keyed in [true, false] {
        let sim = simulator(
            idle_on_info(),
            vec![Fault::DisconnectMidOver {
                after: Duration::from_millis(100),
                radio_stays_keyed: stays_keyed,
            }],
        );
        let mut ladder = Ladder::new();
        let mut c = Client::ready(&sim, config(ladder.previous_handles()));
        c.create_slice();
        c.info();
        c.wait_transmit_ready();
        let old = c.handle();
        c.conn.start(TxStart::Key).expect("keyed");
        let (end, was_keyed, handle) = c.closed();
        assert_eq!(
            (end.clone(), was_keyed, handle),
            (End::Lost, true, Some(old))
        );
        assert!(sim.wait_for(WAIT, |log| log.iter().any(|l| matches!(
            l.event,
            SimEvent::Closed {
                conn: 0,
                by: Closer::Simulator
            }
        ))));
        // Unkey locally before anything else; only then a retry.
        let steps = ladder.ended(&end, was_keyed, handle);
        assert_eq!(steps[0], Step::UnkeyLocally);
        let Step::Retry { after_ms } = steps[1] else {
            panic!("{steps:?}")
        };
        drop(c);
        std::thread::sleep(Duration::from_millis(after_ms));
        let mut next = Client::ready(&sim, config(ladder.previous_handles()));
        // Never swapped: the old connection was gone before the new one opened.
        let closed = position(&sim, |e| matches!(e, SimEvent::Closed { conn: 0, .. })).unwrap();
        let opened = position(&sim, |e| matches!(e, SimEvent::Connected { conn: 1, .. })).unwrap();
        assert!(closed < opened);
        next.create_slice();
        if stays_keyed {
            next.wait("PreviousSessionHoldsTransmitter", |e| {
                *e == Event::PreviousSessionHoldsTransmitter { handle: old }
            });
            assert_eq!(
                next.conn.start(TxStart::Key),
                Err(Some(Refusal::TransmitterHeld { by: old }))
            );
            assert!(matches!(
                next.conn.stop(TxStop::Unkey),
                StopOutcome::Sent { .. }
            ));
            assert!(sim.wait_for(WAIT, |log| log.iter().any(|l| matches!(
                &l.event,
                SimEvent::Command { conn: 1, text, .. } if text == "xmit 0"
            ))));
            assert_keys(&sim, 1);
        } else {
            // The control: the old handle let go, so nothing is reported and the next session
            // keys once the radio says it is idle.
            next.drain();
            assert!(!next
                .seen
                .iter()
                .any(|e| matches!(e, Event::PreviousSessionHoldsTransmitter { .. })));
            assert!(!matches!(
                next.conn.start(TxStart::Key),
                Err(Some(Refusal::TransmitterHeld { .. }))
            ));
            next.info();
            next.wait_transmit_ready();
            next.conn
                .start(TxStart::Key)
                .expect("keyed after a clean reconnect");
            assert_keys(&sim, 2);
            next.conn.stop(TxStop::Unkey);
            next.wait("UnkeyConfirmed", |e| *e == Event::UnkeyConfirmed);
        }
    }
}

#[test]
fn a_refused_registration_is_terminal_until_the_operator_connects() {
    // Translated from upstream's gui_client_registration_recovery_test.cpp: a full radio refuses
    // `client gui` with F3000001, its reason in a fatal message before the reply.
    let mut full = SimSession::v4_gui_client();
    for (pattern, rules) in &mut full.rules {
        match pattern {
            Pattern::Exact(p) if p == "client gui" => {
                for rule in rules.iter_mut() {
                    rule.code = "F3000001".into();
                    rule.message.clear();
                }
            }
            Pattern::Prefix(p) if p == "client program " => {
                for rule in rules.iter_mut() {
                    rule.items.push(Item::Send(
                        "MF3000001|The maximum number of connected clients has been reached".into(),
                    ));
                }
            }
            _ => {}
        }
    }
    let sim = simulator(full, vec![]);
    let mut ladder = Ladder::new();
    let mut c = Client::connect(&sim, config(vec![]));
    assert_eq!(
        c.wait("RegistrationRejected", |e| matches!(
            e,
            Event::RegistrationRejected { .. }
        )),
        Event::RegistrationRejected {
            code: 0xF300_0001,
            detail: "The maximum number of connected clients has been reached".into()
        },
        "the radio's reason survives into the refusal"
    );
    let (end, was_keyed, handle) = c.closed();
    let steps = ladder.ended(&end, was_keyed, handle);
    assert!(
        matches!(steps.as_slice(), [Step::WaitForOperator(_)]),
        "no retry while the radio is full: {steps:?}"
    );
    // One registration, and nothing after it: no station, no subscriptions, no UDP.
    assert_eq!(commands(&sim, 0), ["client program Nexus", "client gui"]);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        sim.events()
            .iter()
            .filter(|e| matches!(e, SimEvent::Connected { .. }))
            .count(),
        1,
        "no reconnect on its own"
    );
    // The operator connects again, to a radio with room: a fresh connection registers.
    ladder.operator_connect();
    let roomy = simulator(SimSession::v4_gui_client(), vec![]);
    let mut again = Client::ready(&roomy, config(ladder.previous_handles()));
    again.wait("Registered", |e| matches!(e, Event::Registered { .. }));
    let sent = commands(&roomy, 0);
    assert!(sent.contains(&"client station Nexus".to_string()));
    assert!(sent.contains(&"sub slice all".to_string()));
}

#[test]
fn teardown_removes_our_stream_and_waits_for_its_reply() {
    // Ours and another client's stream, reported at connect.
    let text = format!(
        "{}\nat 0\nsend S{{h}}|stream 0x04000001 type=dax_rx dax_channel=1 client_handle=0x{{h}} ip={{peer}}\n\
         send S7A3C0001|stream 0x04000002 type=dax_rx dax_channel=2 client_handle=0x7A3C0001 ip=10.0.0.9\n",
        tempo_flexsim::session::V4_GUI_CLIENT
    );
    let sim = simulator(SimSession::parse(&text).expect("session"), vec![]);
    let c = Client::ready(&sim, config(vec![]));
    assert!(c.conn.wait_until(WAIT, |s| s.model.streams.len() == 2));
    c.conn.close();
    assert!(sim.wait_for(WAIT, |log| log
        .iter()
        .any(|l| matches!(l.event, SimEvent::Closed { conn: 0, .. }))));
    let remove = position(
        &sim,
        |e| matches!(e, SimEvent::Command { text, .. } if text == "stream remove 0x04000001"),
    )
    .expect("our stream removed");
    let seq = match &sim.events()[remove] {
        SimEvent::Command { seq, .. } => *seq,
        _ => unreachable!(),
    };
    let acknowledged = position(
        &sim,
        |e| matches!(e, SimEvent::Sent { line, .. } if line.starts_with(&format!("R{seq}|0|"))),
    )
    .expect("acknowledged");
    let closed = position(&sim, |e| {
        matches!(
            e,
            SimEvent::Closed {
                conn: 0,
                by: Closer::Client
            }
        )
    })
    .unwrap();
    assert!(remove < acknowledged && acknowledged < closed);
    assert!(
        !commands(&sim, 0).contains(&"stream remove 0x04000002".to_string()),
        "another client's stream is not ours to remove"
    );
}
