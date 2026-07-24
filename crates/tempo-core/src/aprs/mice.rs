//! Mic-E decoding — the compact position format most 2 m RF trackers/radios transmit.
//!
//! Mic-E is unusual: it splits a position across BOTH AX.25 fields. The **destination address**
//! (the "TOCALL") carries the six latitude digits plus the message code, N/S sign, longitude
//! ≥100° offset, and W/E sign; the **information field** carries longitude, speed, course, and the
//! symbol. This decoder therefore needs the frame's destination callsign AND its info field.
//!
//! The algorithm follows the reference implementation in `aprslib` (rossengeorgiev/aprs-python),
//! constant-for-constant. Conformance is pinned by a hand-worked golden packet (every byte derived
//! from those formulas) plus hemisphere/edge cases — not a self-referential encode→decode loop.

/// Standard message labels, indexed by the 3-bit code read MSB-first from destination chars 0..2
/// (a `1` bit = a `P..Z` char). Std code `000` is the emergency signal.
const MSG_STD: [&str; 8] = [
    "Emergency",
    "M6: Priority",
    "M5: Special",
    "M4: Committed",
    "M3: Returning",
    "M2: In Service",
    "M1: En Route",
    "M0: Off Duty",
];
/// Custom message labels (destination chars in `A..K` set the "custom" bit).
const MSG_CUSTOM: [&str; 8] = [
    "Custom-7 (Emergency)",
    "Custom-6",
    "Custom-5",
    "Custom-4",
    "Custom-3",
    "Custom-2",
    "Custom-1",
    "Custom-0",
];

/// A decoded Mic-E report. Latitude is +north, longitude is +east, both degrees.
#[derive(Debug, Clone, PartialEq)]
pub struct MicE {
    pub lat: f64,
    pub lon: f64,
    /// Speed over ground in knots.
    pub speed_knots: u16,
    /// Course over ground in degrees (0..=359, 0 also = unknown per spec).
    pub course_deg: u16,
    pub symbol_table: char,
    pub symbol_code: char,
    /// Human label for the 3-bit Mic-E message code (e.g. "M0: Off Duty").
    pub message: &'static str,
    /// Trailing free text (comment / altitude / telemetry) — preserved raw.
    pub comment: String,
}

/// True if `info` is a Mic-E information field by its data-type identifier
/// (`` ` `` current, `'` old, or the `0x1c`/`0x1d` legacy forms).
pub fn is_mic_e(info: &[u8]) -> bool {
    matches!(info.first(), Some(0x60 | 0x27 | 0x1c | 0x1d))
}

/// One destination character → its latitude digit (0..9), or `None` for the ambiguity chars
/// `K`/`L`/`Z` (a blanked digit).
fn dest_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'A'..=b'J' => Some(c - b'A'),
        b'P'..=b'Y' => Some(c - b'P'),
        b'K' | b'L' | b'Z' => None,
        _ => None,
    }
}

/// One destination char (of chars 0..2) → its message contribution: 0 (`0..9`/`L`), 1 (`P..Z`,
/// standard), or 2 (`A..K`, custom).
fn msg_bit(c: u8) -> u8 {
    match c {
        b'0'..=b'9' | b'L' => 0,
        b'P'..=b'Z' => 1,
        b'A'..=b'K' => 2,
        _ => 0,
    }
}

