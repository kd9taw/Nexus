//! ⭐ **The ARRL RTTY Roundup, end to end.**
//!
//! Every expected number comes from the sponsor's own current sources, read 2026-10-26:
//! <https://www.arrl.org/rtty-roundup>, the rules PDF it links
//! (<https://contests.arrl.org/ContestRules/RTTY-RU-Rules.pdf>, "Version 3.0b"), the contest
//! calendar (<https://contests.arrl.org/rttyru/cal/>) and the ARRL Contest Multipliers List.
//! The sentence each assertion rests on is quoted at it.
//!
//! What it has that the other contests do not: an exchange whose second item is a state or
//! province from a W/VE station and a SERIAL from everyone else, multipliers counted once per
//! log while contacts are counted once per band, and only one mode — RTTY.
//!
//! ⚠️ **The country file here is a STUB**, as in `tests/cqww_rtty.rs`: the real resolver lives
//! in a crate that depends on this one. Every callsign is an example call.
use tempo_core::contest::{install_call_resolver, CallLocation, ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

const TABLE: &[(&str, &str, &str)] = &[
    ("KL7", "Alaska", "NA"),
    ("KH6", "Hawaii", "OC"),
    ("KP4", "Puerto Rico", "NA"),
    ("CY9", "St. Paul Island", "NA"),
    ("VE", "Canada", "NA"),
    ("VO", "Canada", "NA"),
    ("VY", "Canada", "NA"),
    ("DL", "Fed. Rep. of Germany", "EU"),
    ("G", "England", "EU"),
    ("JA", "Japan", "AS"),
    ("W", "United States", "NA"),
    ("K", "United States", "NA"),
];

fn place(call: &str) -> Option<CallLocation> {
    let up = call.trim().to_ascii_uppercase();
    let mut best: Option<(usize, &'static str, &'static str)> = None;
    for (p, entity, cont) in TABLE {
        if up.starts_with(p) && best.is_none_or(|(n, _, _)| p.len() > n) {
            best = Some((p.len(), entity, cont));
        }
    }
    best.map(|(_, entity, continent)| CallLocation {
        entity,
        continent,
        cq_zone: None,
    })
}

fn resolver() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        install_call_resolver(place).expect("the first install in this test binary");
    });
}

/// A station: my call and my state or province as Settings holds it.
fn station(mycall: &str, qth: &str) -> StationData {
    StationData {
        mycall: mycall.to_string(),
        contest_qth_state: qth.to_string(),
        contest_category_power: "LOW".to_string(),
        contest_category_assisted: "NON-ASSISTED".to_string(),
        ..Default::default()
    }
}

fn select(st: &StationData) -> ContestSession {
    resolver();
    let rs = ruleset_by_id("arrlrtty", CURRENT_RULES_YEAR).expect("arrlrtty is a shipped ruleset");
    ContestSession::for_ruleset(rs, st).unwrap_or_else(|e| panic!("arrlrtty session refused: {e}"))
}

fn composing(s: &ContestSession) -> Vec<(&'static str, String)> {
    s.my_exchange
        .iter()
        .map(|v| (v.key, v.raw.clone()))
        .collect()
}

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// 2027-01-02 18:00:00Z, the next running's first second.
const START: u64 = 1_798_912_800;

/// Log one contact on a band in `submode` (`RTTY` for every contact the contest takes),
/// `min` minutes into the contest.
fn work_in(
    log: &mut FieldDayLog,
    band: &str,
    call: &str,
    ex: &[(&str, &str)],
    class: &str,
    submode: &str,
    min: u64,
) -> bool {
    log.band = band.to_string();
    log.log_fields_at(call, &fields(ex), class, submode, 0, START + min * 60)
}

fn work(log: &mut FieldDayLog, band: &str, call: &str, ex: &[(&str, &str)], min: u64) -> bool {
    work_in(log, band, call, ex, "DIG", "RTTY", min)
}

