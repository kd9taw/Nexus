//! The recorder: what a tester runs to capture a session from their radio for a report.
//!
//! **Observe-only, by construction.** The recorder connects to a real radio. It subscribes to
//! status, asks three read-only questions, registers a UDP port for the meter stream and pings once
//! a second. It can send nothing else:
//!
//! - its only write path is [`Wire::send`], which takes an [`Observe`], a closed set of commands
//!   that has no way to say `xmit`, tune, ATU, CW or CWX, DAX TX, or any slice, transmit or other
//!   setting;
//! - before writing, [`Wire::send`] checks the rendered text against [`ALLOWED_VERBS`] again
//!   ([`admit`]), and refuses anything else;
//! - a test pins [`ALLOWED_VERBS`] word for word, so adding a verb fails it, and another runs the
//!   recorder against the simulator and checks every command that reached the wire.
//!
//! It never registers as a GUI client (`client gui`), so the radio gives it no slices or pans and
//! other clients are not disturbed. Its UDP socket sends one byte, the registration datagram
//! (port plan §4.2), and otherwise only receives.
//!
//! **Nothing identifying is written.** Every line passes through [`Scrubber`] before it reaches the
//! writer: IP and MAC addresses, serial numbers, client ids, host and station names and GPS
//! position are replaced (see [`crate::scrub`]). The prologue's version line is written as it is,
//! because it is a version, not an address. Only meter, FFT and waterfall packets are kept from
//! UDP; audio and anything else is counted and dropped. The output goes to the writer the caller
//! gives, a local file for the `flexrecord` binary, and nowhere else: the tester reads it and
//! chooses whether to attach it.
//!
//! **The output is a session** ([`crate::session`]) the simulator replays. Each command the
//! recorder sent becomes a rule holding the radio's reply and every line that arrived before the
//! next command. Lines that arrive while the recorder only pings become timed blocks, as do the
//! UDP packets. The recorder's own handle becomes `{h}`, so a replay answers whichever connection
//! asks.

use std::io::{self, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use crate::line::{self, LineBuf, Reply};
use crate::scrub::{Kind, Scrubber};
use crate::session::{hex_string, FORMAT_LINE};
use crate::vita;

/// The radio's TCP API port, which is also the UDP port its registration datagram goes to (D,
/// SmartSDR-TCPIP-API; port plan §4.2).
pub const API_PORT: u16 = 4992;

/// Every command head the recorder can put on the wire. A command is admitted only when its text
/// is one of these or starts with one followed by a space.
///
/// ⚠️ A transmit-safety list. Each entry must be unable to transmit or change the radio's state:
/// `sub` subscribes to status, `ping` keeps the session alive, `info`, `slice list` and
/// `meter list` are queries, and `client udpport` names where this client's own UDP data goes.
/// The test `the_allow_list_is_pinned` holds it word for word.
pub const ALLOWED_VERBS: [&str; 6] = [
    "sub",
    "ping",
    "info",
    "slice list",
    "meter list",
    "client udpport",
];

/// A status topic the recorder subscribes to (`sub <topic> all`). The list is the port plan's
/// (§4.2, from TCPIP-sub and AetherSDR's 4.x subscriptions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    Slice,
    Pan,
    Tx,
    Atu,
    Meter,
    Audio,
    Gps,
    Client,
    Radio,
    Xvtr,
}

impl Topic {
    /// Every topic, in the order the recorder subscribes.
    pub const ALL: [Topic; 10] = [
        Topic::Slice,
        Topic::Pan,
        Topic::Tx,
        Topic::Atu,
        Topic::Meter,
        Topic::Audio,
        Topic::Gps,
        Topic::Client,
        Topic::Radio,
        Topic::Xvtr,
    ];

    fn name(self) -> &'static str {
        match self {
            Topic::Slice => "slice",
            Topic::Pan => "pan",
            Topic::Tx => "tx",
            Topic::Atu => "atu",
            Topic::Meter => "meter",
            Topic::Audio => "audio",
            Topic::Gps => "gps",
            Topic::Client => "client",
            Topic::Radio => "radio",
            Topic::Xvtr => "xvtr",
        }
    }
}

/// Everything the recorder can say to a radio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observe {
    /// `sub <topic> all`
    Subscribe(Topic),
    /// `ping`
    Ping,
    /// `info`
    Info,
    /// `slice list`
    SliceList,
    /// `meter list`
    MeterList,
    /// `client udpport <port>`
    UdpPort(u16),
}

