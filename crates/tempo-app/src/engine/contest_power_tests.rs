//! One power story, per contest, by value. Settings ▸ Contesting has three power settings, and each
//! does one thing:
//!
//! - **Power category** (`contest_category_power`) is what the entry's log DECLARES: Cabrillo's
//!   `CATEGORY-POWER` where the sponsor's log has the line (Winter Field Day taking QRP or LOW
//!   only), the Illinois QSO Party's `QRP-COMPETITION`, and Sweepstakes' precedence letter, which
//!   also goes on the air.
//! - **Power sent** (`contest_power`) is a slot an exchange may SEND. No shipped contest's does, so
//!   it reaches no log and no exchange, and Settings shows it for none of them
//!   (`CONTESTS_SENDING_POWER` in ui/src/fdEvent.ts, held to the seed in `fd_rules`).
//! - **Power multiplier** (`fd_power_mult`) is ARRL Field Day's SCORE, and reaches no log either.
//!
//! Every seeded contest this test binary can start is run, each with every piece of station data
//! any of them sends filled in, and what it writes is read back. The CQ World-Wide and WPX
//! contests cannot start here ([`NEEDS_A_COUNTRY_FILE`]); tempo-core's `tests/cqww.rs` runs the
//! same story for them with a country file installed.

use super::*;

/// An engine in `event`'s S&P, with `power` declared, `sent` as Power sent and `mult` as Field
/// Day's power multiplier, and every other piece of station data a shipped contest sends filled
/// in: Wisconsin, which is out of state for every party.
fn running(event: &str, power: &str, sent: &str, mult: u32) -> Engine {
    let mut e = Engine::new("W9XYZ", "EN52", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_event = event.into();
    s.fd_class = if event == "wfd" { "1H" } else { "1D" }.into();
    s.fd_section = "WI".into();
    s.contest_qth_state = "WI".into();
    s.contest_check = "74".into();
    s.contest_cq_zone = 4;
    s.contest_itu_zone = 8;
    s.contest_category_operator = "SINGLE-OP".into();
    s.contest_category_power = power.into();
    s.contest_category_assisted = "NON-ASSISTED".into();
    s.contest_power = sent.into();
    s.fd_power_mult = mult;
    e.apply_settings(s);
    if let Err(r) = e.set_mode("fieldday-sp") {
        panic!("{event} does not start: {r}");
    }
    e
}

/// The Cabrillo log `e` exports, and the exchange it is about to send.
fn written(e: &Engine) -> (String, String) {
    let cab = e.export_log("cabrillo").expect("a running contest exports");
    let sent = e
        .contest_sent_exchange()
        .expect("a running contest sends an exchange");
    (cab, sent)
}

/// The header line starting `tag`, or `""` when the log has none.
fn header(cab: &str, tag: &str) -> String {
    cab.lines()
        .find(|l| l.starts_with(tag))
        .unwrap_or("")
        .to_string()
}

/// The contests this test binary cannot start: CQ World-Wide and WPX score each contact by where
/// the other station is and refuse to run without a country file, and none is loaded here (the
/// call resolver is process-wide, so installing one would change every other test in this binary).
const NEEDS_A_COUNTRY_FILE: [&str; 5] =
    ["cqww_cw", "cqww_ssb", "cqww_rtty", "cqwpx_cw", "cqwpx_ssb"];

/// The seeded contests, in the rules file's order, but for [`NEEDS_A_COUNTRY_FILE`].
fn contests() -> Vec<String> {
    seeded()
        .into_iter()
        .filter(|e| !NEEDS_A_COUNTRY_FILE.contains(&e.as_str()))
        .collect()
}

/// Every seeded contest, in the rules file's order.
fn seeded() -> Vec<String> {
    let seed: serde_json::Value =
        serde_json::from_str(include_str!("../../../tempo-core/src/fd_rules.seed.json"))
            .expect("the seed is JSON");
    let events: Vec<String> = seed["rulesets"]
        .as_array()
        .expect("the seed lists rulesets")
        .iter()
        .filter_map(|r| r["event"].as_str().map(str::to_string))
        .collect();
    // A reader that found nothing would pass every comparison below.
    assert!(events.len() >= 18, "parsed only {} rulesets", events.len());
    events
}

/// ⭐ **Power category is what the log declares, contest by contest.** QRP, LOW and HIGH each
/// written where the sponsor's log has a power line, and nowhere else.
#[test]
fn power_category_is_what_each_contests_log_declares() {
    let mut got = Vec::new();
    for event in contests() {
        for power in ["QRP", "LOW", "HIGH"] {
            let (cab, _) = written(&running(&event, power, "", 2));
            let lines = [
                header(&cab, "CATEGORY-POWER:"),
                header(&cab, "QRP-COMPETITION:"),
            ]
            .into_iter()
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" | ");
            got.push(format!("{event} {power}: {lines}"));
        }
    }
    let mut want = Vec::new();
    for event in contests() {
        for power in ["QRP", "LOW", "HIGH"] {
            let lines = match event.as_str() {
                // The sponsor's log lists CATEGORY-POWER.
                "nyqp" => format!("CATEGORY-POWER: {power}"),
                // The ARRL RTTY Roundup's QRP, LP and HP sub-categories (HCAT.1.3, HCAT.2.4).
                "arrlrtty" => format!("CATEGORY-POWER: {power}"),
                // Winter Field Day takes QRP or LOW, and HIGH claims nothing.
                "wfd" if power != "HIGH" => format!("CATEGORY-POWER: {power}"),
                // The Illinois QSO Party certifies a QRP entry instead.
                "ilqp" if power == "QRP" => "QRP-COMPETITION: YES".to_string(),
                _ => String::new(),
            };
            want.push(format!("{event} {power}: {lines}"));
        }
    }
    assert_eq!(got, want);
}

