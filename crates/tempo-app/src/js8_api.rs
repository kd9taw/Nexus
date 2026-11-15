//! JS8Call's network API (its API v3, `docs/API.md` in JS8Call 3.0.3), READ-ONLY and on THIS
//! COMPUTER only: the protocol half, what each line says. The listeners that move the lines (TCP
//! 2442, UDP 2242, both on 127.0.0.1) are `src-tauri/src/js8_api.rs`; nothing here opens a socket.
//! A CHILD module of `engine` (declared with `#[path]` there, as `js8.rs` is), so it reads the JS8
//! station's state without widening any visibility.
//!
//! WHAT IS ANSWERED. The reads `RX.GET_*`, `STATION.GET_*` and `INBOX.GET_MESSAGES`, and the
//! six other requests JS8Call answers that only read (`RIG.GET_FREQ`, `RIG.GET_PTT`,
//! `MODE.GET_SPEED`, `TX.GET_TEXT`, `TX.GET_QUEUE_DEPTH`, `STATION.VERSION`), from a
//! [`Js8ApiSnapshot`]: a COPY the listeners take under the Engine lock once a tick. A request
//! never takes the lock and has nothing it could change — [`answer`] borrows the snapshot and
//! returns a line. And the messages JS8Call sends on its own: `RX.ACTIVITY` (every decoded frame),
//! `RX.DIRECTED` (every complete directed command, heartbeats and CQs included), `RX.SPOT` (while
//! spotting is on), `TX.FRAME` (the tones of every frame keyed), `RIG.PTT` (a tune carrier keys
//! it too), `RIG.FREQ` (a band change), `STATION.STATUS`, `LOG.QSO` (a JS8 contact logged),
//! `STATION.CLOSING`, and the UDP side's `PING` and `CLOSE`.
//!
//! WHAT IS REFUSED: every other type. `TX.SEND_MESSAGE`, `TX.SET_TEXT`, `RIG.SET_*`,
//! `RIG.TX_HALT`, `MODE.SET_SPEED`, `STATION.SET_*`, `STATION.SEND_HB`, `INBOX.STORE_MESSAGE`,
//! `RX.SET_*`, `WINDOW.RAISE`, and a type nobody has heard of are dropped with no answer, which is
//! what JS8Call does with a request it does not accept: with "Accept requests" off (its default),
//! `tcpNetworkMessage`/`udpNetworkMessage` return before `networkMessage`
//! (mainwindow.cpp:6717-6739), and an unknown type falls through it unanswered
//! (networkMessage.cpp:915). [`classify`] is an ALLOW-list, so a type JS8Call adds later stays
//! refused until someone decides it only reads. A TCP line that is no request is told why in
//! JS8Call's `API.ERROR` ([`read_tcp_line`]); a datagram that is none is dropped, as in JS8Call.
//!
//! THE WIRE. A message is `{"params":{…},"type":"…","value":"…"}` in Qt's compact JSON: keys
//! sorted (a `QJsonObject` is a sorted map, and every object here is built through [`object`]),
//! no spaces, a whole-number double printed with no fraction ([`qt_double`]). `_ID` is -1 on an
//! event. On an answer it is the request's own id, or for a request that carried none a fresh
//! one: a NUMBER over TCP (`Client::readyRead` gives the request its id before it is answered,
//! MessageServer.cpp:209) and a STRING over UDP (the answer's own constructor makes it,
//! Message.cpp:75-81). A fresh id counts milliseconds from JS8Call's epoch, 2017-07-06.
//!
//! WHAT NEXUS DOES NOT KEEP is reported as JS8Call reports an empty value: an inbox message filed
//! before this run, whose dial and speed the inbox does not record, reads `DIAL` 0 and `SUBMODE`
//! 0. The selected call (`RX.CALL_SELECTED`, `SELECTED`) is the cockpit's To box, as the station's
//! QSO pause reads it, and "" while the box is empty.

use std::collections::{BTreeMap, VecDeque};

use ::js8::proto::alphabet::sixbit_to_string;
use ::js8::proto::callsign::split_portable;
use ::js8::proto::reassembly::{render_directed, Message, RxFrame};
use ::js8::proto::station::InboxState;
use ::js8::{MessageEvent, TxFrame};
use modes::Js8Speed;
use serde_json::Value;
use tempo_core::logbook::{datetime_utc, QsoRecord};

use super::Engine;
use crate::dto::Tier;

/// JS8Call's TCP API port (`TCPServerPort`, Configuration.cpp:2440).
pub const DEFAULT_TCP_PORT: u16 = 2442;
/// JS8Call's UDP API port (`UDPServerPort`, Configuration.cpp:2427): where the datagrams go.
pub const DEFAULT_UDP_PORT: u16 = 2242;
/// JS8Call's own limit: one TCP connection at a time (`TCPMaxConnections`, Configuration.cpp:2454).
pub const MAX_TCP_CONNECTIONS: usize = 1;
/// The longest request line or datagram read. JS8Call has no cap; every request this API
/// answers is under 200 bytes, and the longest JS8Call accepts at all (a message to send) is a
/// few hundred, so a longer one is not a request.
pub const MAX_REQUEST_BYTES: usize = 16 * 1024;
/// JS8Call's UDP keep-alive (`PING_INTERVAL`, MessageClient.cpp:31).
pub const PING_INTERVAL_MS: u64 = 15_000;
/// What a datagram's `PING` names as the program (JS8Call names itself).
pub const APP_NAME: &str = "Nexus";

/// JS8Call's id epoch, 2017-07-06 00:00 UTC (Message.cpp:15).
const EPOCH_MS: i64 = 1_499_299_200_000;
/// JS8Call's end-of-transmission mark (`EOTCharacter`, Configuration.cpp:2146).
const EOT: &str = "\u{2662}";
/// JS8Call's `STATUS?` text while the operator has set none (`MyStatus`, Configuration.cpp:2151).
const DEFAULT_STATUS: &str = "IDLE <MYIDLE> VERSION <MYVERSION>";
/// Events held for the listeners between two ticks; the oldest goes first past this.
const EVENT_CAP: usize = 1024;
/// Audio offsets kept in the band activity; the stalest goes first past this.
const BAND_CAP: usize = 1024;
/// Lines kept for `RX.TEXT`, which answers the last 1024 characters (networkMessage.cpp:588).
const PANE_LINES: usize = 64;
const PANE_CHARS: usize = 1024;
/// Inbox messages whose dial, speed and drift this run saw arrive.
const INBOX_META_CAP: usize = 256;
/// `TX.TEXT` answers the last 1024 characters of the text going out (networkMessage.cpp:727).
const TX_TEXT_CHARS: usize = 1024;

// ---------------------------------------------------------------------------------------------
// Speeds as JS8Call numbers them
// ---------------------------------------------------------------------------------------------

/// JS8Call's submode number for `speed` (Varicode.h:24-29): Normal 0, Fast 1, JS8 40 2, Slow 4,
/// JS8 60 8. Read off the symbol length, so a speed added later maps without an edit here
/// (`speeds_number_as_js8call_does` holds every speed to a distinct number).
pub fn submode(speed: Js8Speed) -> i64 {
    match speed.nsps() {
        1920 => 0,
        1200 => 1,
        600 => 2,
        3840 => 4,
        384 => 8,
        _ => 0,
    }
}

/// The speed JS8Call numbers `n`, if Nexus has it.
fn speed_of(n: i64) -> Option<Js8Speed> {
    Js8Speed::ALL.iter().copied().find(|s| submode(*s) == n)
}

/// A signal's width at `speed`: eight tones at 12 000 / samples-per-symbol Hz
/// (`JS8::Submode::bandwidth`, JS8Submode.cpp:67).
fn bandwidth(speed: Js8Speed) -> i64 {
    (8 * 12_000 / speed.nsps()) as i64
}

// ---------------------------------------------------------------------------------------------
// The wire: Qt's JSON
// ---------------------------------------------------------------------------------------------

/// One message on its way out: the compact JSON (no newline) and the `_ID` JS8Call's delivery
/// rule reads. A TCP client gets a message the station sends on its own only when that id is
/// 0 or below (`Client::awaitingResponse`, MessageServer.h:57), which is every event, and never
/// `STATION.STATUS`, `PING` or `CLOSE`, whose ids are fresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub line: String,
    pub id: i64,
}

/// An object with its keys in Qt's order (sorted), whatever serde_json's map keeps.
fn object<K: Into<String>>(pairs: impl IntoIterator<Item = (K, Value)>) -> Value {
    let sorted: BTreeMap<String, Value> = pairs.into_iter().map(|(k, v)| (k.into(), v)).collect();
    Value::Object(sorted.into_iter().collect())
}

/// One message on the wire: `{"params":…,"type":…,"value":…}`, compact.
fn wire(kind: &str, value: &str, params: Value) -> String {
    object([
        ("params", params),
        ("type", Value::from(kind)),
        ("value", Value::from(value)),
    ])
    .to_string()
}

/// A double as Qt writes it: a whole number has no fraction (`1`, not `1.0`).
fn qt_double(x: f64) -> Value {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 9.007_199_254_740_992e15 {
        Value::from(x as i64)
    } else {
        serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
    }
}

/// `QVariant::toLongLong` of a parameter: a number (rounded, as Qt rounds a double), a string
/// holding one, a boolean, else 0.
fn qt_long(v: &Value) -> i64 {
    match v {
        Value::Number(n) => n.as_i64().unwrap_or_else(|| {
            n.as_f64()
                .filter(|f| f.is_finite() && f.abs() < 9.2e18)
                .map_or(0, |f| f.round() as i64)
        }),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        Value::Bool(b) => i64::from(*b),
        _ => 0,
    }
}

/// `QVariant::toInt`: [`qt_long`] as an `int`, held at the ends of an int's range.
fn qt_int(v: &Value) -> i64 {
    qt_long(v).clamp(i64::from(i32::MIN), i64::from(i32::MAX))
}

/// A fresh id: milliseconds since JS8Call's epoch (Message.cpp:23-25).
fn fresh_id(now_ms: u64) -> i64 {
    now_ms as i64 - EPOCH_MS
}

/// The UTC time of day, `HH:mm:ss`.
fn hms(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600 % 24, s / 60 % 60, s % 60)
}

/// `yyyy-MM-dd hh:mm:ss`, UTC: how JS8Call's inbox records the time (mainwindow.cpp:6354).
fn utc_text(ms: u64) -> String {
    let s = ms / 1000;
    let days = (s / 86_400) as i64;
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {}", hms(ms))
}

// ---------------------------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------------------------

/// Which socket a request came in on: the answer's fresh id differs (see the module header).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Tcp,
    Udp,
}

/// One request as JS8Call reads it (`Message::fromJson`, Message.cpp:137-155): a JSON object
/// whose `type` and `value` count only as strings and whose `params` count only as an object.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    kind: String,
    value: String,
    params: serde_json::Map<String, Value>,
}

impl Request {
    /// The request in `bytes` (one TCP line, or one datagram), or `None` for anything that is not
    /// a JSON object within [`MAX_REQUEST_BYTES`]. Never panics, whatever the bytes.
    pub fn parse(bytes: &[u8]) -> Option<Request> {
        if bytes.len() > MAX_REQUEST_BYTES {
            return None;
        }
        Request::read(bytes).ok()
    }

    /// The request in `bytes`, or why there is none in the words of JS8Call's `API.ERROR`
    /// (`Message::fromJson`, Message.cpp:119-135, thrown as a `std::system_error` whose words are
    /// "<what>: <category message>", MessageError.cpp:20-23). JSON that does not parse is
    /// "<the parser's words>: json parsing error", in serde's words where JS8Call's are Qt's. Qt's
    /// parser takes only an object or an array as a document, so any other value is its "illegal
    /// value"; an array parses and is "json not an object". Never panics, whatever the bytes.
    pub fn read(bytes: &[u8]) -> Result<Request, String> {
        let mut o = match serde_json::from_slice::<Value>(bytes) {
            Ok(Value::Object(o)) => o,
            Ok(Value::Array(_)) => return Err("json not an object".to_string()),
            Ok(_) => return Err("illegal value: json parsing error".to_string()),
            Err(e) => return Err(format!("{e}: json parsing error")),
        };
        let text = |v: Option<Value>| match v {
            Some(Value::String(s)) => s,
            _ => String::new(),
        };
        let kind = text(o.remove("type"));
        let value = text(o.remove("value"));
        let params = match o.remove("params") {
            Some(Value::Object(p)) => p,
            _ => serde_json::Map::new(),
        };
        Ok(Request {
            kind,
            value,
            params,
        })
    }

    /// The request's `type`.
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// The request's `_ID`, 0 when it carried none (`Message::id`, Message.cpp:36).
    fn id(&self) -> i64 {
        self.params.get("_ID").map_or(0, qt_long)
    }
}

/// The reads this API answers. Everything else is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    CallActivity,
    CallSelected,
    BandActivity,
    Text,
    FreeOffsets,
    Filter,
    Callsign,
    Grid,
    Info,
    Status,
    Spot,
    Os,
    Config,
    Inbox,
    Freq,
    Ptt,
    Speed,
    TxText,
    QueueDepth,
    Version,
}

/// THE ALLOW-LIST: a request this API answers, or `None` for one it refuses — anything that
/// would transmit, tune, change the rig or a setting, or write, and any type not named here.
/// The reads outside the `GET` families are named one by one, never by family.
pub fn classify(kind: &str) -> Option<Read> {
    Some(match kind {
        "RX.GET_CALL_ACTIVITY" => Read::CallActivity,
        "RX.GET_CALL_SELECTED" => Read::CallSelected,
        "RX.GET_BAND_ACTIVITY" => Read::BandActivity,
        "RX.GET_TEXT" => Read::Text,
        "RX.GET_FREE_OFFSETS" => Read::FreeOffsets,
        "RX.GET_FILTER" => Read::Filter,
        "STATION.GET_CALLSIGN" => Read::Callsign,
        "STATION.GET_GRID" => Read::Grid,
        "STATION.GET_INFO" => Read::Info,
        "STATION.GET_STATUS" => Read::Status,
        "STATION.GET_SPOT" => Read::Spot,
        "STATION.GET_OS" => Read::Os,
        "STATION.GET_CONFIG" => Read::Config,
        "INBOX.GET_MESSAGES" => Read::Inbox,
        "RIG.GET_FREQ" => Read::Freq,
        "RIG.GET_PTT" => Read::Ptt,
        "MODE.GET_SPEED" => Read::Speed,
        "TX.GET_TEXT" => Read::TxText,
        "TX.GET_QUEUE_DEPTH" => Read::QueueDepth,
        "STATION.VERSION" => Read::Version,
        _ => return None,
    })
}

