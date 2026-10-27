//! The recorder: what a tester runs to capture a session from their SDR program for a report, so
//! the simulator can be checked against a real server.
//!
//! **Observe-only, by construction.** The recorder connects to a real TCI server as one more
//! client. It can send nothing that keys a transmitter or changes the radio:
//!
//! - its only write path is `Wire::send`, which takes an [`Observe`]: a closed set of commands with
//!   no way to say `trx`, `tune`, `keyer`, CW, a frequency, a mode, a drive, a spot, `start`,
//!   `stop` or any setting;
//! - before writing, `Wire::send` checks the rendered text against [`ALLOWED`] and each command's
//!   shape again ([`admit`]), and refuses anything else;
//! - it has no way to send a binary message, so it never sends audio and never answers a
//!   TX_CHRONO (a server that sends it one gets nothing back, which the spec allows);
//! - a test pins [`ALLOWED`] word for word, and another runs the recorder against the simulator and
//!   checks every command and frame that reached it.
//!
//! **What it sends.** Its own meter pushes (`rx_sensors_enable`, `tx_sensors_enable`: per client,
//! spec §4.4) and six reads that carry no argument, which no grammar can take as a write. Receive
//! audio and IQ only when asked: a server may give a receiver's audio to one client alone (piHPSDR
//! does), so starting it could take it from another program. At the end it turns off what it
//! turned on and closes the WebSocket cleanly.
//!
//! **What it writes.** Every text message the server sends, in order with its time, scrubbed
//! ([`crate::scrub`]); every command it sent; every frame's header, and a frame's samples only for
//! the first few of each kind when asked. The output goes to the writer the caller gives, a local
//! file for the `tcirecord` binary, and nowhere else: the tester reads it and decides whether to
//! attach it. The format is [`crate::session`]'s, and [`crate::session::Recording::dialect`]
//! replays its init burst in the simulator.

use std::collections::HashMap;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::client::{Incoming, Peer};
use crate::frame::{kind, Header, HEADER_LEN};
use crate::scrub::{Kind, Scrubber};
use crate::session::{Entry, FORMAT_LINE};
use crate::wire;

/// The port the `tcirecord` binary uses when none is given: ExpertSDR3's (per a 2023 forum post),
/// Thetis's and AetherSDR's. piHPSDR's is 40001.
pub const DEFAULT_PORT: u16 = 50001;

/// Every command the recorder can put on the wire, by name.
///
/// ⚠️ A transmit-safety list. Each must be unable to key a transmitter or change the radio: the
/// two sensor switches turn this client's own meter pushes on and off; the four stream commands
/// start and stop this client's own receive streams; the six reads are sent with no argument,
/// which no server can read as a write. The test `the_allow_list_is_pinned` holds it word for word.
pub const ALLOWED: [&str; 12] = [
    "rx_sensors_enable",
    "tx_sensors_enable",
    "audio_start",
    "audio_stop",
    "iq_start",
    "iq_stop",
    "cw_macros_speed",
    "cw_macros_delay",
    "volume",
    "mon_volume",
    "digl_offset",
    "digu_offset",
];

/// A read the recorder may send: always with no argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    CwMacrosSpeed,
    CwMacrosDelay,
    Volume,
    MonVolume,
    DiglOffset,
    DiguOffset,
}

impl Read {
    /// Every read, in the order the recorder sends them.
    pub const ALL: [Read; 6] = [
        Read::CwMacrosSpeed,
        Read::CwMacrosDelay,
        Read::Volume,
        Read::MonVolume,
        Read::DiglOffset,
        Read::DiguOffset,
    ];

    fn name(self) -> &'static str {
        match self {
            Read::CwMacrosSpeed => "cw_macros_speed",
            Read::CwMacrosDelay => "cw_macros_delay",
            Read::Volume => "volume",
            Read::MonVolume => "mon_volume",
            Read::DiglOffset => "digl_offset",
            Read::DiguOffset => "digu_offset",
        }
    }
}

/// Everything the recorder can say to a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observe {
    /// `rx_sensors_enable:true,<ms>;` (`Some`) or `rx_sensors_enable:false;`.
    RxSensors(Option<u16>),
    /// `tx_sensors_enable:true,<ms>;` or `tx_sensors_enable:false;`.
    TxSensors(Option<u16>),
    /// `audio_start:<trx>;`
    AudioStart(u8),
    /// `audio_stop:<trx>;`
    AudioStop(u8),
    /// `iq_start:<trx>;`
    IqStart(u8),
    /// `iq_stop:<trx>;`
    IqStop(u8),
    /// `<name>;`
    Read(Read),
}

