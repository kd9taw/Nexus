//! `fd_rules::install_from` on a downloaded file stamped EXACTLY like the bundled seed: the
//! download is the one used. Equal is allowed on purpose (`install_from`'s own note: an
//! upgrade can bundle exactly the file already downloaded), and it is the half of the seed
//! floor that makes a seed's `generated` stamp load-bearing — a seed that adds a contest
//! without moving its stamp loses to every download of the file before it. See
//! fd_rules_install.rs for the older-loses half and the upgrade case.
//!
//! Own process (D7): `install_from` sets a process-wide `OnceLock`.

use tempo_core::contest::PointsRule;
use tempo_core::fd_rules;
use tempo_core::fieldday::FdEvent;

const SEED: &str = include_str!("../src/fd_rules.seed.json");

#[test]
fn a_downloaded_file_with_the_seeds_own_stamp_is_the_one_used() {
    let mut spec: serde_json::Value = serde_json::from_str(SEED).unwrap();
    assert_eq!(
        spec["generated"].as_str(),
        Some(fd_rules::seed_generated()),
        "fixture anchor: the candidate carries the seed's own stamp"
    );
    // A difference the installed table can show: ARRL Field Day's phone points, 1 → 3.
    assert_eq!(spec["rulesets"][0]["event"], "arrlfd", "fixture anchor");
    assert_eq!(
        spec["rulesets"][0]["scoring"]["points_by_mode_class"]["PH"],
        1
    );
    spec["rulesets"][0]["scoring"]["points_by_mode_class"]["PH"] = 3.into();

    let stats = fd_rules::install_from(&spec.to_string()).expect("an equal stamp installs");
    assert_eq!(stats.generated, fd_rules::seed_generated());
    let rs = fd_rules::ruleset(FdEvent::ArrlFd, fd_rules::CURRENT_RULES_YEAR);
    match rs.scoring.qso_points {
        PointsRule::ByModeClass(p) => assert_eq!(p.ph, 3, "the downloaded table is the one used"),
        other => panic!("ARRL Field Day prices by mode class, got {other:?}"),
    }
}
