//! A CQ run never answers a caller whose call is this station's own base call, as WSJT-X 3.0.2
//! refuses one: `processMessage` returns at once on `m_bDoubleClicked && hiscall==m_baseCall`
//! (`widgets/mainwindow.cpp` 8977), and CQ: First and CQ: Max Dist set `m_bDoubleClicked` before
//! they call it (7024, 7054). Max Dist takes each caller in turn, so a line from my own call
//! never costs the period the station that really answered.

use super::tests::dec_snr;
use super::*;

const ME: &str = "KD9TAW";

/// What the station sends next.
fn tx_now(e: &Engine) -> String {
    e.snapshot()
        .qso
        .as_ref()
        .and_then(|q| q.tx_now.clone())
        .unwrap_or_default()
}

/// ⛔ Operate's Call CQ on FT8, first heard and with a best-caller choice: `KD9TAW KD9TAW` (with
/// a grid, with none, with a report) is passed by, and the CQ stays queued. In the same period
/// the real caller is answered, also where the line from my own call is the stronger and would
/// otherwise win the strongest-caller pick.
#[test]
fn a_cq_run_never_answers_my_own_call() {
    for strategy in ["first", "strongest"] {
        for mine in ["KD9TAW KD9TAW", "KD9TAW KD9TAW EN61", "KD9TAW KD9TAW -03"] {
            let what = format!("{strategy} {mine:?}");
            let mut e = Engine::new(ME, "EN61", 0);
            e.set_tier(Tier::Ft8);
            e.settings.best_caller = strategy.into();
            e.set_mode("qso-run").expect("CQ");
            let cq = tx_now(&e);
            e.ingest_decodes_for_test(&[dec_snr(mine, 5)], 3);
            assert_eq!(e.qso_dxcall(), None, "{what}: nobody worked");
            assert_eq!(tx_now(&e), cq, "{what}: the CQ again");
            e.ingest_decodes_for_test(&[dec_snr(mine, 5), dec_snr("KD9TAW W9XYZ EN37", -11)], 5);
            assert_eq!(e.qso_dxcall(), Some("W9XYZ"), "{what}");
            assert_eq!(tx_now(&e), "W9XYZ KD9TAW -11", "{what}");
        }
    }
}
