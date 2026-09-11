use super::*;

fn queue(s: &mut Station, expected: ReceiverDsp, value: ReceiverDsp) -> Result<Completion, Reason> {
    let connection = s
        .engine
        .remote_monitor_observation()
        .radio
        .readings
        .cat
        .unwrap()
        .connection_generation;
    let mode = match s.engine.settings.operating_mode {
        OperatingMode::Cw => "cw",
        OperatingMode::Phone => "phone",
        _ => "digital",
    };
    s.engine.queue_remote_receiver_dsp(
        mode,
        expected,
        value,
        connection,
        s.authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap(),
    )
}

fn observe(s: &mut Station, value: ReceiverDsp) {
    match value {
        ReceiverDsp::Function { func, on } => s.engine.rig_funcs[func.index()] = Some(on),
        ReceiverDsp::Agc(speed) => s.engine.observe_rig_agc(speed.name().into()),
    }
    let mode = s.engine.rig_mode_effective();
    s.sample(14_074_000, &mode);
}

fn choices() -> Vec<(ReceiverDsp, ReceiverDsp)> {
    let mut choices: Vec<_> = [
        ReceiverFunction::Nb,
        ReceiverFunction::Nr,
        ReceiverFunction::Notch,
        ReceiverFunction::ManualNotch,
    ]
    .into_iter()
    .flat_map(|func| {
        [false, true].map(|on| {
            (
                ReceiverDsp::Function { func, on },
                ReceiverDsp::Function { func, on: !on },
            )
        })
    })
    .collect();
    choices.extend(
        [
            AgcSpeed::Auto,
            AgcSpeed::Fast,
            AgcSpeed::Mid,
            AgcSpeed::Slow,
            AgcSpeed::Off,
        ]
        .map(|speed| (ReceiverDsp::Agc(AgcSpeed::Fast), ReceiverDsp::Agc(speed))),
    );
    choices
}

#[test]
fn remote_dsp_commits_only_readback_without_settings_tx_or_native_pending_slots() {
    for mode in [OperatingMode::Cw, OperatingMode::Phone] {
        for (before, after) in choices() {
            let mut s = Station::new(mode);
            s.engine.settings.save(&s.path).unwrap();
            let file = std::fs::read(&s.path).unwrap();
            let settings = serde_json::to_value(s.engine.settings()).unwrap();
            let decoder = s.engine.source.clone();
            observe(&mut s, before);
            let receipt = queue(&mut s, before, after).unwrap();
            assert!(matches!(receipt.outcome(), Outcome::Pending));
            assert!(s.engine.pending_func.iter().all(Option::is_none));
            assert!(!s.engine.agc_picked);
            let request = s.engine.take_remote_radio().unwrap();
            assert_eq!(request.expected(), request.target());
            assert_eq!(request.receiver_dsp(), Some((before, after)));
            let mode = s.engine.rig_mode_effective();
            s.sample(14_074_000, &mode);
            assert!(request.commit_receiver_dsp_readback(&mut s.engine, Some(after)));
            assert_eq!(
                receipt.outcome(),
                Outcome::Applied {
                    evidence: Evidence::RadioReadback
                }
            );
            match after {
                ReceiverDsp::Function { func, on } => {
                    assert_eq!(s.engine.rig_funcs[func.index()], Some(on))
                }
                ReceiverDsp::Agc(speed) => {
                    assert_eq!(s.engine.rig_agc.as_deref(), Some(speed.name()));
                    assert_eq!(s.engine.agc.as_deref(), Some(speed.name()));
                    assert!(!s.engine.agc_picked);
                }
            }
            assert!(s.engine.pending_func.iter().all(Option::is_none));
            assert!(!s.engine.take_immediate_retune());
            assert!(!s.engine.tx_enabled());
            assert!(std::sync::Arc::ptr_eq(&decoder, &s.engine.source));
            assert_eq!(serde_json::to_value(s.engine.settings()).unwrap(), settings);
            assert_eq!(std::fs::read(&s.path).unwrap(), file);
        }
    }
}

#[test]
fn remote_dsp_local_away_and_back_revokes_the_original_request_and_allows_a_new_gesture() {
    for agc in [false, true] {
        let mut s = Station::new(OperatingMode::Phone);
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
        observe(&mut s, before);
        let receipt = queue(&mut s, before, after).unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        if agc {
            s.engine.set_agc("slow");
            s.engine.set_agc("fast");
            s.engine.agc_to_command();
        } else {
            s.engine.request_rig_func("nb", true);
            s.engine.request_rig_func("nb", false);
            s.engine.take_func_requests();
        }
        observe(&mut s, before);
        assert!(request.permission().begin_write(Instant::now()).is_err());
        assert!(!request.commit_receiver_dsp_readback(&mut s.engine, Some(after)));
        assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
        let fresh = queue(&mut s, before, after).unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        observe(&mut s, before);
        assert!(request.commit_receiver_dsp_readback(&mut s.engine, Some(after)));
        assert!(matches!(fresh.outcome(), Outcome::Applied { .. }));
    }
}

#[test]
fn remote_dsp_rejects_missing_changed_busy_and_wrong_function_readings() {
    let mut s = Station::new(OperatingMode::Cw);
    let before = ReceiverDsp::Function {
        func: ReceiverFunction::Nb,
        on: false,
    };
    let after = ReceiverDsp::Function {
        func: ReceiverFunction::Nb,
        on: true,
    };
    assert!(matches!(
        queue(&mut s, before, after),
        Err(Reason::ReadingUnavailable)
    ));
    observe(&mut s, after);
    assert!(matches!(
        queue(&mut s, before, after),
        Err(Reason::ContextChanged)
    ));
    observe(&mut s, before);
    assert!(matches!(
        queue(
            &mut s,
            before,
            ReceiverDsp::Function {
                func: ReceiverFunction::Nr,
                on: true
            }
        ),
        Err(Reason::InvalidAction)
    ));
    assert!(matches!(
        queue(&mut s, before, ReceiverDsp::Agc(AgcSpeed::Off)),
        Err(Reason::InvalidAction)
    ));
    s.engine.request_rig_func("nr", true);
    assert!(matches!(
        queue(&mut s, before, after),
        Err(Reason::StationBusy)
    ));
    s.engine.take_func_requests();
    s.engine.set_agc("fast");
    assert!(matches!(
        queue(&mut s, before, after),
        Err(Reason::StationBusy)
    ));
    s.engine.agc_to_command();
    let receipt = queue(&mut s, before, after).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    observe(&mut s, before);
    assert!(!request.commit_receiver_dsp_readback(&mut s.engine, None));
    assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
    assert_eq!(s.engine.rig_funcs[0], Some(false));
    assert!(!s.path.exists());
}

#[test]
fn receiver_dsp_names_cannot_select_keying_or_unrecognized_hamlib_levels() {
    for bad in ["vox", "comp", "tuner", "NB", "nb\nT 1", ""] {
        assert_eq!(ReceiverFunction::from_name(bad), None);
    }
    for bad in ["FAST", "6", "fast\nT 1", ""] {
        assert_eq!(AgcSpeed::from_name(bad), None);
    }
    for raw in 7..=u8::MAX {
        assert_eq!(AgcSpeed::from_hamlib(raw), None);
    }
    assert_eq!(AgcSpeed::from_hamlib(1), Some(AgcSpeed::Fast));
    assert_eq!(AgcSpeed::from_hamlib(4), Some(AgcSpeed::Mid));
}