impl Observe {
    /// The command's head, one of [`ALLOWED_VERBS`].
    pub fn verb(&self) -> &'static str {
        match self {
            Observe::Subscribe(_) => "sub",
            Observe::Ping => "ping",
            Observe::Info => "info",
            Observe::SliceList => "slice list",
            Observe::MeterList => "meter list",
            Observe::UdpPort(_) => "client udpport",
        }
    }

    /// The command text, as it goes on the wire after `C<seq>|`.
    pub fn render(&self) -> String {
        match self {
            Observe::Subscribe(topic) => format!("sub {} all", topic.name()),
            Observe::UdpPort(port) => format!("client udpport {port}"),
            other => other.verb().to_string(),
        }
    }

    /// The session pattern a recording answers this command with. The port in `client udpport`
    /// differs between clients, so its rule matches any port.
    fn pattern(&self) -> String {
        match self {
            Observe::UdpPort(_) => "client udpport *".to_string(),
            other => other.render(),
        }
    }
}

/// A command [`admit`] refused, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused(pub String);

/// Whether the recorder may send this command text: one line, no field separator, and headed by a
/// verb in [`ALLOWED_VERBS`]. The check the recorder's only write path runs before every write.
pub fn admit(command: &str) -> Result<(), Refused> {
    if command
        .bytes()
        .any(|b| b.is_ascii_control() || b == b'|' || !b.is_ascii())
    {
        return Err(Refused(format!(
            "{command:?} is not a single plain command"
        )));
    }
    let allowed = ALLOWED_VERBS.iter().any(|verb| {
        command == *verb
            || command
                .strip_prefix(verb)
                .is_some_and(|rest| rest.starts_with(' '))
    });
    if allowed {
        Ok(())
    } else {
        Err(Refused(format!(
            "{command:?} is not an observe-only command; the recorder sends only {ALLOWED_VERBS:?}"
        )))
    }
}

/// The recorder's whole write surface to the radio. Its socket is private, so the only way to put
/// bytes on it is [`Wire::send`].
struct Wire {
    tcp: TcpStream,
    seq: u32,
    sent: usize,
}

impl Wire {
    fn send(&mut self, command: &Observe) -> io::Result<u32> {
        let text = command.render();
        admit(&text).map_err(|refused| io::Error::other(refused.0))?;
        self.seq += 1;
        self.tcp
            .write_all(line::command_line(self.seq, &text).as_bytes())?;
        self.sent += 1;
        Ok(self.seq)
    }
}

/// How the recorder paces itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// How long to keep observing after the setup commands.
    pub duration: Duration,
    /// Time between pings while observing (the API expects one a second).
    pub ping_every: Duration,
    /// After a setup command's reply, how long the radio may stay quiet before the recorder moves
    /// on: lines before then belong to that command's rule.
    pub quiet: Duration,
    /// How long to wait for the prologue, and for a reply.
    pub reply_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            duration: Duration::from_secs(60),
            ping_every: Duration::from_secs(1),
            quiet: Duration::from_millis(300),
            reply_timeout: Duration::from_secs(5),
        }
    }
}

/// What a recording holds, for the tester.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Commands the recorder sent.
    pub commands: usize,
    /// Lines from the radio written to the session.
    pub lines: usize,
    /// VITA packets written.
    pub packets: usize,
    /// UDP packets dropped because their class is not meter, FFT or waterfall data.
    pub packets_skipped: usize,
    /// What the scrubber replaced: each kind with its count of distinct originals.
    pub scrubbed: Vec<(Kind, usize)>,
    /// The radio closed the session before the recording finished.
    pub radio_closed: bool,
}

/// How long to wait for the radio's TCP connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long one read waits before the recorder looks at its clock.
const READ_POLL: Duration = Duration::from_millis(20);

