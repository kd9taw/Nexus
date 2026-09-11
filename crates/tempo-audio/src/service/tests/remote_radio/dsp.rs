use super::*;
use crate::rig::remote_tests::{dsp_peer, dsp_writes};
use tempo_app::engine::remote_radio::{AgcSpeed, ReceiverDsp, ReceiverFunction};

#[test]
fn remote_dsp_uses_the_actual_owner_and_does_not_reconcile_back_to_old_native_choices() {
    for agc in [false, true] {
        let peer = dsp_peer("LSB", if agc { 2 } else { 0 }, |_, _| None);
        let mut s = Station::configured(&peer, |settings| {
            settings.operating_mode = tempo_app::settings::OperatingMode::Phone;
            settings.ensure_radio_profiles();
        });
        let canonical_mode = s.state.last_mode.clone();
        assert_ne!(canonical_mode, "LSB");
        let read = s.state.remote_read(&s.engine).unwrap();
        let (before, after) = if agc {
            (
                ReceiverDsp::Agc(AgcSpeed::Fast),
                ReceiverDsp::Agc(AgcSpeed::Slow),
            )
        } else {
            (
                ReceiverDsp::Function {
                    func: ReceiverFunction::Nb,
                    on: false,
                },
                ReceiverDsp::Function {
                    func: ReceiverFunction::Nb,
                    on: true,
                },
            )
        };
        let receipt = {
            let mut e = engine_lock(&s.engine);
            e.settings().save(&s.path).unwrap();
            e.set_agc("fast");
            e.agc_to_command();
            s.state.last_agc = Some("fast".into());
            e.observe_rig_agc("fast".into());
            e.observe_rig_funcs([Some(false); 6]);
            e.remote_observe_mode(Some(&read), Some("LSB"));
            let connection = e
                .remote_monitor_observation()
                .radio
                .readings
                .cat
                .unwrap()
                .connection_generation;
            e.queue_remote_receiver_dsp(
                "phone",
                before,
                after,
                connection,
                s.authority
                    .permit(Instant::now() + Duration::from_secs(5))
                    .unwrap(),
            )
            .unwrap()
        };
        let file = std::fs::read(&s.path).unwrap();
        s.step();
        assert_eq!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        );
        let expected = if agc { "L AGC 3" } else { "U NB 1" };
        assert_eq!(dsp_writes(&peer), vec![expected]);
        assert_eq!(s.state.last_mode, canonical_mode);
        if agc {
            assert_eq!(s.state.last_agc.as_deref(), Some("slow"));
        } else {
            assert_eq!(s.state.func_state[0], Some(true));
        }
        for _ in 0..3 {
            s.step();
        }
        assert_eq!(dsp_writes(&peer), vec![expected]);
        assert_eq!(std::fs::read(&s.path).unwrap(), file);
        let mut e = engine_lock(&s.engine);
        assert!(e.take_func_requests().iter().all(Option::is_none));
        assert!(!e.agc_to_command().is_some_and(|(_, picked)| picked));
        assert!(!e.tx_enabled());
        if agc {
            assert_eq!(e.snapshot().radio.agc.as_deref(), Some("slow"));
        } else {
            assert_eq!(e.snapshot().radio.nb, Some(true));
        }
    }
}

#[test]
fn remote_dsp_unknown_write_never_enters_the_native_retry_path() {
    for agc in [false, true] {
        let line = if agc { "L AGC 3" } else { "U NB 1" };
        let peer = dsp_peer("CW", if agc { 2 } else { 0 }, move |command, _| {
            // The transport acknowledges, but the radio does not adopt it.
            (command == line).then(|| "RPRT 0\n".into())
        });
        let mut s = Station::configured(&peer, |settings| {
            settings.operating_mode = tempo_app::settings::OperatingMode::Cw;
            settings.ensure_radio_profiles();
        });
        let (before, after) = if agc {
            (
                ReceiverDsp::Agc(AgcSpeed::Fast),
                ReceiverDsp::Agc(AgcSpeed::Slow),
            )
        } else {
            (
                ReceiverDsp::Function {
                    func: ReceiverFunction::Nb,
                    on: false,
                },
                ReceiverDsp::Function {
                    func: ReceiverFunction::Nb,
                    on: true,
                },
            )
        };
        let receipt = {
            let mut e = engine_lock(&s.engine);
            e.settings().save(&s.path).unwrap();
            e.set_agc("fast");
            e.agc_to_command();
            s.state.last_agc = Some("fast".into());
            e.observe_rig_agc("fast".into());
            e.observe_rig_funcs([Some(false); 6]);
            let connection = e
                .remote_monitor_observation()
                .radio
                .readings
                .cat
                .unwrap()
                .connection_generation;
            e.queue_remote_receiver_dsp(
                "cw",
                before,
                after,
                connection,
                s.authority
                    .permit(Instant::now() + Duration::from_secs(5))
                    .unwrap(),
            )
            .unwrap()
        };
        let file = std::fs::read(&s.path).unwrap();
        s.step();
        assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
        assert_eq!(dsp_writes(&peer), vec![line]);
        for _ in 0..3 {
            s.step();
        }
        assert_eq!(dsp_writes(&peer), vec![line]);
        let mut e = engine_lock(&s.engine);
        assert!(e.take_remote_radio().is_none());
        assert!(e.take_func_requests().iter().all(Option::is_none));
        assert_eq!(e.snapshot().radio.agc.as_deref(), Some("fast"));
        assert_eq!(e.snapshot().radio.nb, Some(false));
        assert!(!e.tx_enabled());
        assert_eq!(std::fs::read(&s.path).unwrap(), file);
    }
}

#[test]
fn receiver_agc_mapping_preserves_native_superfast_user_and_unknown_display_policy() {
    for (raw, label) in [
        (0, "off"),
        (1, "fast"),
        (2, "fast"),
        (3, "slow"),
        (4, "mid"),
        (5, "mid"),
        (6, "auto"),
        (7, "mid"),
        (255, "mid"),
    ] {
        assert_eq!(agc_from_hamlib(raw), label);
    }
    for (label, raw) in [
        ("off", 0),
        ("fast", 2),
        ("slow", 3),
        ("mid", 5),
        ("auto", 6),
        ("unknown", 5),
    ] {
        assert_eq!(agc_to_hamlib(label), raw);
    }
}
