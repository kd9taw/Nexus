//! WWFF (World Wide Flora & Fauna) activator-spot fetch (the `live` feature): the public spot
//! list of WWFF's own Spotline service, handed to the pure [`crate::pota::parse_wwff_spots`]. No
//! auth.
//!
//! THE TERMS, from Spotline's information page <https://spots.wwff.co/info> (read 2026-11-03),
//! "Agenda & Spot Data": "You can access the WWFF agenda and spot data directly as plain text
//! JSON files. These files are updated automatically by the system and can be used for
//! integration purposes. Fetch each feed no more frequently than once every 30 seconds. Please
//! use a descriptive HTTP User-Agent that identifies your application and includes your contact
//! information." The recent-spots file is `/static/spots.json`. An API key is asked for only by
//! Spotline's separate API (posting spots), which Nexus does not use. So:
//! * every caller goes through [`fetch_wwff_spots`], which asks Spotline at most once per
//!   [`MIN_FETCH_INTERVAL`] whoever calls it (the board, its pop-outs, the background poller),
//!   and answers in between with the last answer, a failure included;
//! * the User-Agent names Nexus, its version and the project's public page, as the pota.app spot
//!   post's does ([`super::hrdlog::user_agent`]): `Nexus/1.22.0 (+https://github.com/kd9taw/Nexus)`.
//!   Never the operator's callsign or email, nor anything else from Settings.
//!
//! How often it is asked in practice: a board showing WWFF polls once a minute (a board that does
//! not show it asks nothing), and the background poller that warms the Needed board every three
//! minutes, both well above the 30 s floor.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::pota::{parse_wwff_spots, OtaSpot};

const WWFF_SPOTS_URL: &str = "https://spots.wwff.co/static/spots.json";

/// Spotline's own floor: "Fetch each feed no more frequently than once every 30 seconds."
pub const MIN_FETCH_INTERVAL: Duration = Duration::from_secs(30);

/// The most the spot list is read to: it was 17 KB on 2026-11-03 (the recent spots, a few dozen
/// rows), so 1 MB is about sixty times it.
const SPOTS_REPLY_BYTES: u64 = 1024 * 1024;

/// When Spotline was last asked, and what it answered.
type Asked = Option<(Instant, Result<Vec<OtaSpot>, String>)>;

static LAST: Mutex<Asked> = Mutex::new(None);

/// The WWFF activators on the air now ("who's on the air"), at most one request to Spotline per
/// [`MIN_FETCH_INTERVAL`] across the whole process. The lock is held across the request, so two
/// callers at once make one request between them. `app_version` is Nexus's own, for the
/// User-Agent.
pub fn fetch_wwff_spots(app_version: &str) -> Result<Vec<OtaSpot>, String> {
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    throttled(&mut last, Instant::now(), || {
        Ok(parse_wwff_spots(&get_text(
            &client(app_version)?,
            WWFF_SPOTS_URL,
        )?))
    })
}

/// The last answer while it is younger than [`MIN_FETCH_INTERVAL`] at `now`, else `ask`'s, kept
/// as the last answer whatever it was.
fn throttled(
    last: &mut Asked,
    now: Instant,
    ask: impl FnOnce() -> Result<Vec<OtaSpot>, String>,
) -> Result<Vec<OtaSpot>, String> {
    if let Some((at, answer)) = last.as_ref() {
        if now.saturating_duration_since(*at) < MIN_FETCH_INTERVAL {
            return answer.clone();
        }
    }
    let answer = ask();
    *last = Some((now, answer.clone()));
    answer
}

/// The client the fetch uses: HTTPS only, on [`builder`]'s timeout and redirect policy.
fn client(app_version: &str) -> Result<reqwest::blocking::Client, String> {
    builder()
        .user_agent(super::hrdlog::user_agent(app_version))
        .https_only(true)
        .build()
        .map_err(|e| e.to_string())
}

/// A timeout, and no redirect followed (Spotline serves the file at its own address). The
/// loopback tests build on this (plain HTTP); the fetch itself always goes through [`client`].
fn builder() -> reqwest::blocking::ClientBuilder {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
}

fn get_text(c: &reqwest::blocking::Client, url: &str) -> Result<String, String> {
    let resp = c.get(url).send().map_err(|e| e.to_string())?;
    super::body::read_ok(resp, SPOTS_REPLY_BYTES, "WWFF Spotline")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list is read through `get_text` on `client`: HTTPS only, no redirect followed, at
    /// most `SPOTS_REPLY_BYTES`.
    #[test]
    fn the_wire_keeps_its_limits() {
        super::super::body::assert_wire_limits(
            builder,
            || client("test"),
            get_text,
            SPOTS_REPLY_BYTES,
            "WWFF Spotline",
        );
    }

    /// Spotline is asked at most once per 30 s: a second ask inside the window is answered with
    /// the first answer, a failed one included, and the first ask at or after it goes out.
    #[test]
    fn spotline_is_asked_at_most_once_per_thirty_seconds() {
        let mut last: Asked = None;
        let mut asked = 0;
        let t0 = Instant::now();
        let mut at = |last: &mut Asked, secs: u64, answer: Result<Vec<OtaSpot>, String>| {
            throttled(last, t0 + Duration::from_secs(secs), || {
                asked += 1;
                answer
            })
        };
        assert_eq!(
            at(&mut last, 0, Err("blocked".into())),
            Err("blocked".into())
        );
        // Inside the window: the failure stands, and nothing is asked.
        assert_eq!(at(&mut last, 29, Ok(Vec::new())), Err("blocked".into()));
        // At the window's end it is asked again.
        assert_eq!(at(&mut last, 30, Ok(Vec::new())), Ok(Vec::new()));
        assert_eq!(at(&mut last, 45, Err("late".into())), Ok(Vec::new()));
        assert_eq!(asked, 2, "two asks in 45 s");
    }

    /// The User-Agent names the application, its version and the project's public page, and
    /// nothing a caller could smuggle a header into.
    #[test]
    fn the_user_agent_identifies_nexus_its_version_and_the_project() {
        let ua = crate::live::hrdlog::user_agent("1.22.0");
        assert_eq!(ua, "Nexus/1.22.0 (+https://github.com/kd9taw/Nexus)");
        assert!(!crate::live::hrdlog::user_agent("1.0\r\nX: 1").contains('\n'));
    }
}
