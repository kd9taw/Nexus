//! Exercise the actual TCP write boundary with an isolated loopback CAT peer.
use super::*;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tempo_app::remote_control::{Completion, Evidence, Outcome, Reason, Revocation};

use super::remote::{Position, Retune};

pub(crate) struct Peer {
    pub(crate) address: String,
    pub(crate) lines: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Peer {
    fn new(reply: impl Fn(&str) -> String + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (observed, finished) = (lines.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            while !finished.load(Ordering::SeqCst) {
                let Ok((socket, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                };
                socket
                    .set_read_timeout(Some(Duration::from_millis(20)))
                    .unwrap();
                let mut socket = BufReader::new(socket);
                while !finished.load(Ordering::SeqCst) {
                    let mut line = String::new();
                    match socket.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            observed.lock().unwrap().push(line.trim_end().to_string());
                            if socket
                                .get_mut()
                                .write_all(reply(line.trim_end()).as_bytes())
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(e) if read_should_retry(e.kind()) => continue,
                        Err(_) => break,
                    }
                }
            }
        });
        Self {
            address,
            lines,
            stop,
            thread: Some(thread),
        }
    }

    fn healthy() -> Self {
        Self::new(|line| match line {
            "f" => "7074000\n".into(),
            "m" => "PKTUSB\n2400\n".into(),
            _ => "RPRT 0\n".into(),
        })
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}

fn permission(authority: &Revocation, native: &Revocation) -> (WritePermission, Completion) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let authority = authority.permit(deadline).unwrap();
    let completion = Completion::guarded(authority.clone());
    (
        WritePermission::new(
            authority,
            native.permit(deadline).unwrap(),
            completion.clone(),
        ),
        completion,
    )
}

pub(crate) struct RadioState {
    pub(crate) dial: u64,
    pub(crate) mode: String,
    pub(crate) keyed: bool,
    pub(crate) band_stack: bool,
}

pub(crate) fn retuning_peer(
    dial: u64,
    mode: &str,
    intercept: impl Fn(&str, &mut RadioState) -> Option<String> + Send + 'static,
) -> Peer {
    let state = Mutex::new(RadioState {
        dial,
        mode: mode.into(),
        keyed: false,
        band_stack: false,
    });
    Peer::new(move |line| {
        let mut state = state.lock().unwrap();
        if let Some(reply) = intercept(line, &mut state) {
            return reply;
        }
        match line {
            "f" => format!("{}\n", state.dial),
            "m" => format!("{}\n2400\n", state.mode),
            "t" => format!("{}\n", u8::from(state.keyed)),
            "s" => "0\nVFOB\n".into(),
            "T 0" => {
                state.keyed = false;
                "RPRT 0\n".into()
            }
            _ if line.starts_with("M ") => {
                let mode = line.split_whitespace().nth(1).unwrap();
                if mode != state.mode {
                    // A mode change reinterprets the dial on pitch-offset rigs.
                    state.dial += 650;
                }
                state.mode = mode.into();
                "RPRT 0\n".into()
            }
            _ if line.starts_with("F ") => {
                let dial = line[2..].parse().unwrap();
                if state.band_stack && !tuning::same_named_band(state.dial, dial) {
                    state.mode = "CW".into();
                }
                state.dial = dial;
                "RPRT 0\n".into()
            }
            _ => "RPRT -1\n".into(),
        }
    })
}

fn retune(from: (u64, &str), to: (u64, &str)) -> Retune {
    Retune::new(
        Position::new(from.0, from.1).unwrap(),
        Position::new(to.0, to.1).unwrap(),
    )
}

pub(crate) fn writes(peer: &Peer) -> Vec<String> {
    peer.lines
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s.starts_with("M ") || s.starts_with("F ") || s.starts_with("T "))
        .cloned()
        .collect()
}

#[test]
fn remote_retune_preserves_mode_before_dial_and_requires_a_separate_station_commit() {
    let peer = retuning_peer(14_240_000, "USB", |_, _| None);
    let mut rig = Rig::rigctld(&peer.address);
    let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
    let observed = rig
        .remote_retune(retune((14_240_000, "USB"), (14_030_000, "CW")), &permission)
        .unwrap();
    assert_eq!(observed.position().dial_hz(), 14_030_000);
    assert_eq!(observed.position().mode(), "CW");
    assert_eq!(writes(&peer), ["M CW -1", "F 14030000"]);
    assert_eq!(completion.outcome(), Outcome::Pending);
    completion.finish(Outcome::Applied {
        evidence: Evidence::RadioReadback,
    });
    assert!(rig
        .remote_retune(retune((14_030_000, "CW"), (14_240_000, "USB")), &permission,)
        .is_err());
    assert_eq!(writes(&peer), ["M CW -1", "F 14030000"]);
}

