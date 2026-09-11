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
