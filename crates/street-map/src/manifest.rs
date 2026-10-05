//! `streetmaps.json`: the street-map host's index of what it serves.
//!
//! ```json
//! {
//!   "schema": 1,
//!   "build": {
//!     "id": "20261004",
//!     "date": "2026-10-04",
//!     "url": "https://maps.hamradiotools.io/planet-20261004.pmtiles",
//!     "bytes": 138570000000,
//!     "version": "4.15.2",
//!     "upstream": "https://build.protomaps.com/20261004.pmtiles",
//!     "b3sum": "…"
//!   },
//!   "assets": {
//!     "url": "https://maps.hamradiotools.io/street-assets-1.tar.gz",
//!     "sha256": "…64 hex digits…",
//!     "bytes": 11600000
//!   },
//!   "attribution": "© OpenStreetMap",
//!   "license": "ODbL-1.0"
//! }
//! ```
//!
//! `build.url` names one build forever: a refresh adds a new name and retires the old one after
//! an overlap, so a download can never mix two builds. Every URL must be on the street-map host
//! itself; an index that points anywhere else is refused, so a tampered index cannot send Nexus
//! to fetch from another server. Fields not listed are ignored, so the host can add to the file
//! without breaking older installs; a different `schema` is refused.

use serde::Deserialize;

use crate::error::{ErrorKind, StreetError};
use crate::plan::BuildInfo;

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub build: ManifestBuild,
    pub assets: ManifestAssets,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ManifestBuild {
    pub id: String,
    pub date: String,
    pub url: String,
    #[serde(default)]
    pub upstream: Option<String>,
    #[serde(default)]
    pub bytes: Option<u64>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ManifestAssets {
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub bytes: Option<u64>,
}

impl Manifest {
    pub fn build_info(&self) -> BuildInfo {
        BuildInfo {
            id: self.build.id.clone(),
            date: self.build.date.clone(),
            url: self.build.url.clone(),
            upstream: self.build.upstream.clone(),
        }
    }
}

fn refuse(why: String) -> StreetError {
    StreetError::new(ErrorKind::InvalidManifest, why)
}

fn on_host(url: &str, origin: &str) -> bool {
    url.strip_prefix(origin)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|path| {
            !path.is_empty() && path.bytes().all(|b| b.is_ascii_graphic() && b != b'\\')
        })
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

/// Parse and check an index fetched from `origin` (`https://<host>`, no trailing slash).
pub fn parse(bytes: &[u8], origin: &str) -> Result<Manifest, StreetError> {
    let m: Manifest = serde_json::from_slice(bytes)
        .map_err(|e| refuse(format!("streetmaps.json does not parse: {e}")))?;
    if m.schema != SCHEMA {
        return Err(refuse(format!(
            "streetmaps.json schema {} (this Nexus reads schema {SCHEMA})",
            m.schema
        )));
    }
    // The build id becomes part of every pack id, which is limited to [a-z0-9-].
    let id_ok = !m.build.id.is_empty()
        && m.build.id.len() <= 32
        && !m.build.id.starts_with('-')
        && m.build
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !id_ok {
        return Err(refuse(format!("build id {:?}", m.build.id)));
    }
    if !is_date(&m.build.date) {
        return Err(refuse(format!("build date {:?}", m.build.date)));
    }
    for url in [&m.build.url, &m.assets.url] {
        if !on_host(url, origin) {
            return Err(refuse(format!("{url} is not on {origin}")));
        }
    }
    if m.assets.sha256.len() != 64
        || !m
            .assets
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(refuse(
            "the font and icon archive's SHA-256 is not 64 hex digits".into(),
        ));
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://maps.example.org";

    fn manifest(build_url: &str, assets_url: &str) -> String {
        format!(
            r#"{{"schema":1,"later":"ignored","build":{{"id":"20261004","date":"2026-10-04","url":"{build_url}","bytes":5}},
                "assets":{{"url":"{assets_url}","sha256":"{}"}}}}"#,
            "0f".repeat(32)
        )
    }

    #[test]
    fn a_good_index_parses_and_unknown_fields_are_ignored() {
        let m = parse(
            manifest(
                &format!("{ORIGIN}/planet-20261004.pmtiles"),
                &format!("{ORIGIN}/a.tar.gz"),
            )
            .as_bytes(),
            ORIGIN,
        )
        .unwrap();
        assert_eq!(m.build_info().id, "20261004");
        assert_eq!(m.build.bytes, Some(5));
        assert_eq!(m.assets.bytes, None);
    }

    #[test]
    fn an_index_pointing_off_the_host_is_refused() {
        let good = format!("{ORIGIN}/a.tar.gz");
        for bad in [
            "https://evil.example/planet.pmtiles",
            "https://maps.example.org.evil.example/planet.pmtiles",
            "https://maps.example.org@evil.example/planet.pmtiles",
            "http://maps.example.org/planet.pmtiles",
            "https://maps.example.org",
            "https://maps.example.org/",
            "https://maps.example.org/a b.pmtiles",
        ] {
            let e = parse(manifest(bad, &good).as_bytes(), ORIGIN).unwrap_err();
            assert_eq!(e.kind, ErrorKind::InvalidManifest, "{bad}");
            let e = parse(manifest(&good, bad).as_bytes(), ORIGIN).unwrap_err();
            assert_eq!(e.kind, ErrorKind::InvalidManifest, "assets at {bad}");
        }
    }

    #[test]
    fn malformed_fields_are_refused() {
        let ok = manifest(
            &format!("{ORIGIN}/p.pmtiles"),
            &format!("{ORIGIN}/a.tar.gz"),
        );
        assert!(parse(ok.as_bytes(), ORIGIN).is_ok(), "control");
        for (from, to) in [
            ("\"schema\":1", "\"schema\":2"),
            ("\"20261004\"", "\"../x\""),
            ("\"20261004\"", "\"Build_1\""),
            ("\"2026-10-04\"", "\"4 Oct 2026\""),
            (&"0f".repeat(32), &"0F".repeat(32)),
            (&"0f".repeat(32), "abc"),
        ] {
            let bad = ok.replace(from, to);
            assert_ne!(bad, ok);
            assert_eq!(
                parse(bad.as_bytes(), ORIGIN).unwrap_err().kind,
                ErrorKind::InvalidManifest,
                "{to}"
            );
        }
        assert!(parse(b"[]", ORIGIN).is_err());
    }
}