/// The answer to `req`, or `None`: refused, or (a free-offsets request for a speed Nexus does
/// not have) unanswerable. Reads `snap` and nothing else.
pub fn answer(req: &Request, via: Via, snap: &Js8ApiSnapshot, now_ms: u64) -> Option<Outgoing> {
    let read = classify(&req.kind)?;
    let (id_value, id) = match (req.id(), via) {
        (0, Via::Tcp) => {
            let id = fresh_id(now_ms);
            (Value::from(id), id)
        }
        (0, Via::Udp) => {
            let id = fresh_id(now_ms);
            (Value::from(id.to_string()), id)
        }
        (id, _) => (Value::from(id), id),
    };
    let with_id = |pairs: Vec<(String, Value)>| {
        object(
            pairs
                .into_iter()
                .chain([("_ID".to_string(), id_value.clone())]),
        )
    };
    let only_id = || with_id(Vec::new());
    let line = match read {
        Read::CallActivity => {
            let aging = u64::from(snap.callsign_aging_min);
            let calls = snap
                .calls
                .iter()
                .filter(|c| aging == 0 || now_ms.saturating_sub(c.utc_ms) / 1000 / 60 < aging)
                .map(|c| {
                    (
                        c.call.clone(),
                        object([
                            ("GRID", Value::from(c.grid.as_str())),
                            ("SNR", Value::from(c.snr)),
                            ("UTC", Value::from(c.utc_ms)),
                        ]),
                    )
                })
                .collect();
            wire("RX.CALL_ACTIVITY", "", with_id(calls))
        }
        Read::CallSelected => wire("RX.CALL_SELECTED", &snap.selected, only_id()),
        // The decode filter as Settings hold it (networkMessage.cpp:665-675).
        Read::Filter => wire(
            "RX.FILTER",
            "",
            with_id(vec![
                ("CENTER".into(), Value::from(snap.filter.center_hz)),
                ("ENABLED".into(), Value::from(snap.filter.enabled)),
                ("WIDTH".into(), Value::from(snap.filter.width_hz)),
            ]),
        ),
        Read::BandActivity => {
            let rows = snap
                .band_activity
                .iter()
                .map(|(offset, b)| {
                    (
                        offset.to_string(),
                        object([
                            ("DIAL", Value::from(b.dial)),
                            ("FREQ", Value::from(b.dial as i64 + i64::from(*offset))),
                            ("OFFSET", Value::from(*offset)),
                            ("SNR", Value::from(b.snr)),
                            ("TEXT", Value::from(b.text.as_str())),
                            ("UTC", Value::from(b.utc_ms)),
                        ]),
                    )
                })
                .collect();
            wire("RX.BAND_ACTIVITY", "", with_id(rows))
        }
        Read::Text => wire("RX.TEXT", &snap.text, only_id()),
        Read::FreeOffsets => {
            let p = &req.params;
            let n = p.get("SPEED").map_or(snap.speed, qt_int);
            let speed = speed_of(n)?;
            let low = p.get("LOW").map_or(500, qt_int);
            let high = p.get("HIGH").map_or(2500, qt_int);
            let free = free_offsets(&snap.band_activity, speed, low, high, now_ms)
                .into_iter()
                .map(|(lo, hi)| {
                    object([
                        ("HIGH", Value::from(hi)),
                        ("LOW", Value::from(lo)),
                        ("WIDTH", Value::from(hi.saturating_sub(lo))),
                    ])
                })
                .collect();
            wire(
                "RX.FREE_OFFSETS",
                "",
                with_id(vec![
                    ("BANDWIDTH".into(), Value::from(bandwidth(speed))),
                    ("FREE".into(), Value::Array(free)),
                    ("HIGH".into(), Value::from(high)),
                    ("LOW".into(), Value::from(low)),
                    ("SPEED".into(), Value::from(n)),
                ]),
            )
        }
        Read::Callsign => wire("STATION.CALLSIGN", &snap.callsign, only_id()),
        Read::Grid => wire("STATION.GRID", &snap.grid, only_id()),
        Read::Info => wire("STATION.INFO", &snap.info, only_id()),
        Read::Status => wire("STATION.STATUS", &snap.status, only_id()),
        Read::Spot => wire(
            "STATION.SPOT",
            "",
            with_id(vec![("value".into(), Value::from(snap.spot))]),
        ),
        Read::Os => {
            let (name, kernel, version) = os();
            wire(
                "STATION.GET_OS",
                "",
                with_id(vec![
                    ("OS_KERNEL".into(), Value::from(kernel)),
                    ("OS_KERNEL_VERSION".into(), Value::from(version)),
                    ("OS_NAME".into(), Value::from(name)),
                ]),
            )
        }
        Read::Config => {
            let c = &snap.config;
            wire(
                "STATION.CONFIG",
                "",
                with_id(vec![
                    (
                        "AUTOREPLY_CONFIRMATION".into(),
                        Value::from(c.autoreply_confirmation),
                    ),
                    ("AUTO_REPLY".into(), Value::from(c.autoreply)),
                    // JS8Call's "Do not participate in the @ALLCALL group" (`avoid_allcall()`,
                    // networkMessage.cpp:332): Settings' *Do not take part in @ALLCALL*.
                    ("AVOID_ALLCALL".into(), Value::from(c.avoid_allcall)),
                    ("CAN_HB".into(), Value::from(c.can_hb)),
                    ("HBACK".into(), Value::from(c.hb_ack)),
                    ("HB_INTERVAL".into(), Value::from(c.hb_interval_min)),
                    ("HB_TIMER_ACTIVE".into(), Value::from(c.hb_on)),
                    // JS8Call's heartbeat networking (its Mode menu's `actionModeJS8HB`).
                    ("JS8HB".into(), Value::from(c.js8hb)),
                    ("MONITOR".into(), Value::from(c.monitor)),
                    ("MULTI_DECODER".into(), Value::from(c.multi_decoder)),
                    (
                        "MY_GROUPS".into(),
                        Value::from(c.groups.iter().map(String::as_str).collect::<Vec<_>>()),
                    ),
                    ("SPEED".into(), Value::from(snap.speed)),
                    ("TX_ENABLED".into(), Value::from(c.tx_enabled)),
                ]),
            )
        }
        Read::Inbox => {
            // JS8Call matches CALLSIGN with SQL `LIKE`, "%" (everything) when it is empty
            // (networkMessage.cpp:824-828).
            let pattern = match req.params.get("CALLSIGN") {
                Some(Value::String(s)) if !s.is_empty() => s.as_str(),
                _ => "%",
            };
            wire(
                "INBOX.MESSAGES",
                "",
                with_id(vec![(
                    "MESSAGES".into(),
                    Value::Array(inbox_records(&snap.inbox, pattern)),
                )]),
            )
        }
        // The dial and the offset (networkMessage.cpp:96-135). JS8Call also sends its WSJT-X
        // status here when its WSJT-X protocol is on; Nexus's WSJT-X feed is its own.
        Read::Freq => wire(
            "RIG.FREQ",
            "",
            with_id(vec![
                ("DIAL".into(), Value::from(snap.dial_hz)),
                (
                    "FREQ".into(),
                    Value::from(snap.dial_hz as i64 + snap.offset_hz),
                ),
                ("OFFSET".into(), Value::from(snap.offset_hz)),
            ]),
        ),
        // The key, and the frame going out while it is down (networkMessage.cpp:51-60).
        Read::Ptt => wire(
            "RIG.PTT_STATUS",
            "",
            with_id(vec![
                (
                    "MESSAGE".into(),
                    Value::from(if snap.transmitting {
                        snap.on_air.as_str()
                    } else {
                        ""
                    }),
                ),
                ("PTT".into(), Value::from(snap.transmitting)),
            ]),
        ),
        Read::Speed => wire(
            "MODE.SPEED",
            "",
            with_id(vec![("SPEED".into(), Value::from(snap.speed))]),
        ),
        Read::TxText => wire("TX.TEXT", &snap.tx_text, only_id()),
        // JS8Call counts its message queue, and a key that is down over an empty one as 1
        // (networkMessage.cpp:759-767).
        Read::QueueDepth => {
            let depth = if snap.transmitting && snap.waiting == 0 {
                1
            } else {
                snap.waiting
            };
            wire(
                "TX.QUEUE_DEPTH",
                "",
                with_id(vec![("DEPTH".into(), Value::from(depth))]),
            )
        }
        Read::Version => wire(
            "STATION.VERSION",
            "",
            with_id(vec![("VERSION".into(), Value::from(snap.version.as_str()))]),
        ),
    };
    Some(Outgoing { line, id })
}

