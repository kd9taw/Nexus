//! RSGB ETCC repeater list transport (the `live` feature): the UK coordinator's public API at
//! `https://api-beta.rsgb.online`, used for UK origins in the "Program" section.
//!
//! The API's own page (read 2026-10-01) documents `/locator/<4 or 6 character square>`,
//! `/callsign/…`, `/band/…`, `/keeper/…` and `/all/systems` (every public listing). Nexus asks
//! ONLY by locator square, one request per square, for the squares a search actually reaches
//! (`repeaters::plan_rsgb_squares`, at most nine), and the shell caches each square for 7 days
//! on the PC like the other directories, behind the same per-query retry throttle. It never
//! pulls the whole list.
//!
//! Terms: the API page states none. The coordinator's site, ukrepeater.net, offers its data "free
//! of sponsors – free of ads – no cookies – no registration – yours to enjoy!", and the operator's
//! licence call (2026-09-30) is to use it with credit: "Repeater data: RSGB ETCC (ukrepeater.net)"
//! on screen and as a comment line in every exported file. The endpoint is a BETA, so the shell
//! falls back to hearham alone, and says so, on any failure or any payload it cannot read.
//! Parsing lives in [`crate::repeaters::parse_rsgb_json`].

use super::neterr;
use std::time::Duration;

/// The same contactable User-Agent the hearham transport sends.
const UA: &str = "Nexus (radio programming; https://hamradiotools.io; kd9taw@protonmail.com)";
const URL: &str = "https://api-beta.rsgb.online/locator/";

/// Is `square` a 4-character Maidenhead square ("IO83")? It goes into the URL path, so nothing
/// else is ever sent.
fn is_square(square: &str) -> bool {
    let b = square.as_bytes();
    b.len() == 4
        && (b'A'..=b'R').contains(&b[0])
        && (b'A'..=b'R').contains(&b[1])
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
}

/// Fetch one locator square's listings (the JSON body). Anything but a 4-character square in
/// capitals is refused before a request is made.
pub fn fetch_square(square: &str) -> Result<String, String> {
    if !is_square(square) {
        return Err(format!("RSGB: {square:?} is not a locator square"));
    }
    let c = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .https_only(true)
        .build()
        .map_err(|_| "RSGB: HTTP client initialization failed".to_string())?;
    let resp = c
        .get(format!("{URL}{square}"))
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .map_err(|e| neterr::redact_following_redirects("RSGB", &e))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("RSGB: server returned HTTP {}", status.as_u16()));
    }
    resp.text()
        .map_err(|_| "RSGB: could not read the response body".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a square reaches the URL: the bulk endpoint, a path walk, a 6-character subsquare and
    /// lower case are all refused before any request (`fetch_square` returns at the check, so
    /// this test makes no network call).
    #[test]
    fn only_a_four_character_square_is_ever_requested() {
        assert!(is_square("IO83"));
        assert!(is_square("JO01"));
        for bad in [
            "",
            "IO8",
            "IO83PL",
            "io83",
            "IO8X",
            "SZ83",
            "all/systems",
            "../all",
        ] {
            assert!(!is_square(bad), "{bad:?} must not reach the URL");
            let err = fetch_square(bad).expect_err("refused before a request");
            assert!(err.contains("not a locator square"), "{bad:?}: {err}");
        }
    }
}
