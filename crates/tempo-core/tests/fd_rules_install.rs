//! `fd_rules::install_from` — the positive control for the whole rules-as-data
//! change: a downloaded file with a DIFFERENT points value must provably change
//! a computed score (otherwise the "data" is decoration and the loader a
//! no-op). Integration test on purpose (D7): `install_from` sets a process-wide
//! `OnceLock`, so it can only be exercised in its own process — a unit test
//! would poison every other test's view of the global. One `#[test]` fn, run
//! strictly in order: the reject cases must precede the successful install
//! (they never touch the global), and nothing may call `ruleset()` before the
//! install (that would seed-lock the table — see fd_rules_too_late.rs for that
//! path, and for the proof that THIS test could tell the difference).

use tempo_core::contest::ContestSession;
use tempo_core::fd_rules::{self, RulesInitError};
use tempo_core::fieldday::{FdEvent, FieldDayLog};

const SEED: &str = include_str!("../src/fd_rules.seed.json");

fn pinned_log() -> FieldDayLog {
    // The in-crate pinned fixture's shape: 4 PH + 3 CW + 3 DIG, distinct calls.
    let mut log = FieldDayLog::new(
        "W9XYZ",
        ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
        "20m",
    );
    for (i, (call, mode)) in [
        ("PH1AA", "PH"),
        ("PH2AA", "PH"),
        ("PH3AA", "PH"),
        ("PH4AA", "PH"),
        ("CW1AA", "CW"),
        ("CW2AA", "CW"),
        ("CW3AA", "CW"),
        ("DG1AA", "DIG"),
        ("DG2AA", "DIG"),
        ("DG3AA", "DIG"),
    ]
    .iter()
    .enumerate()
    {
        assert!(log.log_mode_at(call, "2A", "IL", mode, 0, 100 + i as u64));
    }
    log
}

#[test]
fn an_installed_file_with_changed_points_changes_the_computed_score() {
    // -- Rejects first (none touches the global table). --------------------
    assert!(
        matches!(
            fd_rules::install_from("not json"),
            Err(RulesInitError::Invalid(_))
        ),
        "garbage is refused"
    );
    // A schema-1 file is one a PREVIOUS build published; this build reads
    // schema 2 and refuses it without touching the global table.
    let mut wrong_schema: serde_json::Value = serde_json::from_str(SEED).unwrap();
    wrong_schema["schema"] = 1.into();
    assert!(
        matches!(
            fd_rules::install_from(&wrong_schema.to_string()),
            Err(RulesInitError::Invalid(_))
        ),
        "schema 1 is refused"
    );
    // Seed floor: a valid file OLDER than the bundled seed loses to it.
    let mut older: serde_json::Value = serde_json::from_str(SEED).unwrap();
    older["generated"] = "2020-01-01T00:00:00Z".into();
    assert!(
        matches!(
            fd_rules::install_from(&older.to_string()),
            Err(RulesInitError::OlderThanSeed { .. })
        ),
        "an older generated stamp loses to the seed"
    );

    // -- The install: seed with SFD phone points edited 1 → 3. -------------
    let mut spec: serde_json::Value = serde_json::from_str(SEED).unwrap();
    assert_eq!(spec["rulesets"][0]["event"], "arrlfd", "fixture anchor");
    assert_eq!(
        spec["rulesets"][0]["scoring"]["points_by_mode_class"]["PH"], 1,
        "the seed's phone points are 1 — the edit below is a real change"
    );
    spec["rulesets"][0]["scoring"]["points_by_mode_class"]["PH"] = 3.into();
    // …and a multiplier, which no SHIPPED ruleset declares. `fd_rules`'s own
    // `no_shipped_ruleset_declares_a_multiplier` asserts both seeded events
    // carry an empty list; this is the positive control for that pair — without
    // it, a loader that dropped the block on the floor would look identical.
    assert_eq!(
        spec["rulesets"][0]["scoring"]["multipliers"],
        serde_json::json!([]),
        "the seed declares no multiplier — the block below is a real addition"
    );
    spec["rulesets"][0]["scoring"]["multipliers"] = serde_json::json!([{
        "id": "section",
        "source": { "type": "field", "key": "SECTION", "domain": "fd_sections" },
        "scope": "per_band",
        "excluding": ["DX"],
        "roles": [""],
    }]);
    spec["generated"] = "2026-12-31T00:00:00Z".into();
    let stats = fd_rules::install_from(&spec.to_string()).expect("valid file installs");
    assert_eq!(stats.generated, "2026-12-31T00:00:00Z");
    assert_eq!(stats.sections, 85);

    // -- The proof: the fetched data reaches the scoring math. -------------
    // Seed scores this log 16 QSO pts (4×1 + 6×2, the in-crate pinned
    // fixture); with PH worth 3 it must score 4×3 + 6×2 = 24.
    let rs = fd_rules::ruleset(FdEvent::ArrlFd, fd_rules::CURRENT_RULES_YEAR);
    let log = pinned_log();
    let (qso_pts, powered) = rs.scoring.qso_and_powered(log.score_rows(), 2);
    assert_eq!(qso_pts, 24, "the installed points table scored the log");
    assert_ne!(
        qso_pts, 16,
        "…and it provably differs from the seed's score"
    );
    assert_eq!(powered, 48, "power tier still multiplies the new points");
    assert_eq!(fd_rules::active_generated(), "2026-12-31T00:00:00Z");

    // -- …and a declared multiplier reaches the built ruleset intact. -------
    // The whole of `MultiplierRule` survives the leak-once build: its id, both
    // halves of the field source, the scope, the exclusion list and the role
    // filter. Nothing SCORES it yet (§11.2 — it lands unused), so this walk is
    // the only thing standing between a typo in `build` and a batch-10 contest
    // silently counting the wrong universe.
    assert_eq!(
        rs.scoring.multipliers,
        &[tempo_core::contest::MultiplierRule {
            id: "section",
            source: tempo_core::contest::MultSource::Field {
                key: "SECTION",
                domain: Some("fd_sections"),
            },
            scope: tempo_core::contest::MultScope::PerBand,
            excluding: &["DX"],
            roles: &[""],
        }],
        "the installed multiplier must reach the ruleset"
    );
    // The scoring MATH is untouched by its presence: 24/48 above is the same
    // pair a multiplier-free file produced, because nothing evaluates one yet.
    assert_eq!((qso_pts, powered), (24, 48));

    // -- A second install is the loud ordering error, not a swap. ----------
    assert!(
        matches!(
            fd_rules::install_from(&spec.to_string()),
            Err(RulesInitError::AlreadyInitialized)
        ),
        "the table is set-once — no live swap"
    );
}
