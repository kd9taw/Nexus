//! ⛔ **EVERY SHIPPED CONTEST'S FT8/FT4 SEQUENCER, BYTE FOR BYTE** — a golden transcript of what
//! the contest sequencer sends, logs and reports for every contest in the rules file, on FT8, FT4
//! and a tier that keeps Field Day's messages (FT2), running and in S&P, under a standard and a
//! nonstandard call, through a fixed set of scripts of what the other station sends.
//!
//! The fixture was written by this test from the sequencer as it stood BEFORE custom contests got
//! WSJT-X's contest messages, and nothing that change does may move one byte of it: a custom
//! contest's messages are a new arm, never an edit to a shipped contest's. A deliberate change to a
//! shipped contest's messages re-writes the fixture (`NEXUS_BLESS_FT_GOLDEN=1`), and the diff is
//! the review.
//!
//! The scripts are a regression net, not a protocol: each one feeds the station every message
//! shape the sequencers know (Field Day's exchange, grids, reports, rogers, closes, a nonstandard
//! partner, `<...>`, and the RTTY Roundup and EU VHF contest messages), so an arm that starts
//! answering something it passed by before shows here.

use tempo_core::contest::{install_call_resolver, CallLocation, ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::{FdEvent, FieldDayStation, FtMessages};

const FIXTURE: &str = "tests/fixtures/ft-contest-messages.golden";

fn resolver() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = install_call_resolver(|call: &str| {
            let up = call.trim().to_ascii_uppercase();
            let (entity, continent) = if up.starts_with("VE") {
                ("Canada", "NA")
            } else if up.starts_with('G') {
                ("England", "EU")
            } else {
                ("United States", "NA")
            };
            Some(CallLocation {
                entity,
                continent,
                cq_zone: None,
            })
        });
    });
}

fn dec(text: &str) -> modes::Decode {
    modes::Decode {
        message: text.into(),
        sync: 1.0,
        snr: -7,
        dt: 0.0,
        freq: 1500.0,
        nap: 0,
        qual: 1.0,
        rv: None,
        raw: None,
        mode: None,
    }
}

/// The scripts: what W9XYZ sends, `@` standing for my call as a decoder prints it to me (`<@>` for
/// it hashed).
const SCRIPTS: &[&[&str]] = &[
    // Field Day's exchange, then its roger and close.
    &[
        "@ W9XYZ 1D WI",
        "@ W9XYZ R 1D WI",
        "@ W9XYZ RR73",
        "@ W9XYZ 73",
    ],
    // A grid, its roger, reports and closes.
    &[
        "@ W9XYZ EN37",
        "@ W9XYZ R EN37",
        "@ W9XYZ -07",
        "@ W9XYZ R-07",
        "@ W9XYZ RR73",
        "@ W9XYZ 73",
    ],
    // A report first, then the close.
    &["@ W9XYZ -07", "@ W9XYZ R-07", "@ W9XYZ RRR", "@ W9XYZ 73"],
    // A first call with no grid.
    &["@ W9XYZ", "@ W9XYZ R 1D WI", "@ W9XYZ R-07", "@ W9XYZ RR73"],
    // Field Day's exchange rogered at once.
    &["@ W9XYZ R 1D WI", "@ W9XYZ RR73"],
    // A nonstandard partner.
    &[
        "<@> W9XYZ/M",
        "@ <W9XYZ/M> EN37",
        "@ <W9XYZ/M> R 1D WI",
        "@ <W9XYZ/M> R-07",
        "<@> W9XYZ/M RR73",
    ],
    // Messages for nobody here, and a call not resolved.
    &[
        "CQ W9XYZ EN37",
        "W1AW W9XYZ EN37",
        "<...> W9XYZ R 1D WI",
        "@ <...> R 1D WI",
        "@ W9XYZ RR73",
    ],
    // The ARRL RTTY Roundup's contest messages.
    &[
        "@ W9XYZ 579 WI",
        "@ W9XYZ R 579 WI",
        "@ W9XYZ 559 0013",
        "TU; @ W9XYZ R 579 0013",
        "@ W9XYZ RR73",
        "@ W9XYZ 73",
    ],
    // The EU VHF contest's messages.
    &[
        "@ W9XYZ EN37",
        "<@> <W9XYZ> 570001 EN37AB",
        "<@> <W9XYZ> R 570001 EN37AB",
        "@ W9XYZ RR73",
        "@ W9XYZ 73",
    ],
];

/// The session `event` starts with this station's data, or why it cannot start.
fn session(event: &str, mycall: &str) -> Result<ContestSession, String> {
    let rs = ruleset_by_id(event, CURRENT_RULES_YEAR).expect("a shipped ruleset");
    match event {
        "arrlfd" => Ok(ContestSession::field_day(FdEvent::ArrlFd, "2B", "EMA")),
        "wfd" => Ok(ContestSession::field_day(FdEvent::WinterFd, "2H", "EMA")),
        _ => ContestSession::for_ruleset(
            rs,
            &StationData {
                mycall: mycall.to_string(),
                mygrid: "FN42".to_string(),
                fd_class: "2B".to_string(),
                fd_section: "EMA".to_string(),
                contest_qth_state: "MA".to_string(),
                contest_qth_county: "MIDD".to_string(),
                contest_check: "99".to_string(),
                contest_cq_zone: "5".to_string(),
                contest_itu_zone: "8".to_string(),
                contest_power: "100".to_string(),
                contest_category_assisted: "NON-ASSISTED".to_string(),
                contest_category_power: "LOW".to_string(),
                ..Default::default()
            },
        ),
    }
}

