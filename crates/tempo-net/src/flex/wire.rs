//! The SmartSDR TCP line codec: one line from the radio parsed into a typed [`Line`], strictly,
//! and one command framed for the wire.
//!
//! The API is line-based ASCII over TCP 4992 (D `SmartSDR-TCPIP-API`):
//!
//! | Line | Meaning |
//! |---|---|
//! | `V<version>` | the TCP API version, first on connect (`V1.4.0.0` on 4.x firmware) |
//! | `H<8 hex digits>` | this client's handle, second |
//! | `R<seq>\|<code, hex>\|<message>[\|<debug>]` | exactly one reply per command, by the client's own sequence number |
//! | `S<handle, hex>\|<object> <key=value …>` | a status update |
//! | `M<number, hex>\|<text>` | a message; bits 24–25 of the number are its [`Severity`] |
//! | `C<seq>\|<command>` | a command, client to radio ([`frame`]) |
//!
//! **Strict, where upstream is tolerant.** Upstream's parser serves a display and turns a malformed
//! number into `0`. Here a malformed sequence number, result code, handle or message number makes
//! the whole line a [`WireError`], never a value: `R102|garbage|` is not a successful reply,
//! `H+1234567` is not a handle, and `S0|…` with a bad envelope is not a status. A line is taken
//! exactly as it arrived (the assembler strips only the terminator): no trimming, so a leading
//! space is an unknown tag and a trailing one on the version is a bad version. A key that one
//! status line repeats is ambiguous and reads as absent ([`Kvs::get`]).
//!
//! **Only rendered commands are framed.** [`frame`] is internal to the Flex module and the session
//! calls it only with text the typed encoder produced (`super::encode::Rendered`), so nothing in
//! Nexus can put raw text on the radio's command channel.
//!
//! PORTED from AetherSDR (https://github.com/aethersdr/AetherSDR, GPL-3.0; the upstream file
//! carries no per-file header, the licence is the repository's), `src/core/backends/flex/CommandParser.h`,
//! `src/core/backends/flex/CommandParser.cpp` and `src/core/RadioMessageTypes.h` at commit
//! `32fa50e4896a846a6970fa3f443bd49d667c139d` (2026-10-03), translated from C++/Qt to Rust.
//! Deliberate differences: strict numeric parsing (a malformed sequence, result code, handle or
//! message number is an error, never `0`); no trimming of the raw line; exactly eight hex digits
//! for the handle; a repeated status key is ambiguous rather than last-wins; bare words are kept
//! apart from `key=` with an empty value; framing takes only encoder output. Recorded in the
//! repo-root NOTICE (AetherSDR entry).

use std::fmt;

/// The longest run of bytes the line assembler holds without a terminator before it ends the
/// session. CAT lines are tens of bytes; this is far beyond any legitimate status burst and stops
/// a peer that never sends `\n` from growing the buffer without bound (upstream `kMaxReadBuffer`).
pub const MAX_LINE: usize = 16 * 1024 * 1024;

/// One line from the radio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// `V…`: the TCP API version text, unvalidated beyond being one printable word. Whether it is
    /// a version Nexus may transmit on is the session's call (`super::session::supports_protocol`).
    Version(String),
    /// `H<8 hex digits>`: this connection's client handle.
    Handle(u32),
    /// `R<seq>|<code>|<message>`.
    Reply(Reply),
    /// `S<handle>|<body>`.
    Status(Status),
    /// `M<number>|<text>`.
    Message(Message),
}

/// A reply to the command the client numbered `seq`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub seq: u32,
    /// The result code, `0` for success. Fatal codes such as `F3000001` keep their high bit.
    pub code: u32,
    /// Everything after the second `|`, which may itself contain `|` (the debug field).
    pub message: String,
}

impl Reply {
    pub fn is_success(&self) -> bool {
        self.code == 0
    }
}

/// A status update. The object is the text before the last space that precedes the first `=`
/// (`slice 0`, `display pan 0x40000000`, `interlock`); the rest is key/value tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The envelope handle (`S<handle>|`): `0`, or the client the status concerns.
    pub handle: u32,
    pub object: String,
    pub kvs: Kvs,
    /// The whole body after the `|`. Meter and profile status are not key/value shaped (`#`
    /// separators, values with spaces) and are decoded from this.
    pub body: String,
}