/// Decode a Mic-E packet from its AX.25 destination callsign (6 encoded chars) and information
/// field. `None` if the destination isn't 6 usable chars or the info field is too short.
pub fn decode(dest_call: &str, info: &[u8]) -> Option<MicE> {
    let dc = dest_call.as_bytes();
    if dc.len() < 6 {
        return None;
    }
    // --- Latitude + flags from the destination address ---
    let mut digits = [0u8; 6];
    for i in 0..6 {
        digits[i] = dest_digit(dc[i]).unwrap_or(0); // ambiguity → 0 (precision loss, per spec)
    }
    let lat_deg = digits[0] as f64 * 10.0 + digits[1] as f64;
    let lat_min =
        digits[2] as f64 * 10.0 + digits[3] as f64 + digits[4] as f64 / 10.0 + digits[5] as f64 / 100.0;
    let mut lat = lat_deg + lat_min / 60.0;
    if dc[3] <= 0x4c {
        lat = -lat; // char 3 ≤ 'L' → South
    }

    // Message code from chars 0..2: any '2' means custom (normalize 2→1 for the index).
    let bits = [msg_bit(dc[0]), msg_bit(dc[1]), msg_bit(dc[2])];
    let custom = bits.contains(&2);
    let idx = bits.iter().fold(0usize, |acc, &b| (acc << 1) | usize::from(b != 0));
    let message = if custom { MSG_CUSTOM[idx] } else { MSG_STD[idx] };

    // --- Longitude, speed, course, symbol from the information field ---
    // info[0] is the DTI; the 8 data bytes are info[1..9], symbol at info[7..9].
    if info.len() < 9 {
        return None;
    }
    let d = &info[1..];
    let lon_offset = dc[4] >= 0x50; // char 4 ≥ 'P' → +100°
    let mut lon_deg = d[0] as i32 - 28;
    if lon_offset {
        lon_deg += 100;
    }
    if (180..=189).contains(&lon_deg) {
        lon_deg -= 80;
    } else if (190..=199).contains(&lon_deg) {
        lon_deg -= 190;
    }
    let mut lon_min = d[1] as f64 - 28.0;
    if lon_min >= 60.0 {
        lon_min -= 60.0;
    }
    lon_min += (d[2] as f64 - 28.0) / 100.0;
    let mut lon = lon_deg as f64 + lon_min / 60.0;
    if dc[5] >= 0x50 {
        lon = -lon; // char 5 ≥ 'P' → West
    }

    let mut speed = (d[3] as i32 - 28) * 10;
    let mut course = d[4] as i32 - 28;
    let q = course / 10;
    course -= q * 10;
    course = course * 100 + (d[5] as i32 - 28);
    speed += q;
    if speed >= 800 {
        speed -= 800;
    }
    if course >= 400 {
        course -= 400;
    }

    Some(MicE {
        lat,
        lon,
        speed_knots: speed.clamp(0, 799) as u16,
        course_deg: course.clamp(0, 399) as u16,
        symbol_code: d[6] as char,
        symbol_table: d[7] as char,
        message,
        comment: String::from_utf8_lossy(&d[8..]).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_mic_e_data_type_identifiers() {
        assert!(is_mic_e(b"`abc"));
        assert!(is_mic_e(b"'abc"));
        assert!(is_mic_e(&[0x1c, 0x00]));
        assert!(!is_mic_e(b"!4903.50N"));
        assert!(!is_mic_e(b""));
    }

    #[test]
    fn decodes_the_hand_worked_golden_packet() {
        // Constructed byte-by-byte from the aprslib formulas:
        //   dest "SSRUVT": digits 3,3,2,5,6,4 → 33°25.64'; char3 'U'(85)>0x4c → North;
        //     char4 'V'(86)≥'P' → lon +100° offset; char5 'T'(84)≥'P' → West; chars S,S,R all
        //     'P'..'Z' → message "111" → M0: Off Duty.
        //   info `(#H<1e><1e>O>/ :
        //     lon deg '('(40)-28=12 +100 = 112; min '#'(35)-28=7; hundredths 'H'(72)-28=44 → 7.44'
        //       → 112°07.44' West = -112.124°.
        //     speed: 0x1e(30)-28=2 → 20 kt; course: 0x1e→2, 'O'(79)-28=51 → 2*100+51 = 251°.
        //     symbol code '>' table '/'.
        let dest = "SSRUVT";
        let info = [0x60, b'(', b'#', b'H', 0x1e, 0x1e, b'O', b'>', b'/'];
        let m = decode(dest, &info).expect("valid Mic-E");
        assert!((m.lat - 33.427333).abs() < 1e-5, "lat {}", m.lat);
        assert!((m.lon - (-112.124)).abs() < 1e-3, "lon {}", m.lon);
        assert_eq!(m.speed_knots, 20);
        assert_eq!(m.course_deg, 251);
        assert_eq!(m.symbol_code, '>');
        assert_eq!(m.symbol_table, '/');
        assert_eq!(m.message, "M0: Off Duty");
        assert_eq!(m.comment, "");
    }

    #[test]
    fn decodes_a_south_east_report_with_no_longitude_offset() {
        // dest "123456": digits 1..6 → 12°34.56'; char3 '4'(52)≤0x4c → South; char4 '5'(53)<'P'
        //   → no offset; char5 '6'(54)<'P' → East.
        // info: lon deg 'I'(73)-28=45; min '"'(34)-28=6; hundredths 'j'(106)-28=78 → 6.78'
        //   → 45°06.78' East = +45.113°. speed/course bytes all 0x1c(28) → 0/0.
        let m = decode("123456", &[0x60, b'I', b'"', b'j', 0x1c, 0x1c, 0x1c, b'>', b'/']).unwrap();
        assert!((m.lat - (-12.576)).abs() < 1e-3, "lat {}", m.lat);
        assert!((m.lon - 45.113).abs() < 1e-3, "lon {}", m.lon);
        assert_eq!(m.speed_knots, 0);
        assert_eq!(m.course_deg, 0);
    }

    #[test]
    fn preserves_a_trailing_comment() {
        let mut info = vec![0x60, b'(', b'#', b'H', 0x1e, 0x1e, b'O', b'>', b'/'];
        info.extend_from_slice(b"Nexus RF");
        let m = decode("SSRUVT", &info).unwrap();
        assert_eq!(m.comment, "Nexus RF");
    }

    #[test]
    fn a_short_info_field_is_rejected() {
        assert!(decode("SSRUVT", &[0x60, b'(', b'#']).is_none());
        assert!(decode("SSR", &[0x60, b'(', b'#', b'H', 0x1e, 0x1e, b'O', b'>', b'/']).is_none());
    }
}
