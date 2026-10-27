//! Pure callook.info JSON callsign-lookup helpers — the offline, unit-testable core of the
//! third callbook, after [`crate::qrz`] and [`crate::hamqth`]. No I/O.
//!
//! callook.info serves the FCC's licence database (ULS) for **United States calls only**, as
//! JSON, with **no account and no key**: `https://callook.info/<CALL>/json`. Its API reference
//! (`https://callook.info/api_reference.php`, read 2026-10-24) says: *"The callook.info API is
//! publicly available and is free to use however you wish."* It names no rate limit; it asks
//! software that uses it to be listed, and for donations, and promises no uptime (it does not
//! answer for a few minutes during its daily update at 11:00 ET). So it is asked last, only for
//! a call that is shaped like a US call ([`is_us_call`]), and only when the operator leaves it on.
//!
//! What it answers: the licensee's name, the mailing address, and a grid and coordinates that
//! callook computes from that address. No zones, county, e-mail or QSL route — those stay
//! `None`. Never a credential either way: the request carries only the call.

/// What a callook.info answer gives a lookup.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CallookLookup {
    pub call: String,
    pub name: Option<String>,
    /// The city of the licence address.
    pub qth: Option<String>,
    /// The two-letter state of the licence address.
    pub state: Option<String>,
    pub grid: Option<String>,
    /// Both or neither; geocoded from the licence address.
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

/// The lookup URL for `call`. The call is upper-cased and kept to letters and digits, so it
/// cannot add a path segment or a query.
pub fn build_lookup_url(call: &str) -> String {
    let call: String = call
        .trim()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    format!("https://callook.info/{call}/json")
}

/// Whether `call` is shaped like a United States call: a `K`, `N` or `W` prefix or `AA`–`AL`,
/// a digit, and a letter suffix, with no stroke. Only these are sent to callook.info, which
/// holds nothing else.
pub fn is_us_call(call: &str) -> bool {
    let c = call.trim().to_ascii_uppercase();
    if c.len() < 3 || !c.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return false;
    }
    let b = c.as_bytes();
    let prefix_ok =
        matches!(b[0], b'K' | b'N' | b'W') || (b[0] == b'A' && (b'A'..=b'L').contains(&b[1]));
    let Some(digit) = c.find(|ch: char| ch.is_ascii_digit()) else {
        return false;
    };
    let suffix = &c[digit + 1..];
    prefix_ok
        && digit <= 2
        && !suffix.is_empty()
        && suffix.chars().all(|ch| ch.is_ascii_alphabetic())
}

/// Whether `json` is callook.info answering: a `VALID` record or an `INVALID` (unknown) call.
/// Anything else — an error page, a body that is not JSON, another status — is callook.info not
/// answering, as in its few minutes offline each day around 11:00 AM US Eastern: try later, and
/// never read it as "not in the callbook".
pub fn answered(json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| v.get("status")?.as_str().map(str::to_string))
        .is_some_and(|s| s == "VALID" || s == "INVALID")
}

