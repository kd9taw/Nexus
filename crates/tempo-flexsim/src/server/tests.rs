//! The simulator against raw clients. Each fault test runs its control, the same exchange without
//! the fault, so a test that passes cannot be passing because the fault never happened. Waits are
//! on events (a line, a reply, the log), never on a fixed sleep, except where a test shows that
//! something keeps not happening.

use super::*;
use crate::fault::Foreign;
use crate::line::{parse_reply, LineBuf};
use crate::vita::{Content, Start, Stream};
use std::collections::VecDeque;
use std::io::ErrorKind;
use tempo_net::flex::vita::{decode_fft, FftAssembler};
use tempo_net::flexvita::{self, VitaGap, VitaSequence};

/// The longest any single wait may take before a test fails.
const WAIT: Duration = Duration::from_secs(10);

/// A read that rides out EINTR, which a stopped and continued test process gets on a socket with
/// a read timeout ([`line::read_again`]). Each retry restarts the socket's timeout, so the retries
/// have a deadline of their own: a test whose data never comes fails after [`WAIT`] rather than
/// hanging. A timeout still fails it.
fn read_some(stream: &mut TcpStream, buf: &mut [u8]) -> io::Result<usize> {
    let deadline = Instant::now() + WAIT;
    loop {
        match stream.read(buf) {
            Err(e) if e.kind() == ErrorKind::Interrupted && Instant::now() < deadline => {}
            other => return other,
        }
    }
}

/// [`read_some`] for a datagram.
fn recv_some(udp: &UdpSocket, buf: &mut [u8]) -> io::Result<usize> {
    let deadline = Instant::now() + WAIT;
    loop {
        match udp.recv_from(buf) {
            Err(e) if e.kind() == ErrorKind::Interrupted && Instant::now() < deadline => {}
            other => return other.map(|(n, _)| n),
        }
    }
}

/// A minimal client: commands out, whole lines in.
struct Client {
    reader: TcpStream,
    writer: TcpStream,
    lines: LineBuf,
    queue: VecDeque<String>,
    seq: u32,
}

impl Client {
    fn connect(sim: &Simulator) -> Client {
        let reader = TcpStream::connect(sim.tcp_addr()).unwrap();
        reader.set_read_timeout(Some(WAIT)).unwrap();
        let writer = reader.try_clone().unwrap();
        Client {
            reader,
            writer,
            lines: LineBuf::default(),
            queue: VecDeque::new(),
            seq: 0,
        }
    }

    /// Connect and read the prologue; returns the handle line.
    fn greeted(sim: &Simulator) -> (Client, String) {
        let mut c = Client::connect(sim);
        assert_eq!(c.line(), "V1.4.0.0");
        let handle = c.line();
        (c, handle)
    }

    fn raw(&mut self, text: &str) {
        self.writer.write_all(text.as_bytes()).unwrap();
    }

    fn send(&mut self, command: &str) -> u32 {
        self.seq += 1;
        self.raw(&line::command_line(self.seq, command));
        self.seq
    }

    fn line(&mut self) -> String {
        loop {
            if let Some(l) = self.queue.pop_front() {
                return l;
            }
            let mut buf = [0u8; 4096];
            let n = read_some(&mut self.reader, &mut buf).expect("a line within the wait");
            assert!(n > 0, "the simulator closed the connection");
            self.queue.extend(self.lines.push(&buf[..n]).unwrap());
        }
    }

    /// Lines up to and including the first that satisfies `done`.
    fn until(&mut self, done: impl Fn(&str) -> bool) -> Vec<String> {
        let mut got = Vec::new();
        loop {
            let l = self.line();
            let stop = done(&l);
            got.push(l);
            if stop {
                return got;
            }
        }
    }

    fn through_reply(&mut self, seq: u32) -> Vec<String> {
        let tag = format!("R{seq}|");
        self.until(|l| l.starts_with(&tag))
    }

    /// Send a command and read through its reply.
    fn ask(&mut self, command: &str) -> Vec<String> {
        let seq = self.send(command);
        self.through_reply(seq)
    }

    /// A command's statuses: what arrives after its reply and before the reply to a ping sent
    /// after it. For rules that do not wait.
    fn statuses(&mut self, command: &str) -> Vec<String> {
        self.ask(command);
        let mut after = self.ask("ping");
        after.pop();
        after
    }

