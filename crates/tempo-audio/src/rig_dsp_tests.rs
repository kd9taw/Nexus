use super::*;
use tempo_app::engine::remote_radio::{AgcSpeed, ReceiverDsp, ReceiverFunction};

#[test]
fn remote_dsp_tcp_changes_only_the_selected_receiver_control_once() {
    for mode in ["CW", "LSB"] {
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
                    u8::from(on),
                    ReceiverDsp::Function { func, on },
                    ReceiverDsp::Function { func, on: !on },
                    format!("U {} {}", func.token(), u8::from(!on)),
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
            .map(|speed| {
                (
                    2,
                    ReceiverDsp::Agc(AgcSpeed::Fast),
                    ReceiverDsp::Agc(speed),
                    format!("L AGC {}", speed.hamlib_value()),
                )
            }),
        );
        for (initial, before, after, line) in choices {
            let peer = dsp_peer(mode, initial, |_, _| None);
            let mut rig = Rig::rigctld(&peer.address);
            let (permission, receipt) = permission(&Revocation::default(), &Revocation::default());
            let expected = Position::new(14_074_000, mode).unwrap();
            let read = rig
                .remote_receiver_dsp(expected.clone(), before, after, &permission)
                .unwrap();
            assert_eq!(read.position(), &expected);
            assert_eq!(read.receiver_dsp(), Some(after));
            assert_eq!(dsp_writes(&peer), vec![line]);
            let lines = peer.lines.lock().unwrap();
            assert!(lines.iter().filter(|s| s.as_str() == "t").count() >= 2);
            assert!(lines.iter().filter(|s| s.as_str() == "s").count() >= 2);
            assert_eq!(receipt.outcome(), Outcome::Pending);
        }
    }
}