/// Parse a callook.info JSON answer. `None` unless it is a `VALID` record with a call:
/// an unknown call answers `{"status":"INVALID"}` and nothing else.
pub fn parse_callsign(json: &str) -> Option<CallookLookup> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("status")?.as_str()? != "VALID" {
        return None;
    }
    let text = |p: &[&str]| -> Option<String> {
        let mut cur = &v;
        for k in p {
            cur = cur.get(k)?;
        }
        Some(cur.as_str()?.trim().to_string()).filter(|s| !s.is_empty())
    };
    let call = text(&["current", "callsign"])?;
    // An individual's licence reads "LAST, FIRST M"; a club's is its plain name.
    let name = text(&["name"]).map(|n| match n.split_once(',') {
        Some((last, first)) if !first.trim().is_empty() => {
            format!("{} {}", first.trim(), last.trim())
        }
        _ => n,
    });
    // "NEWINGTON, CT 06111": the city, then the state and the ZIP.
    let (qth, state) = match text(&["address", "line2"])
        .as_deref()
        .and_then(|l| l.rsplit_once(','))
    {
        Some((city, rest)) => {
            let st = rest
                .split_whitespace()
                .next()
                .filter(|s| s.len() == 2 && s.chars().all(|c| c.is_ascii_uppercase()));
            (
                Some(city.trim().to_string()).filter(|c| !c.is_empty()),
                st.map(str::to_string),
            )
        }
        None => (None, None),
    };
    let grid = text(&["location", "gridsquare"]);
    let coord = |k: &str| text(&["location", k]).and_then(|s| s.parse::<f64>().ok());
    let (lat, lon) = match (coord("latitude"), coord("longitude")) {
        (Some(la), Some(lo)) if (-90.0..=90.0).contains(&la) && (-180.0..=180.0).contains(&lo) => {
            (Some(la), Some(lo))
        }
        _ => (None, None),
    };
    Some(CallookLookup {
        call,
        name,
        qth,
        state,
        grid,
        lat,
        lon,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record and an unknown call are answers; maintenance, an error page and other statuses
    /// are not.
    #[test]
    fn only_valid_and_invalid_are_answers() {
        assert!(answered(
            r#"{"status":"VALID","current":{"callsign":"W1AW"}}"#
        ));
        assert!(answered(r#"{"status":"INVALID"}"#));
        assert!(!answered(r#"{"status":"UPDATING"}"#));
        assert!(!answered("<html>Service Unavailable</html>"));
        assert!(!answered(""));
    }

    /// callook.info's answer for W1AW, as served on 2026-10-24.
    const W1AW: &str = r#"{"status":"VALID","type":"CLUB","current":{"callsign":"W1AW","operClass":""},"previous":{"callsign":"","operClass":""},"trustee":{"callsign":"AF5ID","name":"WELLS, JOSHUA D"},"name":"ARRL HQ OPERATORS CLUB","address":{"line1":"225 MAIN ST","line2":"NEWINGTON, CT 06111","attn":"ATTN: JOSHUA D WELLS"},"location":{"latitude":"41.714707","longitude":"-72.728411","gridsquare":"FN31pr"},"otherInfo":{"grantDate":"12/08/2020","expiryDate":"02/26/2031","lastActionDate":"10/16/2026","frn":"0004511143","ulsUrl":"https://wireless2.fcc.gov/UlsApp/UlsSearch/license.jsp?licKey=780866"}}"#;

    #[test]
    fn a_valid_answer_parses_into_the_lookup() {
        let r = parse_callsign(W1AW).unwrap();
        assert_eq!(
            r,
            CallookLookup {
                call: "W1AW".into(),
                name: Some("ARRL HQ OPERATORS CLUB".into()),
                qth: Some("NEWINGTON".into()),
                state: Some("CT".into()),
                grid: Some("FN31pr".into()),
                lat: Some(41.714707),
                lon: Some(-72.728411),
            }
        );
    }

    #[test]
    fn an_individual_name_reads_first_name_first() {
        let j = W1AW.replace("ARRL HQ OPERATORS CLUB", "SMITH, DAVID J");
        assert_eq!(
            parse_callsign(&j).unwrap().name.as_deref(),
            Some("DAVID J SMITH")
        );
    }

    #[test]
    fn an_unknown_call_or_junk_is_no_record() {
        assert_eq!(parse_callsign(r#"{"status":"INVALID"}"#), None);
        assert_eq!(parse_callsign("<html>down for update</html>"), None);
        assert_eq!(parse_callsign(r#"{"status":"VALID"}"#), None);
    }

    #[test]
    fn only_us_shaped_calls_are_asked() {
        for us in ["W1AW", "K1ABC", "N0X", "AA1A", "AL7XX", "KH6ABC", "WP4AB"] {
            assert!(is_us_call(us), "{us}");
        }
        for not in [
            "G4ABC",
            "AM1A",
            "VE3ABC",
            "W1ABC/P",
            "KH6/W1ABC",
            "W1",
            "JA1ABC",
            "",
        ] {
            assert!(!is_us_call(not), "{not}");
        }
    }

    #[test]
    fn the_url_carries_the_call_and_nothing_else() {
        assert_eq!(build_lookup_url(" w1aw "), "https://callook.info/W1AW/json");
        assert_eq!(
            build_lookup_url("W1AW/../x?y"),
            "https://callook.info/W1AWXY/json"
        );
    }
}
