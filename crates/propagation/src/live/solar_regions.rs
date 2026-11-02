//! NOAA SWPC's numbered active regions (the `live` feature): one capped download of
//! `json/solar_regions.json`, read by [`crate::regions::parse_solar_regions`].
//!
//! The fetch is held to the limits every new SWPC fetch keeps: HTTPS only, no redirect followed
//! (SWPC serves the file at its own address, so a redirect is a fault, not a move), a timeout,
//! and the body read only as far as [`crate::regions::REGIONS_MAX_BYTES`] — a server that sends
//! more is refused, never read to the end. The shack's slot (src-tauri `solar_regions`) decides
//! when to ask and backs off after a failure.

use std::time::Duration;

use crate::regions::{parse_solar_regions, SolarRegions, REGIONS_MAX_BYTES};

const REGIONS_URL: &str = "https://services.swpc.noaa.gov/json/solar_regions.json";
const UA: &str = "nexus-propagation/0.1 (+ham radio sun disc)";

/// Download and read NOAA's region list at `now`. `Err` on network or HTTP trouble, an answer
/// past the size limit, or one that is not a list — the caller keeps the copy it holds.
pub fn fetch_solar_regions(now: i64) -> Result<SolarRegions, String> {
    let c = builder()
        .https_only(true)
        .build()
        .map_err(|e| e.to_string())?;
    let body = fetch_from(&c, REGIONS_URL)?;
    parse_solar_regions(&body, now)
}

/// The client: a timeout, and no redirect followed.
fn builder() -> reqwest::blocking::ClientBuilder {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .redirect(reqwest::redirect::Policy::none())
}

fn fetch_from(c: &reqwest::blocking::Client, url: &str) -> Result<String, String> {
    let resp = c.get(url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "NOAA's region list answered HTTP {} instead of the file",
            resp.status().as_u16()
        ));
    }
    super::body::read_capped(resp, REGIONS_MAX_BYTES as u64, "NOAA region list")
}

#[cfg(test)]
mod tests {
    use super::super::body::{ok_reply, serve};
    use super::*;

    const LIVE: &str = include_str!("../../tests/fixtures/swpc-solar-regions-2026-10-30.json");

    fn fetch_port(port: u16) -> Result<String, String> {
        fetch_from(
            &builder().build().expect("client"),
            &format!("http://127.0.0.1:{port}/json/solar_regions.json"),
        )
    }

    #[test]
    fn the_list_of_a_normal_size_is_read_whole() {
        assert_eq!(
            fetch_port(serve(vec![ok_reply(LIVE.as_bytes())])).as_deref(),
            Ok(LIVE)
        );
    }

    #[test]
    fn a_download_past_the_limit_is_refused_not_read_to_the_end() {
        let huge = vec![b' '; REGIONS_MAX_BYTES + 1];
        let err = fetch_port(serve(vec![ok_reply(&huge)])).unwrap_err();
        assert!(
            err.starts_with("NOAA region list: ") && err.contains("larger than"),
            "{err}"
        );
        // CONTROL: exactly at the limit it is read.
        let edge = vec![b' '; REGIONS_MAX_BYTES];
        assert!(fetch_port(serve(vec![ok_reply(&edge)])).is_ok());
    }

    /// The second reply is the list a followed redirect would have fetched.
    #[test]
    fn a_redirect_is_refused_not_followed() {
        let port = serve(vec![
            b"HTTP/1.1 302 Found\r\nLocation: /elsewhere.json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec(),
            ok_reply(LIVE.as_bytes()),
        ]);
        let err = fetch_port(port).unwrap_err();
        assert!(err.contains("HTTP 302"), "{err}");
    }

    #[test]
    fn an_error_status_is_not_a_list() {
        let port = serve(vec![
            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbusy"
                .to_vec(),
        ]);
        assert!(fetch_port(port).unwrap_err().contains("HTTP 503"));
    }

    /// The real client refuses a plain-HTTP address outright: HTTPS only.
    #[test]
    fn plain_http_is_refused() {
        let c = builder().https_only(true).build().expect("client");
        let port = serve(vec![ok_reply(LIVE.as_bytes())]);
        assert!(fetch_from(&c, &format!("http://127.0.0.1:{port}/x")).is_err());
    }
}
