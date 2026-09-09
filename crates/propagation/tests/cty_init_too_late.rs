//! THE ORDERING HAZARD, made loud: if anything resolves a callsign before
//! [`propagation::dxcc::init_from`] runs, the embedded seed is locked in for the whole
//! session (the resolver is a set-once `OnceLock`), and a later `init_from` must fail with
//! `AlreadyInitialized` — never silently succeed, never swap mid-session.
//!
//! Own process on purpose (design D7): each file under `tests/` runs alone, so the
//! resolve-first ordering here cannot leak into `cty_init_wins.rs` and vice versa.
//!
//! This is also the CONTROL for `cty_init_wins.rs`: the same fixture, the same resolve — but
//! without a pre-resolve install the embedded spelling comes back, proving the wins-test's
//! assertion could actually tell the two files apart.

use propagation::dxcc::CtyInitError;

#[test]
fn resolve_before_init_locks_in_the_embedded_file_and_init_from_errors_loudly() {
    // Something resolves first — the feed-thread hazard init_from's call site exists to beat.
    assert_eq!(
        propagation::dxcc::resolve("C91RU")
            .expect("C9 resolves")
            .entity,
        "Mozambique",
        "control: without init_from the EMBEDDED spelling is served"
    );

    // The same valid, newer, renamed file that wins in cty_init_wins.rs…
    let text = include_str!("../data/cty.dat")
        .replace("Mozambique:", "Mozambique X:")
        .replace("=VER20250115", "=VER20991231");
    assert!(
        text.contains("Mozambique X:") && text.contains("=VER20991231"),
        "fixture edit missed"
    );

    // …must now be REFUSED, loudly and distinctly.
    match propagation::dxcc::init_from(&text) {
        Err(CtyInitError::AlreadyInitialized) => {}
        other => panic!("expected AlreadyInitialized, got {other:?}"),
    }

    // And the embedded file stays active — no partial swap, no torn state.
    assert_eq!(
        propagation::dxcc::resolve("C91RU").unwrap().entity,
        "Mozambique"
    );
    assert_eq!(
        propagation::dxcc::active_cty_ver(),
        propagation::dxcc::embedded_ver(),
        "the active ver must still be the embedded one"
    );
}
