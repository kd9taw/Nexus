//! Exercise the actual TCP write boundary with an isolated loopback CAT peer.
use super::*;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tempo_app::remote_control::{Completion, Evidence, Outcome, Reason, Revocation};

struct Peer {
    address: String,
    lines: Arc<Mutex<Vec<String>>>,
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
