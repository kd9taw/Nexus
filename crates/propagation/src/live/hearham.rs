//! hearham.com open repeater directory transport (the `live` feature) — the
//! no-auth fallback/development source for the "Program" section (and the
//! primary source for non-US locations, which RepeaterBook state planning
//! doesn't cover). One GET returns the whole worldwide directory (~22k rows,
//! ~1 MB gzipped); the shell caches it beside settings.json with a 7-day TTL.
//! Parsing lives in [`crate::repeaters::parse_hearham_json`].

use std::time::Duration;

const UA: &str = "Nexus (radio programming; https://hamradiotools.io; kd9taw@protonmail.com)";
const URL: &str = "https://hearham.com/api/repeaters/v1";

/// Fetch the full hearham repeater list (JSON body). Long timeout — this is a
/// single bulk payload, fetched rarely (cache TTL 7 days).
pub fn fetch_all() -> Result<String, String> {
    let c = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(UA)
        .https_only(true)
        .build()
        .map_err(|_| "hearham: HTTP client initialization failed".to_string())?;
    let resp = c.get(URL).send().map_err(redact)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("hearham: server returned HTTP {}", status.as_u16()));
    }
    resp.text()
        .map_err(|_| "hearham: could not read the response body".to_string())
}

fn redact(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "hearham: request timed out — try again shortly".to_string()
    } else if e.is_connect() {
        "hearham: could not connect — check your network".to_string()
    } else {
        "hearham: request failed".to_string()
    }
}