/// A message's severity, from bits 24–25 of its number (FlexLib `Radio.cs`, as upstream cites it).
/// Info is routine; Warning and above are worth the operator's attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
    Fatal,
}

impl Severity {
    /// The severity encoded in a message number. The wire values are load-bearing: 0 Info,
    /// 1 Warning, 2 Error, 3 Fatal.
    pub fn of(number: u32) -> Severity {
        match (number >> 24) & 0x3 {
            0 => Severity::Info,
            1 => Severity::Warning,
            2 => Severity::Error,
            _ => Severity::Fatal,
        }
    }
}

/// An `M` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The whole number: severity in bits 24–25, the rest an opaque message id.
    pub number: u32,
    pub severity: Severity,
    pub text: String,
}

/// Why a line is not a value. The line is dropped; the session reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    Empty,
    /// A NUL, CR or LF inside the line.
    ControlCharacter,
    /// The first character is none of `V H R S M`.
    UnknownTag(char),
    /// `V` followed by nothing, or by whitespace or a non-printable character.
    BadVersion,
    /// `H` not followed by exactly eight hex digits.
    BadHandle,
    /// `R` whose sequence number is missing, signed, not decimal or wider than 32 bits.
    BadSequence,
    /// `R` with no second `|`: the reply has no message field.
    NoMessageField,
    /// `R` whose result code is empty, not hex or wider than 32 bits.
    BadCode,
    /// `S` with no `|`.
    NoStatusBody,
    /// `S` whose envelope handle is not 1–8 hex digits.
    BadStatusHandle,
    /// `M` with no `|`.
    NoMessageText,
    /// `M` whose number is not 1–8 hex digits.
    BadMessageNumber,
    /// The line's bytes are not UTF-8 (reported by the line assembler; [`parse_line`] takes text).
    NotUtf8,
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for WireError {}

/// Parse one line, terminator already stripped. Pure.
pub fn parse_line(line: &str) -> Result<Line, WireError> {
    if line.is_empty() {
        return Err(WireError::Empty);
    }
    if line.bytes().any(|b| matches!(b, b'\0' | b'\r' | b'\n')) {
        return Err(WireError::ControlCharacter);
    }
    let mut chars = line.chars();
    let tag = chars.next().unwrap_or('\0');
    let rest = chars.as_str();
    match tag {
        'V' => parse_version(rest).map(Line::Version),
        'H' => parse_hex_exact(rest, 8)
            .map(Line::Handle)
            .ok_or(WireError::BadHandle),
        'R' => parse_reply(rest).map(Line::Reply),
        'S' => parse_status(rest).map(Line::Status),
        'M' => parse_message(rest).map(Line::Message),
        other => Err(WireError::UnknownTag(other)),
    }
}

fn parse_version(rest: &str) -> Result<String, WireError> {
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(WireError::BadVersion);
    }
    Ok(rest.to_string())
}

fn parse_reply(rest: &str) -> Result<Reply, WireError> {
    let (seq, rest) = rest.split_once('|').ok_or(WireError::BadSequence)?;
    let seq = parse_dec_u32(seq).ok_or(WireError::BadSequence)?;
    let (code, message) = rest.split_once('|').ok_or(WireError::NoMessageField)?;
    let code = parse_hex_u32(code, 8).ok_or(WireError::BadCode)?;
    Ok(Reply {
        seq,
        code,
        message: message.to_string(),
    })
}

fn parse_status(rest: &str) -> Result<Status, WireError> {
    let (handle, body) = rest.split_once('|').ok_or(WireError::NoStatusBody)?;
    let handle = parse_hex_u32(handle, 8).ok_or(WireError::BadStatusHandle)?;
    let (object, kvs) = split_status(body);
    Ok(Status {
        handle,
        object,
        kvs,
        body: body.to_string(),
    })
}

fn parse_message(rest: &str) -> Result<Message, WireError> {
    let (number, text) = rest.split_once('|').ok_or(WireError::NoMessageText)?;
    let number = parse_hex_u32(number, 8).ok_or(WireError::BadMessageNumber)?;
    Ok(Message {
        number,
        severity: Severity::of(number),
        text: text.to_string(),
    })
}

