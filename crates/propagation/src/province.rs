//! Canadian province/territory from a callsign — the VE side of the roster's
//! "State or Province" column.
//!
//! There is no Canadian equivalent of the FCC callsign index and no province
//! polygon table (`grid_state.bin` is generated from US state polygons only, so
//! every Canadian cell in it is either 0 or — worse — the US state that shares a
//! border cell). What Canada does have that the US lost in 1978 is a callsign
//! numeral that still means something: ISED issues the regional digit by the
//! licensee's province, so `VE3` is Ontario the way `W3` has not been
//! Pennsylvania in half a century.
//!
//! Same class of hint as [`crate::FccStates`], with the same caveat stated the
//! same way: it is the LICENSED province, not tonight's location. A licensee who
//! moves may keep the old call. Award credit still comes from the confirmed
//! QSO's logged ADIF `STATE`; this only labels the row.
//!
//! The codes returned are the ADIF Primary Administrative Subdivision codes for
//! DXCC entity 1 (Canada) — the same 13 codes an ADIF export or a TQSL signing
//! expects, so a resolved value is safe to write into a log record's `STATE`.

/// Canadian province/territory (ADIF `STATE` code) for a callsign, or `None` for
/// a non-Canadian call, a prefix with no province meaning (`VE0` maritime
/// mobile), or a Canadian special-event/government prefix we do not model.
///
/// Portable affixes resolve the way DXCC does — [`crate::dxcc::base_call`] picks
/// the side that indicates the location — so `W1ABC/VE3` is Ontario and
/// `VE3XYZ/W1` is nothing (that operator is in the US). A bare regional digit
/// (`VE3ABC/7`) is a plain operating suffix to that rule and keeps the home
/// province; FT8's compound-call forms send `VE7/VE3ABC`, which resolves.
pub fn province_for_call(call: &str) -> Option<&'static str> {
    let up = call.trim().to_ascii_uppercase();
    let b = crate::dxcc::base_call(&up).as_bytes();
    // Two prefix letters and the regional numeral, and nothing more is required: the location
    // side of a portable call IS a bare prefix (`W1ABC/VE3`), and that form is the one that
    // most needs answering — an operator signing it is telling you where they are.
    if b.len() < 3 {
        return None;
    }
    match (&b[..2], b[2]) {
        // VE/VA share one regional series; ISED issues VA as the second block for the same
        // province, so the numeral means the same thing on both.
        (b"VE" | b"VA", n) => match n {
            b'1' => Some("NS"),
            b'2' => Some("QC"),
            b'3' => Some("ON"),
            b'4' => Some("MB"),
            b'5' => Some("SK"),
            b'6' => Some("AB"),
            b'7' => Some("BC"),
            b'8' => Some("NT"),
            b'9' => Some("NB"),
            // VE0 is maritime mobile — a ship in international water is in no province.
            _ => None,
        },
        // Newfoundland and Labrador are one province with two numerals.
        (b"VO", b'1' | b'2') => Some("NL"),
        (b"VY", b'0') => Some("NU"),
        (b"VY", b'1') => Some("YT"),
        (b"VY", b'2') => Some("PE"),
        // VY9 (Canadian government) and everything else: no province meaning. Canada's
        // special-event blocks (VB/VC/VD/VF/VG/VX, CF–CK, XJ–XO) carry the regional numeral
        // too, but they are rare next to the risk of reading a numeral off a prefix that
        // turns out to belong elsewhere — they resolve to nothing rather than to a guess.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_regional_numeral_to_its_province() {
        for (call, want) in [
            ("VE1ABC", "NS"),
            ("VA1ABC", "NS"),
            ("VE2ABC", "QC"),
            ("VA2ABC", "QC"),
            ("VE3ABC", "ON"),
            ("VA3ABC", "ON"),
            ("VE4ABC", "MB"),
            ("VE5ABC", "SK"),
            ("VE6ABC", "AB"),
            ("VE7ABC", "BC"),
            ("VA7ABC", "BC"),
            ("VE8ABC", "NT"),
            ("VE9ABC", "NB"),
            ("VO1ABC", "NL"),
            ("VO2ABC", "NL"),
            ("VY0ABC", "NU"),
            ("VY1ABC", "YT"),
            ("VY2ABC", "PE"),
        ] {
            assert_eq!(province_for_call(call), Some(want), "{call}");
        }
    }

    #[test]
    fn lowercase_and_whitespace_resolve_the_same() {
        assert_eq!(province_for_call(" ve3abc "), Some("ON"));
    }

    #[test]
    fn non_provincial_and_non_canadian_calls_resolve_to_none() {
        for call in [
            "VE0ABC", // maritime mobile — a boat is not a province
            "W1AW",   // US
            "KD9TAW", // US
            "G0ABC",  // England
            "CY0ABC", // Sable Island — its own DXCC entity, not Nova Scotia
            "CY9ABC", // St. Paul Island — likewise
            "VK3ABC", // Australia: a V-prefix that is not Canada
            "VP8ABC", // Falklands
            "VE",     // a bare prefix, not a callsign
            "VEABC",  // no regional numeral at all
            "",
        ] {
            assert_eq!(province_for_call(call), None, "{call}");
        }
    }

    #[test]
    fn portable_affixes_follow_the_dxcc_rule_not_the_home_call() {
        // The location side wins, exactly as `dxcc::resolve` reads it.
        assert_eq!(province_for_call("W1ABC/VE3"), Some("ON"));
        assert_eq!(province_for_call("VE7/VE3ABC"), Some("BC"));
        // A Canadian operating in the US is NOT in a province.
        assert_eq!(province_for_call("VE3XYZ/W1"), None);
        // Plain operating suffixes keep the home province.
        assert_eq!(province_for_call("VE3ABC/P"), Some("ON"));
        assert_eq!(province_for_call("VE3ABC/M"), Some("ON"));
    }

    #[test]
    fn every_code_is_an_adif_canadian_subdivision() {
        // The ADIF Primary Administrative Subdivision enumeration for DXCC 1, and
        // NONE of them may collide with a WAS state — the worked-states set is shared.
        const CANADA: [&str; 13] = [
            "AB", "BC", "MB", "NB", "NL", "NS", "NT", "NU", "ON", "PE", "QC", "SK", "YT",
        ];
        for n in ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'] {
            for p in ["VE", "VA", "VO", "VY"] {
                if let Some(code) = province_for_call(&format!("{p}{n}ABC")) {
                    assert!(
                        CANADA.contains(&code),
                        "{p}{n} → {code} is not an ADIF CA code"
                    );
                    assert!(
                        crate::awards::valid_state(code).is_none(),
                        "{code} collides with a WAS state — it would pollute worked_states"
                    );
                }
            }
        }
    }
}
