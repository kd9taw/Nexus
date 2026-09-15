//! Remote rig-scope settings: the station rules beneath the browser grammar.
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

fn attempt(s: &mut Station, setting: RemoteScope, family: ScopeFamily) -> Result<(), Reason> {
    let connection = link(s);
    let permit = s.authority.permit(unexpired_deadline()).unwrap();
    s.engine
        .queue_remote_scope(setting, family, connection, &permit)
}

/// What the one-shots queued, drained the way the radio loop drains them.
fn drained(s: &mut Station) -> (Option<u32>, Option<i32>, Option<u8>) {
    (
        s.engine.take_scope_span_request(),
        s.engine.take_scope_ref_request(),
        s.engine.take_yaesu_scope_mode_request(),
    )
}

/// The FT-710 family is the scope MODE code the radio loop last read back ('4' = W/F CENTER).
fn ft710(s: &mut Station) {
    s.engine.set_scope_mode_code(Some(u32::from(b'4')));
}

#[test]
fn each_setting_reaches_its_local_verb_without_arming_retuning_or_saving() {
    for (name, setting, family, yaesu) in [
        (
            "icom span",
            RemoteScope::Span(25_000),
            ScopeFamily::IcomCiv,
            false,
        ),
        (
            "icom ref",
            RemoteScope::Ref(-35),
            ScopeFamily::IcomCiv,
            false,
        ),
        (
            "ft-710 span",
            RemoteScope::Span(500_000),
            ScopeFamily::None,
            true,
        ),
        (
            "ft-710 position",
            RemoteScope::Position(b'A'),
            ScopeFamily::None,
            true,
        ),
        (
            "flex span",
            RemoteScope::PanSpan(200_000),
            ScopeFamily::Flex,
            false,
        ),
        (
            "flex ref",
            RemoteScope::PanRef(Some(-80)),
            ScopeFamily::Flex,
            false,
        ),
        (
            "flex auto ref",
            RemoteScope::PanRef(None),
            ScopeFamily::Flex,
            false,
        ),
    ] {
        let mut s = Station::new(OperatingMode::Phone);
        if yaesu {
            ft710(&mut s);
        }
        if setting == RemoteScope::PanRef(None) {
            // Auto must be a change from a stated level, or the assertion below proves nothing.
            s.engine.set_flex_pan_ref(Some(-100));
        }
        let generation = s.engine.tx_gate_gen;
        let dial = s.engine.settings.dial_hz();
        attempt(&mut s, setting, family).unwrap_or_else(|r| panic!("{name}: {r:?}"));
        let (span, reference, position) = drained(&mut s);
        match setting {
            RemoteScope::Span(hz) => assert_eq!(span, Some(hz), "{name}"),
            RemoteScope::Ref(tenths) => assert_eq!(reference, Some(tenths), "{name}"),
            RemoteScope::Position(code) => assert_eq!(position, Some(code), "{name}"),
            RemoteScope::PanSpan(hz) => {
                assert_eq!(s.engine.flex_pan_span_hz(), f64::from(hz), "{name}")
            }
            RemoteScope::PanRef(dbm) => assert_eq!(s.engine.flex_pan_ref_dbm(), dbm, "{name}"),
        }
        assert!(!s.engine.tx_enabled(), "{name} leaves the latch down");
        assert!(s.engine.tx_owner().is_none(), "{name}");
        assert_eq!(
            s.engine.tx_gate_gen, generation,
            "{name} leaves the TX gate alone"
        );
        assert_eq!(s.engine.settings.dial_hz(), dial, "{name} does not retune");
        assert!(s.engine.take_remote_radio().is_none(), "{name}");
        assert!(!s.path.exists(), "{name} saves nothing");
    }
}

