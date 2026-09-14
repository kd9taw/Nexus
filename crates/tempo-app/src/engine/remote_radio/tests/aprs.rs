//! Remote APRS tune: the desktop's `aprs_tune` (2 m FM simplex on a regional APRS channel) through
//! the readback transaction. Only the closed channel list is admitted, and nothing is armed,
//! keyed or queued for APRS transmit.
use super::*;

fn link(s: &Station) -> u64 {
    s.engine
        .remote_monitor_observation()
        .radio
        .readings
        .cat
        .unwrap()
        .connection_generation
}

fn station(mode: OperatingMode, dial: f64, band: &str) -> Station {
    let mut s = Station::new(mode);
    s.engine.set_frequency(dial, band, "USB");
    s.engine.take_immediate_retune();
    s.engine.set_tx_enabled(false);
    let hz = s.engine.settings.dial_hz();
    let rig = s.engine.rig_mode_effective();
    s.sample(hz, &rig);
    s
}

fn queue(s: &mut Station, dial: f64) -> Result<Completion, Reason> {
    let connection = link(s);
    s.engine.queue_remote_aprs_tune(
        dial,
        connection,
        s.authority.permit(unexpired_deadline()).unwrap(),
    )
}

#[test]
fn aprs_commit_matches_native_aprs_tune_and_never_arms() {
    for (section, dial, band) in [
        (OperatingMode::Phone, 145.5, "2m"),
        (OperatingMode::Digital, 14.074, "20m"),
    ] {
        let mut s = station(section, dial, band);
        let mut native = station(section, dial, band);
        let before = serde_json::to_value(&s.engine.settings).unwrap();
        let receipt = queue(&mut s, 144.39).unwrap();
        // Admission changes nothing: no Settings, no APRS context, no file, no transmit state.
        assert_eq!(serde_json::to_value(&s.engine.settings).unwrap(), before);
        assert!(!s.engine.aprs_fm);
        assert!(!s.path.exists());
        assert!(!s.engine.tx_enabled());
        let request = s.engine.take_remote_radio().unwrap();

        native.engine.aprs_tune(144.39).unwrap();
        assert_eq!(
            request.target(),
            (
                native.engine.settings.dial_hz(),
                native.engine.rig_mode_effective().as_str()
            )
        );
        let config = native.engine.fm_repeater_config();
        assert_eq!(
            request.repeater(),
            Some((config.0.as_str(), config.1, config.2))
        );
        request.permission().begin_write(Instant::now()).unwrap();
        let power = request.power_limit();
        s.sample(144_390_000, "FM");
        assert!(request.commit_tuning_readback(&mut s.engine, power, Some(("simplex", 0, 0.0))));
        assert!(matches!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        ));
        assert!(!s.engine.tx_enabled(), "an APRS tune never arms");
        assert!(s.engine.tx_owner().is_none());
        assert!(
            s.engine.aprs_tx_queue.is_empty(),
            "nothing queued to transmit"
        );
        assert!(
            !s.engine.take_immediate_retune(),
            "readback consumes the sole CAT QSY"
        );
        assert_eq!(
            s.engine.tx_gate_gen, native.engine.tx_gate_gen,
            "the QSY invalidates planned overs exactly as the desktop tune does"
        );
        assert_eq!(s.engine.aprs_fm, native.engine.aprs_fm);
        assert_eq!(s.engine.settings.operating_mode, section);
        for field in ["dialMhz", "band", "sideband", "phoneMode"] {
            assert_eq!(
                serde_json::to_value(&s.engine.settings).unwrap()[field],
                serde_json::to_value(&native.engine.settings).unwrap()[field],
                "{field}"
            );
        }
        assert!(
            s.path.exists(),
            "the confirmed tune persists like the desktop's"
        );
    }
}

#[test]
fn aprs_readback_must_confirm_fm_simplex_without_a_tone() {
    for actual in [
        None,
        Some(("plus", 600_000, 0.0)),
        Some(("simplex", 0, 100.0)),
    ] {
        let mut s = station(OperatingMode::Phone, 145.5, "2m");
        let receipt = queue(&mut s, 144.39).unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        request.permission().begin_write(Instant::now()).unwrap();
        s.sample(144_390_000, "FM");
        let power = request.power_limit();
        assert!(!request.commit_tuning_readback(&mut s.engine, power, actual));
        assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
        assert!(!s.engine.aprs_fm);
        assert_eq!(s.engine.settings.dial_hz(), 145_500_000);
        assert!(!s.path.exists());
        assert!(!s.engine.tx_enabled());
    }
}

