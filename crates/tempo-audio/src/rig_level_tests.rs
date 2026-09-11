use super::*;
use tempo_app::engine::remote_radio::RadioLevel;

fn choices() -> [(RadioLevel, f32, f32); 5] {
    [
        (RadioLevel::Power, 0.5, 0.35),
        (RadioLevel::MicGain, 0.5, 0.35),
        (RadioLevel::NoiseReduction, 0.5, 0.35),
        (RadioLevel::Compression, 0.5, 0.35),
        (RadioLevel::NotchFrequency, 600.0, 1500.0),
    ]
}
fn changes(peer: &Peer) -> Vec<String> {
    peer.lines
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s.starts_with("L "))
        .cloned()
        .collect()
}
#[test]
fn remote_levels_tcp_write_the_exact_native_token_once_and_return_real_readback() {
    for (level, before, target) in choices() {
        let peer = level_peer(level, before, |_, _, _| None);
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, receipt) = permission(&Revocation::default(), &Revocation::default());
        let position = Position::new(14_074_000, "LSB").unwrap();
        let read = rig
            .remote_level(position.clone(), level, before, target, &permission)
            .unwrap();
        assert_eq!(read.position(), &position);
        assert_eq!(read.level(), Some(target));
        assert_eq!(
            changes(&peer),
            vec![format!(
                "L {} {}",
                level.token(),
                if level == RadioLevel::NotchFrequency {
                    "1500"
                } else {
                    "0.350"
                }
            )]
        );
        assert!(matches!(receipt.outcome(), Outcome::Pending));
        assert!(
            peer.lines
                .lock()
                .unwrap()
                .iter()
                .filter(|s| s.as_str() == "t")
                .count()
                >= 2
        );
    }
}
#[test]
fn remote_levels_tcp_refuse_bad_prior_state_and_unconfirmed_writes() {
    for failure in [
        "prior",
        "negative",
        "nan",
        "unsupported",
        "keyed",
        "split",
        "unconfirmed",
        "dial moved",
        "keyed after",
    ] {
        let peer = level_peer(RadioLevel::MicGain, 0.5, move |line, state, _| {
            match (failure, line) {
                ("prior", "l MICGAIN") => Some("0.8\n".into()),
                ("negative", "l MICGAIN") => Some("-1\n".into()),
                ("nan", "l MICGAIN") => Some("NaN\n".into()),
                ("unsupported", "l MICGAIN") => Some("RPRT -11\n".into()),
                ("keyed", "t") => Some("1\n".into()),
                ("split", "s") => Some("1\nVFOB\n".into()),
                ("unconfirmed", "L MICGAIN 0.350") => Some("RPRT 0\n".into()),
                ("dial moved", "L MICGAIN 0.350") => {
                    state.dial += 100;
                    None
                }
                ("keyed after", "L MICGAIN 0.350") => {
                    state.keyed = true;
                    None
                }
                _ => None,
            }
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, receipt) = permission(&Revocation::default(), &Revocation::default());
        assert!(
            rig.remote_level(
                Position::new(14_074_000, "LSB").unwrap(),
                RadioLevel::MicGain,
                0.5,
                0.35,
                &permission
            )
            .is_err(),
            "{failure}"
        );
        assert_eq!(
            changes(&peer).len(),
            usize::from(["unconfirmed", "dial moved", "keyed after"].contains(&failure)),
            "{failure}"
        );
        assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
    }
}
#[test]
fn remote_levels_tcp_recheck_the_original_permission_at_the_socket_and_allow_new_gestures() {
    for (level, before, target) in choices() {
        let peer = level_peer(level, before, |_, _, _| None);
        let authority = Arc::new(Revocation::default());
        let native = Revocation::default();
        let (old, receipt) = permission(&authority, &native);
        let mut rig = Rig::rigctld(&peer.address);
        let revoke = authority.clone();
        rig.before_remote_write = Some(Box::new(move || revoke.revoke()));
        let position = Position::new(14_074_000, "LSB").unwrap();
        assert!(rig
            .remote_level(position.clone(), level, before, target, &old)
            .is_err());
        assert!(changes(&peer).is_empty());
        assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
        let (fresh, _) = permission(&authority, &native);
        assert!(rig
            .remote_level(position, level, before, target, &fresh)
            .is_ok());
        assert_eq!(changes(&peer).len(), 1);
    }
}

#[test]
fn remote_notch_tcp_rejects_out_of_range_rounded_readback_without_replay() {
    for (target, actual) in [(300.0, 299.6), (3400.0, 3400.4)] {
        let peer = level_peer(RadioLevel::NotchFrequency, 600.0, move |line, _, value| {
            if line.starts_with("L NOTCHF ") {
                *value.lock().unwrap() = actual;
                Some("RPRT 0\n".into())
            } else {
                None
            }
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, receipt) = permission(&Revocation::default(), &Revocation::default());
        assert!(rig
            .remote_level(
                Position::new(14_074_000, "LSB").unwrap(),
                RadioLevel::NotchFrequency,
                600.0,
                target,
                &permission
            )
            .is_err());
        assert_eq!(changes(&peer).len(), 1);
        assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
    }
}