/// Split a status body into its object and its key/value tokens. Object names are multi-word
/// (`slice 0`, `display pan 0x40000000`); key/value tokens always hold `=` and object words never
/// do, so the boundary is the last space before the first `=`. A body with no `=` is all object
/// (`stream 0x04000001 removed`); a body that starts with a token is all tokens.
pub fn split_status(body: &str) -> (String, Kvs) {
    match body.find('=') {
        None => (body.trim().to_string(), Kvs::default()),
        Some(eq) => match body[..eq].rfind(' ') {
            None => (String::new(), Kvs::parse(body)),
            Some(space) => (
                body[..space].trim().to_string(),
                Kvs::parse(&body[space + 1..]),
            ),
        },
    }
}

/// The key/value tokens of a status line or reply, in wire order. A bare word (`removed`,
/// `connected`) is a key with no value, kept apart from `key=` with an empty one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Kvs {
    pairs: Vec<(String, Option<String>)>,
}

impl Kvs {
    /// Tokens are separated by single spaces (runs of them are skipped); the first `=` in a token
    /// separates its key from its value.
    pub fn parse(body: &str) -> Kvs {
        let pairs = body
            .split(' ')
            .filter(|t| !t.is_empty())
            .map(|t| match t.split_once('=') {
                Some((k, v)) => (k.to_string(), Some(v.to_string())),
                None => (t.to_string(), None),
            })
            .collect();
        Kvs { pairs }
    }

    /// The value of `key`, when the line carries `key=value` exactly once. A key that appears
    /// twice is ambiguous and reads as absent, as does a bare word.
    pub fn get(&self, key: &str) -> Option<&str> {
        let mut found = None;
        for (k, v) in &self.pairs {
            if k == key {
                if found.is_some() {
                    return None;
                }
                found = Some(v.as_deref());
            }
        }
        found.flatten()
    }

    /// Whether `key` appears at all, as a bare word or with a value.
    pub fn has(&self, key: &str) -> bool {
        self.pairs.iter().any(|(k, _)| k == key)
    }

    /// How many times `key` appears.
    pub fn count(&self, key: &str) -> usize {
        self.pairs.iter().filter(|(k, _)| k == key).count()
    }

