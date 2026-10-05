//! The session core, driven by hand: bytes in, frames out, time passed in. No socket.
//!
//! Translated from upstream's `radio_connection_session_test.cpp` (the line assembly and prologue
//! rows; the demo-radio rows test code that is not ported) and `flex_ptt_wire_session_test.cpp`
//! (the composition rows; the ones about a coordinator's queued certificate have no counterpart,
//! because evidence is consumed in the step that completes it). Each test names the upstream case
//! it translates. The rest are Nexus's own: the transmit rules, keepalive, teardown, and the
//! property test that no input makes the session key on its own.

use super::*;
use crate::flex::encode::tests::is_keying_text;
use crate::flex::reconnect::{Ladder, Step};

const OURS: u32 = 0x1234_5678;
const FOREIGN: u32 = 0x8765_4321;
const PREVIOUS: u32 = 0x1234_5600;
const IDLE: &str =
    "S0|interlock tx_client_handle=0x00000000 state=READY reason= source= tx_allowed=1 amplifier=";
const REQUESTED: &str =
    "S0|interlock tx_client_handle=0x12345678 state=PTT_REQUESTED reason= source=SW tx_allowed=1 amplifier=";
const TRANSMITTING: &str =
    "S0|interlock tx_client_handle=0x12345678 state=TRANSMITTING reason= source=SW tx_allowed=1 amplifier=";
const UNKEY: &str =
    "S0|interlock tx_client_handle=0x12345678 state=UNKEY_REQUESTED reason= source= tx_allowed=1 amplifier=";
const READY_OWNED: &str =
    "S0|interlock tx_client_handle=0x12345678 state=READY reason= source= tx_allowed=1 amplifier=";
const OUR_TX_SLICE: &str =
    "S12345678|slice 0 in_use=1 tx=1 client_handle=0x12345678 RF_frequency=14.074000";

fn config() -> Config {
    let mut c = Config::new(Station::new("Test").unwrap());
    c.ping_interval_ms = 1000;
    c.previous_handles = vec![PREVIOUS];
    c
}

/// Parse written frames back: `(seq, command)`, skipping the disconnect marker.
fn frames(out: &[u8]) -> Vec<(u32, String)> {
    String::from_utf8_lossy(out)
        .split('\n')
        .filter_map(|l| {
            let l = l.trim_start_matches('\u{4}');
            let (seq, cmd) = l.strip_prefix('C')?.split_once('|')?;
            Some((seq.parse().ok()?, cmd.to_string()))
        })
        .collect()
}

struct Harness {
    s: Session,
    out: Vec<u8>,
    now: u64,
}

impl Harness {
    fn new() -> Harness {
        Harness::with(config())
    }

    fn with(config: Config) -> Harness {
        Harness {
            s: Session::new(config, 0),
            out: Vec::new(),
            now: 0,
        }
    }

    fn bytes(&mut self, b: &[u8]) {
        self.s.on_bytes(&mut self.out, b, self.now);
    }

    fn feed(&mut self, text: &str) {
        self.bytes(text.as_bytes());
    }

    /// One line, terminator added.
    fn line(&mut self, text: &str) {
        self.feed(&format!("{text}\n"));
    }

    fn tick(&mut self, ms: u64) {
        self.now += ms;
        self.s.poll(&mut self.out, self.now);
    }

    fn texts(&self) -> Vec<String> {
        frames(&self.out).into_iter().map(|(_, t)| t).collect()
    }

    /// The sequence number of the last command written with this text.
    fn seq_of(&self, text: &str) -> u32 {
        frames(&self.out)
            .into_iter()
            .rev()
            .find(|(_, t)| t == text)
            .unwrap_or_else(|| panic!("{text:?} was not written: {:?}", self.texts()))
            .0
    }

    fn reply(&mut self, text: &str, code: &str, message: &str) {
        let seq = self.seq_of(text);
        self.line(&format!("R{seq}|{code}|{message}"));
    }

    fn events(&mut self) -> Vec<Event> {
        self.s.take_events()
    }

    /// The prologue, then registration and setup accepted, ending Ready.
    fn register(&mut self) {
        self.feed("V1.4.0.0\nH12345678\n");
        self.reply("client gui", "0", "6F1C2A3B-0000-4000-8000-00000000B001");
        self.reply("slice list", "0", "");
        assert_eq!(self.s.phase(), Phase::Ready, "{:?}", self.events());
    }

    /// Registered, with our transmit slice and an idle interlock: everything admission needs.
    fn ready_to_key() -> Harness {
        let mut h = Harness::new();
        h.register();
        h.line(OUR_TX_SLICE);
        h.line(IDLE);
        assert!(h.s.transmit_ready());
        h
    }

    fn start(&mut self) -> Result<u32, Refusal> {
        self.s.start(&mut self.out, TxStart::Key, self.now)
    }

    fn stop(&mut self) -> StopOutcome {
        self.s.stop(&mut self.out, TxStop::Unkey, self.now)
    }

    /// Key and see the radio take it: the reply, PTT_REQUESTED, TRANSMITTING.
    fn key_readback(&mut self) {
        self.reply("xmit 1", "0", "");
        self.line(REQUESTED);
        self.line(TRANSMITTING);
    }