fn score(log: &FieldDayLog) -> (u32, u32, Option<u32>, u32) {
    log.ruleset().scoring.score(log.score_rows(), 1)
}

// ---------------------------------------------------------------------------
// Roles and the exchange
// ---------------------------------------------------------------------------

/// ⭐ **Who sends what.** Special Rules 4.2.1: *"W/VE stations send a signal report and their
/// state or province."* 4.2.2: *"DX stations send a signal report and serial number beginning
/// with 1."* The glossary's W/VE stations are *"stations operating in the United States and
/// District of Columbia with the exception of Alaska and Hawaii, and stations operating within
/// the Canadian provinces and territories"*.
#[test]
fn a_w_ve_station_sends_its_state_and_everyone_else_a_serial() {
    let il = select(&station("W9XYZ", "IL"));
    assert_eq!(il.contest_id, "ARRL-RTTY");
    assert_eq!(il.role().id, "w_ve");
    assert_eq!(
        composing(&il),
        vec![("RST", "599".into()), ("QTH", "IL".into())]
    );
    // The ARRL Contest Multipliers List's Canadian codes, Labrador included.
    for (call, qth) in [
        ("VE8XYZ", "NT"),
        ("VY2XYZ", "PE"),
        ("VO1XYZ", "NF"),
        ("VO2XYZ", "LB"),
        ("VY0XYZ", "NU"),
        ("W3XYZ", "DC"),
    ] {
        let s = select(&station(call, qth));
        assert_eq!(s.role().id, "w_ve", "{qth}");
        assert_eq!(s.field("QTH"), qth);
        assert_eq!(s.location_warning, None, "{qth} is listed");
    }
    // Canada Post's NL is the list's NF, "Newfoundland and Labrador (VO1/VO2)".
    let nl = select(&station("VO1XYZ", "NL"));
    assert_eq!(nl.role().id, "w_ve");
    assert_eq!(nl.field("QTH"), "NF");

    let dl = select(&station("DL9XYZ", ""));
    assert_eq!(dl.role().id, "dx");
    let sent: Vec<&str> = dl.my_exchange.iter().map(|v| v.key).collect();
    assert_eq!(sent, vec!["RST", "NR"], "a DX station sends no state");
    // Alaska and Hawaii are DX here: 5.2.2 "Hawaii (KH6) and Alaska (KL7) count as DXCC
    // entities", and the W/VE list has neither.
    assert_eq!(select(&station("KL7XYZ", "AK")).role().id, "dx");
    assert_eq!(select(&station("KH6XYZ", "HI")).role().id, "dx");
}

/// ⭐ **Both kinds of station receive both kinds of exchange** — "Any station may work any
/// other station" (arrl.org/rtty-roundup), so what arrives depends on where the OTHER station
/// is: a state from W/VE, a serial from DX. Neither box can be required.
#[test]
fn every_role_receives_a_state_or_a_serial() {
    for st in [station("W9XYZ", "IL"), station("DL9XYZ", "")] {
        let s = select(&st);
        let keys: Vec<&str> = s.role().receives.to_vec();
        assert_eq!(keys, vec!["RST", "QTH", "NR"], "{}", s.role().id);
    }
    let rs = ruleset_by_id("arrlrtty", CURRENT_RULES_YEAR).expect("shipped");
    for key in ["QTH", "NR"] {
        assert!(
            !rs.exchange.field(key).expect(key).required,
            "{key} is optional"
        );
    }
    assert!(rs.exchange.field("RST").expect("RST").required);
}