/// Record a session from the radio whose TCP API is at `radio`, sending the UDP registration
/// datagram to `udp_register` (the radio's UDP 4992), and write it to `out`.
pub fn record<W: Write>(
    radio: SocketAddr,
    udp_register: SocketAddr,
    options: &Options,
    out: W,
) -> io::Result<Summary> {
    let tcp = TcpStream::connect_timeout(&radio, CONNECT_TIMEOUT)?;
    tcp.set_read_timeout(Some(READ_POLL))?;
    let _ = tcp.set_nodelay(true);
    let local_ip = tcp.local_addr()?.ip();
    let opened = Instant::now();
    let mut rx = Rx {
        tcp: tcp.try_clone()?,
        lines: LineBuf::default(),
        closed: false,
    };
    let mut wire = Wire {
        tcp,
        seq: 0,
        sent: 0,
    };
    let mut w = Recording {
        out,
        scrub: Scrubber::new(),
        handle: String::new(),
        lines: 0,
        packets: 0,
        skipped: 0,
    };

    let (version, handle, early) = prologue(&mut rx, options.reply_timeout)?;
    w.header(&version, handle)?;
    for l in &early {
        w.timed(opened.elapsed(), l)?;
    }

    // UDP: bound on the interface that reaches the radio, registered with one byte.
    let udp = UdpSocket::bind(SocketAddr::new(local_ip, 0))?;
    udp.set_read_timeout(Some(READ_POLL))?;
    let port = udp.local_addr()?.port();
    udp.send_to(&[0u8], udp_register)?;
    let stop = Arc::new(AtomicBool::new(false));
    let (packets_tx, packets) = mpsc::channel::<(Instant, Vec<u8>)>();
    let receiver = {
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            let mut buf = vec![0u8; 65_536];
            while !stop.load(Ordering::SeqCst) {
                if let Ok((n, _)) = udp.recv_from(&mut buf) {
                    if packets_tx
                        .send((Instant::now(), buf[..n].to_vec()))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        })
    };

    let result = run(&mut wire, &mut rx, &mut w, &packets, port, opened, options);
    stop.store(true, Ordering::SeqCst);
    let _ = receiver.join();
    for (at, bytes) in packets.try_iter() {
        w.vita(at - opened, &bytes)?;
    }
    result?;
    w.out.flush()?;
    Ok(Summary {
        commands: wire.sent,
        lines: w.lines,
        packets: w.packets,
        packets_skipped: w.skipped,
        scrubbed: w.scrub.report(),
        radio_closed: rx.closed,
    })
}

/// The recording itself: the setup commands, then pings until `options.duration` has passed.
fn run<W: Write>(
    wire: &mut Wire,
    rx: &mut Rx,
    w: &mut Recording<W>,
    packets: &mpsc::Receiver<(Instant, Vec<u8>)>,
    port: u16,
    opened: Instant,
    options: &Options,
) -> io::Result<()> {
    let mut setup: Vec<Observe> = Topic::ALL.iter().map(|t| Observe::Subscribe(*t)).collect();
    setup.extend([
        Observe::Info,
        Observe::SliceList,
        Observe::MeterList,
        Observe::UdpPort(port),
    ]);
    for command in &setup {
        let seq = wire.send(command)?;
        let (reply, lines) = collect(rx, seq, options)?;
        match reply {
            Some(reply) => w.rule(&command.pattern(), &reply, &lines)?,
            None => {
                w.note(&format!(
                    "no reply to `{}` within {} ms",
                    command.render(),
                    options.reply_timeout.as_millis()
                ))?;
                for l in &lines {
                    w.timed(opened.elapsed(), l)?;
                }
            }
        }
        for (at, bytes) in packets.try_iter() {
            w.vita(at - opened, &bytes)?;
        }
        if rx.closed {
            return Ok(());
        }
    }

    let end = Instant::now() + options.duration;
    let mut next_ping = Instant::now();
    let mut pending: Option<(u32, Instant)> = None;
    while Instant::now() < end && !rx.closed {
        let now = Instant::now();
        if pending.is_some_and(|(_, since)| now - since >= options.reply_timeout) {
            w.note("a ping went unanswered")?;
            pending = None;
        }
        if pending.is_none() && now >= next_ping {
            pending = Some((wire.send(&Observe::Ping)?, now));
            next_ping += options.ping_every;
        }
        for l in rx.poll()? {
            match line::parse_reply(&l) {
                Some(reply) if pending.is_some_and(|(seq, _)| seq == reply.seq) => {
                    w.rule(&Observe::Ping.pattern(), &reply, &[])?;
                    pending = None;
                }
                _ => w.timed(opened.elapsed(), &l)?,
            }
        }
        for (at, bytes) in packets.try_iter() {
            w.vita(at - opened, &bytes)?;
        }
    }
    Ok(())
}

