//! An FT Tx button (Tx1-Tx5, Alt+1..5, the pop-out's rows, a Remote press) arms TX only when it
//! works the station it names ([`Engine::override_next_tx_checked`]). A press that works nobody —
//! on this station's own call, or on a tier that answers no one (a beacon, or a receive-only
//! mode) — is refused, changes nothing and arms nothing. Before, a press on a configured WSPR or
//! FST4W beacon came back `Ok` from the double-click it shares and armed TX, and the beacon keyed
//! every slot with no contact behind it.

use super::tests::dec_snr;
use super::*;

const ME: &str = "KD9TAW";
const DX: &str = "W1AW";
const TX1: &str = "W1AW KD9TAW EN52";

/// What the station sends next.
fn tx_now(e: &Engine) -> String {
    e.snapshot()
        .qso
        .as_ref()
        .and_then(|q| q.tx_now.clone())
        .unwrap_or_default()
}

/// Everything a press could move: the contact, its next over, TX, the snappy key, and where
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

/// Every tier the press is checked on.
fn tiers_where(pick: impl Fn(&Engine, Tier) -> bool) -> Vec<Tier> {
    let e = Engine::new(ME, "EN52", 0);
    Tier::ALL.into_iter().filter(|t| pick(&e, *t)).collect()
}

/// A beacon on `tier` the operator has set up to transmit (Transmit % and power set), TX off.
fn configured_beacon(tier: Tier) -> Engine {
    let mut e = Engine::new(ME, "EN52", 0);
    e.set_tier(tier);
    e.set_frequency(14.0956, "20m", "USB");
    e.settings.beacon_tx_percent = 100;
    e.settings.beacon_power_dbm = 37;
    e
}

/// The overs keyed over the next eight slots.
fn keyed(e: &mut Engine) -> usize {
    (12..20).filter_map(|s| e.plan_tx(s)).count()
}

/// ⛔ A Tx press on a configured beacon (WSPR, FST4W) is refused: nothing changes, TX stays off
/// and the beacon keys nothing. PREMISE: armed, the same beacon keys, so only the refusal stops it.
#[test]
fn a_tx_press_on_a_configured_beacon_is_refused_and_keys_nothing() {
    let beacons = tiers_where(|e, t| e.tier_is_beacon(t));
    assert_eq!(
        beacons,
        [Tier::Fst4w, Tier::Wspr],
        "premise: the beacon tiers"
    );
    for tier in beacons {
        let mut armed = configured_beacon(tier);
        armed.set_tx_enabled(true);
        assert!(
            keyed(&mut armed) > 0,
            "{tier:?} premise: armed, the beacon keys"
        );

        let mut e = configured_beacon(tier);
        let before = state(&mut e);
        let r = e.override_next_tx_checked("K1ABC", None, "K1ABC KD9TAW EN52");
        assert!(r.is_err(), "{tier:?}: refused, got {r:?}");
        assert_eq!(state(&mut e), before, "{tier:?}: nothing changed");
        assert!(!e.tx_enabled(), "{tier:?}: TX stays off");
        assert_eq!(keyed(&mut e), 0, "{tier:?}: nothing keyed");
    }
}

/// ⛔ A Tx press on this station's own call is refused: a contact left up (TX turned off) keeps
/// its over and is not put back on the air.
#[test]
fn a_tx_press_on_our_own_call_is_refused_and_keeps_the_contact() {
    let mut e = Engine::new(ME, "EN52", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.ingest_decodes_for_test(&[dec_snr("CQ W1AW FN31", -12)], 11);
    e.override_next_tx_checked(DX, None, TX1)
        .expect("premise: W1AW worked");
    e.set_tx_enabled(false);
    let _ = e.take_immediate_tx();
    let before = state(&mut e);
    let r = e.override_next_tx_checked(ME, None, "KD9TAW KD9TAW 73");
    assert!(r.is_err(), "refused, got {r:?}");
    assert_eq!(state(&mut e), before, "nothing changed");
    assert!(
        (14..18).all(|s| e.poll_tx(s).is_empty()),
        "the left contact never keys"
    );
}

/// ⛔ A receive-only tier answers no one: a Tx press there is refused and arms nothing. Vacuous
/// while every shipped mode transmits (the list is empty); it is the contract the next
/// receive-only mode inherits.
#[test]
fn a_tx_press_on_a_receive_only_tier_is_refused() {
    for tier in tiers_where(|e, t| e.tier_is_rx_only(t)) {
        let mut e = Engine::new(ME, "EN52", 0);
        e.set_tier(tier);
        let before = state(&mut e);
        let r = e.override_next_tx_checked(DX, None, TX1);
        assert!(r.is_err(), "{tier:?}: refused, got {r:?}");
        assert_eq!(state(&mut e), before, "{tier:?}: nothing changed");
        assert!(e.plan_tx(12).is_none(), "{tier:?}: nothing to key");
    }
}

/// CONTROL: an FT8 Tx press with TX off works the station and arms, as it always has: the
/// contact, the pressed over, TX on and the snappy key.
#[test]
fn an_ft8_tx_press_works_the_station_and_arms() {
    let mut e = Engine::new(ME, "EN52", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    assert!(!e.tx_enabled(), "premise: TX off");
    e.override_next_tx_checked(DX, Some("FN31"), TX1)
        .expect("the press is taken");
    assert_eq!(e.qso_dxcall(), Some(DX));
    assert_eq!(tx_now(&e), TX1);
    assert!(e.tx_enabled(), "armed");
    assert!(e.take_immediate_tx(), "fires this period when it fits");
    assert!((12..16).any(|s| !e.poll_tx(s).is_empty()), "the over keys");
}
