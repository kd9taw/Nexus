//! POTA / SOTA activator-spot fetch (the `live` feature). Thin blocking HTTP that
//! pulls the public spot feeds and hands the bytes to the pure
//! [`crate::pota`] parsers. No auth.

use std::time::Duration;

use crate::pota::{parse_pota_spots, parse_sota_spots, OtaSpot};

const UA: &str = "nexus-pota/0.1 (+ham radio parks/summits on the air)";
const POTA_SPOTS_URL: &str = "https://api.pota.app/spot/activator";

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(UA)
        .build()
        .map_err(|e| e.to_string())
}

fn get_text(c: &reqwest::blocking::Client, url: &str) -> Result<String, String> {
    c.get(url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())
}

/// Fetch current POTA activator spots ("who's on the air now").
pub fn fetch_pota_spots() -> Result<Vec<OtaSpot>, String> {
    let c = client()?;
    Ok(parse_pota_spots(&get_text(&c, POTA_SPOTS_URL)?))
}

/// Fetch the most recent `count` SOTAwatch spots (clamped 1..=50).
pub fn fetch_sota_spots(count: u32) -> Result<Vec<OtaSpot>, String> {
    let c = client()?;
    let n = count.clamp(1, 50);
    let url = format!("https://api-db2.sota.org.uk/api/spots/{n}/all");
    Ok(parse_sota_spots(&get_text(&c, &url)?))
}

const POTA_PARK_URL: &str = "https://api.pota.app/park/";

/// One park's details from the live POTA directory — includes the lat/lon the local CSV index
/// doesn't carry, and backfills parks missing from a stale (or empty) local list.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveParkDetail {
    pub reference: String,
    pub name: String,
    pub grid: String,
    pub location: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

#[derive(serde::Deserialize)]
struct ApiPark {
    #[serde(default)]
    reference: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    grid6: Option<String>,
    #[serde(default)]
    grid4: Option<String>,
    #[serde(default, rename = "locationDesc")]
    location: String,
    #[serde(default)]
    latitude: Option<f64>,
    #[serde(default)]
    longitude: Option<f64>,
}

/// Fetch one park's details live from the POTA directory (name / grid / location + coordinates).
/// `reference` should be a normalized ref (e.g. `K-1234`). An unknown park or unparseable body
/// surfaces as `Err`. NOTE: the POTA API returns HTTP 200 with a JSON `null` body for an unknown
/// park (not a 404), so we deserialize as `Option` and treat null / empty as "not found".
pub fn fetch_park(reference: &str) -> Result<LiveParkDetail, String> {
    let c = client()?;
    let body = get_text(&c, &format!("{POTA_PARK_URL}{reference}"))?;
    let p = serde_json::from_str::<Option<ApiPark>>(&body)
        .map_err(|e| e.to_string())?
        .filter(|p| !(p.reference.is_empty() && p.name.is_empty()))
        .ok_or_else(|| format!("no park found for {reference}"))?;
    // Prefer the 6-char grid, fall back to the 4-char.
    let grid = p
        .grid6
        .filter(|s| !s.is_empty())
        .or(p.grid4)
        .unwrap_or_default();
    Ok(LiveParkDetail {
        reference: if p.reference.is_empty() {
            reference.to_string()
        } else {
            p.reference
        },
        name: p.name,
        grid,
        location: p.location,
        latitude: p.latitude,
        longitude: p.longitude,
    })
}

// ---------------------------------------------------------------------------
// Posting a spot. pota.app publishes no API doc; every client copies what its website sends, so
// this does too, field for field. Only ever called for the operator's own press (the policy —
// own call only, validation, the throttle, the session's login latch — lives in the app).
// ---------------------------------------------------------------------------