    /// The unkey's whole readback.
    fn stop_readback(&mut self) {
        self.reply("xmit 0", "0", "");
        self.line(UNKEY);
        self.line(READY_OWNED);
        self.line(IDLE);
    }

    /// Starts written, counted by the kind the encoder gave them.
    fn starts_written(&self) -> usize {
        self.s
            .wrote
            .iter()
            .filter(|(_, k, _)| matches!(k, Kind::Start(_)))
            .count()
    }

    /// Keying commands on the wire, counted by an independent reading of the bytes.
    fn keying_on_the_wire(&self) -> usize {
        self.texts().iter().filter(|t| is_keying_text(t)).count()
    }
}

#[test]
fn whitespace_only_lines_are_ignored() {
    // whitespaceOnlyLinesAreIgnored.
    let mut h = Harness::new();
    h.feed("\n\r\n \n\t\r\n \t \n");
    assert!(h.events().is_empty());
    h.feed("V1.4.0.0\nH12345678\n");
    h.line(IDLE);
    assert!(h.s.transmit_ready());
    let before = h.events().len();
    assert!(before > 0);
    h.feed(" \t \r\n");
    assert!(h.s.transmit_ready());
    assert!(h.events().is_empty());
}

#[test]
fn a_partial_interlock_recovers_without_reconnect() {
    // partialInterlockRecoversWithoutReconnect.
    let mut h = Harness::new();
    h.feed("V1.4.0.0\nH12345678\nS0|interlock tx_allowed=0\n");
    assert!(!h.s.transmit_ready());
    h.line(IDLE);
    assert!(h.s.transmit_ready());
    h.line("S0|interlock tx_allowed=1");
    assert!(!h.s.transmit_ready(), "a partial line withdraws readiness");
    h.line(IDLE);
    assert!(h.s.transmit_ready(), "a whole idle restores it");
    assert_eq!(h.keying_on_the_wire(), 0);
}

#[test]
fn the_prologue_decides_transmit() {
    // independentPttPrologue: protocol alone is not idle proof, and only a first, well-formed
    // 1.4 prologue with a nonzero handle lets the session key.
    for (prologue, supported) in [
        ("V1.4.0.0\nH12345678\n", true),
        ("V1.4.0.0\r\nH12345678\r\n", true),
        ("V1.4.7.99\nH12345678\n", true),
        ("V1.3.0.0\nH12345678\n", false),
        ("V2.0.0.0\nH12345678\n", false),
        ("H12345678\n", false),
        ("V1.4.0.0beta\nH12345678\n", false),
        (" V1.4.0.0\nH12345678\n", false),
        ("V1.4.0.0 \nH12345678\n", false),
        ("H12345678\nV1.4.0.0\n", false),
        ("V1.4.0.0\nV1.4.0.0\nH12345678\n", false),
        ("V1.4.0.0\nH12345678\nH12345678\n", false),
        ("V1.4.0.0\nH+1234567\n", false),
        ("V1.4.0.0\nH00000000\n", false),
    ] {
        let mut h = Harness::new();
        h.feed(prologue);
        assert!(
            !h.s.transmit_ready(),
            "{prologue:?}: protocol alone is not idle proof"
        );
        h.line(IDLE);
        assert_eq!(h.s.transmit_ready(), supported, "{prologue:?}");
        // Finish registration (the session goes on either way, receive-only when refused) and
        // offer it everything else admission wants.
        if h.texts().contains(&"client gui".to_string()) {
            h.reply("client gui", "0", "");
            h.reply("slice list", "0", "");
        }
        h.line(OUR_TX_SLICE);
        h.line(IDLE);
        let _ = h.start();
        assert_eq!(
            h.keying_on_the_wire(),
            usize::from(supported),
            "{prologue:?}"
        );
    }
}

#[test]
fn transmit_eligibility_does_not_carry_across_sessions() {
    // independentPttDoesNotCarryAcrossSessions: each connection is a new session, and a
    // prologue that omits the version does not inherit the last session's.
    let mut first = Harness::new();
    first.feed("V1.4.0.0\nH12345678\n");
    first.line(IDLE);
    assert!(first.s.transmit_ready());
    first.s.transport_closed(first.now);
    assert!(!first.s.transmit_ready());
    let mut second = Harness::new();
    second.feed("H12345678\n");
    second.line(IDLE);
    assert!(!second.s.transmit_ready());
    let mut third = Harness::new();
    third.feed("V1.4.0.0\nH12345678\n");
    third.line(IDLE);
    assert!(third.s.transmit_ready());
}

