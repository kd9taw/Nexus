//! The ordering hazard, made loud (mirror of propagation's cty_init_too_late):
//! anything reading `ruleset()` before the startup install locks the BUNDLED
//! SEED in for the whole session, and a later `install_from` must say so with
//! `AlreadyInitialized` — never silently half-apply. Own process (D7); this is
//! also fd_rules_install.rs's control: the same edited file that changed the
//! score there changes nothing here, proving that test could tell the
//! difference.

use tempo_core::fd_rules::{self, RulesInitError};
use tempo_core::fieldday::{Exchange, FdEvent, FieldDayLog};

const SEED: &str = include_str!("../src/fd_rules.seed.json");

#[test]
fn a_read_before_install_locks_the_seed_in_loudly() {
    // Something reads the ruleset first (the engine's first snapshot, say).
    let rs = fd_rules::ruleset(FdEvent::ArrlFd, fd_rules::CURRENT_RULES_YEAR);
    assert_eq!(rs.rules_year, 2026);

    // The same valid, newer, points-edited file fd_rules_install.rs installs…
    let mut spec: serde_json::Value = serde_json::from_str(SEED).unwrap();
    spec["rulesets"][0]["points_by_mode_class"]["PH"] = 3.into();
    spec["generated"] = "2026-12-31T00:00:00Z".into();
    // …is refused with the error that names a code-ordering regression.
    assert_eq!(
        fd_rules::install_from(&spec.to_string()),
        Err(RulesInitError::AlreadyInitialized)
    );

    // And the SEED is what scores: one phone QSO is worth 1, not 3.
    let mut log = FieldDayLog::new("W9XYZ", Exchange::new("3A", "WI"), "20m");
    assert!(log.log_mode_at("K1ABC", "2A", "IL", "PH", 0, 100));
    let (qso_pts, _) = rs.scoring.qso_and_powered(&log, 1);
    assert_eq!(
        qso_pts, 1,
        "the seed's phone points, untouched by the refusal"
    );
    assert_eq!(fd_rules::active_generated(), fd_rules::seed_generated());
}
