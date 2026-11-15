//! ⭐ **The WAE DX Contest — CW, SSB and RTTY — as three built-in rulesets.**
//!
//! Every expected value comes from DARC's own rules page, "Worked All Europe DX Contest -
//! Rules 2025" (<https://www.darc.de/der-club/referate/conteste/wae-dx-contest/en/wae-rules/>,
//! revision 2025-06-17, which gives the 2026 dates), and the Cabrillo contest names from the
//! WA7BNM Cabrillo name list and ADIF 3.1.7's CONTEST_ID table, which agree. The sentence each
//! assertion rests on is quoted at it.
//!
//! What this build models: the window, the report-and-serial exchange (serial `000` for a
//! station that sends none), contacts once per station per band, and a Cabrillo file with the
//! sponsor's contest name. What it does NOT model, on purpose: QTC traffic, the band-weighted
//! multipliers (so no claimed score is written), the Europe/non-Europe working rule on CW and
//! SSB, and the single operator's off times.
//!
//! Every callsign is an example call.
use tempo_core::contest::{install_call_resolver, CallLocation, ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, validate, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

const SEED: &str = include_str!("../src/fd_rules.seed.json");
const GOLDENS: &str = "tests/fixtures/cabrillo-goldens";

/// (event, Cabrillo CONTEST token, the mode class its contacts log in, its report).
const EVENTS: [(&str, &str, &str, &str); 3] = [
    ("waedc_cw", "DARC-WAEDC-CW", "CW", "599"),
    ("waedc_ssb", "DARC-WAEDC-SSB", "PH", "59"),
    ("waedc_rtty", "DARC-WAEDC-RTTY", "DIG", "599"),
];

fn place(call: &str) -> Option<CallLocation> {
    let up = call.trim().to_ascii_uppercase();
    let (entity, continent) = if up.starts_with("DL") {
        ("Fed. Rep. of Germany", "EU")
    } else if up.starts_with("JA") {
        ("Japan", "AS")
    } else if up.starts_with('W') || up.starts_with('K') {
        ("United States", "NA")
    } else {
        return None;
    };
    Some(CallLocation {
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

fn station() -> StationData {
    StationData {
        mycall: "W9XYZ".into(),
        mygrid: "EN52".into(),
        contest_qth_state: "IL".into(),
        contest_category_operator: "SINGLE-OP".into(),
        contest_category_power: "LOW".into(),
        ..Default::default()
    }
}

fn log_for(event: &str) -> FieldDayLog {
    resolver();
    let rs = ruleset_by_id(event, CURRENT_RULES_YEAR)
        .unwrap_or_else(|| panic!("{event} is a shipped ruleset"));
    let session = ContestSession::for_ruleset(rs, &station())
        .unwrap_or_else(|e| panic!("{event} session refused: {e}"));
    FieldDayLog::new("W9XYZ", session, "20m")
}

/// 2026-11-14 00:00:00Z, the RTTY running's first second.
const START: u64 = 1_794_614_400;

/// Work `call` on `band` the way a contact is made: the exchange is composed for it (which
/// issues the next serial), then the received report and number are logged.
fn work(
    log: &mut FieldDayLog,
    mode: &str,
    band: &str,
    call: &str,
    rst: &str,
    nr: &str,
    min: u64,
) -> bool {
    let when = START + min * 60;
    log.band = band.to_string();
    log.session.compose_for(call, when);
    let fields = vec![
        ("RST".to_string(), rst.to_string()),
        ("NR".to_string(), nr.to_string()),
    ];
    // A digital contact in this contest is RTTY, which the QSO line writes RY.
    let submode = if mode == "DIG" { "RTTY" } else { "" };
    log.log_fields_at(call, &fields, mode, submode, 0, when)
}

/// The publish gate (`fd_rules::validate`, what `.github/workflows/fd-rules.yml` runs) takes
/// the seed with the three rows in it, and each one resolves by id to the sponsor's contest name.
#[test]
fn the_publish_gate_takes_the_seed_and_each_wae_row_resolves() {
    validate(SEED).expect("the shipped seed passes the publish gate");
    for (event, token, _, _) in EVENTS {
        let rs = ruleset_by_id(event, CURRENT_RULES_YEAR)
            .unwrap_or_else(|| panic!("{event} is a shipped ruleset"));
        assert_eq!(rs.contest_id, token, "{event}");
        // §2: "3.5 - 7 - 14 - 21 - 28 MHz." Advice for the entry strip, never a refusal.
        assert_eq!(rs.bands, ["80m", "40m", "20m", "15m", "10m"], "{event}");
    }
}

/// ⭐ **The window.** §1: "CW: August, second full weekend / Saturday, August 8th 2026, 00:00
/// UTC until Sunday, August 9th 2026, 23:59 UTC"; "SSB: September, second full weekend /
/// Saturday, September 12th 2026 …"; "RTTY: November, second full weekend / Saturday, November
/// 14th 2026 …". 2026 is DARC's own printed dates, to the second. 2025 and 2027 are the same
/// rule's other readings: November 2025 opens on a Saturday, so its first full weekend is the
/// 1st and the second the 8th; August 2027 opens on a Sunday, so its second full weekend is the
/// 14th, not the 7th.
#[test]
fn the_window_is_the_second_full_weekend_of_august_september_and_november() {
    let cases: [(&str, [(u16, u64); 3]); 3] = [
        (
            "waedc_cw",
            [
                (2025, 1_754_697_600), // Sat 2025-08-09
                (2026, 1_786_147_200), // Sat 2026-08-08
                (2027, 1_818_201_600), // Sat 2027-08-14
            ],
        ),
        (
            "waedc_ssb",
            [
                (2025, 1_757_721_600), // Sat 2025-09-13
                (2026, 1_789_171_200), // Sat 2026-09-12
                (2027, 1_820_620_800), // Sat 2027-09-11
            ],
        ),
        (
            "waedc_rtty",
            [
                (2025, 1_762_560_000), // Sat 2025-11-08
                (2026, 1_794_614_400), // Sat 2026-11-14
                (2027, 1_826_064_000), // Sat 2027-11-13
            ],
        ),
    ];
    for (event, years) in cases {
        let rs = ruleset_by_id(event, CURRENT_RULES_YEAR).expect("shipped");
        for (year, start) in years {
            let w = rs.event_window(year);
            assert_eq!(w.start_unix, start, "{event} {year} starts 0000Z Saturday");
            // 0000Z Saturday until 2359Z Sunday: the 48-hour period §4 counts off times in.
            assert_eq!(w.end_unix - w.start_unix, 48 * 3600, "{event} {year}");
        }
    }
}

/// ⭐ **Once per station per band.** §5: "Each station can be credited only once per band."
/// Each contest is one mode, so the band alone splits a repeat.
#[test]
fn a_station_counts_once_per_band() {
    for (event, _, mode, rst) in EVENTS {
        let mut log = log_for(event);
        assert!(work(&mut log, mode, "20m", "DL1ABC", rst, "001", 1));
        assert!(
            !log.qsos().last().expect("row").dupe,
            "{event}: first on 20 m"
        );
        // THE DUPE: same station, same band — logged, marked.
        assert!(work(&mut log, mode, "20m", "DL1ABC", rst, "001", 2));
        assert!(
            log.qsos().last().expect("row").dupe,
            "{event}: again on 20 m"
        );
        // Another band is a new contact.
        assert!(work(&mut log, mode, "40m", "DL1ABC", rst, "002", 3));
        assert!(
            !log.qsos().last().expect("row").dupe,
            "{event}: once on 40 m"
        );
        // One point a contact: §8 scores "total QSOs plus QTCs", and QTCs are not modelled.
        let (qsos, _, _, _) = log.ruleset().scoring.score(log.score_rows(), 1);
        assert_eq!(qsos, 2, "{event}: the dupe adds nothing");
    }
}

/// ⭐ **Serial 000 is a legal copy.** §5: "If the station worked does not send a serial number,
/// log the contact with number 000." The row keeps the 000 and the file writes it.
#[test]
fn serial_000_from_a_station_that_sends_none_is_logged_and_written() {
    let mut log = log_for("waedc_cw");
    assert!(work(&mut log, "CW", "20m", "DL1ABC", "599", "000", 1));
    let row = log.qsos().last().expect("row");
    assert!(!row.dupe);
    assert_eq!(
        row.rx
            .iter()
            .find(|v| v.key == "NR")
            .map(|v| v.raw.as_str()),
        Some("000")
    );
    let cab = log.cabrillo(14_000).expect("exports");
    let line = cab
        .lines()
        .find(|l| l.starts_with("QSO:"))
        .expect("a QSO line");
    assert!(line.ends_with(" DL1ABC 599 000"), "{line}");
}

/// ⭐ **The Cabrillo file, byte for byte.** The CONTEST token is the sponsor's
/// (`DARC-WAEDC-CW/SSB/RTTY`); each QSO line carries the report and a three-digit serial on
/// both sides ("a progressive serial number starting with 001", §5); and there is no
/// `CLAIMED-SCORE:` line, because without QTCs and the band-weighted multipliers a number here
/// would be a false claim.
#[test]
fn each_wae_contest_exports_its_golden_cabrillo_file() {
    for (event, token, mode, rst) in EVENTS {
        let mut log = log_for(event);
        assert!(work(&mut log, mode, "20m", "DL1ABC", rst, "1", 0));
        assert!(work(&mut log, mode, "20m", "JA1ABC", rst, "17", 1));
        assert!(work(&mut log, mode, "40m", "DL1ABC", rst, "000", 2));
        let got = log
            .cabrillo(14_000)
            .unwrap_or_else(|e| panic!("{event}: {e}"));
        assert!(
            got.lines().any(|l| l == format!("CONTEST: {token}")),
            "{got}"
        );
        assert!(!got.contains("CLAIMED-SCORE"), "{event}: {got}");
        let path = format!("{GOLDENS}/{event}.cbr");
        let want = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(got, want, "{event}: the Cabrillo file changed");
    }
}