#[test]
fn protocol_loss_keeps_the_unkey() {
    // independentPttProtocolLossRetainsCleanup and protocolLossRetainsStop.
    let mut h = Harness::ready_to_key();
    h.start().expect("keyed");
    h.key_readback();
    h.line("V2.0.0.0");
    assert!(!h.s.transmit_ready());
    assert!(h.events().contains(&Event::ProtocolRejected));
    // A second key is refused; the unkey still goes out.
    assert_eq!(h.start(), Err(Refusal::ProtocolUnsupported));
    assert!(matches!(h.stop(), StopOutcome::Sent { .. }));
    assert_eq!(
        h.texts()
            .iter()
            .filter(|t| t.starts_with("xmit"))
            .collect::<Vec<_>>(),
        ["xmit 1", "xmit 0"]
    );
    // The changed protocol cannot prove the handoff: keyed stays, and at the deadline the session
    // unkeys again and closes.
    h.stop_readback();
    assert!(h.s.keyed(), "no proof after a protocol contradiction");
    h.tick(TRANSITION_TIMEOUT_MS);
    let events = h.events();
    assert!(events.contains(&Event::UnkeyUnconfirmed), "{events:?}");
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Closed {
            end: End::UnkeyUnconfirmed,
            was_keyed: true,
            ..
        }
    )));
}

#[test]
fn transmit_needs_a_live_eligible_interlock() {
    // independentPttRequiresLiveTransmitEligibility.
    for status in [
        "S0|interlock tx_client_handle=0x00000000 state=RECEIVE reason= source= tx_allowed=0",
        "S0|interlock tx_client_handle=0x00000000 state=READY reason= source= tx_allowed=0",
        "S0|interlock tx_client_handle=0x87654321 state=TRANSMITTING reason= source=SW tx_allowed=1",
        "S0|interlock state=READY",
    ] {
        let mut h = Harness::new();
        h.register();
        h.line(OUR_TX_SLICE);
        h.line(status);
        assert!(!h.s.transmit_ready(), "{status}");
        assert!(h.start().is_err(), "{status}");
        assert_eq!(h.keying_on_the_wire(), 0, "{status}");
    }
}

#[test]
fn a_partial_line_dies_with_its_session() {
    // partialLineAcrossDisconnect and newTcpSessionResetsOldBytes: a partial line is never a
    // value, and nothing of an ended session reaches the next.
    for partial in ["Vold", "H", "R42|", "S123|radio nickname=old", "Mold"] {
        for ending in 0..3 {
            let mut old = Harness::new();
            old.feed("V1.4.0.0\nH00000001\n");
            old.feed(partial);
            assert!(old.s.assembler.has_partial());
            match ending {
                0 => old.s.transport_closed(old.now),
                1 => old.s.close(&mut old.out, old.now),
                _ => old.tick(60_000),
            }
            assert!(old.s.is_closed(), "{partial:?} {ending}");
            assert_eq!(
                old.s.model().radio.nickname,
                None,
                "{partial:?}: never a value"
            );

            let mut new = Harness::new();
            new.feed("V1.4.");
            assert!(new.events().is_empty(), "half a version is nothing yet");
            new.feed("0.0\nH00AB");
            let events = new.events();
            assert!(
                matches!(events.as_slice(), [Event::Prologue { .. }]),
                "{events:?}"
            );
            new.feed("CDEF\n");
            assert_eq!(new.s.handle(), Some(0x00AB_CDEF));
            new.line("R42|0|late");
            assert!(
                !new.events()
                    .iter()
                    .any(|e| matches!(e, Event::Reply { .. })),
                "an old session's reply is no reply here"
            );
        }
    }
}

#[test]
fn the_assembler_never_returns_a_partial_line() {
    let mut a = LineAssembler::default();
    assert!(a.push(b"S0|slice 0 RF_frequency=14.07").unwrap().is_empty());
    assert!(a.has_partial());
    assert_eq!(
        a.push(b"4000\r\nR1|0|\n\n  \nV1").unwrap(),
        [
            b"S0|slice 0 RF_frequency=14.074000".to_vec(),
            b"R1|0|".to_vec()
        ]
    );
    assert!(a.has_partial());
    // Only one trailing CR is a terminator's.
    assert_eq!(a.push(b"\r\r\n").unwrap(), [b"V1\r".to_vec()]);
    // A run with no terminator past the cap ends the session.
    let mut a = LineAssembler::default();
    assert_eq!(a.push(&vec![b'x'; MAX_LINE + 1]), Err(TooLong));
    let mut h = Harness::new();
    h.bytes(&vec![b'x'; MAX_LINE + 1]);
    assert!(h.s.is_closed());
    assert!(h.events().iter().any(|e| matches!(
        e,
        Event::Closed {
            end: End::ProtocolError,
            ..
        }
    )));
}

#[test]
fn supports_protocol_rows() {
    // protocolEligibility.
    for v in ["1.4.0.0", "1.4.9.12345", "1.4.2147483647.0"] {
        assert!(supports_protocol(v), "{v}");
    }
    for v in [
        "",
        "1.4",
        "1.4.0",
        "1.4.0.0.1",
        "1.4.0.0beta",
        "1.4.0.-1",
        "1.4.0.+1",
        "1.4.0.01",
        "1.4.0.2147483648",
        "1.4.0.999999999999999999999999999999999999",
        " 1.4.0.0",
        "1.4.0.0 ",
        "1.3.0.0",
        "1.5.0.0",
        "2.4.0.0",
        "4.2.18.41174",
    ] {
        assert!(!supports_protocol(v), "{v:?}");
    }
}