/// JS8Call's free segments of `[low, high]` for a signal at `speed` (networkMessage.cpp:602-663):
/// each offset heard in the last 30 s blocks half its own width plus half the asked-for width on
/// either side; what is left, at least one signal wide, is free. Saturating, so no bound a
/// request can carry overflows.
fn free_offsets(
    band: &[(i32, BandRow)],
    speed: Js8Speed,
    low: i64,
    high: i64,
    now_ms: u64,
) -> Vec<(i64, i64)> {
    let bw = bandwidth(speed);
    let mut occupied: Vec<(i64, i64)> = band
        .iter()
        // `secsTo(now) >= 30` is stale: whole seconds, so 29.9 s still blocks.
        .filter(|(_, b)| now_ms.saturating_sub(b.utc_ms) / 1000 < 30)
        .map(|(offset, b)| {
            let half = bw / 2 + bandwidth(b.speed) / 2;
            (i64::from(*offset) - half, i64::from(*offset) + half)
        })
        .collect();
    occupied.sort_unstable();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (start, end) in occupied {
        match merged.last_mut() {
            Some(last) if last.1 >= start => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    let mut free = Vec::new();
    let mut cursor = low;
    for (start, end) in merged {
        if end <= cursor {
            continue;
        }
        if start > cursor {
            let top = start.min(high);
            if cursor < high && top.saturating_sub(cursor) >= bw {
                free.push((cursor, top));
            }
        }
        cursor = cursor.max(end);
        if cursor >= high {
            break;
        }
    }
    if cursor < high && high.saturating_sub(cursor) >= bw {
        free.push((cursor, high));
    }
    free
}

/// SQLite's `LIKE` as JS8Call's inbox query uses it: `%` any run, `_` any one character, ASCII
/// letters matched without case.
fn like(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().map(|c| c.to_ascii_uppercase()).collect();
    let t: Vec<char> = text.chars().map(|c| c.to_ascii_uppercase()).collect();
    // Iterative wildcard match with one backtrack point.
    let (mut i, mut j) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while j < t.len() {
        if i < p.len() && (p[i] == '_' || p[i] == t[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '%' {
            star = Some((i, j));
            i += 1;
        } else if let Some((si, sj)) = star {
            i = si + 1;
            j = sj + 1;
            star = Some((si, sj + 1));
        } else {
            return false;
        }
    }
    p[i..].iter().all(|c| *c == '%')
}

/// `INBOX.MESSAGES`'s list (networkMessage.cpp:823-862): held messages whose TO matches, then
/// read and unread ones whose FROM matches, at most 1000 of each, newest first by their UTC
/// text (a stable sort, as JS8Call's).
fn inbox_records(rows: &[InboxRow], pattern: &str) -> Vec<Value> {
    let pick = |kind: &'static str, by_to: bool| {
        rows.iter()
            .filter(move |r| r.kind == kind)
            .filter(move |r| like(pattern, if by_to { &r.to } else { &r.from }))
            .take(1000)
    };
    let mut picked: Vec<(String, &InboxRow)> = pick("STORE", true)
        .chain(pick("READ", false))
        .chain(pick("UNREAD", false))
        .map(|r| (utc_text(r.utc_ms), r))
        .collect();
    picked.sort_by(|a, b| b.0.cmp(&a.0));
    picked
        .into_iter()
        .map(|(utc, r)| {
            let mut params = vec![
                ("CMD", Value::from(r.cmd)),
                ("DIAL", Value::from(r.dial)),
                ("FREQ", Value::from(r.dial as i64 + r.offset)),
                ("FROM", Value::from(r.from.as_str())),
                ("OFFSET", Value::from(r.offset)),
                ("PATH", Value::from(r.path.as_str())),
                ("SNR", Value::from(r.snr)),
                ("SUBMODE", Value::from(r.submode)),
                ("TDRIFT", qt_double(f64::from(r.tdrift))),
                ("TO", Value::from(r.to.as_str())),
                ("UTC", Value::from(utc)),
                // Stored records carry the id their own constructor made: a string.
                ("_ID", Value::from(fresh_id(r.utc_ms).to_string())),
            ];
            // JS8Call stores TEXT only when there is some (mainwindow.cpp:6375-6377).
            if !r.text.is_empty() {
                params.push(("TEXT", Value::from(r.text.as_str())));
            }
            object([
                ("params", object(params)),
                ("type", Value::from(r.kind)),
                ("value", Value::from("")),
            ])
        })
        .collect()
}

/// `STATION.GET_OS`'s three facts (Qt's `prettyProductName`, `kernelType`, `kernelVersion`),
/// read once. Linux names its release; Windows and macOS give their names with no version.
fn os() -> (String, String, String) {
    static OS: std::sync::OnceLock<(String, String, String)> = std::sync::OnceLock::new();
    OS.get_or_init(|| {
        let kernel = match std::env::consts::OS {
            "windows" => "winnt",
            "macos" => "darwin",
            other => other,
        };
        let read = |path: &str| std::fs::read_to_string(path).unwrap_or_default();
        let name = match std::env::consts::OS {
            "linux" => read("/etc/os-release")
                .lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| v.trim().trim_matches('"').to_string())
                .unwrap_or_else(|| "Linux".to_string()),
            "windows" => "Windows".to_string(),
            "macos" => "macOS".to_string(),
            other => other.to_string(),
        };
        let version = if std::env::consts::OS == "linux" {
            read("/proc/sys/kernel/osrelease").trim().to_string()
        } else {
            String::new()
        };
        (name, kernel.to_string(), version)
    })
    .clone()
}

// ---------------------------------------------------------------------------------------------
// What the station says on its own
// ---------------------------------------------------------------------------------------------

/// A message the station sends unasked, as the engine saw it happen.
#[derive(Debug, Clone, PartialEq)]
pub enum Js8ApiEvent {
    /// A decoded frame (processRxActivity.cpp:24-34).
    Activity {
        text: String,
        dial: u64,
        offset: i32,
        snr: i32,
        speed: i64,
        tdrift: f32,
        utc_ms: u64,
        bits: u8,
    },
    /// A complete directed command (processCommandActivity.cpp:159-176).
    Directed {
        from: String,
        to: String,
        cmd: String,
        grid: String,
        extra: String,
        text: String,
        dial: u64,
        offset: i32,
        snr: i32,
        speed: i64,
        tdrift: f32,
        utc_ms: u64,
    },
    /// A station spotted, while spotting is on (mainwindow.cpp:6564-6574).
    Spot {
        call: String,
        grid: String,
        dial: u64,
        offset: i32,
        snr: i32,
    },
    /// The tones of a frame going out (mainwindow.cpp:6702-6715).
    TxFrame { tones: Vec<u8> },
    /// A JS8 contact logged (`acceptQSO`, mainwindow.cpp:3920-3940).
    LogQso(LoggedContact),
}

/// A JS8 contact as JS8Call's `LOG.QSO` tells it, from the record the log took.
#[derive(Debug, Clone, PartialEq)]
pub struct LoggedContact {
    pub call: String,
    pub grid: String,
    /// The frequency it was worked on, the dial plus the TX offset, in Hz.
    pub freq_hz: u64,
    pub band: String,
    pub rpt_sent: String,
    pub rpt_recv: String,
    pub name: String,
    pub comments: String,
    /// Who was at the key: the record's operator, else the station's call.
    pub op: String,
    pub station_call: String,
    pub station_grid: String,
    pub utc_on_ms: u64,
    pub utc_off_ms: u64,
}

impl LoggedContact {
    /// The contact as JS8Call's log writes it, which is `LOG.QSO`'s value (`ADIF::QSOToADIF`,
    /// ADIF.cpp:385-436): lower-case tags one space apart, each length counted as JS8Call counts
    /// it (UTF-16 units), the mode MFSK with the submode JS8 (`LogQSO::accept`,
    /// LogQSO.cpp:252-255), the dates and times UTC, the frequency in MHz to six places, and the
    /// comment, the name and the operator only where there is one.
    fn adif(&self) -> String {
        let tag =
            |name: &str, value: &str| format!("<{name}:{}>{value}", value.encode_utf16().count());
        let date_time = |ms: u64| {
            let (y, mo, d, h, mi, s) = datetime_utc(ms / 1000);
            (
                format!("{y:04}{mo:02}{d:02}"),
                format!("{h:02}{mi:02}{s:02}"),
            )
        };
        let (date_on, time_on) = date_time(self.utc_on_ms);
        let (date_off, time_off) = date_time(self.utc_off_ms);
        let mut fields = vec![
            tag("call", &self.call),
            tag("gridsquare", &self.grid),
            tag("mode", "MFSK"),
            tag("submode", "JS8"),
            tag("rst_sent", &self.rpt_sent),
            tag("rst_rcvd", &self.rpt_recv),
            tag("qso_date", &date_on),
            tag("time_on", &time_on),
            tag("qso_date_off", &date_off),
            tag("time_off", &time_off),
            tag("band", &self.band),
            tag("freq", &format!("{:.6}", self.freq_hz as f64 / 1e6)),
            tag("station_callsign", &self.station_call),
            tag("my_gridsquare", &self.station_grid),
        ];
        for (name, value) in [
            ("comment", &self.comments),
            ("name", &self.name),
            ("operator", &self.op),
        ] {
            if !value.is_empty() {
                fields.push(tag(name, value));
            }
        }
        fields.join(" ")
    }
}

impl Js8ApiEvent {
    /// The line JS8Call sends for it. Every event's id is -1.
    pub fn to_outgoing(&self) -> Outgoing {
        let event =
            |pairs: Vec<(&str, Value)>| object(pairs.into_iter().chain([("_ID", Value::from(-1))]));
        let line = match self {
            Js8ApiEvent::Activity {
                text,
                dial,
                offset,
                snr,
                speed,
                tdrift,
                utc_ms,
                bits,
            } => wire(
                "RX.ACTIVITY",
                text,
                event(vec![
                    ("BITS", Value::from(*bits)),
                    ("DIAL", Value::from(*dial)),
                    ("FREQ", Value::from(*dial as i64 + i64::from(*offset))),
                    ("OFFSET", Value::from(*offset)),
                    ("SNR", Value::from(*snr)),
                    ("SPEED", Value::from(*speed)),
                    ("TDRIFT", qt_double(f64::from(*tdrift))),
                    ("UTC", Value::from(*utc_ms)),
                ]),
            ),
            Js8ApiEvent::Directed {
                from,
                to,
                cmd,
                grid,
                extra,
                text,
                dial,
                offset,
                snr,
                speed,
                tdrift,
                utc_ms,
            } => wire(
                "RX.DIRECTED",
                text,
                event(vec![
                    ("CMD", Value::from(cmd.as_str())),
                    ("DIAL", Value::from(*dial)),
                    ("EXTRA", Value::from(extra.as_str())),
                    ("FREQ", Value::from(*dial as i64 + i64::from(*offset))),
                    ("FROM", Value::from(from.as_str())),
                    ("GRID", Value::from(grid.as_str())),
                    ("OFFSET", Value::from(*offset)),
                    ("SNR", Value::from(*snr)),
                    ("SPEED", Value::from(*speed)),
                    ("TDRIFT", qt_double(f64::from(*tdrift))),
                    ("TEXT", Value::from(text.as_str())),
                    ("TO", Value::from(to.as_str())),
                    ("UTC", Value::from(*utc_ms)),
                ]),
            ),
            Js8ApiEvent::Spot {
                call,
                grid,
                dial,
                offset,
                snr,
            } => wire(
                "RX.SPOT",
                "",
                event(vec![
                    ("CALL", Value::from(call.as_str())),
                    ("DIAL", Value::from(*dial)),
                    ("FREQ", Value::from(*dial as i64 + i64::from(*offset))),
                    ("GRID", Value::from(grid.as_str())),
                    ("OFFSET", Value::from(*offset)),
                    ("SNR", Value::from(*snr)),
                ]),
            ),
            Js8ApiEvent::TxFrame { tones } => wire(
                "TX.FRAME",
                "",
                event(vec![("TONES", Value::from(tones.clone()))]),
            ),
            Js8ApiEvent::LogQso(c) => wire(
                "LOG.QSO",
                &c.adif(),
                event(vec![
                    ("CALL", Value::from(c.call.as_str())),
                    ("COMMENTS", Value::from(c.comments.as_str())),
                    // The fields JS8Call's log dialog adds by name; Nexus's log form has none.
                    ("EXTRA", Value::Object(serde_json::Map::new())),
                    ("FREQ", Value::from(c.freq_hz)),
                    ("GRID", Value::from(c.grid.as_str())),
                    ("MODE", Value::from("MFSK")),
                    ("NAME", Value::from(c.name.as_str())),
                    ("RPT.RECV", Value::from(c.rpt_recv.as_str())),
                    ("RPT.SENT", Value::from(c.rpt_sent.as_str())),
                    ("STATION.CALL", Value::from(c.station_call.as_str())),
                    ("STATION.GRID", Value::from(c.station_grid.as_str())),
                    ("STATION.OP", Value::from(c.op.as_str())),
                    ("SUBMODE", Value::from("JS8")),
                    ("UTC.OFF", Value::from(c.utc_off_ms)),
                    ("UTC.ON", Value::from(c.utc_on_ms)),
                ]),
            ),
        };
        Outgoing { line, id: -1 }
    }
}

/// `RIG.PTT` (mainwindow.cpp:6685-6700).
fn rig_ptt(on: bool, now_ms: u64) -> Outgoing {
    let params = object([
        ("PTT", Value::from(on)),
        ("UTC", Value::from(now_ms)),
        ("_ID", Value::from(-1)),
    ]);
    Outgoing {
        line: wire("RIG.PTT", if on { "on" } else { "off" }, params),
        id: -1,
    }
}

/// `RIG.FREQ` as a band change announces it (mainwindow.cpp:1325-1332).
fn rig_freq(snap: &Js8ApiSnapshot) -> Outgoing {
    let params = object([
        ("BAND", Value::from(snap.band.as_str())),
        ("DIAL", Value::from(snap.dial_hz)),
        ("FREQ", Value::from(snap.dial_hz as i64 + snap.offset_hz)),
        ("OFFSET", Value::from(snap.offset_hz)),
        ("_ID", Value::from(-1)),
    ]);
    Outgoing {
        line: wire("RIG.FREQ", "", params),
        id: -1,
    }
}

/// `STATION.STATUS` as the station announces a change (mainwindow.cpp:6812-6858). It names no
/// id, so it gets a fresh one, and a TCP client never receives it: JS8Call sends it over UDP only.
fn station_status(snap: &Js8ApiSnapshot, now_ms: u64) -> Outgoing {
    let id = fresh_id(now_ms);
    let params = object([
        ("DIAL", Value::from(snap.dial_hz)),
        ("FREQ", Value::from(snap.dial_hz as i64 + snap.offset_hz)),
        ("OFFSET", Value::from(snap.offset_hz)),
        ("SELECTED", Value::from(snap.selected.as_str())),
        ("SPEED", Value::from(snap.speed)),
        ("_ID", Value::from(id.to_string())),
    ]);
    Outgoing {
        line: wire("STATION.STATUS", "", params),
        id,
    }
}

/// `STATION.CLOSING`, as the program closes (mainwindow.cpp:1478-1482).
pub fn closing() -> Outgoing {
    let params = object([
        ("REASON", Value::from("User closed application")),
        ("_ID", Value::from(-1)),
    ]);
    Outgoing {
        line: wire("STATION.CLOSING", "", params),
        id: -1,
    }
}

/// The UDP side's keep-alive (MessageClient.cpp:92-102).
pub fn ping(version: &str, now_ms: u64) -> Outgoing {
    let id = fresh_id(now_ms);
    let params = object([
        ("NAME", Value::from(APP_NAME)),
        ("UTC", Value::from(now_ms)),
        ("VERSION", Value::from(version)),
        ("_ID", Value::from(id.to_string())),
    ]);
    Outgoing {
        line: wire("PING", "", params),
        id,
    }
}

/// The UDP side's goodbye (MessageClient.cpp:85-87).
pub fn close(now_ms: u64) -> Outgoing {
    let id = fresh_id(now_ms);
    let params = object([("_ID", Value::from(id.to_string()))]);
    Outgoing {
        line: wire("CLOSE", "", params),
        id,
    }
}

/// What a TCP connection past the limit is told before it is closed (MessageServer.cpp:130-136).
pub fn connections_full(now_ms: u64) -> Outgoing {
    api_error("Connections Full", now_ms)
}

/// `API.ERROR`, which JS8Call's TCP server tells one program: `why`, under a fresh id that is a
/// string, as `Message`'s two-argument constructor makes it (Message.cpp:70-73).
pub fn api_error(why: &str, now_ms: u64) -> Outgoing {
    let id = fresh_id(now_ms);
    let params = object([("_ID", Value::from(id.to_string()))]);
    Outgoing {
        line: wire("API.ERROR", why, params),
        id,
    }
}

/// One line a TCP program sent, as JS8Call's server takes it (`Client::readyRead`,
/// MessageServer.cpp:199-214): trimmed, a blank one passed over, and one that is no request
/// answered `API.ERROR` to that program alone. The server answers it before the request reaches
/// the Accept switch (mainwindow.cpp:6729-6739), so it is answered with the switch off too.
#[derive(Debug, Clone, PartialEq)]
pub enum TcpLine {
    /// Nothing on it but whitespace: passed over, unanswered.
    Blank,
    /// A request, for the side's Accept switch and then [`answer`].
    Request(Request),
    /// No request: the `API.ERROR` that program is told.
    Error(Outgoing),
}

/// `line` (its newline taken off) as JS8Call's TCP server reads it. The listener keeps lines
/// past [`MAX_REQUEST_BYTES`] from here: it closes that connection instead.
pub fn read_tcp_line(line: &[u8], now_ms: u64) -> TcpLine {
    // `QByteArray::trimmed`: ASCII whitespace, `\v` and `\f` among it, off both ends.
    let space = |c: &u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let start = line.iter().position(|c| !space(c)).unwrap_or(line.len());
    let end = line
        .iter()
        .rposition(|c| !space(c))
        .map_or(start, |i| i + 1);
    let line = &line[start..end];
    if line.is_empty() {
        return TcpLine::Blank;
    }
    match Request::read(line) {
        Ok(r) => TcpLine::Request(r),
        Err(why) => TcpLine::Error(api_error(&why, now_ms)),
    }
}

/// The changes the station announces between two looks at it: the transmitter, the band, and
/// the dial, offset, speed and selection `STATION.STATUS` carries.
#[derive(Debug, Default)]
pub struct StateWatch {
    last: Option<Js8ApiSnapshot>,
}

impl StateWatch {
    /// The announcements `snap` calls for, as (before the tick's events, after them): the key
    /// going down is announced after the frames that went out under it, as JS8Call orders them.
    /// The first look announces the band and the status, as JS8Call does when it starts.
    pub fn step(&mut self, snap: &Js8ApiSnapshot, now_ms: u64) -> (Vec<Outgoing>, Vec<Outgoing>) {
        let (mut before, mut after) = (Vec::new(), Vec::new());
        let status = |s: &Js8ApiSnapshot| (s.dial_hz, s.offset_hz, s.speed, s.selected.clone());
        match &self.last {
            None => {
                before.push(rig_freq(snap));
                if snap.transmitting {
                    before.push(rig_ptt(true, now_ms));
                }
                before.push(station_status(snap, now_ms));
            }
            Some(last) => {
                if last.band != snap.band {
                    before.push(rig_freq(snap));
                }
                if !last.transmitting && snap.transmitting {
                    before.push(rig_ptt(true, now_ms));
                    before.push(station_status(snap, now_ms));
                } else if last.transmitting && !snap.transmitting {
                    after.push(rig_ptt(false, now_ms));
                    after.push(station_status(snap, now_ms));
                } else if status(last) != status(snap) {
                    before.push(station_status(snap, now_ms));
                }
            }
        }
        self.last = Some(Js8ApiSnapshot {
            band: snap.band.clone(),
            transmitting: snap.transmitting,
            dial_hz: snap.dial_hz,
            offset_hz: snap.offset_hz,
            speed: snap.speed,
            selected: snap.selected.clone(),
            ..Js8ApiSnapshot::default()
        });
        (before, after)
    }
}

// ---------------------------------------------------------------------------------------------
// The snapshot the reads are answered from
// ---------------------------------------------------------------------------------------------

/// One heard station, as `RX.CALL_ACTIVITY` lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct CallRow {
    pub call: String,
    pub grid: String,
    pub snr: i32,
    pub utc_ms: u64,
}

/// The newest frame at one audio offset: an entry of JS8Call's band activity.
#[derive(Debug, Clone, PartialEq)]
pub struct BandRow {
    pub dial: u64,
    pub text: String,
    pub snr: i32,
    pub utc_ms: u64,
    pub speed: Js8Speed,
}

/// One inbox message, as `INBOX.MESSAGES` lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct InboxRow {
    /// `UNREAD`, `READ`, `STORE` (held for another station) or `DELIVERED`.
    pub kind: &'static str,
    pub cmd: &'static str,
    pub from: String,
    pub to: String,
    pub text: String,
    pub path: String,
    pub utc_ms: u64,
    pub dial: u64,
    pub offset: i64,
    pub snr: i32,
    pub submode: i64,
    pub tdrift: f32,
}

/// `STATION.CONFIG`'s switches.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConfigRow {
    pub autoreply: bool,
    pub autoreply_confirmation: bool,
    pub can_hb: bool,
    pub hb_ack: bool,
    pub hb_interval_min: u16,
    pub hb_on: bool,
    pub monitor: bool,
    pub multi_decoder: bool,
    pub groups: Vec<String>,
    pub tx_enabled: bool,
    /// Heartbeat networking (`JS8HB`): the Settings switch.
    pub js8hb: bool,
    /// Not taking part in @ALLCALL (`AVOID_ALLCALL`): the Settings switch.
    pub avoid_allcall: bool,
}

/// Everything a read is answered from, copied out of the Engine.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Js8ApiSnapshot {
    pub callsign: String,
    pub grid: String,
    pub info: String,
    pub status: String,
    /// Spotting on (`STATION.SPOT`): Nexus's PSK Reporter switch.
    pub spot: bool,
    pub dial_hz: u64,
    /// The TX offset, JS8Call's `freq()`.
    pub offset_hz: i64,
    /// The transmit speed, as JS8Call numbers it.
    pub speed: i64,
    pub selected: String,
    /// The key is down: an over on the air, or a tune carrier.
    pub transmitting: bool,
    pub band: String,
    pub calls: Vec<CallRow>,
    pub callsign_aging_min: u16,
    pub band_activity: Vec<(i32, BandRow)>,
    /// The decode filter (`RX.FILTER`).
    pub filter: crate::settings::Js8Filter,
    /// `RX.TEXT`: the last 1024 characters of the directed pane.
    pub text: String,
    pub inbox: Vec<InboxRow>,
    pub config: ConfigRow,
    /// What is on the air, as `RIG.PTT_STATUS`'s `MESSAGE` names it: the JS8 frame in JS8Call's
    /// twelve characters, "TUNE" for a tune carrier, empty while neither is keyed.
    pub on_air: String,
    /// The text going out (`TX.TEXT`): the message JS8Call's compose box would hold.
    pub tx_text: String,
    /// The messages waiting behind the one in the compose box (`TX.QUEUE_DEPTH`).
    pub waiting: i64,
    /// The program's version (`STATION.VERSION`): the listeners fill it in.
    pub version: String,
}

/// One tick's worth for the listeners: the ports to serve, what happened, and the state.
#[derive(Debug, Clone)]
pub struct Js8ApiPoll {
    /// `Some(port)` while TCP is on.
    pub tcp_port: Option<u16>,
    /// `Some(port)` while UDP is on: where the datagrams go.
    pub udp_port: Option<u16>,
    /// Requests a TCP program sends are answered ("Accept TCP requests").
    pub tcp_accept: bool,
    /// Requests a UDP program sends are answered ("Accept UDP requests").
    pub udp_accept: bool,
    pub events: Vec<Js8ApiEvent>,
    pub snapshot: Js8ApiSnapshot,
}

// ---------------------------------------------------------------------------------------------
// The engine's side: what it keeps for the API
// ---------------------------------------------------------------------------------------------

/// An inbox message as the station files it: (from, when filed, the offset's bits).
type InboxKey = (String, u64, u32);
/// What this run saw of it arriving: (dial, JS8Call's speed number, drift).
type InboxMeta = (u64, i64, f32);

/// What the engine keeps for the API between two ticks: the events, and the JS8Call state the
/// rest of Nexus has no use for (band activity by offset with its text, the directed pane).
#[derive(Debug, Default)]
pub struct Js8ApiTap {
    events: VecDeque<Js8ApiEvent>,
    band: BTreeMap<i32, BandRow>,
    /// The band `band` was heard on: a band change clears it, as JS8Call's `clearActivity`.
    band_name: String,
    pane: VecDeque<String>,
    /// The pane's last line is a transmission still going out.
    tx_open: bool,
    /// The frame just decoded: its drift and whether it ended a message.
    last_frame: Option<(f32, bool)>,
    /// The inbox messages this run saw arrive, and what it saw of each.
    inbox_meta: VecDeque<(InboxKey, InboxMeta)>,
    /// The JS8 frame keyed last: its twelve characters, whether it ended its message, and the
    /// time its period is over, which no over outlasts.
    on_air: Option<(String, bool, u64)>,
}

impl Js8ApiTap {
    fn push(&mut self, ev: Js8ApiEvent) {
        self.events.push_back(ev);
        while self.events.len() > EVENT_CAP {
            self.events.pop_front();
        }
    }

    fn pane_line(&mut self, line: String) {
        self.pane.push_back(line);
        while self.pane.len() > PANE_LINES {
            self.pane.pop_front();
        }
    }

    /// JS8Call's band activity filing (processDecodeEvent.cpp:283-299, :346-349): an offset not
    /// yet filed first takes over the filed offset within the speed's drift that comes first
    /// counting up, so a drifting signal keeps one entry.
    fn file_band(&mut self, rx: &RxFrame, dial: u64) {
        let offset = rx.freq_hz as i32;
        if !self.band.contains_key(&offset) {
            let range = rx.speed.drift_hz() as i32;
            let prev = self
                .band
                .range(offset - range..=offset + range)
                .next()
                .map(|(&o, _)| o);
            if let Some(prev) = prev {
                self.band.remove(&prev);
            }
        }
        self.band.insert(
            offset,
            BandRow {
                dial,
                text: rx.display.clone(),
                snr: rx.snr_db,
                utc_ms: rx.at_ms,
                speed: rx.speed,
            },
        );
        while self.band.len() > BAND_CAP {
            let stalest = self
                .band
                .iter()
                .min_by_key(|(_, b)| b.utc_ms)
                .map(|(&o, _)| o);
            match stalest {
                Some(o) => self.band.remove(&o),
                None => break,
            };
        }
    }
}