fn kv(v: &[tempo_core::contest::FieldValue]) -> String {
    v.iter()
        .map(|f| format!("{}={}", f.key, f.raw))
        .collect::<Vec<_>>()
        .join(",")
}

fn next(st: &FieldDayStation) -> String {
    st.outgoing().map(|m| m.to_text()).unwrap_or_default()
}

/// One script run on a fresh station, every step written to `out`.
fn run(
    out: &mut String,
    event: &str,
    mode: &str,
    mycall: &str,
    running: bool,
    answer: Option<&str>,
    script: &[&str],
) {
    use std::fmt::Write;
    let rs = ruleset_by_id(event, CURRENT_RULES_YEAR).expect("a shipped ruleset");
    let s = match session(event, mycall) {
        Ok(s) => s,
        Err(e) => {
            writeln!(out, "  session refused: {e}").unwrap();
            return;
        }
    };
    let mut st = if running {
        FieldDayStation::running(mycall, "FN42", s, "20m")
    } else {
        FieldDayStation::search_and_pounce(mycall, "FN42", s, "20m")
    };
    st.set_messages(FtMessages::for_contest(rs, mode), running);
    st.set_wsjtx_field_day(FtMessages::wsjtx_field_day(rs, mode));
    writeln!(
        out,
        "  messages={:?} sends_grid={} start={:?} out={:?}",
        st.messages(),
        st.sends_grid(),
        st.state,
        next(&st)
    )
    .unwrap();
    if let Some(grid) = answer {
        st.answer_cq("W9XYZ", grid, -10);
        writeln!(
            out,
            "  answer W9XYZ {grid:?} -> {:?} {:?}",
            st.state,
            next(&st)
        )
        .unwrap();
    }
    let me = if mycall.contains('/') {
        format!("<{mycall}>")
    } else {
        mycall.to_string()
    };
    for (k, line) in script.iter().enumerate() {
        st.after_tx();
        let text = line
            .replace("<@>", &format!("<{mycall}>"))
            .replace('@', &me);
        st.observe(&[dec(&text)], 10 + 2 * k as u64);
        writeln!(out, "  hear {text:?} -> {:?} {:?}", st.state, next(&st)).unwrap();
    }
    for q in st.log.qsos() {
        writeln!(
            out,
            "  logged {} rx[{}] tx[{}] dupe={}",
            q.call,
            kv(&q.rx),
            kv(&q.tx),
            q.dupe
        )
        .unwrap();
    }
}

fn transcript() -> String {
    use std::fmt::Write;
    resolver();
    let seed: serde_json::Value =
        serde_json::from_str(include_str!("../src/fd_rules.seed.json")).expect("the seed parses");
    let mut events: Vec<&str> = seed["rulesets"]
        .as_array()
        .expect("rulesets")
        .iter()
        .filter_map(|r| r["event"].as_str())
        .collect();
    events.sort_unstable();
    let mut out = String::new();
    for event in events {
        writeln!(
            out,
            "== {event} special_op={}",
            FtMessages::of_contest(event).special_op()
        )
        .unwrap();
        for mode in ["FT8", "FT4", "FT2"] {
            for mycall in ["K1ABC", "K1ABC/QRP"] {
                let rs = ruleset_by_id(event, CURRENT_RULES_YEAR).expect("ruleset");
                let m = FtMessages::for_contest(rs, mode);
                writeln!(
                    out,
                    "-- {event} {mode} {mycall} cq_on_air={:?}",
                    m.cq_on_air(mycall, "FN42ab", "")
                )
                .unwrap();
                for (n, script) in SCRIPTS.iter().enumerate() {
                    writeln!(out, " run script {n}").unwrap();
                    run(&mut out, event, mode, mycall, true, None, script);
                    for grid in ["EN37", ""] {
                        writeln!(out, " s&p script {n} cq grid {grid:?}").unwrap();
                        run(&mut out, event, mode, mycall, false, Some(grid), script);
                    }
                }
            }
        }
    }
    out
}

/// ⛔ Every shipped contest sends, logs and reports exactly what the fixture says.
#[test]
fn shipped_contests_ft_messages_are_byte_identical() {
    let got = transcript();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    if std::env::var_os("NEXUS_BLESS_FT_GOLDEN").is_some() {
        std::fs::write(&path, &got).expect("write the fixture");
        return;
    }
    let want = std::fs::read_to_string(&path).expect("the fixture exists");
    if got != want {
        let line = got
            .lines()
            .zip(want.lines())
            .position(|(g, w)| g != w)
            .unwrap_or_else(|| got.lines().count().min(want.lines().count()));
        panic!(
            "a shipped contest's FT8/FT4 messages changed, first at line {}:\n got: {:?}\nwant: {:?}",
            line + 1,
            got.lines().nth(line),
            want.lines().nth(line)
        );
    }
}
