//! FT8/FT4 click-to-work from a browser: the station tunes, sets the Digital section and the
//! tier through the same verbs the desktop Work uses, but never arms transmit or starts a QSO.
use super::*;
use crate::dto::Tier;

fn station(section: &str, tier: Tier) -> Station {
    let mut s = Station::new(OperatingMode::Digital);
    s.engine.set_tier(tier);
    if section != "digital" {
        s.engine.set_operating_mode(section, false);
    }
    s.engine.set_tx_enabled(false);
    s.engine.take_immediate_retune();
    resample(&mut s);
    s
}

fn resample(s: &mut Station) {
    let hz = s.engine.settings.dial_hz();
    let mode = s.engine.rig_mode_effective();
    s.sample(hz, &mode);
}

fn queue(
    s: &mut Station,
    tier: Tier,
    dial: f64,
    band: &str,
    call: &str,
) -> Result<Completion, Reason> {
    let connection = s
        .engine
        .remote_monitor_observation()
        .radio
        .readings
        .cat
        .unwrap()
        .connection_generation;
    s.engine.queue_remote_digital_spot(
        tier,
        dial,
        band,
        call,
        connection,
        s.authority.permit(unexpired_deadline()).unwrap(),
    )
}

#[test]
fn digital_spot_commit_matches_native_tiered_work_without_arming_or_calling() {
    for (section, from, tier, dial, band) in [
        ("digital", Tier::Ft8, Tier::Ft4, 14.0815, "20m"),
        ("digital", Tier::Ft8, Tier::Ft8, 14.0765, "20m"),
        ("phone", Tier::Ft8, Tier::Ft4, 7.0485, "40m"),
        ("cw", Tier::Ft4, Tier::Ft8, 21.0765, "15m"),
    ] {
        let mut s = station(section, from);
        let mut native = station(section, from);
        let before = serde_json::to_value(&s.engine.settings).unwrap();
        let tick = s.engine.work_tick;
        let receipt = queue(&mut s, tier, dial, band, "ja2def/p").unwrap();
        // Admission changes nothing: no Settings, tier, tick, file or transmit state.
        assert_eq!(serde_json::to_value(&s.engine.settings).unwrap(), before);
        assert_eq!(s.engine.tier(), from);
        assert_eq!(s.engine.work_tick, tick);
        assert!(!s.path.exists());
        assert!(!s.engine.tx_enabled());
        let request = s.engine.take_remote_radio().unwrap();

        native
            .engine
            .work_spot_tiered(Some(tier), "digital", dial, band, None);
        native.engine.note_work_call(Some("JA2DEF/P".into()));
        assert!(
            !native.engine.tx_enabled(),
            "native digital Work never arms"
        );
        assert_eq!(
            request.target(),
            (
                native.engine.settings.dial_hz(),
                native.engine.rig_mode_effective().as_str()
            )
        );
        let target = request.target().1.to_owned();
        let power = request.power_limit();
        request.permission().begin_write(Instant::now()).unwrap();
        s.sample((dial * 1e6).round() as u64, &target);
        assert!(request.commit_readback(&mut s.engine, power));
        assert!(matches!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        ));
        assert_eq!(s.engine.tier(), tier);
        assert_eq!(s.engine.settings.operating_mode, OperatingMode::Digital);
        assert_eq!(s.engine.settings.dial_hz(), (dial * 1e6).round() as u64);
        // Never keys, never starts a QSO: the latch stays down, nobody owns the transmitter,
        // and the QSO state is exactly what the desktop's own Work leaves (no call, no CQ).
        assert!(!s.engine.tx_enabled());
        assert!(s.engine.tx_owner().is_none());
        assert_eq!(
            serde_json::to_value(&s.engine.snapshot().qso).unwrap(),
            serde_json::to_value(&native.engine.snapshot().qso).unwrap()
        );
        assert!(
            !s.engine.take_immediate_retune(),
            "readback consumes the sole QSY"
        );
        assert_eq!(s.engine.work_tick, tick + 1);
        assert_eq!(s.engine.work_view.as_deref(), Some("digital"));
        assert_eq!(s.engine.work_call.as_deref(), Some("JA2DEF/P"));
        native.engine.settings.save(&native.path).unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&s.path).unwrap()).unwrap();
        let local: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&native.path).unwrap()).unwrap();
        assert_eq!(saved, local, "the saved station matches the desktop Work");
    }
}