#[test]
fn a_setting_no_live_scope_takes_is_refused_before_any_verb() {
    for (name, setting, family, yaesu, expected) in [
        (
            "reference on a Flex",
            RemoteScope::Ref(0),
            ScopeFamily::Flex,
            false,
            Reason::HardwareUnavailable,
        ),
        (
            "pan span on an Icom",
            RemoteScope::PanSpan(200_000),
            ScopeFamily::IcomCiv,
            false,
            Reason::HardwareUnavailable,
        ),
        (
            "pan reference on an FT-710",
            RemoteScope::PanRef(None),
            ScopeFamily::None,
            true,
            Reason::HardwareUnavailable,
        ),
        (
            "position on an Icom",
            RemoteScope::Position(b'A'),
            ScopeFamily::IcomCiv,
            false,
            Reason::HardwareUnavailable,
        ),
        (
            "span with no native scope",
            RemoteScope::Span(25_000),
            ScopeFamily::None,
            false,
            Reason::HardwareUnavailable,
        ),
        (
            "an FT-710 rung on an Icom",
            RemoteScope::Span(500),
            ScopeFamily::IcomCiv,
            false,
            Reason::InvalidAction,
        ),
        (
            "an unknown mode code",
            RemoteScope::Position(b'Z'),
            ScopeFamily::None,
            true,
            Reason::InvalidAction,
        ),
        (
            "a reference past +20 dB",
            RemoteScope::Ref(201),
            ScopeFamily::IcomCiv,
            false,
            Reason::InvalidAction,
        ),
        (
            "a pan below 5 kHz",
            RemoteScope::PanSpan(4_999),
            ScopeFamily::Flex,
            false,
            Reason::InvalidAction,
        ),
        (
            "a pan reference above +20 dBm",
            RemoteScope::PanRef(Some(21)),
            ScopeFamily::Flex,
            false,
            Reason::InvalidAction,
        ),
    ] {
        let mut s = Station::new(OperatingMode::Phone);
        if yaesu {
            ft710(&mut s);
        }
        let pan = (s.engine.flex_pan_span_hz(), s.engine.flex_pan_ref_dbm());
        assert_eq!(attempt(&mut s, setting, family), Err(expected), "{name}");
        assert_eq!(drained(&mut s), (None, None, None), "{name}");
        assert_eq!(
            (s.engine.flex_pan_span_hz(), s.engine.flex_pan_ref_dbm()),
            pan,
            "{name}"
        );
    }
    // Positive controls: the same settings on the family that runs them are taken.
    let mut s = Station::new(OperatingMode::Phone);
    ft710(&mut s);
    assert_eq!(
        attempt(&mut s, RemoteScope::Span(500), ScopeFamily::None),
        Ok(())
    );
    assert_eq!(
        attempt(&mut s, RemoteScope::Position(b'A'), ScopeFamily::None),
        Ok(())
    );
    let mut s = Station::new(OperatingMode::Phone);
    assert_eq!(
        attempt(&mut s, RemoteScope::Ref(0), ScopeFamily::IcomCiv),
        Ok(())
    );
    assert_eq!(
        attempt(&mut s, RemoteScope::PanSpan(200_000), ScopeFamily::Flex),
        Ok(())
    );
}

#[test]
fn an_owned_transmitter_a_tune_carrier_or_lapsed_authority_refuses_it() {
    for context in ["manual PTT", "tune carrier", "lapsed authority", "idle"] {
        let mut s = Station::new(OperatingMode::Phone);
        let connection = link(&s);
        let permit = s.authority.permit(unexpired_deadline()).unwrap();
        match context {
            "manual PTT" => s.engine.manual_ptt = true,
            "tune carrier" => s.engine.tuning = true,
            "lapsed authority" => s.authority.revoke(),
            _ => {}
        }
        let generation = s.engine.tx_gate_gen;
        let result = s.engine.queue_remote_scope(
            RemoteScope::Ref(-35),
            ScopeFamily::IcomCiv,
            connection,
            &permit,
        );
        let expected = match context {
            "manual PTT" | "tune carrier" => Err(Reason::StationBusy),
            "lapsed authority" => Err(Reason::AuthorityExpired),
            _ => Ok(()),
        };
        assert_eq!(result, expected, "{context}");
        assert_eq!(
            s.engine.take_scope_ref_request(),
            (context == "idle").then_some(-35),
            "{context}"
        );
        assert_eq!(s.engine.tx_gate_gen, generation, "{context}");
        assert!(!s.engine.tx_enabled(), "{context}");
    }
}
