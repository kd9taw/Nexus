//! SDRplay's SDRconnect over its WebSocket API: the protocol core and one connection, for the RF
//! scope's "SDRconnect" source. Receive only.
//!
//! ## The specification
//! Implemented from SDRplay's published "SDRconnect WebSocket API 1.0.3" ([`SPEC`]); no SDRplay
//! code, header or example was used, and nothing of SDRplay's is shipped or loaded. SDRconnect is a
//! separate program the operator installs; Nexus talks to it over a socket, as it does to an SDR
//! program over TCI.
//! - Text messages are one JSON object each: `event_type`, `property`, `value` (always a string)
//!   and an optional `device` (`primary` or `secondary`; absent means primary).
//! - SDRconnect pushes `property_changed` and answers `get_property` with `get_property_response`.
//! - A binary message is a little-endian u16 type, then its payload. Type 2 is the primary
//!   receiver's IQ, signed 16-bit little-endian, interleaved I then Q. The others (audio, the
//!   secondary receiver, SDRconnect's display-scaled FFT bins) are not used.
//! - The IQ's rate is the receiver's `device_sample_rate` and its centre `device_center_frequency`,
//!   each as last reported before the samples, in stream order. The specification does not say
//!   whether the IQ on the socket is at the device rate; a stream that arrives slower than the
//!   reported rate is seen as lost samples by the scope and is not drawn.
//!
//! ## Receive only, by construction
//! [`Outbound`] is everything Nexus can send, and [`Link::send`] takes nothing else: property
//! reads, the receiver's centre frequency, the IQ stream on or off, and starting the receiver's
//! stream. There is no variant for audio, recording, device profiles or any other property, and
//! SDRconnect's protocol has no transmit of any kind: the RSP receivers it serves cannot transmit.
//! The connection holds no radio and no CAT; the rig is tuned only by the operator and Nexus's own
//! CAT path, never from here.
//!
//! ## Bounds
//! What SDRconnect sends is outside data: a message is at most [`MAX_MESSAGE`] bytes, a text that
//! is not the envelope, a value that does not parse, a binary message too short for its type or
//! with half an IQ pair is reported as such, never a panic.
//!
//! Nexus's own design, not a port.

use std::io::{self, ErrorKind};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tungstenite::error::ProtocolError;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Bytes, Message, WebSocket};

/// The specification this is written from.
pub const SPEC: &str = "SDRconnect WebSocket API 1.0.3";

/// The largest message taken from SDRconnect. An IQ message at the RSP's top rate is far smaller.
pub const MAX_MESSAGE: usize = 4 << 20;

/// The highest IQ rate taken as a receiver's: the RSP family samples at up to 10.66 MHz.
pub const MAX_RATE_HZ: u32 = 20_000_000;

/// How long the TCP connect, and the WebSocket handshake, may each take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// How long one message may take to write before the connection counts as gone.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// The binary message type of the primary receiver's IQ.
pub const IQ_PRIMARY: u16 = 2;

/// The properties Nexus reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prop {
    /// `api_version`.
    ApiVersion,
    /// `device_center_frequency`: the receiver's LO, the IQ's centre (Hz).
    CentreFrequency,
    /// `device_sample_rate`: the receiver's sample rate, the IQ's rate (Hz).
    SampleRate,
    /// `started`: whether the receiver is streaming.
    Started,
    /// `can_control`: whether this client may set the receiver's hardware.
    CanControl,
}

impl Prop {
    /// Every property Nexus reads.
    pub const ALL: [Prop; 5] = [
        Prop::ApiVersion,
        Prop::CentreFrequency,
        Prop::SampleRate,
        Prop::Started,
        Prop::CanControl,
    ];

    /// Its name on the wire.
    pub const fn wire(self) -> &'static str {
        match self {
            Prop::ApiVersion => "api_version",
            Prop::CentreFrequency => "device_center_frequency",
            Prop::SampleRate => "device_sample_rate",
            Prop::Started => "started",
            Prop::CanControl => "can_control",
        }
    }

    fn from_wire(name: &str) -> Option<Prop> {
        Prop::ALL.into_iter().find(|p| p.wire() == name)
    }
}

/// Everything Nexus ever sends to SDRconnect. Nothing else can be sent: [`Link::send`] takes only
/// this, and the JSON is written only by [`Outbound::to_json`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outbound {
    /// `get_property` of the primary receiver.
    Get(Prop),
    /// `set_property device_center_frequency` of the primary receiver (Hz): the one setting Nexus
    /// makes, so that the scope follows the rig's dial.
    SetCentre(u64),
    /// `iq_stream_enable`: IQ over the socket on or off.
    IqStream(bool),
    /// `device_stream_enable true`: start the selected receiver's stream, when SDRconnect reports
    /// it stopped (headless SDRconnect starts with none).
    StartStream,
}

