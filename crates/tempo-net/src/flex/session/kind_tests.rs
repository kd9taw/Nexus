//! The readback per kind against the simulator's radio, on the test's own clock.
//!
//! Every command the session writes is answered from `tempo-flexsim`'s bundled session: its reply
//! at once, its statuses at the times their waits say, each rule's statuses after the earlier
//! rules' (the API's ordering rule, as the simulator keeps it). A fault that swallows a stop
//! swallows it here exactly as in the simulator ([`withholds`]). Time moves a millisecond at a
//! time and the session is polled at each, so a deadline shows at the millisecond it falls.
//!
//! The kinds no bench has confirmed are admitted here through the configuration's test-only door
//! ([`Config::unbenched`]); in production admission refuses their starts. Nexus's own tests.

use super::*;
use crate::flex::encode::CwxText;
use tempo_flexsim::fault::withholds;
use tempo_flexsim::session::Item;
use tempo_flexsim::{Fault, Foreign, Session as SimSession};

/// The bundled session's first handle: ours.
const OURS: u32 = 0x2B6E_1F40;
/// A previous session of ours on this radio.
const PREVIOUS: u32 = 0x2B6E_1F00;

/// An interlock sample naming `handle`, keyed under `source`.
fn keyed_by(handle: u32, source: &str) -> String {
    format!(
        "S0|interlock tx_client_handle=0x{handle:08X} state=TRANSMITTING reason= source={source} \
         tx_allowed=1 amplifier="
    )
}

/// The radio, played from the simulator's rules on the test's clock.
struct Radio {
    rules: SimSession,
    faults: Vec<Fault>,
    /// Lines to deliver: when, in what order they were queued, the line.
    queue: Vec<(u64, u64, String)>,
    queued: u64,
    /// When the last status of the latest rule is due.
    last_status_due: u64,
    /// Pings so far, for the simulator's dropped-ping fault.
    pings: usize,
}

impl Radio {
    fn push(&mut self, due: u64, line: String) {
        self.queued += 1;
        self.queue.push((due, self.queued, line));
    }

    /// The reply to one command at `now`, then its rule's statuses unless a fault withholds them.
    fn answer(&mut self, now: u64, seq: u32, text: &str) {
        if text == "ping" {
            self.pings += 1;
            let lost = self.faults.iter().any(|f| {
                matches!(f, Fault::DropPings { first, count }
                    if (*first..first + count).contains(&self.pings))
            });
            if lost {
                return;
            }
        }
        let expand = |t: &str| t.replace("{h}", &format!("{OURS:08X}"));
        let (code, message, items) = match self.rules.lookup(text) {
            Some(group) => {
                let rule = self.rules.rule(group, 0);
                (rule.code.clone(), rule.message.clone(), rule.items.clone())
            }
            None => (self.rules.default_code.clone(), String::new(), Vec::new()),
        };
        self.push(now, format!("R{seq}|{code}|{}", expand(&message)));
        if withholds(&self.faults, text) {
            return;
        }
        let mut due = self.last_status_due.max(now);
        for item in items {
            match item {
                Item::Wait(ms) => due += ms,
                Item::Send(line) => self.push(due, expand(&line)),
                Item::Vita(_) => {}
            }
        }
        self.last_status_due = self.last_status_due.max(due);
    }

    /// The lines due by `now`, in order.
    fn due(&mut self, now: u64) -> Vec<String> {
        self.queue.sort_by_key(|(due, order, _)| (*due, *order));
        let n = self
            .queue
            .iter()
            .take_while(|(due, _, _)| *due <= now)
            .count();
        self.queue.drain(..n).map(|(_, _, line)| line).collect()
    }
}

/// A session and the radio it talks to.
struct Bench {
    s: Session,
    out: Vec<u8>,
    now: u64,
    radio: Radio,
    /// How much of `out` the radio has read.
    read: usize,
    /// Every command written, with the time it was written.
    wrote: Vec<(u64, String)>,
    events: Vec<Event>,
}

impl Bench {
    /// Connected, registered, with our transmit slice made and the radio idle.
    fn ready(faults: Vec<Fault>, previous_handles: Vec<u32>) -> Bench {
        let mut config = Config::new(Station::new("Nexus").unwrap());
        config.previous_handles = previous_handles;
        config.unbenched = true;
        let mut radio = Radio {
            rules: SimSession::v4_gui_client(),
            faults,
            queue: Vec::new(),
            queued: 0,
            last_status_due: 0,
            pings: 0,
        };
        radio.push(0, radio.rules.prologue.clone());
        radio.push(0, format!("H{OURS:08X}"));
        let mut b = Bench {
            s: Session::new(config, 0),
            out: Vec::new(),
            now: 0,
            radio,
            read: 0,
            wrote: Vec::new(),
            events: Vec::new(),
        };
        b.settle();
        assert_eq!(b.s.phase(), Phase::Ready, "{:?}", b.events);
        let create = Command::SliceCreate {
            pan: None,
            freq_hz: 14_074_000.0,
            mode: None,
        };
        b.s.send(&mut b.out, &create, b.now).expect("slice create");
        b.settle();
        assert!(b.s.transmit_ready(), "our TX slice, the radio idle");
        b
    }