#[test]
fn rapid_release_and_handoff() {
    // rapidReleaseAndHandoff: the unkey goes out before the key's readback, and only the whole
    // ordered sequence clears keyed.
    let mut h = Harness::ready_to_key();
    h.start().expect("keyed");
    assert!(matches!(h.stop(), StopOutcome::Sent { .. }));
    assert_eq!(
        h.texts()
            .iter()
            .filter(|t| t.starts_with("xmit"))
            .collect::<Vec<_>>(),
        ["xmit 1", "xmit 0"]
    );
    h.key_readback();
    h.reply("xmit 0", "0", "");
    h.line(UNKEY);
    h.line(READY_OWNED);
    assert!(h.s.keyed(), "READY still naming us is not the release");
    h.line(IDLE);
    assert!(!h.s.keyed());
    assert!(h.events().contains(&Event::UnkeyConfirmed));
    // The next key reaches the writer.
    h.start().expect("a fresh key after the confirmed handoff");
    assert_eq!(h.keying_on_the_wire(), 2);
}

#[test]
fn a_stop_with_nothing_of_ours_writes_nothing() {
    // noDispatch: a release with no key is locally provable, and sends no unkey that could stop
    // another client.
    let mut h = Harness::new();
    h.register();
    h.line("S0|interlock tx_client_handle=0x87654321 state=TRANSMITTING reason= source=SW tx_allowed=1");
    let before = h.texts().len();
    assert_eq!(h.stop(), StopOutcome::NothingOfOurs);
    assert_eq!(h.texts().len(), before);
    // The interlock naming us is ours to stop, keyed or not.
    h.line(READY_OWNED);
    assert!(matches!(h.stop(), StopOutcome::Sent { .. }));
}

#[test]
fn keyed_clears_only_on_the_readback() {
    let mut h = Harness::ready_to_key();
    h.start().expect("keyed");
    h.key_readback();
    assert!(matches!(h.stop(), StopOutcome::Sent { .. }));
    h.reply("xmit 0", "0", "");
    assert!(
        h.s.keyed(),
        "a successful reply to xmit 0 is not RF cessation"
    );
    assert_eq!(
        h.start(),
        Err(Refusal::AlreadyKeyed),
        "no second key while unconfirmed"
    );
    h.line(UNKEY);
    h.line(READY_OWNED);
    h.line(IDLE);
    assert!(!h.s.keyed());
}

#[test]
fn an_unconfirmed_unkey_escalates() {
    // The stuck-transmitter guard, by hand: xmit 0 answered, the interlock never moves.
    let mut h = Harness::ready_to_key();
    h.start().expect("keyed");
    h.key_readback();
    h.stop();
    h.reply("xmit 0", "0", "");
    h.tick(TRANSITION_TIMEOUT_MS - 1);
    assert!(h.s.keyed());
    assert!(!h.s.is_closed());
    h.tick(1);
    let events = h.events();
    assert!(events.contains(&Event::UnkeyUnconfirmed), "{events:?}");
    assert_eq!(
        h.texts().iter().filter(|t| *t == "xmit 0").count(),
        2,
        "the unkey went out again"
    );
    assert!(h.s.is_closed());
    // The ladder then unkeys locally before anything else.
    let (end, was_keyed, handle) = events
        .iter()
        .find_map(|e| match e {
            Event::Closed {
                end,
                was_keyed,
                handle,
            } => Some((end.clone(), *was_keyed, *handle)),
            _ => None,
        })
        .expect("closed");
    assert_eq!(end, End::UnkeyUnconfirmed);
    let steps = Ladder::new().ended(&end, was_keyed, handle);
    assert_eq!(steps.first(), Some(&Step::UnkeyLocally));
}

#[test]
fn a_missed_ping_while_keyed_unkeys_first() {
    let mut h = Harness::ready_to_key();
    h.tick(0); // the first ping
    h.reply("ping", "0", "");
    h.start().expect("keyed");
    h.key_readback();
    h.tick(1000); // ping 2 goes out
    assert!(!h.texts().contains(&"xmit 0".to_string()));
    h.tick(1000); // ping 2 was not answered
    let texts = h.texts();
    let unkey = texts.iter().position(|t| t == "xmit 0").expect("unkeyed");
    let last_ping = texts.iter().rposition(|t| t == "ping").unwrap();
    assert!(
        unkey < last_ping,
        "the unkey goes before the next ping: {texts:?}"
    );
    assert!(h
        .events()
        .contains(&Event::UnkeyedOnMissedPing { misses: 1 }));
}

#[test]
fn five_missed_pings_end_the_session_and_four_do_not() {
    let mut h = Harness::new();
    h.register();
    h.tick(0);
    for _ in 0..4 {
        h.tick(1000);
    }
    assert!(!h.s.is_closed(), "four missed replies are survivable");
    h.reply("ping", "0", "");
    for _ in 0..5 {
        h.tick(1000);
    }
    assert!(!h.s.is_closed());
    h.tick(1000);
    assert!(h.s.is_closed());
    assert!(h.events().iter().any(|e| matches!(
        e,
        Event::Closed {
            end: End::KeepaliveLost,
            ..
        }
    )));
}