/// ⭐ **The window.** Rules p.1: *"First full weekend of January (but never on January 1)"*,
/// *"Begins 1800 UTC Saturday and runs through 2359 UTC Sunday."* The sponsor's calendar
/// prints 2026 Jan 3-4 … 2031 Jan 4-5 — and 2028 Jan 8-9, the year January 1 is a Saturday.
#[test]
fn the_window_is_the_first_full_weekend_of_january_never_january_first() {
    let rs = ruleset_by_id("arrlrtty", CURRENT_RULES_YEAR).expect("shipped");
    for (year, start) in [
        (2026, 1_767_463_200_u64),
        (2027, 1_798_912_800),
        (2028, 1_830_967_200),
        (2029, 1_862_416_800),
        (2030, 1_893_866_400),
        (2031, 1_925_316_000),
    ] {
        let w = rs.event_window(year);
        assert_eq!(w.start_unix, start, "{year} starts 1800Z Saturday");
        assert_eq!(w.end_unix - w.start_unix, 30 * 3600, "{year} runs 30 hours");
    }
    // Never on January 1, in any year this build could run in.
    for year in 2026..=2060u16 {
        let start = rs.event_window(year).start_unix;
        let day_of_year = (start - jan_first(year)) / 86_400;
        assert!(
            (1..=7).contains(&day_of_year),
            "{year}: starts on day {day_of_year} of January (0 = Jan 1)"
        );
    }
}

/// 0000Z on January 1 of `year`, by the civil calendar.
fn jan_first(year: u16) -> u64 {
    let y = i64::from(year) - 1;
    let days = 365 * (y - 1969) + (y - 1968) / 4 - (y - 1900) / 100 + (y - 1600) / 400;
    days as u64 * 86_400
}

// ---------------------------------------------------------------------------
// Points, dupes and multipliers
// ---------------------------------------------------------------------------

/// ⭐ **One point a contact, once a band; multipliers once a log.** Rules p.2: *"Each contact
/// counts for one (1) QSO point. Multiply total QSO points by the number of DXCC entities (not
/// including the US and Canada), plus US states, and Canadian provinces contacted to get your
/// final score. Each multiplier counts once, not once per band."* Special Rules 2.3: *"Contact
/// each station only once per band."*
#[test]
fn one_point_a_band_and_multipliers_once_a_log() {
    let mut log = FieldDayLog::new("W9XYZ", select(&station("W9XYZ", "IL")), "20m");
    assert!(work(
        &mut log,
        "20m",
        "K2DEF",
        &[("RST", "599"), ("QTH", "NY")],
        1
    ));
    assert!(work(
        &mut log,
        "20m",
        "VE3XYZ",
        &[("RST", "599"), ("QTH", "ON")],
        2
    ));
    assert!(work(
        &mut log,
        "20m",
        "JA1ABC",
        &[("RST", "599"), ("NR", "12")],
        3
    ));
    assert_eq!(score(&log).0, 3, "one point each");

    // THE DUPE: same station, same band — logged, marked, worth nothing.
    assert!(work(
        &mut log,
        "20m",
        "K2DEF",
        &[("RST", "599"), ("QTH", "NY")],
        4
    ));
    assert!(log.qsos().last().expect("the dupe row").dupe);
    assert_eq!(score(&log).0, 3, "the dupe adds nothing");
    // …and the same station on ANOTHER band is a new contact and a new point.
    assert!(work(
        &mut log,
        "40m",
        "K2DEF",
        &[("RST", "599"), ("QTH", "NY")],
        5
    ));
    assert!(!log.qsos().last().expect("40 m row").dupe);
    assert!(work(
        &mut log,
        "40m",
        "JA1ABC",
        &[("RST", "599"), ("NR", "40")],
        6
    ));
    assert_eq!(score(&log).0, 5);

    // MULTIPLIERS, once each across both bands: QTH {NY, ON}; DXCC {Japan} — the United
    // States and Canada are not DXCC multipliers here.
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("qth", 2), ("dxcc", 1)]
    );
    let (qso, _, mults, total) = score(&log);
    assert_eq!(mults, Some(3));
    assert_eq!(total, qso * 3);
}