/// The radio's side of the TCP session, read in whole lines.
struct Rx {
    tcp: TcpStream,
    lines: LineBuf,
    closed: bool,
}

impl Rx {
    /// The lines completed within one read timeout. A closed or reset connection ends the
    /// recording rather than failing it: what was recorded so far is already written.
    fn poll(&mut self) -> io::Result<Vec<String>> {
        let mut buf = [0u8; 8192];
        match self.tcp.read(&mut buf) {
            Ok(0) => {
                self.closed = true;
                Ok(Vec::new())
            }
            Ok(n) => self
                .lines
                .push(&buf[..n])
                .map_err(|_| io::Error::other("the radio sent a line longer than 16 MiB")),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                Ok(Vec::new())
            }
            Err(_) => {
                self.closed = true;
                Ok(Vec::new())
            }
        }
    }
}

/// Read the prologue: the version line and the handle, plus any line that came before them.
fn prologue(rx: &mut Rx, timeout: Duration) -> io::Result<(String, u32, Vec<String>)> {
    let deadline = Instant::now() + timeout;
    let (mut version, mut handle, mut early) = (None, None, Vec::new());
    while Instant::now() < deadline && !rx.closed {
        for l in rx.poll()? {
            if version.is_none() && l.starts_with('V') {
                version = Some(l);
            } else if handle.is_none() && l.starts_with('H') {
                let hex = &l[1..];
                let parsed = if !hex.is_empty()
                    && hex.len() <= 8
                    && hex.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    u32::from_str_radix(hex, 16).ok().filter(|h| *h != 0)
                } else {
                    None
                };
                match parsed {
                    Some(h) => handle = Some(h),
                    None => return Err(io::Error::other(format!("a malformed handle line {l:?}"))),
                }
            } else {
                early.push(l);
            }
        }
        if let (Some(v), Some(h)) = (&version, handle) {
            return Ok((v.clone(), h, early));
        }
    }
    Err(io::Error::new(
        ErrorKind::TimedOut,
        "the radio sent no version and handle (is this a FlexRadio on its API port?)",
    ))
}

/// Collect a setup command's reply and the lines that follow it until the radio goes quiet.
fn collect(rx: &mut Rx, seq: u32, options: &Options) -> io::Result<(Option<Reply>, Vec<String>)> {
    let started = Instant::now();
    let mut last = started;
    let (mut reply, mut lines) = (None, Vec::new());
    loop {
        let got = rx.poll()?;
        if !got.is_empty() {
            last = Instant::now();
        }
        for l in got {
            match line::parse_reply(&l) {
                Some(r) if r.seq == seq && reply.is_none() => reply = Some(r),
                _ => lines.push(l),
            }
        }
        let now = Instant::now();
        if rx.closed
            || (reply.is_some() && now - last >= options.quiet)
            || now - started >= options.reply_timeout
        {
            return Ok((reply, lines));
        }
    }
}

/// The session being written: every line goes through the scrubber first.
struct Recording<W: Write> {
    out: W,
    scrub: Scrubber,
    /// The recorder's own handle as eight hex digits, replaced by `{h}`.
    handle: String,
    lines: usize,
    packets: usize,
    skipped: usize,
}

impl<W: Write> Recording<W> {
    fn header(&mut self, version: &str, handle: u32) -> io::Result<()> {
        self.handle = format!("{handle:08X}");
        writeln!(self.out, "{FORMAT_LINE}")?;
        writeln!(
            self.out,
            "# Recorded from a FlexRadio by flexrecord, observe-only. It sent only these command\n\
             # heads: {ALLOWED_VERBS:?}.\n\
             # Replaced before writing: IP and MAC addresses, serial numbers, client ids, host and\n\
             # station names, GPS position and location. Kept as recorded: the callsign and the\n\
             # radio's nickname."
        )?;
        writeln!(self.out, "prologue {version}")?;
        writeln!(self.out, "handle {}", self.handle)?;
        writeln!(self.out)?;
        self.out.flush()
    }

    /// A radio line made safe to write: the recorder's handle templated, then scrubbed.
    fn clean(&mut self, line: &str) -> String {
        let lower = self.handle.to_ascii_lowercase();
        let mut l = line.to_string();
        for h in [self.handle.as_str(), lower.as_str()] {
            l = l
                .replace(&format!("S{h}|"), "S{h}|")
                .replace(&format!("0x{h}"), "0x{h}");
        }
        self.scrub.line(&l)
    }