    /// Answer what the session wrote and deliver what is due, until nothing changes.
    fn settle(&mut self) {
        loop {
            let mut moved = false;
            let end = self
                .out
                .iter()
                .rposition(|b| *b == b'\n')
                .map_or(0, |i| i + 1);
            if end > self.read {
                let text = String::from_utf8_lossy(&self.out[self.read..end]).into_owned();
                self.read = end;
                for line in text.split('\n') {
                    let line = line.trim_start_matches('\u{4}');
                    let Some((seq, command)) =
                        line.strip_prefix('C').and_then(|l| l.split_once('|'))
                    else {
                        continue;
                    };
                    let seq: u32 = seq.parse().expect("a sequence number");
                    self.wrote.push((self.now, command.to_string()));
                    self.radio.answer(self.now, seq, command);
                }
                moved = true;
            }
            for line in self.radio.due(self.now) {
                self.s
                    .on_bytes(&mut self.out, format!("{line}\n").as_bytes(), self.now);
                moved = true;
            }
            self.events.extend(self.s.take_events());
            if !moved {
                return;
            }
        }
    }

    /// Let `ms` pass, a millisecond at a time.
    fn advance(&mut self, ms: u64) {
        for _ in 0..ms {
            self.now += 1;
            self.s.poll(&mut self.out, self.now);
            self.settle();
        }
    }

    /// A line from the radio, now, besides what the rules send.
    fn line(&mut self, line: &str) {
        self.s
            .on_bytes(&mut self.out, format!("{line}\n").as_bytes(), self.now);
        self.settle();
    }

    fn start(&mut self, start: TxStart, lasting_ms: Option<u64>) -> Result<u32, Refusal> {
        let started = self
            .s
            .start_lasting(&mut self.out, start, lasting_ms, self.now);
        self.settle();
        started
    }

    /// rigctld's `T 0`: what it wrote, in order.
    fn end_ours(&mut self) -> Vec<String> {
        let before = self.wrote.len();
        let ended = self.s.end_ours(&mut self.out, self.now);
        self.settle();
        let wrote: Vec<String> = self.wrote[before..]
            .iter()
            .map(|(_, c)| c.clone())
            .collect();
        match ended {
            EndOutcome::Sent(sent) => assert_eq!(sent.len(), wrote.len(), "{wrote:?}"),
            EndOutcome::NotConnected => panic!("closed"),
        }
        wrote
    }

    /// The transmit commands written, starts and stops, in order.
    fn transmit_wire(&self) -> Vec<String> {
        self.wrote
            .iter()
            .map(|(_, c)| c.clone())
            .filter(|c| {
                c.starts_with("xmit ")
                    || c.starts_with("transmit tune ")
                    || c.starts_with("cwx ")
                    || c.starts_with("atu ")
            })
            .collect()
    }

    /// When `command` was first written.
    fn written_at(&self, command: &str) -> Option<u64> {
        self.wrote
            .iter()
            .find(|(_, c)| c == command)
            .map(|(t, _)| *t)
    }

    fn count(&self, event: &Event) -> usize {
        self.events.iter().filter(|e| *e == event).count()
    }

    fn closed_unconfirmed(&self) -> bool {
        self.events.iter().any(|e| {
            matches!(
                e,
                Event::Closed {
                    end: End::UnkeyUnconfirmed,
                    was_keyed: true,
                    ..
                }
            )
        })
    }
}

fn cwx(text: &str) -> TxStart {
    TxStart::CwxSend(CwxText::new(text).unwrap())
}

