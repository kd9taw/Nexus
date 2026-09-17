//! ⭐ **Every contest but CQ WW RTTY exports the Cabrillo file it exported before the rules
//! file could declare a sponsor's own template** — byte for byte.
//!
//! The `cabrillo` block (fixed columns, optional headers, LOCATION spellings), the advisory
//! `bands` list and `location_aliases` are OPTIONAL ruleset keys, and absent must mean exactly
//! the output every other contest already had. The files under
//! `tests/fixtures/cabrillo-goldens/` were written by THIS test running against the writer as
//! it stood before those keys existed (commit `ca5ca56f`), with `NEXUS_WRITE_CABRILLO_GOLDENS=1`;
//! the default mode compares today's writer with them. A log per contest, in every role that
//! contest's station data selects, over two bands, so the QSO lines, the header block and the
//! trailing transmitter column are all in the comparison.
//!
//! Regenerating them against today's writer would make this test prove nothing. A deliberate
//! change to another contest's file is a new golden AND a sentence in its commit saying why.
//! Every callsign is an example call.
use tempo_core::contest::{install_call_resolver, CallLocation, ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

const DIR: &str = "tests/fixtures/cabrillo-goldens";

fn place(call: &str) -> Option<CallLocation> {
    let up = call.trim().to_ascii_uppercase();
    let (entity, continent) = if up.starts_with("DL") {
        ("Fed. Rep. of Germany", "EU")
    } else if up.starts_with("JA") {
        ("Japan", "AS")
    } else if up.starts_with('W') || up.starts_with('K') || up.starts_with('N') {
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

/// One row's received exchange, as the entry strip hands it over: `(slot, raw)` pairs.
type Fields = &'static [(&'static str, &'static str)];

/// One row of a golden log: the call worked, the band it was worked on, and what it sent.
type Row = (&'static str, &'static str, Fields);

/// One contest's golden log: the station, and three received rows.
struct Case {
    event: &'static str,
    mode: &'static str,
    station: StationData,
    rows: [Row; 3],
}

fn station(fd_class: &str) -> StationData {
    StationData {
        mycall: "W9XYZ".into(),
        mygrid: "EN52".into(),
        fd_class: fd_class.into(),
        fd_section: "WI".into(),
        contest_qth_state: "IL".into(),
        contest_cq_zone: "4".into(),
        contest_check: "74".into(),
        contest_category_operator: "SINGLE-OP".into(),
        contest_category_power: "LOW".into(),
        contest_category_assisted: "NON-ASSISTED".into(),
        ..Default::default()
    }
}

fn cases() -> Vec<Case> {
    const HF: [&str; 3] = ["20m", "20m", "40m"];
    const VHF: [&str; 3] = ["6m", "6m", "2m"];
    let hf = |f: [Fields; 3]| {
        [
            ("K1ABC", HF[0], f[0]),
            ("DL1ABC", HF[1], f[1]),
            ("JA1ABC", HF[2], f[2]),
        ]
    };
    let vhf = |f: [Fields; 3]| {
        [
            ("K1ABC", VHF[0], f[0]),
            ("W2DEF", VHF[1], f[1]),
            ("N3GHI", VHF[2], f[2]),
        ]
    };
    let ss = [
        (
            "K1ABC",
            "20m",
            &[
                ("NR", "1"),
                ("PREC", "A"),
                ("CALL", "K1ABC"),
                ("CK", "74"),
                ("SEC", "CT"),
            ][..],
        ),
        (
            "W2DEF",
            "20m",
            &[
                ("NR", "22"),
                ("PREC", "B"),
                ("CALL", "W2DEF"),
                ("CK", "99"),
                ("SEC", "ENY"),
            ][..],
        ),
        (
            "N3GHI",
            "40m",
            &[
                ("NR", "5"),
                ("PREC", "Q"),
                ("CALL", "N3GHI"),
                ("CK", "05"),
                ("SEC", "EPA"),
            ][..],
        ),
    ];
    let party = |q: [&'static str; 3]| -> [Fields; 3] {
        match q {
            ["FRAN", "MI", "ON"] => [
                &[("RST", "599"), ("QTH", "FRAN")],
                &[("RST", "599"), ("QTH", "MI")],
                &[("RST", "599"), ("QTH", "ON")],
            ],
            _ => [
                &[("RST", "599"), ("QTH", "IL")],
                &[("RST", "599"), ("QTH", "OH")],
                &[("RST", "599"), ("QTH", "CA")],
            ],
        }
    };
    vec![
        Case {
            event: "arrlfd",
            mode: "CW",
            station: station("3A"),
            rows: hf([
                &[("CLASS", "2A"), ("SECTION", "IL")],
                &[("CLASS", "1D"), ("SECTION", "EMA")],
                &[("CLASS", "4F"), ("SECTION", "ONS")],
            ]),
        },
        Case {
            event: "wfd",
            mode: "CW",
            station: station("3H"),
            rows: hf([
                &[("CLASS", "2H"), ("SECTION", "IL")],
                &[("CLASS", "1O"), ("SECTION", "EMA")],
                &[("CLASS", "3M"), ("SECTION", "ONS")],
            ]),
        },
        Case {
            event: "tnqp",
            mode: "CW",
            station: station("3A"),
            rows: hf(party(["IL", "OH", "CA"])),
        },
        Case {
            event: "ohqp",
            mode: "CW",
            station: station("3A"),
            rows: hf(party(["FRAN", "MI", "ON"])),
        },
        Case {
            event: "cqp",
            mode: "CW",
            station: station("3A"),
            rows: hf([
                &[("NR", "1"), ("QTH", "IL")],
                &[("NR", "2"), ("QTH", "NV")],
                &[("NR", "3"), ("QTH", "DX")],
            ]),
        },
        Case {
            event: "txqp",
            mode: "CW",
            station: station("3A"),
            rows: hf(party(["IL", "OH", "CA"])),
        },
        Case {
            event: "arrlss_cw",
            mode: "CW",
            station: station("3A"),
            rows: ss,
        },
        Case {
            event: "arrlss_ssb",
            mode: "PH",
            station: station("3A"),
            rows: ss,
        },
        Case {
            event: "cqww_cw",
            mode: "CW",
            station: station("3A"),
            rows: hf([
                &[("RST", "599"), ("ZN", "5")],
                &[("RST", "599"), ("ZN", "14")],
                &[("RST", "599"), ("ZN", "25")],
            ]),
        },
        Case {
            event: "cqww_ssb",
            mode: "PH",
            station: station("3A"),
            rows: hf([
                &[("RST", "59"), ("ZN", "5")],
                &[("RST", "59"), ("ZN", "14")],
                &[("RST", "59"), ("ZN", "25")],
            ]),
        },
        Case {
            event: "cqwpx_cw",
            mode: "CW",
            station: station("3A"),
            rows: hf([
                &[("RST", "599"), ("NR", "1")],
                &[("RST", "599"), ("NR", "2")],
                &[("RST", "599"), ("NR", "3")],
            ]),
        },
        Case {
            event: "cqwpx_ssb",
            mode: "PH",
            station: station("3A"),
            rows: hf([
                &[("RST", "59"), ("NR", "1")],
                &[("RST", "59"), ("NR", "2")],
                &[("RST", "59"), ("NR", "3")],
            ]),
        },
        Case {
            event: "arrlvhf_jan",
            mode: "PH",
            station: station("3A"),
            rows: vhf([
                &[("GRID", "FN31")],
                &[("GRID", "EM12")],
                &[("GRID", "DM79")],
            ]),
        },
        Case {
            event: "arrlvhf_jun",
            mode: "PH",
            station: station("3A"),
            rows: vhf([
                &[("GRID", "FN31")],
                &[("GRID", "EM12")],
                &[("GRID", "DM79")],
            ]),
        },
        Case {
            event: "arrlvhf_sep",
            mode: "PH",
            station: station("3A"),
            rows: vhf([
                &[("GRID", "FN31")],
                &[("GRID", "EM12")],
                &[("GRID", "DM79")],
            ]),
        },
    ]
}

/// A contest's log, built and exported the one way every version of the writer supports.
fn export(case: &Case) -> String {
    export_on(case, |_| 0)
}

/// [`export`], with each row logged on the dial `dial(band)` gives (kHz; `0` = unknown).
fn export_on(case: &Case, dial: impl Fn(&str) -> u32) -> String {
    resolver();
    let rs = ruleset_by_id(case.event, CURRENT_RULES_YEAR).expect("a shipped ruleset");
    let session = ContestSession::for_ruleset(rs, &case.station)
        .unwrap_or_else(|e| panic!("{} refused: {e}", case.event));
    let mut log = FieldDayLog::new("W9XYZ", session, case.rows[0].1);
    for (i, (call, band, fields)) in case.rows.iter().enumerate() {
        log.band = band.to_string();
        log.dial_khz = dial(band);
        let fields: Vec<(String, String)> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert!(
            log.log_fields_at(
                call,
                &fields,
                case.mode,
                "",
                0,
                1_790_380_800 + 60 * i as u64
            ),
            "{}: {call} logs",
            case.event
        );
    }
    log.cabrillo(14_000)
        .unwrap_or_else(|e| panic!("{} exports: {e}", case.event))
}

#[test]
fn every_other_contests_cabrillo_file_is_byte_identical() {
    let write = std::env::var_os("NEXUS_WRITE_CABRILLO_GOLDENS").is_some();
    let cases = cases();
    assert_eq!(cases.len(), 15, "every shipped contest but CQ WW RTTY");
    for case in &cases {
        let got = export(case);
        let path = format!("{DIR}/{}.cbr", case.event);
        if write {
            std::fs::create_dir_all(DIR).expect("golden dir");
            std::fs::write(&path, &got).expect("golden written");
            continue;
        }
        let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!("{path}: {e} (goldens are written by the pre-template writer)")
        });
        assert_eq!(got, want, "{}: the Cabrillo file changed", case.event);
    }
    assert!(
        !write,
        "goldens written — this run proves nothing; run again without the variable"
    );
}

/// ⭐ **With the dial known, only the frequency column moves** — and only where it should.
/// Every other contest now writes the dial a contact was logged on in place of the band edge
/// (a deliberate change, pinned here as exactly that): on an HF row outside Field Day the
/// QSO line's frequency is the dial, and every other byte of the file — every header, every
/// other column, Field Day's file entire, a VHF band token — is the golden.
#[test]
fn with_a_dial_only_the_frequency_column_moves() {
    let dial = |band: &str| match band {
        "20m" => 14_025,
        "40m" => 7_025,
        "6m" => 50_125,
        "2m" => 144_200,
        _ => 0,
    };
    let mut moved = 0;
    for case in &cases() {
        let got = export_on(case, dial);
        let want = std::fs::read_to_string(format!("{DIR}/{}.cbr", case.event)).expect("golden");
        let field_day = matches!(case.event, "arrlfd" | "wfd");
        assert_eq!(got.lines().count(), want.lines().count(), "{}", case.event);
        for (g, w) in got.lines().zip(want.lines()) {
            if !w.starts_with("QSO:") {
                assert_eq!(g, w, "{}: a header moved", case.event);
                continue;
            }
            let (gt, wt): (Vec<&str>, Vec<&str>) = (g.split(' ').collect(), w.split(' ').collect());
            assert_eq!(
                gt[2..],
                wt[2..],
                "{}: only the frequency may move: {g}",
                case.event
            );
            let hf_dial = match wt[1] {
                "14000" => Some("14025"),
                "7000" => Some("7025"),
                _ => None,
            };
            match hf_dial {
                Some(d) if !field_day => {
                    assert_eq!(gt[1], d, "{}: {g}", case.event);
                    moved += 1;
                }
                _ => assert_eq!(gt[1], wt[1], "{}: {g}", case.event),
            }
        }
    }
    // POSITIVE CONTROL: the comparison saw dials move — 10 HF contests x 3 rows.
    assert_eq!(moved, 30);
}