/// The address the pota.app website's own spot form posts to — its app bundle (fetched
/// 2026-09-14): `O.a.post("https://".concat("api.pota.app","/spot"),e)` — and Ham2K PoLo's
/// `fetch('https://api.pota.app/spot', { method: 'POST', … })` (ham2k/app-polo,
/// `src/extensions/activities/pota/POTAPostSpotAPI.js`). hunterlog (`POST_SPOT_URL =
/// "https://api.pota.app/spot/"`) and POTACAT add a trailing slash; the website does not.
/// No login is sent: the website's form sends none (its `POST /activation` is the call that does).
pub const POTA_SPOT_POST_URL: &str = "https://api.pota.app/spot";

/// The most of a refusal body a log line may carry. pota.app's 200 reply is the whole spot list.
pub const SPOT_EXCERPT_CHARS: usize = 160;

/// The most of any reply read at all.
const SPOT_REPLY_BYTES: u64 = 64 * 1024;

/// One spot: exactly the seven fields the pota.app website's spot form sends, in its order. From
/// its app bundle: `{activator:this.activator, spotter:this.spotter, frequency:this.frequency,
/// reference:this.reference, mode:this.mode, source:"Web", comments:this.comments}`. PoLo sends
/// the same seven with `source: 'Ham2K Portable Logger'` and hunterlog with `'source':
/// 'hunterlog'`; `source` names the app. `frequency` is kHz as text, the unit the website's form
/// takes ("Frequency in kHz (> 1000)", "Example: 7123 or 14234").
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SpotPost {
    pub activator: String,
    pub spotter: String,
    pub frequency: String,
    pub reference: String,
    pub mode: String,
    pub source: String,
    pub comments: String,
}

/// What pota.app answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpotAnswer {
    /// A 2xx. The website treats its reply (the refreshed spot list) as success.
    Posted,
    /// An auth refusal: 401/403, or a body asking for a login.
    LoginRequired,
    /// Any other answer, with a short single-line excerpt for a log line.
    Refused { status: u16, excerpt: String },
}

fn spot_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        // A redirect is an answer to classify (a login page, say), never a second POST elsewhere.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())
}

/// The request, built and not sent, so its shape is testable offline. The User-Agent is set on
/// the request itself: a client's default headers are only merged in when it executes. Like PoLo
/// (`'User-Agent': \`Ham2K Portable Logger/${version}\``), Nexus names itself; it does not copy
/// the website's `origin`/`referer` the way hunterlog and POTACAT do.
fn spot_request(
    client: &reqwest::blocking::Client,
    app_version: &str,
    spot: &SpotPost,
) -> Result<reqwest::blocking::Request, String> {
    let body = serde_json::to_vec(spot).map_err(|e| e.to_string())?;
    client
        .post(POTA_SPOT_POST_URL)
        .header(
            reqwest::header::USER_AGENT,
            super::hrdlog::user_agent(app_version),
        )
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .build()
        .map_err(|e| e.to_string())
}

/// POST one spot, once. `Err` is a transport failure; the caller never retries it.
pub fn post_spot(app_version: &str, spot: &SpotPost) -> Result<SpotAnswer, String> {
    use std::io::Read;
    let client = spot_client()?;
    let response = client
        .execute(spot_request(&client, app_version, spot)?)
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let mut body = Vec::new();
    response
        .take(SPOT_REPLY_BYTES)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    let body = String::from_utf8_lossy(&body);
    let text = if location.is_empty() {
        body.into_owned()
    } else {
        format!("{location} {body}")
    };
    Ok(classify_spot_reply(status, &text))
}

/// Words that mean "log in first" in a refusal body.
const LOGIN_WORDS: [&str; 9] = [
    "login",
    "log in",
    "sign in",
    "signin",
    "unauthorized",
    "unauthorised",
    "not authorized",
    "authorization",
    "authenticat",
];

/// Classify pota.app's answer. A 2xx whose body is JSON is a post, whatever the spot list's
/// comments say; a 2xx page asking for a login is not.
pub fn classify_spot_reply(status: u16, body: &str) -> SpotAnswer {
    let lower = body.to_ascii_lowercase();
    let asks_login = LOGIN_WORDS.iter().any(|w| lower.contains(w));
    let json = serde_json::from_str::<serde_json::Value>(body).is_ok();
    match status {
        401 | 403 => SpotAnswer::LoginRequired,
        200..=299 if json || !asks_login => SpotAnswer::Posted,
        _ if asks_login => SpotAnswer::LoginRequired,
        _ => SpotAnswer::Refused {
            status,
            excerpt: spot_excerpt(body),
        },
    }
}