impl Observe {
    /// The command's name, one of [`ALLOWED`].
    pub fn name(&self) -> &'static str {
        match self {
            Observe::RxSensors(_) => "rx_sensors_enable",
            Observe::TxSensors(_) => "tx_sensors_enable",
            Observe::AudioStart(_) => "audio_start",
            Observe::AudioStop(_) => "audio_stop",
            Observe::IqStart(_) => "iq_start",
            Observe::IqStop(_) => "iq_stop",
            Observe::Read(r) => r.name(),
        }
    }

    /// The command as it goes on the wire.
    pub fn render(&self) -> String {
        match self {
            Observe::RxSensors(Some(ms)) | Observe::TxSensors(Some(ms)) => {
                format!("{}:true,{ms};", self.name())
            }
            Observe::RxSensors(None) | Observe::TxSensors(None) => {
                format!("{}:false;", self.name())
            }
            Observe::AudioStart(t)
            | Observe::AudioStop(t)
            | Observe::IqStart(t)
            | Observe::IqStop(t) => {
                format!("{}:{t};", self.name())
            }
            Observe::Read(_) => format!("{};", self.name()),
        }
    }
}

/// A command [`admit`] refused, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused(pub String);

/// Whether the recorder may send this text: one plain command, named in [`ALLOWED`], in that
/// command's one allowed shape. The check the recorder's only write path runs before every write.
pub fn admit(text: &str) -> Result<(), Refused> {
    let refuse = |why: &str| {
        Err(Refused(format!(
            "{text:?}: {why}; the recorder sends only {ALLOWED:?}"
        )))
    };
    if !text.is_ascii()
        || text
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
    {
        return refuse("not one plain command");
    }
    if !text.ends_with(';') || text.matches(';').count() != 1 {
        return refuse("not exactly one command");
    }
    let parsed = wire::parse(text);
    let ([c], []) = (parsed.commands.as_slice(), parsed.flaws.as_slice()) else {
        return refuse("not a well-formed command");
    };
    if !ALLOWED.contains(&c.name.as_str()) || c.text.split(':').next() != Some(c.name.as_str()) {
        return refuse("not an observe-only command");
    }
    let shaped = match c.name.as_str() {
        "rx_sensors_enable" | "tx_sensors_enable" => match c.args.as_slice() {
            [on] => wire::boolean(on).is_some(),
            [on, ms] => {
                wire::boolean(on) == Some(true)
                    && ms.parse::<u16>().is_ok_and(|v| (30..=1000).contains(&v))
            }
            _ => false,
        },
        "audio_start" | "audio_stop" | "iq_start" | "iq_stop" => {
            matches!(c.args.as_slice(), [t] if t.len() == 1 && t.parse::<u8>().is_ok_and(|t| t < 8))
        }
        _ => c.args.is_empty() && !text.contains(':'),
    };
    if shaped {
        Ok(())
    } else {
        refuse("not the shape the recorder sends")
    }
}

/// The recorder's whole write surface. Its connection is private, so the only way to put bytes on
/// it is [`Wire::send`]; there is no binary path.
struct Wire {
    peer: Peer,
    sent: usize,
}

impl Wire {
    fn send(&mut self, command: &Observe) -> io::Result<String> {
        let text = command.render();
        admit(&text).map_err(|refused| io::Error::other(refused.0))?;
        self.peer.send_text(&text)?;
        self.sent += 1;
        Ok(text)
    }

    fn recv(&mut self, timeout: Duration) -> io::Result<Option<Incoming>> {
        self.peer.recv(timeout)
    }
}

/// What to record, and for how long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// How long to observe after the init burst.
    pub duration: Duration,
    /// Also start this transceiver's receive audio, to record its frame headers.
    pub audio: Option<u8>,
    /// Also start this transceiver's IQ.
    pub iq: Option<u8>,
    /// Write the samples of the first this-many frames of each kind.
    pub payload_frames: usize,
    /// The interval asked for the meter pushes, ms (30–1000).
    pub sensor_interval_ms: u16,
    /// How long to wait for the connection, and for the burst's `ready;`.
    pub timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            duration: Duration::from_secs(60),
            audio: None,
            iq: None,
            payload_frames: 0,
            sensor_interval_ms: 200,
            timeout: Duration::from_secs(5),
        }
    }
}

