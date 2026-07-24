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
    Status { timestamp: Option<String>, text: String },
    Message(Message),
    /// Any type this parser doesn't decode — the DTI plus the raw remainder, preserved.
    Other { dti: char, body: String },
}

fn parse_lat(s: &str) -> Option<f64> {
    // "DDMM.hhN" — 8 ASCII chars.
    if s.len() != 8 || !s.is_ascii() {
        return None;
    }
    let deg: f64 = s[0..2].parse().ok()?;
    let min: f64 = s[2..7].parse().ok()?; // "MM.hh"
    if !(0.0..60.0).contains(&min) || deg > 90.0 {
        return None;
    }
    let mag = deg + min / 60.0;
    match s.as_bytes()[7] {
        b'N' => Some(mag),
        b'S' => Some(-mag),
        _ => None,
    }
}

fn parse_lon(s: &str) -> Option<f64> {
    // "DDDMM.hhW" — 9 ASCII chars.
    if s.len() != 9 || !s.is_ascii() {
        return None;
    }
    let deg: f64 = s[0..3].parse().ok()?;
    let min: f64 = s[3..8].parse().ok()?;
    if !(0.0..60.0).contains(&min) || deg > 180.0 {
        return None;
    }
    let mag = deg + min / 60.0;
    match s.as_bytes()[8] {
        b'E' => Some(mag),
        b'W' => Some(-mag),
        _ => None,
    }
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
    format!("{:0deg_width$}{:02}.{:02}{}", deg, rem / 100, rem % 100, hemi, deg_width = deg_width)
}

impl Position {
    fn parse(body: &str, messaging: bool, has_ts: bool) -> Option<Position> {
        let (timestamp, rest) = if has_ts {
            if body.len() < 7 || !body.is_char_boundary(7) {
                return None;
            }
            (Some(body[..7].to_string()), &body[7..])
        } else {
            (None, body)
        };
        // lat(8) + symtable(1) + lon(9) + symcode(1) = 19 fixed chars, then the comment.
        if rest.len() < 19 || !rest.is_char_boundary(19) {
            return None;
        }
        let lat = parse_lat(&rest[0..8])?;
        let symbol_table = rest.as_bytes()[8] as char;
        let lon = parse_lon(&rest[9..18])?;
        let symbol_code = rest.as_bytes()[18] as char;
        Some(Position {
            lat,
            lon,
            symbol_table,
            symbol_code,
            timestamp,
            messaging,
            comment: rest[19..].to_string(),
        })
    }
}

/// Parse an APRS information field into structured data (never fails — unknown types become
/// [`AprsInfo::Other`]).
pub fn parse(info: &[u8]) -> AprsInfo {
    let Some(&dti_byte) = info.first() else {
        return AprsInfo::Other { dti: '\0', body: String::new() };
    };
    let s = String::from_utf8_lossy(info);
    let dti = dti_byte as char;
    // DTI is ASCII → byte 1 is a char boundary.
    let body = if dti_byte.is_ascii() { &s[1..] } else { &s[..] };

    let parsed = match dti {
        '!' => Position::parse(body, false, false).map(AprsInfo::Position),
        '=' => Position::parse(body, true, false).map(AprsInfo::Position),
        '/' => Position::parse(body, false, true).map(AprsInfo::Position),
        '@' => Position::parse(body, true, true).map(AprsInfo::Position),
        ':' => parse_message(body),
        '>' => Some(parse_status(body)),
        _ => None,
    };
    parsed.unwrap_or(AprsInfo::Other { dti, body: body.to_string() })
}

fn parse_message(body: &str) -> Option<AprsInfo> {
    // ":<addressee: 9 chars>:<text>[{id]"
    if body.len() < 10 || !body.is_char_boundary(9) || body.as_bytes()[9] != b':' {
        return None;
    }
    let addressee = body[..9].trim_end().to_string();
    let payload = &body[10..];
    let (text, id) = match payload.rsplit_once('{') {
        Some((t, i)) if !i.is_empty() && i.chars().all(|c| c.is_ascii_alphanumeric()) => {
            (t.to_string(), Some(i.to_string()))
        }
        _ => (payload.to_string(), None),
    };
    Some(AprsInfo::Message(Message { addressee, text, id }))
}

fn parse_status(body: &str) -> AprsInfo {
    // A status may open with a zulu day/hour/min timestamp "DDHHMMz".
    if body.len() >= 7 && body.is_char_boundary(7) && body.as_bytes()[6] == b'z' && body[..6].bytes().all(|b| b.is_ascii_digit())
    {
        AprsInfo::Status { timestamp: Some(body[..7].to_string()), text: body[7..].to_string() }
    } else {
        AprsInfo::Status { timestamp: None, text: body.to_string() }
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
        let AprsInfo::Position(p) = &info else { unreachable!() };
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
        assert_eq!(String::from_utf8(info.encode()).unwrap(), ":N0CALL   :Hello, APRS{042");
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
        assert_eq!(plain, AprsInfo::Status { timestamp: None, text: "Just a status".into() });
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
    fn format_dm_carries_a_rounding_boundary_into_degrees() {
        // 48.99999° must not emit "48 60.00"; it carries to 49°00.00'.
        assert_eq!(format_dm(48.999999, 2, 'N', 'S'), "4900.00N");
    }
}