    fn rule(&mut self, pattern: &str, reply: &Reply, lines: &[String]) -> io::Result<()> {
        let message = self.clean(&reply.message);
        writeln!(self.out, "on {pattern}")?;
        if message.is_empty() {
            writeln!(self.out, "reply {}", reply.code)?;
        } else {
            writeln!(self.out, "reply {} {message}", reply.code)?;
        }
        for l in lines {
            let clean = self.clean(l);
            writeln!(self.out, "send {clean}")?;
            self.lines += 1;
        }
        writeln!(self.out)?;
        self.lines += 1;
        self.out.flush()
    }

    fn timed(&mut self, at: Duration, line: &str) -> io::Result<()> {
        let clean = self.clean(line);
        writeln!(self.out, "at {}\nsend {clean}\n", at.as_millis())?;
        self.lines += 1;
        self.out.flush()
    }

    fn vita(&mut self, at: Duration, bytes: &[u8]) -> io::Result<()> {
        let kept = vita::header(bytes).is_some_and(|h| {
            matches!(
                h.class,
                vita::class::METER | vita::class::FFT | vita::class::WATERFALL
            )
        });
        if !kept {
            self.skipped += 1;
            return Ok(());
        }
        writeln!(
            self.out,
            "at {}\nvita {}\n",
            at.as_millis(),
            hex_string(bytes)
        )?;
        self.packets += 1;
        self.out.flush()
    }

    fn note(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.out, "# {text}\n")?;
        self.out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fault::{Fault, Foreign};
    use crate::server::{Config, Simulator};
    use crate::session::Session;
    use crate::vita::{Content, Start, Stream};
    use std::collections::BTreeSet;

    /// One of each command shape. The match below has no wildcard arm, so a variant added to
    /// [`Observe`] stops this file compiling until it is looked at here.
    fn every_shape() -> Vec<Observe> {
        let mut shapes: Vec<Observe> = Topic::ALL.iter().map(|t| Observe::Subscribe(*t)).collect();
        shapes.extend([
            Observe::Ping,
            Observe::Info,
            Observe::SliceList,
            Observe::MeterList,
            Observe::UdpPort(4993),
        ]);
        for shape in &shapes {
            match shape {
                Observe::Subscribe(_)
                | Observe::Ping
                | Observe::Info
                | Observe::SliceList
                | Observe::MeterList
                | Observe::UdpPort(_) => {}
            }
        }
        shapes
    }

    /// Commands that can transmit or change the radio's state (port plan §3.2 and §4.2), and
    /// more besides. The recorder must be unable to send any of them.
    const FORBIDDEN: &[&str] = &[
        "xmit 1",
        "xmit 0",
        "transmit tune 1",
        "transmit set rfpower=100",
        "transmit set dax=1",
        "atu start",
        "atu bypass",
        "cwx send \"CQ\" 1",
        "cwx clear",
        "cw key 1",
        "cw ptt 1",
        "slice set 0 tx=1",
        "slice tune 0 14.074",
        "slice create freq=14.074",
        "slice remove 0",
        "slice m 14.074 pan=0x40000000",
        "filt 0 100 2900",
        "dax audio set 1 tx=1",
        "stream create type=dax_tx",
        "stream create type=dax_rx dax_channel=1",
        "stream remove 0x84000000",
        "display panafall create x=100 y=100",
        "display pan remove 0x40000000",
        "interlock timeout=1000",
        "mic input MIC",
        "profile tx load \"Default\"",
        "client gui",
        "client program flexrecord",
        "client station Shack",
        "client set local_ptt=1",
        "client disconnect 0x2B6E1F40",
        "client bind client_id=00000000-0000-4000-8000-000000000001",
        "radio set tnf_enabled=1",
        "tnf create freq=14.1",
        "amplifier set 1 operate=1",
        "tgxl autotune",
        "keepalive enable",
        "waveform remove x",
        "dvk play 1",
        "eq rxsc mode=1",
        "mixer mic gain=50",
        "memory apply 1",
        "file upload 1 update",
        "xvtr create",
        "spot add",
        "audio client 0 slice 0 gain 50",
        "mox 1",
        "tune 1",
        "subscribe",
        "pinger",
        "information",
    ];

