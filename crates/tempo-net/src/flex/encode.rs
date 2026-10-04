//! The typed command encoder: every command Nexus can send to the radio, as a value, rendered to
//! the wire text the radio expects.
//!
//! There are three kinds, and the type says which:
//!
//! - [`Command`]: everything that cannot key the transmitter: registration, keepalive,
//!   subscriptions, queries, slices, display objects, stream removal, ATU bypass. [`render`]
//!   turns it into text.
//! - [`TxStart`]: what can key the transmitter (`xmit 1`, `transmit tune 1`, `atu start`,
//!   `cwx send`). It reaches the wire only through [`render_start`], which takes an
//!   [`Admitted`], and only `super::admission::admit` constructs one. Nothing else in Nexus can
//!   produce the text of a keying command.
//! - [`TxStop`]: what ends a transmission (`xmit 0`, `transmit tune 0`, `cwx clear`). Never
//!   gated: [`render_stop`] takes the stop alone.
//!
//! Each [`Rendered`] command carries its [`Kind`] and the object it is aimed at
//! ([`Rendered::target`]). The session uses the kind to tell the readback which write was the
//! key and which the unkey, and the target to refuse a command aimed at an object that is not
//! ours. Nothing anywhere classifies a command by matching its text.
//!
//! Not representable here, so nothing in Nexus can send them: `slice set <n> tx=1` (moving the
//! transmit slice), power and other `transmit set` writes, DAX streams and `transmit set dax`,
//! `cw key`/`cw ptt`, `dvk`, slice `play`, amplifier and tuner relays, waveform commands,
//! `interlock` writes, TNF commands, and any raw text. Later changes add what they need as typed
//! variants.
//!
//! PORTED from AetherSDR (https://github.com/aethersdr/AetherSDR, GPL-3.0; the upstream file
//! carries no per-file header, the licence is the repository's), the verb encodings of
//! `src/core/backends/flex/FlexBackend.cpp` at commit `32fa50e4896a846a6970fa3f443bd49d667c139d`
//! (2026-10-03), translated from C++/Qt to Rust. The connect-sequence commands, `slice create`
//! and `display panafall create` follow `src/models/RadioModel.cpp`, and the CWX space encoding
//! follows `src/models/CwxModel.cpp`, both read as protocol facts; no code is taken from either.
//! Deliberate differences: typed intents in place of command strings, so a keying command can
//! only be rendered from an admission; the slice and transmit sinks and the operation-fenced TX
//! sink are replaced by the kind and target each rendered command carries; values are validated
//! before rendering (a frequency must be positive and finite, a filter's low edge below its high,
//! levels in range); free text is restricted to a safe token, so no command can carry a line
//! break or a `|`. Recorded in the repo-root NOTICE (AetherSDR entry).

use std::fmt;

use super::admission::Admitted;
use super::handshake::protocol_safe_station;

/// A subscription topic (`sub <topic> all`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    Slice,
    Pan,
    /// Transmit status, which carries the interlock.
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
    fn word(self) -> &'static str {
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

/// A station or program name as the protocol carries it unquoted: no whitespace, `|`, `=` or
/// control character, at most 48 characters, never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Station(String);

