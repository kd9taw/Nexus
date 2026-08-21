//! Self-update version check — the PURE parse/compare of the release-version feeds.
//!
//! Two sources, tried in order by the shell: our own update endpoint
//! (`hamradiotools.io/nexus/version.json`, a schema-1 doc with a direct `"latest"` field —
//! parsed by [`parse_latest_from_endpoint`]), and, as a fallback, SourceForge's
//! `best_release.json` (the filename-parsing path in [`parse_latest_version`]). The own endpoint
//! is authoritative and GitHub-first; SF is the safety net so a site outage can't disable the check.
//!
//! The HTTP fetch (IO) lives in the Tauri shell; this module stays pure and unit-tested so the
//! version logic is verifiable without a network — and without building `src-tauri`, which the
//! dev environment can't compile. Phase 1 only tells the operator a newer build exists and opens
//! the download page; it never downloads or runs anything.

/// Parse the latest version from the Nexus update endpoint's `version.json` (schema 1): a top-level
/// `"latest": "X.Y.Z"`. Tolerates a leading `v`. Returns `None` if the JSON is unparseable or
/// `latest` isn't a recognizable version triple — the caller then falls back to the SF feed, never
/// a phantom update.
pub fn parse_latest_from_endpoint(json_body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json_body).ok()?;
    let latest = v["latest"].as_str()?.trim().trim_start_matches('v');
    parse_triple(latest).map(|_| latest.to_string())
}

/// Parse the latest Windows release version from a SourceForge `best_release.json` body.
/// Reads `platform_releases.windows.filename`, falling back to `release.filename`, and pulls the
/// `Nexus_X.Y.Z` version out of it (e.g. `/v0.4.1/Nexus_0.4.1_x64-setup.exe` → `"0.4.1"`).
/// Returns `None` if the JSON is unparseable or carries no recognizable Nexus installer name —
/// callers then treat it as "no update info", never a phantom update.
pub fn parse_latest_version(json_body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json_body).ok()?;
    let filename = v["platform_releases"]["windows"]["filename"]
        .as_str()
        .or_else(|| v["release"]["filename"].as_str())?;
    version_from_filename(filename)
}

/// Extract `"0.3.0"` from a filename containing `Nexus_0.3.0_…`. `None` if absent/malformed.
fn version_from_filename(filename: &str) -> Option<String> {
    // Try EVERY "Nexus_" occurrence (a parent dir could also carry the token) and take the first
    // that yields a real version.
    filename.split("Nexus_").skip(1).find_map(|after| {
        // Leading run of digits and dots (stops at the '_' before "x64", the '-' before "beta", …).
        let ver: String = after
            .chars()
            .take_while(|c| *c == '.' || c.is_ascii_digit())
            .collect();
        // "Nexus_0.4.1.exe" (no "_x64" separator) leaves a trailing dot — trim before parsing.
        let ver = ver.trim_matches('.');
        parse_triple(ver).map(|_| ver.to_string())
    })
}

/// `"1.2.3"` → `(1, 2, 3)`. Accepts 1–3 dotted numeric parts (missing parts are 0); rejects
/// empty, non-numeric, or 4+-part strings so a junk capture never compares as a real version.
fn parse_triple(v: &str) -> Option<(u32, u32, u32)> {
    if v.is_empty() {
        return None;
    }
    let mut it = v.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next().unwrap_or("0").parse().ok()?;
    let c = it.next().unwrap_or("0").parse().ok()?;
    if it.next().is_some() {
        return None; // more than 3 parts — not a version we recognize
    }
    Some((a, b, c))
}

/// A comparable version: the numeric triple, then RELEASE-BEATS-PRERELEASE, then the
/// prerelease tag itself so `-test2` follows `-test1`.
///
/// ⚠️ WHY THE PRERELEASE ARM EXISTS AT ALL. Tester builds take a suffix (`1.7.1-test1`) rather
/// than eating a public patch number, so a tester is never left holding a version whose number
/// a later public build reuses with different contents. Without this arm the rule backfired:
/// `parse_triple` rejected the suffixed string, and an unparseable CURRENT version makes
/// `version_is_newer` answer false for every candidate — the tester would be offered nothing,
/// ever. A rule the comparator cannot read is not a rule.
///
/// This is semver's precedence for the shapes we ship, not a semver implementation: build
/// metadata (`+sha`) is not handled because nothing here produces it, and prerelease tags are
/// compared as plain strings rather than as dot-separated identifiers.
fn parse_version(v: &str) -> Option<(u32, u32, u32, u8, String)> {
    let (core, pre) = match v.split_once('-') {
        Some((core, pre)) if !pre.is_empty() => (core, Some(pre)),
        Some(_) => return None, // "1.7.1-" — a truncated tag, not a version
        None => (v, None),
    };
    let (a, b, c) = parse_triple(core)?;
    // 0 sorts a prerelease BELOW the release of the same number; 1 is the release itself.
    Some(match pre {
        Some(tag) => (a, b, c, 0, tag.to_string()),
        None => (a, b, c, 1, String::new()),
    })
}