    #[test]
    fn the_recorders_own_handle_becomes_the_template() {
        let mut w = Recording {
            out: Vec::new(),
            scrub: Scrubber::new(),
            handle: String::new(),
            lines: 0,
            packets: 0,
            skipped: 0,
        };
        w.header("V1.4.0.0", 0x2B6E_1F40).unwrap();
        assert_eq!(
            w.clean("S2B6E1F40|client 0x2B6E1F40 connected program=x"),
            "S{h}|client 0x{h} connected program=x"
        );
        assert_eq!(
            w.clean("S0|interlock tx_client_handle=0x2b6e1f40 state=READY"),
            "S0|interlock tx_client_handle=0x{h} state=READY"
        );
        // Another client's handle is not ours and stays as it was.
        let other = "S7A3C0001|slice 1 client_handle=0x7A3C0001";
        assert_eq!(w.clean(other), other);
        let text = String::from_utf8(w.out).unwrap();
        assert!(
            text.contains("\nprologue V1.4.0.0\nhandle 2B6E1F40\n"),
            "{text}"
        );
    }

    #[test]
    fn the_allow_list_is_pinned() {
        // Changing this list is a transmit-safety review: every entry must be unable to transmit
        // or change the radio's state. Do not edit it to make a test pass. Compared as slices so
        // that a longer list fails here with both lists printed, not as a type error.
        assert_eq!(
            ALLOWED_VERBS.as_slice(),
            [
                "sub",
                "ping",
                "info",
                "slice list",
                "meter list",
                "client udpport"
            ]
            .as_slice()
        );
    }

    #[test]
    fn every_command_shape_renders_an_allowed_verb_and_the_list_has_no_spare_entry() {
        let shapes = every_shape();
        for shape in &shapes {
            let text = shape.render();
            assert_eq!(admit(&text), Ok(()), "{text:?}");
            assert!(ALLOWED_VERBS.contains(&shape.verb()));
            assert!(
                text == shape.verb() || text.starts_with(&format!("{} ", shape.verb())),
                "{text:?} is not headed by {:?}",
                shape.verb()
            );
        }
        let used: BTreeSet<&str> = shapes.iter().map(|s| s.verb()).collect();
        assert_eq!(used, ALLOWED_VERBS.into_iter().collect::<BTreeSet<_>>());
    }

    #[test]
    fn no_transmit_capable_or_state_changing_command_is_admitted() {
        for command in FORBIDDEN {
            assert!(admit(command).is_err(), "{command:?} was admitted");
        }
        // The check also refuses a second command smuggled into one, in every way the line
        // grammar allows.
        for smuggled in [
            "sub slice all\nC9|xmit 1",
            "sub slice all\rxmit 1",
            "ping|xmit 1",
            "info\u{0}",
            "sub slice all\u{00e9}",
        ] {
            assert!(admit(smuggled).is_err(), "{smuggled:?} was admitted");
        }
        // The control: the same check does admit what the recorder sends.
        for allowed in [
            "sub tx all",
            "ping",
            "info",
            "slice list",
            "meter list",
            "client udpport 4993",
        ] {
            assert_eq!(admit(allowed), Ok(()), "{allowed:?}");
        }
    }

    /// A raw client: sends each command after the previous reply, and returns every line it
    /// received after the prologue. A closing `ping` flushes the last command's statuses; its
    /// reply is left out.
    fn exchange(sim: &Simulator, commands: &[String]) -> Vec<String> {
        let mut reader = TcpStream::connect(sim.tcp_addr()).unwrap();
        reader
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut writer = reader.try_clone().unwrap();
        let mut lb = LineBuf::default();
        let mut buf = [0u8; 4096];
        let mut got: Vec<String> = Vec::new();
        let mut read_until = |got: &mut Vec<String>, from: usize, tag: &str| {
            while !got[from..].iter().any(|l| l.starts_with(tag)) {
                let n = reader.read(&mut buf).expect("the simulator answers");
                assert!(n > 0, "the simulator closed the connection");
                got.extend(lb.push(&buf[..n]).unwrap());
            }
        };
        read_until(&mut got, 0, "H");
        got.clear();
        let ping = "ping".to_string();
        for (i, command) in commands.iter().chain([&ping]).enumerate() {
            let seq = i as u32 + 1;
            let from = got.len();
            writer
                .write_all(line::command_line(seq, command).as_bytes())
                .unwrap();
            read_until(&mut got, from, &format!("R{seq}|"));
        }
        let last = format!("R{}|", commands.len() + 1);
        got.retain(|l| !l.starts_with(&last));
        got
    }