/// ⭐ rigctld's `T 0` sends the open operation's own stop, and for what only a previous session of
/// ours may hold, every stop its status shows; with nothing of ours, nothing at all.
#[test]
fn t_0_sends_only_the_stops_that_are_ours() {
    let none: Vec<String> = Vec::new();
    // Nothing of ours.
    let mut b = Bench::ready(vec![], vec![PREVIOUS]);
    assert_eq!(b.end_ours(), none);
    // Our over.
    let mut b = Bench::ready(vec![], vec![PREVIOUS]);
    b.start(TxStart::Key, None).expect("keyed");
    b.advance(100);
    assert_eq!(b.end_ours(), ["xmit 0"]);
    // Our tune.
    let mut b = Bench::ready(vec![], vec![PREVIOUS]);
    b.start(TxStart::TuneOn, Some(10_000)).expect("tuning");
    b.advance(100);
    assert_eq!(b.end_ours(), ["transmit tune 0"]);
    // A previous session's tune.
    let mut b = Bench::ready(vec![], vec![PREVIOUS]);
    b.line("S0|transmit tune=1");
    b.line(&keyed_by(PREVIOUS, "TUNE"));
    assert_eq!(b.end_ours(), ["transmit tune 0", "cwx clear", "xmit 0"]);
    // Our CWX word and our ATU cycle: theirs, the ATU's being both (no stop for a cycle is
    // documented).
    let mut b = Bench::ready(vec![], vec![PREVIOUS]);
    b.start(cwx("CQ"), Some(300)).expect("sending");
    b.advance(20);
    assert_eq!(b.end_ours(), ["cwx clear"]);
    let mut b = Bench::ready(vec![], vec![PREVIOUS]);
    b.start(TxStart::AtuStart, None).expect("tuning");
    b.advance(100);
    assert_eq!(b.end_ours(), ["xmit 0", "transmit tune 0"]);
}

/// ⭐ A tune whose carrier stays up after its stop (the simulator's StuckTune) is stopped again,
/// unkeyed as well, the operator told, the session closed: its stop armed the escalation clock as
/// an unkey does.
#[test]
fn a_tune_whose_carrier_stays_up_escalates() {
    let mut b = Bench::ready(vec![Fault::StuckTune], vec![]);
    b.start(TxStart::TuneOn, Some(10_000)).expect("tuning");
    b.advance(100);
    assert_eq!(b.end_ours(), ["transmit tune 0"]);
    let stopped = b.now;
    b.advance(TRANSITION_TIMEOUT_MS - 1);
    assert_eq!(b.count(&Event::UnkeyUnconfirmed), 0);
    assert!(b.s.keyed());
    b.advance(1);
    assert_eq!(
        b.transmit_wire(),
        [
            "transmit tune 1",
            "transmit tune 0",
            "transmit tune 0",
            "xmit 0"
        ]
    );
    assert_eq!(
        b.written_at("xmit 0"),
        Some(stopped + TRANSITION_TIMEOUT_MS)
    );
    assert_eq!(b.count(&Event::UnkeyUnconfirmed), 1);
    assert_eq!(b.s.phase(), Phase::Closed);
    assert!(b.closed_unconfirmed());
}

/// ⭐ With nobody asking, the session ends a latched tune at its hold plus the margin, whether or
/// not the radio loop runs; then the radio's release proves it.
#[test]
fn the_session_ends_a_latched_tune_at_its_deadline_without_the_loop() {
    let mut b = Bench::ready(vec![], vec![]);
    let started = b.now;
    b.start(TxStart::TuneOn, Some(3_000)).expect("tuning");
    b.advance(3_000 + ptt_evidence::TUNE_MARGIN_MS - 1);
    assert_eq!(b.written_at("transmit tune 0"), None);
    b.advance(1);
    assert_eq!(
        b.written_at("transmit tune 0"),
        Some(started + 3_000 + ptt_evidence::TUNE_MARGIN_MS)
    );
    b.advance(1_000);
    assert!(!b.s.keyed(), "the release is the proof");
    assert_eq!(b.count(&Event::UnkeyConfirmed), 1);
    assert_eq!(b.transmit_wire(), ["transmit tune 1", "transmit tune 0"]);
    // No hold given: the ceiling.
    let mut b = Bench::ready(vec![], vec![]);
    let started = b.now;
    b.start(TxStart::TuneOn, Some(10 * ptt_evidence::TUNE_HOLD_MAX_MS))
        .expect("tuning");
    b.advance(ptt_evidence::TUNE_HOLD_MAX_MS + ptt_evidence::TUNE_MARGIN_MS);
    assert_eq!(
        b.written_at("transmit tune 0"),
        Some(started + ptt_evidence::TUNE_HOLD_MAX_MS + ptt_evidence::TUNE_MARGIN_MS)
    );
}

