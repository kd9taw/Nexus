//! The ARRL/RAC section universe, pinned against the SPONSOR'S OWN CURRENT LIST.
//!
//! ## The source, and why it is pinned here rather than reasoned about
//!
//! Two ARRL-published lists, both read **2026-09-09**, agree exactly:
//!
//! - <https://www.arrl.org/section-abbreviations> — ARRL's own section table, grouped
//!   by US call-sign area 1–0 plus a Canadian block.
//! - <https://www.arrl.org/files/file/Field-Day/Generic/ARRL-RAC%20Section%20List.pdf>
//!   — the generic **Field Day** section package, footer *"Revised 2025"*.
//!
//! Both carry **85** W/VE sections: 71 US + 14 RAC. The seed shipped 83, from a
//! pre-2017 era, and an operator in one of the five sections it lacked could not
//! select their own section at all.
//!
//! ## The three retirements, in ARRL's own words
//!
//! The Field Day PDF prints the transitions inline, which is why two of the three can
//! be migrated without asking anybody:
//!
//! - `Golden Horseshoe   GH (formerly GTA)` — a **rename**. Same territory.
//! - `Territories        TER (formerly NT)` — a **rename**. Same territory. (The
//!   PDF's footnote on the HTML page: *"Includes Yukon (VY1), Northwest Territories
//!   (VE8), and Nunavut (VY0)."*)
//! - `MAR` (Maritime) simply **is not on either list**, and `NB` / `NS` / `PE` are,
//!   as three separate entries. ARRL publishes no `MAR → x` alias, because there is
//!   no single successor: the section SPLIT. See [`fd_rules::retired_section`] and
//!   the migration tests in `tempo-app` for what that costs a stored `MAR`.
//!
//! ## Why the two lists in the seed had to be reconciled
//!
//! Batch 9 shipped Sweepstakes with its own `ss_sections` domain — the sponsor's
//! current 85, parsed off `contestmultipliers.php?a=wve` — and recorded in the seed's
//! own provenance that the top-level `sections` array disagreed with it. One seed file
//! carrying two different answers to "what is an ARRL section" is the drift this file
//! exists to make impossible: [`the_field_day_and_sweepstakes_section_lists_agree`]
//! compares them as sets, so neither can move without the other.

use std::collections::BTreeSet;

use tempo_core::fd_rules::{self, CURRENT_RULES_YEAR};

/// The 85 W/VE section codes, transcribed from ARRL's own two lists (above) in the
/// sponsor's order: US call-sign areas 1 through 0, then the Canadian block.
///
/// ⚠️ **This is a transcription of a published list, not a derivation.** It must never
/// be computed from `sections()` — a test that builds its expectation out of the thing
/// under test proves only that the thing equals itself.
const ARRL_85: [&str; 85] = [
    // Call Sign Area 1
    "CT", "EMA", "ME", "NH", "RI", "VT", "WMA", // Call Sign Area 2
    "ENY", "NLI", "NNJ", "NNY", "SNJ", "WNY", // Call Sign Area 3
    "DE", "EPA", "MDC", "WPA", // Call Sign Area 4
    "AL", "GA", "KY", "NC", "NFL", "SC", "SFL", "WCF", "TN", "VA", "PR", "VI",
    // Call Sign Area 5
    "AR", "LA", "MS", "NM", "NTX", "OK", "STX", "WTX", // Call Sign Area 6
    "EB", "LAX", "ORG", "SB", "SCV", "SDG", "SF", "SJV", "SV", "PAC",
    // Call Sign Area 7
    "AZ", "EWA", "ID", "MT", "NV", "OR", "UT", "WWA", "WY", "AK", // Call Sign Area 8
    "MI", "OH", "WV", // Call Sign Area 9
    "IL", "IN", "WI", // Call Sign Area 0
    "CO", "IA", "KS", "MN", "MO", "NE", "ND", "SD", // Canada (RAC)
    "NL", "NB", "NS", "PE", "QC", "ONE", "ONN", "ONS", "GH", "MB", "SK", "AB", "BC", "TER",
];

/// The codes ARRL's current lists no longer carry.
const RETIRED: [&str; 3] = ["MAR", "GTA", "NT"];

fn seed_codes() -> BTreeSet<&'static str> {
    fd_rules::sections().iter().map(|s| s.code).collect()
}

#[test]
fn the_section_universe_is_the_sponsors_current_85() {
    let want: BTreeSet<&str> = ARRL_85.iter().copied().collect();
    assert_eq!(want.len(), 85, "the transcription itself has no duplicate");
    let got = seed_codes();
    // Name the drifted codes rather than only the counts — "83 != 85" does not say
    // which operator cannot select their section.
    let missing: Vec<&str> = want.difference(&got).copied().collect();
    let extra: Vec<&str> = got.difference(&want).copied().collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "section universe drifted from ARRL's published list — missing {missing:?}, \
         carries {extra:?} that ARRL does not publish"
    );
    assert_eq!(fd_rules::sections().len(), 85, "71 US + 14 RAC");
}

