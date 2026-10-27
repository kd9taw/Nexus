//! CQ WPX: the prefix a callsign counts for.
//!
//! THE PREFIX RULE IS CQ'S, QUOTED. The CQ WPX Contest rules (<https://www.cqwpx.com/rules/>,
//! section V.C.1, read 2026-10-24) define the prefix the WPX programme counts:
//!
//! * "A PREFIX is the letter/numeral combination which forms the first part of the amateur
//!   call." Examples there: N8, W8, WD8, HG1, HG19, KC2, OE2, OE25, LY1000. "Any difference in
//!   the numbering, lettering, or order of same shall count as a separate prefix."
//! * "In cases of portable operation, the portable designator will then become the prefix."
//! * "Portable designators without numbers will be assigned a zero (Ø) after the second letter
//!   of the portable designator": PA/N8BJQ becomes PA0.
//! * "All calls without numbers will be assigned a zero (Ø) after the first two letters to form
//!   the prefix": XEFTJW counts as XE0.
//! * "Maritime mobile, mobile, /A, /E, /J, /P, or other license class identifiers do not count as
//!   prefixes."
//!
//! ⚠️ NOT VERIFIED, and stated so rather than guessed silently:
//! * The WPX AWARD page (cq-amateur-radio.com) could not be read (its host did not resolve on
//!   2026-10-24), so the award's own thresholds and endorsement steps are not shown anywhere in
//!   Nexus: the Awards view counts prefixes and claims no level. The prefix rule above is the
//!   contest's; the contest page says the contest "is based on an award offered by CQ Magazine
//!   for working all prefixes", and the award is assumed to count prefixes the same way.
//! * A portable designator that is a bare digit (N8BJQ/4) is not covered by an example on the
//!   rules page. Nexus follows the common reading: the digit replaces the call's own numeral
//!   (N8BJQ/4 counts N4).
//! * Confirmation: LoTW credits WPX (<https://lotw.arrl.org/lotw-help/wpx-award-credit/>), and
//!   cards do. Whether CQ takes an Authenticity-Guaranteed eQSL for WPX was not verified, so
//!   Nexus counts LoTW and the card only (an under-count, never an over-claim).

/// Suffixes that "do not count as prefixes": maritime mobile, aeronautical mobile, mobile,
/// portable, the license class identifiers, and QRP.
const NOT_A_PREFIX: &[&str] = &[
    "MM", "AM", "M", "P", "A", "E", "J", "QRP", "QRPP", "AG", "AE", "KT",
];

/// The WPX prefix `call` counts for, upper case ("W8", "LY1000", "PA0"), or `None` when the call
/// is not a callsign at all.
pub fn wpx_prefix(call: &str) -> Option<String> {
    let call = call.trim().to_ascii_uppercase();
    let parts: Vec<&str> = call
        .split('/')
        .map(str::trim)
        .filter(|p| !p.is_empty() && !NOT_A_PREFIX.contains(p))
        .collect();
    if parts
        .iter()
        .any(|p| !p.bytes().all(|b| b.is_ascii_alphanumeric()))
    {
        return None;
    }
    match parts.as_slice() {
        [base] => base_prefix(base),
        [a, b] => {
            // The home call is the longer part; the other is the portable designator, whichever
            // side of the stroke it was written on (PA/N8BJQ and N8BJQ/KH9 alike).
            let (home, port) = if b.len() > a.len() {
                (*b, *a)
            } else {
                (*a, *b)
            };
            base_prefix(home)?;
            if port.bytes().all(|c| c.is_ascii_digit()) {
                // N8BJQ/4 → N4: the digit replaces the call's numeral (see the module header).
                let home_prefix = base_prefix(home)?;
                let letters = home_prefix.trim_end_matches(|c: char| c.is_ascii_digit());
                return Some(format!("{letters}{port}"));
            }
            designator_prefix(port)
        }
        _ => None,
    }
}

/// A call's own prefix: everything up to and including its last numeral ("LY1000ABC" → LY1000,
/// "2E0ABC" → 2E0, "9A1A" → 9A1); a call with no numeral takes its first two letters and a zero.
fn base_prefix(call: &str) -> Option<String> {
    if call.len() < 3 || !call.bytes().any(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    match call.rfind(|c: char| c.is_ascii_digit()) {
        // A call must have letters after its numeral to be a call ("W1" alone is not).
        Some(i) if i + 1 < call.len() => Some(call[..=i].to_string()),
        Some(_) => None,
        None => Some(format!("{}0", &call[..2])),
    }
}

/// A portable designator's prefix: the designator itself up to its last numeral (KH9, VP2E →
/// VP2), or, with no numeral, its first two letters and a zero (PA → PA0, F → F0).
fn designator_prefix(port: &str) -> Option<String> {
    if !port.bytes().any(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    match port.rfind(|c: char| c.is_ascii_digit()) {
        Some(i) => Some(port[..=i].to_string()),
        None => {
            let head: String = port.chars().take(2).collect();
            Some(format!("{head}0"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(call: &str) -> Option<String> {
        wpx_prefix(call)
    }

    /// The rules page's own examples, each as it prints it.
    #[test]
    fn the_rules_pages_examples() {
        for (call, want) in [
            ("N8BJQ", "N8"),
            ("W8AA", "W8"),
            ("WD8XYZ", "WD8"),
            ("HG1A", "HG1"),
            ("HG19XYZ", "HG19"),
            ("KC2ABC", "KC2"),
            ("OE2XYZ", "OE2"),
            ("OE25ABC", "OE25"),
            ("LY1000A", "LY1000"),
            // "XEFTJW would count as XE0."
            ("XEFTJW", "XE0"),
            // "N8BJQ operating from Wake Island would sign N8BJQ/KH9."
            ("N8BJQ/KH9", "KH9"),
            // "PA/N8BJQ would become PAØ."
            ("PA/N8BJQ", "PA0"),
        ] {
            assert_eq!(p(call).as_deref(), Some(want), "{call}");
        }
    }

    /// Mobile, maritime mobile, portable and the class identifiers are not prefixes; a call
    /// starting with a digit keeps it.
    #[test]
    fn suffixes_that_do_not_count_and_digit_led_calls() {
        for (call, want) in [
            ("N8BJQ/P", "N8"),
            ("N8BJQ/MM", "N8"),
            ("N8BJQ/M", "N8"),
            ("N8BJQ/AG", "N8"),
            ("n8bjq/qrp", "N8"),
            ("PA/N8BJQ/P", "PA0"),
            ("2E0ABC", "2E0"),
            ("9A1A", "9A1"),
            ("3DA0RS", "3DA0"),
            ("4X4AA", "4X4"),
            ("VP2E/K1ABC", "VP2"),
        ] {
            assert_eq!(p(call).as_deref(), Some(want), "{call}");
        }
    }

    /// The digit-only designator follows the reading the module header names (unverified).
    #[test]
    fn a_bare_digit_designator_replaces_the_numeral() {
        assert_eq!(p("N8BJQ/4").as_deref(), Some("N4"));
        assert_eq!(p("WD8XYZ/0").as_deref(), Some("WD0"));
    }

    /// Junk is no prefix at all, so it can never inflate the count.
    #[test]
    fn junk_is_not_a_prefix() {
        for call in ["", "W1", "123", "N8BJQ/KH9/XYZ/ABC", "AB-1CD", "/", "W1AW/"] {
            if call == "W1AW/" {
                assert_eq!(
                    p(call).as_deref(),
                    Some("W1"),
                    "a trailing stroke is ignored"
                );
                continue;
            }
            assert_eq!(p(call), None, "{call:?}");
        }
    }
}
