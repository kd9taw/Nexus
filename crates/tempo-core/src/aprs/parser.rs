//! APRS information-field parser + formatter.
//!
//! The AX.25 UI info field's first byte is the APRS **data-type identifier** (DTI) that selects the
//! payload format (APRS 1.0.1 spec §5). This layer decodes the common text types into structured
//! data and re-encodes them:
//!
//! - `!` `=` `/` `@` — position reports (uncompressed lat/lon; `/ @` carry a timestamp, `= @` are messaging-capable)
//! - `:` — messages (addressee + text + optional line number)
//! - `>` — status
//!
//! Anything else (compressed positions, Mic-E, weather, telemetry, objects…) is preserved verbatim
//! as [`AprsInfo::Other`] so nothing is lost. Pure Rust, unit-tested by parse↔format round-trips
//! against the spec's worked examples.

/// A decoded APRS position (uncompressed). Latitude is +north, longitude is +east, both in degrees.
#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    pub lat: f64,
    pub lon: f64,
    /// Symbol-table selector (`/` primary, `\` alternate, or an overlay char).
    pub symbol_table: char,
    /// Symbol code within the table.
    pub symbol_code: char,
    /// Raw 7-char APRS timestamp (e.g. `092345z`) when the report carried one.
    pub timestamp: Option<String>,
    /// `true` for the messaging-capable variants (`=` / `@`).
    pub messaging: bool,
    pub comment: String,
}

/// A decoded APRS text message.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// Addressee callsign (trailing pad spaces trimmed).
    pub addressee: String,
    pub text: String,
    /// Optional message line number (the `{NNN` suffix).
    pub id: Option<String>,
}

/// A decoded APRS information field.
#[derive(Debug, Clone, PartialEq)]
pub enum AprsInfo {
    Position(Position),
    Status {
        timestamp: Option<String>,
        text: String,
    },
    Message(Message),
    /// An object report (`;`) — a named point (repeater, NWS alert, event…) with its own position.
    Object {
        name: String,
        killed: bool,
        position: Position,
    },
    /// Any type this parser doesn't decode — the DTI plus the raw remainder, preserved.
    Other {
        dti: char,
        body: String,
    },
}

fn parse_lat(b: &[u8]) -> Option<f64> {
    // "DDMM.hhN" — 8 ASCII bytes. Takes raw bytes and validates ASCII before any str view
    // exists, so the fixed offsets below cannot land inside a multi-byte char.
    if b.len() != 8 || !b.is_ascii() {
        return None;
    }
    let s = std::str::from_utf8(&b[0..7]).ok()?;
    let deg: f64 = s[0..2].parse().ok()?;
    let min: f64 = s[2..7].parse().ok()?; // "MM.hh"
    if !(0.0..60.0).contains(&min) || deg > 90.0 {
        return None;
    }
    let mag = deg + min / 60.0;
    match b[7] {
        b'N' => Some(mag),
        b'S' => Some(-mag),
        _ => None,
    }
}

fn parse_lon(b: &[u8]) -> Option<f64> {
    // "DDDMM.hhW" — 9 ASCII bytes; same raw-byte contract as [`parse_lat`].
    if b.len() != 9 || !b.is_ascii() {
        return None;
    }
    let s = std::str::from_utf8(&b[0..8]).ok()?;
    let deg: f64 = s[0..3].parse().ok()?;
    let min: f64 = s[3..8].parse().ok()?;
    if !(0.0..60.0).contains(&min) || deg > 180.0 {
        return None;
    }
    let mag = deg + min / 60.0;
    match b[8] {
        b'E' => Some(mag),
        b'W' => Some(-mag),
        _ => None,
    }
}

/// Lossy-decode a genuinely free-text region (comment, message/status text, names) for
/// storage. Fixed-layout regions are parsed as raw bytes BEFORE this runs: replacement
/// expands an 8-bit byte (a latin-1 degree sign is routine off the air) into a 3-byte
/// U+FFFD, which shifts every later fixed offset — decode-then-slice panicked on exactly
/// that, so the order is load-bearing.
fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Format degrees as `DDMM.hhH`. `deg_width` is 2 (lat) or 3 (lon); `pos`/`neg` are the hemisphere
/// letters. Uses integer hundredths-of-a-minute so a value that rounds up carries into degrees
/// instead of emitting an invalid `60.00`.
fn format_dm(value: f64, deg_width: usize, pos: char, neg: char) -> String {
    // `is_sign_negative` (not `< 0.0`) so an exactly-zero coordinate keeps its hemisphere: parsing
    // `W`/`S` of 0 yields -0.0, which must re-emit as W/S, not E/N.
    let hemi = if value.is_sign_negative() { neg } else { pos };
    let hmin = (value.abs() * 6000.0).round() as u64; // hundredths of a minute
    let deg = hmin / 6000;
    let rem = hmin % 6000;
    format!(
        "{:0deg_width$}{:02}.{:02}{}",
        deg,
        rem / 100,
        rem % 100,
        hemi,
        deg_width = deg_width
    )
}