#[test]
fn replies_match_by_sequence_number_not_order() {
    let mut h = Harness::new();
    h.feed("V1.4.0.0\nH12345678\n");
    h.reply("client gui", "0", "");
    // The slice list reply comes before the mic list reply.
    h.reply("slice list", "0", "");
    h.reply("mic list", "0", "MIC,BAL,LINE,ACC");
    assert_eq!(
        h.s.model().mic_inputs.as_deref(),
        Some(&["MIC", "BAL", "LINE", "ACC"].map(String::from)[..])
    );
    assert!(h.events().contains(&Event::Ready {
        slices: Some(vec![])
    }));
    // A reply to a number this session never issued is nobody's.
    h.line("R9999|0|stray");
    assert!(!h.events().iter().any(|e| matches!(e, Event::Reply { .. })));
}

#[test]
fn commands_at_objects_not_ours_are_refused() {
    let mut h = Harness::new();
    h.register();
    h.line("S87654321|slice 1 in_use=1 tx=1 client_handle=0x87654321 pan=0x40000001");
    h.line("S87654321|display pan 0x40000001 client_handle=0x87654321");
    h.line(OUR_TX_SLICE);
    let before = h.texts().len();
    for command in [
        Command::SliceTune {
            slice: 1,
            freq_hz: 7_100_000.0,
            keep_pan: false,
        },
        Command::SliceRemove { slice: 1 },
        Command::PanCenter {
            pan: 0x4000_0001,
            freq_hz: 7_100_000.0,
        },
        Command::SliceCreate {
            pan: Some(0x4000_0001),
            freq_hz: 7_100_000.0,
            mode: None,
        },
        Command::SliceTune {
            slice: 5,
            freq_hz: 7_100_000.0,
            keep_pan: false,
        },
    ] {
        assert!(
            matches!(
                h.s.send(&mut h.out, &command, h.now),
                Err(SendError::NotOurs { .. })
            ),
            "{command:?}"
        );
    }
    assert_eq!(h.texts().len(), before, "nothing reached the wire");
    // Our own slice is ours to tune.
    h.s.send(
        &mut h.out,
        &Command::SliceTune {
            slice: 0,
            freq_hz: 14_075_000.0,
            keep_pan: true,
        },
        h.now,
    )
    .expect("ours");
    assert_eq!(
        h.texts().last().unwrap(),
        "slice tune 0 14.075000 autopan=0"
    );
}

#[test]
fn teardown_removes_our_streams_before_the_marker() {
    let mut h = Harness::new();
    h.register();
    h.line("S12345678|stream 0x04000001 type=dax_rx client_handle=0x12345678 ip=10.0.0.2");
    h.line("S87654321|stream 0x04000002 type=dax_rx client_handle=0x87654321 ip=10.0.0.3");
    h.s.close(&mut h.out, h.now);
    assert_eq!(h.s.phase(), Phase::Closing);
    assert_eq!(
        h.texts().last().unwrap(),
        "stream remove 0x04000001",
        "ours only"
    );
    assert!(!h.out.contains(&0x04), "the marker waits for the reply");
    h.reply("stream remove 0x04000001", "0", "");
    assert!(h.s.is_closed());
    assert_eq!(h.out.last(), Some(&0x04), "then the marker");
    // An unanswered removal closes at the deadline.
    let mut h = Harness::new();
    h.register();
    h.line("S12345678|stream 0x04000001 type=dax_rx client_handle=0x12345678 ip=10.0.0.2");
    h.s.close(&mut h.out, h.now);
    h.tick(1999);
    assert!(!h.s.is_closed());
    h.tick(1);
    assert!(h.s.is_closed());
}

#[test]
fn closing_while_keyed_unkeys_first() {
    let mut h = Harness::ready_to_key();
    h.start().expect("keyed");
    h.key_readback();
    h.s.close(&mut h.out, h.now);
    assert_eq!(h.texts().last().unwrap(), "xmit 0");
    assert!(h.events().iter().any(|e| matches!(
        e,
        Event::Closed {
            end: End::Closed,
            was_keyed: true,
            ..
        }
    )));
}

#[test]
fn a_refused_registration_sends_no_setup() {
    let mut h = Harness::new();
    h.feed(
        "V1.4.0.0\nH12345678\nMF3000001|The maximum number of connected clients has been reached\n",
    );
    h.reply("client gui", "F3000001", "");
    let events = h.events();
    assert!(
        events.contains(&Event::RegistrationRejected {
            code: 0xF300_0001,
            detail: "The maximum number of connected clients has been reached".into()
        }),
        "{events:?}"
    );
    assert!(h.s.is_closed());
    assert_eq!(h.texts(), ["client program Nexus", "client gui"]);
    assert_eq!(h.out.last(), Some(&0x04));
}

