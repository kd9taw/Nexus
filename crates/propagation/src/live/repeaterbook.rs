//! RepeaterBook export transport (the `live` feature) — one authenticated GET
//! per US state. Parsing lives in the pure [`crate::repeaters`]; caching, TTL
//! and per-state throttling live in the shell.
//!
//! Compliance (RepeaterBook API terms, 2026): access is approval-gated, and
//! there are two mutually exclusive paths, chosen per user.
//!
//! 1. PERSONAL. The user generates their own `rbuapp_…` token from their
//!    RepeaterBook account ("API Apps" dashboard) and pastes it into Settings.
//!    [`fetch_state`] then calls RepeaterBook directly under that user's own
//!    account. The token rides the `X-RB-App-Token` header (never a URL, never
//!    logged).
//! 2. SHARED. A user with no personal token holds no credential at all;
//!    [`fetch_state_proxy`] calls our Worker, which is the only thing that ever
//!    holds a shared token. Pending RepeaterBook's approval, so it currently
//!    503s and the shell falls through to hearham.
//!
//! No RepeaterBook credential is embedded in this client, the installer, or
//! the repo, in any form including build-time injection — baking one is
//! forbidden by RB's terms (unlike ClubLog, whose app keys are meant to be
//! embedded; that pattern does NOT transfer). The User-Agent uniquely
//! identifies the app with a contact address; rate limits are unpublished, so a
//! 429 maps to a DISTINCT error the shell treats as "serve the stale cache and
//! back off". Data is fetched per-user, on demand, for programming that user's
//! own radios — never redistributed or bundled.

use super::neterr;
use std::time::Duration;

/// Uniquely-identifying UA per RepeaterBook's API requirements, in their
/// `AppName/1.0 (VendorOrSite; Contact)` format. This MUST match the value on
/// file with the approved application byte-for-byte (a mismatch is denied), so
/// the version here is the API-CLIENT version — deliberately NOT the app
/// version, and bumped only if the integration itself changes (update the
/// approved value with RepeaterBook first).
const UA: &str = "Nexus/1.0 (hamradiotools.io; kd9taw@protonmail.com)";

/// Rate-limit sentinel: the shell matches this to serve stale cache + back off.
pub const ERR_RATE_LIMITED: &str = "RepeaterBook: rate limited";

/// The Nexus RepeaterBook proxy (centralized model): a Cloudflare Worker on
/// hamradiotools.io holding the approved `app_` token as a SERVER-side secret
/// (never in this client, per RepeaterBook's terms) with a 7-day edge cache
/// per state. Returns 503 until activated — the shell then falls back to
/// hearham, so the feature works before/without RepeaterBook approval.
const PROXY_URL: &str = "https://rb.hamradiotools.io/rb/export";

/// Proxy client key, baked at build time from `NEXUS_RB_CLIENT_KEY`.
///
/// ⚠️ DETERRENCE, NOT AUTHENTICATION, and it must never be described as
/// authentication. Nexus is a distributed desktop application: anything shipped
/// in the binary can be extracted from it, so this cannot establish that a
/// caller IS Nexus. It filters casual scrapers off the proxy and identifies our
/// traffic, nothing more. Real per-caller authentication needs a per-user
/// credential, which is exactly what the personal `rbuapp_` path already is.
///
/// Absent in a build (the env var wasn't set) ⇒ the proxy path returns a LOUD
/// error and the shell falls through to hearham, rather than silently shipping
/// a build whose shared path 403s against a configured Worker.
const CLIENT_KEY: Option<&str> = option_env!("NEXUS_RB_CLIENT_KEY");

/// Fetch one state's export through the Nexus proxy (no RepeaterBook token on
/// this side). Same body/parse contract as [`fetch_state`].
pub fn fetch_state_proxy(state_id: &str) -> Result<String, String> {
    let key = CLIENT_KEY
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .ok_or_else(|| "RepeaterBook: this build has no proxy client key".to_string())?;
    let c = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(UA)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "RepeaterBook: HTTP client initialization failed".to_string())?;
    let url = format!("{PROXY_URL}?state_id={}", state_id.trim());
    let resp = c
        .get(url)
        .header("x-nexus-client", key)
        .send()
        .map_err(redact)?;
    let status = resp.status();
    if status.as_u16() == 429 {
        return Err(ERR_RATE_LIMITED.to_string());
    }
    if status.as_u16() == 503 {
        return Err("RepeaterBook: proxy not activated".to_string());
    }
    if status.as_u16() == 403 {
        // The Worker has a client key configured and ours didn't match — a
        // build/deploy mismatch, not a user-facing condition. Distinct so it
        // isn't mistaken for a RepeaterBook outage.
        return Err("RepeaterBook: proxy rejected this build's client key".to_string());
    }
    if !status.is_success() {
        return Err(format!(
            "RepeaterBook: proxy returned HTTP {}",
            status.as_u16()
        ));
    }
    resp.text()
        .map_err(|_| "RepeaterBook: could not read the response body".to_string())
}

/// Fetch one state's repeater export (JSON body, parsed by
/// [`crate::repeaters::parse_repeaterbook_json`]). `state_id` is a US FIPS code
/// from [`crate::repeaters::state_id_for`].
pub fn fetch_state(token: &str, state_id: &str) -> Result<String, String> {
    let c = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(UA)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "RepeaterBook: HTTP client initialization failed".to_string())?;
    let url = format!(
        "https://www.repeaterbook.com/api/export.php?state_id={}",
        state_id.trim()
    );
    let resp = c
        .get(url)
        .header("X-RB-App-Token", token.trim())
        .send()
        .map_err(redact)?;
    let status = resp.status();
    if status.as_u16() == 429 {
        return Err(ERR_RATE_LIMITED.to_string());
    }
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(
            "RepeaterBook: token rejected — check the token in Settings ▸ Integrations".to_string(),
        );
    }
    if !status.is_success() {
        return Err(format!(
            "RepeaterBook: server returned HTTP {}",
            status.as_u16()
        ));
    }
    resp.text()
        .map_err(|_| "RepeaterBook: could not read the response body".to_string())
}

/// Category-only error mapping — the request carries the token in a header, so
/// we never stringify the transport error (qrz.rs discipline).
fn redact(e: reqwest::Error) -> String {
    neterr::redact("RepeaterBook", &e)
}