#[test]
fn the_three_retired_codes_are_gone_and_their_successors_are_present() {
    let got = seed_codes();
    for gone in RETIRED {
        assert!(
            !got.contains(gone),
            "{gone} was retired by ARRL and must not be offered as a section"
        );
    }
    // The five that 83 lacked: the Maritime split plus the two renames.
    for now in ["NB", "NS", "PE", "GH", "TER"] {
        assert!(got.contains(now), "{now} is a current ARRL/RAC section");
    }
}

#[test]
fn every_section_carries_a_name_and_a_division_and_no_code_repeats() {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for s in fd_rules::sections() {
        assert!(
            !s.code.is_empty() && s.code == s.code.to_ascii_uppercase(),
            "{:?} is not a canonical code",
            s.code
        );
        assert!(
            !s.name.is_empty() && !s.division.is_empty(),
            "{} misses a name or a division",
            s.code
        );
        assert!(seen.insert(s.code), "duplicate section code {}", s.code);
    }
    assert_eq!(seen.len(), 85);
}

/// ⭐ **The two lists in one seed file, held equal.**
///
/// The top-level `sections` array feeds Field Day; the `ss_sections` domain feeds
/// Sweepstakes. They are the same real-world fact — the W/VE section universe — and
/// batch 9 shipped them disagreeing by six codes. Nothing in the loader made that
/// impossible, so this does: a section added, dropped or respelled on one side and not
/// the other fails here, naming the difference.
#[test]
fn the_field_day_and_sweepstakes_section_lists_agree() {
    let rs = fd_rules::ruleset_by_id("arrlss_cw", CURRENT_RULES_YEAR).expect("Sweepstakes ships");
    let sec = rs
        .exchange
        .field("SEC")
        .expect("Sweepstakes sends a SEC slot");
    let tempo_core::contest::FieldKind::Enum { domain } = sec.kind else {
        panic!("SEC is an enum over the section list")
    };
    let ss: BTreeSet<&str> = domain.values.iter().map(|(code, _)| *code).collect();
    let fd = seed_codes();
    let only_ss: Vec<&str> = ss.difference(&fd).copied().collect();
    let only_fd: Vec<&str> = fd.difference(&ss).copied().collect();
    assert!(
        only_ss.is_empty() && only_fd.is_empty(),
        "one seed file, two section universes: ss_sections alone has {only_ss:?}, \
         the sections array alone has {only_fd:?}"
    );
}

/// ⭐ **Rename versus split — the distinction the whole migration rests on.**
///
/// `GTA` and `NT` carry ARRL's own published alias (*"GH (formerly GTA)"*, *"TER
/// (formerly NT)"*), so `rename_target` answers and `Settings::load` applies it without
/// asking. `MAR` carries three successors and therefore answers `None`: the Maritime
/// section split, and choosing one of three provinces for an operator would put a section
/// they never picked into the exchange they transmit.
#[test]
fn the_retired_codes_map_to_what_arrl_published_and_a_split_maps_to_nothing() {
    let want: [(&str, &str, &[&str], Option<&str>); 3] = [
        ("GTA", "Greater Toronto Area", &["GH"], Some("GH")),
        ("NT", "Northern Territories", &["TER"], Some("TER")),
        ("MAR", "Maritime", &["NB", "NS", "PE"], None),
    ];
    for (code, name, successors, rename) in want {
        let r = fd_rules::retired_section(code)
            .unwrap_or_else(|| panic!("{code} was retired by ARRL and must be recognised"));
        assert_eq!(r.name, name, "{code}");
        assert_eq!(r.successors, successors, "{code}");
        assert_eq!(
            r.rename_target(),
            rename,
            "{code}: a rename is applied silently, a split must be asked"
        );
    }
    // Normalised like `valid_section`, so a hand-edited settings.json migrates too.
    assert_eq!(
        fd_rules::retired_section(" gta ").map(|r| r.code),
        Some("GTA")
    );
}

/// The two lists must not overlap, in either direction.
///
/// A code that is both current and retired would migrate an operator off a section that
/// still exists; a successor that is not a current section would migrate them onto one
/// that does not. Both are silent, and both end in an unsubmittable log.
#[test]
fn retired_codes_are_not_current_and_every_successor_is() {
    let current = seed_codes();
    for code in RETIRED {
        let r = fd_rules::retired_section(code).expect("listed above");
        assert!(
            !current.contains(code),
            "{code} is offered as a section AND marked retired"
        );
        for s in r.successors {
            assert!(
                current.contains(s),
                "{code} migrates to {s}, which is not a section this build offers"
            );
        }
    }
    // The control, both ways: a live section is not retired, and neither is junk.
    for live in ["WI", "EMA", "GH", "TER", "NB", "NS", "PE"] {
        assert!(
            fd_rules::retired_section(live).is_none(),
            "{live} is current, not retired"
        );
    }
    assert!(fd_rules::retired_section("ZZ").is_none());
    assert!(fd_rules::retired_section("").is_none());
}