impl Station {
    /// Sanitize `raw` (whitespace and unsafe runs become `-`); `None` if nothing is left.
    pub fn new(raw: &str) -> Option<Station> {
        let safe = protocol_safe_station(raw);
        (!safe.is_empty()).then_some(Station(safe))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A GUI client id as the radio issues it (a UUID): hex digits and dashes, 1 to 64 of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientId(String);

impl ClientId {
    pub fn parse(text: &str) -> Option<ClientId> {
        let ok = !text.is_empty()
            && text.len() <= 64
            && text.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
        ok.then(|| ClientId(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A demodulation mode token (`USB`, `DIGU`, `CW`, `FM`, …): one to eight uppercase letters,
/// digits or underscores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mode(String);

impl Mode {
    pub fn new(text: &str) -> Option<Mode> {
        let ok = !text.is_empty()
            && text.len() <= 8
            && text
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
        ok.then(|| Mode(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A slice's AGC mode (D `TCPIP-slice`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgcMode {
    Off,
    Slow,
    Med,
    Fast,
}

impl AgcMode {
    fn word(self) -> &'static str {
        match self {
            AgcMode::Off => "off",
            AgcMode::Slow => "slow",
            AgcMode::Med => "med",
            AgcMode::Fast => "fast",
        }
    }
}

/// Text for the radio's CW keyer. Whitespace runs collapse to one space (a macro's line breaks
/// key as a word gap); printable ASCII only, and no `"` or `|`, which would break the quoting or
/// the frame. Spaces go on the wire as byte `0x7f` (A, `CwxModel.cpp`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CwxText(String);

impl CwxText {
    pub fn new(text: &str) -> Option<CwxText> {
        let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let ok = !collapsed.is_empty()
            && collapsed
                .bytes()
                .all(|b| (b' '..=b'~').contains(&b) && b != b'"' && b != b'|');
        ok.then_some(CwxText(collapsed))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A command that cannot key the transmitter.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    // ── Registration and session (port plan §4.5) ──
    /// `client program <name>`.
    ClientProgram(Station),
    /// `client low_bw_connect`: only between `program` and `gui`.
    ClientLowBwConnect,
    /// `client gui [<client id>]`: with the stored id, the radio restores this client's slices.
    ClientGui(Option<ClientId>),
    /// `client station <name>`.
    ClientStation(Station),
    /// `client set send_reduced_bw_dax=1`.
    ClientSetReducedBwDax,
    /// `client set enforce_network_mtu=1 network_mtu=<n>`, the VITA-49 packet size.
    ClientSetNetworkMtu(u16),
    /// `client udpport <port>`.
    ClientUdpPort(u16),
    /// `keepalive enable`: the radio drops a client silent for 15 s.
    KeepaliveEnable,
    Ping,
    /// `sub <topic> all`.
    Subscribe(Topic),
    Info,
    Version,
    SliceList,
    MicList,
    MeterList,
    // ── Slices ──
    /// `slice create [pan=<pan>] freq=<MHz> [mode=<mode>]`.
    SliceCreate {
        pan: Option<u32>,
        freq_hz: f64,
        mode: Option<Mode>,
    },
    /// `slice remove <n>`.
    SliceRemove {
        slice: u8,
    },
    /// `slice tune <n> <MHz> [autopan=0]`; `keep_pan` holds the pan where it is.
    SliceTune {
        slice: u8,
        freq_hz: f64,
        keep_pan: bool,
    },
    /// `slice set <n> mode=<mode>`.
    SliceMode {
        slice: u8,
        mode: Mode,
    },
    /// `filt <n> <low Hz> <high Hz>`.
    SliceFilter {
        slice: u8,
        low_hz: i32,
        high_hz: i32,
    },
    /// `slice set <n> agc_mode=<mode>`.
    SliceAgcMode {
        slice: u8,
        mode: AgcMode,
    },
    /// `slice set <n> agc_threshold=<0–100>`.
    SliceAgcThreshold {
        slice: u8,
        level: i32,
    },
    /// `slice set <n> agc_off_level=<0–100>`.
    SliceAgcOffLevel {
        slice: u8,
        level: i32,
    },
    // ── Display ──
    /// `display panafall create x=<w> y=<h>`.
    PanafallCreate {
        x: u16,
        y: u16,
    },
    /// `display pan set <pan> center=<MHz>`.
    PanCenter {
        pan: u32,
        freq_hz: f64,
    },
    /// `display pan remove <pan>`. The radio keeps the pan's waterfall: remove that too.
    PanRemove {
        pan: u32,
    },
    /// `display panafall remove <waterfall>`.
    WaterfallRemove {
        waterfall: u32,
    },
    // ── Streams ──
    /// `stream remove 0x<id>`.
    StreamRemove {
        stream: u32,
    },
    // ── Transmit chain, not keying ──
    /// `atu bypass`.
    AtuBypass,
}

/// A command that can key the transmitter. Rendered only through [`render_start`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxStart {
    /// `xmit 1`.
    Key,
    /// `transmit tune 1`: the radio's tune carrier at its tune power.
    TuneOn,
    /// `atu start`: the radio's ATU keys a tune cycle.
    AtuStart,
    /// `cwx send "<text>" <block>`: the radio's keyer sends the text.
    CwxSend(CwxText),
}

/// A command that ends a transmission. Never gated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxStop {
    /// `xmit 0`.
    Unkey,
    /// `transmit tune 0`.
    TuneOff,
    /// `cwx clear`.
    CwxClear,
}

/// Which start a [`TxStart`] is, without its data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartKind {
    Key,
    Tune,
    Atu,
    Cwx,
}

impl TxStart {
    pub fn kind(&self) -> StartKind {
        match self {
            TxStart::Key => StartKind::Key,
            TxStart::TuneOn => StartKind::Tune,
            TxStart::AtuStart => StartKind::Atu,
            TxStart::CwxSend(_) => StartKind::Cwx,
        }
    }
}

/// What a rendered command is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// It cannot key the transmitter.
    Ordinary,
    /// It can.
    Start(StartKind),
    /// It ends a transmission.
    Stop(TxStop),
}

/// The object a command acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Slice(u8),
    Pan(u32),
    Waterfall(u32),
    Stream(u32),
}

/// A command rendered to wire text. Constructed only in this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    text: String,
    kind: Kind,
    target: Option<Target>,
}

impl Rendered {
    /// The command text, without sequence number or terminator.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn target(&self) -> Option<Target> {
        self.target
    }

    fn ordinary(text: String, target: Option<Target>) -> Rendered {
        Rendered {
            text,
            kind: Kind::Ordinary,
            target,
        }
    }
}

/// Why a command could not be rendered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EncodeError {
    /// Not positive and finite, or beyond any radio's tuning range.
    Frequency(f64),
    /// The low edge is not below the high edge.
    Filter { low_hz: i32, high_hz: i32 },
    /// Outside 0–100.
    Level(i32),
    /// Outside 576–9000 bytes.
    Mtu(u16),
    /// Port zero.
    Port,
    /// A pan dimension of zero.
    Pixels,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for EncodeError {}

/// Hz to the wire's MHz with six decimals.
fn mhz(freq_hz: f64) -> Result<String, EncodeError> {
    // 100 GHz is far above any Flex coverage, transverters included; the check is against
    // nonsense (NaN, infinity, negative, zero), not a band plan.
    if !freq_hz.is_finite() || freq_hz <= 0.0 || freq_hz > 100e9 {
        return Err(EncodeError::Frequency(freq_hz));
    }
    Ok(format!("{:.6}", freq_hz / 1e6))
}

fn level(value: i32) -> Result<i32, EncodeError> {
    if (0..=100).contains(&value) {
        Ok(value)
    } else {
        Err(EncodeError::Level(value))
    }
}

/// The object id as the radio's commands take it.
fn id(value: u32) -> String {
    format!("0x{value:08x}")
}

impl Command {
    /// The object this command acts on, if any. A slice created on a pan acts on that pan.
    pub fn target(&self) -> Option<Target> {
        match self {
            Command::SliceRemove { slice }
            | Command::SliceTune { slice, .. }
            | Command::SliceMode { slice, .. }
            | Command::SliceFilter { slice, .. }
            | Command::SliceAgcMode { slice, .. }
            | Command::SliceAgcThreshold { slice, .. }
            | Command::SliceAgcOffLevel { slice, .. } => Some(Target::Slice(*slice)),
            Command::SliceCreate { pan: Some(pan), .. }
            | Command::PanCenter { pan, .. }
            | Command::PanRemove { pan } => Some(Target::Pan(*pan)),
            Command::WaterfallRemove { waterfall } => Some(Target::Waterfall(*waterfall)),
            Command::StreamRemove { stream } => Some(Target::Stream(*stream)),
            Command::ClientProgram(_)
            | Command::ClientLowBwConnect
            | Command::ClientGui(_)
            | Command::ClientStation(_)
            | Command::ClientSetReducedBwDax
            | Command::ClientSetNetworkMtu(_)
            | Command::ClientUdpPort(_)
            | Command::KeepaliveEnable
            | Command::Ping
            | Command::Subscribe(_)
            | Command::Info
            | Command::Version
            | Command::SliceList
            | Command::MicList
            | Command::MeterList
            | Command::SliceCreate { pan: None, .. }
            | Command::PanafallCreate { .. }
            | Command::AtuBypass => None,
        }
    }
}

/// Render a command that cannot key the transmitter.
pub fn render(command: &Command) -> Result<Rendered, EncodeError> {
    let text = match command {
        Command::ClientProgram(name) => format!("client program {}", name.as_str()),
        Command::ClientLowBwConnect => "client low_bw_connect".to_string(),
        Command::ClientGui(None) => "client gui".to_string(),
        Command::ClientGui(Some(id)) => format!("client gui {}", id.as_str()),
        Command::ClientStation(name) => format!("client station {}", name.as_str()),
        Command::ClientSetReducedBwDax => "client set send_reduced_bw_dax=1".to_string(),
        Command::ClientSetNetworkMtu(mtu) => {
            if !(576..=9000).contains(mtu) {
                return Err(EncodeError::Mtu(*mtu));
            }
            format!("client set enforce_network_mtu=1 network_mtu={mtu}")
        }
        Command::ClientUdpPort(port) => {
            if *port == 0 {
                return Err(EncodeError::Port);
            }
            format!("client udpport {port}")
        }
        Command::KeepaliveEnable => "keepalive enable".to_string(),
        Command::Ping => "ping".to_string(),
        Command::Subscribe(topic) => format!("sub {} all", topic.word()),
        Command::Info => "info".to_string(),
        Command::Version => "version".to_string(),
        Command::SliceList => "slice list".to_string(),
        Command::MicList => "mic list".to_string(),
        Command::MeterList => "meter list".to_string(),
        Command::SliceCreate { pan, freq_hz, mode } => {
            let mut text = "slice create".to_string();
            if let Some(pan) = pan {
                text.push_str(&format!(" pan={}", id(*pan)));
            }
            text.push_str(&format!(" freq={}", mhz(*freq_hz)?));
            if let Some(mode) = mode {
                text.push_str(&format!(" mode={}", mode.as_str()));
            }
            text
        }
        Command::SliceRemove { slice } => format!("slice remove {slice}"),
        Command::SliceTune {
            slice,
            freq_hz,
            keep_pan,
        } => {
            // FlexLib 4.2.18 Slice.Freq, as upstream cites it: MHz to six places, autopan=0 only
            // when the pan is to stay put.
            let mut text = format!("slice tune {slice} {}", mhz(*freq_hz)?);
            if *keep_pan {
                text.push_str(" autopan=0");
            }
            text
        }
        Command::SliceMode { slice, mode } => format!("slice set {slice} mode={}", mode.as_str()),
        Command::SliceFilter {
            slice,
            low_hz,
            high_hz,
        } => {
            if low_hz >= high_hz {
                return Err(EncodeError::Filter {
                    low_hz: *low_hz,
                    high_hz: *high_hz,
                });
            }
            format!("filt {slice} {low_hz} {high_hz}")
        }
        // Each AGC setter writes only its own field (FlexLib 4.2.18 Slice, as upstream cites it).
        Command::SliceAgcMode { slice, mode } => {
            format!("slice set {slice} agc_mode={}", mode.word())
        }
        Command::SliceAgcThreshold { slice, level: l } => {
            format!("slice set {slice} agc_threshold={}", level(*l)?)
        }
        Command::SliceAgcOffLevel { slice, level: l } => {
            format!("slice set {slice} agc_off_level={}", level(*l)?)
        }
        Command::PanafallCreate { x, y } => {
            if *x == 0 || *y == 0 {
                return Err(EncodeError::Pixels);
            }
            format!("display panafall create x={x} y={y}")
        }
        Command::PanCenter { pan, freq_hz } => {
            format!("display pan set {} center={}", id(*pan), mhz(*freq_hz)?)
        }
        Command::PanRemove { pan } => format!("display pan remove {}", id(*pan)),
        Command::WaterfallRemove { waterfall } => {
            format!("display panafall remove {}", id(*waterfall))
        }
        Command::StreamRemove { stream } => format!("stream remove {}", id(*stream)),
        Command::AtuBypass => "atu bypass".to_string(),
    };
    Ok(Rendered::ordinary(text, command.target()))
}

/// Render an admitted start. The only way a keying command's text comes to exist. `cwx_block` is
/// the session's next CWX block number.
pub fn render_start(admitted: Admitted, cwx_block: u32) -> Rendered {
    let start = admitted.into_start();
    let kind = start.kind();
    let text = match start {
        TxStart::Key => "xmit 1".to_string(),
        TxStart::TuneOn => "transmit tune 1".to_string(),
        TxStart::AtuStart => "atu start".to_string(),
        TxStart::CwxSend(text) => format!(
            "cwx send \"{}\" {cwx_block}",
            text.as_str().replace(' ', "\u{7f}")
        ),
    };
    Rendered {
        text,
        kind: Kind::Start(kind),
        target: None,
    }
}

/// Render a stop. Never gated: it takes nothing but the stop.
pub fn render_stop(stop: TxStop) -> Rendered {
    let text = match stop {
        TxStop::Unkey => "xmit 0",
        TxStop::TuneOff => "transmit tune 0",
        TxStop::CwxClear => "cwx clear",
    };
    Rendered {
        text: text.to_string(),
        kind: Kind::Stop(stop),
        target: None,
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// One of every command, each variant spelled out so that a new variant fails to compile here
    /// until it is listed (the match below has no wildcard).
    pub(in crate::flex) fn every_command() -> Vec<Command> {
        let station = Station::new("Nexus").unwrap();
        let mode = Mode::new("USB").unwrap();
        let all = vec![
            Command::ClientProgram(station.clone()),
            Command::ClientLowBwConnect,
            Command::ClientGui(None),
            Command::ClientGui(ClientId::parse("6F1C2A3B-0000-4000-8000-00000000B001")),
            Command::ClientStation(station),
            Command::ClientSetReducedBwDax,
            Command::ClientSetNetworkMtu(1450),
            Command::ClientUdpPort(4993),
            Command::KeepaliveEnable,
            Command::Ping,
            Command::Subscribe(Topic::Slice),
            Command::Subscribe(Topic::Pan),
            Command::Subscribe(Topic::Tx),
            Command::Subscribe(Topic::Atu),
            Command::Subscribe(Topic::Meter),
            Command::Subscribe(Topic::Audio),
            Command::Subscribe(Topic::Gps),
            Command::Subscribe(Topic::Client),
            Command::Subscribe(Topic::Radio),
            Command::Subscribe(Topic::Xvtr),
            Command::Info,
            Command::Version,
            Command::SliceList,
            Command::MicList,
            Command::MeterList,
            Command::SliceCreate {
                pan: Some(0x4000_0000),
                freq_hz: 14_074_000.0,
                mode: Some(mode.clone()),
            },
            Command::SliceCreate {
                pan: None,
                freq_hz: 7_074_000.0,
                mode: None,
            },
            Command::SliceRemove { slice: 1 },
            Command::SliceTune {
                slice: 0,
                freq_hz: 14_074_000.0,
                keep_pan: true,
            },
            Command::SliceMode { slice: 0, mode },
            Command::SliceFilter {
                slice: 0,
                low_hz: 100,
                high_hz: 2900,
            },
            Command::SliceAgcMode {
                slice: 0,
                mode: AgcMode::Med,
            },
            Command::SliceAgcThreshold {
                slice: 0,
                level: 65,
            },
            Command::SliceAgcOffLevel {
                slice: 0,
                level: 10,
            },
            Command::PanafallCreate { x: 1024, y: 480 },
            Command::PanCenter {
                pan: 0x4000_0000,
                freq_hz: 14_100_000.0,
            },
            Command::PanRemove { pan: 0x4000_0000 },
            Command::WaterfallRemove {
                waterfall: 0x4200_0000,
            },
            Command::StreamRemove {
                stream: 0x0400_000A,
            },
            Command::AtuBypass,
        ];
        for c in &all {
            // Exhaustiveness: adding a variant breaks this match until the list covers it.
            match c {
                Command::ClientProgram(_)
                | Command::ClientLowBwConnect
                | Command::ClientGui(_)
                | Command::ClientStation(_)
                | Command::ClientSetReducedBwDax
                | Command::ClientSetNetworkMtu(_)
                | Command::ClientUdpPort(_)
                | Command::KeepaliveEnable
                | Command::Ping
                | Command::Subscribe(_)
                | Command::Info
                | Command::Version
                | Command::SliceList
                | Command::MicList
                | Command::MeterList
                | Command::SliceCreate { .. }
                | Command::SliceRemove { .. }
                | Command::SliceTune { .. }
                | Command::SliceMode { .. }
                | Command::SliceFilter { .. }
                | Command::SliceAgcMode { .. }
                | Command::SliceAgcThreshold { .. }
                | Command::SliceAgcOffLevel { .. }
                | Command::PanafallCreate { .. }
                | Command::PanCenter { .. }
                | Command::PanRemove { .. }
                | Command::WaterfallRemove { .. }
                | Command::StreamRemove { .. }
                | Command::AtuBypass => {}
            }
        }
        all
    }

    /// The command words that key a Flex (port plan §3.2), as an independent oracle for tests.
    /// Used only to check output; nothing in Nexus decides anything by matching text.
    pub(in crate::flex) fn is_keying_text(text: &str) -> bool {
        text == "xmit 1"
            || text.starts_with("xmit 1 ")
            || text.starts_with("transmit tune 1")
            || text.starts_with("transmit tune on")
            || text.starts_with("atu start")
            || text.starts_with("cwx send")
            || text.starts_with("cw key")
            || text.starts_with("cw ptt")
            || text.contains(" tx=1")
            || text.starts_with("dvk")
            || text.starts_with("stream create type=dax_tx")
            || text.starts_with("transmit set dax=1")
    }

    #[test]
    fn no_ordinary_command_can_key_the_transmitter() {
        let all = every_command();
        assert!(all.len() >= 40, "the list covers every variant");
        for c in &all {
            let r = render(c).unwrap_or_else(|e| panic!("{c:?}: {e}"));
            assert_eq!(r.kind(), Kind::Ordinary, "{c:?}");
            assert!(!is_keying_text(r.text()), "{c:?} renders {:?}", r.text());
            assert!(
                !r.text().contains(['\n', '\r', '|', '\0']),
                "{c:?} renders a frame breaker: {:?}",
                r.text()
            );
        }
    }

    #[test]
    fn the_wire_text_of_each_command() {
        let text = |c: Command| render(&c).unwrap().text().to_string();
        let station = |s: &str| Station::new(s).unwrap();
        assert_eq!(
            text(Command::ClientProgram(station("Nexus"))),
            "client program Nexus"
        );
        assert_eq!(text(Command::ClientGui(None)), "client gui");
        assert_eq!(
            text(Command::ClientGui(ClientId::parse("6F1C-00B1"))),
            "client gui 6F1C-00B1"
        );
        assert_eq!(
            text(Command::ClientSetNetworkMtu(1450)),
            "client set enforce_network_mtu=1 network_mtu=1450"
        );
        assert_eq!(text(Command::Subscribe(Topic::Tx)), "sub tx all");
        assert_eq!(
            text(Command::SliceTune {
                slice: 0,
                freq_hz: 14_074_000.0,
                keep_pan: true
            }),
            "slice tune 0 14.074000 autopan=0"
        );
        assert_eq!(
            text(Command::SliceTune {
                slice: 2,
                freq_hz: 7_074_500.0,
                keep_pan: false
            }),
            "slice tune 2 7.074500"
        );
        assert_eq!(
            text(Command::SliceCreate {
                pan: Some(0x4000_0000),
                freq_hz: 14_225_000.0,
                mode: Mode::new("USB")
            }),
            "slice create pan=0x40000000 freq=14.225000 mode=USB"
        );
        assert_eq!(
            text(Command::SliceFilter {
                slice: 1,
                low_hz: -2700,
                high_hz: -100
            }),
            "filt 1 -2700 -100"
        );
        assert_eq!(
            text(Command::SliceAgcThreshold {
                slice: 3,
                level: 65
            }),
            "slice set 3 agc_threshold=65"
        );
        assert_eq!(
            text(Command::PanCenter {
                pan: 0x4000_000A,
                freq_hz: 14_100_000.0
            }),
            "display pan set 0x4000000a center=14.100000"
        );
        assert_eq!(
            text(Command::WaterfallRemove {
                waterfall: 0x4200_0000
            }),
            "display panafall remove 0x42000000"
        );
        assert_eq!(
            text(Command::StreamRemove {
                stream: 0x0400_000A
            }),
            "stream remove 0x0400000a"
        );
    }

    #[test]
    fn values_are_validated_before_rendering() {
        for f in [0.0, -1.0, f64::NAN, f64::INFINITY, 2e11] {
            assert!(
                render(&Command::SliceTune {
                    slice: 0,
                    freq_hz: f,
                    keep_pan: false
                })
                .is_err(),
                "{f}"
            );
        }
        assert!(render(&Command::SliceFilter {
            slice: 0,
            low_hz: 3000,
            high_hz: 100
        })
        .is_err());
        assert!(render(&Command::SliceAgcThreshold {
            slice: 0,
            level: 101
        })
        .is_err());
        assert!(render(&Command::ClientSetNetworkMtu(100)).is_err());
        assert!(render(&Command::ClientUdpPort(0)).is_err());
        assert!(render(&Command::PanafallCreate { x: 0, y: 10 }).is_err());
    }

    #[test]
    fn free_text_is_a_safe_token() {
        assert_eq!(
            Station::new(" Shack PC|x=1 ").unwrap().as_str(),
            "Shack-PC-x-1"
        );
        assert_eq!(Station::new("  \t "), None);
        assert_eq!(ClientId::parse("not a uuid"), None);
        assert_eq!(ClientId::parse(""), None);
        assert_eq!(Mode::new("usb"), None);
        assert_eq!(Mode::new("DIGU").unwrap().as_str(), "DIGU");
        assert_eq!(Mode::new("USB LSB"), None);
        assert_eq!(
            CwxText::new("cq  cq\nde N0CALL").unwrap().as_str(),
            "cq cq de N0CALL"
        );
        assert_eq!(CwxText::new("say \"hi\""), None);
        assert_eq!(CwxText::new("a|b"), None);
        assert_eq!(CwxText::new("   "), None);
        assert_eq!(CwxText::new("café"), None);
    }

    #[test]
    fn stops_render_without_any_condition() {
        assert_eq!(render_stop(TxStop::Unkey).text(), "xmit 0");
        assert_eq!(render_stop(TxStop::TuneOff).text(), "transmit tune 0");
        assert_eq!(render_stop(TxStop::CwxClear).text(), "cwx clear");
        for stop in [TxStop::Unkey, TxStop::TuneOff, TxStop::CwxClear] {
            let r = render_stop(stop);
            assert_eq!(r.kind(), Kind::Stop(stop));
            assert!(!is_keying_text(r.text()));
        }
    }

    #[test]
    fn a_start_renders_only_from_an_admission() {
        // The test-only door into admission mints the value; production has one door, `admit`.
        for (start, text) in [
            (TxStart::Key, "xmit 1"),
            (TxStart::TuneOn, "transmit tune 1"),
            (TxStart::AtuStart, "atu start"),
            (
                TxStart::CwxSend(CwxText::new("CQ DE N0CALL").unwrap()),
                "cwx send \"CQ\u{7f}DE\u{7f}N0CALL\" 7",
            ),
        ] {
            let kind = start.kind();
            let r = render_start(Admitted::for_test(start), 7);
            assert_eq!(r.text(), text);
            assert_eq!(r.kind(), Kind::Start(kind));
            assert!(
                is_keying_text(r.text()),
                "the oracle knows every start: {text}"
            );
        }
    }
}
