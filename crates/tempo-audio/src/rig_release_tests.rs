//! Selection release uses the native PTT method and final socket permission.
use super::*;

#[test]
fn idle_release_uses_real_ptt_method_then_reads_back_without_retuning() {
    for method in [PttMode::Cat, PttMode::Vox] {
        let peer = retuning_peer(7_142_000, "LSB", |_, _| None);
        let mut rig = Rig::with_control(Some(peer.address.clone()), method.clone());
        let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
        let expected = Position::new(7_142_000, "LSB").unwrap();
        let after = rig
            .remote_unkey_idle(expected.clone(), &permission)
            .unwrap();
        assert_eq!(after.position(), &expected);
        assert!(!rig.keyed);
        assert_eq!(completion.outcome(), Outcome::Pending);
        let lines = peer.lines.lock().unwrap();
        let expected_lines = if method == PttMode::Cat {
            vec!["f", "m", "t", "s", "T 0", "f", "m", "t", "s"]
        } else {
            vec!["f", "m", "t", "s", "f", "m", "t", "s"]
        };
        assert_eq!(*lines, expected_lines);
    }
}

#[test]
fn idle_release_never_stops_a_radio_reported_as_keyed_or_at_another_position() {
    for keyed in [false, true] {
        let peer = retuning_peer(7_142_000, "LSB", move |_, state| {
            state.keyed = keyed;
            None
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
        let expected = Position::new(if keyed { 7_142_000 } else { 14_225_000 }, "LSB").unwrap();
        let reason = if keyed {
            Reason::StationBusy
        } else {
            Reason::ContextChanged
        };
        assert_eq!(
            rig.remote_unkey_idle(expected, &permission).unwrap_err(),
            reason
        );
        assert!(peer
            .lines
            .lock()
            .unwrap()
            .iter()
            .all(|s| ["f", "m", "t", "s"].contains(&s.as_str())));
        assert_eq!(completion.outcome(), Outcome::Rejected { reason });
    }
}

#[test]
fn idle_release_checks_original_authority_at_the_actual_unkey_write() {
    for revoke_native in [false, true] {
        let authority = Arc::new(Revocation::default());
        let native = Arc::new(Revocation::default());
        let revoked = if revoke_native {
            native.clone()
        } else {
            authority.clone()
        };
        let peer = retuning_peer(7_142_000, "LSB", |_, _| None);
        let mut rig = Rig::rigctld(&peer.address);
        rig.before_remote_write = Some(Box::new(move || revoked.revoke()));
        let (permission, completion) = permission(&authority, &native);
        assert!(rig
            .remote_unkey_idle(Position::new(7_142_000, "LSB").unwrap(), &permission)
            .is_err());
        assert_eq!(*peer.lines.lock().unwrap(), ["f", "m", "t", "s"]);
        assert_eq!(
            completion.outcome(),
            Outcome::Rejected {
                reason: if revoke_native {
                    Reason::ContextChanged
                } else {
                    Reason::AuthorityExpired
                }
            }
        );
    }
}

#[test]
fn idle_release_never_retries_a_failed_or_unconfirmed_unkey() {
    for refused in [false, true] {
        let peer = retuning_peer(7_142_000, "LSB", move |line, state| {
            if line == "T 0" {
                state.keyed = true;
                Some(if refused { "RPRT -1\n" } else { "RPRT 0\n" }.into())
            } else {
                None
            }
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
        let expected = Position::new(7_142_000, "LSB").unwrap();
        assert!(rig
            .remote_unkey_idle(expected.clone(), &permission)
            .is_err());
        assert_eq!(
            completion.outcome(),
            Outcome::Unknown {
                reason: Reason::HardwareUnconfirmed
            }
        );
        let first = peer.lines.lock().unwrap().clone();
        assert_eq!(first.iter().filter(|s| *s == "T 0").count(), 1);
        assert!(rig.remote_unkey_idle(expected, &permission).is_err());
        assert_eq!(*peer.lines.lock().unwrap(), first);
    }
}

#[cfg(not(feature = "serial"))]
#[test]
fn idle_release_cannot_claim_serial_control_without_the_backend() {
    let peer = retuning_peer(7_142_000, "LSB", |_, _| None);
    let mut rig = Rig::with_control(
        Some(peer.address.clone()),
        PttMode::Serial {
            port: "test-port".into(),
            line: SerialLine::Rts,
        },
    );
    let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
    assert_eq!(
        rig.remote_unkey_idle(Position::new(7_142_000, "LSB").unwrap(), &permission)
            .unwrap_err(),
        Reason::UnsupportedAction
    );
    assert_eq!(
        completion.outcome(),
        Outcome::Rejected {
            reason: Reason::UnsupportedAction
        }
    );
    assert_eq!(*peer.lines.lock().unwrap(), ["f", "m", "t", "s"]);
}