/// ⭐ **Alaska, Hawaii and the other DXCC entities in North America count; the USA and Canada do
/// not.** 5.2.2: *"All Stations count DXCC entities (Except USA and Canada) … Hawaii (KH6) and
/// Alaska (KL7) count as DXCC entities."* The glossary's DX stations include *"US possessions
/// and territories in the Caribbean and Pacific and St. Paul Island (CY9)"*.
#[test]
fn alaska_hawaii_and_north_american_dx_are_entities_and_w_ve_is_not() {
    let mut log = FieldDayLog::new("DL9XYZ", select(&station("DL9XYZ", "")), "20m");
    for (i, call) in ["KL7ABC", "KH6ABC", "KP4ABC", "CY9ABC", "G3ABC"]
        .iter()
        .enumerate()
    {
        assert!(work(
            &mut log,
            "20m",
            call,
            &[("RST", "599"), ("NR", "7")],
            i as u64
        ));
    }
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("qth", 0), ("dxcc", 5)]
    );
    // CONTROL: a W and a VE add a state and a province, and no entity.
    assert!(work(
        &mut log,
        "20m",
        "W1ABC",
        &[("RST", "599"), ("QTH", "MA")],
        10
    ));
    assert!(work(
        &mut log,
        "20m",
        "VE3ABC",
        &[("RST", "599"), ("QTH", "ON")],
        11
    ));
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("qth", 2), ("dxcc", 5)]
    );
}

/// A state the list does not hold (an ARRL section, Alaska) logs and counts no QTH multiplier;
/// a serial is never a state.
#[test]
fn an_unlisted_state_logs_and_counts_no_multiplier() {
    let mut log = FieldDayLog::new("W9XYZ", select(&station("W9XYZ", "IL")), "20m");
    assert!(work(
        &mut log,
        "20m",
        "W1ABC",
        &[("RST", "599"), ("QTH", "EMA")],
        1
    ));
    assert!(work(
        &mut log,
        "20m",
        "W1DEF",
        &[("RST", "599"), ("QTH", "AK")],
        2
    ));
    assert_eq!(log.qso_count(), 2, "both logged and counted as contacts");
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows())[0],
        ("qth", 0)
    );
    // POSITIVE CONTROL.
    assert!(work(
        &mut log,
        "20m",
        "K1ABC",
        &[("RST", "599"), ("QTH", "MA")],
        3
    ));
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows())[0],
        ("qth", 1)
    );
}

// ---------------------------------------------------------------------------
// Only RTTY
// ---------------------------------------------------------------------------

/// ⭐ **Only RTTY counts — and nothing else is refused.** arrl.org/rtty-roundup: *"Only
/// contacts using Radioteletype (RTTY) mode are allowed."* Special Rules 2.2: *"Use only the
/// Radioteletype (RTTY) mode."* A contact in JTTY, FT8, PSK31 or CW is LOGGED and kept (it is
/// a real contact, and the other station may be credited for nothing either), and it scores no
/// point and no multiplier.
#[test]
fn a_contact_in_another_mode_is_logged_and_scores_nothing() {
    let rs = ruleset_by_id("arrlrtty", CURRENT_RULES_YEAR).expect("shipped");
    for mode in ["JTTY", "FT8", "FT4", "PSK31", "CW", "SSB", "USB"] {
        assert!(rs.mode_banned(mode), "{mode} is not allowed");
    }
    assert!(!rs.mode_banned("RTTY"), "RTTY is the contest's mode");

    let mut log = FieldDayLog::new("W9XYZ", select(&station("W9XYZ", "IL")), "20m");
    let ny = [("RST", "599"), ("QTH", "NY")];
    assert!(
        work_in(&mut log, "20m", "K2AAA", &ny, "DIG", "JTTY", 1),
        "JTTY: logged"
    );
    assert!(
        work_in(&mut log, "20m", "K2BBB", &ny, "DIG", "FT8", 2),
        "FT8: logged"
    );
    assert!(
        work_in(&mut log, "20m", "K2CCC", &ny, "DIG", "PSK31", 3),
        "PSK31: logged"
    );
    // A CW row carries no submode — CW is its own on-air mode — and is still not RTTY.
    assert!(
        work_in(&mut log, "20m", "K2DDD", &ny, "CW", "", 4),
        "CW: logged"
    );
    assert_eq!(log.qsos().len(), 4, "every contact is kept");
    assert_eq!(log.qso_count(), 0, "none of them counts");
    assert_eq!(score(&log).0, 0);
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows())[0],
        ("qth", 0)
    );
    // POSITIVE CONTROL: the same exchange in RTTY counts, point and multiplier.
    assert!(work(&mut log, "20m", "K2EEE", &ny, 5));
    assert_eq!(log.qso_count(), 1);
    assert_eq!(score(&log).0, 1);
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows())[0],
        ("qth", 1)
    );
}