#[test]
fn the_connect_sequence_on_the_wire() {
    let mut c = config();
    c.connect.udp_port = Some(4993);
    let mut h = Harness::with(c);
    h.feed("V1.4.0.0\nH12345678\n");
    assert_eq!(h.texts(), ["client program Nexus", "client gui"]);
    h.reply("client gui", "0", "6F1C2A3B-0000-4000-8000-00000000B001");
    let events = h.events();
    assert!(events.contains(&Event::Registered {
        client_id: ClientId::parse("6F1C2A3B-0000-4000-8000-00000000B001")
    }));
    assert_eq!(
        h.texts(),
        [
            "client program Nexus",
            "client gui",
            "client station Test",
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
            "client udpport 4993",
            "slice list",
        ]
    );
    // Sequence numbers are the client's own, increasing from 1.
    let seqs: Vec<u32> = frames(&h.out).into_iter().map(|(s, _)| s).collect();
    assert_eq!(seqs, (1..=19).collect::<Vec<u32>>());
    h.reply(
        "client udpport 4993",
        "500000A9",
        "Port/IP pair already in use",
    );
    assert!(h.events().contains(&Event::UdpPortInUse { port: 4993 }));
    h.reply("slice list", "0", "0 2");
    assert!(h.events().contains(&Event::Ready {
        slices: Some(vec![0, 2])
    }));
    // The first ping is due as soon as time is looked at.
    h.tick(0);
    assert_eq!(h.texts().last().unwrap(), "ping");
}

/// What the test's UDP registration writes into the wire's log.
const DATAGRAM: &str = "<the registration datagram>";

/// A transport that also logs each command's text, in a log shared with the test's UDP
/// registration: one log, in the order the session acted, rather than two sockets' arrivals.
struct LoggingWire {
    out: Vec<u8>,
    log: Arc<Mutex<Vec<String>>>,
}

impl Write for LoggingWire {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.out.extend_from_slice(buf);
        let mut log = self.log.lock().unwrap();
        log.extend(frames(buf).into_iter().map(|(_, text)| text));
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A session past its prologue whose UDP registration logs [`DATAGRAM`] in the wire's log.
fn registering() -> (Session, LoggingWire) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut c = config();
    c.connect.udp_port = Some(4993);
    let sent = Arc::clone(&log);
    c.udp_registration = Some(UdpRegistration::new(move || {
        sent.lock().unwrap().push(DATAGRAM.to_string());
    }));
    let mut s = Session::new(c, 0);
    let mut wire = LoggingWire {
        out: Vec::new(),
        log,
    };
    s.on_bytes(&mut wire, b"V1.4.0.0\nH12345678\n", 0);
    (s, wire)
}

/// The radio's reply to `client gui`: `code|message`.
fn answer_gui(s: &mut Session, wire: &mut LoggingWire, reply: &str) {
    let (seq, _) = frames(&wire.out)
        .into_iter()
        .find(|(_, text)| text == "client gui")
        .expect("premise: client gui was sent");
    s.on_bytes(wire, format!("R{seq}|{reply}\n").as_bytes(), 0);
}

/// ⭐ THE UDP REGISTRATION'S DATAGRAM GOES AFTER REGISTRATION, JUST BEFORE `client udpport`, where
/// upstream sends it (port plan §4.5, step 7). The radio learns the client's UDP endpoint from its
/// source, which firmware that answers `client udpport` with "not supported" needs. Its owner sent
/// it after the whole bring-up (about 46 ms after the command), and then before the session even
/// connected. The datagram is its owner's to send (the session has no I/O of its own); here it is
/// a mark in the wire's own log, so the order asserted is the order the session acted in.
#[test]
fn the_registration_datagram_goes_after_registration_just_before_client_udpport() {
    let (mut s, mut wire) = registering();
    assert_eq!(
        *wire.log.lock().unwrap(),
        ["client program Nexus", "client gui"],
        "the datagram went before the radio registered the client"
    );
    answer_gui(&mut s, &mut wire, "0|6F1C2A3B-0000-4000-8000-00000000B001");
    let sent = wire.log.lock().unwrap().clone();
    let at = |text: &str| {
        sent.iter()
            .position(|l| l == text)
            .unwrap_or_else(|| panic!("no {text:?} in {sent:?}"))
    };
    assert_eq!(at(DATAGRAM), at("mic list") + 1, "{sent:?}");
    assert_eq!(at("client udpport 4993"), at(DATAGRAM) + 1, "{sent:?}");
    assert_eq!(
        sent.iter().filter(|l| *l == DATAGRAM).count(),
        1,
        "{sent:?}"
    );

    // A radio that refuses the registration is sent none.
    let (mut s, mut wire) = registering();
    answer_gui(&mut s, &mut wire, "F3000001|too many clients");
    assert!(s.is_closed(), "premise: the radio refused the registration");
    assert!(
        !wire.log.lock().unwrap().iter().any(|l| l == DATAGRAM),
        "a radio that refused the client was sent the datagram"
    );
}

#[test]
fn a_previous_session_holding_the_transmitter_is_reported_and_stoppable() {
    let mut h = Harness::new();
    h.register();
    h.line(OUR_TX_SLICE);
    h.line("S0|interlock tx_client_handle=0x12345600 state=TRANSMITTING reason= source=SW tx_allowed=1");
    assert!(h
        .events()
        .contains(&Event::PreviousSessionHoldsTransmitter { handle: PREVIOUS }));
    assert_eq!(h.start(), Err(Refusal::TransmitterHeld { by: PREVIOUS }));
    // Ours from before: the unkey is sent, though this session never keyed.
    assert!(matches!(h.stop(), StopOutcome::Sent { .. }));
    assert_eq!(h.texts().last().unwrap(), "xmit 0");
    // Another client's transmitter is never ours to stop.
    let mut h = Harness::new();
    h.register();
    h.line("S0|interlock tx_client_handle=0x87654321 state=TRANSMITTING reason= source=SW tx_allowed=1");
    assert_eq!(h.stop(), StopOutcome::NothingOfOurs);
}

