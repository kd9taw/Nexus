//! Remote repeater tune: the desktop's one-step `repeater_tune` through the readback transaction,
//! with the repeater INPUT judged against the licence before anything is queued, and nothing
//! armed or keyed.
use super::*;
use crate::settings::LicenseClass;

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

fn queue(
    s: &mut Station,
    output: f64,
    shift: &str,
    offset: i64,
    tone: f32,
) -> Result<Completion, Reason> {
    let connection = link(s);
    s.engine.queue_remote_repeater(
        output,
        shift,
        offset,
        tone,
        connection,
        s.authority.permit(unexpired_deadline()).unwrap(),
    )
}

#[test]
fn repeater_commit_matches_native_repeater_tune_and_never_arms() {
    for (section, dial, band) in [
        (OperatingMode::Phone, 145.5, "2m"),
        (OperatingMode::Digital, 14.074, "20m"),
    ] {
        let mut s = station(section, dial, band);
        let mut native = station(section, dial, band);
        let before = serde_json::to_value(&s.engine.settings).unwrap();
        let receipt = queue(&mut s, 146.94, "minus", 600_000, 100.0).unwrap();
        // Admission changes nothing: no Settings, no hold, no file, no transmit state.
        assert_eq!(serde_json::to_value(&s.engine.settings).unwrap(), before);
        assert!(!s.engine.fm_channel);
        assert!(!s.path.exists());
        assert!(!s.engine.tx_enabled());
        let request = s.engine.take_remote_radio().unwrap();

        native
            .engine
            .repeater_tune(146.94, "minus", 600_000, 100.0)
            .unwrap();
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
        s.sample(146_940_000, "FM");
        assert!(request.commit_tuning_readback(
            &mut s.engine,
            power,
            Some(("minus", 600_000, 100.0))
        ));
        assert!(matches!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        ));
        assert!(!s.engine.tx_enabled(), "a repeater tune never arms");
        assert!(s.engine.tx_owner().is_none());
        assert!(
            !s.engine.take_immediate_retune(),
            "readback consumes the sole CAT QSY"
        );
        assert_eq!(
            s.engine.tx_gate_gen, native.engine.tx_gate_gen,
            "the QSY invalidates planned overs exactly as the desktop tune does"
        );
        assert_eq!(s.engine.fm_channel, native.engine.fm_channel);
        assert_eq!(s.engine.settings.operating_mode, section);
        for field in [
            "phoneMode",
            "rptrShift",
            "rptrOffsetOverrideHz",
            "ctcssToneHz",
            "dialMhz",
            "band",
        ] {
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
fn repeater_readback_must_confirm_the_requested_machine() {
    for actual in [
        None,
        Some(("plus", 600_000, 100.0)),
        Some(("minus", 600_000, 88.5)),
        Some(("minus", 5_000_000, 100.0)),
    ] {
        let mut s = station(OperatingMode::Phone, 145.5, "2m");
        let receipt = queue(&mut s, 146.94, "minus", 600_000, 100.0).unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        request.permission().begin_write(Instant::now()).unwrap();
        s.sample(146_940_000, "FM");
        let power = request.power_limit();
        assert!(!request.commit_tuning_readback(&mut s.engine, power, actual));
        assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
        assert!(!s.engine.fm_channel);
        assert_eq!(s.engine.settings.dial_hz(), 145_500_000);
        assert!(!s.path.exists());
        assert!(!s.engine.tx_enabled());
    }
}

#[test]
fn the_repeater_input_is_judged_against_the_licence_before_queueing() {
    // Technician on 6 m: phone is legal from 50.1 MHz, so a machine whose INPUT lands in the
    // CW-only 50.0-50.1 slice is refused though its output is legal.
    for (output, shift, offset, allowed) in [
        (51.2, "minus", 1_000_000, true),
        (51.0, "minus", 1_000_000, false),
        // 0 = the band convention (1 MHz on 6 m), judged exactly as the rig would key it.
        (51.05, "minus", 0, false),
        (51.05, "simplex", 0, true),
        (147.3, "plus", 600_000, true),
    ] {
        let mut s = station(OperatingMode::Phone, 145.5, "2m");
        s.engine.settings.license_class = LicenseClass::Technician;
        let result = queue(&mut s, output, shift, offset, 0.0);
        if allowed {
            assert!(
                result.is_ok(),
                "{output} {shift} {offset}: {:?}",
                result.err()
            );
            assert!(s.engine.take_remote_radio().is_some());
        } else {
            assert!(
                matches!(result, Err(Reason::OutsidePrivileges)),
                "{output} {shift} {offset}"
            );
            assert!(s.engine.take_remote_radio().is_none());
        }
        assert!(!s.engine.tx_enabled());
    }
}

#[test]
fn invalid_busy_and_routed_repeater_tunes_are_refused_without_mutation() {
    let mut s = station(OperatingMode::Phone, 145.5, "2m");
    for (output, shift, offset, tone) in [
        (146.94, "up", 600_000, 0.0),
        (146.94, "minus", -1, 0.0),
        (146.94, "minus", 20_000_001, 0.0),
        (146.94, "minus", 600_000, 59.9),
        (146.94, "minus", 600_000, f32::NAN),
        (f64::NAN, "minus", 600_000, 0.0),
        (60.0, "minus", 600_000, 0.0),
    ] {
        assert!(
            matches!(
                queue(&mut s, output, shift, offset, tone),
                Err(Reason::InvalidAction)
            ),
            "{output} {shift} {offset} {tone}"
        );
    }
    s.engine.set_tx_enabled(true);
    assert!(matches!(
        queue(&mut s, 146.94, "minus", 600_000, 0.0),
        Err(Reason::StationBusy)
    ));
    assert!(s.engine.take_remote_radio().is_none());
    s.engine.set_tx_enabled(false);
    s.engine.take_immediate_retune();
    let hz = s.engine.settings.dial_hz();
    let rig = s.engine.rig_mode_effective();
    s.sample(hz, &rig);
    // Positive control: the same request with the latch down is admitted.
    assert!(queue(&mut s, 146.94, "minus", 600_000, 0.0).is_ok());
    s.engine.take_remote_radio().unwrap();

    // A radio that provably cannot receive the machine is refused before any write.
    let mut hf = station(OperatingMode::Phone, 145.5, "2m");
    hf.engine.rig_rx_ranges = Some(vec![(1_800_000, 54_000_000)]);
    assert!(matches!(
        queue(&mut hf, 146.94, "minus", 600_000, 0.0),
        Err(Reason::HardwareUnavailable)
    ));
    assert!(queue(&mut hf, 51.2, "minus", 1_000_000, 0.0).is_ok());
    hf.engine.take_remote_radio().unwrap();

    let mut routed = station(OperatingMode::Phone, 145.5, "2m");
    let other = routed.engine.add_radio();
    routed.engine.set_radio_bands(other, vec!["70cm".into()]);
    assert!(matches!(
        queue(&mut routed, 442.1, "plus", 5_000_000, 0.0),
        Err(Reason::UnsupportedAction)
    ));
    assert!(routed.engine.take_remote_radio().is_none());
    assert_eq!(routed.engine.settings.dial_hz(), 145_500_000);
}

#[test]
fn a_hold_the_browser_set_does_not_strand_it_but_a_local_hold_still_refuses() {
    let mut s = station(OperatingMode::Phone, 145.5, "2m");
    queue(&mut s, 146.94, "minus", 600_000, 100.0).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    request.permission().begin_write(Instant::now()).unwrap();
    s.sample(146_940_000, "FM");
    let power = request.power_limit();
    assert!(request.commit_tuning_readback(&mut s.engine, power, Some(("minus", 600_000, 100.0))));
    assert!(s.engine.fm_channel);
    s.sample(146_940_000, "FM");
    let connection = link(&s);
    // A retune past the channel this browser set is admitted: the desktop's own QSY ends it.
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
    // Tier selection is not a retune this rule admits.
    s.engine.settings.operating_mode = OperatingMode::Digital;
    assert!(matches!(
        s.engine.queue_remote_tier(
            crate::dto::Tier::Ft4,
            connection,
            s.authority.permit(unexpired_deadline()).unwrap()
        ),
        Err(Reason::StationBusy)
    ));
    s.engine.settings.operating_mode = OperatingMode::Phone;

    // Positive control: a hold set at the station (a different machine) still refuses.
    let mut local = station(OperatingMode::Phone, 145.5, "2m");
    local
        .engine
        .repeater_tune(147.3, "plus", 600_000, 0.0)
        .unwrap();
    local.engine.take_immediate_retune();
    local.sample(147_300_000, "FM");
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
