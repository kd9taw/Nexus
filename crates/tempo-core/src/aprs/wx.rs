//! APRS weather reports — the common core of the WX data format.
//!
//! A known parse gap: weather stations were decoding as ordinary positions with their readings
//! sitting unparsed in the comment field, so a station reporting 77 °F and a 20 mph gust showed the
//! operator `_220/004g005t077r000p000P000h50b09900` and left them to read it.
//!
//! Two shapes carry the same field syntax (APRS 1.0.1 ch. 12):
//!
//! * **Positionless** — DTI `_`, then an 8-digit `MDHM` timestamp, then the fields:
//!   `_10090556c220s004g005t077r000p000P000h50b09900`
//! * **Position + weather** — an ordinary position report whose symbol is a weather one and whose
//!   comment opens with the wind course/speed slot, then the same fields:
//!   `!4903.50N/07201.75W_220/004g005t077r000p000P000h50b09900`
//!
//! In the position form the leading `ddd/sss` is **wind direction and speed**, not course and speed
//! — the same three-digit slot an ordinary moving station uses for its heading. Reading it as a
//! course is how a weather station ends up drawn with a speed vector.
//!
//! Scope is deliberately the common core an operator actually reads: temperature, wind, rain,
//! humidity, barometer. Luminosity, snowfall, raw telemetry and the DAO precision extension are
//! left in the comment rather than half-parsed.

/// A decoded APRS weather report. Every field is optional because real stations omit what they do
/// not have, and the spec's own placeholder for "no data" is a run of `.` or ` ` in the digits.
/// Plain data, no serde: tempo-core carries no serialization dependency, and turning core types
/// into wire DTOs is tempo-app's job (see `AprsWxDto`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AprsWx {
    /// Wind direction, degrees true (0..=360).
    pub wind_dir_deg: Option<u16>,
    /// Sustained wind speed, mph.
    pub wind_mph: Option<u16>,
    /// Peak gust in the last 5 minutes, mph.
    pub gust_mph: Option<u16>,
    /// Air temperature, °F. Signed: the spec allows `t-05` for -5 °F.
    pub temp_f: Option<i16>,
    /// Rainfall in the last hour, hundredths of an inch.
    pub rain_1h_in100: Option<u16>,
    /// Rainfall in the last 24 hours, hundredths of an inch.
    pub rain_24h_in100: Option<u16>,
    /// Rainfall since local midnight, hundredths of an inch.
    pub rain_midnight_in100: Option<u16>,
    /// Relative humidity, percent. The wire encodes 100% as `00`; this is the real percentage.
    pub humidity_pct: Option<u8>,
    /// Barometric pressure, tenths of a hectopascal (millibar) — `b09900` is 990.0 hPa.
    pub pressure_tenth_hpa: Option<u32>,
}

impl AprsWx {
    /// Did anything at all parse? A comment that merely happened to contain a `t` should not
    /// present as a weather report.
    pub fn is_empty(&self) -> bool {
        *self == AprsWx::default()
    }

    /// Barometric pressure in hectopascals, for display.
    pub fn pressure_hpa(&self) -> Option<f32> {
        self.pressure_tenth_hpa.map(|p| p as f32 / 10.0)
    }

    /// Rainfall in inches, for display.
    pub fn rain_1h_inches(&self) -> Option<f32> {
        self.rain_1h_in100.map(|r| f32::from(r) / 100.0)
    }
}

/// Read `width` digits at `i`, or `None` when the field is a "no data" placeholder.
///
/// Stations pad missing readings with `.`, ` ` or sometimes `-`, so `t...` means "no thermometer",
/// NOT zero degrees. Treating those as 0 would report a freezing day in July.
fn digits(b: &[u8], i: usize, width: usize) -> Option<u32> {
    let s = b.get(i..i + width)?;
    if !s.iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    std::str::from_utf8(s).ok()?.parse().ok()
}

/// Signed variant for temperature, which may be `t-05`.
fn signed(b: &[u8], i: usize) -> Option<i16> {
    let s = b.get(i..i + 3)?;
    if s[0] == b'-' {
        let v = digits(b, i + 1, 2)?;
        return Some(-(v as i16));
    }
    Some(digits(b, i, 3)? as i16)
}