#[test]
fn digital_spot_is_refused_while_transmit_is_armed_with_idle_as_the_positive_control() {
    for armed in [false, true] {
        let mut s = station("digital", Tier::Ft8);
        if armed {
            s.engine.set_tx_enabled(true);
            s.engine.take_immediate_retune();
            resample(&mut s);
        }
        let result = queue(&mut s, Tier::Ft4, 14.0815, "20m", "JA2DEF");
        if armed {
            assert!(matches!(result, Err(Reason::StationBusy)));
            assert!(s.engine.take_remote_radio().is_none());
            assert_eq!(s.engine.tier(), Tier::Ft8);
            assert!(
                s.engine.tx_enabled(),
                "a refusal never touches the operator's latch"
            );
        } else {
            assert!(result.is_ok());
            assert!(s.engine.take_remote_radio().is_some());
            assert!(!s.engine.tx_enabled());
        }
    }
}

#[test]
fn digital_spot_rejects_other_tiers_and_invalid_targets_before_changing_the_station() {
    for (tier, dial, band, call) in [
        (Tier::Ft2, 14.080, "20m", "JA2DEF"),
        (Tier::Js8, 14.078, "20m", "JA2DEF"),
        (Tier::TempoFast, 14.074, "20m", "JA2DEF"),
        (Tier::Ft8, f64::NAN, "20m", "JA2DEF"),
        (Tier::Ft8, 14.074, "40m", "JA2DEF"),
        (Tier::Ft8, 10.0, "", "JA2DEF"),
        (Tier::Ft8, 14.074, "20m", ""),
        (Tier::Ft8, 14.074, "20m", "JA 2DEF"),
        (
            Tier::Ft8,
            14.074,
            "20m",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ),
    ] {
        let mut s = station("digital", Tier::Ft8);
        let before = serde_json::to_value(&s.engine.settings).unwrap();
        assert!(matches!(
            queue(&mut s, tier, dial, band, call),
            Err(Reason::InvalidAction)
        ));
        assert_eq!(serde_json::to_value(&s.engine.settings).unwrap(), before);
        assert!(s.engine.take_remote_radio().is_none());
        assert_eq!(s.engine.tier(), Tier::Ft8);
        assert!(!s.path.exists());
    }
}

#[test]
fn a_tier_change_waits_for_no_decode_and_cannot_follow_a_later_local_tier_pick() {
    // A running decode holds the source: a tier change is refused, the same tier is not.
    let mut s = station("digital", Tier::Ft8);
    let source = s.engine.source.clone();
    {
        let _held = source.lock().unwrap();
        assert!(matches!(
            queue(&mut s, Tier::Ft4, 14.0815, "20m", "JA2DEF"),
            Err(Reason::StationBusy)
        ));
        assert!(s.engine.take_remote_radio().is_none());
        assert!(queue(&mut s, Tier::Ft8, 14.0765, "20m", "JA2DEF").is_ok());
        assert!(s.engine.take_remote_radio().is_some());
    }
    // A local tier pick after admission retires the request instead of stacking on it.
    let mut s = station("digital", Tier::Ft8);
    let receipt = queue(&mut s, Tier::Ft4, 14.0815, "20m", "JA2DEF").unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    s.engine.set_tier(Tier::Ft4);
    s.engine.take_immediate_retune();
    request.permission().begin_write(Instant::now()).ok();
    s.sample(14_081_500, request.target().1);
    assert!(!request.commit(&mut s.engine));
    assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
    assert_eq!(s.engine.work_tick, 0);
    assert!(s.engine.work_call.is_none());
    assert!(!s.path.exists());
}

#[test]
fn a_routed_digital_spot_that_would_change_tier_has_no_transaction() {
    let mut s = station("digital", Tier::Ft8);
    s.engine.configure_remote_selection_host(true);
    let other = s.engine.add_radio();
    s.engine.set_radio_bands(other, vec!["20m".into()]);
    resample(&mut s);
    assert!(matches!(
        queue(&mut s, Tier::Ft4, 14.0815, "20m", "JA2DEF"),
        Err(Reason::UnsupportedAction)
    ));
    assert!(s.engine.take_remote_radio().is_none());
    assert!(s.engine.take_remote_radio_selection().is_none());
    assert_eq!(s.engine.tier(), Tier::Ft8);
    // Positive control: the same-tier spot rides the existing routed Work transaction.
    assert!(queue(&mut s, Tier::Ft8, 14.0765, "20m", "JA2DEF").is_ok());
    assert!(s.engine.take_remote_radio_selection().is_some());
    assert!(!s.engine.tx_enabled());
}