#[test]
fn a_failed_key_write_ends_the_session_keyed() {
    // partialAndStale, first row: a key write that did not complete is never proof that nothing
    // was sent. Here a failed write ends the session, and the ladder unkeys locally first.
    struct Failing;
    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(ErrorKind::BrokenPipe, "gone"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut h = Harness::ready_to_key();
    let mut failing = Failing;
    let _ = h.s.start(&mut failing, TxStart::Key, h.now);
    let events = h.events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::Closed {
                end: End::Lost,
                was_keyed: true,
                ..
            }
        )),
        "{events:?}"
    );
}

// ── The property: no input makes the session key on its own ──────────────────────────────────

/// xorshift64*: a seeded, dependency-free generator, so every run is reproducible by seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

fn handle_text(rng: &mut Rng) -> String {
    format!("0x{:08X}", [0, OURS, FOREIGN, PREVIOUS][rng.below(4)])
}

/// A line drawn from what a radio says, what a broken radio might say, and noise: every object
/// the decoders route, values well formed and not, interlock samples whole and partial, replies
/// to numbers in flight and not.
fn random_line(rng: &mut Rng, in_flight: &[u32]) -> String {
    let value = |rng: &mut Rng| {
        rng.pick(&[
            "0",
            "1",
            "-5",
            "14.074000",
            "garbage",
            "",
            "0x40000000",
            "1e9",
            "NaN",
        ])
        .to_string()
    };
    match rng.below(14) {
        0..=2 => {
            let mut parts = vec![
                format!("tx_client_handle={}", handle_text(rng)),
                format!(
                    "state={}",
                    rng.pick(&[
                        "READY",
                        "RECEIVE",
                        "NOT_READY",
                        "PTT_REQUESTED",
                        "TRANSMITTING",
                        "UNKEY_REQUESTED"
                    ])
                ),
                format!("source={}", rng.pick(&["", "", "SW", "MIC", "ACC"])),
                format!("tx_allowed={}", rng.pick(&["1", "1", "0", "x"])),
                format!("reason={}", rng.pick(&["", "", "OUT_OF_BAND"])),
            ];
            if rng.chance(30) {
                parts.remove(rng.below(parts.len()));
            }
            if rng.chance(10) {
                parts.push(parts[0].clone());
            }
            format!(
                "S{}|interlock {} amplifier=",
                rng.pick(&["0", "12345678", "87654321"]),
                parts.join(" ")
            )
        }
        3 | 4 => format!(
            "S{}|slice {} in_use={} tx={} client_handle={} RF_frequency={} mode={}",
            rng.pick(&["0", "12345678"]),
            rng.below(4),
            rng.pick(&["1", "1", "0"]),
            rng.pick(&["1", "0"]),
            handle_text(rng),
            value(rng),
            rng.pick(&["USB", "DIGU", "CW"])
        ),
        5 => format!(
            "S0|transmit tune={} mox={} rfpower={} dax={} synccwx={}",
            value(rng),
            value(rng),
            value(rng),
            value(rng),
            value(rng)
        ),
        6 => {
            let seq = if !in_flight.is_empty() && rng.chance(70) {
                in_flight[rng.below(in_flight.len())].to_string()
            } else {
                rng.pick(&["0", "1", "4294967295", "garbage", "77"])
                    .to_string()
            };
            format!(
                "R{seq}|{}|{}",
                rng.pick(&["0", "0", "F3000001", "500000A9", "50000015", "zz", ""]),
                rng.pick(&["", "0", "MIC,BAL", "0 1 2", "pan=0x40000000"])
            )
        }
        7 => rng
            .pick(&["V1.4.0.0", "V2.0.0.0", "H12345678", "H00000000", "Hzz", "V"])
            .to_string(),
        8 => format!(
            "M{}|{}",
            rng.pick(&["10000001", "F3000001", "00000001", "zz"]),
            rng.pick(&["Client connected", "xmit 1", "atu start", ""])
        ),
        9 => format!(
            "S{}|client {} {} local_ptt={} program={} station={}",
            rng.pick(&["0", "87654321"]),
            handle_text(rng),
            rng.pick(&["connected", "disconnected", ""]),
            value(rng),
            rng.pick(&["SmartSDR-Win", "Nexus"]),
            rng.pick(&["Shack", "xmit"])
        ),
        10 => format!(
            "S0|{}",
            rng.pick(&[
                "display pan 0x40000000 client_handle=0x12345678 center=14.1 bandwidth=0.2",
                "display waterfall 0x42000000 client_handle=0x87654321 panadapter=0x40000000",
                "stream 0x04000001 type=dax_tx client_handle=0x12345678 ip=0.0.0.0",
                "stream 0x04000001 removed",
                "atu status=TUNE_IN_PROGRESS atu_enabled=1",
                "meter 1.src=TX-#1.nam=FWDPWR#1.unit=dBm",
                "profile tx list=Default^DX",
                "interlock band 20 band_name=20m",
                "radio slices=4 model=FLEX-6400",
            ])
        ),
        11 => rng
            .pick(&[
                IDLE,
                REQUESTED,
                TRANSMITTING,
                UNKEY,
                READY_OWNED,
                OUR_TX_SLICE,
            ])
            .to_string(),
        12 => String::from_utf8_lossy(&[0xff, b'S', b'0', b'|', 0xfe]).into_owned(),
        _ => rng
            .pick(&[
                "",
                " ",
                "\t",
                "Xjunk",
                "S0",
                "R",
                "C1|xmit 1",
                "S0|",
                "S0|=",
                "R1|0",
            ])
            .to_string(),
    }
}