/// The directed command JS8Call's `RX.DIRECTED` carries for `m`: (cmd, extra, text). The text
/// is 3.0.3's `baseText`, "TO CMD EXTRA TEXT" with the end mark when the message is finished
/// (processCommandActivity.cpp:127-152), which is the DIRECTED.TXT line less its sender.
fn directed_parts(m: &Message, last: bool) -> (String, String, String) {
    let (cmd, extra) = if m.is_heartbeat() {
        let cmd = if m.cq.is_some() { " CQ" } else { " HEARTBEAT" };
        (cmd.to_string(), String::new())
    } else {
        let extra = match (m.cmd, m.num) {
            (Some(c), Some(n)) if c.carries_snr() => ::js8::proto::frame::format_snr(n as i32),
            (_, Some(n)) => n.to_string(),
            _ => String::new(),
        };
        (m.cmd.map_or("", |c| c.text()).to_string(), extra)
    };
    let full = render_directed(m);
    let prefix = format!("{}: ", m.from);
    let base = full.strip_prefix(&prefix).unwrap_or(&full);
    let text = if last {
        format!("{} {EOT} ", base.trim_end())
    } else {
        base.to_string()
    };
    (cmd, extra, text)
}

impl Engine {
    /// TEST SEAM: a heartbeat from `call` at `grid`, heard at `freq_hz` — the row the JS8 decoder
    /// hands `js8_ingest`, for the listeners' tests, which reach the engine only from outside.
    #[cfg(any(test, feature = "test-util"))]
    pub fn js8_api_hear_heartbeat(&mut self, call: &str, grid: &str, freq_hz: f32, snr: i32) {
        let frame = ::js8::Frame::Heartbeat {
            call: call.to_string(),
            grid: Some(grid.to_string()),
            is_cq: false,
            idx: 0,
        };
        let i3 = ::js8::I3 {
            first: true,
            last: true,
            data: false,
        };
        let word = ::js8::proto::frame::encode_frame(&frame, i3, Js8Speed::Normal)
            .expect("a heartbeat packs");
        let decode = modes::Decode {
            message: frame.render(),
            sync: 5.0,
            snr,
            dt: 0.1,
            freq: freq_hz,
            nap: 0,
            qual: 0.9,
            rv: None,
            mode: Some(modes::ModeKind::Js8 {
                speed: Js8Speed::Normal,
            }),
            raw: Some(*word.as_bytes()),
            unchecked: false,
        };
        self.js8_ingest(&[decode], 0);
    }

    /// TEST SEAM: `<call>: <own call> SNR?`, a directed query the station may answer with an
    /// automatic reply, as the JS8 decoder hands it to `js8_ingest`: for tests that reach the
    /// engine only from outside.
    #[cfg(any(test, feature = "test-util"))]
    pub fn js8_api_hear_snr_query(&mut self, call: &str) {
        use ::js8::proto::callsign::CallRef;
        let frame = ::js8::Frame::Directed {
            from: CallRef::Base(call.to_string()),
            to: CallRef::Base(self.settings.mycall.clone()),
            cmd: ::js8::Command::SnrQuery,
            num: None,
            portable_from: false,
            portable_to: false,
        };
        let i3 = ::js8::I3 {
            first: true,
            last: true,
            data: false,
        };
        let word =
            ::js8::proto::frame::encode_frame(&frame, i3, Js8Speed::Normal).expect("a query packs");
        let decode = modes::Decode {
            message: frame.render(),
            sync: 5.0,
            snr: -5,
            dt: 0.1,
            freq: 1500.0,
            nap: 0,
            qual: 0.9,
            rv: None,
            mode: Some(modes::ModeKind::Js8 {
                speed: Js8Speed::Normal,
            }),
            raw: Some(*word.as_bytes()),
            unchecked: false,
        };
        self.js8_ingest(&[decode], 0);
    }

    /// The API is on: TCP or UDP.
    fn js8_api_on(&self) -> bool {
        self.settings.js8_api_tcp || self.settings.js8_api_udp
    }

    /// The API's view of one reassembler event, at `now_ms` (the time the station files it).
    /// Keeps the band activity and the directed pane whether or not the API is on, so a program
    /// that connects later reads what JS8Call would show it; queues events only while it is on.
    pub(crate) fn js8_api_observe(&mut self, ev: &MessageEvent, now_ms: u64) {
        let dial = self.settings.dial_hz();
        if self.js8_api.band_name != self.settings.band {
            self.js8_api.band.clear();
            self.js8_api.band_name = self.settings.band.clone();
        }
        let on = self.js8_api_on();
        match ev {
            MessageEvent::Frame(rx) => {
                self.js8_api.file_band(rx, dial);
                self.js8_api.last_frame = Some((rx.dt_s, rx.i3.last));
                if on {
                    self.js8_api.push(Js8ApiEvent::Activity {
                        text: rx.display.clone(),
                        dial,
                        offset: rx.freq_hz as i32,
                        snr: rx.snr_db,
                        speed: submode(rx.speed),
                        tdrift: rx.dt_s,
                        utc_ms: rx.at_ms,
                        bits: rx.i3.to_u8(),
                    });
                }
            }
            // JS8Call processes a command only once it is whole and its checksum holds
            // (processBufferedActivity.cpp:84-96); a message closed incomplete is not one.
            MessageEvent::Message(m) if m.complete => {
                // A single frame ends the message when its own bit says so; a buffered one is
                // closed by its last frame (processBufferedActivity.cpp:86).
                let (tdrift, frame_last) = self.js8_api.last_frame.unwrap_or((0.0, true));
                let last = m.frames > 1 || frame_last;
                let (cmd, extra, text) = directed_parts(m, last);
                let offset = m.freq_hz as i32;
                let speed = submode(m.speed);
                self.js8_api.inbox_meta.push_back((
                    (m.from.to_ascii_uppercase(), now_ms, m.freq_hz.to_bits()),
                    (dial, speed, tdrift),
                ));
                while self.js8_api.inbox_meta.len() > INBOX_META_CAP {
                    self.js8_api.inbox_meta.pop_front();
                }
                if self.js8_api_pane_shows(m, &cmd, &text) {
                    let line = format!("{} - ({offset}) - {}: {text}", hms(m.first_ms), m.from);
                    self.js8_api.pane_line(line);
                    self.js8_api.tx_open = false;
                }
                if !on {
                    return;
                }
                let grid = m.grid.clone().unwrap_or_default();
                self.js8_api.push(Js8ApiEvent::Directed {
                    from: m.from.clone(),
                    to: m.to_text.clone(),
                    cmd,
                    grid: grid.clone(),
                    extra,
                    text,
                    dial,
                    offset,
                    snr: m.snr_db,
                    speed,
                    tdrift,
                    utc_ms: m.first_ms,
                });
                // Every station a command comes from is spotted while spotting is on, with the
                // grid it last gave (`logCallActivity`, mainwindow.cpp:2285-2324); never a relay
                // path.
                if self.settings.pskreporter && !m.from.is_empty() && !m.from.contains('>') {
                    let grid = if grid.is_empty() {
                        self.js8_station
                            .heard()
                            .iter()
                            .find(|h| h.call.eq_ignore_ascii_case(&m.from))
                            .and_then(|h| h.grid.clone())
                            .unwrap_or_default()
                    } else {
                        grid
                    };
                    self.js8_api.push(Js8ApiEvent::Spot {
                        call: m.from.clone(),
                        grid,
                        dial,
                        offset,
                        snr: m.snr_db,
                    });
                }
            }
            MessageEvent::Message(_) => {}
        }
    }

    /// JS8Call's directed pane takes a command addressed to my call or its base, to a group I
    /// have joined, or free text to @ALLCALL, never a heartbeat
    /// (processCommandActivity.cpp:181-210).
    fn js8_api_pane_shows(&self, m: &Message, cmd: &str, text: &str) -> bool {
        let my = self.settings.mycall.trim().to_ascii_uppercase();
        let (base, _) = split_portable(&my);
        let to = m.to_text.to_ascii_uppercase();
        let to_me = !my.is_empty() && (to == my || to == base);
        let group = to == "@APRSIS"
            || self
                .settings
                .js8_groups
                .iter()
                .any(|g| g.trim().eq_ignore_ascii_case(&to));
        let allcall = to == "@ALLCALL";
        if allcall {
            return cmd == " " && !text.contains("@HB HEARTBEAT");
        }
        to_me || group
    }

    /// The API's view of a frame keyed at `f0` (plan time, where JS8Call's `emitTones` runs):
    /// its tones while the API is on, and the directed pane's transmit line either way. Reads the
    /// frame; changes nothing that is keyed.
    pub(crate) fn js8_api_note_tx(&mut self, tf: &TxFrame, speed: Js8Speed, f0: f32) {
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        if tf.first || !self.js8_api.tx_open {
            let line = format!("{} - ({}) - {}", hms(now_ms), f0 as i32, tf.display);
            self.js8_api.pane_line(line);
        } else if let Some(open) = self.js8_api.pane.back_mut() {
            open.push_str(&tf.display);
        }
        if tf.last {
            if let Some(open) = self.js8_api.pane.back_mut() {
                let closed = format!("{} {EOT} ", open.trim_end());
                *open = closed;
            }
        }
        self.js8_api.tx_open = !tf.last;
        // JS8Call names the frame it keys in its twelve characters (`m_currentMessage`,
        // mainwindow.cpp:2602-2617).
        let period_ms = u64::from(speed.period_s()) * 1000;
        self.js8_api.on_air = Some((
            sixbit_to_string(&tf.word.payload72().chars12()),
            tf.last,
            now_ms + period_ms,
        ));
        if self.js8_api_on() {
            let tones = ::js8::phy::encode_word(&tf.word, speed).to_vec();
            self.js8_api.push(Js8ApiEvent::TxFrame { tones });
        }
    }

    /// A contact the log took, at the logged-contact fan-out beside the WSJT-X sink's
    /// (`log_qso`): a JS8 one is told to the API's programs as JS8Call 3.0.3's `acceptQSO` tells
    /// them (`LOG.QSO`, mainwindow.cpp:3920-3940), while the API is on. Reads the record.
    pub(crate) fn js8_api_note_logged(&mut self, rec: &QsoRecord) {
        if !self.js8_api_on() || !rec.mode.trim().eq_ignore_ascii_case("JS8") {
            return;
        }
        let s = &self.settings;
        // As JS8Call's log dialog has them: the call and grid upper-cased (`LogQSO::accept`,
        // LogQSO.cpp:249-250), the frequency the dial plus the TX offset
        // (mainwindow.cpp:3901-3902), the operator its op call or its own (:3878-3881).
        let station_call = rec
            .station_callsign
            .clone()
            .unwrap_or_else(|| s.mycall.trim().to_ascii_uppercase());
        let utc_on_ms = rec.when_unix * 1000;
        let contact = LoggedContact {
            call: rec.call.trim().to_ascii_uppercase(),
            grid: rec
                .grid
                .as_deref()
                .unwrap_or_default()
                .trim()
                .to_ascii_uppercase(),
            freq_hz: (rec.freq_mhz * 1e6).round().max(0.0) as u64,
            band: rec.band.clone(),
            rpt_sent: rec.rst_sent.clone().unwrap_or_default(),
            rpt_recv: rec.rst_rcvd.clone().unwrap_or_default(),
            name: rec.name.clone().unwrap_or_default(),
            comments: rec.comment.clone().unwrap_or_default(),
            op: rec.operator.clone().unwrap_or_else(|| station_call.clone()),
            station_grid: rec
                .my_grid
                .clone()
                .unwrap_or_else(|| s.mygrid.trim().to_string()),
            station_call,
            utc_on_ms,
            utc_off_ms: rec.time_off_unix.map_or(utc_on_ms, |t| t * 1000),
        };
        self.js8_api.push(Js8ApiEvent::LogQso(contact));
    }

    /// One tick for the listeners: `None` while both servers are off (and nothing is held), else
    /// the ports, the events since the last tick, and a snapshot to answer reads from.
    pub fn js8_api_poll(&mut self) -> Option<Js8ApiPoll> {
        if !self.js8_api_on() {
            self.js8_api.events.clear();
            return None;
        }
        let s = &self.settings;
        let tcp_port = s.js8_api_tcp.then_some(s.js8_api_tcp_port);
        let udp_port = s.js8_api_udp.then_some(s.js8_api_udp_port);
        let events = self.js8_api.events.drain(..).collect();
        let snapshot = self.js8_api_snapshot();
        Some(Js8ApiPoll {
            tcp_port,
            udp_port,
            tcp_accept: s.js8_api_tcp_accept,
            udp_accept: s.js8_api_udp_accept,
            events,
            snapshot,
        })
    }