/// Parse the weather field sequence — the part shared by both report shapes.
///
/// `s` starts at the first field letter (`c`, `t`, `g`, …). Unknown letters are skipped rather than
/// aborting the parse, because real stations append software tags (`wRSW`, `.DsVP`, `eCumulusFO`)
/// straight after the readings.
pub fn parse_fields(s: &str) -> AprsWx {
    let b = s.as_bytes();
    let mut wx = AprsWx::default();
    let mut i = 0;
    while i < b.len() {
        let tag = b[i];
        let at = i + 1;
        match tag {
            b'c' => {
                // Direction 000 and 360 both mean north on the wire.
                wx.wind_dir_deg = digits(b, at, 3).map(|v| (v % 360) as u16);
                i = at + 3;
            }
            b's' => {
                wx.wind_mph = digits(b, at, 3).map(|v| v as u16);
                i = at + 3;
            }
            b'g' => {
                wx.gust_mph = digits(b, at, 3).map(|v| v as u16);
                i = at + 3;
            }
            b't' => {
                wx.temp_f = signed(b, at);
                i = at + 3;
            }
            b'r' => {
                wx.rain_1h_in100 = digits(b, at, 3).map(|v| v as u16);
                i = at + 3;
            }
            b'p' => {
                wx.rain_24h_in100 = digits(b, at, 3).map(|v| v as u16);
                i = at + 3;
            }
            b'P' => {
                wx.rain_midnight_in100 = digits(b, at, 3).map(|v| v as u16);
                i = at + 3;
            }
            b'h' => {
                // `h00` is 100%, not 0% — the field is two digits and 100 does not fit.
                wx.humidity_pct = digits(b, at, 2).map(|v| if v == 0 { 100 } else { v as u8 });
                i = at + 2;
            }
            b'b' => {
                wx.pressure_tenth_hpa = digits(b, at, 5);
                i = at + 5;
            }
            // A software tag or anything else: step one byte and keep looking. The readings always
            // come first, so this only ever skips trailing junk.
            _ => i += 1,
        }
    }
    wx
}

/// Parse a **positionless** weather report body — the bytes after the `_` DTI.
///
/// `_MDHMcdddsdddgdddtddd…`: an 8-digit month/day/hour/minute stamp, then the fields. `None` if the
/// timestamp is missing or nothing parsed.
pub fn parse_positionless(body: &str) -> Option<AprsWx> {
    if body.len() < 8 || !body.as_bytes()[..8].iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let wx = parse_fields(&body[8..]);
    (!wx.is_empty()).then_some(wx)
}

/// Parse the weather readings out of a **position report's comment**.
///
/// The comment opens `ddd/sss` — wind direction and speed, occupying the same slot an ordinary
/// moving station uses for course and speed — followed by the field sequence. `None` when the
/// comment is not a weather comment, so an ordinary station's free text is never mined for
/// accidental readings.
pub fn parse_position_comment(comment: &str) -> Option<AprsWx> {
    let b = comment.as_bytes();
    // Require the exact `ddd/sss` shape up front. Without this gate a comment like
    // "gone to lunch" would yield a 0 mph gust.
    if b.len() < 7 || b[3] != b'/' {
        return None;
    }
    let dir = digits(b, 0, 3);
    let speed = digits(b, 4, 3);
    if dir.is_none() && speed.is_none() {
        return None;
    }
    // ⭐ The wind slot ALONE cannot identify a weather report, because it is byte-for-byte the same
    // slot a moving station uses for course and speed: `088/036` is a car on a heading of 88 doing
    // 36 knots. So require at least one field that only a weather station sends — temperature,
    // rain, humidity, barometer, gust. Without this every mobile on the map becomes a weather
    // station reporting a mysterious wind.
    // ⚠️ `comment[7..]` PANICS on a multi-byte character inside the wind slot. The slot is
    // `ddd/sss` — seven ASCII bytes by the spec — but a tracker is free to send anything, and
    // one in the wild put a 3-byte thin space at bytes 6..9 (`248/04\u{2009}) 000059Vin:…`),
    // which made byte 7 land mid-character and took the thread down. The direction had parsed
    // cleanly, so the shape gate above let it through. `get` returns None on a non-boundary,
    // which is exactly the right answer: a slot that is not seven ASCII bytes is not a weather
    // comment, and an ordinary station's free text must never be mined for readings anyway.
    // (Found in an operator's diagnostic log, 2026-08-19 — the log's first catch.)
    let rest = comment.get(7..)?;
    let mut wx = parse_fields(rest);
    if wx.is_empty() {
        return None;
    }
    wx.wind_dir_deg = dir.map(|v| (v % 360) as u16);
    wx.wind_mph = speed.map(|v| v as u16);
    Some(wx)
}

