//! The stream's wire contract: every message the station, the relay and the page exchange for a
//! streamed Remote session, and the bounds each holds the others to.
//!
//! `remote/test/fixtures/stream/README.md` defines each field in prose, and the JSON files beside it
//! hold one case of every message. The tests at the bottom of this file hold these types to those
//! files, and the relay and the page test against the same ones, so a side that changes the
//! contract alone fails its own tests instead of failing a session.
//!
//! ## Parse, then validate
//!
//! serde refuses what is shaped wrong (an unknown key, a missing key, a number where a string
//! belongs). What is shaped right but out of bounds (a pointer off the frame, a key value with a
//! control character, an SDP too big for the relay) is refused by [`Validate`]. A receiver calls
//! both; the `parse_*` functions here do exactly that, and nothing else in the station reads these
//! messages any other way.
//!
//! ## The offer is signed by the browser (A5)
//!
//! An offer carries the browser's device key and its signature over [`offer_binding`], so a relay
//! that stamps another browser's `deviceId` cannot offer a stream as that browser. The station
//! checks both against the key it pinned when the operator approved the browser at the radio (the
//! README's A5 section). This file holds the shapes and the bytes that are signed; the station's
//! admission does the cryptography.
//!
//! ## No wall clock crosses the boundary
//!
//! `decodedFrameAt` is the station's own video RTP timestamp, echoed back, never the page's clock.
//! The station maps it to its own capture clock (see the README), the same discipline the receive
//! audio lane keeps for `firstFrameMs`.
use serde::{Deserialize, Serialize};

/// The version a station advertises with `x-nexus-stream-version`.
pub const STREAM_VERSION: u8 = 1;
/// The largest stamped signalling message the relay may hand a station. The station's control
/// socket closes on any frame over 8,192 bytes, and that closes all of Remote, not just the
/// stream, so the relay stops well short of it.
pub const SIGNAL_BYTES: usize = 7168;
/// The largest station → relay stream message the station will send.
pub const STATION_SIGNAL_BYTES: usize = 8192;
/// The largest SDP either side sends.
pub const SDP_BYTES: usize = 6144;
/// The largest ICE candidate string either side sends.
pub const CANDIDATE_BYTES: usize = 512;
/// The longest `sdpMid`.
pub const SDP_MID_CHARS: usize = 32;
/// The largest message the page sends on the `control` or `ptt` data channel.
pub const CONTROL_BYTES: usize = 1024;
/// How often the page re-asserts a held PTT, and its held keys and buttons (`held`).
pub const PTT_HOLD_EVERY_MS: u64 = 100;
/// How long the station keeps a PTT keyed with no hold arriving, and how long the station's
/// window keeps a key or button held with no `held` re-asserting it.
pub const PTT_GAP_MS: u64 = 200;
/// The most keys one `held` may name.
pub const HELD_KEYS: usize = 16;
/// The oldest decoded frame that still renews transmit presence.
pub const FRESH_FRAME_MS: u64 = 2000;
/// The largest side a page may say its picture area has (`view`), in device pixels. VP8's own
/// limit is 16,383.
pub const MAX_VIEW: u32 = 16_384;
/// The video track's RTP clock.
pub const VIDEO_CLOCK_HZ: u64 = 90_000;
/// The longest the station goes without sending a video frame, so that a still window never reads
/// as a stale picture.
pub const STILL_FRAME_MS: u64 = 500;
/// A browser's device key: the SPKI DER of a P-256 ECDSA public key, as lowercase hex.
pub const DEVICE_KEY_HEX_CHARS: usize = 182;
/// Every P-256 SPKI begins with these bytes (the algorithm and the curve), then `04` and the point.
pub const P256_SPKI_PREFIX_HEX: &str = "3059301306072a8648ce3d020106082a8648ce3d030107034200";
/// An offer's signature: ECDSA P-256 over SHA-256, IEEE P1363 `r‖s` (64 bytes), as lowercase hex.
pub const SIGNATURE_HEX_CHARS: usize = 128;
/// What the signed bytes begin with, so a signature made for this can never be taken for another.
pub const OFFER_BINDING_LABEL: &[u8] = b"nexus-stream-offer/1";
/// The data-channel labels, which the page creates and the station recognises.
pub const CONTROL_CHANNEL: &str = "control";
pub const PTT_CHANNEL: &str = "ptt";
pub const AUDIO_CHANNEL: &str = "audio";

/// A UUID in the lowercase form every Remote identifier takes. The same rule as the station
/// transport's own `identifier`, which this crate cannot reach.
pub fn identifier(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit() && !b.is_ascii_uppercase()
            }
        })
}

/// Bounds a parsed message must also meet.
pub trait Validate {
    fn valid(&self) -> bool;
}

/// Why a message was refused. Deliberately without detail: the reason is only ever turned into a
/// fixed refusal code, never shown or logged verbatim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    TooLarge,
    Malformed,
    OutOfBounds,
}

fn parse<T>(bytes: &[u8], limit: usize) -> Result<T, Refused>
where
    T: for<'de> Deserialize<'de> + Validate,
{
    if bytes.len() > limit {
        return Err(Refused::TooLarge);
    }
    let value: T = serde_json::from_slice(bytes).map_err(|_| Refused::Malformed)?;
    if value.valid() {
        Ok(value)
    } else {
        Err(Refused::OutOfBounds)
    }
}