/// True only when `latest` is a strictly newer version than `current`, compared NUMERICALLY
/// (so 0.10.0 > 0.9.0, which a lexical string compare gets wrong). Either side being unparseable
/// yields false — never nag the operator over a version string we don't understand.
pub fn version_is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shape mirrors the live SF response: `release` + per-platform entries, mac/linux null.
    const SAMPLE: &str = r#"{
        "release": {"filename": "/v0.3.0-beta/Nexus_0.3.0_x64-setup.exe"},
        "platform_releases": {
            "windows": {"filename": "/v0.4.1/Nexus_0.4.1_x64-setup.exe"},
            "mac": null,
            "linux": null
        }
    }"#;

    // A TESTER BUILD MUST STILL BE OFFERED THE PUBLIC RELEASE THAT SUPERSEDES IT.
    //
    // The versioning rule says tester builds take a prerelease suffix (1.7.1-test1) rather
    // than eating a public patch number, precisely so the holder is never stranded on a
    // version that shares a number with different content. But `parse_triple` split on '.'
    // and parsed each part as a u32, so "1.7.1-test1" failed on its third part, and an
    // unparseable CURRENT version makes `version_is_newer` false for EVERYTHING — the tester
    // would never be offered another build again. The rule and the comparator disagreed, and
    // the comparator wins on the operator's machine.
    #[test]
    fn a_prerelease_is_older_than_its_release_and_newer_than_the_patch_below() {
        assert!(
            version_is_newer("1.7.1", "1.7.1-test1"),
            "the public release supersedes it"
        );
        assert!(version_is_newer("1.7.2", "1.7.1-test1"));
        assert!(version_is_newer("1.8.0", "1.7.1-test1"));
        assert!(
            !version_is_newer("1.7.0", "1.7.1-test1"),
            "an older public build is not an update"
        );
        assert!(
            !version_is_newer("1.7.1-test1", "1.7.1"),
            "a tester build never supersedes a release"
        );
        // Successive tester builds on the same machine.
        assert!(version_is_newer("1.7.1-test2", "1.7.1-test1"));
        assert!(!version_is_newer("1.7.1-test1", "1.7.1-test2"));
        // Control: the ordinary cases still hold, including the numeric one this exists for.
        assert!(version_is_newer("0.10.0", "0.9.0"));
        assert!(!version_is_newer("1.7.0", "1.7.0"));
        assert!(!version_is_newer("garbage", "1.7.0"));
    }

    #[test]
    fn parses_the_windows_installer_version() {
        assert_eq!(parse_latest_version(SAMPLE), Some("0.4.1".to_string()));
    }

    #[test]
    fn falls_back_to_release_filename_when_windows_is_null() {
        let j = r#"{"release":{"filename":"Nexus_0.3.0_x64-setup.exe"},
                    "platform_releases":{"windows":null,"mac":null,"linux":null}}"#;
        assert_eq!(parse_latest_version(j), Some("0.3.0".to_string()));
    }

    #[test]
    fn version_from_filename_survives_odd_names() {
        assert_eq!(
            version_from_filename("Nexus_0.4.1.exe"),
            Some("0.4.1".into())
        ); // trailing dot
        assert_eq!(
            version_from_filename("/Nexus_Setup/Nexus_0.4.1_x64-setup.exe"),
            Some("0.4.1".into()) // parent dir also has "Nexus_"
        );
        assert_eq!(
            version_from_filename("/v0.4.1-beta/Nexus_0.4.1_x64-setup.exe"),
            Some("0.4.1".into())
        );
        assert_eq!(version_from_filename("readme.txt"), None);
        assert_eq!(version_from_filename("Nexus_setup.exe"), None); // "Nexus_" but no version
    }

    #[test]
    fn none_on_garbage_or_a_non_nexus_filename() {
        assert_eq!(parse_latest_version("not json"), None);
        assert_eq!(
            parse_latest_version(r#"{"release":{"filename":"readme.txt"}}"#),
            None
        );
        assert_eq!(parse_latest_version("{}"), None);
    }

    // The live endpoint shape (schema 1): a direct `latest`, plus downloads/mirrors we ignore here.
    const ENDPOINT_SAMPLE: &str = r#"{
        "schema": 1,
        "latest": "0.11.1",
        "downloads": {"windows": {"url": "…"}, "linuxAppimage": {"url": "…"}},
        "mirrors": {"github": "…", "sourceforge": "…"}
    }"#;

    #[test]
    fn parses_latest_from_endpoint_json() {
        assert_eq!(
            parse_latest_from_endpoint(ENDPOINT_SAMPLE),
            Some("0.11.1".to_string())
        );
        // tolerate a leading "v"
        assert_eq!(
            parse_latest_from_endpoint(r#"{"latest":"v0.12.0"}"#),
            Some("0.12.0".to_string())
        );
    }

    #[test]
    fn endpoint_none_on_missing_or_bad_latest() {
        assert_eq!(parse_latest_from_endpoint("not json"), None);
        assert_eq!(parse_latest_from_endpoint(r#"{"schema":1}"#), None); // no `latest`
        assert_eq!(parse_latest_from_endpoint(r#"{"latest":"soon"}"#), None); // not a version
        assert_eq!(parse_latest_from_endpoint(r#"{"latest":123}"#), None); // wrong type
    }

    #[test]
    fn newer_is_numeric_not_lexical() {
        assert!(version_is_newer("0.4.0", "0.3.0"));
        assert!(version_is_newer("0.10.0", "0.9.0")); // lexical would wrongly say 0.10 < 0.9
        assert!(version_is_newer("1.0.0", "0.9.9"));
        assert!(!version_is_newer("0.3.0", "0.3.0")); // equal is not newer
        assert!(!version_is_newer("0.2.9", "0.3.0"));
        assert!(!version_is_newer("garbage", "0.3.0")); // never nag on an unparseable version
        assert!(!version_is_newer("0.4.0", "junk"));
    }
}