#[cfg(test)]
mod tests {

    /// FIELD CRASH 2026-08-19, found in an operator's diagnostic log — the log's first catch.
    ///
    /// `byte index 7 is not a char boundary; it is inside ' ' (bytes 6..9) of
    /// `248/04 ) 000059Vin:15.21V.SAT:29Topspeed:77kmh DX:2km``
    ///
    /// The wind slot is `ddd/sss` — seven ASCII bytes by the APRS spec — and the parser sliced
    /// `comment[7..]` on that assumption. A tracker sending a MULTI-BYTE character inside the
    /// slot (this one puts a 3-byte space at bytes 6..9) makes byte 7 land mid-character, and
    /// Rust panics rather than truncating. The direction parsed fine, which is why the earlier
    /// guard let it through.
    #[test]
    fn a_multibyte_char_in_the_wind_slot_is_refused_not_a_panic() {
        // The exact comment from the operator's log.
        let real = "248/04\u{2009}) 000059Vin:15.21V.SAT:29Topspeed:77kmh DX:2km";
        assert_eq!(
            parse_position_comment(real),
            None,
            "malformed slot ⇒ not a weather report"
        );

        // The control: the SAME packet with an ASCII space still parses, so the fix refuses the
        // malformed shape rather than refusing everything.
        let ascii = "248/004t072r000p000h50b10130";
        assert!(
            parse_position_comment(ascii).is_some(),
            "control: a well-formed weather comment still parses"
        );
    }
    use super::*;

    // Every fixture is a real captured APRS-IS weather packet's payload (javAPRSlib's aprs.txt and
    // cwop.txt captures, and the Ham::APRS::FAP test vectors).

    #[test]
    fn parses_a_real_position_weather_comment() {
        // From `SR9WXL>AKLPRZ,WIDE2-1,qAR,SR9NSK:!4947.70N/01926.80E_310/000g002t056r...p...P...b09174h75`
        let wx = parse_position_comment("310/000g002t056r...p...P...b09174h75").unwrap();
        assert_eq!(wx.wind_dir_deg, Some(310));
        assert_eq!(wx.wind_mph, Some(0));
        assert_eq!(wx.gust_mph, Some(2));
        assert_eq!(wx.temp_f, Some(56));
        assert_eq!(wx.pressure_tenth_hpa, Some(9174));
        assert_eq!(wx.humidity_pct, Some(75));
        // ⭐ `r...` is "no rain GAUGE", not "no rain". Reporting 0.00 in would invent a measurement.
        assert_eq!(wx.rain_1h_in100, None);
        assert_eq!(wx.rain_24h_in100, None);
        assert_eq!(wx.pressure_hpa(), Some(917.4));
    }

    #[test]
    fn parses_a_real_cwop_station() {
        // From `WB8HRV>APRS,TCPXX*,qAX,CWOP-4:@291813z3913.47N/08424.67W_220/004g011t085r000p000P000h68b10156.DsVP`
        let wx = parse_position_comment("220/004g011t085r000p000P000h68b10156.DsVP").unwrap();
        assert_eq!(wx.wind_dir_deg, Some(220));
        assert_eq!(wx.wind_mph, Some(4));
        assert_eq!(wx.gust_mph, Some(11));
        assert_eq!(wx.temp_f, Some(85));
        assert_eq!(
            wx.rain_1h_in100,
            Some(0),
            "this one HAS a gauge, reading zero"
        );
        assert_eq!(wx.humidity_pct, Some(68));
        assert_eq!(wx.pressure_hpa(), Some(1015.6));
        // The trailing `.DsVP` software tag must not derail the parse.
    }

