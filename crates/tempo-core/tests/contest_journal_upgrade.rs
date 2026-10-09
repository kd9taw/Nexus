//! ⭐ **A contest journal written by 1.17.0 opens, and scores, exactly as 1.17.0 read it.**
//!
//! The journals in `fixtures/contest-journals/` were WRITTEN BY 1.17.0: a scratch program
//! built against the `v1.17.0` tag's own `tempo-core` (with that tag's `Cargo.lock`) logged
//! the contacts below and saved `FieldDayLog::adif()`, then read each journal back the way a
//! restart does and saved its report — every row by value, the counts, the score, the worked
//! counties, the seq and serial runs, and the Cabrillo QSO lines. The test reads the same
//! journals with THIS build and renders the same report, so the comparison is 1.17.0's own
//! answer against this one, never a reading of the code.
//!
//! - `ilqp-1.17.0`: the Illinois QSO Party from Kane County — two contacts, a county line
//!   (one contact, two rows, one time), a phone contact, and a dupe that was refused.
//! - `ss-1.17.0`: Sweepstakes CW — three contacts with issued serials, the third a logged dupe.
use tempo_core::contest::{ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

fn session(contest: &str) -> ContestSession {
    let (id, station) = match contest {
        "ilqp" => (
            "ilqp",
            StationData {
                contest_qth_state: "IL".into(),
                contest_qth_county: "KANE".into(),
                ..Default::default()
            },
        ),
        "ss" => (
            "arrlss_cw",
            StationData {
                mycall: "W9XYZ".into(),
                fd_section: "WI".into(),
                contest_check: "74".into(),
                contest_category_operator: "SINGLE-OP".into(),
                contest_category_power: "LOW".into(),
                contest_category_assisted: "NON-ASSISTED".into(),
                ..Default::default()
            },
        ),
        other => panic!("no such fixture contest {other}"),
    };
    let rs = ruleset_by_id(id, CURRENT_RULES_YEAR).expect("shipped");
    ContestSession::for_ruleset(rs, &station).expect("declared")
}

/// The 1.17.0 program's report, line for line.
fn report(log: &FieldDayLog) -> String {
    let mut s = String::new();
    for q in log.qsos() {
        let rx: Vec<String> =
            q.rx.iter()
                .map(|v| format!("{}={}", v.key, v.raw))
                .collect();
        let tx: Vec<String> =
            q.tx.iter()
                .map(|v| format!("{}={}", v.key, v.raw))
                .collect();
        s.push_str(&format!(
            "ROW {} {} {} {} when={} seq={} dupe={} rx=[{}] tx=[{}]\n",
            q.call,
            q.band,
            q.mode,
            q.submode,
            q.when_unix,
            q.seq,
            q.dupe,
            rx.join(","),
            tx.join(",")
        ));
    }
    let (pts, powered, mults, total) = log.ruleset().scoring.score(log.score_rows(), 1);
    s.push_str(&format!("QSO_COUNT {}\n", log.qso_count()));
    s.push_str(&format!("SCORE {pts} {powered} {mults:?} {total}\n"));
    s.push_str(&format!("WORKED_QTH {:?}\n", log.worked_values("QTH")));
    s.push_str(&format!("MAX_SEQ {}\n", log.max_seq()));
    s.push_str(&format!("NEXT_SERIAL {}\n", log.session.next_serial));
    let cab = log.cabrillo(14_000).unwrap_or_else(|e| e);
    for l in cab.lines().filter(|l| l.starts_with("QSO:")) {
        s.push_str(l);
        s.push('\n');
    }
    s
}

#[test]
fn a_contest_journal_written_by_1_17_0_opens_and_scores_unchanged() {
    for (contest, journal, expected) in [
        (
            "ilqp",
            include_str!("fixtures/contest-journals/ilqp-1.17.0.adi"),
            include_str!("fixtures/contest-journals/ilqp-1.17.0.report"),
        ),
        (
            "ss",
            include_str!("fixtures/contest-journals/ss-1.17.0.adi"),
            include_str!("fixtures/contest-journals/ss-1.17.0.report"),
        ),
    ] {
        let mut log = FieldDayLog::new("W9XYZ", session(contest), "20m");
        let merged = log.merge_adif(journal, 0);
        assert_eq!(merged.unreadable, 0, "{contest}: every record read");
        assert_eq!(
            report(&log),
            expected,
            "{contest}: this build reads 1.17.0's journal as 1.17.0 does"
        );
        assert!(
            log.removed().is_empty(),
            "{contest}: nothing in an older journal comes back removed"
        );
        assert_eq!(
            log.adif(),
            journal,
            "{contest}: and the next save writes 1.17.0's bytes back unchanged"
        );
    }
}
