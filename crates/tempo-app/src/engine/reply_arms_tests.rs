//! A logger's WSJT-X UDP Reply arms TX only when it works the station it names
//! ([`Engine::answer_udp_reply`]). A Reply that works nobody — on this station's own line (its CQ
//! or an over it sent, echoed back by the logger), or on a tier that answers no one (a beacon, or
//! a receive-only mode) — changes nothing and arms nothing. Before, such a Reply came back `Ok`
//! from the double-click it shares and TX was armed for whatever contact was already up: an old,
//! given-up contact went back on the air because a logger clicked our own CQ.
//!
//! A Reply that does work a station still arms TX whatever "Double-click on call sets Tx enable"
//! says (the maintainer's call, which WSJT-X does not make), and a Reply on a CQ while a contest
//! runs still goes to the contest's S&P (`contest_ft_messages_tests`).

use super::tests::dec_snr;
use super::*;

const ME: &str = "KD9TAW";
const DX: &str = "W1AW";
const DX_CQ: &str = "CQ W1AW FN31";

/// What the station sends next.
fn tx_now(e: &Engine) -> String {
    e.snapshot()
        .qso
        .as_ref()
        .and_then(|q| q.tx_now.clone())
        .unwrap_or_default()
}

/// Everything a Reply could move: the contact, its next over, TX, the snappy key, and where
/// RX and TX sit.
fn state(e: &mut Engine) -> (Option<String>, String, bool, bool, f32, f32) {
    (
        e.qso_dxcall().map(str::to_string),
        tx_now(e),
        e.tx_enabled(),
        e.take_immediate_tx(),
        e.rx_offset_hz,
        e.tx_offset_hz,
    )
}

/// A contact with W1AW, started from its CQ and then left: the operator turned TX off (not a
/// stop, which refuses every Reply by itself). Its Tx1 is still queued.
fn left_contact() -> Engine {
    let mut e = Engine::new(ME, "EN52", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.ingest_decodes_for_test(&[dec_snr(DX_CQ, -12)], 11);
    e.work_station(DX, None, Some(DX_CQ), Some(-12), Some(1500.0))
        .expect("premise: W1AW worked");
    assert_eq!(tx_now(&e), "W1AW KD9TAW EN52", "premise: Tx1 queued");
    e.set_tx_enabled(false);
    let _ = e.take_immediate_tx();
    e
}

/// ⛔ (a) A Reply on our own CQ, or on an over we sent, echoed back by the logger: nothing
/// changes, TX stays off, and the contact left behind is not put back on the air.
#[test]
fn a_reply_on_our_own_line_arms_nothing_and_keeps_the_contact() {
    for own in ["CQ KD9TAW EN52", "W1AW KD9TAW EN52", "W1AW KD9TAW -12"] {
        let mut e = left_contact();
        e.ingest_decodes_for_test(&[dec_snr(own, 0)], 13);
        let before = state(&mut e);
        let r = e.answer_udp_reply(ME, own, 0, Some(800.0));
        assert_eq!(state(&mut e), before, "{own:?}: nothing changed");
        assert!(r.is_err(), "{own:?}: refused for the diag log, got {r:?}");
        assert!(!e.tx_enabled(), "{own:?}: TX stays off");
        assert!(
            (14..18).all(|s| e.poll_tx(s).is_empty()),
            "{own:?}: the left contact never keys"
        );
    }
}

/// Every tier the Reply is checked on.
fn tiers_where(pick: impl Fn(&Engine, Tier) -> bool) -> Vec<Tier> {
    let e = Engine::new(ME, "EN52", 0);
    Tier::ALL.into_iter().filter(|t| pick(&e, *t)).collect()
}

/// ⛔ (c) A beacon (WSPR, FST4W) answers no one: a Reply there arms nothing. The licence does
/// not stop it: TX armed on a beacon tier is the beacon schedule, which keys.
#[test]
fn a_reply_on_a_beacon_tier_arms_nothing() {
    let beacons = tiers_where(|e, t| e.tier_is_beacon(t));
    assert_eq!(
        beacons,
        [Tier::Fst4w, Tier::Wspr],
        "premise: the beacon tiers"
    );
    for tier in beacons {
        let mut e = Engine::new(ME, "EN52", 0);
        e.set_tier(tier);
        e.set_frequency(14.0956, "20m", "USB");
        e.set_tx_enabled(true);
        assert!(
            e.tx_enabled() && e.tx_allowed(),
            "{tier:?} premise: the beacon may key here, so only the Reply's refusal stops it"
        );
        e.set_tx_enabled(false);
        let before = state(&mut e);
        let r = e.answer_udp_reply("K1ABC", "K1ABC FN42 37", -20, Some(1500.0));
        assert_eq!(state(&mut e), before, "{tier:?}: nothing changed");
        assert!(r.is_err(), "{tier:?}: refused for the diag log, got {r:?}");
    }
}

/// ⛔ (b) A receive-only tier answers no one: a Reply there arms nothing, and TX could not be
/// armed there anyway. Vacuous while every shipped mode transmits (the list is empty); it is the
/// contract the next receive-only mode inherits.
#[test]
fn a_reply_on_a_receive_only_tier_arms_nothing() {
    for tier in tiers_where(|e, t| e.tier_is_rx_only(t)) {
        let mut e = Engine::new(ME, "EN52", 0);
        e.set_tier(tier);
        e.ingest_decodes_for_test(&[dec_snr(DX_CQ, -12)], 11);
        let before = state(&mut e);
        let r = e.answer_udp_reply(DX, DX_CQ, -12, Some(1500.0));
        assert_eq!(state(&mut e), before, "{tier:?}: nothing changed");
        assert!(r.is_err(), "{tier:?}: refused, got {r:?}");
        e.set_tx_enabled(true);
        assert!(
            !e.tx_enabled(),
            "{tier:?}: the tier gate refuses TX by itself"
        );
        assert!(e.plan_tx(12).is_none(), "{tier:?}: nothing to key");
    }
}

/// (d) A Reply on a station's CQ works it and arms TX, with TX off before and "Double-click on
/// call sets Tx enable" off: the contact, its Tx1, the snappy key, RX and TX on the station.
#[test]
fn a_reply_on_a_cq_works_the_station_and_arms() {
    let mut e = Engine::new(ME, "EN52", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.settings.double_click_sets_tx = false;
    e.ingest_decodes_for_test(&[dec_snr(DX_CQ, -12)], 11);
    assert!(!e.tx_enabled(), "premise: TX off");
    e.answer_udp_reply(DX, DX_CQ, -12, Some(1234.0))
        .expect("the Reply is taken");
    assert_eq!(
        state(&mut e),
        (
            Some(DX.to_string()),
            "W1AW KD9TAW EN52".to_string(),
            true,
            true,
            1234.0,
            1234.0
        )
    );
}

/// (d) A Reply on another station's line while a contact is left up replaces it, as before:
/// the new station is worked and TX armed.
#[test]
fn a_reply_on_another_station_replaces_the_left_contact_and_arms() {
    let mut e = left_contact();
    e.ingest_decodes_for_test(&[dec_snr("CQ K2DEF FN31", -9)], 13);
    e.answer_udp_reply("K2DEF", "CQ K2DEF FN31", -9, Some(1500.0))
        .expect("the Reply is taken");
    assert_eq!(e.qso_dxcall(), Some("K2DEF"));
    assert_eq!(tx_now(&e), "K2DEF KD9TAW EN52");
    assert!(e.tx_enabled());
}