// ---------------------------------------------------------------------------
// The Cabrillo log
// ---------------------------------------------------------------------------

fn qso_lines(cab: &str) -> Vec<&str> {
    cab.lines().filter(|l| l.starts_with("QSO:")).collect()
}

fn work_on(
    log: &mut FieldDayLog,
    band: &str,
    khz: u32,
    call: &str,
    ex: &[(&str, &str)],
    min: u64,
) -> bool {
    log.dial_khz = khz;
    work(log, band, call, ex, min)
}

/// ⭐ **The QSO lines, byte for byte, from a W/VE station.** ARRL publishes no RTTY Roundup QSO
/// template, so the line is WWROF's generic *"QSO: freq mo date time call rst exch call rst
/// exch"*, the exchange being what the rules' own examples show (*"K9CT 599 CT"* answered
/// *"599 IL"*; *"WK6I 599 428"* answered *"599 CA"*), and 2.4 *"contacts must be coded as RY
/// (RTTY)"*. No transmitter column on a single-operator line.
#[test]
fn a_w_ve_log_writes_rst_and_state_and_takes_a_serial() {
    let mut log = FieldDayLog::new("W9XYZ", select(&station("W9XYZ", "IL")), "20m");
    assert!(work_on(
        &mut log,
        "20m",
        14_080,
        "K2DEF",
        &[("RST", "599"), ("QTH", "NY")],
        1
    ));
    assert!(work_on(
        &mut log,
        "20m",
        14_085,
        "JA1ABC",
        &[("RST", "599"), ("NR", "428")],
        2
    ));
    assert!(work_on(
        &mut log,
        "40m",
        7_085,
        "VE3XYZ",
        &[("RST", "579"), ("QTH", "ON")],
        3
    ));
    let cab = log.cabrillo(14_080).expect("one entry");
    assert_eq!(
        qso_lines(&cab),
        vec![
            "QSO: 14080 RY 2027-01-02 1801 W9XYZ 599 IL K2DEF 599 NY",
            "QSO: 14085 RY 2027-01-02 1802 W9XYZ 599 IL JA1ABC 599 428",
            "QSO: 7085 RY 2027-01-02 1803 W9XYZ 599 IL VE3XYZ 579 ON",
        ]
    );
    for line in [
        "START-OF-LOG: 3.0",
        "CONTEST: ARRL-RTTY",
        "CALLSIGN: W9XYZ",
        "CATEGORY-OPERATOR: SINGLE-OP",
        "CATEGORY-ASSISTED: NON-ASSISTED",
        "CATEGORY-MODE: RTTY",
        "CATEGORY-POWER: LOW",
        "LOCATION: IL",
        // 3 points × (NY, ON, Japan).
        "CLAIMED-SCORE: 9",
    ] {
        assert!(
            cab.lines().any(|l| l == line),
            "missing {line:?} in:\n{cab}"
        );
    }
}