#[test]
fn remote_retune_requires_fresh_simplex_before_writing() {
    for split in ["1\nVFOB\n", "RPRT -1\n"] {
        let peer = retuning_peer(14_074_000, "PKTUSB", move |line, _| {
            (line == "s").then(|| split.to_string())
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
        assert!(rig
            .remote_retune(
                retune((14_074_000, "PKTUSB"), (7_074_000, "PKTUSB")),
                &permission
            )
            .is_err());
        assert!(writes(&peer).is_empty());
        assert!(matches!(completion.outcome(), Outcome::Rejected { .. }));
    }
}

#[test]
fn same_dial_mode_changes_still_restore_the_pitch_shifted_dial() {
    let peer = retuning_peer(14_030_000, "USB", |_, _| None);
    let mut rig = Rig::rigctld(&peer.address);
    let (permission, _) = permission(&Revocation::default(), &Revocation::default());
    rig.remote_retune(retune((14_030_000, "USB"), (14_030_000, "CW")), &permission)
        .unwrap();
    assert_eq!(writes(&peer), ["M CW -1", "F 14030000"]);
    assert_eq!(rig.read_freq().unwrap(), 14_030_000);
}

#[test]
fn remote_retune_uses_native_filter_policy_and_corrects_band_stacking_once() {
    for (dial, expected) in [
        (14_090_000, vec!["M PKTUSB -1", "F 14090000"]),
        (
            21_074_000,
            vec!["M PKTUSB 3000", "F 21074000", "M PKTUSB 3000", "F 21074000"],
        ),
    ] {
        let peer = retuning_peer(14_074_000, "PKTUSB", |_, state| {
            state.band_stack = true;
            None
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
        let observed = rig
            .remote_retune(
                retune((14_074_000, "PKTUSB"), (dial, "PKTUSB")),
                &permission,
            )
            .unwrap();
        assert_eq!(writes(&peer), expected);
        assert_eq!(observed.position().dial_hz(), dial);
        assert_eq!(observed.position().mode(), "PKTUSB");
        assert_eq!(completion.outcome(), Outcome::Pending);
    }
}

#[test]
fn changed_or_unavailable_preconditions_refuse_all_remote_writes() {
    for failure in ["frequency", "mode", "keyed", "missingPtt", "missingMode"] {
        let peer = retuning_peer(14_074_000, "PKTUSB", move |line, state| match failure {
            "frequency" => {
                state.dial = 14_080_000;
                None
            }
            "mode" => {
                state.mode = "USB".into();
                None
            }
            "keyed" => {
                state.keyed = true;
                None
            }
            "missingPtt" if line == "t" => Some("RPRT -1\n".into()),
            "missingMode" if line == "m" => Some("RPRT -1\n".into()),
            _ => None,
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
        assert!(rig
            .remote_retune(
                retune((14_074_000, "PKTUSB"), (14_240_000, "USB")),
                &permission,
            )
            .is_err());
        assert!(matches!(completion.outcome(), Outcome::Rejected { .. }));
        assert!(writes(&peer).is_empty(), "{failure}");
        // The refusal is scoped to Remote; local CAT and unconditional unkey
        // still pass through the actual socket.
        rig.ptt(false).unwrap();
        rig.set_freq(14_090_000).unwrap();
        assert_eq!(writes(&peer), ["T 0", "F 14090000"]);
    }
}

#[test]
fn an_accepted_mode_that_is_not_reported_never_receives_a_dial_write() {
    let peer = retuning_peer(14_240_000, "USB", |line, _| {
        line.starts_with("M ").then(|| "RPRT 0\n".into())
    });
    let mut rig = Rig::rigctld(&peer.address);
    let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
    assert!(rig
        .remote_retune(retune((14_240_000, "USB"), (14_030_000, "CW")), &permission,)
        .is_err());
    assert_eq!(writes(&peer), ["M CW -1"]);
    assert!(matches!(completion.outcome(), Outcome::Unknown { .. }));
}

#[test]
fn a_same_band_mode_discrepancy_does_not_trigger_band_stack_correction() {
    let peer = retuning_peer(14_074_000, "PKTUSB", |line, state| {
        if line.starts_with("F ") {
            state.mode = "USB".into();
        }
        None
    });
    let mut rig = Rig::rigctld(&peer.address);
    let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
    assert!(rig
        .remote_retune(
            retune((14_074_000, "PKTUSB"), (14_090_000, "PKTUSB")),
            &permission,
        )
        .is_err());
    assert_eq!(writes(&peer), ["M PKTUSB -1", "F 14090000"]);
    assert!(matches!(completion.outcome(), Outcome::Unknown { .. }));
}

#[test]
fn context_loss_after_a_mode_write_forbids_the_next_dial_write() {
    for cancel_authority in [true, false] {
        let authority = Arc::new(Revocation::default());
        let native = Arc::new(Revocation::default());
        let changed = if cancel_authority {
            authority.clone()
        } else {
            native.clone()
        };
        let peer = retuning_peer(14_240_000, "USB", move |line, _| {
            if line.starts_with("M ") {
                changed.revoke();
            }
            None
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, completion) = permission(&authority, &native);
        assert!(rig
            .remote_retune(retune((14_240_000, "USB"), (14_030_000, "CW")), &permission,)
            .is_err());
        assert_eq!(writes(&peer), ["M CW -1"]);
        assert!(matches!(completion.outcome(), Outcome::Unknown { .. }));
        rig.ptt(false).unwrap();
        assert_eq!(writes(&peer), ["M CW -1", "T 0"]);
    }
}

#[test]
fn final_readback_after_authority_loss_cannot_become_success() {
    let authority = Arc::new(Revocation::default());
    let changed = authority.clone();
    let peer = retuning_peer(14_240_000, "USB", move |line, state| {
        if line == "f" && state.dial == 14_030_000 {
            changed.revoke();
        }
        None
    });
    let mut rig = Rig::rigctld(&peer.address);
    let (permission, completion) = permission(&authority, &Revocation::default());
    assert!(rig
        .remote_retune(retune((14_240_000, "USB"), (14_030_000, "CW")), &permission,)
        .is_err());
    assert_eq!(writes(&peer), ["M CW -1", "F 14030000"]);
    assert!(matches!(completion.outcome(), Outcome::Unknown { .. }));
}

#[test]
fn a_refused_or_ignored_frequency_stays_unknown_without_retry() {
    for reply in ["RPRT -1\n", "RPRT 0\n"] {
        let peer = retuning_peer(14_074_000, "PKTUSB", move |line, _| {
            line.starts_with("F ").then(|| reply.into())
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
        assert!(rig
            .remote_retune(
                retune((14_074_000, "PKTUSB"), (14_090_000, "PKTUSB")),
                &permission,
            )
            .is_err());
        assert_eq!(writes(&peer), ["M PKTUSB -1", "F 14090000"]);
        assert!(matches!(completion.outcome(), Outcome::Unknown { .. }));
        assert_eq!(rig.read_freq().unwrap(), 14_074_000);
    }
}

#[test]
fn bad_band_cross_readback_cannot_trigger_blind_or_repeated_corrections() {
    for failure in ["noise", "ignoredCorrection"] {
        let peer = retuning_peer(14_074_000, "PKTUSB", move |line, state| {
            state.band_stack = true;
            if failure == "noise" && state.dial == 21_074_000 && line == "m" {
                return Some("not a mode\n".into());
            }
            if failure == "ignoredCorrection" && state.mode == "CW" && line.starts_with("M ") {
                return Some("RPRT 0\n".into());
            }
            None
        });
        let mut rig = Rig::rigctld(&peer.address);
        let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
        assert!(rig
            .remote_retune(
                retune((14_074_000, "PKTUSB"), (21_074_000, "PKTUSB")),
                &permission,
            )
            .is_err());
        assert_eq!(
            writes(&peer),
            if failure == "noise" {
                vec!["M PKTUSB 3000", "F 21074000"]
            } else {
                vec!["M PKTUSB 3000", "F 21074000", "M PKTUSB 3000"]
            }
        );
        assert!(matches!(completion.outcome(), Outcome::Unknown { .. }));
    }
}

#[test]
fn expired_or_cancelled_retunes_cannot_write_and_fresh_local_control_still_works() {
    let peer = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
    let mut rig = Rig::rigctld(&peer.address);
    let authority = Revocation::default();
    let native = Arc::new(Revocation::default());
    let expired = authority.permit(std::time::Instant::now()).unwrap();
    let completion = Completion::guarded(expired.clone());
    let stale = WritePermission::new(expired.clone(), expired, completion.clone());
    assert!(rig
        .remote_retune(
            retune((14_074_000, "PKTUSB"), (14_090_000, "PKTUSB")),
            &stale,
        )
        .is_err());
    assert!(peer.lines.lock().unwrap().is_empty());
    assert_eq!(
        completion.outcome(),
        Outcome::Rejected {
            reason: Reason::AuthorityExpired
        }
    );

    let (permission, completion) = permission(&authority, &native);
    let changed = native.clone();
    rig.before_remote_write = Some(Box::new(move || changed.revoke()));
    assert!(rig
        .remote_retune(
            retune((14_074_000, "PKTUSB"), (14_090_000, "PKTUSB")),
            &permission,
        )
        .is_err());
    assert!(writes(&peer).is_empty());
    assert_eq!(
        completion.outcome(),
        Outcome::Rejected {
            reason: Reason::ContextChanged
        }
    );
    rig.set_freq(14_090_000).unwrap();
    assert_eq!(rig.read_freq().unwrap(), 14_090_000);
    assert_eq!(writes(&peer), ["F 14090000"]);
}

#[test]
fn an_unchanged_mode_and_dial_reasserts_mode_without_rewriting_frequency() {
    let peer = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
    let mut rig = Rig::rigctld(&peer.address);
    let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
    rig.remote_retune(
        retune((14_074_000, "PKTUSB"), (14_074_000, "PKTUSB")),
        &permission,
    )
    .unwrap();
    assert_eq!(writes(&peer), ["M PKTUSB -1"]);
    assert_eq!(completion.outcome(), Outcome::Pending);
    for (hz, mode) in [
        (0, "USB"),
        (14_074_000, "USB\nT 1"),
        (14_074_000, "futureMode"),
    ] {
        assert_eq!(Position::new(hz, mode).unwrap_err(), Reason::InvalidAction);
    }
}

#[test]
fn remote_cat_uses_the_existing_codec_but_an_ack_is_not_a_completion() {
    let peer = Peer::healthy();
    let mut rig = Rig::rigctld(&peer.address);
    let (permission, completion) = permission(&Revocation::default(), &Revocation::default());
    rig.remote_set_mode("PKTUSB", -1, &permission).unwrap();
    rig.remote_set_freq(7_074_000, &permission).unwrap();
    assert_eq!(completion.outcome(), Outcome::Pending);
    assert_eq!(rig.read_freq().unwrap(), 7_074_000);
    assert_eq!(
        *peer.lines.lock().unwrap(),
        ["M PKTUSB -1", "F 7074000", "f"]
    );
    completion.finish(Outcome::Applied {
        evidence: Evidence::RadioReadback,
    });
    assert!(
        rig.remote_set_freq(14_074_000, &permission).is_err(),
        "a terminal operation cannot write again"
    );
    rig.ptt(false).unwrap();
    assert_eq!(peer.lines.lock().unwrap().last().unwrap(), "T 0");
}

#[test]
fn revocation_after_connection_and_stale_reply_drain_stops_the_actual_write() {
    for source in ["authority", "native"] {
        let peer = Peer::healthy();
        let mut rig = Rig::rigctld(&peer.address);
        let authority = Arc::new(Revocation::default());
        let native = Arc::new(Revocation::default());
        let (permission, completion) = permission(&authority, &native);
        let changed = if source == "authority" {
            authority
        } else {
            native
        };
        rig.before_remote_write = Some(Box::new(move || changed.revoke()));
        rig.keyed = true; // a lost link must not erase the native unkey obligation
        assert_eq!(
            rig.remote_set_freq(7_074_000, &permission)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert!(matches!(completion.outcome(), Outcome::Rejected { .. }));
        assert!(rig.keyed);
        rig.ptt(false).unwrap();
        assert!(!rig.keyed);
        assert_eq!(
            *peer.lines.lock().unwrap(),
            ["T 0"],
            "{source}: no frequency byte reached the peer, and unkey still works"
        );
        // Positive local control: Remote cancellation must not gate local CAT.
        rig.set_freq(14_074_000).unwrap();
        assert_eq!(peer.lines.lock().unwrap().last().unwrap(), "F 14074000");
    }
}

#[test]
fn losing_authority_during_a_mode_reply_prevents_the_next_write_and_stays_unknown() {
    let authority = Arc::new(Revocation::default());
    let lost = authority.clone();
    let peer = Peer::new(move |line| {
        if line.starts_with("M ") {
            lost.revoke();
        }
        "RPRT 0\n".into()
    });
    let mut rig = Rig::rigctld(&peer.address);
    let (permission, completion) = permission(&authority, &Revocation::default());
    rig.remote_set_mode("CW", -1, &permission).unwrap();
    assert!(rig.remote_set_freq(7_030_000, &permission).is_err());
    assert_eq!(
        completion.outcome(),
        Outcome::Unknown {
            reason: Reason::HardwareUnconfirmed
        }
    );
    rig.ptt(false).unwrap();
    assert_eq!(*peer.lines.lock().unwrap(), ["M CW -1", "T 0"]);
}

#[test]
fn remote_cat_refuses_missing_hardware_and_mode_injection() {
    let peer = Peer::healthy();
    let mut rig = Rig::rigctld(&peer.address);
    let (permission, _) = permission(&Revocation::default(), &Revocation::default());
    for mode in ["", "CW\nT 1", "USB 0"] {
        assert_eq!(
            rig.remote_set_mode(mode, -1, &permission)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
    }
    let mut missing = Rig::with_control(None, PttMode::Vox);
    assert_eq!(
        missing
            .remote_set_freq(7_074_000, &permission)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::NotConnected
    );
    rig.ptt(false).unwrap();
    assert_eq!(*peer.lines.lock().unwrap(), ["T 0"]);
}
