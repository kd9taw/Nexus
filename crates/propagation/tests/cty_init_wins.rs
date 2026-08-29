//! POSITIVE CONTROL for the cty.dat refresh pipeline: a downloaded country file installed via
//! [`propagation::dxcc::init_from`] must actually WIN over the embedded seed — every later
//! `resolve()` serves the downloaded file's entities, not the `include_str!` copy.
//!
//! An integration test on purpose: the resolver is a set-once `OnceLock` global, so this MUST
//! run as its own process (each file under `tests/` is one). A unit test calling `init_from`
//! would poison every other unit test's view of the global (design D7).
//!
//! The paired control lives in `cty_init_too_late.rs`: WITHOUT `init_from`, the same resolve
//! returns the embedded spelling — proving this test could tell the difference.

/// The vendored seed, edited the way a real AD1C refresh edits it: one entity renamed and the
/// `=VER` marker advanced. "Mozambique" is deliberately NOT one of the names `init_from`'s
/// spot-validation pins (W1AW → United States, JA1XYZ → Japan — see dxcc.rs), so the renamed
/// fixture still passes validation.
fn renamed_newer_cty() -> String {
    let embedded = include_str!("../data/cty.dat");
    let text = embedded
        .replace("Mozambique:", "Mozambique X:")
        .replace("=VER20250115", "=VER20991231");
    // Fixture sanity: if either replace missed, the assertions below would test nothing.
    assert_ne!(text, embedded, "fixture edit did not change the text");
    assert!(text.contains("Mozambique X:"), "entity rename missed");
    assert!(text.contains("=VER20991231"), "=VER bump missed");
    text
}

#[test]
fn a_downloaded_file_installed_before_first_resolve_wins_over_the_embedded_seed() {
    let text = renamed_newer_cty();

    let stats = propagation::dxcc::init_from(&text).expect("valid newer file installs");
    assert_eq!(stats.ver.as_deref(), Some("20991231"));
    assert!(stats.entities >= 340, "entities: {}", stats.entities);

    // The crux: the RENAMED entity is what resolves — the downloaded file is live, the
    // embedded one is not.
    assert_eq!(
        propagation::dxcc::resolve("C91RU")
            .expect("C9 resolves")
            .entity,
        "Mozambique X",
        "resolve() must serve the installed file, not the embedded seed"
    );

    // Status surfaces: the active ver is the installed file's; the embedded ver is unchanged.
    assert_eq!(
        propagation::dxcc::active_cty_ver().as_deref(),
        Some("20991231")
    );
    assert_eq!(
        propagation::dxcc::embedded_ver().as_deref(),
        Some("20250115"),
        "embedded_ver() must read the seed, never the active file"
    );

    // Everything untouched by the rename still resolves normally through the installed file.
    assert_eq!(
        propagation::dxcc::resolve("W1AW").unwrap().entity,
        "United States"
    );
}