    /// Read until the simulator closes the connection; `false` if it is still open after the wait.
    fn closed_by_peer(&mut self) -> bool {
        let mut buf = [0u8; 4096];
        loop {
            match read_some(&mut self.reader, &mut buf) {
                Ok(0) => return true,
                Ok(_) => {}
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    return false
                }
                Err(_) => return true,
            }
        }
    }
}

/// A small session with a ping rule and the given blocks.
fn session(body: &str) -> Session {
    Session::parse(&format!(
        "flexsession 1\nprologue V1.4.0.0\nhandle 2B6E1F40\non ping\nreply 0\n{body}"
    ))
    .unwrap()
}

fn start(session: Session, faults: Vec<Fault>) -> Simulator {
    Simulator::start(
        session,
        Config {
            faults,
            ..Config::default()
        },
    )
    .unwrap()
}

/// Take a client of the bundled v4 session to TRANSMITTING.
fn keyed(c: &mut Client) {
    c.ask("sub tx all");
    c.ask("slice create pan=0x40000000 freq=14.074000 mode=DIGU");
    c.send("xmit 1");
    c.until(|l| l.contains("state=TRANSMITTING"));
}

fn closed(sim: &Simulator, conn: usize, by: Closer) -> bool {
    sim.wait_for(WAIT, |log| {
        log.iter().any(|l| l.event == Event::Closed { conn, by })
    })
}

#[test]
fn the_prologue_then_replies_carry_the_clients_own_sequence_numbers() {
    let sim = start(Session::v4_gui_client(), Vec::new());
    let (mut c, handle) = Client::greeted(&sim);
    assert_eq!(handle, "H2B6E1F40");
    c.raw("C7|info\n");
    let r = parse_reply(&c.line()).unwrap();
    assert_eq!((r.seq, r.code.as_str()), (7, "0"));
    assert!(
        r.message.starts_with("model=\"FLEX-6400\""),
        "{}",
        r.message
    );
    c.raw("C3|frobnicate\n");
    assert_eq!(
        c.line(),
        "R3|50000015|",
        "an unmatched command gets the default code"
    );
    c.raw("garbage\nCD12|ping\n");
    assert_eq!(c.line(), "R12|0|", "a malformed line is not answered");
    let events = sim.events();
    let malformed = events
        .iter()
        .position(|e| matches!(e, Event::Malformed { line, .. } if line == "garbage"))
        .expect("the malformed line is logged");
    let ping = events
        .iter()
        .position(|e| matches!(e, Event::Command { seq: 12, .. }))
        .unwrap();
    assert!(malformed < ping);
    // The next connection gets the next handle.
    let (_d, handle) = Client::greeted(&sim);
    assert_eq!(handle, "H2B6E1F41");
}

#[test]
fn a_waiting_rule_holds_back_later_statuses_but_not_later_replies() {
    let sim = start(
        session("on a\nreply 0\nsend S0|a1\nwait 1000\nsend S0|a2\non b\nreply 0\nsend S0|b1\n"),
        Vec::new(),
    );
    let (mut c, _) = Client::greeted(&sim);
    c.send("a");
    assert_eq!(c.line(), "R1|0|");
    assert_eq!(c.line(), "S0|a1");
    c.send("b");
    assert_eq!(c.line(), "R2|0|", "a later reply is not held back");
    assert_eq!(c.line(), "S0|a2");
    assert_eq!(
        c.line(),
        "S0|b1",
        "a later status waits for the earlier rule's"
    );
}

#[test]
fn a_timed_block_is_sent_at_its_offset() {
    let sim = start(session("at 150\nsend S0|late\n"), Vec::new());
    let (mut c, _) = Client::greeted(&sim);
    assert_eq!(c.line(), "S0|late");
    // A line is logged as sent once its write has returned, which can be after the client has
    // already read it.
    let late = |e: &Event| matches!(e, Event::Sent { line, .. } if line == "S0|late");
    assert!(sim.wait_for(WAIT, |log| log.iter().any(|l| late(&l.event))));
    let log = sim.log();
    let at = |want: &dyn Fn(&Event) -> bool| log.iter().find(|l| want(&l.event)).unwrap().at;
    let connected = at(&|e| matches!(e, Event::Connected { .. }));
    assert!(at(&late) - connected >= Duration::from_millis(150));
}

