//! The IOTA island-group list (the `live` feature): one download of IOTA's `groups.json`.
//!
//! Source: <https://www.iota-world.org/islands-on-the-air/downloads/download-file.html?path=groups.json>,
//! the address IOTA Ltd publishes for software on its Developers page
//! (<https://www.iota-world.org/iota-directory/developers.html>: "an API key is no longer
//! required"). The list is © Radio Society of Great Britain and © Islands On The Air (IOTA) Ltd;
//! its copyright notice lets anyone download it "for your own personal non-commercial home use"
//! (<https://www.iota-world.org/copyright-notice.html>). So Nexus never ships a copy: the shell
//! calls this only after the operator has said yes, onto their own machine, at most monthly, and
//! the server sends no validators to make a refresh conditional. Parsing is
//! [`crate::iota::IotaGroups::parse`].

use std::time::Duration;

const GROUPS_URL: &str =
    "https://www.iota-world.org/islands-on-the-air/downloads/download-file.html?path=groups.json";
const UA: &str = "Nexus (amateur radio logbook; https://hamradiotools.io; kd9taw@protonmail.com)";

/// The most the download is read to: the list was 294 KB in October 2026.
const GROUPS_MAX_BYTES: u64 = 2 * 1024 * 1024;

/// Download `groups.json` (about 300 KB). `Err` on network or HTTP trouble — the caller keeps the
/// copy it holds, or the shape-only check when it holds none.
pub fn fetch_groups() -> Result<String, String> {
    let c = builder()
        .https_only(true)
        .build()
        .map_err(|e| e.to_string())?;
    fetch_from(&c, GROUPS_URL)
}

/// The client: IOTA serves the file at its address itself, so a redirect is never followed.
fn builder() -> reqwest::blocking::ClientBuilder {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(UA)
        .redirect(reqwest::redirect::Policy::none())
}

fn fetch_from(c: &reqwest::blocking::Client, url: &str) -> Result<String, String> {
    let resp = c
        .get(url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "the IOTA list server answered HTTP {} instead of the file",
            resp.status().as_u16()
        ));
    }
    super::body::read_capped(resp, GROUPS_MAX_BYTES, "IOTA list")
}

#[cfg(test)]
mod tests {
    use super::super::body::{ok_reply, serve};
    use super::*;

    const SAMPLE: &str = include_str!("../../tests/fixtures/iota_groups_sample.json");

    fn fetch_port(port: u16) -> Result<String, String> {
        fetch_from(
            &builder().build().expect("client"),
            &format!("http://127.0.0.1:{port}/groups.json"),
        )
    }

    #[test]
    fn the_list_of_a_normal_size_is_read_whole() {
        assert_eq!(
            fetch_port(serve(vec![ok_reply(SAMPLE.as_bytes())])).as_deref(),
            Ok(SAMPLE)
        );
    }

    /// A download past the limit is refused, not read to the end, and the copy held is kept.
    #[test]
    fn a_download_past_the_limit_is_refused() {
        let huge = vec![b' '; GROUPS_MAX_BYTES as usize + 1];
        let err = fetch_port(serve(vec![ok_reply(&huge)])).unwrap_err();
        assert!(
            err.starts_with("IOTA list: ") && err.contains("larger than"),
            "{err}"
        );
    }

    /// IOTA serves the file at its own address, so a redirect is refused rather than followed —
    /// the second reply, which a followed redirect would fetch, is the list itself.
    #[test]
    fn a_redirect_is_refused_not_followed() {
        let port = serve(vec![
            b"HTTP/1.1 302 Found\r\nLocation: /elsewhere.json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec(),
            ok_reply(SAMPLE.as_bytes()),
        ]);
        let err = fetch_port(port).unwrap_err();
        assert!(err.contains("HTTP 302"), "{err}");
    }
}
