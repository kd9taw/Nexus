use super::*;
use crate::rig::remote::RepeaterConfig;

fn peer(
    mode: &str,
    intercept: impl Fn(&str, &mut RadioState) -> Option<String> + Send + 'static,
) -> Peer {
    let config = Mutex::new(("None".to_string(), 600_000i64, 0u32));
    retuning_peer(146_520_000, mode, move |line, state| {
        if let Some(reply) = intercept(line, state) {
            return Some(reply);
        }
        let mut c = config.lock().unwrap();
        match line {
            "r" => return Some(format!("{}\n", c.0)),
            "o" => return Some(format!("{}\n", c.1)),
            "c" => return Some(format!("{}\n", c.2)),
            _ => (),
        }
        if let Some(value) = line.strip_prefix("R ") {
            c.0 = value.into();
        } else if let Some(value) = line.strip_prefix("O ") {
            c.1 = value.parse().unwrap();
        } else if let Some(value) = line.strip_prefix("C ") {
            c.2 = value.parse().unwrap();
        } else {
            return None;
        }
        Some("RPRT 0\n".into())
    })
}
fn writes(peer: &Peer) -> Vec<String> {
    peer.lines
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s.starts_with("R ") || s.starts_with("O ") || s.starts_with("C "))
        .cloned()
        .collect()
}
#[test]
fn remote_fm_exact_native_writes_read_back_shift_offset_and_tone_without_keying() {
    for mode in ["FM", "PKTFM"] {
        let peer = peer(mode, |_, _| None);
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, receipt) = permission(&Revocation::default(), &Revocation::default());
        let target = RepeaterConfig::new("plus", 5_000_000, 88.56).unwrap();
        let position = Position::new(146_520_000, mode).unwrap();
        let read = rig
            .remote_fm_repeater(position.clone(), &target, &permission)
            .unwrap();
        assert_eq!(read.position(), &position);
        assert_eq!(read.repeater(), Some(&target));
        assert_eq!(writes(&peer), ["R +", "O 5000000", "C 886"]);
        assert!(matches!(receipt.outcome(), Outcome::Pending));
        assert!(!peer
            .lines
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.starts_with("T ")));
        let before = writes(&peer).len();
        let read = rig
            .remote_fm_repeater(position, &target, &permission)
            .unwrap();
        assert_eq!(read.repeater(), Some(&target));
        assert_eq!(
            writes(&peer).len(),
            before,
            "matching hardware needs no rewrite"
        );
    }
}
#[test]
fn remote_fm_zero_offset_keeps_the_native_leave_offset_unchanged_behavior() {
    let peer = peer("FM", |_, _| None);
    let mut rig = Rig::rigctld(&peer.address);
    let (permission, _) = permission(&Revocation::default(), &Revocation::default());
    let target = RepeaterConfig::new("minus", 0, 100.0).unwrap();
    let read = rig
        .remote_fm_repeater(
            Position::new(146_520_000, "FM").unwrap(),
            &target,
            &permission,
        )
        .unwrap();
    let observed = read.repeater().unwrap();
    assert_eq!(observed.shift(), "minus");
    assert_eq!(observed.offset_hz(), 600_000);
    assert_eq!(observed.tone_hz(), 100.0);
    assert_eq!(writes(&peer), ["R -", "C 1000"]);
}
#[test]
fn remote_fm_missing_or_unconfirmed_state_never_claims_a_completed_configuration() {
    for failure in [
        "missing",
        "invalid shift",
        "negative offset",
        "error with value",
        "bad tone",
        "keyed",
        "split",
        "ack",
        "unconfirmed",
        "keyed after",
        "dial after",
    ] {
        let peer = peer("FM", move |line, state| match (failure, line) {
            ("missing", "o") => Some("RPRT -11\n".into()),
            ("invalid shift", "r") => Some("unknown\n".into()),
            ("negative offset", "o") => Some("-1\n".into()),
            ("error with value", "o") => Some("600000\nRPRT -1\n".into()),
            ("bad tone", "c") => Some("NaN\n".into()),
            ("keyed", "t") => Some("1\n".into()),
            ("split", "s") => Some("1\nVFOB\n".into()),
            ("ack", "R +") => Some("RPRT -1\n".into()),
            ("unconfirmed", "C 885") => Some("RPRT 0\n".into()),
            ("keyed after", "R +") => {
                state.keyed = true;
                None
            }
            ("dial after", "R +") => {
                state.dial += 100;
                None
            }
            _ => None,
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, receipt) = permission(&Revocation::default(), &Revocation::default());
        assert!(
            rig.remote_fm_repeater(
                Position::new(146_520_000, "FM").unwrap(),
                &RepeaterConfig::new("plus", 5_000_000, 88.5).unwrap(),
                &permission
            )
            .is_err(),
            "{failure}"
        );
        let attempted = ["ack", "unconfirmed", "keyed after", "dial after"].contains(&failure);
        assert_eq!(!writes(&peer).is_empty(), attempted, "{failure}");
        if attempted {
            assert!(
                matches!(receipt.outcome(), Outcome::Unknown { .. }),
                "{failure}"
            );
        } else {
            assert!(
                !matches!(receipt.outcome(), Outcome::Applied { .. }),
                "{failure}"
            );
        }
        assert!(writes(&peer).len() <= 3);
    }
}
#[test]
fn remote_fm_revocation_between_fields_stops_remaining_writes_and_allows_a_fresh_intent() {
    let authority = Arc::new(Revocation::default());
    let revoke = authority.clone();
    let peer = peer("FM", move |line, _| {
        if line == "R +" {
            revoke.revoke();
        }
        None
    });
    let mut rig = Rig::rigctld(&peer.address);
    let native = Revocation::default();
    let (old, receipt) = permission(&authority, &native);
    let position = Position::new(146_520_000, "FM").unwrap();
    let target = RepeaterConfig::new("plus", 5_000_000, 88.5).unwrap();
    assert!(rig
        .remote_fm_repeater(position.clone(), &target, &old)
        .is_err());
    assert_eq!(writes(&peer), ["R +"]);
    assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
    let (fresh, _) = permission(&authority, &native);
    assert!(rig.remote_fm_repeater(position, &target, &fresh).is_ok());
    assert_eq!(writes(&peer), ["R +", "O 5000000", "C 885"]);
}
#[test]
fn remote_fm_uses_closed_finite_configuration_and_an_fm_position() {
    for shift in ["plus\nT 1", "", "UP"] {
        assert!(RepeaterConfig::new(shift, 600_000, 88.5).is_err());
    }
    for tone in [f32::NAN, f32::INFINITY, -1.0, f32::MAX] {
        assert!(RepeaterConfig::new("simplex", 600_000, tone).is_err());
    }
    assert!(RepeaterConfig::new("plus", -1, 0.0).is_err());
    let peer = peer("LSB", |_, _| None);
    let mut rig = Rig::rigctld(&peer.address);
    let (permission, _) = permission(&Revocation::default(), &Revocation::default());
    assert!(rig
        .remote_fm_repeater(
            Position::new(146_520_000, "LSB").unwrap(),
            &RepeaterConfig::new("simplex", 0, 0.0).unwrap(),
            &permission
        )
        .is_err());
    assert!(peer.lines.lock().unwrap().is_empty());
}

#[test]
fn remote_fm_checks_permission_at_the_first_socket_write() {
    let peer = peer("FM", |_, _| None);
    let authority = Arc::new(Revocation::default());
    let native = Revocation::default();
    let (old, receipt) = permission(&authority, &native);
    let revoke = authority.clone();
    let mut rig = Rig::rigctld(&peer.address);
    rig.before_remote_write = Some(Box::new(move || revoke.revoke()));
    let position = Position::new(146_520_000, "FM").unwrap();
    let target = RepeaterConfig::new("plus", 5_000_000, 88.5).unwrap();
    assert!(rig
        .remote_fm_repeater(position.clone(), &target, &old)
        .is_err());
    assert!(writes(&peer).is_empty());
    assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
    let (fresh, _) = permission(&authority, &native);
    assert!(rig.remote_fm_repeater(position, &target, &fresh).is_ok());
    assert_eq!(writes(&peer), ["R +", "O 5000000", "C 885"]);
}