#[test]
fn only_aprs_channels_on_a_radio_that_can_receive_them_are_admitted() {
    let mut s = station(OperatingMode::Phone, 145.5, "2m");
    for dial in [146.52, 144.391, 14.105, 0.0, f64::NAN, f64::INFINITY] {
        assert!(
            matches!(queue(&mut s, dial), Err(Reason::InvalidAction)),
            "{dial}"
        );
    }
    for dial in [144.39, 144.8, 145.175, 144.575, 144.66, 144.93, 145.57] {
        assert!(queue(&mut s, dial).is_ok(), "{dial}");
        s.engine.take_remote_radio().unwrap();
    }
    s.engine.set_tx_enabled(true);
    assert!(matches!(queue(&mut s, 144.39), Err(Reason::StationBusy)));
    assert!(s.engine.take_remote_radio().is_none());
    s.engine.set_tx_enabled(false);
    s.engine.take_immediate_retune();
    let hz = s.engine.settings.dial_hz();
    let rig = s.engine.rig_mode_effective();
    s.sample(hz, &rig);
    // Positive control: the same request with the latch down is admitted.
    assert!(queue(&mut s, 144.39).is_ok());
    s.engine.take_remote_radio().unwrap();

    // An HF-only radio provably cannot receive 2 m: refused before any write.
    let mut hf = station(OperatingMode::Phone, 14.2, "20m");
    hf.engine.rig_rx_ranges = Some(vec![(1_800_000, 54_000_000)]);
    assert!(matches!(
        queue(&mut hf, 144.39),
        Err(Reason::HardwareUnavailable)
    ));
    assert!(hf.engine.take_remote_radio().is_none());
    assert_eq!(hf.engine.settings.dial_hz(), 14_200_000);

    // FM routing would hand the tune to another radio: no remote transaction carries that.
    let mut routed = station(OperatingMode::Phone, 14.2, "20m");
    let other = routed.engine.add_radio();
    routed.engine.set_radio_bands(other, vec!["2m".into()]);
    assert!(matches!(
        queue(&mut routed, 144.39),
        Err(Reason::UnsupportedAction)
    ));
    assert!(routed.engine.take_remote_radio().is_none());
}

#[test]
fn an_aprs_hold_the_browser_set_does_not_strand_it_but_a_local_one_still_refuses() {
    let mut s = station(OperatingMode::Phone, 145.5, "2m");
    queue(&mut s, 144.39).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    request.permission().begin_write(Instant::now()).unwrap();
    s.sample(144_390_000, "FM");
    let power = request.power_limit();
    assert!(request.commit_tuning_readback(&mut s.engine, power, Some(("simplex", 0, 0.0))));
    assert!(s.engine.aprs_fm);
    s.sample(144_390_000, "FM");
    let connection = link(&s);
    // The browser's own APRS context does not strand it: a retune ends it as the desktop QSY does.
    s.engine
        .queue_remote_frequency(
            146.52,
            "2m",
            "FM",
            connection,
            s.authority.permit(unexpired_deadline()).unwrap(),
        )
        .unwrap();
    s.engine.take_remote_radio().unwrap();

    // Positive control: once the station itself re-tunes APRS on that same channel, the context is
    // the station's again and the browser may not end it.
    s.engine.set_frequency(146.52, "2m", "FM");
    s.engine.aprs_tune(144.39).unwrap();
    s.engine.take_immediate_retune();
    s.sample(144_390_000, "FM");
    let connection = link(&s);
    assert!(matches!(
        s.engine.queue_remote_frequency(
            146.52,
            "2m",
            "FM",
            connection,
            s.authority.permit(unexpired_deadline()).unwrap()
        ),
        Err(Reason::StationBusy)
    ));
    assert!(s.engine.take_remote_radio().is_none());

    // Positive control: an APRS context set at the station still refuses remote tuning.
    let mut local = station(OperatingMode::Phone, 145.5, "2m");
    local.engine.aprs_tune(144.39).unwrap();
    local.engine.take_immediate_retune();
    local.sample(144_390_000, "FM");
    let connection = link(&local);
    assert!(matches!(
        local.engine.queue_remote_frequency(
            146.52,
            "2m",
            "FM",
            connection,
            local.authority.permit(unexpired_deadline()).unwrap()
        ),
        Err(Reason::StationBusy)
    ));
    assert!(local.engine.take_remote_radio().is_none());
}
