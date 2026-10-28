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

/// Download `groups.json` (about 300 KB). `Err` on network or HTTP trouble — the caller keeps the
/// copy it holds, or the shape-only check when it holds none.
pub fn fetch_groups() -> Result<String, String> {
    let c = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(UA)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = c
        .get(GROUPS_URL)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    resp.text().map_err(|e| e.to_string())
}