/// A page's message on the `control` channel.
pub fn parse_control(bytes: &[u8]) -> Result<ControlIn, Refused> {
    parse(bytes, CONTROL_BYTES)
}

/// A page's message on the `ptt` channel.
pub fn parse_ptt(bytes: &[u8]) -> Result<PttIn, Refused> {
    parse(bytes, CONTROL_BYTES)
}

fn no_control(text: &str) -> bool {
    !text.chars().any(char::is_control)
}

fn sdp_mid(mid: &str) -> bool {
    !mid.is_empty()
        && mid.len() <= SDP_MID_CHARS
        && mid
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn candidate(line: &str) -> bool {
    !line.is_empty() && line.len() <= CANDIDATE_BYTES && no_control(line)
}

fn sdp(text: &str) -> bool {
    // An SDP is CRLF-separated text; nothing else in it may be a control character.
    !text.is_empty()
        && text.len() <= SDP_BYTES
        && !text
            .chars()
            .any(|c| c.is_control() && c != '\r' && c != '\n')
}

fn lower_hex(text: &str, chars: usize) -> bool {
    text.len() == chars
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The shape of a device key: a P-256 SPKI with an uncompressed point. Whether the point is on the
/// curve is the station's verification to find out, not the parser's.
pub fn device_key(text: &str) -> bool {
    lower_hex(text, DEVICE_KEY_HEX_CHARS)
        && text.starts_with(P256_SPKI_PREFIX_HEX)
        && text[P256_SPKI_PREFIX_HEX.len()..].starts_with("04")
}

/// Hex to bytes. `None` for anything but pairs of hex digits.
pub fn hex_bytes(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// The offer's DTLS certificate fingerprint: the 32 bytes of its `a=fingerprint:sha-256` value.
/// Every such line must carry the same value (a browser writes one per media section): `None` when
/// there is none, when they disagree, or when one is not 32 colon-separated hex bytes.
pub fn offer_fingerprint(sdp: &str) -> Option<[u8; 32]> {
    let mut found: Option<[u8; 32]> = None;
    for line in sdp.lines() {
        let Some(value) = line.trim().strip_prefix("a=fingerprint:") else {
            continue;
        };
        let (hash, digest) = value.split_once(' ')?;
        if !hash.eq_ignore_ascii_case("sha-256") {
            continue;
        }
        let bytes: Vec<u8> = digest
            .trim()
            .split(':')
            .map(|pair| {
                (pair.len() == 2)
                    .then(|| u8::from_str_radix(pair, 16).ok())
                    .flatten()
            })
            .collect::<Option<_>>()?;
        let bytes: [u8; 32] = bytes.try_into().ok()?;
        if found.is_some_and(|seen| seen != bytes) {
            return None;
        }
        found = Some(bytes);
    }
    found
}

/// The bytes an offer's `signature` covers (the README's A5 section): the label, `SHA-256(fp)`
/// (the caller hashes; this crate has no hash), and the three ids as the relay stamps them.
pub fn offer_binding(
    fingerprint_digest: &[u8; 32],
    station_id: &str,
    device_id: &str,
    session_id: &str,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(OFFER_BINDING_LABEL.len() + 32 + 3 * 36);
    bytes.extend_from_slice(OFFER_BINDING_LABEL);
    bytes.extend_from_slice(fingerprint_digest);
    for id in [station_id, device_id, session_id] {
        bytes.extend_from_slice(id.as_bytes());
    }
    bytes
}

// ---------------------------------------------------------------------------------------------
// Signalling: over the relay sockets that already exist.
// ---------------------------------------------------------------------------------------------

/// Page → station, carried in `streamSignal.payload`.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum BrowserSignal {
    Offer {
        sdp: String,
        /// The browser's device key ([`DEVICE_KEY_HEX_CHARS`]); the station pinned its SHA-256.
        /// With `signature`, or neither: an unsigned offer parses, and admission refuses it by
        /// name instead of the parser closing the whole control socket over it.
        #[serde(rename = "publicKey", default, skip_serializing_if = "Option::is_none")]
        public_key: Option<String>,
        /// Its signature over [`offer_binding`] ([`SIGNATURE_HEX_CHARS`]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    Candidate {
        candidate: String,
        #[serde(rename = "sdpMid")]
        sdp_mid: String,
    },
    /// The page is done with the stream. Empty braces rather than a unit variant, so serde keeps
    /// refusing unknown keys on it.
    Close {},
}

impl Validate for BrowserSignal {
    fn valid(&self) -> bool {
        match self {
            Self::Offer {
                sdp: text,
                public_key,
                signature,
            } => {
                sdp(text)
                    && match (public_key, signature) {
                        (Some(key), Some(signature)) => {
                            device_key(key) && lower_hex(signature, SIGNATURE_HEX_CHARS)
                        }
                        (None, None) => true,
                        _ => false,
                    }
            }
            Self::Candidate {
                candidate: line,
                sdp_mid: mid,
            } => candidate(line) && sdp_mid(mid),
            Self::Close {} => true,
        }
    }
}

/// Station → page, carried in `streamSignal.payload`.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum StationSignal {
    Answer {
        sdp: String,
    },
    Candidate {
        candidate: String,
        #[serde(rename = "sdpMid")]
        sdp_mid: String,
    },
}

impl Validate for StationSignal {
    fn valid(&self) -> bool {
        match self {
            Self::Answer { sdp: text } => sdp(text),
            Self::Candidate {
                candidate: line,
                sdp_mid: mid,
            } => candidate(line) && sdp_mid(mid),
        }
    }
}

/// Why a stream is not running. A closed vocabulary: the page refuses anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StreamReason {
    /// No station-control grant, or no live lease matching this session.
    NotController,
    /// The operator has not turned streaming on at the station.
    StreamDisabled,
    /// The station cannot stream right now: not a platform it captures on, no window to capture,
    /// the capture or the encoder failed, or it could not find a network path.
    StreamUnavailable,
    /// Another session is streaming this station.
    StreamInUse,
    /// The offer was not DTLS-SRTP, had no VP8 video to receive or no data channel, or was too big.
    InvalidOffer,
    /// ICE or DTLS failed or timed out.
    ConnectionFailed,
    /// The page closed the stream.
    StreamClosed,
    /// Remote access was switched off, and the relay ended the stream (`streamEnd`). The station
    /// echoes the reason in its last `streamState`.
    RemoteOff,
    /// The station pinned no device key for this browser: it was approved before keys existed, and
    /// is approved again at the radio, once (A5).
    DeviceNotPinned,
    /// The offer's key is not the pinned one or its signature does not hold, or the certificate the
    /// page presented after DTLS is not the one it signed (A5).
    DeviceKeyMismatch,
    /// Relay-originated: the account's command entitlement has lapsed. The relay refuses an offer
    /// with it, or ends a running stream with it. The station never sends it of its own accord.
    ServiceAccessExpired,
    /// Relay-originated: the page is over the relay's signalling budget. The station never sends
    /// it.
    TryLater,
}

