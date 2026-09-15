//! Remote memory recall: the desktop's recall (section, exact dial, phone mode, FM repeater
//! plumbing, the memory's own sideband) through the readback transaction, without the manual-mode
//! arming the desktop recall carries, and with an FM machine's input judged against the licence.
use super::*;
use crate::settings::LicenseClass;

type Fm<'a> = Option<(&'a str, i64, f32)>;

fn link(s: &Station) -> u64 {
    s.engine
        .remote_monitor_observation()
        .radio
        .readings
        .cat
        .unwrap()
        .connection_generation
}

fn resample(s: &mut Station) {
    let hz = s.engine.settings.dial_hz();
    let rig = s.engine.rig_mode_effective();
    s.sample(hz, &rig);
}

fn queue(
    s: &mut Station,
    section: &str,
    dial: f64,
    band: &str,
    sideband: Option<&str>,
    fm: Fm,
) -> Result<Completion, Reason> {
    let connection = link(s);
    s.engine.queue_remote_memory_recall(
        section,
        dial,
        band,
        sideband,
        fm,
        connection,
        s.authority.permit(unexpired_deadline()).unwrap(),
    )
}

/// What the desktop's `recallMemory` does at the station: the Settings patch, the atomic Work
/// verb (no call, no tier), then the memory's exact sideband.
fn native_recall(
    e: &mut Engine,
    section: &str,
    dial: f64,
    band: &str,
    sideband: Option<&str>,
    fm: Fm,
) {
    if section == "phone" {
        e.settings.phone_mode = if fm.is_some() { "fm" } else { "ssb" }.into();
    }
    if let Some((shift, offset, tone)) = fm {
        e.settings.rptr_shift = shift.into();
        e.settings.rptr_offset_override_hz = offset;
        e.settings.ctcss_tone_hz = tone;
    }
    e.work_spot(section, dial, band);
    e.note_work_call(None);
    if let Some(sideband) = sideband {
        e.request_sideband_override(Some(sideband));
    }
}

#[test]
fn recall_commit_matches_the_desktop_recall_without_arming() {
    for (section, dial, band, sideband, fm) in [
        ("cw", 7.030, "40m", None, None),
        ("phone", 7.188, "40m", Some("USB"), None),
        ("phone", 14.300, "20m", None, None),
        ("digital", 21.074, "15m", None, None),
        ("phone", 146.94, "2m", None, Some(("minus", 600_000, 100.0))),
        ("phone", 441.3, "70cm", None, Some(("plus", 0, 0.0))),
    ] {
        let name = format!("{section} {dial}");
        let mut s = Station::new(OperatingMode::Digital);
        let mut native = Station::new(OperatingMode::Digital);
        let before = serde_json::to_value(&s.engine.settings).unwrap();
        let receipt = queue(&mut s, section, dial, band, sideband, fm).unwrap();
        // Admission changes nothing: no Settings, file or transmit state.
        assert_eq!(
            serde_json::to_value(&s.engine.settings).unwrap(),
            before,
            "{name}"
        );
        assert!(!s.path.exists());
        assert!(!s.engine.tx_enabled());
        let request = s.engine.take_remote_radio().unwrap();

        native_recall(&mut native.engine, section, dial, band, sideband, fm);
        assert_eq!(
            native.engine.tx_enabled(),
            section != "digital",
            "{name}: the desktop recall keeps its manual-mode arming"
        );
        assert_eq!(
            request.target(),
            (
                native.engine.settings.dial_hz(),
                native.engine.rig_mode_effective().as_str()
            ),
            "{name}"
        );
        let config = native.engine.fm_repeater_config();
        let readback = fm.map(|_| (config.0.as_str(), config.1, config.2));
        assert_eq!(request.repeater(), readback, "{name}");
        request.permission().begin_write(Instant::now()).unwrap();
        let target = request.target();
        let (hz, mode) = (target.0, target.1.to_owned());
        s.sample(hz, &mode);
        let power = request.power_limit();
        let actual = fm.map(|(shift, _, tone)| (shift, config.1, tone));
        assert!(
            request.commit_tuning_readback(&mut s.engine, power, actual),
            "{name}"
        );
        assert!(matches!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        ));
        assert!(
            !s.engine.tx_enabled(),
            "{name}: a browser recall never arms"
        );
        assert!(s.engine.tx_owner().is_none());
        assert!(!s.engine.take_immediate_retune(), "{name}");
        assert_eq!(
            s.engine.sideband_override, native.engine.sideband_override,
            "{name}"
        );
        assert_eq!(s.engine.work_call, native.engine.work_call, "{name}");
        let (remote, local) = (
            serde_json::to_value(&s.engine.settings).unwrap(),
            serde_json::to_value(&native.engine.settings).unwrap(),
        );
        for field in [
            "operatingMode",
            "dialMhz",
            "band",
            "phoneMode",
            "rptrShift",
            "rptrOffsetOverrideHz",
            "ctcssToneHz",
        ] {
            assert_eq!(remote[field], local[field], "{name}: {field}");
        }
        assert!(s.path.exists(), "{name}: a confirmed recall persists");
    }
}