/// ⭐ A second CWX word while our own CWX is open joins it, and a key inside it is refused; the
/// radio's own break-in then ends the operation, with no stop from Nexus.
#[test]
fn a_second_word_appends_and_a_key_inside_our_cwx_is_refused() {
    let mut b = Bench::ready(vec![], vec![]);
    b.start(cwx("CQ"), Some(300)).expect("sending");
    b.advance(50);
    assert!(b.s.keyed());
    b.start(cwx("TEST"), Some(300))
        .expect("a second word joins");
    assert_eq!(b.start(TxStart::Key, None), Err(Refusal::AlreadyKeyed));
    assert_eq!(
        b.start(TxStart::TuneOn, Some(1_000)),
        Err(Refusal::AlreadyKeyed)
    );
    assert_eq!(
        b.transmit_wire(),
        ["cwx send \"CQ\" 1", "cwx send \"TEST\" 2"]
    );
    b.advance(5_000);
    assert!(!b.s.keyed());
    assert_eq!(b.count(&Event::UnkeyConfirmed), 1);
    assert_eq!(
        b.transmit_wire(),
        ["cwx send \"CQ\" 1", "cwx send \"TEST\" 2"],
        "no stop: the radio ended it"
    );
    // A word with no keying time cannot be proven ended.
    assert_eq!(
        b.start(cwx("CQ"), None),
        Err(Refusal::NoReadback(StartKind::Cwx))
    );
    // Once a stop has gone out for our CWX, it takes no more words.
    let mut b = Bench::ready(vec![], vec![]);
    b.start(cwx("CQ"), Some(300)).expect("sending");
    b.advance(20);
    assert_eq!(b.end_ours(), ["cwx clear"]);
    assert_eq!(b.start(cwx("TEST"), Some(300)), Err(Refusal::AlreadyKeyed));
}

/// ⭐ A physical source inside a CWX window fails the attempt: the CWX stops go out at once and
/// the operator is told.
#[test]
fn a_foreign_source_inside_a_cwx_window_fails_the_attempt() {
    for handle in [OURS, 0] {
        let mut b = Bench::ready(vec![], vec![]);
        b.start(cwx("CQ"), Some(300)).expect("sending");
        b.advance(20);
        b.line(&keyed_by(handle, "MIC"));
        assert_eq!(
            b.transmit_wire(),
            ["cwx send \"CQ\" 1", "cwx clear", "xmit 0"],
            "0x{handle:08X}"
        );
        assert_eq!(b.count(&Event::UnkeyUnconfirmed), 1);
        assert!(b.closed_unconfirmed());
    }
    // The control: the same word without the sample ends by the radio's hand.
    let mut b = Bench::ready(vec![], vec![]);
    b.start(cwx("CQ"), Some(300)).expect("sending");
    b.advance(3_000);
    assert_eq!(b.count(&Event::UnkeyConfirmed), 1);
    assert_eq!(b.transmit_wire(), ["cwx send \"CQ\" 1"]);
}

/// ⭐ After a reconnect, a previous session's CWX is ours to clear; another client's never is.
#[test]
fn a_previous_sessions_cwx_is_cleared_and_another_clients_is_not() {
    let mut b = Bench::ready(vec![], vec![PREVIOUS]);
    b.line(&keyed_by(PREVIOUS, ptt_evidence::CWX.source));
    assert_eq!(b.end_ours(), ["cwx clear", "xmit 0"]);
    assert!(matches!(
        b.s.stop(&mut b.out, TxStop::CwxClear, b.now),
        StopOutcome::Sent { .. }
    ));
    let foreign = Foreign::default().handle;
    let mut b = Bench::ready(vec![], vec![PREVIOUS]);
    b.line(&keyed_by(foreign, ptt_evidence::CWX.source));
    assert_eq!(b.end_ours(), Vec::<String>::new());
    assert_eq!(
        b.s.stop(&mut b.out, TxStop::CwxClear, b.now),
        StopOutcome::NothingOfOurs
    );
}

/// The session's own stops follow the kind too: closing, or a missed ping, while our tune is up
/// sends `transmit tune 0`, not an unkey the tune may not obey.
#[test]
fn closing_or_a_missed_ping_ends_a_tune_with_its_own_stop() {
    let mut b = Bench::ready(vec![], vec![]);
    b.start(TxStart::TuneOn, Some(10_000)).expect("tuning");
    b.advance(100);
    b.s.close(&mut b.out, b.now);
    b.settle();
    assert_eq!(b.transmit_wire(), ["transmit tune 1", "transmit tune 0"]);

    let mut b = Bench::ready(
        vec![Fault::DropPings {
            first: 1,
            count: 100,
        }],
        vec![],
    );
    b.start(TxStart::TuneOn, Some(10_000)).expect("tuning");
    b.advance(2_100);
    assert!(b
        .events
        .iter()
        .any(|e| matches!(e, Event::UnkeyedOnMissedPing { .. })));
    assert_eq!(
        b.transmit_wire().first().map(String::as_str),
        Some("transmit tune 1")
    );
    assert_eq!(b.transmit_wire()[1], "transmit tune 0");
}