/// ⭐ **…and from a DX station**, whose own exchange is its serial, from 1 (4.2.2).
#[test]
fn a_dx_log_writes_rst_and_its_serial() {
    let mut log = FieldDayLog::new("DL9XYZ", select(&station("DL9XYZ", "")), "20m");
    // The serial is issued when the exchange is composed for a station, as in Sweepstakes.
    let sent = log.session.compose_for("W9ABC", START + 50).tx.clone();
    let sent: Vec<(&str, &str)> = sent.iter().map(|v| (v.key, v.raw.as_str())).collect();
    assert_eq!(
        sent,
        vec![("RST", "599"), ("NR", "1")],
        "serial number beginning with 1"
    );
    assert!(work_on(
        &mut log,
        "20m",
        14_080,
        "W9ABC",
        &[("RST", "599"), ("QTH", "IL")],
        1
    ));
    log.session.compose_for("G3ABC", START + 110);
    assert!(work_on(
        &mut log,
        "20m",
        14_081,
        "G3ABC",
        &[("RST", "599"), ("NR", "77")],
        2
    ));
    let cab = log.cabrillo(14_080).expect("one entry");
    assert_eq!(
        qso_lines(&cab),
        vec![
            "QSO: 14080 RY 2027-01-02 1801 DL9XYZ 599 1 W9ABC 599 IL",
            "QSO: 14081 RY 2027-01-02 1802 DL9XYZ 599 2 G3ABC 599 77",
        ]
    );
    assert!(cab.lines().any(|l| l == "LOCATION: DX"), "{cab}");
}

/// ⭐ **LOCATION is the ARRL/RAC section for a W/VE entry, and DX for everyone else.** ARRL's log
/// upload (contest-log-submission.arrl.org, read 2026-10-26): *"Location During Contest: Select
/// ARRL/RAC sections or DX Outside US/Canada"*. The Cabrillo header specification
/// (wwrof.org/cabrillo/cabrillo-v3-header, read 2026-10-26): *"For USA and Canada stations
/// LOCATION must be the ARRL section abbreviation. For foreign stations LOCATION must be 'DX'.
/// This information is required for IARU-HF and for all ARRL and CQ contests."* A station in
/// Massachusetts sends `MA` on the air and heads its file with its section, `EMA`; a DX station
/// with a section left in its settings is still `DX`.
#[test]
fn location_is_the_arrl_section_and_dx_for_a_dx_entry() {
    let mut st = station("W1XYZ", "MA");
    st.fd_section = "EMA".to_string();
    let mut log = FieldDayLog::new("W1XYZ", select(&st), "20m");
    assert!(work_on(
        &mut log,
        "20m",
        14_080,
        "K2DEF",
        &[("RST", "599"), ("QTH", "NY")],
        1
    ));
    let cab = log.cabrillo(14_080).expect("one entry");
    assert!(cab.lines().any(|l| l == "LOCATION: EMA"), "{cab}");
    assert_eq!(
        qso_lines(&cab),
        vec!["QSO: 14080 RY 2027-01-02 1801 W1XYZ 599 MA K2DEF 599 NY"],
        "the state is what goes out on the air"
    );

    let mut dx = station("DL9XYZ", "");
    dx.fd_section = "EMA".to_string();
    let mut log = FieldDayLog::new("DL9XYZ", select(&dx), "20m");
    log.session.compose_for("W9ABC", START + 50);
    assert!(work_on(
        &mut log,
        "20m",
        14_080,
        "W9ABC",
        &[("RST", "599"), ("QTH", "IL")],
        1
    ));
    let cab = log.cabrillo(14_080).expect("one entry");
    assert!(cab.lines().any(|l| l == "LOCATION: DX"), "{cab}");
}

/// The token every registry agrees on, and the advisory band list: *"Use only the 3.5, 7, 14,
/// 21, and 28 MHz bands."*
#[test]
fn the_token_and_the_bands_are_the_sponsors() {
    let rs = ruleset_by_id("arrlrtty", CURRENT_RULES_YEAR).expect("shipped");
    assert_eq!(rs.contest_id, "ARRL-RTTY");
    assert_eq!(
        tempo_core::contest::cabrillo_contest_token(rs.contest_id),
        "ARRL-RTTY"
    );
    assert!(!rs.transmitter_column);
    assert_eq!(rs.bands, &["80m", "40m", "20m", "15m", "10m"]);
}