#[test]
fn an_fm_recall_whose_input_is_outside_the_licence_is_refused() {
    for (dial, allowed) in [(51.2, true), (51.0, false)] {
        let mut s = Station::new(OperatingMode::Digital);
        s.engine.settings.license_class = LicenseClass::Technician;
        let result = queue(
            &mut s,
            "phone",
            dial,
            "6m",
            None,
            Some(("minus", 1_000_000, 0.0)),
        );
        if allowed {
            assert!(result.is_ok(), "positive control: {:?}", result.err());
            assert!(s.engine.take_remote_radio().is_some());
        } else {
            assert!(matches!(result, Err(Reason::OutsidePrivileges)));
            assert!(s.engine.take_remote_radio().is_none());
        }
        assert!(!s.engine.tx_enabled());
    }
}

#[test]
fn invalid_busy_and_routed_recalls_are_refused_without_mutation() {
    let mut s = Station::new(OperatingMode::Digital);
    let minus = Some(("minus", 600_000, 0.0));
    for (section, dial, band, sideband, fm) in [
        ("rtty", 14.080, "20m", None, None),
        ("cw", 7.030, "40m", Some("USB"), None),
        ("digital", 14.074, "20m", Some("LSB"), None),
        ("cw", 146.94, "2m", None, minus),
        ("phone", 146.94, "2m", Some("USB"), minus),
        ("phone", 146.94, "2m", None, Some(("up", 600_000, 0.0))),
        ("phone", 146.94, "2m", None, Some(("minus", -1, 0.0))),
        ("phone", 146.94, "2m", None, Some(("minus", 600_000, 88.55))),
        ("phone", 7.188, "20m", None, None),
        ("phone", f64::NAN, "20m", None, None),
        ("phone", 7.188, "40m", Some("usb"), None),
    ] {
        assert!(
            matches!(
                queue(&mut s, section, dial, band, sideband, fm),
                Err(Reason::InvalidAction)
            ),
            "{section} {dial} {band} {sideband:?} {fm:?}"
        );
    }
    s.engine.set_tx_enabled(true);
    assert!(matches!(
        queue(&mut s, "cw", 7.030, "40m", None, None),
        Err(Reason::StationBusy)
    ));
    assert!(s.engine.take_remote_radio().is_none());
    s.engine.set_tx_enabled(false);
    s.engine.take_immediate_retune();
    resample(&mut s);
    // Positive control: the same recall with the latch down is admitted.
    assert!(queue(&mut s, "cw", 7.030, "40m", None, None).is_ok());
    s.engine.take_remote_radio().unwrap();

    let mut routed = Station::new(OperatingMode::Phone);
    let other = routed.engine.add_radio();
    routed.engine.set_radio_bands(other, vec!["40m".into()]);
    assert!(matches!(
        queue(&mut routed, "phone", 7.188, "40m", None, None),
        Err(Reason::UnsupportedAction)
    ));
    assert!(routed.engine.take_remote_radio().is_none());
    assert_eq!(routed.engine.settings.dial_hz(), 14_074_000);
}