/// What a recording holds, for the tester.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Text messages written.
    pub messages: usize,
    /// Frame headers written.
    pub frames: usize,
    /// TX_CHRONO frames the server sent (never answered).
    pub chronos: usize,
    /// Commands sent.
    pub commands: usize,
    /// The init burst ended with `ready;`.
    pub ready: bool,
    /// What the scrubber replaced: each kind with its count of distinct originals.
    pub scrubbed: Vec<(Kind, usize)>,
    /// The server closed the connection before the recording finished.
    pub server_closed: bool,
}

/// Record a session from the TCI server at `server` and write it to `out`.
pub fn record<W: Write>(server: SocketAddr, options: &Options, out: W) -> io::Result<Summary> {
    let peer = Peer::connect(server, options.timeout)?;
    let opened = Instant::now();
    let mut wire = Wire { peer, sent: 0 };
    let mut w = Recording {
        out,
        opened,
        scrub: Scrubber::new(),
        messages: 0,
        frames: 0,
        chronos: 0,
        payloads: HashMap::new(),
        payload_frames: options.payload_frames,
        closed: false,
    };
    w.header(server.port(), options)?;

    // The init burst, through `ready;`.
    let mut ready = false;
    let deadline = Instant::now() + options.timeout;
    while !ready && !w.closed {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        if let Some(text) = w.take(wire.recv(left)?)? {
            ready = wire::parse(&text)
                .commands
                .iter()
                .any(|c| c.name == "ready");
        }
    }
    if !ready {
        w.note(&format!(
            "no ready; within {} ms",
            options.timeout.as_millis()
        ))?;
    }

    // What the recorder turns on.
    let interval = options.sensor_interval_ms.clamp(30, 1000);
    let mut setup = vec![
        Observe::RxSensors(Some(interval)),
        Observe::TxSensors(Some(interval)),
    ];
    setup.extend(Read::ALL.map(Observe::Read));
    if let Some(t) = options.audio {
        setup.push(Observe::AudioStart(t));
    }
    if let Some(t) = options.iq {
        setup.push(Observe::IqStart(t));
    }
    for command in &setup {
        if w.closed {
            break;
        }
        let text = wire.send(command)?;
        w.entry(Entry::Sent { ms: w.ms(), text })?;
    }

    // Observe.
    let end = Instant::now() + options.duration;
    while !w.closed {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        w.take(wire.recv(left.min(Duration::from_millis(100)))?)?;
    }

    // Turn off what it turned on, then close cleanly.
    if !w.closed {
        let mut teardown = Vec::new();
        if let Some(t) = options.audio {
            teardown.push(Observe::AudioStop(t));
        }
        if let Some(t) = options.iq {
            teardown.push(Observe::IqStop(t));
        }
        teardown.extend([Observe::RxSensors(None), Observe::TxSensors(None)]);
        for command in &teardown {
            let text = wire.send(command)?;
            w.entry(Entry::Sent { ms: w.ms(), text })?;
        }
        let drain = Instant::now() + Duration::from_millis(200);
        while !w.closed && Instant::now() < drain {
            w.take(wire.recv(Duration::from_millis(50))?)?;
        }
    }
    let server_closed = w.closed;
    w.end(if server_closed {
        "the server closed the connection"
    } else {
        "the time was up"
    })?;
    if !server_closed {
        wire.peer.close(Duration::from_secs(1));
    }
    w.out.flush()?;
    Ok(Summary {
        messages: w.messages,
        frames: w.frames,
        chronos: w.chronos,
        commands: wire.sent,
        ready,
        scrubbed: w.scrub.report(),
        server_closed,
    })
}

/// The recording being written: every text passes through the scrubber first.
struct Recording<W: Write> {
    out: W,
    opened: Instant,
    scrub: Scrubber,
    messages: usize,
    frames: usize,
    chronos: usize,
    /// Payloads written so far, per frame type.
    payloads: HashMap<u32, usize>,
    payload_frames: usize,
    closed: bool,
}