const SLICE: &str = "S0|slice 0 RF_frequency=14.074000 mode=DIGU";

fn split_sim(max_hold: Duration) -> (Simulator, usize) {
    let cut = SLICE.find("14.07").unwrap() + "14.07".len();
    let sim = start(
        session(&format!("on sub slice all\nreply 0\nsend {SLICE}\n")),
        vec![Fault::SplitLine {
            containing: "RF_frequency".into(),
            cuts: vec![cut],
            max_hold,
        }],
    );
    (sim, cut)
}

#[test]
fn a_split_line_arrives_in_pieces_and_the_rest_waits_for_the_next_command() {
    let (sim, cut) = split_sim(WAIT);
    let (mut c, _) = Client::greeted(&sim);
    c.send("sub slice all");
    // The reply, then the first piece, and nothing more until the client speaks again.
    let want = format!("R1|0|\n{}", &SLICE[..cut]);
    let mut raw = Vec::new();
    while raw.len() < want.len() {
        let mut buf = [0u8; 256];
        let n = read_some(&mut c.reader, &mut buf).unwrap();
        assert!(n > 0);
        raw.extend_from_slice(&buf[..n]);
    }
    let text = String::from_utf8(raw).unwrap();
    assert_eq!(text, want);

    // The bug the fault exists to catch: taking what was read as whole lines reads a frequency
    // the radio never sent.
    let naive = text
        .lines()
        .last()
        .and_then(|l| l.split(' ').find_map(|kv| kv.strip_prefix("RF_frequency=")));
    assert_eq!(naive, Some("14.07"));
    // A line assembler holds the piece instead.
    let mut assembler = LineBuf::default();
    assert_eq!(assembler.push(text.as_bytes()).unwrap(), vec!["R1|0|"]);
    assert!(assembler.has_partial());

    c.send("ping");
    let mut rest = Vec::new();
    while !rest.iter().any(|l: &String| l.starts_with("R2|")) {
        let mut buf = [0u8; 256];
        let n = read_some(&mut c.reader, &mut buf).unwrap();
        assert!(n > 0);
        rest.extend(assembler.push(&buf[..n]).unwrap());
    }
    assert_eq!(rest, vec![SLICE, "R2|0|"]);
    let events = sim.events();
    let pieces = vec![SLICE[..cut].to_string(), format!("{}\n", &SLICE[cut..])];
    assert!(events.contains(&Event::Split { conn: 0, pieces }));
    // The rest was released by the client's command, not by luck of timing: the line counts as
    // sent only after the ping arrived. Read boundaries alone could not show this.
    let ping = events
        .iter()
        .position(|e| matches!(e, Event::Command { seq: 2, .. }))
        .unwrap();
    let sent = events
        .iter()
        .position(|e| matches!(e, Event::Sent { line, .. } if line == SLICE))
        .unwrap();
    assert!(
        ping < sent,
        "the rest of the line went out before the client spoke"
    );
}

#[test]
fn a_split_line_is_completed_after_max_hold_when_the_client_stays_silent() {
    let (sim, _) = split_sim(Duration::from_millis(100));
    let (mut c, _) = Client::greeted(&sim);
    c.send("sub slice all");
    assert_eq!(
        c.until(|l| l.contains("RF_frequency")),
        vec!["R1|0|", SLICE]
    );
}

#[test]
fn a_reordered_reply_is_told_apart_only_by_its_sequence_number() {
    for reorder in [false, true] {
        let faults = if reorder {
            vec![Fault::ReorderReply {
                command: "info".into(),
            }]
        } else {
            Vec::new()
        };
        let sim = start(Session::v4_gui_client(), faults);
        let (mut c, _) = Client::greeted(&sim);
        let info = c.send("info");
        let mics = c.send("mic list");
        let first = parse_reply(&c.line()).unwrap();
        let second = parse_reply(&c.line()).unwrap();
        // By arrival order, the first reply would be taken as info's.
        assert_eq!(first.seq == info, !reorder);
        assert_eq!(
            [first.seq, second.seq],
            if reorder { [mics, info] } else { [info, mics] }
        );
        let by_seq = |seq: u32| {
            [&first, &second]
                .into_iter()
                .find(|r| r.seq == seq)
                .unwrap()
        };
        assert!(by_seq(info).message.starts_with("model="));
        assert_eq!(by_seq(mics).message, "MIC,BAL,LINE,ACC");
        assert_eq!(
            sim.events()
                .contains(&Event::ReplyHeld { conn: 0, seq: info }),
            reorder
        );
    }
}