    #[test]
    fn a_recording_of_the_simulator_sends_only_allowed_verbs_strips_identifiers_and_replays() {
        let config = || Config {
            faults: vec![Fault::ForeignClient(Foreign::default())],
            streams: vec![Stream {
                stream_id: vita::METER_STREAM_ID,
                content: Content::Meters(vec![(1, -9000), (3, 128)]),
                period: Duration::from_millis(10),
                ticks: Some(5),
                start: Start::Registered,
                until: None,
            }],
            ..Config::default()
        };
        let radio = Simulator::start(Session::v4_gui_client(), config()).unwrap();
        let options = Options {
            duration: Duration::from_millis(300),
            ping_every: Duration::from_millis(100),
            // The simulator writes a reply and its statuses back to back; the quiet window only
            // has to outlast a scheduling delay between them.
            quiet: Duration::from_millis(100),
            reply_timeout: Duration::from_secs(5),
        };
        let mut file = Vec::new();
        let summary = record(radio.tcp_addr(), radio.udp_addr(), &options, &mut file).unwrap();
        let text = String::from_utf8(file).unwrap();

        // 1. What reached the wire: only admitted commands, every allowed verb but `ping` in
        // setup, and pings while observing. The last ping may still be in flight.
        let arrived = |log: &[crate::server::Logged]| {
            log.iter()
                .filter(|l| matches!(l.event, crate::server::Event::Command { .. }))
                .count()
        };
        assert!(radio.wait_for(Duration::from_secs(5), |log| arrived(log)
            >= summary.commands));
        let sent = radio.commands();
        assert_eq!(sent.len(), summary.commands);
        for command in &sent {
            assert_eq!(admit(command), Ok(()), "the recorder sent {command:?}");
        }
        let heads: BTreeSet<&str> = sent
            .iter()
            .map(|c| {
                ALLOWED_VERBS
                    .iter()
                    .find(|v| c == *v || c.starts_with(&format!("{v} ")))
                    .copied()
                    .unwrap()
            })
            .collect();
        assert_eq!(heads, ALLOWED_VERBS.into_iter().collect::<BTreeSet<_>>());

        // 2. Nothing identifying survived; the placeholders and the kept callsign are there.
        for secret in [
            "0000-1111-6400-0001",
            "00:1C:2D:00:00:01",
            "192.168.1.20",
            "192.168.1.1",
            "255.255.255.0",
            "6F1C2A3B-0000-4000-8000-00000000B001",
            "9D2E4F60-0000-4000-8000-00000000F001",
            "Shack-PC",
            "127.0.0.1",
        ] {
            assert!(
                !text.contains(secret),
                "{secret} reached the recording:\n{text}"
            );
        }
        for kept in [
            "serial-1",
            "ip=192.0.2.",
            "station=station-",
            "callsign=\"N0CALL\"",
        ] {
            assert!(text.contains(kept), "{kept} missing:\n{text}");
        }
        let kinds: Vec<Kind> = summary.scrubbed.iter().map(|(k, _)| *k).collect();
        for kind in [
            Kind::Ipv4,
            Kind::Mac,
            Kind::Serial,
            Kind::ClientId,
            Kind::Station,
        ] {
            assert!(kinds.contains(&kind), "{kind:?} not reported in {kinds:?}");
        }
        assert!(summary.packets >= 1, "no meter packet was recorded");
        assert!(text.contains("\nvita "));
        assert!(!summary.radio_closed);

        // 3. The recording replays: a client asking the recorded session what the recorder
        // asked hears what a client asking the original hears, scrubbed.
        let replayed = Simulator::start(Session::parse(&text).unwrap(), Config::default()).unwrap();
        let original = Simulator::start(Session::v4_gui_client(), config()).unwrap();
        let mut commands: Vec<String> = Topic::ALL
            .iter()
            .map(|t| Observe::Subscribe(*t).render())
            .collect();
        commands
            .extend(["info", "slice list", "meter list", "client udpport 4993"].map(String::from));
        let heard = exchange(&replayed, &commands);
        let mut scrub = Scrubber::new();
        let expected: Vec<String> = exchange(&original, &commands)
            .iter()
            .map(|l| scrub.line(l))
            .collect();
        assert_eq!(heard, expected);
        assert!(heard
            .iter()
            .any(|l| l.contains("client 0x7A3C0001 connected")));
    }
}