/// The JSON envelope, both ways.
#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    event_type: String,
    #[serde(default)]
    property: String,
    #[serde(default)]
    value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    device: Option<String>,
}

impl Outbound {
    /// The message on the wire.
    pub fn to_json(&self) -> String {
        let (event_type, property, value, device) = match self {
            Outbound::Get(p) => ("get_property", p.wire(), String::new(), Some("primary")),
            Outbound::SetCentre(hz) => (
                "set_property",
                Prop::CentreFrequency.wire(),
                hz.to_string(),
                Some("primary"),
            ),
            Outbound::IqStream(on) => ("iq_stream_enable", "", on.to_string(), None),
            Outbound::StartStream => ("device_stream_enable", "", "true".to_string(), None),
        };
        let envelope = Envelope {
            event_type: event_type.to_string(),
            property: property.to_string(),
            value,
            device: device.map(str::to_string),
        };
        serde_json::to_string(&envelope).unwrap_or_default()
    }
}

/// A property of the primary receiver, as SDRconnect reported it.
#[derive(Debug, Clone, PartialEq)]
pub enum Report {
    ApiVersion(String),
    /// Hz.
    Centre(u64),
    /// Hz, whole.
    Rate(u32),
    Started(bool),
    CanControl(bool),
}

/// What a text message was.
#[derive(Debug, Clone, PartialEq)]
pub enum Text {
    /// A property Nexus reads, of the primary receiver.
    Report(Report),
    /// The envelope, but nothing Nexus reads: another property, or the secondary receiver.
    Other,
    /// Not the envelope, or a value Nexus reads that does not parse.
    Malformed,
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim() {
        v if v.eq_ignore_ascii_case("true") => Some(true),
        v if v.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

fn parse_hz(value: &str) -> Option<f64> {
    let v = value.trim().parse::<f64>().ok()?;
    (v.is_finite() && v >= 0.0).then_some(v)
}

/// Read a text message from SDRconnect.
pub fn parse_text(text: &str) -> Text {
    let Ok(envelope) = serde_json::from_str::<Envelope>(text) else {
        return Text::Malformed;
    };
    if !matches!(
        envelope.event_type.as_str(),
        "property_changed" | "get_property_response"
    ) {
        return Text::Other;
    }
    let primary = envelope
        .device
        .as_deref()
        .is_none_or(|d| d.trim().is_empty() || d.trim().eq_ignore_ascii_case("primary"));
    let Some(prop) = Prop::from_wire(envelope.property.trim()).filter(|_| primary) else {
        return Text::Other;
    };
    let value = envelope.value.as_str();
    let report = match prop {
        Prop::ApiVersion => Some(Report::ApiVersion(value.trim().to_string())),
        // A u64 in Hz; read through f64 so a value written as a decimal is taken too. Exact for
        // every frequency an RSP tunes (below 2^53 Hz).
        Prop::CentreFrequency => parse_hz(value)
            .filter(|hz| *hz < 1e12)
            .map(|hz| Report::Centre(hz.round() as u64)),
        Prop::SampleRate => parse_hz(value)
            .map(f64::round)
            .filter(|hz| *hz >= 1.0 && *hz <= f64::from(MAX_RATE_HZ))
            .map(|hz| Report::Rate(hz as u32)),
        Prop::Started => parse_bool(value).map(Report::Started),
        Prop::CanControl => parse_bool(value).map(Report::CanControl),
    };
    report.map_or(Text::Malformed, Text::Report)
}

/// What a binary message was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binary<'a> {
    /// The primary receiver's IQ: whole I/Q pairs of little-endian i16, at least one.
    Iq(&'a [u8]),
    /// Another type: audio, the secondary receiver, SDRconnect's display bins, or one unknown.
    Other(u16),
    /// Too short for a type, or IQ that is empty or holds half a pair.
    Malformed,
}

/// Read a binary message from SDRconnect.
pub fn parse_binary(bytes: &[u8]) -> Binary<'_> {
    let Some((kind, payload)) = bytes.split_first_chunk::<2>() else {
        return Binary::Malformed;
    };
    match u16::from_le_bytes(*kind) {
        IQ_PRIMARY if payload.is_empty() || payload.len() % 4 != 0 => Binary::Malformed,
        IQ_PRIMARY => Binary::Iq(payload),
        other => Binary::Other(other),
    }
}

/// The pairs in an IQ payload ([`Binary::Iq`]).
pub fn iq_pairs(payload: &[u8]) -> usize {
    payload.len() / 4
}

/// An IQ payload's samples, I then Q, scaled so that full scale is 1.
pub fn iq_samples(payload: &[u8]) -> impl Iterator<Item = f32> + '_ {
    payload
        .chunks_exact(2)
        .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_768.0)
}