#[test]
fn dropped_pings_get_no_reply() {
    let sim = start(
        Session::v4_gui_client(),
        vec![Fault::DropPings { first: 2, count: 2 }],
    );
    let (mut c, _) = Client::greeted(&sim);
    for _ in 0..5 {
        c.send("ping");
    }
    let last = c.send("slice list");
    let answered: Vec<u32> = c
        .through_reply(last)
        .iter()
        .filter_map(|l| parse_reply(l))
        .map(|r| r.seq)
        .collect();
    assert_eq!(answered, [1, 4, 5, 6]);
    let dropped: Vec<u32> = sim
        .events()
        .iter()
        .filter_map(|e| match e {
            Event::PingDropped { seq, .. } => Some(*seq),
            _ => None,
        })
        .collect();
    assert_eq!(dropped, [2, 3]);
}

#[test]
fn the_radio_keepalive_closes_a_session_whose_pings_stop_arriving() {
    // A ping goes out every 40 ms or so, twenty times inside the timeout, so a stalled test
    // thread cannot pass for silence.
    let timeout = Duration::from_millis(800);
    let config = |faults| Config {
        faults,
        keepalive_timeout: timeout,
        ..Config::default()
    };
    // Control: pings that arrive hold the session open past the timeout.
    let sim = Simulator::start(Session::v4_gui_client(), config(Vec::new())).unwrap();
    let (mut c, _) = Client::greeted(&sim);
    c.ask("keepalive enable");
    let until = Instant::now() + timeout * 2;
    while Instant::now() < until {
        c.ask("ping");
        thread::sleep(Duration::from_millis(40));
    }
    assert!(
        !sim.events()
            .iter()
            .any(|e| matches!(e, Event::Closed { .. })),
        "closed while pinging"
    );
    // Silence closes it.
    assert!(c.closed_by_peer());
    assert!(closed(&sim, 0, Closer::Keepalive));

    // Lost pings are silence to the radio, however often the client sends them.
    let lossy = Simulator::start(
        Session::v4_gui_client(),
        config(vec![Fault::DropPings {
            first: 1,
            count: 1_000_000,
        }]),
    )
    .unwrap();
    let (mut d, _) = Client::greeted(&lossy);
    d.ask("keepalive enable");
    d.reader
        .set_read_timeout(Some(Duration::from_millis(40)))
        .unwrap();
    let deadline = Instant::now() + WAIT;
    while !d.closed_by_peer() {
        assert!(
            Instant::now() < deadline,
            "the keepalive never closed the session"
        );
        d.seq += 1;
        // The session may close between the read and this write: a failed write is fine.
        let _ = d
            .writer
            .write_all(line::command_line(d.seq, "ping").as_bytes());
    }
    assert!(closed(&lossy, 0, Closer::Keepalive));
}