    /// The bare words (tokens with no `=`), in order.
    pub fn bare_words(&self) -> impl Iterator<Item = &str> {
        self.pairs
            .iter()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// Every token, in wire order: `(key, Some(value))` or `(word, None)`.
    pub fn iter(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.pairs.iter().map(|(k, v)| (k.as_str(), v.as_deref()))
    }
}

/// `C<seq>|<command>\n`. Internal to the Flex module: the session frames only text the typed
/// encoder rendered, which never holds a line terminator.
pub(super) fn frame(seq: u32, command: &str) -> Vec<u8> {
    format!("C{seq}|{command}\n").into_bytes()
}

/// Decimal digits only, at most ten, fitting 32 bits: no sign, no space, not empty.
pub(super) fn parse_dec_u32(text: &str) -> Option<u32> {
    if text.is_empty() || text.len() > 10 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u32>().ok()
}

/// Hex digits only, one to `max_digits` of them.
pub(super) fn parse_hex_u32(text: &str, max_digits: usize) -> Option<u32> {
    if text.is_empty() || text.len() > max_digits || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(text, 16).ok()
}

/// Exactly `digits` hex digits.
fn parse_hex_exact(text: &str, digits: usize) -> Option<u32> {
    if text.len() != digits {
        return None;
    }
    parse_hex_u32(text, digits)
}

/// An object id or client handle as status carries it: `0x` and one to eight hex digits. A value
/// without the prefix, or with anything else in it, is not an id (upstream's tolerant parser read
/// it as `0`, which its own stream-ownership check then took as "ours").
pub(super) fn parse_id(text: &str) -> Option<u32> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))?;
    parse_hex_u32(digits, 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(line: &str) -> Status {
        match parse_line(line) {
            Ok(Line::Status(s)) => s,
            other => panic!("{line:?} is not a status: {other:?}"),
        }
    }

    #[test]
    fn each_line_kind_parses() {
        assert_eq!(parse_line("V1.4.0.0"), Ok(Line::Version("1.4.0.0".into())));
        assert_eq!(parse_line("H2B6E1F40"), Ok(Line::Handle(0x2B6E_1F40)));
        assert_eq!(parse_line("h2B6E1F40"), Err(WireError::UnknownTag('h')));
        assert_eq!(
            parse_line("R1|0|"),
            Ok(Line::Reply(Reply {
                seq: 1,
                code: 0,
                message: String::new()
            }))
        );
        // The message keeps everything after the second pipe, a debug field included.
        assert_eq!(
            parse_line("R2|50001001|No Such Object|dbg"),
            Ok(Line::Reply(Reply {
                seq: 2,
                code: 0x5000_1001,
                message: "No Such Object|dbg".into()
            }))
        );
        // A fatal code keeps its high bit: it is a refusal, never success.
        match parse_line("R7|F3000001|full") {
            Ok(Line::Reply(r)) => {
                assert_eq!(r.code, 0xF300_0001);
                assert!(!r.is_success());
            }
            other => panic!("{other:?}"),
        }
        let m = match parse_line("M01000001|Client connected") {
            Ok(Line::Message(m)) => m,
            other => panic!("{other:?}"),
        };
        assert_eq!(m.severity, Severity::Warning);
        assert_eq!(m.number, 0x0100_0001);
        assert_eq!(m.text, "Client connected");
    }

    #[test]
    fn severity_is_bits_24_and_25() {
        assert_eq!(Severity::of(0x0000_0001), Severity::Info);
        assert_eq!(Severity::of(0x0100_0000), Severity::Warning);
        assert_eq!(Severity::of(0x0200_0000), Severity::Error);
        assert_eq!(Severity::of(0x0300_0000), Severity::Fatal);
        // Only bits 24–25: F3000001 is Fatal through its low two bits of the top byte.
        assert_eq!(Severity::of(0xF300_0001), Severity::Fatal);
        assert_eq!(Severity::of(0x5000_0000), Severity::Info);
    }

    #[test]
    fn a_malformed_number_is_never_a_value() {
        // Upstream's reply rows (flex_ptt_stop_tracker_test, strictFramesAndOwnership): each of
        // these must not become result code zero.
        for (line, err) in [
            ("R102|garbage|", WireError::BadCode),
            ("R102||", WireError::BadCode),
            ("R102|", WireError::NoMessageField),
            ("R102|100000000|", WireError::BadCode),
            ("R102|+0|", WireError::BadCode),
            ("R102| 0|", WireError::BadCode),
            ("R4294967296|0|", WireError::BadSequence),
            ("R|0|", WireError::BadSequence),
            ("R-1|0|", WireError::BadSequence),
            ("R1", WireError::BadSequence),
        ] {
            assert_eq!(parse_line(line), Err(err), "{line:?}");
        }
        // Upstream's prologue rows (radio_connection_session_test): a signed, short or zero-padded
        // wrong-width handle is not a handle.
        for line in ["H+1234567", "H1234567", "H123456789", "H", "H1234567g"] {
            assert_eq!(parse_line(line), Err(WireError::BadHandle), "{line:?}");
        }
        // Zero is well formed here; that zero is no identity is the session's rule.
        assert_eq!(parse_line("H00000000"), Ok(Line::Handle(0)));
        assert_eq!(
            parse_line("Sg|interlock state=READY"),
            Err(WireError::BadStatusHandle)
        );
        assert_eq!(parse_line("S|radio x=1"), Err(WireError::BadStatusHandle));
        assert_eq!(parse_line("S0 radio x=1"), Err(WireError::NoStatusBody));
        assert_eq!(parse_line("Mxyz|text"), Err(WireError::BadMessageNumber));
        assert_eq!(parse_line("M10000001"), Err(WireError::NoMessageText));
    }

    #[test]
    fn the_raw_line_is_not_trimmed() {
        assert_eq!(parse_line(" V1.4.0.0"), Err(WireError::UnknownTag(' ')));
        assert_eq!(parse_line("V1.4.0.0 "), Err(WireError::BadVersion));
        assert_eq!(parse_line("V"), Err(WireError::BadVersion));
        // A version with junk is still one printable word; refusing to transmit on it is the
        // session's decision, not a parse error.
        assert_eq!(
            parse_line("V1.4.0.0beta"),
            Ok(Line::Version("1.4.0.0beta".into()))
        );
        assert_eq!(parse_line(""), Err(WireError::Empty));
        assert_eq!(
            parse_line("S0|radio\rx=1"),
            Err(WireError::ControlCharacter)
        );
        assert_eq!(parse_line("R1|0|a\0b"), Err(WireError::ControlCharacter));
        assert_eq!(parse_line("Xfoo"), Err(WireError::UnknownTag('X')));
    }

    #[test]
    fn the_object_is_the_text_before_the_last_space_preceding_the_first_equals() {
        let s = status("S0|slice 0 RF_frequency=14.074000 mode=DIGU filter_lo=0");
        assert_eq!(s.object, "slice 0");
        assert_eq!(s.kvs.get("RF_frequency"), Some("14.074000"));
        assert_eq!(s.kvs.get("mode"), Some("DIGU"));

        let s = status("S2B6E1F40|display pan 0x40000000 center=14.100000 bandwidth=0.200000");
        assert_eq!(s.handle, 0x2B6E_1F40);
        assert_eq!(s.object, "display pan 0x40000000");
        assert_eq!(s.kvs.get("bandwidth"), Some("0.200000"));

        // No '=': the whole body is the object.
        let s = status("S0|stream 0x04000001 removed");
        assert_eq!(s.object, "stream 0x04000001 removed");
        assert!(s.kvs.is_empty());

        // A bare word before the first token stays in the object.
        let s = status("S7A3C0001|client 0x7A3C0001 connected local_ptt=1 program=SmartSDR-Win");
        assert_eq!(s.object, "client 0x7A3C0001 connected");
        assert_eq!(s.kvs.get("local_ptt"), Some("1"));

        // A body that starts with a token has no object.
        let s = status("S0|in_use=1 x=2");
        assert_eq!(s.object, "");
        assert_eq!(s.kvs.get("x"), Some("2"));

        // The body is kept whole for the decoders that need it.
        let s = status("S0|meter 1.src=TX-#1.num=1#1.nam=FWDPWR");
        assert_eq!(s.object, "meter");
        assert_eq!(s.body, "meter 1.src=TX-#1.num=1#1.nam=FWDPWR");
    }

    #[test]
    fn a_repeated_key_is_ambiguous_and_a_bare_word_has_no_value() {
        let kvs = Kvs::parse("state=READY  state=TRANSMITTING reason= source= SW removed");
        assert_eq!(kvs.get("state"), None, "said twice: not a reading");
        assert_eq!(kvs.count("state"), 2);
        assert_eq!(kvs.get("reason"), Some(""), "an empty value is a value");
        assert_eq!(kvs.get("SW"), None, "a bare word has no value");
        assert!(kvs.has("removed"));
        assert_eq!(kvs.bare_words().collect::<Vec<_>>(), ["SW", "removed"]);
        // Only the first '=' separates: the value keeps the rest.
        assert_eq!(Kvs::parse("a=b=c").get("a"), Some("b=c"));
    }

    #[test]
    fn the_numeric_helpers_are_strict() {
        assert_eq!(parse_dec_u32("4294967295"), Some(u32::MAX));
        for bad in [
            "",
            "4294967296",
            "+1",
            "-1",
            " 1",
            "1 ",
            "0x1",
            "12345678901",
        ] {
            assert_eq!(parse_dec_u32(bad), None, "{bad:?}");
        }
        assert_eq!(parse_hex_u32("F3000001", 8), Some(0xF300_0001));
        assert_eq!(parse_hex_u32("0", 8), Some(0));
        for bad in ["", "123456789", "0x1", "g", "+1"] {
            assert_eq!(parse_hex_u32(bad, 8), None, "{bad:?}");
        }
        assert_eq!(parse_id("0x2B6E1F40"), Some(0x2B6E_1F40));
        assert_eq!(parse_id("0X0a"), Some(0x0A));
        assert_eq!(parse_id("0x00000000"), Some(0));
        for bad in [
            "2B6E1F40",
            "0x",
            "0x123456789",
            "0xgarbage",
            "305419896",
            " 0x1",
        ] {
            assert_eq!(parse_id(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_command_is_framed_with_its_sequence_number() {
        assert_eq!(frame(7, "sub slice all"), b"C7|sub slice all\n");
    }
}