/// Why a connection could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// No TCP connection: refused, timed out, unreachable. SDRconnect is not running there, or its
    /// API is off.
    Unreachable(String),
    /// The address answered, but not as a WebSocket server.
    Handshake(String),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::Unreachable(why) => write!(f, "nothing answered: {why}"),
            ConnectError::Handshake(why) => write!(f, "it is not a WebSocket server: {why}"),
        }
    }
}

/// A message from SDRconnect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    Text(String),
    Binary(Bytes),
}

/// One WebSocket connection to SDRconnect. It sends only [`Outbound`].
pub struct Link {
    ws: WebSocket<TcpStream>,
}

fn retryable(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
    )
}

impl Link {
    /// Open TCP to SDRconnect at `addr` and make the WebSocket handshake.
    pub fn connect(addr: SocketAddr) -> Result<Link, ConnectError> {
        let unreachable = |e: io::Error| ConnectError::Unreachable(e.to_string());
        let tcp = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(unreachable)?;
        tcp.set_nodelay(true).map_err(unreachable)?;
        tcp.set_read_timeout(Some(CONNECT_TIMEOUT))
            .map_err(unreachable)?;
        tcp.set_write_timeout(Some(WRITE_TIMEOUT))
            .map_err(unreachable)?;
        let limits = WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE))
            .max_frame_size(Some(MAX_MESSAGE));
        let url = format!("ws://{addr}/");
        let (ws, _) = tungstenite::client::client_with_config(url.as_str(), tcp, Some(limits))
            .map_err(|e| ConnectError::Handshake(e.to_string()))?;
        Ok(Link { ws })
    }

    /// Send one message. An error means the connection is gone.
    pub fn send(&mut self, message: &Outbound) -> Result<(), String> {
        self.ws
            .send(Message::text(message.to_json()))
            .map_err(|e| e.to_string())
    }

    /// The next message, waiting at most `wait`: `Ok(None)` when none came in that time. An error
    /// means the connection is gone, and says why.
    pub fn read(&mut self, wait: Duration) -> Result<Option<Incoming>, String> {
        let wait = wait.max(Duration::from_millis(1));
        self.ws
            .get_mut()
            .set_read_timeout(Some(wait))
            .map_err(|e| e.to_string())?;
        match self.ws.read() {
            Ok(Message::Text(text)) => Ok(Some(Incoming::Text(text.as_str().to_string()))),
            Ok(Message::Binary(bytes)) => Ok(Some(Incoming::Binary(bytes))),
            Ok(Message::Close(_)) => Err("SDRconnect closed the connection".to_string()),
            Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => Ok(None),
            Err(tungstenite::Error::Io(e)) if retryable(&e) => Ok(None),
            Err(tungstenite::Error::Protocol(ProtocolError::ResetWithoutClosingHandshake)) => {
                Err("the connection was lost".to_string())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Close the connection, saying so to SDRconnect first.
    pub fn close(mut self) {
        let _ = self.ws.close(None);
        let _ = self.ws.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every [`Outbound`], one of each kind: the match below stops compiling when a kind is added,
    /// so a new message cannot reach the wire without being placed in this list and judged by
    /// [`only_receive_side_messages_reach_the_wire`].
    fn every_outbound() -> Vec<Outbound> {
        let all: Vec<Outbound> = Prop::ALL
            .into_iter()
            .map(Outbound::Get)
            .chain([
                Outbound::SetCentre(14_074_000),
                Outbound::IqStream(true),
                Outbound::IqStream(false),
                Outbound::StartStream,
            ])
            .collect();
        for o in &all {
            match o {
                Outbound::Get(_)
                | Outbound::SetCentre(_)
                | Outbound::IqStream(_)
                | Outbound::StartStream => {}
            }
        }
        all
    }

    /// The receive-only proof on the wire: every message Nexus can send is a property read, the
    /// receiver's centre frequency, the IQ stream, or starting the receiver's stream. Never audio,
    /// recording, a device profile, a device change or any other property.
    #[test]
    fn only_receive_side_messages_reach_the_wire() {
        for o in every_outbound() {
            let v: serde_json::Value = serde_json::from_str(&o.to_json()).expect("JSON");
            let event = v["event_type"].as_str().expect("event_type");
            let property = v["property"].as_str().expect("property");
            let value = v["value"].as_str().expect("value");
            match event {
                "get_property" => assert!(
                    Prop::ALL.iter().any(|p| p.wire() == property),
                    "{o:?} reads {property}"
                ),
                "set_property" => assert_eq!(property, "device_center_frequency", "{o:?}"),
                "iq_stream_enable" => assert!(value == "true" || value == "false", "{o:?}"),
                "device_stream_enable" => assert_eq!(value, "true", "{o:?}"),
                other => panic!("{o:?} sends {other}, which is not a receive-side message"),
            }
        }
    }

    #[test]
    fn the_messages_are_the_specifications_envelope() {
        let v: serde_json::Value =
            serde_json::from_str(&Outbound::SetCentre(14_094_000).to_json()).expect("JSON");
        assert_eq!(
            v,
            serde_json::json!({
                "event_type": "set_property", "property": "device_center_frequency",
                "value": "14094000", "device": "primary",
            })
        );
        let v: serde_json::Value =
            serde_json::from_str(&Outbound::IqStream(true).to_json()).expect("JSON");
        assert_eq!(
            v,
            serde_json::json!({"event_type": "iq_stream_enable", "property": "", "value": "true"})
        );
    }

    #[test]
    fn reports_of_the_primary_receiver_are_read_and_the_rest_are_not() {
        let msg = |event: &str, prop: &str, value: &str, device: Option<&str>| {
            let mut v = serde_json::json!({"event_type": event, "property": prop, "value": value});
            if let Some(d) = device {
                v["device"] = d.into();
            }
            parse_text(&v.to_string())
        };
        assert_eq!(
            msg(
                "property_changed",
                "device_center_frequency",
                "14074000",
                Some("primary")
            ),
            Text::Report(Report::Centre(14_074_000))
        );
        assert_eq!(
            msg(
                "get_property_response",
                "device_sample_rate",
                "2000000.000",
                None
            ),
            Text::Report(Report::Rate(2_000_000))
        );
        assert_eq!(
            msg("property_changed", "started", "True", None),
            Text::Report(Report::Started(true))
        );
        assert_eq!(
            msg("property_changed", "can_control", "false", None),
            Text::Report(Report::CanControl(false))
        );
        assert_eq!(
            msg("get_property_response", "api_version", "1.0.3", None),
            Text::Report(Report::ApiVersion("1.0.3".into()))
        );
        assert_eq!(
            msg(
                "property_changed",
                "device_center_frequency",
                "7000000",
                Some("secondary")
            ),
            Text::Other,
            "the secondary receiver's centre is not the scope's"
        );
        assert_eq!(msg("property_changed", "lna_state", "3", None), Text::Other);
        assert_eq!(
            msg("property_changed", "device_sample_rate", "0", None),
            Text::Malformed
        );
        assert_eq!(
            msg("property_changed", "device_sample_rate", "NaN", None),
            Text::Malformed
        );
        assert_eq!(
            msg("property_changed", "device_center_frequency", "-5", None),
            Text::Malformed
        );
        assert_eq!(parse_text("not json"), Text::Malformed);
        assert_eq!(parse_text("{\"property\":\"started\"}"), Text::Malformed);
    }

    #[test]
    fn binary_messages_are_typed_and_bounded() {
        let mut iq = IQ_PRIMARY.to_le_bytes().to_vec();
        for s in [16_384i16, -16_384, 32_767, i16::MIN] {
            iq.extend_from_slice(&s.to_le_bytes());
        }
        let Binary::Iq(payload) = parse_binary(&iq) else {
            panic!("IQ");
        };
        assert_eq!(iq_pairs(payload), 2);
        let samples: Vec<f32> = iq_samples(payload).collect();
        assert_eq!(samples, vec![0.5, -0.5, 32_767.0 / 32_768.0, -1.0]);
        assert_eq!(parse_binary(&[]), Binary::Malformed);
        assert_eq!(parse_binary(&[2]), Binary::Malformed);
        assert_eq!(parse_binary(&[2, 0]), Binary::Malformed, "IQ with no pair");
        assert_eq!(
            parse_binary(&[2, 0, 1, 2]),
            Binary::Malformed,
            "half a pair"
        );
        assert_eq!(parse_binary(&[5, 0, 1, 2, 3, 4]), Binary::Other(5));
        assert_eq!(parse_binary(&[1, 0]), Binary::Other(1));
    }

    /// No message SDRconnect sends may panic a reader.
    #[test]
    fn no_message_panics_a_reader() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..20_000 {
            let len = (next() % 64) as usize;
            let bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            if let Binary::Iq(p) = parse_binary(&bytes) {
                assert_eq!(iq_samples(p).count(), 2 * iq_pairs(p));
            }
            let _ = parse_text(&String::from_utf8_lossy(&bytes));
        }
    }
}