    /// The copy reads are answered from. Reads the JS8 station and Settings; changes nothing.
    pub fn js8_api_snapshot(&self) -> Js8ApiSnapshot {
        let s = &self.settings;
        let speed = self.js8_tx_speed();
        let status = s.js8_status.trim();
        let calls = self
            .js8_station
            .heard()
            .iter()
            .filter(|h| !h.call.is_empty())
            .map(|h| CallRow {
                call: h.call.clone(),
                grid: h.grid.clone().unwrap_or_default(),
                snr: h.snr_db,
                utc_ms: h.last_ms,
            })
            .collect();
        let band_activity = if self.js8_api.band_name == s.band {
            self.js8_api
                .band
                .iter()
                .map(|(o, b)| (*o, b.clone()))
                .collect()
        } else {
            Vec::new()
        };
        let text = if self.js8_api.pane.is_empty() {
            String::new()
        } else {
            let all = format!(
                "\n{}",
                self.js8_api
                    .pane
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n\n")
            );
            let n = all.chars().count();
            all.chars().skip(n.saturating_sub(PANE_CHARS)).collect()
        };
        let inbox = self
            .js8_station
            .inbox()
            .iter()
            .map(|e| {
                let (kind, cmd) = match e.state {
                    InboxState::Unread => ("UNREAD", " MSG"),
                    InboxState::Read => ("READ", " MSG"),
                    InboxState::Store => ("STORE", " MSG TO:"),
                    InboxState::Delivered => ("DELIVERED", " MSG TO:"),
                };
                let key = (e.from.to_ascii_uppercase(), e.at_ms, e.freq_hz.to_bits());
                let (dial, submode, tdrift) = self
                    .js8_api
                    .inbox_meta
                    .iter()
                    .rev()
                    .find(|(k, _)| *k == key)
                    .map_or((0, 0, 0.0), |(_, meta)| *meta);
                InboxRow {
                    kind,
                    cmd,
                    from: e.from.clone(),
                    to: e.to.clone(),
                    text: e.text.clone(),
                    path: if e.path.is_empty() {
                        e.from.clone()
                    } else {
                        e.path.join(">")
                    },
                    utc_ms: e.at_ms,
                    dial,
                    offset: e.freq_hz as i64,
                    snr: e.snr_db,
                    submode,
                    tdrift,
                }
            })
            .collect();
        let rx = if s.js8_rx_speeds == 0 {
            15
        } else {
            s.js8_rx_speeds
        };
        // A tune carrier is the key down, as JS8Call counts its own (`m_transmitting` while
        // `m_tune`, mainwindow.cpp:2560-2571, :2675), and the frame it names is "TUNE"
        // (:2623-2626).
        let tuning = self.tuning();
        let slot_tx = self.app.transmitting();
        let transmitting = slot_tx || tuning;
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        // The JS8 frame keyed last, while the key is down within its period: whatever else is on
        // the air is no JS8 frame.
        let on_air = self
            .js8_api
            .on_air
            .as_ref()
            .filter(|(_, _, until)| slot_tx && now_ms < *until);
        // JS8Call's compose box holds a message from its first frame until its last is sent
        // (`stopTx`, mainwindow.cpp:2893-2899), and its queue holds what waits behind it. The
        // outbox here lets a message go when its last frame is handed out: while that frame is
        // on the air the box still holds it, and the outbox's first message waits behind it.
        let st = &self.js8_station;
        let (going, boxed) = match on_air {
            Some((_, last, _)) => (st.last_tx_display().unwrap_or(""), !*last),
            None => (st.going_out().unwrap_or(""), st.outbox_count() > 0),
        };
        let waiting = (st.queued_len() + st.outbox_count()).saturating_sub(usize::from(boxed));
        let going_chars = going.chars().count();
        Js8ApiSnapshot {
            callsign: s.mycall.trim().to_ascii_uppercase(),
            grid: s.mygrid.trim().to_string(),
            info: s.js8_info.trim().to_string(),
            status: if status.is_empty() {
                DEFAULT_STATUS.to_string()
            } else {
                status.to_string()
            },
            spot: s.pskreporter,
            dial_hz: s.dial_hz(),
            offset_hz: self.tx_offset_hz() as i64,
            speed: submode(speed),
            selected: self.js8_station.selected().unwrap_or_default().to_string(),
            transmitting,
            band: s.band.clone(),
            calls,
            callsign_aging_min: s.js8_callsign_aging_min,
            band_activity,
            filter: s.js8_filter,
            text,
            inbox,
            config: ConfigRow {
                autoreply: s.js8_autoreply,
                autoreply_confirmation: s.js8_autoreply_confirmation,
                // Heartbeats go out at Slow, Normal and Fast only (`canCurrentModeSendHeartbeat`,
                // mainwindow.cpp:4113-4117).
                can_hb: matches!(submode(speed), 0 | 1 | 4),
                hb_ack: s.js8_hb_ack,
                hb_interval_min: s.js8_hb_interval_min,
                hb_on: self.js8_hb_on,
                monitor: self.tier() == Tier::Js8,
                multi_decoder: rx.count_ones() > 1,
                groups: s.js8_groups.clone(),
                tx_enabled: self.tx_enabled(),
                js8hb: s.js8_hb_networking,
                avoid_allcall: s.js8_avoid_allcall,
            },
            on_air: if tuning {
                "TUNE".to_string()
            } else {
                on_air.map(|(text, _, _)| text.clone()).unwrap_or_default()
            },
            tx_text: going
                .chars()
                .skip(going_chars.saturating_sub(TX_TEXT_CHARS))
                .collect(),
            waiting: waiting as i64,
            version: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::js8::proto::callsign::CallRef;
    use ::js8::proto::frame::encode_frame;
    use ::js8::{Command, Frame, I3};
    use std::collections::BTreeSet;

    /// Every type JS8Call 3.0.3 handles (`networkMessage.cpp` and `docs/API.md`).
    const JS8CALL_TYPES: &[&str] = &[
        "PING",
        "RIG.GET_PTT",
        "RIG.SET_TUNE",
        "RIG.TX_HALT",
        "RIG.GET_FREQ",
        "RIG.SET_FREQ",
        "STATION.GET_CALLSIGN",
        "STATION.GET_GRID",
        "STATION.SET_GRID",
        "STATION.GET_INFO",
        "STATION.SET_INFO",
        "STATION.GET_STATUS",
        "STATION.SET_STATUS",
        "STATION.VERSION",
        "STATION.GET_OS",
        "STATION.GET_SPOT",
        "STATION.SET_SPOT",
        "STATION.GET_CONFIG",
        "STATION.SET_AUTO_REPLY",
        "STATION.SET_JS8HB",
        "STATION.SET_HBACK",
        "STATION.SET_MULTI_DECODER",
        "STATION.SET_HB_INTERVAL",
        "STATION.SET_HB_TIMER",
        "STATION.SEND_HB",
        "STATION.SET_AUTOREPLY_CONFIRMATION",
        "STATION.AUTOREPLY_CONFIRM_RESPONSE",
        "STATION.SET_GROUPS",
        "STATION.SET_AVOID_ALLCALL",
        "RX.GET_CALL_ACTIVITY",
        "RX.GET_CALL_SELECTED",
        "RX.GET_BAND_ACTIVITY",
        "RX.GET_TEXT",
        "RX.GET_FREE_OFFSETS",
        "RX.GET_FILTER",
        "RX.SET_FILTER",
        "RX.SET_FILTER_ENABLED",
        "TX.GET_TEXT",
        "TX.SET_TEXT",
        "TX.SEND_MESSAGE",
        "TX.GET_QUEUE_DEPTH",
        "MODE.GET_SPEED",
        "MODE.SET_SPEED",
        "INBOX.GET_MESSAGES",
        "INBOX.STORE_MESSAGE",
        "WINDOW.RAISE",
    ];

    const READS: &[&str] = &[
        "RX.GET_CALL_ACTIVITY",
        "RX.GET_CALL_SELECTED",
        "RX.GET_BAND_ACTIVITY",
        "RX.GET_TEXT",
        "RX.GET_FREE_OFFSETS",
        "STATION.GET_CALLSIGN",
        "STATION.GET_GRID",
        "STATION.GET_INFO",
        "STATION.GET_STATUS",
        "STATION.GET_SPOT",
        "STATION.GET_OS",
        "STATION.GET_CONFIG",
        "INBOX.GET_MESSAGES",
        "RIG.GET_FREQ",
        "RIG.GET_PTT",
        "MODE.GET_SPEED",
        "TX.GET_TEXT",
        "TX.GET_QUEUE_DEPTH",
        "STATION.VERSION",
        "RX.GET_FILTER",
    ];

    const NOW: u64 = 1_769_740_328_005;

    fn json(line: &str) -> Value {
        serde_json::from_str(line).expect("the line is JSON")
    }

    fn keys(v: &Value) -> BTreeSet<String> {
        v.as_object().expect("an object").keys().cloned().collect()
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn req(text: &str) -> Request {
        Request::parse(text.as_bytes()).expect("a request")
    }

    fn ask(kind: &str, snap: &Js8ApiSnapshot) -> Value {
        let r = req(&format!(
            r#"{{"params":{{"_ID":7}},"type":"{kind}","value":""}}"#
        ));
        json(&answer(&r, Via::Tcp, snap, NOW).expect("answered").line)
    }

    fn row(frame: &Frame, i3: I3, speed: Js8Speed, freq: f32, snr: i32) -> modes::Decode {
        let word = encode_frame(frame, i3, speed).expect("packable");
        modes::Decode {
            message: frame.render(),
            sync: 5.0,
            snr,
            dt: 0.265,
            freq,
            nap: 0,
            qual: 0.9,
            rv: None,
            mode: Some(modes::ModeKind::Js8 { speed }),
            raw: Some(*word.as_bytes()),
            unchecked: false,
        }
    }

    fn whole() -> I3 {
        I3 {
            first: true,
            last: true,
            data: false,
        }
    }

    fn hb(call: &str, grid: &str) -> Frame {
        Frame::Heartbeat {
            call: call.to_string(),
            grid: Some(grid.to_string()),
            is_cq: false,
            idx: 0,
        }
    }

    fn snr_query(from: &str, to: &str) -> Frame {
        Frame::Directed {
            from: CallRef::Base(from.to_string()),
            to: CallRef::parse(to).expect("a call"),
            cmd: Command::SnrQuery,
            num: None,
            portable_from: false,
            portable_to: false,
        }
    }

    /// A JS8 engine on 20 m, KD9TAW at EN52, the API's TCP side on.
    fn engine() -> Engine {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.settings.js8_api_tcp = true;
        e.js8_apply_station_config();
        e.js8_enter();
        e.set_frequency(14.078, "20m", "USB");
        e.set_tx_offset(1500.0);
        e
    }

    fn drain(e: &mut Engine) -> Vec<Value> {
        e.js8_api_poll()
            .expect("the API is on")
            .events
            .iter()
            .map(|ev| json(&ev.to_outgoing().line))
            .collect()
    }

    fn of_type<'a>(lines: &'a [Value], kind: &str) -> Vec<&'a Value> {
        lines.iter().filter(|l| l["type"] == kind).collect()
    }

    // ---- what is answered, what is refused ----

    /// The allow-list is exactly the reads: of every type JS8Call 3.0.3 knows, the
    /// `RX.GET_*`/`STATION.GET_*`/`INBOX.GET_MESSAGES` reads and the six others that only read
    /// (`RIG.GET_FREQ`, `RIG.GET_PTT`, `MODE.GET_SPEED`, `TX.GET_TEXT`, `TX.GET_QUEUE_DEPTH`,
    /// `STATION.VERSION`) are answered, `RX.GET_FILTER` among them, and all the rest are refused.
    #[test]
    fn only_the_reads_are_answered() {
        let answered: BTreeSet<String> = JS8CALL_TYPES
            .iter()
            .filter(|k| classify(k).is_some())
            .map(|k| k.to_string())
            .collect();
        assert_eq!(answered, set(READS));
        let snap = Js8ApiSnapshot::default();
        for kind in JS8CALL_TYPES {
            let r = req(&format!(r#"{{"params":{{}},"type":"{kind}","value":""}}"#));
            let got = answer(&r, Via::Tcp, &snap, NOW);
            assert_eq!(got.is_some(), READS.contains(kind), "{kind}");
        }
        // Case matters, as in JS8Call (`type == "PING"`), and nothing near a read is one.
        for kind in ["rx.get_text", "RX.GET_TEXT ", "RX.GET_*", "", "STATION.GET"] {
            assert_eq!(classify(kind), None, "{kind:?}");
        }
    }

    /// Before and after a refused request: everything a request could have changed.
    fn state(e: &mut Engine) -> (String, String, bool, bool, u64, Vec<Value>) {
        (
            serde_json::to_string(e.settings()).unwrap(),
            serde_json::to_string(&e.js8_state()).unwrap(),
            e.tx_enabled(),
            e.app.transmitting(),
            e.settings().dial_hz(),
            e.js8_api_poll()
                .map(|p| {
                    p.events
                        .iter()
                        .map(|ev| json(&ev.to_outgoing().line))
                        .collect()
                })
                .unwrap_or_default(),
        )
    }

    /// One refused family: each request goes the whole way a socket's would — parsed, judged,
    /// answered from a snapshot — and NOTHING happens: no answer, Settings, the JS8 state, the
    /// latch, the dial and the queue as they were, and no frame keyed in the next minute.
    fn refused_family(requests: &[&str]) {
        let mut e = engine();
        e.set_tx_enabled(true);
        e.js8_ingest(
            &[row(
                &hb("W1AW", "FN31"),
                whole(),
                Js8Speed::Normal,
                1200.0,
                -7,
            )],
            0,
        );
        let _ = e.js8_api_poll();
        let before = state(&mut e);
        for text in requests {
            let r = Request::parse(text.as_bytes()).expect("well-formed");
            for via in [Via::Tcp, Via::Udp] {
                let snap = e.js8_api_snapshot();
                assert_eq!(answer(&r, via, &snap, NOW), None, "{text} via {via:?}");
            }
        }
        let after = state(&mut e);
        assert_eq!(after, before, "a refused request changed something");
        let period_ms = u64::from(e.js8_tx_speed().period_s()) * 1000;
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let mut last = t0 / period_ms;
        for k in 0..=60 {
            let t = t0 + k * 1000;
            e.js8_tick(t);
            let slot = t / period_ms;
            if slot != last {
                last = slot;
                assert!(e.plan_tx(slot).is_none(), "a refused request keyed a frame");
            }
        }
    }

    #[test]
    fn a_tx_request_does_nothing() {
        refused_family(&[
            r#"{"params":{},"type":"TX.SEND_MESSAGE","value":"W1AW SNR?"}"#,
            r#"{"params":{},"type":"TX.SET_TEXT","value":"CQ CQ"}"#,
        ]);
    }

    #[test]
    fn a_rig_request_does_nothing() {
        refused_family(&[
            r#"{"params":{"DIAL":14078000,"OFFSET":800},"type":"RIG.SET_FREQ","value":""}"#,
            r#"{"params":{},"type":"RIG.SET_TUNE","value":"true"}"#,
            r#"{"params":{},"type":"RIG.TX_HALT","value":""}"#,
        ]);
    }

    #[test]
    fn a_mode_request_does_nothing() {
        refused_family(&[r#"{"params":{"SPEED":2},"type":"MODE.SET_SPEED","value":""}"#]);
    }

    #[test]
    fn a_station_write_does_nothing() {
        refused_family(&[
            r#"{"params":{},"type":"STATION.SET_GRID","value":"FN31"}"#,
            r#"{"params":{},"type":"STATION.SET_INFO","value":"QRP"}"#,
            r#"{"params":{},"type":"STATION.SET_STATUS","value":"AWAY"}"#,
            r#"{"params":{},"type":"STATION.SET_SPOT","value":"true"}"#,
            r#"{"params":{},"type":"STATION.SET_AUTO_REPLY","value":"false"}"#,
            r#"{"params":{},"type":"STATION.SET_JS8HB","value":"true"}"#,
            r#"{"params":{},"type":"STATION.SET_HBACK","value":"true"}"#,
            r#"{"params":{},"type":"STATION.SET_MULTI_DECODER","value":"false"}"#,
            r#"{"params":{"INTERVAL":5},"type":"STATION.SET_HB_INTERVAL","value":""}"#,
            r#"{"params":{},"type":"STATION.SET_HB_TIMER","value":"true"}"#,
            r#"{"params":{},"type":"STATION.SEND_HB","value":""}"#,
            r#"{"params":{},"type":"STATION.SET_AUTOREPLY_CONFIRMATION","value":"false"}"#,
            r#"{"params":{"CONFIRM_ID":1},"type":"STATION.AUTOREPLY_CONFIRM_RESPONSE","value":"true"}"#,
            r#"{"params":{"GROUPS":["@FUN"]},"type":"STATION.SET_GROUPS","value":""}"#,
            r#"{"params":{},"type":"STATION.SET_AVOID_ALLCALL","value":"true"}"#,
        ]);
    }

    #[test]
    fn an_inbox_write_does_nothing() {
        refused_family(&[
            r#"{"params":{"CALLSIGN":"W1AW","TEXT":"HELLO"},"type":"INBOX.STORE_MESSAGE","value":""}"#,
        ]);
    }

    #[test]
    fn an_rx_setting_does_nothing() {
        refused_family(&[
            r#"{"params":{"CENTER":1500,"WIDTH":500},"type":"RX.SET_FILTER","value":""}"#,
            r#"{"params":{},"type":"RX.SET_FILTER_ENABLED","value":"true"}"#,
        ]);
    }

    #[test]
    fn a_window_or_unknown_request_does_nothing() {
        refused_family(&[
            r#"{"params":{},"type":"WINDOW.RAISE","value":""}"#,
            r#"{"params":{},"type":"PING","value":""}"#,
            r#"{"params":{},"type":"MAIN.TX","value":""}"#,
            r#"{"type":"TX.SEND_MESSAGE"}"#,
        ]);
    }

    // ---- the reads outside the GET families ----

    /// One read's whole answer line, asked under id 7 over TCP, from a fresh snapshot of `e`
    /// carrying the program's version as the listeners fill it in.
    fn line_for(e: &Engine, kind: &str) -> String {
        let mut snap = e.js8_api_snapshot();
        snap.version = "1.19.0".into();
        let r = req(&format!(
            r#"{{"params":{{"_ID":7}},"type":"{kind}","value":""}}"#
        ));
        answer(&r, Via::Tcp, &snap, NOW).expect("answered").line
    }

    /// The reads JS8Call 3.0.3 answers outside the `GET` families, byte for byte as its
    /// `networkMessage` builds them: `RIG.FREQ` (networkMessage.cpp:96-135), `RIG.PTT_STATUS`
    /// (:51-60), `MODE.SPEED` (:778-785), `TX.TEXT` (:725-732), `TX.QUEUE_DEPTH` (:759-767) and
    /// `STATION.VERSION` (:255-264).
    #[test]
    fn the_harmless_reads_are_answered_as_js8call_answers_them() {
        let mut e = engine();
        assert_eq!(
            line_for(&e, "RIG.GET_FREQ"),
            r#"{"params":{"DIAL":14078000,"FREQ":14079500,"OFFSET":1500,"_ID":7},"type":"RIG.FREQ","value":""}"#
        );
        assert_eq!(
            line_for(&e, "RIG.GET_PTT"),
            r#"{"params":{"MESSAGE":"","PTT":false,"_ID":7},"type":"RIG.PTT_STATUS","value":""}"#
        );
        assert_eq!(
            line_for(&e, "MODE.GET_SPEED"),
            r#"{"params":{"SPEED":0,"_ID":7},"type":"MODE.SPEED","value":""}"#
        );
        assert_eq!(
            line_for(&e, "TX.GET_TEXT"),
            r#"{"params":{"_ID":7},"type":"TX.TEXT","value":""}"#
        );
        assert_eq!(
            line_for(&e, "TX.GET_QUEUE_DEPTH"),
            r#"{"params":{"DEPTH":0,"_ID":7},"type":"TX.QUEUE_DEPTH","value":""}"#
        );
        assert_eq!(
            line_for(&e, "STATION.VERSION"),
            r#"{"params":{"VERSION":"1.19.0","_ID":7},"type":"STATION.VERSION","value":""}"#
        );
        // Each Nexus speed as JS8Call numbers it: Slow 4, Normal 0, Fast 1, JS8 40 2, JS8 60 8.
        for (idx, n) in [(0u8, 4), (1, 0), (2, 1), (3, 2), (4, 8)] {
            e.js8_set_speed(idx).expect("a speed Nexus has");
            assert_eq!(
                line_for(&e, "MODE.GET_SPEED"),
                format!(r#"{{"params":{{"SPEED":{n},"_ID":7}},"type":"MODE.SPEED","value":""}}"#)
            );
        }
        // A new dial and offset are read as they are.
        e.set_frequency(7.078, "40m", "USB");
        e.set_tx_offset(1750.0);
        assert_eq!(
            line_for(&e, "RIG.GET_FREQ"),
            r#"{"params":{"DIAL":7078000,"FREQ":7079750,"OFFSET":1750,"_ID":7},"type":"RIG.FREQ","value":""}"#
        );
    }

    /// While a JS8 frame is keyed, `RIG.PTT_STATUS` names it in JS8Call's twelve characters,
    /// `TX.TEXT` carries its message until the message's last frame is sent, and
    /// `TX.QUEUE_DEPTH` counts what waits behind that message, reading 1 when the key is down
    /// and nothing waits. Something other than JS8 on the air is no frame.
    #[test]
    fn the_transmitter_and_what_goes_out_read_as_js8call_reads_them() {
        let ask = |e: &Engine, kind: &str| json(&line_for(e, kind));
        let reads = |e: &Engine| {
            (
                ask(e, "RIG.GET_PTT")["params"].clone(),
                ask(e, "TX.GET_TEXT")["value"].clone(),
                ask(e, "TX.GET_QUEUE_DEPTH")["params"]["DEPTH"].clone(),
            )
        };
        let ptt =
            |message: &str, on: bool| serde_json::json!({"MESSAGE": message, "PTT": on, "_ID": 7});
        // An FT over (the slot flag alone): the key down, no JS8 frame, nothing going out, and
        // JS8Call's 1 for a key down with an empty queue.
        let mut e = engine();
        e.app.set_transmitting(true);
        assert_eq!(reads(&e), (ptt("", true), Value::from(""), Value::from(1)));

        let mut e = engine();
        e.set_tx_enabled(true);
        e.js8_send(Some("W1AW".into()), "HELLO THERE FROM NEXUS".into())
            .expect("queued");
        let first = e.js8_state().queue[0].display.clone();
        // Queued, the key up: the message waits in the box with nothing behind it.
        assert_eq!(
            reads(&e),
            (ptt("", false), Value::from(first.as_str()), Value::from(0))
        );
        e.js8_send(None, "SECOND".into()).expect("queued");
        e.js8_send(None, "THIRD".into()).expect("queued");
        let period_ms = u64::from(e.js8_tx_speed().period_s()) * 1000;
        let mut slot = tempo_core::timing::now_unix_ms() as u64 / period_ms;
        let mut frames = 0;
        loop {
            slot += 1;
            let plan = e
                .plan_tx(slot)
                .expect("the first message keys frame by frame");
            let super::super::TxWaveform::Js8 { word, .. } = &plan.waveform else {
                panic!("a JS8 plan carries the typed waveform");
            };
            let twelve = sixbit_to_string(&word.payload72().chars12());
            let last = word.i3().last;
            frames += 1;
            // Keyed: the frame named, its message in the box, the other two behind it.
            assert_eq!(
                reads(&e),
                (
                    ptt(&twelve, true),
                    Value::from(first.as_str()),
                    Value::from(2)
                ),
                "frame {frames}"
            );
            e.app.set_transmitting(false);
            if last {
                break;
            }
            // Between two frames: the key up, the message still in the box.
            assert_eq!(
                reads(&e),
                (ptt("", false), Value::from(first.as_str()), Value::from(2)),
                "after frame {frames}"
            );
        }
        assert!(frames > 1, "a multi-frame message");
        assert_eq!(twelve_chars(&e.js8_api.on_air), 12);
        // Sent: the second message is in the box and the third waits behind it.
        let second = e.js8_state().queue[0].display.clone();
        assert_ne!(second, first);
        assert_eq!(
            reads(&e),
            (ptt("", false), Value::from(second.as_str()), Value::from(1))
        );
    }

    /// `RX.CALL_SELECTED` and `STATION.STATUS`'s `SELECTED` are JS8Call's selected call
    /// (`callsignSelected`, networkMessage.cpp:552-559), which in Nexus is the call in the
    /// cockpit's To box as the station holds it (`Station::select_call`): announced when it
    /// changes, and "" once the box is empty.
    #[test]
    fn the_selected_call_is_the_to_box() {
        let mut e = engine();
        let selected = |e: &Engine| line_for(e, "RX.GET_CALL_SELECTED");
        assert_eq!(
            selected(&e),
            r#"{"params":{"_ID":7},"type":"RX.CALL_SELECTED","value":""}"#
        );
        let mut watch = StateWatch::default();
        watch.step(&e.js8_api_snapshot(), NOW);
        let announced = |watch: &mut StateWatch, e: &Engine| -> Vec<(String, String)> {
            let (before, after) = watch.step(&e.js8_api_snapshot(), NOW);
            before
                .iter()
                .chain(&after)
                .map(|o| json(&o.line))
                .map(|v| {
                    let kind = v["type"].as_str().unwrap_or_default().to_string();
                    let call = v["params"]["SELECTED"].as_str().unwrap_or("-").to_string();
                    (kind, call)
                })
                .collect()
        };
        e.js8_select_call(Some("w1aw".into()));
        assert_eq!(
            selected(&e),
            r#"{"params":{"_ID":7},"type":"RX.CALL_SELECTED","value":"W1AW"}"#
        );
        assert_eq!(
            announced(&mut watch, &e),
            [("STATION.STATUS".to_string(), "W1AW".to_string())]
        );
        e.js8_select_call(None);
        assert_eq!(
            selected(&e),
            r#"{"params":{"_ID":7},"type":"RX.CALL_SELECTED","value":""}"#
        );
        assert_eq!(
            announced(&mut watch, &e),
            [("STATION.STATUS".to_string(), String::new())]
        );
    }

    /// `STATION.CONFIG`'s `JS8HB` is JS8Call's heartbeat networking (its Mode menu's
    /// `actionModeJS8HB`, networkMessage.cpp:321): the Settings switch, off as JS8Call ships it.
    #[test]
    fn js8hb_is_the_heartbeat_networking_switch() {
        let mut e = engine();
        let js8hb =
            |e: &Engine| json(&line_for(e, "STATION.GET_CONFIG"))["params"]["JS8HB"].clone();
        assert_eq!(js8hb(&e), Value::from(false), "off, as JS8Call ships it");
        e.settings.js8_hb_networking = true;
        assert_eq!(js8hb(&e), Value::from(true));
    }

    /// `STATION.CONFIG`'s `AVOID_ALLCALL` is JS8Call's "Do not participate in the @ALLCALL group"
    /// (`m_config.avoid_allcall()`, networkMessage.cpp:332; `AvoidAllcall`, off by default,
    /// Configuration.cpp:2407): the Settings switch, *Do not take part in @ALLCALL*.
    #[test]
    fn avoid_allcall_is_the_settings_switch() {
        let mut e = engine();
        let avoid = |e: &Engine| {
            json(&line_for(e, "STATION.GET_CONFIG"))["params"]["AVOID_ALLCALL"].clone()
        };
        assert_eq!(avoid(&e), Value::from(false), "off, as JS8Call ships it");
        e.settings.js8_avoid_allcall = true;
        assert_eq!(avoid(&e), Value::from(true));
        e.settings.js8_avoid_allcall = false;
        assert_eq!(avoid(&e), Value::from(false), "and off again");
    }

    /// `RX.GET_FILTER` is answered `RX.FILTER` as JS8Call 3.0.3 answers it
    /// (networkMessage.cpp:665-675): the decode filter's centre, on/off and width as Settings
    /// hold them, in whole hertz. Setting it stays refused (`an_rx_setting_does_nothing`).
    #[test]
    fn the_decode_filter_is_read_as_js8call_reads_it() {
        let mut e = engine();
        assert_eq!(
            line_for(&e, "RX.GET_FILTER"),
            r#"{"params":{"CENTER":1500,"ENABLED":true,"WIDTH":2000,"_ID":7},"type":"RX.FILTER","value":""}"#
        );
        e.js8_set_filter(crate::settings::Js8Filter {
            enabled: false,
            center_hz: 1210,
            width_hz: 500,
        });
        assert_eq!(
            line_for(&e, "RX.GET_FILTER"),
            r#"{"params":{"CENTER":1210,"ENABLED":false,"WIDTH":500,"_ID":7},"type":"RX.FILTER","value":""}"#
        );
    }

    fn twelve_chars(on_air: &Option<(String, bool, u64)>) -> usize {
        on_air.as_ref().map_or(0, |(t, _, _)| {
            t.chars()
                .filter(|c| ::js8::proto::alphabet::SIXBIT.contains(&(*c as u8)))
                .count()
        })
    }

    /// Answering them changes nothing: each goes the whole way a socket's would, with the
    /// parameters a write would carry, and Settings, the JS8 state, the latch, the dial and the
    /// queue are as they were, and nothing keys in the next minute.
    #[test]
    fn the_harmless_reads_change_nothing() {
        let mut e = engine();
        e.set_tx_enabled(true);
        e.js8_ingest(
            &[row(
                &hb("W1AW", "FN31"),
                whole(),
                Js8Speed::Normal,
                1200.0,
                -7,
            )],
            0,
        );
        let _ = e.js8_api_poll();
        let before = state(&mut e);
        for kind in [
            "RIG.GET_FREQ",
            "RIG.GET_PTT",
            "MODE.GET_SPEED",
            "TX.GET_TEXT",
            "TX.GET_QUEUE_DEPTH",
            "STATION.VERSION",
        ] {
            let r = req(&format!(
                r#"{{"params":{{"_ID":7,"DIAL":7078000,"OFFSET":800,"SPEED":2,"TEXT":"HI"}},"type":"{kind}","value":"W1AW SNR?"}}"#
            ));
            for via in [Via::Tcp, Via::Udp] {
                let snap = e.js8_api_snapshot();
                assert!(answer(&r, via, &snap, NOW).is_some(), "{kind} via {via:?}");
            }
        }
        assert_eq!(state(&mut e), before, "a read changed something");
        let period_ms = u64::from(e.js8_tx_speed().period_s()) * 1000;
        let t0 = tempo_core::timing::now_unix_ms() as u64;
        let mut last = t0 / period_ms;
        for k in 0..=60 {
            let t = t0 + k * 1000;
            e.js8_tick(t);
            let slot = t / period_ms;
            if slot != last {
                last = slot;
                assert!(e.plan_tx(slot).is_none(), "a read keyed a frame");
            }
        }
    }

    /// Malformed or hostile input is dropped and never panics: a deterministic fuzz loop over
    /// raw bytes, truncated and spliced requests, and odd parameter types, every survivor
    /// answered from a full snapshot by both transports.
    #[test]
    fn malformed_requests_are_dropped_and_never_panic() {
        let mut e = engine();
        e.js8_ingest(
            &[row(
                &hb("W1AW", "FN31"),
                whole(),
                Js8Speed::Normal,
                1200.0,
                -7,
            )],
            0,
        );
        let snap = e.js8_api_snapshot();
        let seeds: Vec<String> = READS
            .iter()
            .map(|k| format!(r#"{{"params":{{"_ID":12,"SPEED":2,"LOW":500,"HIGH":2500,"CALLSIGN":"%"}},"type":"{k}","value":""}}"#))
            .collect();
        let odd = [
            "null",
            "true",
            "-1",
            "1e999",
            "\"x\"",
            "[]",
            "{}",
            "[1,2]",
            "{\"a\":[]}",
            "-9223372036854775808",
            "18446744073709551615",
            "1.5",
            "\"12\"",
            "\"-1\"",
        ];
        let mut x: u32 = 0x9e37_79b9;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        let mut answered = 0;
        for i in 0..20_000 {
            let bytes: Vec<u8> = match i % 4 {
                0 => (0..(next() % 64)).map(|_| next() as u8).collect(),
                1 => {
                    let s = seeds[next() as usize % seeds.len()].as_bytes();
                    s[..(next() as usize % (s.len() + 1))].to_vec()
                }
                2 => {
                    let mut s = seeds[next() as usize % seeds.len()].as_bytes().to_vec();
                    let at = next() as usize % s.len();
                    s[at] = next() as u8;
                    s
                }
                _ => {
                    let k = READS[next() as usize % READS.len()];
                    let v = odd[next() as usize % odd.len()];
                    let w = odd[next() as usize % odd.len()];
                    format!(
                        r#"{{"params":{{"_ID":{v},"SPEED":{w},"LOW":{v},"HIGH":{w},"CALLSIGN":{v}}},"type":"{k}","value":{w}}}"#
                    )
                    .into_bytes()
                }
            };
            if let Some(r) = Request::parse(&bytes) {
                for via in [Via::Tcp, Via::Udp] {
                    if let Some(out) = answer(&r, via, &snap, NOW) {
                        answered += 1;
                        let v = json(&out.line);
                        assert!(
                            v["type"].is_string() && v["params"].is_object(),
                            "{}",
                            out.line
                        );
                    }
                }
            }
        }
        assert!(
            answered > 1000,
            "the fuzz loop must also reach the answers: {answered}"
        );
        // Past the cap is not read at all, however well formed.
        let mut big = br#"{"params":{"PAD":""#.to_vec();
        big.extend(std::iter::repeat_n(b'A', MAX_REQUEST_BYTES));
        big.extend(br#""},"type":"RX.GET_TEXT","value":""}"#);
        assert_eq!(Request::parse(&big), None);
        assert!(Request::parse(&big[big.len() - 40..]).is_none());
    }

    // ---- a line that is no request; the Accept switches ----

    /// A TCP line that is no request is told why in JS8Call's `API.ERROR` (MessageServer.cpp:
    /// 207-213; the words are `std::system_error`'s, "<what>: <category message>", from
    /// Message.cpp:119-135 and MessageError.cpp:20-23), under a fresh string id: JSON that does
    /// not parse is "<the parser's words>: json parsing error" (serde's words here where
    /// JS8Call's are Qt's: docs/API.md:76 shows "unterminated object"), a value Qt's parser
    /// takes no document from (anything but an object or an array) is "illegal value: json
    /// parsing error", and an array is "json not an object". A blank line is passed over, and a
    /// line is read once trimmed as Qt trims one (`\v` and `\f` included).
    #[test]
    fn a_line_that_is_no_request_is_told_why_as_js8call_tells_it() {
        let fresh = (NOW as i64 - EPOCH_MS).to_string();
        let error = |line: &[u8]| match read_tcp_line(line, NOW) {
            TcpLine::Error(out) => out.line,
            other => panic!("{:?} drew {other:?}", String::from_utf8_lossy(line)),
        };
        assert_eq!(
            error(br#"{"params":{"_ID":9"#),
            format!(
                r#"{{"params":{{"_ID":"{fresh}"}},"type":"API.ERROR","value":"EOF while parsing an object at line 1 column 18: json parsing error"}}"#
            )
        );
        assert_eq!(
            error(b"[1,2]"),
            format!(
                r#"{{"params":{{"_ID":"{fresh}"}},"type":"API.ERROR","value":"json not an object"}}"#
            )
        );
        for scalar in ["42", "\"RX.GET_TEXT\"", "true", "null"] {
            assert_eq!(
                json(&error(scalar.as_bytes()))["value"],
                "illegal value: json parsing error",
                "{scalar}"
            );
        }
        assert_eq!(
            json(&error(br#"{"type":"RX.GET_TEXT"} x"#))["value"],
            "trailing characters at line 1 column 24: json parsing error"
        );
        assert_eq!(
            json(&error(&[0xff, b'{']))["value"],
            "expected value at line 1 column 1: json parsing error"
        );
        for blank in [&b""[..], b"   ", b"\r", b"\t\x0b\x0c "] {
            assert_eq!(read_tcp_line(blank, NOW), TcpLine::Blank, "{blank:?}");
        }
        assert_eq!(
            read_tcp_line(b"\x0b {\"type\":\"RX.GET_TEXT\"}\x0c\r", NOW),
            TcpLine::Request(req(r#"{"type":"RX.GET_TEXT"}"#))
        );
        // "Connections Full" is the same message.
        assert_eq!(connections_full(NOW), api_error("Connections Full", NOW));
    }

    /// "Accept TCP requests" and "Accept UDP requests" ship OFF, as JS8Call ships them
    /// (Configuration.cpp:2448-2451); a settings file from before them loads so; each
    /// round-trips under its own wire name; and the listeners' tick carries each as it is.
    #[test]
    fn the_accept_switches_ship_off_and_reach_the_listeners() {
        use crate::settings::Settings;
        let s = Settings::default();
        assert_eq!((s.js8_api_tcp_accept, s.js8_api_udp_accept), (false, false));
        let mut v = serde_json::to_value(&s).unwrap();
        let o = v.as_object_mut().unwrap();
        for k in ["js8ApiTcpAccept", "js8ApiUdpAccept"] {
            assert_eq!(
                o.remove(k),
                Some(Value::Bool(false)),
                "{k} is the wire name"
            );
        }
        let old: Settings = serde_json::from_value(v).unwrap();
        assert_eq!(
            (old.js8_api_tcp_accept, old.js8_api_udp_accept),
            (false, false)
        );
        for (tcp, udp) in [(true, false), (false, true), (true, true), (false, false)] {
            let on = Settings {
                js8_api_tcp: true,
                js8_api_udp: true,
                js8_api_tcp_accept: tcp,
                js8_api_udp_accept: udp,
                ..Settings::default()
            };
            let back: Settings =
                serde_json::from_str(&serde_json::to_string(&on).unwrap()).unwrap();
            assert_eq!(
                (back.js8_api_tcp_accept, back.js8_api_udp_accept),
                (tcp, udp)
            );
            let poll = Engine::with_settings(on)
                .js8_api_poll()
                .expect("the API is on");
            assert_eq!((poll.tcp_accept, poll.udp_accept), (tcp, udp));
        }
    }

    /// JS8Call reads a request leniently (`Message::fromJson`): a `type`, `value` or `params` of
    /// the wrong kind counts as empty, never as an error.
    #[test]
    fn a_request_is_read_as_js8call_reads_one() {
        assert_eq!(Request::parse(b"[]"), None);
        assert_eq!(Request::parse(b"\"RX.GET_TEXT\""), None);
        assert_eq!(Request::parse(b"{\"type\":\"RX.GET_TEXT\"} x"), None);
        let r = req(r#"{"type":7,"value":[],"params":"x"}"#);
        assert_eq!((r.kind(), r.value.as_str(), r.params.len()), ("", "", 0));
        let r = req(r#"  {"type":"RX.GET_TEXT"}  "#);
        assert_eq!(r.kind(), "RX.GET_TEXT");
    }

    /// The answer's `_ID`: the request's own, as a number; a fresh number over TCP and a fresh
    /// STRING over UDP when it carried none (or none JS8Call can read as one).
    #[test]
    fn an_answer_carries_the_id_js8call_gives_it() {
        let snap = Js8ApiSnapshot::default();
        let id_of = |text: &str, via| {
            let out = answer(&req(text), via, &snap, NOW).unwrap();
            (json(&out.line)["params"]["_ID"].clone(), out.id)
        };
        let fresh = NOW as i64 - EPOCH_MS;
        let none = r#"{"params":{},"type":"RX.GET_TEXT","value":""}"#;
        assert_eq!(id_of(none, Via::Tcp), (Value::from(fresh), fresh));
        assert_eq!(
            id_of(none, Via::Udp),
            (Value::from(fresh.to_string()), fresh)
        );
        for (text, id) in [
            (r#"{"params":{"_ID":123},"type":"RX.GET_TEXT"}"#, 123),
            (r#"{"params":{"_ID":"456"},"type":"RX.GET_TEXT"}"#, 456),
            (r#"{"params":{"_ID":-1},"type":"RX.GET_TEXT"}"#, -1),
            (r#"{"params":{"_ID":1.6},"type":"RX.GET_TEXT"}"#, 2),
        ] {
            for via in [Via::Tcp, Via::Udp] {
                assert_eq!(id_of(text, via), (Value::from(id), id), "{text}");
            }
        }
        let junk = r#"{"params":{"_ID":"abc"},"type":"RX.GET_TEXT"}"#;
        assert_eq!(id_of(junk, Via::Udp).0, Value::from(fresh.to_string()));
    }

    // ---- the shapes, against docs/API.md ----

    /// Every answer's params against `docs/API.md`'s example of it (and `networkMessage.cpp`
    /// for the two the document does not show): the same keys, and the type each carries.
    #[test]
    fn every_answer_has_the_documented_shape() {
        let mut e = engine();
        e.settings.js8_info = "QRP 5W".into();
        e.settings.js8_groups = vec!["@FUN".into()];
        e.js8_ingest(
            &[row(
                &hb("W1AW", "FN31"),
                whole(),
                Js8Speed::Normal,
                1200.0,
                -7,
            )],
            0,
        );
        let snap = e.js8_api_snapshot();
        let id = set(&["_ID"]);
        for (kind, reply, value) in [
            ("STATION.GET_CALLSIGN", "STATION.CALLSIGN", "KD9TAW"),
            ("STATION.GET_GRID", "STATION.GRID", "EN52"),
            ("STATION.GET_INFO", "STATION.INFO", "QRP 5W"),
            ("STATION.GET_STATUS", "STATION.STATUS", DEFAULT_STATUS),
            ("RX.GET_CALL_SELECTED", "RX.CALL_SELECTED", ""),
        ] {
            let v = ask(kind, &snap);
            assert_eq!(
                (v["type"].as_str(), v["value"].as_str()),
                (Some(reply), Some(value))
            );
            assert_eq!(keys(&v["params"]), id, "{kind}");
            assert_eq!(v["params"]["_ID"], 7);
        }
        let v = ask("STATION.GET_SPOT", &snap);
        assert_eq!(v["type"], "STATION.SPOT");
        assert_eq!(keys(&v["params"]), set(&["_ID", "value"]));
        assert!(v["params"]["value"].is_boolean());
        let v = ask("STATION.GET_OS", &snap);
        assert_eq!(v["type"], "STATION.GET_OS");
        assert_eq!(
            keys(&v["params"]),
            set(&["OS_KERNEL", "OS_KERNEL_VERSION", "OS_NAME", "_ID"])
        );
        let v = ask("RX.GET_CALL_ACTIVITY", &snap);
        assert_eq!(v["type"], "RX.CALL_ACTIVITY");
        assert_eq!(keys(&v["params"]), set(&["W1AW", "_ID"]));
        assert_eq!(keys(&v["params"]["W1AW"]), set(&["GRID", "SNR", "UTC"]));
        assert_eq!(v["params"]["W1AW"]["GRID"], "FN31");
        assert_eq!(v["params"]["W1AW"]["SNR"], -7);
        assert!(v["params"]["W1AW"]["UTC"].is_u64());
        let v = ask("RX.GET_BAND_ACTIVITY", &snap);
        assert_eq!(v["type"], "RX.BAND_ACTIVITY");
        assert_eq!(keys(&v["params"]), set(&["1200", "_ID"]));
        assert_eq!(
            keys(&v["params"]["1200"]),
            set(&["DIAL", "FREQ", "OFFSET", "SNR", "TEXT", "UTC"])
        );
        assert_eq!(v["params"]["1200"]["DIAL"], 14_078_000);
        assert_eq!(v["params"]["1200"]["FREQ"], 14_079_200);
        assert_eq!(v["params"]["1200"]["TEXT"], hb("W1AW", "FN31").render());
        let v = ask("RX.GET_TEXT", &snap);
        assert_eq!(
            (v["type"].as_str(), keys(&v["params"])),
            (Some("RX.TEXT"), id.clone())
        );
        let v = ask("RX.GET_FREE_OFFSETS", &snap);
        assert_eq!(v["type"], "RX.FREE_OFFSETS");
        assert_eq!(
            keys(&v["params"]),
            set(&["BANDWIDTH", "FREE", "HIGH", "LOW", "SPEED", "_ID"])
        );
        for seg in v["params"]["FREE"].as_array().unwrap() {
            assert_eq!(keys(seg), set(&["HIGH", "LOW", "WIDTH"]));
        }
        let v = ask("STATION.GET_CONFIG", &snap);
        assert_eq!(v["type"], "STATION.CONFIG");
        assert_eq!(
            keys(&v["params"]),
            set(&[
                "AUTOREPLY_CONFIRMATION",
                "AUTO_REPLY",
                "AVOID_ALLCALL",
                "CAN_HB",
                "HBACK",
                "HB_INTERVAL",
                "HB_TIMER_ACTIVE",
                "JS8HB",
                "MONITOR",
                "MULTI_DECODER",
                "MY_GROUPS",
                "SPEED",
                "TX_ENABLED",
                "_ID",
            ])
        );
        assert_eq!(v["params"]["MY_GROUPS"], serde_json::json!(["@FUN"]));
        assert_eq!(v["params"]["SPEED"], 0, "Normal is JS8Call's 0");
        assert_eq!(v["params"]["MONITOR"], true);
        let v = ask("INBOX.GET_MESSAGES", &snap);
        assert_eq!(v["type"], "INBOX.MESSAGES");
        assert_eq!(keys(&v["params"]), set(&["MESSAGES", "_ID"]));
    }

    /// Every message the station sends unasked, against `docs/API.md`, by value where Nexus
    /// knows the value. `RX.ACTIVITY` carries `BITS` beyond the document's example: JS8Call
    /// 3.0.3 sends it (processRxActivity.cpp:33).
    #[test]
    fn every_event_has_the_documented_shape() {
        let mut e = engine();
        e.settings.pskreporter = true;
        e.js8_ingest(
            &[row(
                &hb("W1AW", "FN31"),
                whole(),
                Js8Speed::Normal,
                1200.0,
                -7,
            )],
            0,
        );
        let lines = drain(&mut e);
        let activity = of_type(&lines, "RX.ACTIVITY");
        assert_eq!(activity.len(), 1);
        let a = activity[0];
        assert_eq!(
            keys(&a["params"]),
            set(&["BITS", "DIAL", "FREQ", "OFFSET", "SNR", "SPEED", "TDRIFT", "UTC", "_ID"])
        );
        assert_eq!(a["value"], hb("W1AW", "FN31").render());
        assert_eq!(
            (
                &a["params"]["DIAL"],
                &a["params"]["FREQ"],
                &a["params"]["OFFSET"]
            ),
            (
                &Value::from(14_078_000),
                &Value::from(14_079_200),
                &Value::from(1200)
            )
        );
        assert_eq!(a["params"]["SNR"], -7);
        assert_eq!(a["params"]["SPEED"], 0);
        assert_eq!(a["params"]["BITS"], 3, "first | last");
        assert_eq!(a["params"]["_ID"], -1);
        // The document's own TDRIFT, for the same float.
        assert!(a.to_string().contains(r#""TDRIFT":0.26499998569488525"#));
        let directed = of_type(&lines, "RX.DIRECTED");
        assert_eq!(directed.len(), 1);
        let d = directed[0];
        assert_eq!(
            keys(&d["params"]),
            set(&[
                "CMD", "DIAL", "EXTRA", "FREQ", "FROM", "GRID", "OFFSET", "SNR", "SPEED", "TDRIFT",
                "TEXT", "TO", "UTC", "_ID"
            ])
        );
        assert_eq!(d["value"], "@HB HEARTBEAT \u{2662} ");
        assert_eq!(d["params"]["TEXT"], "@HB HEARTBEAT \u{2662} ");
        assert_eq!(
            (
                &d["params"]["FROM"],
                &d["params"]["TO"],
                &d["params"]["CMD"]
            ),
            (
                &Value::from("W1AW"),
                &Value::from("@HB"),
                &Value::from(" HEARTBEAT")
            )
        );
        assert_eq!(
            (&d["params"]["GRID"], &d["params"]["EXTRA"]),
            (&Value::from("FN31"), &Value::from(""))
        );
        let spot = of_type(&lines, "RX.SPOT");
        assert_eq!(spot.len(), 1);
        assert_eq!(
            spot[0].to_string(),
            r#"{"params":{"CALL":"W1AW","DIAL":14078000,"FREQ":14079200,"GRID":"FN31","OFFSET":1200,"SNR":-7,"_ID":-1},"type":"RX.SPOT","value":""}"#
        );
        // The ones a tick's look at the state announces, and the program's last word.
        let mut snap = e.js8_api_snapshot();
        let mut watch = StateWatch::default();
        let (first, _) = watch.step(&snap, NOW);
        let freq = json(&first[0].line);
        assert_eq!(freq["type"], "RIG.FREQ");
        assert_eq!(
            keys(&freq["params"]),
            set(&["BAND", "DIAL", "FREQ", "OFFSET", "_ID"])
        );
        assert_eq!(
            (&freq["params"]["BAND"], &freq["params"]["FREQ"]),
            (&Value::from("20m"), &Value::from(14_079_500))
        );
        let status = json(&first[1].line);
        assert_eq!(status["type"], "STATION.STATUS");
        assert_eq!(
            keys(&status["params"]),
            set(&["DIAL", "FREQ", "OFFSET", "SELECTED", "SPEED", "_ID"])
        );
        assert!(
            status["params"]["_ID"].is_string(),
            "a fresh id, never sent over TCP"
        );
        assert!(first[1].id > 0);
        snap.transmitting = true;
        let (on, _) = watch.step(&snap, NOW);
        assert_eq!(
            on[0].line,
            format!(
                r#"{{"params":{{"PTT":true,"UTC":{NOW},"_ID":-1}},"type":"RIG.PTT","value":"on"}}"#
            )
        );
        assert_eq!(
            closing().line,
            r#"{"params":{"REASON":"User closed application","_ID":-1},"type":"STATION.CLOSING","value":""}"#
        );
        let p = json(&ping("1.19.0", NOW).line);
        assert_eq!(keys(&p["params"]), set(&["NAME", "UTC", "VERSION", "_ID"]));
        assert_eq!(
            (&p["type"], &p["params"]["NAME"]),
            (&Value::from("PING"), &Value::from("Nexus"))
        );
        let c = json(&close(NOW).line);
        assert_eq!(
            (&c["type"], keys(&c["params"])),
            (&Value::from("CLOSE"), id_only())
        );
        let full = json(&connections_full(NOW).line);
        assert_eq!(
            (&full["type"], &full["value"]),
            (&Value::from("API.ERROR"), &Value::from("Connections Full"))
        );
    }

    fn id_only() -> BTreeSet<String> {
        set(&["_ID"])
    }

    /// Qt's compact JSON, byte for byte: sorted keys, no spaces, a whole double with no fraction.
    #[test]
    fn the_wire_is_qts_compact_json() {
        let ev = Js8ApiEvent::Activity {
            text: "W1AW: ".into(),
            dial: 7_078_000,
            offset: 2420,
            snr: -22,
            speed: 1,
            tdrift: 1.0,
            utc_ms: 1_769_740_328_005,
            bits: 1,
        };
        assert_eq!(
            ev.to_outgoing().line,
            r#"{"params":{"BITS":1,"DIAL":7078000,"FREQ":7080420,"OFFSET":2420,"SNR":-22,"SPEED":1,"TDRIFT":1,"UTC":1769740328005,"_ID":-1},"type":"RX.ACTIVITY","value":"W1AW: "}"#
        );
        assert_eq!(qt_double(0.0), Value::from(0));
        assert_eq!(qt_double(-2.0), Value::from(-2));
        assert_eq!(
            qt_double(f64::from(0.265_f32)).to_string(),
            "0.26499998569488525"
        );
        let tones = Js8ApiEvent::TxFrame {
            tones: vec![4, 2, 5],
        }
        .to_outgoing();
        assert_eq!(
            tones.line,
            r#"{"params":{"TONES":[4,2,5],"_ID":-1},"type":"TX.FRAME","value":""}"#
        );
    }

    // ---- what each read says ----

    /// A directed query to me: an `RX.DIRECTED` with the command and no text, and a line in
    /// `RX.TEXT` as JS8Call's pane shows it; a heartbeat does not reach the pane.
    #[test]
    fn a_query_to_me_is_directed_and_shown_in_the_pane() {
        let mut e = engine();
        e.js8_ingest(
            &[row(
                &hb("W1AW", "FN31"),
                whole(),
                Js8Speed::Normal,
                1200.0,
                -7,
            )],
            0,
        );
        e.js8_ingest(
            &[row(
                &snr_query("W1AW", "KD9TAW"),
                whole(),
                Js8Speed::Normal,
                1210.0,
                -9,
            )],
            0,
        );
        let lines = drain(&mut e);
        let d = of_type(&lines, "RX.DIRECTED");
        assert_eq!(d.len(), 2);
        assert_eq!(d[1]["value"], "KD9TAW SNR? \u{2662} ");
        assert_eq!(
            (&d[1]["params"]["CMD"], &d[1]["params"]["TO"]),
            (&Value::from(" SNR?"), &Value::from("KD9TAW"))
        );
        let text = ask("RX.GET_TEXT", &e.js8_api_snapshot());
        let pane = text["value"].as_str().unwrap();
        assert!(pane.starts_with('\n'), "{pane:?}");
        assert!(
            pane.ends_with(" - (1210) - W1AW: KD9TAW SNR? \u{2662} "),
            "{pane:?}"
        );
        assert!(!pane.contains("HEARTBEAT"), "{pane:?}");
        assert_eq!(pane.matches(" - (").count(), 1);
        // The band activity followed the signal's drift: 1200 then 1210 is one entry, at 1210.
        let band = ask("RX.GET_BAND_ACTIVITY", &e.js8_api_snapshot());
        assert_eq!(keys(&band["params"]), set(&["1210", "_ID"]));
        // A band change clears it, as JS8Call's `clearActivity`.
        e.set_frequency(7.078, "40m", "USB");
        let band = ask("RX.GET_BAND_ACTIVITY", &e.js8_api_snapshot());
        assert_eq!(keys(&band["params"]), id_only());
    }

    /// A multi-frame MSG to me is ONE `RX.DIRECTED`, the body whole; heard with the API off,
    /// nothing is queued, but the band activity and pane still fill.
    #[test]
    fn a_message_is_one_directed_event_and_off_queues_nothing() {
        let msg = |e: &mut Engine| {
            for (f, i3) in ::js8::proto::compose::frames(
                "W1AW",
                Some(&CallRef::Base("KD9TAW".to_string())),
                "MSG HELLO FROM OHIO",
                Js8Speed::Normal,
            )
            .expect("composes")
            {
                e.js8_ingest(&[row(&f, i3, Js8Speed::Normal, 1750.0, -12)], 0);
            }
        };
        let mut e = engine();
        msg(&mut e);
        let lines = drain(&mut e);
        let d = of_type(&lines, "RX.DIRECTED");
        assert_eq!(d.len(), 1, "{lines:?}");
        assert_eq!(d[0]["params"]["CMD"], " MSG");
        assert_eq!(d[0]["value"], "KD9TAW MSG HELLO FROM OHIO \u{2662} ");
        assert!(of_type(&lines, "RX.ACTIVITY").len() > 1);
        let mut off = engine();
        off.settings.js8_api_tcp = false;
        msg(&mut off);
        assert!(off.js8_api_poll().is_none());
        assert!(
            off.js8_api.events.is_empty(),
            "nothing queued while the API is off"
        );
        off.settings.js8_api_udp = true;
        let snap = off.js8_api_snapshot();
        assert!(!snap.band_activity.is_empty());
        assert!(
            snap.text.contains("W1AW: KD9TAW MSG HELLO FROM OHIO"),
            "{:?}",
            snap.text
        );
        assert!(off.js8_api_poll().unwrap().events.is_empty());
    }

    /// Spotting off, no `RX.SPOT`; on, one per station a command comes from.
    #[test]
    fn spots_follow_the_spotting_switch() {
        for on in [false, true] {
            let mut e = engine();
            e.settings.pskreporter = on;
            e.js8_ingest(
                &[row(
                    &hb("W1AW", "FN31"),
                    whole(),
                    Js8Speed::Normal,
                    1200.0,
                    -7,
                )],
                0,
            );
            let lines = drain(&mut e);
            assert_eq!(of_type(&lines, "RX.SPOT").len(), usize::from(on));
            let spot = ask("STATION.GET_SPOT", &e.js8_api_snapshot());
            assert_eq!(spot["params"]["value"], on);
        }
    }

    /// Callsign aging leaves out a call heard that many minutes ago or more (`secsTo / 60`).
    #[test]
    fn call_activity_ages_as_js8call_ages_it() {
        let snap = Js8ApiSnapshot {
            callsign_aging_min: 2,
            calls: vec![
                CallRow {
                    call: "OLD".into(),
                    grid: String::new(),
                    snr: 0,
                    utc_ms: NOW - 120_000,
                },
                CallRow {
                    call: "NEW".into(),
                    grid: String::new(),
                    snr: 0,
                    utc_ms: NOW - 119_999,
                },
            ],
            ..Js8ApiSnapshot::default()
        };
        assert_eq!(
            keys(&ask("RX.GET_CALL_ACTIVITY", &snap)["params"]),
            set(&["NEW", "_ID"])
        );
        let all = Js8ApiSnapshot {
            callsign_aging_min: 0,
            ..snap
        };
        assert_eq!(
            keys(&ask("RX.GET_CALL_ACTIVITY", &all)["params"]),
            set(&["NEW", "OLD", "_ID"])
        );
    }

    fn band(entries: &[(i32, Js8Speed, u64)]) -> Vec<(i32, BandRow)> {
        entries
            .iter()
            .map(|&(o, speed, age)| {
                (
                    o,
                    BandRow {
                        dial: 7_078_000,
                        text: String::new(),
                        snr: 0,
                        utc_ms: NOW - age,
                        speed,
                    },
                )
            })
            .collect()
    }

    /// JS8Call's free-offset arithmetic, by value.
    #[test]
    fn free_offsets_are_js8calls() {
        use Js8Speed::{Normal, Turbo};
        assert_eq!(free_offsets(&[], Normal, 500, 2500, NOW), vec![(500, 2500)]);
        // One Normal signal at 1000, asked at Normal: 25 + 25 either side.
        let one = band(&[(1000, Normal, 0)]);
        assert_eq!(
            free_offsets(&one, Normal, 500, 2500, NOW),
            vec![(500, 950), (1050, 2500)]
        );
        // Asked at Turbo: 80 + 25 either side, and a gap narrower than 160 is no gap.
        let two = band(&[(1000, Normal, 0), (1300, Turbo, 0)]);
        assert_eq!(
            free_offsets(&two, Turbo, 500, 2500, NOW),
            vec![(500, 895), (1460, 2500)]
        );
        // 29.9 s still blocks; 30 s is clear.
        assert_eq!(
            free_offsets(&band(&[(1000, Normal, 29_999)]), Normal, 500, 2500, NOW).len(),
            2
        );
        assert_eq!(
            free_offsets(&band(&[(1000, Normal, 30_000)]), Normal, 500, 2500, NOW),
            vec![(500, 2500)]
        );
        // Overlaps merge; an upside-down range is empty; no number overflows.
        let close = band(&[(1000, Normal, 0), (1040, Normal, 0)]);
        assert_eq!(
            free_offsets(&close, Normal, 500, 2500, NOW),
            vec![(500, 950), (1090, 2500)]
        );
        assert!(free_offsets(&one, Normal, 2500, 500, NOW).is_empty());
        assert_eq!(free_offsets(&one, Normal, i64::MIN, i64::MAX, NOW).len(), 2);
        // Through the request: the documented defaults, the speed asked for, and an unknown
        // speed is left unanswered (JS8Call's lookup throws there).
        let snap = Js8ApiSnapshot {
            band_activity: one,
            ..Js8ApiSnapshot::default()
        };
        let v = ask("RX.GET_FREE_OFFSETS", &snap);
        assert_eq!(
            v["params"].to_string(),
            r#"{"BANDWIDTH":50,"FREE":[{"HIGH":950,"LOW":500,"WIDTH":450},{"HIGH":2500,"LOW":1050,"WIDTH":1450}],"HIGH":2500,"LOW":500,"SPEED":0,"_ID":7}"#
        );
        let r = req(r#"{"params":{"SPEED":3},"type":"RX.GET_FREE_OFFSETS","value":""}"#);
        assert_eq!(answer(&r, Via::Tcp, &snap, NOW), None);
        let r = req(
            r#"{"params":{"SPEED":"2","LOW":"700","HIGH":900.4},"type":"RX.GET_FREE_OFFSETS"}"#,
        );
        let v = json(&answer(&r, Via::Tcp, &snap, NOW).unwrap().line);
        assert_eq!(
            (
                &v["params"]["SPEED"],
                &v["params"]["LOW"],
                &v["params"]["HIGH"]
            ),
            (&Value::from(2), &Value::from(700), &Value::from(900))
        );
        assert_eq!(v["params"]["BANDWIDTH"], 160);
        assert_eq!(
            v["params"]["FREE"],
            serde_json::json!([{"HIGH": 895, "LOW": 700, "WIDTH": 195}])
        );
    }

    #[test]
    fn speeds_number_as_js8call_does() {
        let numbers: Vec<(i64, i64)> = Js8Speed::ALL
            .iter()
            .map(|s| (submode(*s), bandwidth(*s)))
            .collect();
        assert_eq!(numbers, vec![(4, 25), (0, 50), (1, 80), (2, 160), (8, 250)]);
        let distinct: BTreeSet<i64> = numbers.iter().map(|n| n.0).collect();
        assert_eq!(distinct.len(), Js8Speed::ALL.len());
        for s in Js8Speed::ALL {
            assert_eq!(speed_of(submode(s)), Some(s));
        }
        assert_eq!(speed_of(3), None);
    }

    #[test]
    fn like_is_sqlites() {
        for (p, t, m) in [
            ("%", "W1AW", true),
            ("%", "", true),
            ("w1aw", "W1AW", true),
            ("W1%", "W1AW", true),
            ("_1AW", "W1AW", true),
            ("%AW", "W1AW", true),
            ("W1", "W1AW", false),
            ("", "W1AW", false),
            ("", "", true),
            ("%1%W", "W1AW", true),
            ("%X%", "W1AW", false),
        ] {
            assert_eq!(like(p, t), m, "{p:?} ~ {t:?}");
        }
    }

    /// The inbox list as JS8Call builds it: held messages by TO, read and unread by FROM,
    /// newest first, a delivered one left out, TEXT only when there is some.
    #[test]
    fn the_inbox_is_listed_as_js8call_lists_it() {
        let r = |kind: &'static str, from: &str, to: &str, text: &str, utc_ms: u64| InboxRow {
            kind,
            cmd: if kind == "STORE" { " MSG TO:" } else { " MSG" },
            from: from.into(),
            to: to.into(),
            text: text.into(),
            path: from.into(),
            utc_ms,
            dial: 0,
            offset: 1500,
            snr: -10,
            submode: 0,
            tdrift: 0.0,
        };
        let rows = vec![
            r("UNREAD", "W1AW", "KD9TAW", "ONE", 1_000_000),
            r("STORE", "K1ABC", "W1AW", "HELD", 3_000_000),
            r("READ", "N0CALL", "KD9TAW", "", 2_000_000),
            r("DELIVERED", "K1ABC", "W1AW", "GONE", 4_000_000),
        ];
        let listed = inbox_records(&rows, "%");
        let order: Vec<(&str, &str)> = listed
            .iter()
            .map(|m| {
                (
                    m["type"].as_str().unwrap(),
                    m["params"]["FROM"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            order,
            vec![("STORE", "K1ABC"), ("READ", "N0CALL"), ("UNREAD", "W1AW")]
        );
        assert_eq!(
            keys(&listed[0]["params"]),
            set(&[
                "CMD", "DIAL", "FREQ", "FROM", "OFFSET", "PATH", "SNR", "SUBMODE", "TDRIFT",
                "TEXT", "TO", "UTC", "_ID"
            ])
        );
        assert!(!keys(&listed[1]["params"]).contains("TEXT"));
        assert_eq!(listed[0]["params"]["UTC"], "1970-01-01 00:50:00");
        assert!(listed[0]["params"]["_ID"].is_string());
        assert_eq!(keys(&listed[0]), set(&["params", "type", "value"]));
        // CALLSIGN matches a held message's TO, a received one's FROM.
        let w1aw: Vec<String> = inbox_records(&rows, "w1aw")
            .iter()
            .map(|m| m["type"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(w1aw, vec!["STORE", "UNREAD"]);
    }

    #[test]
    fn times_are_utc_text() {
        assert_eq!(utc_text(0), "1970-01-01 00:00:00");
        assert_eq!(utc_text(1_769_740_328_005), "2026-01-30 02:32:08");
        assert_eq!(utc_text(951_782_400_000), "2000-02-29 00:00:00");
        assert_eq!(utc_text(4_107_542_400_000), "2100-03-01 00:00:00");
        assert_eq!(hms(1_769_740_328_005), "02:32:08");
    }

    /// What a tick's look at the station announces: the band once, the status when its four
    /// fields move, the key going up before the tick's frames and down after them.
    #[test]
    fn the_watch_announces_changes_once() {
        let kinds = |v: &[Outgoing]| -> Vec<String> {
            v.iter()
                .map(|o| json(&o.line)["type"].as_str().unwrap().to_string())
                .collect()
        };
        let mut w = StateWatch::default();
        let mut s = Js8ApiSnapshot {
            band: "40m".into(),
            dial_hz: 7_078_000,
            offset_hz: 1500,
            ..Js8ApiSnapshot::default()
        };
        assert_eq!(kinds(&w.step(&s, NOW).0), ["RIG.FREQ", "STATION.STATUS"]);
        let (b, a) = w.step(&s, NOW);
        assert!(b.is_empty() && a.is_empty());
        s.offset_hz = 1600;
        assert_eq!(kinds(&w.step(&s, NOW).0), ["STATION.STATUS"]);
        s.transmitting = true;
        let (b, a) = w.step(&s, NOW);
        assert_eq!(
            (kinds(&b), kinds(&a)),
            (vec!["RIG.PTT".to_string(), "STATION.STATUS".into()], vec![])
        );
        s.transmitting = false;
        let (b, a) = w.step(&s, NOW);
        assert_eq!(
            (kinds(&b), kinds(&a)),
            (vec![], vec!["RIG.PTT".to_string(), "STATION.STATUS".into()])
        );
        assert_eq!(json(&a[0].line)["value"], "off");
        s.band = "20m".into();
        s.dial_hz = 14_078_000;
        assert_eq!(kinds(&w.step(&s, NOW).0), ["RIG.FREQ", "STATION.STATUS"]);
    }

    /// Both servers ship OFF on JS8Call's ports, and a settings file from before them loads so.
    #[test]
    fn the_api_ships_off_on_js8calls_ports() {
        let s = crate::settings::Settings::default();
        assert_eq!(
            (
                s.js8_api_tcp,
                s.js8_api_tcp_port,
                s.js8_api_udp,
                s.js8_api_udp_port
            ),
            (false, 2442, false, 2242)
        );
        let mut v = serde_json::to_value(&s).unwrap();
        let o = v.as_object_mut().unwrap();
        for k in ["js8ApiTcp", "js8ApiTcpPort", "js8ApiUdp", "js8ApiUdpPort"] {
            assert!(o.remove(k).is_some(), "{k} is the wire name");
        }
        let old: crate::settings::Settings = serde_json::from_value(v).unwrap();
        assert_eq!(
            (
                old.js8_api_tcp,
                old.js8_api_tcp_port,
                old.js8_api_udp,
                old.js8_api_udp_port
            ),
            (false, 2442, false, 2242)
        );
        assert!(Engine::with_settings(s).js8_api_poll().is_none());
    }

    /// The transmit tap reads the frame and changes nothing keyed: the same message planned
    /// with the API on and off keys the same words at the same offsets in the same periods, and
    /// with it on each frame's `TX.FRAME` carries that word's 79 tones.
    #[test]
    fn tx_frames_carry_the_keyed_tones_and_change_nothing_keyed() {
        let run = |api: bool| {
            let mut e = engine();
            e.settings.js8_api_tcp = api;
            e.set_tx_enabled(true);
            e.js8_send(Some("W1AW".into()), "HELLO THERE FROM NEXUS".into())
                .expect("queued");
            // The radio loop's shape (`run_js8_loop_from` in js8.rs): a tick a second, and at
            // each period boundary plan → build → commit. Periods counted from the start, so the
            // two runs compare whatever millisecond each began on.
            let period_ms = u64::from(e.js8_tx_speed().period_s()) * 1000;
            let t0 = tempo_core::timing::now_unix_ms() as u64;
            let first = t0 / period_ms;
            let mut overs = Vec::new();
            let mut last = first;
            for k in 0..=120 {
                let t = t0 + k * 1000;
                e.js8_tick(t);
                let slot = t / period_ms;
                if slot == last {
                    continue;
                }
                last = slot;
                let Some(plan) = e.plan_tx(slot) else {
                    continue;
                };
                let super::super::TxWaveform::Js8 { speed, word, f0 } = &plan.waveform else {
                    panic!("a JS8 plan carries the typed waveform");
                };
                overs.push((slot - first, *speed, *word.as_bytes(), *f0));
                let wave = plan.waveform.build();
                e.commit_tx(&plan, wave, slot);
            }
            let tones: Vec<Vec<u8>> = e
                .js8_api
                .events
                .iter()
                .filter_map(|ev| match ev {
                    Js8ApiEvent::TxFrame { tones } => Some(tones.clone()),
                    _ => None,
                })
                .collect();
            (overs, tones, e.js8_api_snapshot().text)
        };
        let (on, tones, pane) = run(true);
        let (off, none, _) = run(false);
        assert!(on.len() > 1, "a multi-frame message keyed: {on:?}");
        assert_eq!(on, off, "the API changed what was keyed");
        assert!(none.is_empty());
        let want: Vec<Vec<u8>> = on
            .iter()
            .map(|(_, speed, word, _)| {
                ::js8::phy::encode_word(&::js8::Word87::from_bytes(*word), *speed).to_vec()
            })
            .collect();
        assert_eq!(tones, want);
        assert!(tones
            .iter()
            .all(|t| t.len() == 79 && t[..7] == [4, 2, 5, 6, 1, 3, 0]));
        // The pane holds the transmission as one block, closed with the end mark.
        assert_eq!(pane.matches(" - (1500) - ").count(), 1, "{pane:?}");
        assert!(pane.ends_with(" \u{2662} "), "{pane:?}");
    }
}