#[test]
fn a_stuck_transmitter_acknowledges_xmit_0_and_stays_keyed_across_a_reconnect() {
    for stuck in [false, true] {
        let faults = if stuck {
            vec![Fault::StuckTransmit]
        } else {
            Vec::new()
        };
        let sim = start(Session::v4_gui_client(), faults);
        let (mut c, _) = Client::greeted(&sim);
        keyed(&mut c);
        let off = c.send("xmit 0");
        let ping = c.send("ping");
        let lines = c.through_reply(ping);
        let reply = lines
            .iter()
            .position(|l| *l == format!("R{off}|0|"))
            .expect("xmit 0 is acknowledged either way");
        let interlock: Vec<&String> = lines[reply..]
            .iter()
            .filter(|l| l.contains("|interlock tx_client_handle="))
            .collect();
        if stuck {
            assert!(interlock.is_empty(), "{interlock:?}");
            assert!(sim.events().contains(&Event::StatusWithheld {
                conn: 0,
                command: "xmit 0".into(),
                lines: 3,
            }));
        } else {
            // The control, and the order the port plan's readback needs (§3.4).
            let mut seen: Vec<String> = interlock.iter().map(|l| l.to_string()).collect();
            seen.extend(c.until(|l| l.contains("tx_client_handle=0x00000000 state=READY")));
            let owners: Vec<&str> = seen
                .iter()
                .filter(|l| l.contains("|interlock tx_client_handle="))
                .map(|l| {
                    l.split_once(" state=")
                        .unwrap()
                        .0
                        .rsplit_once('=')
                        .unwrap()
                        .1
                })
                .collect();
            assert_eq!(owners, ["0x2B6E1F40", "0x2B6E1F40", "0x00000000"]);
            assert!(seen[0].contains("state=UNKEY_REQUESTED"));
            assert!(seen[1].contains("state=READY"));
        }
        drop(c);
        assert!(closed(&sim, 0, Closer::Client));
        let (mut d, handle) = Client::greeted(&sim);
        assert_eq!(handle, "H2B6E1F41");
        let tx = d.statuses("sub tx all");
        let last = tx
            .iter()
            .rev()
            .find(|l| l.contains("|interlock tx_client_handle="))
            .unwrap();
        assert_eq!(
            last.contains("tx_client_handle=0x2B6E1F40 state=TRANSMITTING"),
            stuck,
            "{last}"
        );
    }
}

/// The tune carrier the same way: `transmit tune 0` acknowledged either way, its statuses (the
/// transmit status's `tune=0` among them) withheld under the fault, and a later connection told
/// the old handle still holds a tune.
#[test]
fn a_stuck_tune_acknowledges_tune_off_and_stays_up_across_a_reconnect() {
    for stuck in [false, true] {
        let faults = if stuck {
            vec![Fault::StuckTune]
        } else {
            Vec::new()
        };
        let sim = start(Session::v4_gui_client(), faults);
        let (mut c, _) = Client::greeted(&sim);
        c.ask("sub tx all");
        c.ask("slice create pan=0x40000000 freq=14.074000 mode=DIGU");
        c.send("transmit tune 1");
        let up = c.until(|l| l.contains("state=TRANSMITTING"));
        assert!(up.iter().any(|l| l == "S0|transmit tune=1"), "{up:?}");
        assert!(
            up.last().unwrap().contains("source=TUNE"),
            "the tune profile's source: {up:?}"
        );
        let off = c.send("transmit tune 0");
        let ping = c.send("ping");
        let lines = c.through_reply(ping);
        let reply = lines
            .iter()
            .position(|l| *l == format!("R{off}|0|"))
            .expect("transmit tune 0 is acknowledged either way");
        let after: Vec<&String> = lines[reply..].iter().collect();
        if stuck {
            assert!(
                !after
                    .iter()
                    .any(|l| l.contains("tune=0") || l.contains("|interlock ")),
                "{after:?}"
            );
            assert!(sim.events().contains(&Event::StatusWithheld {
                conn: 0,
                command: "transmit tune 0".into(),
                lines: 4,
            }));
        } else {
            let mut seen: Vec<String> = after.iter().map(|l| l.to_string()).collect();
            seen.extend(c.until(|l| l.contains("tx_client_handle=0x00000000 state=READY")));
            assert_eq!(
                seen.iter().filter(|l| *l == "S0|transmit tune=0").count(),
                1
            );
        }
        drop(c);
        assert!(closed(&sim, 0, Closer::Client));
        let (mut d, handle) = Client::greeted(&sim);
        assert_eq!(handle, "H2B6E1F41");
        let tx = d.statuses("sub tx all");
        assert_eq!(
            tx.iter().any(|l| l == "S0|transmit tune=1"),
            stuck,
            "{tx:?}"
        );
        let last = tx
            .iter()
            .rev()
            .find(|l| l.contains("|interlock tx_client_handle="))
            .unwrap();
        assert_eq!(
            last.contains("tx_client_handle=0x2B6E1F40 state=TRANSMITTING")
                && last.contains("source=TUNE"),
            stuck,
            "{last}"
        );
    }
}