/// Page → relay, before the relay stamps it.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum PageToRoom {
    StreamSignal {
        #[serde(rename = "leaseId")]
        lease_id: String,
        payload: BrowserSignal,
    },
}

/// Relay → station: the relay writes `sessionId` and `deviceId` from its own admission record.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum RoomToStation {
    StreamSignal {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        #[serde(rename = "leaseId")]
        lease_id: String,
        payload: BrowserSignal,
    },
    /// The relay ends this session's stream: Remote access was switched off (operator decision
    /// 2026-09-27, "within about 2 s"), or the relay ended it for another reason it names. The
    /// station ends the session's transmit presence at once and tears the session down.
    StreamEnd {
        #[serde(rename = "sessionId")]
        session_id: String,
        reason: StreamReason,
    },
}

/// Station → relay, addressed to one session. The relay strips `sessionId` before delivery.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum StationToRoom {
    StreamSignal {
        #[serde(rename = "sessionId")]
        session_id: String,
        payload: StationSignal,
    },
    StreamState {
        #[serde(rename = "sessionId")]
        session_id: String,
        streaming: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<StreamReason>,
    },
}

impl StationToRoom {
    /// The message as the station sends it, or `None` if it would break a bound the relay holds
    /// the station to. A station never sends what the other end is bound to refuse.
    pub fn to_wire(&self) -> Option<String> {
        let valid = match self {
            Self::StreamSignal { payload, .. } => payload.valid(),
            Self::StreamState { .. } => true,
        };
        let text = serde_json::to_string(self).ok()?;
        (valid && text.len() <= STATION_SIGNAL_BYTES).then_some(text)
    }
}

/// Relay → page.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum RoomToPage {
    StreamSignal {
        payload: StationSignal,
    },
    StreamState {
        streaming: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<StreamReason>,
    },
}

// ---------------------------------------------------------------------------------------------
// Data channels: `control` (reliable, ordered), `ptt` and `audio` (unordered, no retransmits).
// ---------------------------------------------------------------------------------------------

/// Page → station on `control`. The four operation requests are today's, byte for byte, except
/// that a stream heartbeat must say how fresh the page's picture is.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum ControlIn {
    State {
        #[serde(rename = "requestId")]
        request_id: String,
    },
    Heartbeat {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "leaseId")]
        lease_id: String,
        /// The video RTP timestamp of the last frame the page showed, or null before the first.
        /// Required: serde would otherwise read a missing key as null, and a page that forgot the
        /// field must be refused rather than read as blind forever.
        #[serde(rename = "decodedFrameAt", deserialize_with = "Option::deserialize")]
        decoded_frame_at: Option<u32>,
    },
    Release {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "leaseId")]
        lease_id: String,
    },
    StopTransmit {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "stationBootId")]
        station_boot_id: String,
        #[serde(rename = "leaseId")]
        lease_id: String,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
    },
    Pointer(PointerInput),
    Wheel(WheelInput),
    Key(KeyInput),
    Text(TextInput),
    /// The page's picture area, in its own device pixels: the most the station encodes the
    /// picture at, so the page can show it pixel for pixel. Not input, and needs no presence.
    View {
        width: u32,
        height: u32,
    },
}