    #[test]
    fn a_trailing_software_tag_never_derails_the_readings() {
        // Real tags seen on the air: `.DsVP`, `eCumulusFO`, `wRSW`, `eMB44`, `{UIV32N}`.
        for tail in [".DsVP", "eCumulusFO", "wRSW", "eMB44", "{UIV32N}"] {
            let wx = parse_position_comment(&format!("240/001g002t077r000p071P001h67b10135{tail}"))
                .unwrap();
            assert_eq!(wx.temp_f, Some(77), "tail {tail}");
            assert_eq!(wx.pressure_tenth_hpa, Some(10135), "tail {tail}");
        }
    }

    #[test]
    fn parses_a_positionless_report() {
        // From `KN4CI-1>APTW01,...:_08221205c117s000g000t081r000p000P000h47b10206tRSW`
        let wx = parse_positionless("08221205c117s000g000t081r000p000P000h47b10206").unwrap();
        assert_eq!(wx.wind_dir_deg, Some(117));
        assert_eq!(wx.wind_mph, Some(0));
        assert_eq!(wx.temp_f, Some(81));
        assert_eq!(wx.humidity_pct, Some(47));
        assert_eq!(wx.pressure_tenth_hpa, Some(10206));
    }

    #[test]
    fn a_positionless_report_needs_its_timestamp() {
        assert!(parse_positionless("c117s000t081").is_none());
        assert!(parse_positionless("").is_none());
    }

    #[test]
    fn humidity_of_00_means_one_hundred_percent() {
        // The field is two digits, so 100 cannot be written; the spec encodes it as 00. Reporting
        // 0% relative humidity would be a physically absurd reading in a saturated fog.
        assert_eq!(
            parse_position_comment("000/000t050h00")
                .unwrap()
                .humidity_pct,
            Some(100)
        );
        assert_eq!(
            parse_position_comment("000/000t050h99")
                .unwrap()
                .humidity_pct,
            Some(99)
        );
    }

    #[test]
    fn a_negative_temperature_parses() {
        // `t-05` is -5 °F. Reading it unsigned would report a summer day mid-blizzard.
        assert_eq!(
            parse_position_comment("000/000t-05h50").unwrap().temp_f,
            Some(-5)
        );
    }

    #[test]
    fn wind_direction_360_normalises_to_north() {
        assert_eq!(
            parse_position_comment("360/005t050").unwrap().wind_dir_deg,
            Some(0)
        );
        assert_eq!(
            parse_position_comment("000/005t050").unwrap().wind_dir_deg,
            Some(0)
        );
    }

    #[test]
    fn an_ordinary_comment_is_never_mined_for_accidental_readings() {
        // ⭐ The gate that matters. Without the `ddd/sss` requirement, ordinary free text — which is
        // full of letters this parser cares about — would yield phantom weather.
        for comment in [
            "Home of KA0RID",
            "gone to lunch, back at 3",
            "146.520MHz UHF/VHF/HF Mobile Truck Driver",
            "",
            "/A=001244Lora Tracker Rudi Batt=4.10V",
            "279/015/A=001041 APRS via LoRa", // a real course/speed + altitude, NOT weather
        ] {
            let got = parse_position_comment(comment);
            assert!(
                got.is_none() || got.unwrap().temp_f.is_none(),
                "must not read weather out of {comment:?}"
            );
        }
    }

    #[test]
    fn a_moving_station_course_speed_is_not_read_as_weather() {
        // ⭐ `088/036` is a car doing 36 knots on a heading of 88 — byte-for-byte the same slot a
        // weather station uses for wind. Only a field no mobile sends (temperature, rain, humidity,
        // barometer, gust) makes it a weather report. Getting this wrong turns every mobile on the
        // map into a weather station.
        assert!(parse_position_comment("088/036").is_none());
        assert!(parse_position_comment("279/015/A=001041 APRS via LoRa").is_none());
        assert!(parse_position_comment("098/065/146.520MHz/A=000829 Blue Dodge GC").is_none());
        // ...but the same slot followed by a real reading IS weather.
        assert!(parse_position_comment("088/036t072").is_some());
    }

    #[test]
    fn an_empty_report_is_none_rather_than_a_hollow_reading() {
        assert!(parse_fields("").is_empty());
        assert!(parse_position_comment("000/000").is_none());
    }
}
