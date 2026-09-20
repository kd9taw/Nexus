//! ⭐ **A role that SENDS a serial always has one to send.**
//!
//! [`ContestSession::serial_now`] answers `None` when nothing in the composing exchange
//! carries a `Serial`-kinded slot, and `Engine::contest_sent_exchange` renders the slot
//! through a `filter_map` — so a `None` there would drop the serial from `{EXCH}`
//! silently and key a SHORT exchange. In CQ WPX the serial is most of the exchange:
//! `K1ABC 599` with no number, and nothing on screen to say so.
//!
//! That cannot happen, and the reason is structural rather than a property of today's
//! rules data. [`ContestSession::for_ruleset`] omits a sent slot in exactly one place —
//! when its value comes out empty AND the slot is not required — and a serial slot's
//! value is `session::sent_value`'s `"serial"` arm, an unconditional `"0"` that reads no
//! station data at all. `normalise_sent` truncates only `Grid` kinds, so the value stays
//! `"0"`, which is not empty, so the omission branch is never taken — whatever
//! `required` says.
//!
//! ⚠️ **What this file is FOR is the day that stops being true.** A rules file is data:
//! it may one day declare a `Serial` slot sourced from something other than `"serial"`,
//! and a source that resolves empty on an under-filled station would omit the slot. The
//! visible symptom would be the short exchange above; the real defect is worse, because
//! `compose_for` walks a clone of the same vector — it would issue nothing, the row
//! would be stamped with no serial, and the run would sit at 1 forever. So this asserts
//! the slot is THERE, per shipped ruleset, rather than asserting anything about sources.

use tempo_core::contest::{install_call_resolver, CallLocation, ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};

/// CQ WPX prices a contact by the relation between two stations, so its session refuses
/// to build with no country file. Process-wide, so once per test binary.
fn resolver() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        install_call_resolver(|_call| {
            Some(CallLocation {
                entity: "United States",
                continent: "NA",
                cq_zone: None,
            })
        })
        .expect("the first install in this test binary");
    });
}

/// An entrant with every sent-slot source filled, so a refusal here is about the slot
/// under test and never about an under-declared operator.
fn declared() -> StationData {
    StationData {
        mycall: "W9XYZ".into(),
        fd_class: "2A".into(),
        fd_section: "WI".into(),
        contest_qth_state: "WI".into(),
        contest_qth_county: "DANE".into(),
        contest_check: "74".into(),
        contest_cq_zone: "4".into(),
        contest_itu_zone: "7".into(),
        contest_power: "LOW".into(),
        contest_category_operator: "SINGLE-OP".into(),
        contest_category_power: "LOW".into(),
        contest_category_assisted: "NON-ASSISTED".into(),
        mygrid: "EN53".into(),
        ..Default::default()
    }
}

/// Every shipped ruleset whose exchange declares a serial. A sixth that ships without
/// joining this list is the gap this file cannot see, which is why the count is asserted
/// against the seed below.
const SERIAL_EVENTS: [&str; 5] = ["cqwpx_cw", "cqwpx_ssb", "arrlss_cw", "arrlss_ssb", "cqp"];

#[test]
fn every_shipped_serial_ruleset_composes_a_serial_to_send() {
    resolver();
    for event in SERIAL_EVENTS {
        let rs = ruleset_by_id(event, CURRENT_RULES_YEAR)
            .unwrap_or_else(|| panic!("{event} is a shipped ruleset"));
        let s = ContestSession::for_ruleset(rs, &declared())
            .unwrap_or_else(|e| panic!("{event}: a fully declared entrant builds: {e}"));

        // The role sends a serial…
        let sends_serial = s.role().sends.iter().any(|k| {
            matches!(
                s.exchange.field(k).map(|f| f.kind),
                Some(tempo_core::contest::FieldKind::Serial { .. })
            )
        });
        assert!(
            sends_serial,
            "{event}: expected a serial in the sent exchange"
        );

        // …so the composing exchange carries the slot, holding the placeholder…
        let slot = s
            .my_exchange
            .iter()
            .find(|v| {
                matches!(
                    s.exchange.field(v.key).map(|f| f.kind),
                    Some(tempo_core::contest::FieldKind::Serial { .. })
                )
            })
            .unwrap_or_else(|| {
                panic!("{event}: the role sends a serial the composing exchange has no slot for")
            });
        assert_eq!(slot.raw, "0", "{event}: the placeholder moved");

        // …and the number to send is the first of the run, not nothing.
        assert_eq!(
            s.serial_now(),
            Some(1),
            "{event}: nothing to key into the serial slot — `{{EXCH}}` would go out short"
        );
    }
}

/// POSITIVE CONTROL — `serial_now` really can answer `None`, so the assertions above are
/// about the serial rulesets and not about a function that never returns `None` at all.
///
/// Field Day sends a class and a section and no number, which is also why it is the
/// exchange the byte-pinned goldens ride on.
#[test]
fn an_exchange_with_no_serial_slot_has_no_number_to_show() {
    let s = ContestSession::field_day(tempo_core::fieldday::FdEvent::ArrlFd, "2A", "WI");
    assert!(s.role().sends.iter().all(|k| !matches!(
        s.exchange.field(k).map(|f| f.kind),
        Some(tempo_core::contest::FieldKind::Serial { .. })
    )));
    assert_eq!(s.serial_now(), None);
}

/// The list above is hand-written, and a sixth serial contest would not join it by
/// itself. The seed is the authority, so it is counted: ship one and this goes red with
/// the name of what to add.
#[test]
fn the_serial_ruleset_list_is_the_whole_of_what_ships() {
    let seed = include_str!("../src/fd_rules.seed.json");
    let v: serde_json::Value = serde_json::from_str(seed).expect("the seed parses");
    let mut found: Vec<String> = Vec::new();
    for rs in v["rulesets"].as_array().expect("rulesets is a list") {
        let has_serial = rs["exchange"]["fields"]
            .as_array()
            .map(|fs| fs.iter().any(|f| f["kind"]["type"] == "serial"))
            .unwrap_or(false);
        if has_serial {
            found.push(rs["event"].as_str().unwrap_or_default().to_string());
        }
    }
    found.sort();
    let mut expected: Vec<String> = SERIAL_EVENTS.iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(
        found, expected,
        "the seed's serial-bearing rulesets and SERIAL_EVENTS have diverged — add the \
         new one to the list so it is covered by the test above"
    );
}