/// ⭐ **Sweepstakes puts the declared power on the air**, as its precedence letter (QRP is Q, low
/// power A, high power B); every other contest sends the same exchange whatever is declared.
#[test]
fn only_sweepstakes_sends_the_declared_power() {
    let mut varies = Vec::new();
    for event in contests() {
        let sent: Vec<String> = ["QRP", "LOW", "HIGH"]
            .iter()
            .map(|p| written(&running(&event, p, "", 2)).1)
            .collect();
        if sent[0] != sent[1] || sent[1] != sent[2] {
            varies.push((event, sent));
        }
    }
    let letters = |sent: &[String]| -> Vec<bool> {
        ["Q", "A", "B"]
            .iter()
            .zip(sent)
            .map(|(l, s)| s.split_whitespace().any(|w| w == *l))
            .collect()
    };
    assert_eq!(
        varies.iter().map(|(e, _)| e.as_str()).collect::<Vec<_>>(),
        ["arrlss_cw", "arrlss_ssb"]
    );
    for (event, sent) in &varies {
        assert_eq!(letters(sent), [true, true, true], "{event}: {sent:?}");
    }
}

/// ⭐ **Power sent and the power multiplier reach no shipped contest's log or exchange**: the
/// Cabrillo file and the sent exchange are the same, byte for byte, whatever either holds.
#[test]
fn power_sent_and_the_multiplier_reach_no_log_and_no_exchange() {
    let mut differs = Vec::new();
    for event in contests() {
        let plain = written(&running(&event, "LOW", "", 2));
        for (sent, mult) in [("KW", 2), ("", 5), ("", 1), ("KW", 5)] {
            if written(&running(&event, "LOW", sent, mult)) != plain {
                differs.push(format!("{event}: Power sent {sent:?}, multiplier {mult}"));
            }
        }
    }
    assert_eq!(differs, Vec::<String>::new());
}

/// POSITIVE CONTROL for the comparison above: it sees a setting that DOES reach the log. Power
/// category changes the New York QSO Party's file, so "the same, byte for byte" is not vacuous.
#[test]
fn control_the_comparison_sees_a_change_in_the_log() {
    let low = written(&running("nyqp", "LOW", "", 2));
    let high = written(&running("nyqp", "HIGH", "", 2));
    assert_ne!(low.0, high.0);
    assert_eq!(header(&high.0, "CATEGORY-POWER:"), "CATEGORY-POWER: HIGH");
}

/// The contests left out above are left out for the reason given, and only they: each refuses to
/// start here with the country-file sentence, and every other seeded contest starts.
#[test]
fn the_contests_left_out_are_the_ones_that_need_a_country_file() {
    let mut refused = Vec::new();
    for event in seeded() {
        let mut e = Engine::new("W9XYZ", "EN52", 0);
        let mut s = e.settings().clone();
        s.fd_active = true;
        s.fd_event = event.clone();
        s.fd_class = if event == "wfd" { "1H" } else { "1D" }.into();
        s.fd_section = "WI".into();
        s.contest_qth_state = "WI".into();
        s.contest_check = "74".into();
        s.contest_cq_zone = 4;
        s.contest_category_operator = "SINGLE-OP".into();
        s.contest_category_power = "LOW".into();
        s.contest_category_assisted = "NON-ASSISTED".into();
        e.apply_settings(s);
        if let Err(r) = e.set_mode("fieldday-sp") {
            assert!(r.contains("no country file loaded"), "{event}: {r}");
            refused.push(event);
        }
    }
    assert_eq!(refused, NEEDS_A_COUNTRY_FILE);
}
