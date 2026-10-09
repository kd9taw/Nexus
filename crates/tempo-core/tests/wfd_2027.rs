//! ⭐ **Winter Field Day 2027, as its sponsor wrote it.**
//!
//! Every number here is the sponsor's own, from the 2027 rules PDF (*"Version 3, 04 OCT
//! 2026"*, <https://winterfieldday.org/downloads/2027-rules-v3.pdf>, read 2026-10-08), cited
//! by page. Every callsign is an example call.
//!
//! The shared fixture is one station, W9XYZ sending `3O WI`, with three contacts: CW K1ABC on
//! 20 m, phone N0XYZ on 40 m and RTTY N7OUT on 80 m — 2 + 1 + 2 = **5 QSO points** (p.5:
//! *"Phone contacts count as one point each, and all CW and digital modes count as two
//! points each"*).
use tempo_core::contest::{CabrilloEntrant, ContestSession, StationData};
use tempo_core::fd_rules::{active_rules_year, ruleset, ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::{FdEvent, FieldDayLog};

/// Sat 23 January 2027, 18:00Z: two hours into the event (p.3: *"starts at 1600 UTC on
/// Saturday … the 23rd"*).
const DURING: u64 = 1_800_727_200;

/// The station, as `Engine::set_mode` reads it out of Settings.
fn station(class: &str, power: &str) -> StationData {
    StationData {
        mycall: "W9XYZ".into(),
        mygrid: "EN52".into(),
        fd_class: class.into(),
        fd_section: "WI".into(),
        contest_category_power: power.into(),
        ..Default::default()
    }
}

/// A session built the way mode entry builds one: `for_ruleset` over the station data.
fn session(event: &str, class: &str, power: &str) -> ContestSession {
    let rs = ruleset_by_id(event, CURRENT_RULES_YEAR).expect("a shipped ruleset");
    ContestSession::for_ruleset(rs, &station(class, power)).expect("the session starts")
}

/// The shared fixture's log under `event`. The worked stations send classes legal for that
/// event, so a control under ARRL Field Day is the same three contacts.
fn fixture(event: &str, power: &str) -> FieldDayLog {
    let (mine, theirs) = if event == "wfd" {
        ("3O", ["2O", "1H", "4I"])
    } else {
        ("3A", ["2A", "1D", "4A"])
    };
    let mut log = FieldDayLog::new("W9XYZ", session(event, mine, power), "20m");
    for ((call, section, band, mode, submode), class) in [
        ("K1ABC", "CT", "20m", "CW", ""),
        ("N0XYZ", "MN", "40m", "PH", ""),
        ("N7OUT", "AZ", "80m", "DIG", "RTTY"),
    ]
    .into_iter()
    .zip(theirs)
    {
        log.band = band.into();
        assert!(
            log.log_submode_at(call, class, section, mode, submode, 0, DURING),
            "{call} logs under {event}"
        );
    }
    log
}

/// ⭐ **The WFD ruleset is the 2027 one** — and the newest rules year is 2027, which is what
/// silences January's "Rules data is from 2026 — check for updates" hint on a station that
/// has the update.
#[test]
fn wfd_rules_are_2027() {
    // One comparison, so a red names both values rather than only the first to flip.
    assert_eq!(
        (
            ruleset(FdEvent::WinterFd, CURRENT_RULES_YEAR).rules_year,
            active_rules_year()
        ),
        (2027, 2027),
        "(the WFD ruleset's year, the newest rules year): a station holding these rules \
         would otherwise be told in January 2027 that its data is old"
    );
}

/// ⭐ **Winter Field Day runs its own objectives and carries no copy of ARRL's bonus menu.**
///
/// p.5: *"an Objective Multiplier (OM) has been assigned to each objective"*; p.7: *"Total
/// score = (total QSO points) x (OM+1)"*. An ARRL bonus is not a WFD objective, so ticking
/// one under WFD scores nothing.
#[test]
fn wfd_carries_no_bonus_menu() {
    let rs = ruleset(FdEvent::WinterFd, CURRENT_RULES_YEAR);
    assert_eq!(rs.bonuses.len(), 0, "WFD's bonus menu: {:?}", rs.bonuses);
    assert_eq!(
        rs.bonus_points(&["emergency-power".to_string()]),
        0,
        "an ARRL bonus ticked under Winter Field Day"
    );
    // CONTROL: ARRL Field Day keeps its menu, so the zero above is WFD's, not the lookup's.
    assert_eq!(
        ruleset(FdEvent::ArrlFd, CURRENT_RULES_YEAR).bonus_points(&["emergency-power".to_string()]),
        100
    );
}

/// ⭐ **The WFD Cabrillo names the entrant the way the sponsor's example does** (p.10:
/// `CLUB`, `OPERATORS`, `NAME` and `EMAIL` in the header).
#[test]
fn wfd_cabrillo_names_the_entrant() {
    let me = CabrilloEntrant {
        name: "Test Operator".into(),
        email: "op@example.org".into(),
        club: "Test Club".into(),
        operators: "KB9QRS".into(),
        ..Default::default()
    };
    let cab = fixture("wfd", "LOW")
        .cabrillo_with(14_000, &me)
        .expect("one entry");
    let missing: Vec<&str> = [
        "NAME: Test Operator\n",
        "EMAIL: op@example.org\n",
        "CLUB: Test Club\n",
        "OPERATORS: KB9QRS\n",
    ]
    .into_iter()
    .filter(|line| !cab.contains(line))
    .collect();
    assert_eq!(missing, Vec::<&str>::new(), "lines missing from:\n{cab}");
    // CONTROL: ARRL Field Day's ruleset lists none of these, so the same entrant writes none.
    let cab = fixture("arrlfd", "LOW")
        .cabrillo_with(14_000, &me)
        .expect("one entry");
    for tag in ["NAME:", "EMAIL:", "CLUB:", "OPERATORS:"] {
        assert!(!cab.contains(tag), "ARRL Field Day wrote {tag}\n{cab}");
    }
}

/// Objective ids as the settings hold them.
fn ticked(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

/// The shared fixture's ticks: 100% alternative power (×2, which brings "station equipment
/// on alternative power", ×1, with it) and QRP (×4) — OM 7.
const TICKED: [&str; 2] = ["wfd-alt-power-100", "wfd-qrp"];

/// ⭐ **The thirteen objectives are the sponsor's, at the sponsor's multipliers** — the
/// worksheet on p.11, OM 1, 2, 3, 1, 2, 3, 1, 1, 6, 6, 2, 4, 2. Two bring another with them:
/// 100% alternative power *"qualifies you for the 'Operate station equipment on alternative
/// power' objective"* (p.6), and of twelve bands *"The six bands from the previous objective
/// count toward this one"* (p.7).
#[test]
fn the_objectives_are_the_sponsors_thirteen() {
    let rs = ruleset(FdEvent::WinterFd, CURRENT_RULES_YEAR);
    let menu: Vec<(&str, u32)> = rs
        .objective_menu
        .iter()
        .map(|o| (o.id, o.multiplier))
        .collect();
    assert_eq!(
        menu,
        [
            ("wfd-alt-power-equipment", 1),
            ("wfd-alt-power-100", 2),
            ("wfd-away-from-home", 3),
            ("wfd-multiple-antennas", 1),
            ("wfd-sstv-image", 2),
            ("wfd-crossband-repeater", 3),
            ("wfd-winlink-email", 1),
            ("wfd-bulletin", 1),
            ("wfd-six-bands", 6),
            ("wfd-twelve-bands", 6),
            ("wfd-multiple-modes", 2),
            ("wfd-qrp", 4),
            ("wfd-six-hours", 2),
        ]
    );
    let implies: Vec<(&str, Vec<&str>)> = rs
        .objective_menu
        .iter()
        .filter(|o| !o.implies.is_empty())
        .map(|o| (o.id, o.implies.to_vec()))
        .collect();
    assert_eq!(
        implies,
        [
            ("wfd-alt-power-100", vec!["wfd-alt-power-equipment"]),
            ("wfd-twelve-bands", vec!["wfd-six-bands"]),
        ]
    );
}

/// ⭐ **The objective multiplier counts what a ticked objective comes with, once, and the total
/// is the sponsor's formula** — *"Total score = (total QSO points) x (OM+1)"* (p.7): the shared
/// fixture's 5 QSO points × (7 + 1) = 40.
#[test]
fn the_objective_total_is_the_sponsors_formula() {
    let rs = ruleset(FdEvent::WinterFd, CURRENT_RULES_YEAR);
    let om = |ids: &[&str]| rs.objective_multiplier(&ticked(ids));
    let all: Vec<&str> = rs.objective_menu.iter().map(|o| o.id).collect();
    assert_eq!(
        (
            om(&TICKED),
            om(&["wfd-alt-power-100", "wfd-alt-power-equipment", "wfd-qrp"]),
            om(&all),
            om(&["wfd-twelve-bands"]),
            om(&["emergency-power", "satellite"]),
            om(&["not-an-objective"]),
        ),
        (7, 7, 34, 12, 0, 0),
        "(the fixture's ticks, the same with the implied one ticked too, all thirteen, twelve \
         bands alone, two ARRL bonus ids, an unknown id)"
    );
    assert_eq!(
        (
            rs.objective_total(5, &ticked(&TICKED)),
            rs.claimed_total(5, 5, 0, &ticked(&TICKED)),
            rs.objective_total(5, &[]),
        ),
        (40, 40, 5),
        "(the fixture's total, the same through the one formula every surface asks, and \
         nothing ticked: \"The +1 is for participating\")"
    );
    // CONTROL: ARRL Field Day has no objectives, and its claimed total is still its powered
    // points plus its bonuses, whatever objective ids the settings hold.
    let arrl = ruleset(FdEvent::ArrlFd, CURRENT_RULES_YEAR);
    assert_eq!(
        (
            arrl.objective_menu.len(),
            arrl.claimed_total(5, 10, 100, &ticked(&TICKED))
        ),
        (0, 110)
    );
}

/// ⭐ **The WFD Cabrillo claims the sponsor's total** — `CLAIMED-SCORE` is *"your calculated
/// total score including multipliers"* (p.10): the shared fixture's 5 × (7 + 1) = 40.
#[test]
fn wfd_cabrillo_claims_the_objective_total() {
    let me = CabrilloEntrant {
        objectives: ticked(&TICKED),
        ..Default::default()
    };
    let cab = fixture("wfd", "QRP")
        .cabrillo_with(14_000, &me)
        .expect("one entry");
    let claimed = |cab: &str| {
        cab.lines()
            .find_map(|l| l.strip_prefix("CLAIMED-SCORE: "))
            .map(str::to_string)
    };
    // CONTROL: nothing ticked claims the QSO points × 1, and ARRL Field Day, whose rules list
    // no CLAIMED-SCORE, writes none whatever is ticked.
    let bare = fixture("wfd", "QRP")
        .cabrillo_with(14_000, &CabrilloEntrant::default())
        .expect("one entry");
    let arrl = fixture("arrlfd", "QRP")
        .cabrillo_with(14_000, &me)
        .expect("one entry");
    assert_eq!(
        (claimed(&cab), claimed(&bare), claimed(&arrl)),
        (Some("40".to_string()), Some("5".to_string()), None),
        "(ticked, nothing ticked, ARRL Field Day):\n{cab}"
    );
}

/// ⭐ **The WFD header carries the sponsor's X-EXCHANGE and a power it takes** — p.10's example
/// log heads `X-EXCHANGE: 3O` and `CATEGORY-POWER: QRP (≤5W DIG/CW or ≤10W PH) or LOW (≤100W)`.
/// The power is the one Settings declares, written only when it is QRP or LOW: Winter Field Day
/// has no high-power entry (*"All stations are limited to a maximum of 100 Watts PEP"*, p.8), so
/// a station left at HIGH claims nothing rather than a category the sponsor does not have.
#[test]
fn wfd_cabrillo_header_has_the_exchange_and_a_power_the_sponsor_takes() {
    let line = |cab: &str, tag: &str| {
        cab.lines()
            .find_map(|l| l.strip_prefix(tag))
            .map(str::to_string)
    };
    let cab = |event: &str, power: &str| {
        fixture(event, power)
            .cabrillo_with(14_000, &CabrilloEntrant::default())
            .expect("one entry")
    };
    let (qrp, low, high) = (cab("wfd", "QRP"), cab("wfd", "LOW"), cab("wfd", "HIGH"));
    let arrl = cab("arrlfd", "QRP");
    assert_eq!(
        (
            line(&qrp, "X-EXCHANGE: "),
            line(&qrp, "CATEGORY-POWER: "),
            line(&low, "CATEGORY-POWER: "),
            line(&high, "CATEGORY-POWER: "),
            line(&arrl, "X-EXCHANGE: "),
            line(&arrl, "CATEGORY-POWER: "),
        ),
        (
            Some("3O".to_string()),
            Some("QRP".to_string()),
            Some("LOW".to_string()),
            None,
            None,
            None
        ),
        "(X-EXCHANGE, QRP declared, LOW declared, HIGH declared, then ARRL Field Day's two, \
         which its rules ask for neither of):\n{qrp}"
    );
}
