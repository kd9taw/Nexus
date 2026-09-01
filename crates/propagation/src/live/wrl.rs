//! World Radio League transport (the `live` feature) — thin authenticated calls to
//! `api.worldradioleague.com`.
//!
//! All WRL knowledge (JSON body shape, error-code classification, the URLs) lives in
//! the pure [`tempo_core::wrl`]; this module just moves bytes. Mirrors the
//! `qrz.rs`/`hrdlog.rs` discipline: HTTPS enforced, no redirect-following, and
//! **redacted errors** — the key travels in the Authorization header and a
//! `reqwest::Error` can echo the request, so we never stringify the raw error.

use super::neterr;
use std::time::Duration;

const UA: &str = "nexus-propagation/0.1 (+ham radio propagation nowcast)";

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "WRL: HTTP client initialization failed".to_string())
}

/// POST one contact (JSON body built by [`tempo_core::wrl::build_contact_json`]).
/// Returns `(status, body)` for [`tempo_core::wrl::classify_response`]. `Err` only on
/// a transport failure, always redacted. Non-2xx is NOT an `Err` here — WRL's error
/// body carries the stable code the classifier needs.
pub fn post_contact(url: &str, key: &str, body: String) -> Result<(u16, String), String> {
    let resp = client()?
        .post(url)
        .bearer_auth(key)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .map_err(redact)?;
    let status = resp.status().as_u16();
    let text = resp
        .text()
        .map_err(|_| "WRL: could not read the response body".to_string())?;
    Ok((status, text))
}

/// Authenticated GET (`/v1/me`, `/v1/logbooks`) returning the raw JSON body.
/// Non-2xx returns the body too — the caller branches on `error.code`.
pub fn get_json(url: &str, key: &str) -> Result<(u16, String), String> {
    let resp = client()?.get(url).bearer_auth(key).send().map_err(redact)?;
    let status = resp.status().as_u16();
    let text = resp
        .text()
        .map_err(|_| "WRL: could not read the response body".to_string())?;
    Ok((status, text))
}

fn redact(e: reqwest::Error) -> String {
    neterr::redact("WRL", &e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_url_rejected_without_leaking_key_or_host() {
        let err = post_contact(
            "http://api.wrl.example/v1/contacts",
            "wrl_live_SECRETKEY",
            "{}".to_string(),
        )
        .unwrap_err();
        assert!(!err.contains("SECRETKEY"), "key leaked: {err}");
        assert!(!err.contains("wrl.example"), "host leaked: {err}");
        assert!(err.starts_with("WRL: "), "unexpected message: {err}");
    }
}