/// One line, no control characters, at most [`SPOT_EXCERPT_CHARS`]. For any text bound for a log.
pub fn spot_excerpt(body: &str) -> String {
    body.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(SPOT_EXCERPT_CHARS)
        .collect()
}

#[cfg(test)]
mod spot_post_tests {
    use super::*;

    fn spot() -> SpotPost {
        SpotPost {
            activator: "W9XYZ".into(),
            spotter: "W9XYZ".into(),
            frequency: "14285".into(),
            reference: "US-0001".into(),
            mode: "SSB".into(),
            source: "Nexus".into(),
            comments: "QRV".into(),
        }
    }

    /// Built, never sent: nothing in this crate's tests reaches pota.app.
    #[test]
    fn the_spot_request_is_the_websites_json_post_with_an_honest_user_agent() {
        let client = spot_client().expect("client");
        let request = spot_request(&client, "1.12.0", &spot()).expect("request");
        assert_eq!(request.method(), reqwest::Method::POST);
        assert_eq!(request.url().as_str(), "https://api.pota.app/spot");
        let header = |name: &str| {
            request
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string()
        };
        assert_eq!(
            header("user-agent"),
            "Nexus/1.12.0 (+https://github.com/kd9taw/Nexus)"
        );
        assert_eq!(header("content-type"), "application/json");
        // No borrowed identity: Nexus does not claim to be the website.
        assert!(request.headers().get("origin").is_none());
        assert!(request.headers().get("referer").is_none());
        assert!(request.headers().get("authorization").is_none());
        let body: serde_json::Value = serde_json::from_slice(
            request
                .body()
                .and_then(|b| b.as_bytes())
                .expect("a buffered body"),
        )
        .expect("json");
        assert_eq!(
            body,
            serde_json::json!({
                "activator": "W9XYZ", "spotter": "W9XYZ", "frequency": "14285",
                "reference": "US-0001", "mode": "SSB", "source": "Nexus", "comments": "QRV"
            })
        );
        assert_eq!(body.as_object().map(|o| o.len()), Some(7));
    }

    #[test]
    fn a_2xx_is_posted_and_an_auth_refusal_is_login_required() {
        assert_eq!(classify_spot_reply(200, "[]"), SpotAnswer::Posted);
        assert_eq!(
            classify_spot_reply(201, r#"[{"comments":"login at the park"}]"#),
            SpotAnswer::Posted
        );
        for status in [401, 403] {
            assert_eq!(classify_spot_reply(status, ""), SpotAnswer::LoginRequired);
        }
        // A login-required body is a login refusal whatever the status says.
        assert_eq!(
            classify_spot_reply(400, "Authorization header required"),
            SpotAnswer::LoginRequired
        );
        assert_eq!(
            classify_spot_reply(200, "<html>Please sign in to continue</html>"),
            SpotAnswer::LoginRequired
        );
        // Positive control: a refusal that names no login is not one.
        assert_eq!(
            classify_spot_reply(400, "Invalid reference"),
            SpotAnswer::Refused {
                status: 400,
                excerpt: "Invalid reference".into()
            }
        );
    }

    #[test]
    fn a_refusal_carries_only_a_short_single_line_excerpt() {
        let body = format!("bad\r\nrequest\u{7}{}", "x".repeat(10_000));
        let SpotAnswer::Refused { status, excerpt } = classify_spot_reply(500, &body) else {
            panic!("a 500 without login wording is a refusal");
        };
        assert_eq!(status, 500);
        assert!(excerpt.chars().count() <= SPOT_EXCERPT_CHARS);
        assert!(excerpt.starts_with("bad request"));
        assert!(!excerpt.chars().any(char::is_control));
    }
}