impl Validate for ControlIn {
    fn valid(&self) -> bool {
        match self {
            // Identifiers and epochs are checked where they are used, by the same authority that
            // checks them on the relay path, so that the two paths cannot disagree about them.
            Self::State { .. }
            | Self::Heartbeat { .. }
            | Self::Release { .. }
            | Self::StopTransmit { .. } => true,
            Self::Pointer(input) => input.valid(),
            Self::Wheel(input) => input.valid(),
            Self::Key(input) => input.valid(),
            Self::Text(input) => input.valid(),
            Self::View { width, height } => {
                (1..=MAX_VIEW).contains(width) && (1..=MAX_VIEW).contains(height)
            }
        }
    }
}

impl ControlIn {
    /// The input this message describes, as the station hands it to its own window.
    pub fn input(&self) -> Option<WebviewInput> {
        match self {
            Self::Pointer(input) => Some(WebviewInput::Pointer(input.clone())),
            Self::Wheel(input) => Some(WebviewInput::Wheel(input.clone())),
            Self::Key(input) => Some(WebviewInput::Key(input.clone())),
            Self::Text(input) => Some(WebviewInput::Text(input.clone())),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PointerAction {
    Down,
    Move,
    Up,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PointerType {
    Mouse,
    Touch,
    Pen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyAction {
    Down,
    Up,
}

/// Shift, Control, Alt, Meta.
const MODIFIER_BITS: u8 = 0b1111;

fn fraction(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointerInput {
    pub action: PointerAction,
    /// Fractions of the video frame, which is the Nexus window's client area.
    pub x: f64,
    pub y: f64,
    /// The DOM `button`: -1 for none changed, 0 main, 1 auxiliary, 2 secondary, 3 back, 4 forward.
    pub button: i8,
    /// The DOM `buttons` bitmask.
    pub buttons: u8,
    pub modifiers: u8,
    pub pointer_type: PointerType,
    /// The DOM click count: 0 for a move, 2 on the second press of a double-click.
    pub clicks: u8,
}

impl Validate for PointerInput {
    fn valid(&self) -> bool {
        fraction(self.x)
            && fraction(self.y)
            && (-1..=4).contains(&self.button)
            && self.buttons <= 31
            && self.modifiers <= MODIFIER_BITS
            && self.clicks <= 3
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WheelInput {
    pub x: f64,
    pub y: f64,
    pub delta_x: f64,
    pub delta_y: f64,
    /// 0 pixels, 1 lines, 2 pages.
    pub delta_mode: u8,
    pub modifiers: u8,
}

impl Validate for WheelInput {
    fn valid(&self) -> bool {
        let delta = |d: f64| d.is_finite() && d.abs() <= 10_000.0;
        fraction(self.x)
            && fraction(self.y)
            && delta(self.delta_x)
            && delta(self.delta_y)
            && self.delta_mode <= 2
            && self.modifiers <= MODIFIER_BITS
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeyInput {
    pub action: KeyAction,
    /// The DOM `key` value: a character, or a name such as `Enter`.
    pub key: String,
    /// The DOM `code` value, such as `KeyA` or `Space`; empty when the page has none.
    pub code: String,
    pub modifiers: u8,
    pub repeat: bool,
}

impl Validate for KeyInput {
    fn valid(&self) -> bool {
        let key = self.key.chars().count();
        (1..=32).contains(&key)
            && no_control(&self.key)
            && self.code.len() <= 32
            && self.code.bytes().all(|b| b.is_ascii_alphanumeric())
            && self.modifiers <= MODIFIER_BITS
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextInput {
    /// Committed text, for the focused field.
    pub text: String,
}

impl Validate for TextInput {
    fn valid(&self) -> bool {
        (1..=256).contains(&self.text.chars().count()) && no_control(&self.text)
    }
}

/// Station → page on `control`.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum ControlOut {
    /// The answer to an operation request: today's `value` or `error`, and on a heartbeat whether
    /// the station holds transmit presence for this session after it.
    OperationResponse {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        presence: Option<bool>,
    },
    PttState {
        #[serde(rename = "holdId")]
        hold_id: String,
        keyed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<PttReason>,
    },
    /// The operator's microphone over, as the station holds it (plan S6, the audio design's M1).
    /// Sent when any field changes. `armed`: a press (the page's PTT, or one made through the
    /// picture) armed an over; `keyed`: the operator's voice has keyed the rig; `noPowerOut`: the
    /// voice has been arriving for about two seconds and the rig reports no power out, which is
    /// display only and changes nothing about the over; `ended`: on the message that reports an
    /// over's end, why it ended.
    MicState {
        armed: bool,
        keyed: bool,
        #[serde(rename = "noPowerOut")]
        no_power_out: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ended: Option<MicEnded>,
    },
}

/// Why a microphone over ended, as the page is told (the audio design's §7 captions).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MicEnded {
    /// The operator let go.
    Released,
    /// A stop at the station, TX turned off, leaving Phone, a tune, or the licence.
    Stopped,
    /// No audio arrived for 200 ms (M2): the uplink stalled or the microphone went quiet.
    AudioGap,
    /// The session's transmit presence lapsed (M7).
    Presence,
    /// The 10-minute ceiling.
    Ceiling,
    /// The wall-clock TX watchdog.
    Watchdog,
    /// The station's transmit audio route changed under the over (G7).
    RouteChanged,
}

/// Why a held PTT is not keyed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PttReason {
    /// The station would not key: transmit is off, the dial is outside the licence, the mode keys
    /// no PTT, or the session holds no transmit presence.
    Refused,
    /// No hold arrived for [`PTT_GAP_MS`].
    Lapsed,
    /// The page released it.
    Released,
    /// A stop at the station ended it.
    Stopped,
}

/// Page → station on `ptt`.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum PttIn {
    PttHold {
        #[serde(rename = "holdId")]
        hold_id: String,
        seq: u32,
    },
    PttRelease {
        #[serde(rename = "holdId")]
        hold_id: String,
        seq: u32,
    },
    Held(HeldInput),
}

impl Validate for PttIn {
    fn valid(&self) -> bool {
        match self {
            Self::PttHold { hold_id, .. } | Self::PttRelease { hold_id, .. } => identifier(hold_id),
            Self::Held(held) => held.valid(),
        }
    }
}

/// Everything the page is holding down over the picture: the held-key dead-man. The page sends it
/// at once when the set changes and then every [`PTT_HOLD_EVERY_MS`] while anything is held; the
/// station's window releases, with a proper key-up or pointer-up, anything not re-asserted within
/// [`PTT_GAP_MS`]. A held Space is an ordinary held key: what it does is the window's to decide.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HeldInput {
    /// The DOM `code` of each key held, without repeats.
    pub keys: Vec<String>,
    /// The DOM `buttons` bitmask of the pointer buttons held on the picture.
    pub buttons: u8,
    /// Counts up from 0 over the stream, one per message; the window ignores one that is not
    /// above the last it saw.
    pub seq: u32,
}

impl Validate for HeldInput {
    fn valid(&self) -> bool {
        let code = |k: &String| {
            (1..=32).contains(&k.len()) && k.bytes().all(|b| b.is_ascii_alphanumeric())
        };
        self.keys.len() <= HELD_KEYS
            && self.keys.iter().all(code)
            && self
                .keys
                .iter()
                .enumerate()
                .all(|(i, k)| !self.keys[..i].contains(k))
            && self.buttons <= 31
    }
}

/// Station → page on `audio`: the relay's receive-audio messages, unchanged, so the page's
/// existing player takes them as they are.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum AudioOut {
    AudioRx {
        seq: u32,
        epoch: String,
        #[serde(rename = "firstFrameMs")]
        first_frame_ms: u64,
        #[serde(rename = "frameMs")]
        frame_ms: u32,
        count: u8,
        payload: String,
    },
    AudioState {
        listening: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

/// Station → its own main window, as the Tauri event [`WEBVIEW_INPUT_EVENT`]. Never leaves the
/// station, and never becomes OS input.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum WebviewInput {
    Pointer(PointerInput),
    Wheel(WheelInput),
    Key(KeyInput),
    Text(TextInput),
    /// The page's held set, re-asserted: it presses nothing, it only keeps held what already is.
    Held(HeldInput),
    /// Release anything the window's input module is still holding down.
    Reset {},
}

/// The Tauri event name the station's window listens on.
pub const WEBVIEW_INPUT_EVENT: &str = "remote-stream-input";

impl Validate for WebviewInput {
    fn valid(&self) -> bool {
        match self {
            Self::Pointer(input) => input.valid(),
            Self::Wheel(input) => input.valid(),
            Self::Key(input) => input.valid(),
            Self::Text(input) => input.valid(),
            Self::Held(input) => input.valid(),
            Self::Reset {} => true,
        }
    }
}

#[cfg(test)]
mod tests {
    //! ★ THE FIXTURES ARE THE CONTRACT. Each accepted case parses into its type and serialises back
    //! to the same JSON; each refused case is refused by the receiver's parser or its bounds. The
    //! relay and the page read these same files, so this is where a one-sided change is caught.
    use super::*;
    use serde::de::DeserializeOwned;
    use serde_json::Value;

    const SIGNAL: &str = include_str!("../../../remote/test/fixtures/stream/signal.json");
    const CHANNEL: &str = include_str!("../../../remote/test/fixtures/stream/channel.json");
    const WEBVIEW: &str = include_str!("../../../remote/test/fixtures/stream/webview.json");

    fn cases(file: &str, list: &str) -> Vec<(String, Value)> {
        let file: Value = serde_json::from_str(file).expect("a fixture file is JSON");
        let cases = file[list]
            .as_array()
            .unwrap_or_else(|| panic!("the fixture list {list} exists"));
        assert!(!cases.is_empty(), "premise: {list} holds cases");
        cases
            .iter()
            .map(|case| {
                (
                    case["name"].as_str().expect("a case is named").to_string(),
                    case["message"].clone(),
                )
            })
            .collect()
    }

    /// JSON equality that reads `0` and `0.0` as the same number, because a page written in
    /// JavaScript sends one and serde writes the other.
    fn same(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
            (Value::Array(x), Value::Array(y)) => {
                x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y))
            }
            (Value::Object(x), Value::Object(y)) => {
                x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
            }
            _ => a == b,
        }
    }

    /// Parse, validate, and serialise back to the same JSON.
    fn round_trip<T: DeserializeOwned + Serialize + Validate>(file: &str, list: &str) -> usize {
        let cases = cases(file, list);
        for (name, message) in &cases {
            let parsed: T = serde_json::from_value(message.clone())
                .unwrap_or_else(|e| panic!("{list} / {name}: refused: {e}"));
            assert!(parsed.valid(), "{list} / {name}: out of bounds");
            let back = serde_json::to_value(&parsed).unwrap();
            assert!(
                same(&back, message),
                "{list} / {name}: did not come back the same:\n{back}\n{message}"
            );
        }
        cases.len()
    }

    /// Every case must be refused, by the parser or by the bounds.
    fn refused<T: DeserializeOwned + Validate>(file: &str, list: &str) {
        for (name, message) in cases(file, list) {
            let accepted = serde_json::from_value::<T>(message).is_ok_and(|m| m.valid());
            assert!(!accepted, "{list} / {name}: accepted, and must be refused");
        }
    }

    impl Validate for PageToRoom {
        fn valid(&self) -> bool {
            let Self::StreamSignal { lease_id, payload } = self;
            identifier(lease_id) && payload.valid()
        }
    }
    impl Validate for RoomToStation {
        fn valid(&self) -> bool {
            match self {
                Self::StreamSignal {
                    session_id,
                    device_id,
                    lease_id,
                    payload,
                } => {
                    [session_id, device_id, lease_id]
                        .into_iter()
                        .all(|id| identifier(id))
                        && payload.valid()
                }
                Self::StreamEnd { session_id, .. } => identifier(session_id),
            }
        }
    }
    impl Validate for StationToRoom {
        fn valid(&self) -> bool {
            self.to_wire().is_some()
        }
    }
    impl Validate for RoomToPage {
        fn valid(&self) -> bool {
            match self {
                Self::StreamSignal { payload } => payload.valid(),
                Self::StreamState { .. } => true,
            }
        }
    }
    impl Validate for ControlOut {
        fn valid(&self) -> bool {
            true
        }
    }
    impl Validate for AudioOut {
        fn valid(&self) -> bool {
            true
        }
    }

    #[test]
    fn every_signalling_case_round_trips_on_its_hop() {
        assert_eq!(round_trip::<PageToRoom>(SIGNAL, "browserToRoom"), 5);
        assert_eq!(round_trip::<RoomToStation>(SIGNAL, "roomToStation"), 5);
        assert_eq!(round_trip::<StationToRoom>(SIGNAL, "stationToRoom"), 14);
        assert_eq!(round_trip::<RoomToPage>(SIGNAL, "roomToBrowser"), 14);
        // The relay's own: it ends a station's stream, and it answers a page by itself.
        assert_eq!(round_trip::<RoomToStation>(SIGNAL, "roomToStationEnd"), 2);
        assert_eq!(
            round_trip::<RoomToPage>(SIGNAL, "roomToBrowserFromRelay"),
            3
        );
    }

    #[test]
    fn the_station_refuses_every_malformed_relay_message() {
        refused::<RoomToStation>(SIGNAL, "roomToStationRefused");
        refused::<RoomToPage>(SIGNAL, "pageRefused");
    }

    /// A5's shapes: an offer's key and signature are parsed, then held to their bounds. These are
    /// well-formed JSON, so they reach the bounds (the station answers `invalidOffer`), and the
    /// structural cases (a missing key or signature) never get past serde.
    #[test]
    fn an_offer_key_or_signature_out_of_bounds_is_refused_after_it_parses() {
        let bounds = cases(SIGNAL, "roomToStationOutOfBounds");
        assert_eq!(bounds.len(), 5);
        for (name, message) in bounds {
            let parsed: RoomToStation = serde_json::from_value(message)
                .unwrap_or_else(|e| panic!("{name}: premise, it parses: {e}"));
            assert!(
                !parsed.valid(),
                "{name}: within bounds, and must be refused"
            );
        }
        // Control: the signed and the unsigned offer are both within bounds (admission decides).
        for (name, message) in cases(SIGNAL, "roomToStation") {
            let parsed: RoomToStation = serde_json::from_value(message).unwrap();
            assert!(parsed.valid(), "{name}");
        }
    }

    /// The signed bytes, laid out as the README says: 20 + 32 + 3 × 36.
    #[test]
    fn the_offer_binding_is_the_label_the_digest_and_the_three_ids() {
        let digest = [7u8; 32];
        let station = "10000000-0000-4000-8000-00000000000a";
        let device = "10000000-0000-4000-8000-000000000002";
        let session = "10000000-0000-4000-8000-000000000001";
        let bytes = offer_binding(&digest, station, device, session);
        assert_eq!(bytes.len(), 160);
        assert_eq!(&bytes[..20], b"nexus-stream-offer/1");
        assert_eq!(&bytes[20..52], &digest);
        assert_eq!(&bytes[52..88], station.as_bytes());
        assert_eq!(&bytes[88..124], device.as_bytes());
        assert_eq!(&bytes[124..], session.as_bytes());
        // Control: another session is other bytes, so a signature for one is not one for another.
        assert_ne!(bytes, offer_binding(&digest, station, device, station));
    }

    /// The offer's fingerprint: one value, however many sections carry it.
    #[test]
    fn the_offer_fingerprint_is_its_one_sha256_value() {
        let offer = cases(SIGNAL, "browserToRoom")
            .into_iter()
            .find(|(name, _)| name == "offer")
            .unwrap()
            .1;
        let sdp = offer["payload"]["sdp"].as_str().unwrap();
        assert!(
            sdp.matches("a=fingerprint:sha-256").count() >= 2,
            "premise: one per media section"
        );
        let expected =
            hex_bytes("0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9").unwrap();
        assert_eq!(offer_fingerprint(sdp).unwrap().to_vec(), expected);
        // Two certificates in one offer: there is no one fingerprint to sign.
        let first = "a=fingerprint:sha-256 0A:1B:2C:3D:4E:5F:60:71:82:93:A4:B5:C6:D7:E8:F9:0A:1B:2C:3D:4E:5F:60:71:82:93:A4:B5:C6:D7:E8:F9";
        let other = first.replace("0A:1B", "0A:1C");
        assert_eq!(offer_fingerprint(&sdp.replacen(first, &other, 1)), None);
        // None, and not a short or a guessed one.
        assert_eq!(
            offer_fingerprint(&sdp.replace(first, "a=fingerprint:sha-1 0A:1B")),
            None
        );
        assert_eq!(
            offer_fingerprint(&sdp.replace(first, "a=fingerprint:sha-256 0A:1B")),
            None
        );
        assert_eq!(offer_fingerprint("v=0\r\n"), None);
    }

    /// The relay stamps and strips; it never rewrites. So each hop's cases are the other hop's plus
    /// or minus exactly the relay's own fields.
    #[test]
    fn the_relay_only_stamps_and_strips() {
        let page = cases(SIGNAL, "browserToRoom");
        let station = cases(SIGNAL, "roomToStation");
        assert_eq!(page.len(), station.len());
        for ((name, sent), (other, stamped)) in page.iter().zip(&station) {
            assert_eq!(name, other);
            let mut expected = sent.clone();
            expected["sessionId"] = stamped["sessionId"].clone();
            expected["deviceId"] = stamped["deviceId"].clone();
            assert!(identifier(stamped["sessionId"].as_str().unwrap()));
            assert!(identifier(stamped["deviceId"].as_str().unwrap()));
            assert_eq!(&expected, stamped, "{name}");
        }
        let from = cases(SIGNAL, "stationToRoom");
        let to = cases(SIGNAL, "roomToBrowser");
        assert_eq!(from.len(), to.len());
        for ((name, sent), (other, delivered)) in from.iter().zip(&to) {
            assert_eq!(name, other);
            let mut expected = sent.clone();
            expected.as_object_mut().unwrap().remove("sessionId");
            assert_eq!(&expected, delivered, "{name}");
        }
    }

    /// The bounds the README states, measured on the fixtures themselves: the stamped offer fits
    /// the relay's ceiling with room to spare, so a real page's offer has room too.
    #[test]
    fn every_fixture_fits_its_bound() {
        for list in ["roomToStation", "roomToStationEnd"] {
            for (name, message) in cases(SIGNAL, list) {
                let wire = serde_json::to_string(&message).unwrap();
                assert!(wire.len() <= SIGNAL_BYTES, "{name}: {} bytes", wire.len());
            }
        }
        for (name, message) in cases(SIGNAL, "stationToRoom") {
            let parsed: StationToRoom = serde_json::from_value(message).unwrap();
            assert!(parsed.to_wire().is_some(), "{name}");
        }
        for list in ["controlBrowserToStation", "pttBrowserToStation"] {
            for (name, message) in cases(CHANNEL, list) {
                let wire = serde_json::to_string(&message).unwrap();
                assert!(wire.len() <= CONTROL_BYTES, "{list} / {name}");
            }
        }
    }

    #[test]
    fn every_data_channel_case_round_trips() {
        assert_eq!(
            round_trip::<ControlIn>(CHANNEL, "controlBrowserToStation"),
            16
        );
        assert_eq!(
            round_trip::<ControlOut>(CHANNEL, "controlStationToBrowser"),
            20
        );
        assert_eq!(round_trip::<PttIn>(CHANNEL, "pttBrowserToStation"), 7);
        assert_eq!(round_trip::<AudioOut>(CHANNEL, "audioStationToBrowser"), 3);
    }

    /// The station's own parsers, bytes in: the size bound, then serde, then the bounds.
    #[test]
    fn the_station_parsers_accept_the_fixtures_and_refuse_the_rest() {
        for (name, message) in cases(CHANNEL, "controlBrowserToStation") {
            let bytes = serde_json::to_vec(&message).unwrap();
            assert!(parse_control(&bytes).is_ok(), "{name}");
        }
        for (name, message) in cases(CHANNEL, "controlRefused") {
            let bytes = serde_json::to_vec(&message).unwrap();
            assert!(parse_control(&bytes).is_err(), "{name}: accepted");
        }
        for (name, message) in cases(CHANNEL, "pttBrowserToStation") {
            let bytes = serde_json::to_vec(&message).unwrap();
            assert!(parse_ptt(&bytes).is_ok(), "{name}");
        }
        for (name, message) in cases(CHANNEL, "pttRefused") {
            let bytes = serde_json::to_vec(&message).unwrap();
            assert!(parse_ptt(&bytes).is_err(), "{name}: accepted");
        }
        // The size bound comes first: a message over it is refused unread.
        let big = format!(
            r#"{{"type":"text","text":"{}"}}"#,
            "a".repeat(CONTROL_BYTES)
        );
        assert_eq!(parse_control(big.as_bytes()), Err(Refused::TooLarge));
    }

    #[test]
    fn a_heartbeat_must_say_how_fresh_its_picture_is() {
        let hb = |extra: &str| {
            format!(
                r#"{{"type":"heartbeat","requestId":"{id}","leaseId":"{id}"{extra}}}"#,
                id = "10000000-0000-4000-8000-000000000001"
            )
        };
        assert!(matches!(
            parse_control(hb(r#","decodedFrameAt":null"#).as_bytes()),
            Ok(ControlIn::Heartbeat {
                decoded_frame_at: None,
                ..
            })
        ));
        assert!(matches!(
            parse_control(hb(r#","decodedFrameAt":4294967295"#).as_bytes()),
            Ok(ControlIn::Heartbeat {
                decoded_frame_at: Some(u32::MAX),
                ..
            })
        ));
        // Missing is not null: it is a page that does not know the field, and it is refused.
        assert_eq!(parse_control(hb("").as_bytes()), Err(Refused::Malformed));
    }

    #[test]
    fn the_window_receives_exactly_the_input_the_channel_carried() {
        assert_eq!(round_trip::<WebviewInput>(WEBVIEW, "stationToWebview"), 12);
        refused::<WebviewInput>(WEBVIEW, "refused");
        // Every input case on `control` reaches the window unchanged, and nothing else does.
        let channel: Vec<Value> = cases(CHANNEL, "controlBrowserToStation")
            .into_iter()
            .filter_map(|(_, m)| {
                let parsed: ControlIn = serde_json::from_value(m).unwrap();
                parsed.input().map(|i| serde_json::to_value(i).unwrap())
            })
            .collect();
        let window: Vec<Value> = cases(WEBVIEW, "stationToWebview")
            .into_iter()
            .map(|(_, m)| m)
            .filter(|m| m["type"] != "reset" && m["type"] != "held")
            .collect();
        assert_eq!(channel.len(), 10, "premise: ten input cases");
        assert_eq!(channel.len(), window.len());
        for (a, b) in channel.iter().zip(&window) {
            assert!(same(a, b), "{a} vs {b}");
        }
        // And a `held` from `ptt` reaches it unchanged too.
        let held: Vec<Value> = cases(WEBVIEW, "stationToWebview")
            .into_iter()
            .map(|(_, m)| m)
            .filter(|m| m["type"] == "held")
            .collect();
        assert_eq!(held.len(), 1, "premise: one held case");
        let on_ptt = cases(CHANNEL, "pttBrowserToStation");
        assert!(
            on_ptt.iter().any(|(_, m)| same(m, &held[0])),
            "the window's held case is not one the ptt channel carries"
        );
        let Ok(PttIn::Held(parsed)) = parse_ptt(&serde_json::to_vec(&held[0]).unwrap()) else {
            panic!("the held case does not parse on ptt");
        };
        assert!(same(
            &serde_json::to_value(WebviewInput::Held(parsed)).unwrap(),
            &held[0]
        ));
    }

    /// The held set's bounds, beyond the fixtures: sixteen keys pass and seventeen do not; a key
    /// named twice, an empty key or a space in a key is refused. CONTROL for each: the same set
    /// without the fault passes.
    #[test]
    fn a_held_set_is_bounded() {
        let held = |keys: &[&str], buttons: u8| HeldInput {
            keys: keys.iter().map(|k| k.to_string()).collect(),
            buttons,
            seq: 1,
        };
        let sixteen: Vec<String> = (0..16)
            .map(|i| format!("Key{}", (b'A' + i) as char))
            .collect();
        let sixteen: Vec<&str> = sixteen.iter().map(String::as_str).collect();
        assert!(held(&sixteen, 31).valid());
        let mut seventeen = sixteen.clone();
        seventeen.push("KeyZ");
        assert!(!held(&seventeen, 0).valid());
        assert!(!held(&["Space", "Space"], 0).valid());
        assert!(held(&["Space", "KeyA"], 0).valid());
        assert!(!held(&[""], 0).valid());
        assert!(!held(&["Key A"], 0).valid());
        assert!(!held(&[], 32).valid());
        assert!(held(&[], 0).valid());
    }

    #[test]
    fn identifiers_are_lowercase_uuids() {
        assert!(identifier("10000000-0000-4000-8000-00000000000a"));
        // CONTROL: the same UUID in capitals, and one short, are refused.
        assert!(!identifier("10000000-0000-4000-8000-00000000000A"));
        assert!(!identifier("10000000-0000-4000-8000-00000000000"));
    }
}