/// Decode 4 base-91 chars (each `!`..`{`) to their integer value. `None` on an out-of-range byte.
fn base91(b: &[u8]) -> Option<f64> {
    let mut v = 0.0;
    for &c in b {
        if !(0x21..=0x7b).contains(&c) {
            return None;
        }
        v = v * 91.0 + f64::from(c - 33);
    }
    Some(v)
}

impl Position {
    fn parse(body: &[u8], messaging: bool, has_ts: bool) -> Option<Position> {
        let (timestamp, rest) = if has_ts {
            if body.len() < 7 {
                return None;
            }
            (Some(lossy(&body[..7])), &body[7..])
        } else {
            (None, body)
        };
        // Uncompressed positions start with a digit (the latitude's tens); compressed positions
        // start with the symbol-table char. Route on the first byte.
        match rest.first()? {
            b'0'..=b'9' => Self::parse_uncompressed(rest, timestamp, messaging),
            _ => Self::parse_compressed(rest, timestamp, messaging),
        }
    }

    fn parse_uncompressed(
        rest: &[u8],
        timestamp: Option<String>,
        messaging: bool,
    ) -> Option<Position> {
        // lat(8) + symtable(1) + lon(9) + symcode(1) = 19 fixed BYTES, then the comment.
        // The whole fixed region is sliced from raw bytes — `&[u8]` indexing cannot land
        // inside a char — and only the trailing comment is UTF-8-decoded.
        if rest.len() < 19 {
            return None;
        }
        let lat = parse_lat(&rest[0..8])?;
        let symbol_table = rest[8] as char;
        let lon = parse_lon(&rest[9..18])?;
        let symbol_code = rest[18] as char;
        Some(Position {
            lat,
            lon,
            symbol_table,
            symbol_code,
            timestamp,
            messaging,
            comment: lossy(&rest[19..]),
        })
    }

    /// Compressed (base-91) position: `<sym-table><YYYY lat><XXXX lon><sym-code><cs><T>` = 13 bytes,
    /// then the comment. `lat = 90 − Y/380926`, `lon = −180 + X/190463` (APRS 1.0.1 §9).
    fn parse_compressed(
        rest: &[u8],
        timestamp: Option<String>,
        messaging: bool,
    ) -> Option<Position> {
        if rest.len() < 13 {
            return None;
        }
        let symbol_table = rest[0] as char;
        let lat = 90.0 - base91(&rest[1..5])? / 380_926.0;
        let lon = -180.0 + base91(&rest[5..9])? / 190_463.0;
        if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
            return None;
        }
        let symbol_code = rest[9] as char;
        // rest[10..13] = compression type (course/speed, altitude, or range) — kept out of the comment.
        Some(Position {
            lat,
            lon,
            symbol_table,
            symbol_code,
            timestamp,
            messaging,
            comment: lossy(&rest[13..]),
        })
    }
}

/// Parse an APRS information field into structured data (never fails — unknown types become
/// [`AprsInfo::Other`]).
pub fn parse(info: &[u8]) -> AprsInfo {
    let Some(&dti_byte) = info.first() else {
        return AprsInfo::Other {
            dti: '\0',
            body: String::new(),
        };
    };
    let dti = dti_byte as char;
    // DTI is one byte; a non-ASCII first byte is "no DTI" — the whole field stays the body.
    // Everything downstream takes raw bytes and decodes only its free-text tail (see [`lossy`]).
    let body: &[u8] = if dti_byte.is_ascii() {
        &info[1..]
    } else {
        info
    };

    let parsed = match dti {
        '!' => Position::parse(body, false, false).map(AprsInfo::Position),
        '=' => Position::parse(body, true, false).map(AprsInfo::Position),
        '/' => Position::parse(body, false, true).map(AprsInfo::Position),
        '@' => Position::parse(body, true, true).map(AprsInfo::Position),
        ':' => parse_message(body),
        '>' => Some(parse_status(body)),
        ';' => parse_object(body),
        _ => None,
    };
    parsed.unwrap_or_else(|| AprsInfo::Other {
        dti,
        body: lossy(body),
    })
}