/// The two faults that change a rule's statuses, each against its control: a reason on every
/// keying report (the PGXL profile), and a radio that holds transmit after a CWX send until
/// `xmit 0`.
#[test]
fn a_held_cwx_and_a_keying_reason_reach_the_wire_as_their_faults_say() {
    // A reason on every keying report, and its control.
    for reason in [None, Some(crate::fault::PGXL)] {
        let faults = reason
            .map(|r| Fault::KeyingReason { reason: r.into() })
            .into_iter()
            .collect();
        let sim = start(Session::v4_gui_client(), faults);
        let (mut c, _) = Client::greeted(&sim);
        c.ask("sub tx all");
        c.ask("slice create pan=0x40000000 freq=14.074000 mode=DIGU");
        c.send("xmit 1");
        let keyed = c.until(|l| l.contains("state=TRANSMITTING"));
        let requested = keyed
            .iter()
            .find(|l| l.contains("state=PTT_REQUESTED"))
            .unwrap();
        let want = format!(" reason={} source=SW ", reason.unwrap_or(""));
        assert!(requested.contains(&want), "{requested}");
        assert_eq!(
            keyed
                .last()
                .unwrap()
                .ends_with(" amplifier=0x5A0F0001,0x5A0F0002"),
            reason.is_some(),
            "{keyed:?}"
        );
    }
    // A radio that holds transmit after a CWX send, and its control.
    for held in [false, true] {
        let faults = if held {
            vec![Fault::HoldsCwx]
        } else {
            Vec::new()
        };
        let sim = start(Session::v4_gui_client(), faults);
        let (mut c, _) = Client::greeted(&sim);
        c.ask("sub tx all");
        c.ask("slice create pan=0x40000000 freq=14.074000 mode=DIGU");
        c.send("cwx send \"CQ\" 1");
        c.until(|l| l.contains("state=TRANSMITTING"));
        c.ask("cwx clear");
        // The free radio lets go 600 ms after it keys; this one keeps not doing so.
        std::thread::sleep(Duration::from_millis(1_000));
        let lines = c.ask("ping");
        assert_eq!(
            lines.iter().any(|l| l.contains("state=UNKEY_REQUESTED")),
            !held,
            "{lines:?}"
        );
        if held {
            c.send("xmit 0");
            let released = c.until(|l| l.contains("tx_client_handle=0x00000000 state=READY"));
            assert!(
                released.iter().any(|l| l.contains("state=UNKEY_REQUESTED")),
                "{released:?}"
            );
        }
    }
}

#[test]
fn a_foreign_client_owns_a_slice_a_pan_and_the_transmitter() {
    let foreign = Foreign {
        transmitting: true,
        ..Foreign::default()
    };
    let sim = start(
        Session::v4_gui_client(),
        vec![Fault::ForeignClient(foreign.clone())],
    );
    let (mut c, _) = Client::greeted(&sim);
    let clients = c.statuses("sub client all");
    assert!(clients
        .iter()
        .any(|l| l.starts_with("S7A3C0001|client 0x7A3C0001 connected")));
    let slices = c.statuses("sub slice all");
    assert_eq!(slices.len(), 1, "{slices:?}");
    assert!(slices[0].contains(" tx=1 client_handle=0x7A3C0001 pan=0x40000001 "));
    let pans = c.statuses("sub pan all");
    assert!(pans[0].starts_with("S7A3C0001|display pan 0x40000001 client_handle=0x7A3C0001 "));
    assert!(pans[1].starts_with("S7A3C0001|display waterfall 0x42000001 "));
    let tx = c.statuses("sub tx all");
    assert!(tx
        .last()
        .unwrap()
        .contains("tx_client_handle=0x7A3C0001 state=TRANSMITTING"));
    // One TX slice per radio: ours is created without the flag.
    let ours = c.statuses("slice create pan=0x40000000 freq=14.074000 mode=DIGU");
    assert!(
        ours[0].contains(" tx=0 client_handle=0x2B6E1F40 "),
        "{}",
        ours[0]
    );

    // The evidence a guard's test reads, and its control: a command that does name the foreign
    // pan is seen.
    c.ask("display pan set 0x40000000 xpixels=1024 ypixels=480");
    assert!(!sim.commands().iter().any(|cmd| foreign.is_named_by(cmd)));
    c.ask("display pan set 0x40000001 center=7.100000");
    assert!(sim.commands().iter().any(|cmd| foreign.is_named_by(cmd)));
}