impl<W: Write> Recording<W> {
    fn ms(&self) -> u64 {
        u64::try_from(self.opened.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn header(&mut self, port: u16, options: &Options) -> io::Result<()> {
        writeln!(self.out, "{FORMAT_LINE}")?;
        writeln!(
            self.out,
            "# Recorded from a TCI server by tcirecord, observe-only. It could send only these\n\
             # commands: {ALLOWED:?}. It never transmits, never changes a setting and never sends audio.\n\
             # Replaced before writing: IP and MAC addresses and serial numbers. Kept: everything else.\n\
             # Asked for: receive audio {}, IQ {}, the samples of {} frames of each kind.",
            options.audio.map_or("no".to_string(), |t| format!("on transceiver {t}")),
            options.iq.map_or("no".to_string(), |t| format!("on transceiver {t}")),
            options.payload_frames,
        )?;
        writeln!(self.out, "port {port}")
    }

    fn entry(&mut self, entry: Entry) -> io::Result<()> {
        writeln!(self.out, "{}", entry.line())
    }

    /// A remark for the reader, as a comment line.
    fn note(&mut self, text: &str) -> io::Result<()> {
        let ms = self.ms();
        writeln!(self.out, "# at {ms} ms: {}", crate::session::escape(text))
    }

    fn end(&mut self, why: &str) -> io::Result<()> {
        let ms = self.ms();
        self.entry(Entry::End {
            ms,
            why: why.to_string(),
        })
    }

    /// Write what arrived; returns a text message's (scrubbed) text.
    fn take(&mut self, got: Option<Incoming>) -> io::Result<Option<String>> {
        let ms = self.ms();
        match got {
            Some(Incoming::Text(text)) => {
                let text = self.scrub.text(&text);
                self.entry(Entry::Recv {
                    ms,
                    text: text.clone(),
                })?;
                self.messages += 1;
                Ok(Some(text))
            }
            Some(Incoming::Binary(bytes)) => {
                let Some(header) = Header::decode(&bytes) else {
                    self.note(&format!(
                        "a binary message of {} bytes, too short for a header",
                        bytes.len()
                    ))?;
                    return Ok(None);
                };
                if header.kind == kind::TX_CHRONO {
                    self.chronos += 1;
                }
                let payload = &bytes[HEADER_LEN..];
                self.entry(Entry::Frame {
                    ms,
                    header,
                    payload: payload.len(),
                })?;
                self.frames += 1;
                let written = self.payloads.entry(header.kind).or_insert(0);
                if *written < self.payload_frames && !payload.is_empty() {
                    *written += 1;
                    self.entry(Entry::Payload {
                        ms,
                        bytes: payload.to_vec(),
                    })?;
                }
                Ok(None)
            }
            Some(Incoming::Closed) => {
                self.closed = true;
                Ok(None)
            }
            Some(Incoming::Pong) | None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::Dialect;
    use crate::event::Event;
    use crate::fault::Fault;
    use crate::server::{Config, Simulator};
    use crate::session::Recording as Session;
    use std::collections::BTreeSet;

    /// One of each command shape. The match below has no wildcard arm, so a variant added to
    /// [`Observe`] stops this file compiling until it is looked at here.
    fn every_shape() -> Vec<Observe> {
        let mut shapes = vec![
            Observe::RxSensors(Some(200)),
            Observe::RxSensors(None),
            Observe::TxSensors(Some(30)),
            Observe::TxSensors(None),
            Observe::AudioStart(0),
            Observe::AudioStop(1),
            Observe::IqStart(0),
            Observe::IqStop(7),
        ];
        shapes.extend(Read::ALL.map(Observe::Read));
        for shape in &shapes {
            match shape {
                Observe::RxSensors(_)
                | Observe::TxSensors(_)
                | Observe::AudioStart(_)
                | Observe::AudioStop(_)
                | Observe::IqStart(_)
                | Observe::IqStop(_)
                | Observe::Read(_) => {}
            }
        }
        shapes
    }

    /// Commands that key, unkey, tune, or change the radio or another client's streams, and more
    /// besides, in every spelling the grammar allows. The recorder must be unable to send any.
    const FORBIDDEN: &[&str] = &[
        "trx:0,true;",
        "trx:0,true,tci;",
        "trx:0,false;",
        "trx:0;",
        "TRX:0,TRUE;",
        "tune:0,true;",
        "tune:0;",
        "keyer:0,true,0;",
        "cw_macros:0,CQ;",
        "cw_msg:0,TU,K1ABC,599;",
        "cw_macros_stop;",
        "cw_terminal:true;",
        "cw_macros_speed:30;",
        "cw_macros_speed_up:5;",
        "cw_macros_delay:0;",
        "drive:0,100;",
        "drive:0;",
        "drive:100;",
        "tune_drive:0,100;",
        "vfo:0,0,14074000;",
        "vfo:0,0;",
        "dds:0,14000000;",
        "if:0,0,1000;",
        "modulation:0,usb;",
        "modulation:0;",
        "split_enable:0,true;",
        "rit_enable:0,true;",
        "xit_offset:0,100;",
        "rx_filter_band:0,100,2900;",
        "spot:K1ABC,CW,14025000,0,;",
        "spot_delete:K1ABC;",
        "spot_clear;",
        "start;",
        "stop;",
        "volume:-10;",
        "volume:0;",
        "mute:true;",
        "mute;",
        "mon_enable:true;",
        "digu_offset:1500;",
        "agc_mode:0,fast;",
        "lock:0,true;",
        "audio_samplerate:12000;",
        "audio_stream_samples:512;",
        "audio_stream_channels:1;",
        "tx_stream_audio_buffering:500;",
        "iq_samplerate:96000;",
        "line_out_start:0;",
        "line_out_recorder_save:0,x.wav;",
        "set_in_focus;",
        "tx_enable:0,true;",
        "rx_sensors_enable:maybe;",
        "rx_sensors_enable:true,5;",
        "rx_sensors_enable:false,200;",
        "rx_sensors_enable;",
        "audio_start;",
        "audio_start:0,1;",
        "audio_start:x;",
        "audio_start:99;",
        "audio_start:-1;",
        "audio_start:00;",
        "iq_start:8;",
        "volume:;",
        "ready;",
        "protocol;",
    ];

    #[test]
    fn the_allow_list_is_pinned() {
        // Changing this list is a transmit-safety review: every entry must be unable to key or to
        // change the radio. Do not edit it to make a test pass. Compared as slices so that a
        // longer list fails here with both lists printed, not as a type error.
        assert_eq!(
            ALLOWED.as_slice(),
            [
                "rx_sensors_enable",
                "tx_sensors_enable",
                "audio_start",
                "audio_stop",
                "iq_start",
                "iq_stop",
                "cw_macros_speed",
                "cw_macros_delay",
                "volume",
                "mon_volume",
                "digl_offset",
                "digu_offset",
            ]
            .as_slice()
        );
    }

    #[test]
    fn every_command_shape_is_admitted_and_the_list_has_no_spare_entry() {
        let shapes = every_shape();
        for shape in &shapes {
            let text = shape.render();
            assert_eq!(admit(&text), Ok(()), "{text:?}");
            assert!(ALLOWED.contains(&shape.name()));
        }
        let used: BTreeSet<&str> = shapes.iter().map(Observe::name).collect();
        assert_eq!(used, ALLOWED.into_iter().collect::<BTreeSet<_>>());
    }

    #[test]
    fn no_command_that_keys_or_changes_anything_is_admitted() {
        for command in FORBIDDEN {
            assert!(admit(command).is_err(), "{command:?} was admitted");
        }
        // A second command smuggled into one, in every way the grammar allows.
        for smuggled in [
            "volume;trx:0,true;",
            "volume;\ntrx:0,true;",
            "audio_start:0;trx:0,true,tci;",
            "audio_start:0,trx:0,true;",
            "volume;;",
            "volume\u{0};",
            "volume\u{e9};",
            " volume;",
            "VOLUME;",
            "volume:",
        ] {
            assert!(admit(smuggled).is_err(), "{smuggled:?} was admitted");
        }
        // The control: the same check admits exactly what the recorder sends.
        for allowed in [
            "volume;",
            "audio_start:0;",
            "rx_sensors_enable:true,200;",
            "tx_sensors_enable:false;",
        ] {
            assert_eq!(admit(allowed), Ok(()), "{allowed:?}");
        }
    }

    fn options(audio: Option<u8>, iq: Option<u8>, payload: usize) -> Options {
        Options {
            duration: Duration::from_millis(400),
            audio,
            iq,
            payload_frames: payload,
            sensor_interval_ms: 100,
            timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn a_recording_sends_only_allowed_commands_never_answers_a_chrono_and_writes_the_session() {
        // Another program keys, with its chronos sent to every client whose audio runs: the
        // recorder sees them and must send nothing back.
        let sim =
            Simulator::start(
                Config::dialect(Dialect::expert_sdr3()).with([Fault::ForeignKey {
                    after: Duration::from_millis(250),
                    hold: Duration::from_millis(250),
                    chrono_to_all: true,
                }]),
            )
            .unwrap();
        let mut file = Vec::new();
        let mut o = options(Some(0), Some(0), 2);
        o.duration = Duration::from_millis(700);
        let summary = record(sim.addr(), &o, &mut file).unwrap();
        let text = String::from_utf8(file).unwrap();

        // 1. What reached the simulator: only admitted commands, every one of them, and no frame.
        assert!(sim.wait_for(Duration::from_secs(5), |log| log
            .iter()
            .any(|l| matches!(l.event, Event::Closed { .. }))));
        let sent = sim.commands();
        assert_eq!(sent.len(), summary.commands);
        for command in &sent {
            assert_eq!(
                admit(&format!("{command};")),
                Ok(()),
                "the recorder sent {command:?}"
            );
        }
        let names: BTreeSet<&str> = sent.iter().map(|c| c.split(':').next().unwrap()).collect();
        assert_eq!(names, ALLOWED.into_iter().collect::<BTreeSet<_>>());
        assert!(
            !sim.events()
                .iter()
                .any(|e| matches!(e, Event::FrameIn { .. })),
            "no frame sent"
        );
        assert!(
            summary.chronos > 0,
            "the foreign key's chronos reached the recorder"
        );
        assert!(sim.connections().is_empty(), "the recorder closed");

        // 2. The file: the burst, the commands, frame headers, two payloads of each kind, the end.
        let session = Session::parse(&text).unwrap();
        assert_eq!(session.port, Some(sim.addr().port()));
        assert!(summary.ready && !summary.server_closed);
        assert!(text.contains(" vfo_limits:10000,30000000;\n"));
        assert!(text.contains(" audio_start:0;\n"));
        let payloads: Vec<&Entry> = session
            .entries
            .iter()
            .filter(|e| matches!(e, Entry::Payload { .. }))
            .collect();
        assert_eq!(payloads.len(), 4, "two receive audio and two IQ payloads");
        assert!(text.contains("\nend "));
        assert!(!text.contains("127.0.0.1"), "the address is never written");
        assert!(summary.frames > 10);
    }

    #[test]
    fn addresses_a_server_sends_are_replaced_before_anything_is_written() {
        let mut dialect = Dialect::expert_sdr3();
        dialect
            .initial
            .extra
            .push("x_vendor_ip:192.168.1.20;".into());
        dialect.initial.extra.push("device_serial:SN-0042;".into());
        let sim = Simulator::start(Config::dialect(dialect)).unwrap();
        let mut file = Vec::new();
        let summary = record(sim.addr(), &options(None, None, 0), &mut file).unwrap();
        let text = String::from_utf8(file).unwrap();
        for secret in ["192.168.1.20", "SN-0042", "127.0.0.1"] {
            assert!(!text.contains(secret), "{secret} reached the recording");
        }
        assert!(text.contains("x_vendor_ip:192.0.2.1;"));
        assert!(text.contains("device_serial:serial-1;"));
        let kinds: Vec<Kind> = summary.scrubbed.iter().map(|(k, _)| *k).collect();
        assert_eq!(kinds, [Kind::Ipv4, Kind::Serial]);
    }

    #[test]
    fn a_recording_of_every_profile_replays_its_burst_message_for_message() {
        // Only the burst matters here: a short observation.
        let mut short = options(None, None, 0);
        short.duration = Duration::from_millis(50);
        for dialect in Dialect::all() {
            let sim = Simulator::start(Config::dialect(dialect.clone())).unwrap();
            let mut file = Vec::new();
            record(sim.addr(), &short, &mut file).unwrap();
            let session = Session::parse(&String::from_utf8(file).unwrap()).unwrap();
            let replayed = session.dialect();
            assert_eq!(replayed.program, dialect.program, "{}", dialect.name);
            assert_eq!(
                replayed.identity_order, dialect.identity_order,
                "{}",
                dialect.name
            );
            let replay = Simulator::start(Config::dialect(replayed)).unwrap();
            let mut file = Vec::new();
            record(replay.addr(), &short, &mut file).unwrap();
            let again = Session::parse(&String::from_utf8(file).unwrap()).unwrap();
            assert_eq!(again.burst(), session.burst(), "{}", dialect.name);
        }
    }

    #[test]
    fn a_server_that_never_says_ready_is_recorded_and_says_so() {
        let sim = Simulator::start(Config::dialect(Dialect::expert_sdr3()).with([Fault::NoReady]))
            .unwrap();
        let mut file = Vec::new();
        let mut o = options(None, None, 0);
        o.timeout = Duration::from_millis(300);
        let summary = record(sim.addr(), &o, &mut file).unwrap();
        assert!(!summary.ready);
        let text = String::from_utf8(file).unwrap();
        assert!(text.contains(" ms: no ready; within 300 ms\n"), "{text}");
        assert!(Session::parse(&text).is_ok(), "a remark is a comment line");
    }
}