#[test]
fn remote_dsp_tcp_refuses_stale_busy_and_unconfirmed_controls_without_a_retry() {
    for failure in [
        "prior",
        "mode",
        "keyed",
        "split",
        "unsupported",
        "unconfirmed",
        "dial moved",
        "mode moved",
        "keyed after",
    ] {
        let writes_seen = std::sync::atomic::AtomicBool::new(false);
        let peer = dsp_peer("CW", 0, move |line, state| {
            if line == "U NB 1" {
                writes_seen.store(true, Ordering::SeqCst);
            }
            match (failure, line) {
                ("prior", "u NB") => Some("1\n".into()),
                ("mode", "m") => Some("USB\n2400\n".into()),
                ("keyed", "t") => Some("1\n".into()),
                ("split", "s") => Some("1\nVFOB\n".into()),
                ("unsupported", "u NB") => Some("RPRT -11\n".into()),
                ("unconfirmed", "U NB 1") => Some("RPRT 0\n".into()),
                ("dial moved", "U NB 1") => {
                    state.dial += 100;
                    None
                }
                ("mode moved", "U NB 1") => {
                    state.mode = "USB".into();
                    None
                }
                ("keyed after", "t") if writes_seen.load(Ordering::SeqCst) => Some("1\n".into()),
                _ => None,
            }
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, receipt) = permission(&Revocation::default(), &Revocation::default());
        assert!(
            rig.remote_receiver_dsp(
                Position::new(14_074_000, "CW").unwrap(),
                ReceiverDsp::Function {
                    func: ReceiverFunction::Nb,
                    on: false
                },
                ReceiverDsp::Function {
                    func: ReceiverFunction::Nb,
                    on: true
                },
                &permission
            )
            .is_err(),
            "{failure}"
        );
        let expected =
            if ["unconfirmed", "dial moved", "mode moved", "keyed after"].contains(&failure) {
                vec!["U NB 1".to_owned()]
            } else {
                vec![]
            };
        assert_eq!(dsp_writes(&peer), expected, "{failure}");
        assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
    }
}

#[test]
fn remote_dsp_tcp_rechecks_takeover_after_blocking_reads_and_requires_exact_agc_confirmation() {
    let native = Arc::new(Revocation::default());
    let revoke = native.clone();
    let reads = std::sync::atomic::AtomicUsize::new(0);
    let peer = dsp_peer("LSB", 2, move |line, _| {
        if line == "l AGC" && reads.fetch_add(1, Ordering::SeqCst) == 0 {
            revoke.revoke();
        }
        None
    });
    let authority = Revocation::default();
    let mut rig = Rig::rigctld(&peer.address);
    let (old, _) = permission(&authority, &native);
    let position = Position::new(14_074_000, "LSB").unwrap();
    let before = ReceiverDsp::Agc(AgcSpeed::Fast);
    let after = ReceiverDsp::Agc(AgcSpeed::Slow);
    assert!(rig
        .remote_receiver_dsp(position.clone(), before, after, &old)
        .is_err());
    assert!(dsp_writes(&peer).is_empty());
    let (fresh, _) = permission(&authority, &native);
    assert!(rig
        .remote_receiver_dsp(position, before, after, &fresh)
        .is_ok());
    assert_eq!(dsp_writes(&peer), vec!["L AGC 3"]);
    for raw in ["1", "2.5", "NaN", "7"] {
        let peer = dsp_peer("CW", 2, move |line, _| {
            (line == "l AGC").then(|| format!("{raw}\n"))
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permit, _) = permission(&authority, &native);
        assert!(
            rig.remote_receiver_dsp(
                Position::new(14_074_000, "CW").unwrap(),
                before,
                before,
                &permit
            )
            .is_err(),
            "{raw}"
        );
        assert_eq!(
            dsp_writes(&peer),
            if raw == "1" {
                vec!["L AGC 2".to_owned()]
            } else {
                vec![]
            }
        );
    }
}

/// Every AGC chip, every model this could reach, as the exact `L AGC` line the radio's driver
/// receives. The FTX-1 (1051) gets its own `GT` P2 code (#403); every other model, the FT-710
/// beside it included, keeps the Hamlib enum byte for byte.
const AGC_SPEEDS: [AgcSpeed; 5] = [
    AgcSpeed::Off,
    AgcSpeed::Fast,
    AgcSpeed::Mid,
    AgcSpeed::Slow,
    AgcSpeed::Auto,
];

fn agc_wire(model: u32, speed: AgcSpeed) -> &'static str {
    match (model, speed) {
        // FTX-1 CAT manual, GT Set P2: 0 OFF, 1 FAST, 2 MID, 3 SLOW, 4 AUTO.
        (1051, AgcSpeed::Off) => "L AGC 0",
        (1051, AgcSpeed::Fast) => "L AGC 1",
        (1051, AgcSpeed::Mid) => "L AGC 2",
        (1051, AgcSpeed::Slow) => "L AGC 3",
        (1051, AgcSpeed::Auto) => "L AGC 4",
        // Hamlib's enum: OFF 0, FAST 2, SLOW 3, MEDIUM 5, AUTO 6.
        (_, AgcSpeed::Off) => "L AGC 0",
        (_, AgcSpeed::Fast) => "L AGC 2",
        (_, AgcSpeed::Mid) => "L AGC 5",
        (_, AgcSpeed::Slow) => "L AGC 3",
        (_, AgcSpeed::Auto) => "L AGC 6",
    }
}

const AGC_MODELS: [u32; 6] = [1051, 1049, 1, 2, 3073, 0];

#[test]
fn native_agc_sends_and_reads_the_ftx1_gt_code_and_every_other_model_unchanged() {
    for model in AGC_MODELS {
        for speed in AGC_SPEEDS {
            let peer = dsp_peer("LSB", 0, |_, _| None);
            let mut rig = Rig::rigctld(&peer.address);
            rig.set_hamlib_model(model);
            rig.set_agc(speed.hamlib_value()).unwrap();
            assert_eq!(
                dsp_writes(&peer),
                vec![agc_wire(model, speed)],
                "model {model} {speed:?}"
            );
            // The peer answers `l AGC` with what was written: the radio's own code comes back
            // as the speed that was picked.
            assert_eq!(
                rig.read_agc(),
                Some(speed.hamlib_value()),
                "model {model} {speed:?}"
            );
        }
    }
}

#[test]
fn ftx1_agc_answers_read_as_the_radio_means_them() {
    // FTX-1 GT Answer P3: 0 OFF, 1 FAST, 2 MID, 3 SLOW, 4/5/6 AUTO-FAST/-MID/-SLOW; 7+ undefined.
    let ftx1 = [
        (0, Some(AgcSpeed::Off)),
        (1, Some(AgcSpeed::Fast)),
        (2, Some(AgcSpeed::Mid)),
        (3, Some(AgcSpeed::Slow)),
        (4, Some(AgcSpeed::Auto)),
        (5, Some(AgcSpeed::Auto)),
        (6, Some(AgcSpeed::Auto)),
        (7, None),
    ];
    for (raw, speed) in ftx1 {
        let peer = dsp_peer("LSB", raw, |_, _| None);
        let mut rig = Rig::rigctld(&peer.address);
        rig.set_hamlib_model(1051);
        assert_eq!(
            rig.read_agc(),
            speed.map(AgcSpeed::hamlib_value),
            "GT answer {raw}"
        );
        assert!(dsp_writes(&peer).is_empty());
    }
    // Every other model reads the reply as the Hamlib enum it is, unchanged.
    for model in AGC_MODELS.into_iter().filter(|m| *m != 1051) {
        for raw in 0..=7u8 {
            let peer = dsp_peer("LSB", raw, |_, _| None);
            let mut rig = Rig::rigctld(&peer.address);
            rig.set_hamlib_model(model);
            assert_eq!(rig.read_agc(), Some(raw), "model {model} answer {raw}");
        }
    }
}

#[test]
fn remote_agc_sends_the_ftx1_gt_code_confirms_it_and_every_other_model_unchanged() {
    for model in AGC_MODELS {
        for speed in AGC_SPEEDS {
            // The radio starts on FAST, in its own code.
            let start = if model == 1051 { 1 } else { 2 };
            let peer = dsp_peer("CW", start, |_, _| None);
            let mut rig = Rig::rigctld(&peer.address);
            rig.set_hamlib_model(model);
            let (permission, _) = permission(&Revocation::default(), &Revocation::default());
            let read = rig
                .remote_receiver_dsp(
                    Position::new(14_074_000, "CW").unwrap(),
                    ReceiverDsp::Agc(AgcSpeed::Fast),
                    ReceiverDsp::Agc(speed),
                    &permission,
                )
                .unwrap_or_else(|e| panic!("model {model} {speed:?}: {e:?}"));
            assert_eq!(read.receiver_dsp(), Some(ReceiverDsp::Agc(speed)));
            assert_eq!(
                dsp_writes(&peer),
                vec![agc_wire(model, speed)],
                "model {model} {speed:?}"
            );
        }
    }
    // An FTX-1 on AUTO answers which AUTO it chose (AUTO-SLOW here): that confirms AUTO.
    let wrote = std::sync::atomic::AtomicBool::new(false);
    let peer = dsp_peer("CW", 1, move |line, _| {
        if line == "L AGC 4" {
            wrote.store(true, Ordering::SeqCst);
            return Some("RPRT 0\n".into());
        }
        (line == "l AGC" && wrote.load(Ordering::SeqCst)).then(|| "6\n".into())
    });
    let mut rig = Rig::rigctld(&peer.address);
    rig.set_hamlib_model(1051);
    let (permission, _) = permission(&Revocation::default(), &Revocation::default());
    let read = rig
        .remote_receiver_dsp(
            Position::new(14_074_000, "CW").unwrap(),
            ReceiverDsp::Agc(AgcSpeed::Fast),
            ReceiverDsp::Agc(AgcSpeed::Auto),
            &permission,
        )
        .unwrap();
    assert_eq!(read.receiver_dsp(), Some(ReceiverDsp::Agc(AgcSpeed::Auto)));
    assert_eq!(dsp_writes(&peer), vec!["L AGC 4"]);
}