/// Feed `lines` in chunks cut at random byte offsets, with time passing between them.
fn feed_randomly(h: &mut Harness, rng: &mut Rng, lines: &str) {
    let bytes = lines.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        let n = 1 + rng.below(bytes.len() - at);
        h.bytes(&bytes[at..at + n]);
        at += n;
        if rng.chance(30) {
            h.tick(rng.below(1500) as u64);
        }
    }
}

#[test]
fn no_input_makes_the_session_originate_a_key() {
    // The transmit rule's property test. For thousands of seeded runs, a session in each state
    // (fresh, registered with our transmit slice and an idle interlock, keyed by the test, closing)
    // is fed random lines in random pieces with time passing. The session may write stops and
    // ordinary commands; the only start it may ever write is the one the test asked for. Checked
    // twice: by the kind the encoder gave each write, and by reading the bytes independently.
    let mut cases = 0;
    for seed in 1..=600u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let state = seed % 4;
        let mut h = match state {
            0 => Harness::new(),
            _ => Harness::ready_to_key(),
        };
        let mut asked = 0;
        if state == 2 {
            h.start().expect("the test's own key");
            asked = 1;
        }
        if state == 3 {
            h.line("S12345678|stream 0x04000001 type=dax_rx client_handle=0x12345678 ip=10.0.0.2");
            h.s.close(&mut h.out, h.now);
        }
        for _ in 0..40 {
            let in_flight: Vec<u32> = h.s.pending.keys().copied().collect();
            let mut batch = String::new();
            for _ in 0..1 + rng.below(4) {
                batch.push_str(&random_line(&mut rng, &in_flight));
                batch.push_str(if rng.chance(10) { "\r\n" } else { "\n" });
            }
            // Answer the latest ping often enough that most runs stay connected for the length
            // of the run instead of ending on keepalive.
            if rng.chance(60) {
                let ping =
                    h.s.pending
                        .iter()
                        .rev()
                        .find(|(_, p)| **p == Pending::Ping)
                        .map(|(seq, _)| *seq);
                if let Some(seq) = ping {
                    batch.push_str(&format!("R{seq}|0|\n"));
                }
            }
            feed_randomly(&mut h, &mut rng, &batch);
            // Stops are always allowed, and must never become a start.
            if rng.chance(5) {
                let stop = [TxStop::Unkey, TxStop::TuneOff, TxStop::CwxClear][rng.below(3)];
                let _ = h.s.stop(&mut h.out, stop, h.now);
            }
            cases += 1;
        }
        assert_eq!(
            h.starts_written(),
            asked,
            "seed {seed}: kinds {:?}",
            h.s.wrote
        );
        assert_eq!(
            h.keying_on_the_wire(),
            asked,
            "seed {seed}: wire {:?}",
            h.texts()
        );
        for (_, kind, text) in &h.s.wrote {
            if !matches!(kind, Kind::Start(_)) {
                assert!(!is_keying_text(text), "seed {seed}: {kind:?} {text:?}");
            }
        }
    }
    assert!(cases >= 20_000);

    // The reconnect ladder's steps: whatever ends a session, none of them is a keying command
    // (the step type has none), and a keyed end unkeys first.
    let mut ladder = Ladder::new();
    let mut rng = Rng(7);
    for _ in 0..1000 {
        let end = match rng.below(6) {
            0 => End::Lost,
            1 => End::KeepaliveLost,
            2 => End::UnkeyUnconfirmed,
            3 => End::ProtocolError,
            4 => End::Closed,
            _ => End::RegistrationRejected {
                code: 0xF300_0001,
                detail: String::new(),
            },
        };
        let keyed = rng.chance(50);
        let steps = ladder.ended(&end, keyed, Some(OURS));
        for step in &steps {
            match step {
                Step::UnkeyLocally | Step::Retry { .. } | Step::WaitForOperator(_) => {}
            }
        }
        if keyed {
            assert_eq!(steps.first(), Some(&Step::UnkeyLocally));
        }
    }
}

/// One connection serves several callers at once (the rigctld shim's client threads and the
/// radio loop), so it must be shareable across threads. A compile-time check.
#[test]
fn a_connection_can_be_shared_across_threads() {
    fn shared<T: Send + Sync>() {}
    shared::<Connection>();
}
