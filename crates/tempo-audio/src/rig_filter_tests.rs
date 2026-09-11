use super::*;

#[test]
fn remote_filter_tcp_changes_only_the_reported_width_once() {
    for (mode, before, after) in [("CW", 500, 550), ("LSB", 2400, 2300)] {
        let peer = filtering_peer(14_074_000, mode, before, |_, _, _| None);
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
        let expected = Position::new(14_074_000, mode).unwrap();
        let read = rig
            .remote_filter_width(expected.clone(), before, after, &permission)
            .unwrap();
        assert_eq!(read.position(), &expected);
        assert_eq!(read.passband(), Some(after));
        assert_eq!(writes(&peer), vec![format!("M {mode} {after}")]);
        assert!(peer.lines.lock().unwrap().iter().any(|s| s == "t"));
        assert!(peer.lines.lock().unwrap().iter().any(|s| s == "s"));
        assert!(matches!(completion.outcome(), Outcome::Pending));
        // Reasserting an already-read width is a read-only operation.
        peer.lines.lock().unwrap().clear();
        let read = rig
            .remote_filter_width(expected, after, after, &permission)
            .unwrap();
        assert_eq!(read.passband(), Some(after));
        assert!(writes(&peer).is_empty());
    }
}

#[test]
fn remote_filter_tcp_refuses_stale_busy_and_unconfirmed_widths_without_a_retry() {
    for failure in [
        "width",
        "mode",
        "keyed",
        "split",
        "unconfirmed",
        "dial moved",
    ] {
        let peer = filtering_peer(14_074_000, "CW", 500, move |line, state, width| {
            match (failure, line) {
                ("width", "m") => {
                    width.store(600, Ordering::SeqCst);
                    None
                }
                ("mode", "m") => {
                    state.mode = "USB".into();
                    None
                }
                ("keyed", "t") => Some("1\n".into()),
                ("split", "s") => Some("1\nVFOB\n".into()),
                ("unconfirmed", "M CW 550") => Some("RPRT 0\n".into()),
                ("dial moved", "M CW 550") => {
                    state.dial += 100;
                    None
                }
                _ => None,
            }
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
        assert!(
            rig.remote_filter_width(
                Position::new(14_074_000, "CW").unwrap(),
                500,
                550,
                &permission
            )
            .is_err(),
            "{failure}"
        );
        let expected = if matches!(failure, "unconfirmed" | "dial moved") {
            vec!["M CW 550".to_owned()]
        } else {
            vec![]
        };
        assert_eq!(writes(&peer), expected, "{failure}");
        assert!(!matches!(completion.outcome(), Outcome::Applied { .. }));
    }
}

#[test]
fn remote_filter_tcp_checks_native_takeover_after_the_blocking_width_read() {
    let native = Arc::new(Revocation::default());
    let revoke = native.clone();
    let modes = std::sync::atomic::AtomicUsize::new(0);
    let peer = filtering_peer(14_074_000, "CW", 500, move |line, _, _| {
        if line == "m" && modes.fetch_add(1, Ordering::SeqCst) == 1 {
            revoke.revoke();
        }
        None
    });
    let authority = Revocation::default();
    let mut rig = Rig::rigctld(&peer.address);
    let (old, _) = permission(&authority, &native);
    assert!(rig
        .remote_filter_width(Position::new(14_074_000, "CW").unwrap(), 500, 550, &old)
        .is_err());
    assert!(writes(&peer).is_empty());
    let (fresh, _) = permission(&authority, &native);
    assert!(rig
        .remote_filter_width(Position::new(14_074_000, "CW").unwrap(), 500, 550, &fresh)
        .is_ok());
    assert_eq!(writes(&peer), vec!["M CW 550"]);
}