#[test]
fn a_disconnect_mid_over_closes_the_session_while_transmitting() {
    for stays in [false, true] {
        let sim = start(
            Session::v4_gui_client(),
            vec![Fault::DisconnectMidOver {
                after: Duration::from_millis(50),
                radio_stays_keyed: stays,
            }],
        );
        let (mut c, _) = Client::greeted(&sim);
        keyed(&mut c);
        assert!(c.closed_by_peer(), "the session outlived the over");
        assert!(closed(&sim, 0, Closer::Simulator));

        let (mut d, _) = Client::greeted(&sim);
        let tx = d.statuses("sub tx all");
        assert_eq!(
            tx.last()
                .unwrap()
                .contains("tx_client_handle=0x2B6E1F40 state=TRANSMITTING"),
            stays
        );
        // It fires once: an over on the next session runs on.
        keyed(&mut d);
        thread::sleep(Duration::from_millis(200));
        d.ask("ping");
        let cut = sim
            .events()
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Event::Closed {
                        by: Closer::Simulator,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(cut, 1);
    }
}

fn meters(ticks: usize) -> Stream {
    Stream {
        stream_id: vita::METER_STREAM_ID,
        content: Content::Meters(vec![(1, -9000)]),
        period: Duration::from_millis(2),
        ticks: Some(ticks),
        start: Start::Registered,
        until: None,
    }
}

/// Register a UDP socket with the one-byte datagram, as the API does.
fn register(sim: &Simulator) -> UdpSocket {
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    udp.set_read_timeout(Some(WAIT)).unwrap();
    udp.send_to(&[0], sim.udp_addr()).unwrap();
    udp
}

fn receive(udp: &UdpSocket, n: usize) -> Vec<Vec<u8>> {
    let mut buf = [0u8; 2048];
    (0..n)
        .map(|_| {
            let len = recv_some(udp, &mut buf).expect("a packet within the wait");
            buf[..len].to_vec()
        })
        .collect()
}

#[test]
fn lost_and_reordered_vita_packets_trip_the_shipped_continuity_guard() {
    let verdicts = |faults: Vec<Fault>, arriving: usize| -> Vec<VitaGap> {
        let sim = Simulator::start(
            session(""),
            Config {
                faults,
                streams: vec![meters(12)],
                ..Config::default()
            },
        )
        .unwrap();
        let (_c, _) = Client::greeted(&sim);
        let udp = register(&sim);
        let mut sequence = VitaSequence::default();
        receive(&udp, arriving)
            .iter()
            .map(|p| sequence.observe(flexvita::parse_vita(p).unwrap().packet_count))
            .collect()
    };
    // Control: a clean stream is in sequence throughout.
    assert_eq!(verdicts(Vec::new(), 12), vec![VitaGap::InSequence; 12]);
    // Packet 3 lost, packets 7 and 8 swapped: counts 0 1 2 4 5 6 8 7 9 10 11.
    let faulted = verdicts(
        vec![Fault::Vita {
            stream_id: vita::METER_STREAM_ID,
            drop: vec![3],
            swap: vec![7],
        }],
        11,
    );
    use VitaGap::{InSequence as In, Lost, Stale};
    assert_eq!(
        faulted,
        vec![In, In, In, Lost(1), In, In, Lost(1), Stale, In, In, In]
    );
}

#[test]
fn a_lost_fft_fragment_costs_its_own_frame_and_no_other() {
    let frames = |faults: Vec<Fault>, arriving: usize| -> Vec<(u32, Vec<u16>)> {
        let sim = Simulator::start(
            session(""),
            Config {
                faults,
                streams: vec![Stream {
                    stream_id: 0x4000_0000,
                    content: Content::Fft {
                        total_bins: 8,
                        per_packet: 4,
                        floor_row: 300,
                        peak: Some((2, 10)),
                    },
                    period: Duration::from_millis(2),
                    ticks: Some(3),
                    start: Start::Registered,
                    until: None,
                }],
                ..Config::default()
            },
        )
        .unwrap();
        let (_c, _) = Client::greeted(&sim);
        let udp = register(&sim);
        let mut assembler = FftAssembler::new();
        receive(&udp, arriving)
            .iter()
            .filter_map(|p| {
                let packet = flexvita::parse_vita(p).unwrap();
                let fragment = decode_fft(packet.payload, packet.has_trailer)?;
                assembler
                    .push(&fragment)
                    .map(|frame| (frame.frame_index, frame.rows))
            })
            .collect()
    };
    let row = vec![300, 300, 10, 300, 300, 300, 300, 300];
    let complete = frames(Vec::new(), 6);
    assert_eq!(
        complete,
        vec![(0, row.clone()), (1, row.clone()), (2, row.clone())]
    );
    // Frame 1's second fragment (packet 3) is lost: frame 1 never appears, frames 0 and 2 do.
    let lossy = frames(
        vec![Fault::Vita {
            stream_id: 0x4000_0000,
            drop: vec![3],
            swap: Vec::new(),
        }],
        5,
    );
    assert_eq!(lossy, vec![(0, row.clone()), (2, row)]);
}

#[test]
fn client_udpport_sets_the_destination_and_client_datagrams_are_logged() {
    let sim = Simulator::start(
        Session::v4_gui_client(),
        Config {
            streams: vec![meters(3)],
            ..Config::default()
        },
    )
    .unwrap();
    let (mut c, _) = Client::greeted(&sim);
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    udp.set_read_timeout(Some(WAIT)).unwrap();
    c.ask(&format!(
        "client udpport {}",
        udp.local_addr().unwrap().port()
    ));
    for packet in receive(&udp, 3) {
        assert_eq!(vita::header(&packet).unwrap().class, vita::class::METER);
    }
    // A client's own datagram, here a DAX TX packet, is logged as it arrived.
    let samples = [0.25f32; vita::DAX_FRAMES];
    let dax = vita::packet(
        vita::TYPE_IF_DATA,
        0x8400_0000,
        vita::class::AUDIO_F32_STEREO,
        5,
        &vita::audio_payload(vita::class::AUDIO_F32_STEREO, &samples),
    );
    udp.send_to(&dax, sim.udp_addr()).unwrap();
    assert!(sim.wait_for(WAIT, |log| log
        .iter()
        .any(|l| matches!(&l.event, Event::UdpIn { bytes, .. } if *bytes == dax))));
}

#[test]
fn a_stream_starts_after_its_command_is_answered_and_stops_on_its_until() {
    const DAX: u32 = 0x0400_0001;
    let sim = Simulator::start(
        Session::v4_gui_client(),
        Config {
            streams: vec![Stream {
                stream_id: DAX,
                content: Content::DaxAudio {
                    class: vita::class::AUDIO_F32_STEREO,
                    tone_hz: 1000.0,
                    amplitude: 0.5,
                },
                period: Stream::dax_period(),
                ticks: None,
                start: Start::After("stream create type=dax_rx".into()),
                until: Some("stream remove 0x04000001".into()),
            }],
            ..Config::default()
        },
    )
    .unwrap();
    let (mut c, _) = Client::greeted(&sim);
    let udp = register(&sim);
    let create = c.send("stream create type=dax_rx dax_channel=1");
    let lines = c.through_reply(create);
    assert_eq!(lines.last().unwrap(), &format!("R{create}|0|0x04000001"));
    for packet in receive(&udp, 5) {
        let h = vita::header(&packet).unwrap();
        assert_eq!((h.stream_id, h.class), (DAX, vita::class::AUDIO_F32_STEREO));
    }
    let events = sim.events();
    let answered = events
        .iter()
        .position(
            |e| matches!(e, Event::Sent { line, .. } if line.starts_with(&format!("R{create}|"))),
        )
        .unwrap();
    let first_packet = events
        .iter()
        .position(|e| matches!(e, Event::VitaOut { stream_id: DAX, .. }))
        .unwrap();
    assert!(
        answered < first_packet,
        "audio flowed before the stream was created"
    );

    let remove = c.send("stream remove 0x04000001");
    c.through_reply(remove);
    let ended = Event::StreamEnded {
        conn: 0,
        stream_id: DAX,
    };
    assert!(sim.wait_for(WAIT, |log| log.iter().any(|l| l.event == ended)));
    let events = sim.events();
    let removed = events
        .iter()
        .position(|e| matches!(e, Event::Command { seq, .. } if *seq == remove))
        .unwrap();
    let end = events.iter().position(|e| *e == ended).unwrap();
    assert!(removed < end, "the stream ended before it was removed");
    assert!(
        !events[end..]
            .iter()
            .any(|e| matches!(e, Event::VitaOut { stream_id: DAX, .. })),
        "audio flowed after the stream ended"
    );
}