/// Parse an object report body: `NNNNNNNNN` (9-byte name) + `*` (live) / `_` (killed) + a
/// timestamped position (uncompressed or compressed).
fn parse_object(body: &[u8]) -> Option<AprsInfo> {
    if body.len() < 10 {
        return None;
    }
    let name = lossy(&body[..9]).trim_end().to_string();
    let killed = match body[9] {
        b'*' => false,
        b'_' => true,
        _ => return None,
    };
    let position = Position::parse(&body[10..], false, true)?;
    Some(AprsInfo::Object {
        name,
        killed,
        position,
    })
}

fn parse_message(body: &[u8]) -> Option<AprsInfo> {
    // ":<addressee: 9 bytes>:<text>[{id]"
    if body.len() < 10 || body[9] != b':' {
        return None;
    }
    let addressee = lossy(&body[..9]).trim_end().to_string();
    let payload = lossy(&body[10..]);
    let with_id = payload
        .rsplit_once('{')
        .filter(|(_, i)| !i.is_empty() && i.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(|(t, i)| (t.to_string(), i.to_string()));
    let (text, id) = match with_id {
        Some((t, i)) => (t, Some(i)),
        None => (payload, None),
    };
    Some(AprsInfo::Message(Message {
        addressee,
        text,
        id,
    }))
}

fn parse_status(body: &[u8]) -> AprsInfo {
    // A status may open with a zulu day/hour/min timestamp "DDHHMMz".
    if body.len() >= 7 && body[6] == b'z' && body[..6].iter().all(|b| b.is_ascii_digit()) {
        AprsInfo::Status {
            timestamp: Some(lossy(&body[..7])),
            text: lossy(&body[7..]),
        }
    } else {
        AprsInfo::Status {
            timestamp: None,
            text: lossy(body),
        }
    }
}

impl AprsInfo {
    /// Encode structured data back into an APRS information field.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = String::new();
        match self {
            AprsInfo::Position(p) => {
                out.push(match (p.messaging, p.timestamp.is_some()) {
                    (false, false) => '!',
                    (true, false) => '=',
                    (false, true) => '/',
                    (true, true) => '@',
                });
                if let Some(ts) = &p.timestamp {
                    out.push_str(ts);
                }
                out.push_str(&format_dm(p.lat, 2, 'N', 'S'));
                out.push(p.symbol_table);
                out.push_str(&format_dm(p.lon, 3, 'E', 'W'));
                out.push(p.symbol_code);
                out.push_str(&p.comment);
            }
            AprsInfo::Object {
                name,
                killed,
                position,
            } => {
                out.push(';');
                out.push_str(&format!("{name:<9}"));
                out.push(if *killed { '_' } else { '*' });
                out.push_str(position.timestamp.as_deref().unwrap_or("000000z"));
                out.push_str(&format_dm(position.lat, 2, 'N', 'S'));
                out.push(position.symbol_table);
                out.push_str(&format_dm(position.lon, 3, 'E', 'W'));
                out.push(position.symbol_code);
                out.push_str(&position.comment);
            }
            AprsInfo::Status { timestamp, text } => {
                out.push('>');
                if let Some(ts) = timestamp {
                    out.push_str(ts);
                }
                out.push_str(text);
            }
            AprsInfo::Message(m) => {
                out.push(':');
                out.push_str(&format!("{:<9}", m.addressee));
                out.push(':');
                out.push_str(&m.text);
                if let Some(id) = &m.id {
                    out.push('{');
                    out.push_str(id);
                }
            }
            AprsInfo::Other { dti, body } => {
                if *dti != '\0' {
                    out.push(*dti);
                }
                out.push_str(body);
            }
        }
        out.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_spec_position_example() {
        // APRS 1.0.1 §8 worked example: 49°03.50'N, 072°01.75'W, house symbol.
        let info = AprsInfo::Position(match parse(b"!4903.50N/07201.75W-") {
            AprsInfo::Position(p) => p,
            other => panic!("expected position, got {other:?}"),
        });
        let AprsInfo::Position(p) = &info else {
            unreachable!()
        };
        assert!((p.lat - 49.0583333).abs() < 1e-6);
        assert!((p.lon - (-72.029166)).abs() < 1e-5);
        assert_eq!(p.symbol_table, '/');
        assert_eq!(p.symbol_code, '-');
        assert!(!p.messaging);
        assert_eq!(p.timestamp, None);
        assert_eq!(p.comment, "");
    }

    #[test]
    fn position_round_trips_byte_for_byte() {
        for s in [
            "!4903.50N/07201.75W-",
            "=4903.50N/07201.75W-Test with comment =",
            "@092345z4903.50N/07201.75W>Timestamped car",
            "/092345z4903.50N/07201.75W>",
            "!0000.00N/00000.00W.at the null island",
            "!5132.07S\\00007.40Woverlay+south",
        ] {
            let round = String::from_utf8(parse(s.as_bytes()).encode()).unwrap();
            assert_eq!(round, s, "position must round-trip");
        }
    }

    #[test]
    fn parses_and_round_trips_a_message() {
        let info = parse(b":N0CALL   :Hello, APRS{042");
        match &info {
            AprsInfo::Message(m) => {
                assert_eq!(m.addressee, "N0CALL");
                assert_eq!(m.text, "Hello, APRS");
                assert_eq!(m.id.as_deref(), Some("042"));
            }
            other => panic!("expected message, got {other:?}"),
        }
        assert_eq!(
            String::from_utf8(info.encode()).unwrap(),
            ":N0CALL   :Hello, APRS{042"
        );
    }

    #[test]
    fn message_without_a_line_number() {
        let info = parse(b":WIDE2-1  :ack");
        match &info {
            AprsInfo::Message(m) => {
                assert_eq!(m.addressee, "WIDE2-1");
                assert_eq!(m.text, "ack");
                assert_eq!(m.id, None);
            }
            other => panic!("expected message, got {other:?}"),
        }
        assert_eq!(String::from_utf8(info.encode()).unwrap(), ":WIDE2-1  :ack");
    }

    #[test]
    fn parses_status_with_and_without_timestamp() {
        match parse(b">123456zStation online") {
            AprsInfo::Status { timestamp, text } => {
                assert_eq!(timestamp.as_deref(), Some("123456z"));
                assert_eq!(text, "Station online");
            }
            other => panic!("expected status, got {other:?}"),
        }
        let plain = parse(b">Just a status");
        assert_eq!(
            plain,
            AprsInfo::Status {
                timestamp: None,
                text: "Just a status".into()
            }
        );
        assert_eq!(String::from_utf8(plain.encode()).unwrap(), ">Just a status");
    }

    #[test]
    fn unknown_type_is_preserved_verbatim() {
        let raw = b"T#005,199,000,255,073,123,01101001";
        let info = parse(raw);
        assert!(matches!(info, AprsInfo::Other { dti: 'T', .. }));
        assert_eq!(info.encode(), raw);
    }

    #[test]
    fn a_malformed_position_falls_back_to_other_not_a_panic() {
        // Too short to be a valid position → preserved as Other rather than mis-parsed.
        let info = parse(b"!nonsense");
        assert!(matches!(info, AprsInfo::Other { dti: '!', .. }));
        assert_eq!(info.encode(), b"!nonsense");
    }

    #[test]
    fn eight_bit_bytes_inside_the_fixed_position_layout_do_not_panic() {
        // Real trackers and igate feeds emit latin-1 — a degree sign (0xB0) is routine.
        // Lossy UTF-8 decoding expands each such byte to a 3-byte U+FFFD, so any
        // char-boundary arithmetic over the fixed 19-byte position layout panics.
        // One placement per fixed field: latitude, symbol table, longitude.
        for info in [
            b"!4903.5\xB0N/07201.75W-Nexus test".as_slice(),
            b"!4903.50\xB0/07201.75W-Nexus test",
            b"!4903.50N/07201.7\xB0W-Nexus test",
        ] {
            // A malformed position degrades to Other; it must never take the thread.
            assert!(matches!(parse(info), AprsInfo::Other { dti: '!', .. }));
        }
    }

    #[test]
    fn eight_bit_bytes_in_the_free_text_comment_still_yield_a_position() {
        // The comment is the one genuinely free-text field: 8-bit bytes there are
        // replaced (U+FFFD), not rejected — the coordinates still count.
        match parse(b"!4903.50N/07201.75W-25\xB0C and sunny") {
            AprsInfo::Position(p) => {
                assert!((p.lat - 49.0583333).abs() < 1e-6);
                assert_eq!(p.comment, "25\u{FFFD}C and sunny");
            }
            o => panic!("expected a position, got {o:?}"),
        }
        // Valid multi-byte UTF-8 in the comment survives verbatim.
        match parse("!4903.50N/07201.75W-caf\u{e9} \u{2615}".as_bytes()) {
            AprsInfo::Position(p) => assert_eq!(p.comment, "caf\u{e9} \u{2615}"),
            o => panic!("expected a position, got {o:?}"),
        }
    }

    #[test]
    fn eight_bit_bytes_in_message_object_and_status_bodies_still_parse() {
        // The byte-first rewrite's ACCEPTANCE contract for the other fixed-layout
        // regions: an 8-bit byte inside a 9-byte addressee/object-name region (or
        // a position timestamp) is replaced and the packet still parses to its
        // real type — where the old char-boundary guards rejected the whole
        // packet to Other. Real igate feeds carry latin-1 in exactly these spots.
        match parse(b":N0CALL\xB0  :hello") {
            AprsInfo::Message(m) => assert_eq!(m.text, "hello"),
            o => panic!("expected a message, got {o:?}"),
        }
        assert!(matches!(
            parse(b";OBJ\xB0     *092345z4903.50N/07201.75W-x"),
            AprsInfo::Object { .. }
        ));
        assert!(matches!(
            parse(b">\xB0 status with 8-bit noise"),
            AprsInfo::Status { .. }
        ));
        // A degree sign inside the 7-byte timestamp: the coordinates still count.
        match parse(b"@09234\xB0z4903.50N/07201.75W>car") {
            AprsInfo::Position(p) => assert!((p.lat - 49.0583333).abs() < 1e-6),
            o => panic!("expected a position, got {o:?}"),
        }
    }

    fn b91(mut v: u32) -> [u8; 4] {
        let mut o = [0u8; 4];
        for i in (0..4).rev() {
            o[i] = (v % 91) as u8 + 33;
            v /= 91;
        }
        o
    }

    #[test]
    fn decodes_a_compressed_position() {
        let (lat, lon) = (49.05833_f64, -72.02917_f64);
        let mut info = vec![b'!', b'/'];
        info.extend_from_slice(&b91(((90.0 - lat) * 380_926.0).round() as u32));
        info.extend_from_slice(&b91(((lon + 180.0) * 190_463.0).round() as u32));
        info.push(b'>'); // symbol code
        info.extend_from_slice(b"  A"); // cs + compression type (ignored)
        info.extend_from_slice(b"compressed");
        match parse(&info) {
            AprsInfo::Position(p) => {
                assert!((p.lat - lat).abs() < 1e-4, "lat {}", p.lat);
                assert!((p.lon - lon).abs() < 1e-4, "lon {}", p.lon);
                assert_eq!(p.symbol_table, '/');
                assert_eq!(p.symbol_code, '>');
                assert_eq!(p.comment, "compressed");
            }
            o => panic!("expected a compressed position, got {o:?}"),
        }
    }

    #[test]
    fn decodes_an_object_report() {
        match parse(b";LEIXLIGHT*092345z4903.50N/07201.75W-object here") {
            AprsInfo::Object {
                name,
                killed,
                position,
            } => {
                assert_eq!(name, "LEIXLIGHT");
                assert!(!killed);
                assert!((position.lat - 49.0583333).abs() < 1e-6);
                assert!((position.lon - (-72.029166)).abs() < 1e-5);
                assert_eq!(position.comment, "object here");
            }
            o => panic!("expected an object, got {o:?}"),
        }
        // A killed object.
        match parse(b";EVENT    _092345z4903.50N/07201.75W-") {
            AprsInfo::Object { killed, name, .. } => {
                assert!(killed);
                assert_eq!(name, "EVENT");
            }
            o => panic!("expected a killed object, got {o:?}"),
        }
    }

    #[test]
    fn format_dm_carries_a_rounding_boundary_into_degrees() {
        // 48.99999° must not emit "48 60.00"; it carries to 49°00.00'.
        assert_eq!(format_dm(48.999999, 2, 'N', 'S'), "4900.00N");
    }
}
