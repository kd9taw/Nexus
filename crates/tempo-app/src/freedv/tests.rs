//! FreeDV's logged contacts and heard calls, by value, through the importer the listener runs
//! and through real sockets on this computer. Every datagram here is built by Nexus's own WSJT-X
//! encoder; none is a capture of FreeDV's.

use super::*;
use crate::test_util::StoredLog;
use std::sync::atomic::AtomicUsize;
use tempo_net::wsjtx::{encode_heartbeat, encode_qso_logged, QsoLogged};

/// 2026-10-14 01:02:03 UTC.
const WHEN: i64 = 1_791_939_723;

fn qso<'a>(my_call: &'a str, dx_call: &'a str) -> QsoLogged<'a> {
    QsoLogged {
        time_off: WHEN,
        time_on: WHEN,
        dx_call,
        dx_grid: "EM12kx",
        tx_freq: 14_236_000,
        mode: "DIGITALVOICE",
        report_sent: "59",
        report_recvd: "57",
        name: "Pat",
        comments: "RADE V1, 7 dB",
        my_call,
        my_grid: "FN31",
        ..Default::default()
    }
}

fn datagram(id: &str, my_call: &str, dx_call: &str) -> Vec<u8> {
    encode_qso_logged(id, &qso(my_call, dx_call))
}

/// The lines a note was given, `(level, message)`, in order.
type Lines = Arc<Mutex<Vec<(String, String)>>>;

/// An engine whose station call is K2DEF, and a note that keeps its lines.
fn station() -> (Mutex<Engine>, Note, Lines) {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let l = Arc::clone(&lines);
    let note: Note = Arc::new(move |level, msg| {
        l.lock().unwrap().push((level.to_string(), msg));
    });
    (Mutex::new(Engine::new("K2DEF", "FN31", 0)), note, lines)
}

fn calls(e: &Mutex<Engine>) -> Vec<String> {
    e.stored_log().iter().map(|r| r.call.clone()).collect()
}

#[test]
fn freedvs_logged_contact_lands_in_the_log_as_a_freedv_contact() {
    let (e, note, lines) = station();
    take_log_datagram(&e, &datagram("FreeDV", "k2def", "W5FDV"), &note);
    let log = e.stored_log();
    assert_eq!(log.len(), 1, "one contact");
    let r = &log[0];
    assert_eq!(r.call, "W5FDV");
    assert_eq!(r.mode, "FREEDV");
    assert_eq!(r.band, "20m");
    assert!((r.freq_mhz - 14.236).abs() < 1e-9, "{}", r.freq_mhz);
    assert_eq!(r.grid.as_deref(), Some("EM12kx"));
    assert_eq!(r.rst_sent.as_deref(), Some("59"));
    assert_eq!(r.rst_rcvd.as_deref(), Some("57"));
    assert_eq!(r.name.as_deref(), Some("Pat"));
    assert_eq!(r.comment.as_deref(), Some("RADE V1, 7 dB"));
    assert_eq!(r.station_callsign.as_deref(), Some("K2DEF"));
    // FreeDV sends one instant for start and end; the record says exactly that.
    assert_eq!(r.when_unix, WHEN as u64);
    assert_eq!(r.time_off_unix, Some(WHEN as u64));
    // Written the way LoTW counts it.
    let adif = tempo_core::logbook::adif_record(r);
    assert!(
        adif.contains("<MODE:12>DIGITALVOICE<SUBMODE:6>FREEDV"),
        "{adif}"
    );
    let f = engine_lock(&e).freedv_mut().clone();
    assert_eq!(f.last_contact.as_deref(), Some("W5FDV · 14.236 MHz"));
    assert_eq!(f.notice, None);
    assert_eq!(
        lines.lock().unwrap().as_slice(),
        [(
            "info".to_string(),
            "FreeDV logged W5FDV · 14.236 MHz: in the log".to_string()
        )]
    );
}

#[test]
fn the_same_contact_twice_is_one_contact() {
    let (e, note, lines) = station();
    let d = datagram("FreeDV", "K2DEF", "W5FDV");
    take_log_datagram(&e, &d, &note);
    take_log_datagram(&e, &d, &note);
    assert_eq!(calls(&e), ["W5FDV"]);
    assert_eq!(
        lines.lock().unwrap()[1].1,
        "FreeDV logged W5FDV · 14.236 MHz: already in the log"
    );
}

#[test]
fn another_programs_contact_and_freedvs_heartbeat_add_nothing() {
    let (e, note, lines) = station();
    take_log_datagram(&e, &datagram("WSJT-X", "K2DEF", "W5FDV"), &note);
    assert!(calls(&e).is_empty(), "another id is not FreeDV");
    assert_eq!(engine_lock(&e).freedv_mut().alive_unix, None);
    take_log_datagram(&e, &encode_heartbeat("FreeDV", 3, "2.4.0", ""), &note);
    assert!(calls(&e).is_empty());
    assert!(
        engine_lock(&e).freedv_mut().alive_unix.is_some(),
        "FreeDV's heartbeat says it is there"
    );
    assert!(lines.lock().unwrap().is_empty());
}

#[test]
fn a_contact_logged_under_another_call_is_refused_with_a_notice() {
    let (e, note, lines) = station();
    take_log_datagram(&e, &datagram("FreeDV", "W1AW", "W5FDV"), &note);
    assert!(calls(&e).is_empty(), "never written under the wrong call");
    let why = "FreeDV logged W5FDV as W1AW, not imported: this station is K2DEF";
    assert_eq!(engine_lock(&e).freedv_mut().notice.as_deref(), Some(why));
    assert_eq!(
        lines.lock().unwrap().as_slice(),
        [("error".to_string(), why.to_string())]
    );
    // No call sent by FreeDV, and none set in Nexus: refused, each with its reason.
    take_log_datagram(&e, &datagram("FreeDV", "", "W5FDV"), &note);
    assert!(engine_lock(&e)
        .freedv_mut()
        .notice
        .as_deref()
        .is_some_and(|n| n.contains("FreeDV sent no station call")));
    let (_, note2, _) = station();
    let blank = Mutex::new(Engine::new("", "FN31", 0));
    take_log_datagram(&blank, &datagram("FreeDV", "K2DEF", "W5FDV"), &note2);
    assert!(calls(&blank).is_empty());
    assert!(engine_lock(&blank)
        .freedv_mut()
        .notice
        .as_deref()
        .is_some_and(|n| n.contains("Nexus has no station call")));
}

#[test]
fn a_malformed_datagram_leaves_the_log_untouched() {
    let (e, note, lines) = station();
    take_log_datagram(&e, &datagram("FreeDV", "K2DEF", "W5FDV"), &note);
    let good = datagram("FreeDV", "K2DEF", "N0FDV");
    // The encoder ends with WSJT-X's three optional trailing fields (satellite, satellite mode,
    // Rx frequency: 12 bytes while empty), which the reader takes when a sender includes them and
    // does without when it stops early. So the last cut takes one byte off the last field every
    // sender writes, the propagation mode, not off that optional tail.
    const OPTIONAL_TAIL: usize = 12;
    assert!(
        good.ends_with(&[0; OPTIONAL_TAIL]),
        "premise: the tail is three empty fields"
    );
    for cut in [0, 4, 12, 30, good.len() - OPTIONAL_TAIL - 1] {
        take_log_datagram(&e, &good[..cut], &note);
    }
    take_log_datagram(&e, b"\xff\xfe garbage", &note);
    assert_eq!(calls(&e), ["W5FDV"], "only the whole contact");
    assert_eq!(lines.lock().unwrap().len(), 1);
}

/// FreeDV's text fields are untrusted: a comment that spells out an ADIF tag stays text in
/// the one contact it came with, and a call that is no call, or no time, is refused.
#[test]
fn text_from_freedv_cannot_open_a_field_or_a_second_contact() {
    let (e, note, _) = station();
    let mut q = qso("K2DEF", "W5FDV");
    q.comments = "hi<EOR><CALL:4>W1XX<MODE:2>CW<EOR>";
    q.name = "\u{7}Pat\u{0}";
    take_log_datagram(&e, &encode_qso_logged("FreeDV", &q), &note);
    let log = e.stored_log();
    assert_eq!(calls(&e), ["W5FDV"]);
    assert_eq!(
        log[0].comment.as_deref(),
        Some("hiEORCALL:4W1XXMODE:2CWEOR")
    );
    assert_eq!(log[0].name.as_deref(), Some("Pat"));

    let mut q = qso("K2DEF", "NOTACALL");
    take_log_datagram(&e, &encode_qso_logged("FreeDV", &q), &note);
    q.dx_call = "W6FDV";
    q.time_on = 0;
    take_log_datagram(&e, &encode_qso_logged("FreeDV", &q), &note);
    assert_eq!(calls(&e), ["W5FDV"]);
}

#[test]
fn adif_time_is_utc_by_value() {
    assert_eq!(
        adif_time(WHEN),
        Some(("20261014".to_string(), "010203".to_string()))
    );
    assert_eq!(
        adif_time(951_782_400), // 2000-02-29 00:00:00
        Some(("20000229".to_string(), "000000".to_string()))
    );
    assert_eq!(adif_time(0), None);
    assert_eq!(adif_time(i64::MAX), None);
}

/// THE LISTENER IS THIS COMPUTER'S ALONE. `bind_log` binds 127.0.0.1, read back from the socket;
/// a datagram sent to it from this computer is imported; the same datagram sent to one of this
/// computer's network addresses on that port reaches nothing. The control is the first send:
/// the path works, so the second's silence is the bind, not a broken sender.
#[test]
fn the_contact_listener_is_bound_to_this_computer_alone() {
    let sock = bind_log(0).expect("bind an ephemeral loopback port");
    let at = sock.local_addr().unwrap();
    assert!(at.ip().is_loopback(), "bound to {at}");
    let port = at.port();
    let (e, note, _) = station();
    let e = Arc::new(e);
    let shutdown = Arc::new(AtomicBool::new(false));
    let (e2, sd) = (Arc::clone(&e), Arc::clone(&shutdown));
    let served = std::thread::spawn(move || serve_log_until(sock, e2, sd, note));
    let sender = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
    // Off-loopback first, if this computer has a network address: nothing may arrive.
    let lan = tempo_net::fdsync::lan_addrs();
    if let Some(a) = lan.first() {
        let _ = sender.send_to(&datagram("FreeDV", "K2DEF", "W9LAN"), (a.ip, port));
    }
    sender
        .send_to(
            &datagram("FreeDV", "K2DEF", "W5FDV"),
            (Ipv4Addr::LOCALHOST, port),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while engine_lock(&e).freedv_mut().last_contact.is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(300));
    shutdown.store(true, Ordering::Relaxed);
    served.join().unwrap();
    assert_eq!(
        calls(&e),
        ["W5FDV"],
        "loopback arrives; {} LAN address(es) tried",
        lan.len()
    );
}

/// A datagram one byte past the bound is dropped whole by the listener, and one at the bound is
/// read: the buffer is one byte longer than the bound so the two can be told apart. The loop runs
/// with a bound under the loopback MTU (`serve_log_within`), since some hosts' loopback drops a
/// 4 KB UDP datagram before the socket sees it; the shipped bound itself is the reader's
/// (`tempo_net::freedv`'s own test of a datagram one byte past `MAX_DATAGRAM`), and
/// `serve_log_until` hands the loop exactly that bound.
#[test]
fn the_listener_drops_a_datagram_past_the_bound() {
    const BOUND: usize = 1024;
    let sock = bind_log(0).unwrap();
    let port = sock.local_addr().unwrap().port();
    let seen = Arc::new(AtomicUsize::new(0));
    let (e, _, _) = station();
    let e = Arc::new(e);
    let shutdown = Arc::new(AtomicBool::new(false));
    let s2 = Arc::clone(&seen);
    let note: Note = Arc::new(move |_, _| {
        s2.fetch_add(1, Ordering::Relaxed);
    });
    let (e2, sd) = (Arc::clone(&e), Arc::clone(&shutdown));
    let served = std::thread::spawn(move || serve_log_within(sock, e2, sd, note, BOUND));
    let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let mut long = datagram("FreeDV", "K2DEF", "W1BIG");
    long.resize(BOUND + 1, b' ');
    sender.send_to(&long, (Ipv4Addr::LOCALHOST, port)).unwrap();
    let mut fits = datagram("FreeDV", "K2DEF", "W5FDV");
    fits.resize(BOUND, b' ');
    sender.send_to(&fits, (Ipv4Addr::LOCALHOST, port)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while seen.load(Ordering::Relaxed) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    shutdown.store(true, Ordering::Relaxed);
    served.join().unwrap();
    assert_eq!(calls(&e), ["W5FDV"]);
}

#[test]
fn heard_calls_are_newest_first_one_row_each_and_capped() {
    let mut f = FreedvDto::default();
    let h = |c: &str| HeardCall {
        call: c.into(),
        mode: "RADEV1".into(),
        snr: Some(3),
        frequency_hz: Some(14_236_000),
    };
    f.add_heard(h("W5FDV"), 1);
    f.add_heard(h("VK5DGR"), 2);
    f.add_heard(h("W5FDV"), 3);
    let order: Vec<_> = f
        .heard
        .iter()
        .map(|r| (r.call.as_str(), r.heard_unix))
        .collect();
    assert_eq!(order, [("W5FDV", 3), ("VK5DGR", 2)]);
    for i in 0..(HEARD_CAP + 10) {
        f.add_heard(h(&format!("W{i}AA")), 10 + i as i64);
    }
    assert_eq!(f.heard.len(), HEARD_CAP);
    assert_eq!(f.heard[0].call, format!("W{}AA", HEARD_CAP + 9));
}

/// The heard listener reads FreeDV's JSON from this computer into the list. Bound to loopback
/// here (the test's socket); the shipped bind is [`tempo_net::freedv::bind_heard`].
#[test]
fn the_heard_listener_lists_freedvs_broadcast() {
    let sock = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = sock.local_addr().unwrap().port();
    let (e, _, _) = station();
    let e = Arc::new(e);
    let shutdown = Arc::new(AtomicBool::new(false));
    let (e2, sd) = (Arc::clone(&e), Arc::clone(&shutdown));
    let served = std::thread::spawn(move || serve_heard_until(sock, e2, sd));
    let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    for b in [
        &b"not json"[..],
        br#"{"type":"fdv_callsign","callsign":"W5FDV","mode":"RADEV1","snr":7,"frequency_hz":14236000}"#,
    ] {
        sender.send_to(b, (Ipv4Addr::LOCALHOST, port)).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while engine_lock(&e).freedv_mut().heard.is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    shutdown.store(true, Ordering::Relaxed);
    served.join().unwrap();
    let heard = engine_lock(&e).freedv_mut().heard.clone();
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].call, "W5FDV");
    assert_eq!(heard[0].snr, Some(7));
    assert_eq!(heard[0].frequency_hz, Some(14_236_000));
}

/// The snapshot carries `freedv` only while a listener is wanted, so a snapshot with both off
/// is the one it always was.
#[test]
fn the_snapshot_carries_freedv_only_while_a_listener_is_wanted() {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    assert_eq!(e.snapshot().freedv, None);
    let v = serde_json::to_value(e.snapshot()).unwrap();
    assert!(v.get("freedv").is_none());
    e.freedv_mut().log_port = Some(2239);
    assert_eq!(e.snapshot().freedv.and_then(|f| f.log_port), Some(2239));
    e.freedv_mut().log_port = None;
    e.freedv_mut().heard_error = Some("x".into());
    assert!(e.snapshot().freedv.is_some());
}

/// The manager's slot: a port taken by another socket is a refusal said once, not every
/// second; a different port is tried at once; switching off stops the listener.
#[test]
fn a_refused_bind_is_said_once_and_a_new_port_is_tried_at_once() {
    let holder = bind_log(0).unwrap();
    let taken = holder.local_addr().unwrap().port();
    let mut slot = Slot::default();
    let serve = |_: UdpSocket, _: Arc<AtomicBool>| {};
    assert!(
        matches!(slot.reconcile(Some(taken), bind_log, serve), Some(Err((p, _))) if p == taken)
    );
    assert!(
        slot.reconcile(Some(taken), bind_log, serve).is_none(),
        "said once"
    );
    let free = {
        let probe = bind_log(0).unwrap();
        probe.local_addr().unwrap().port()
    };
    assert!(matches!(slot.reconcile(Some(free), bind_log, serve), Some(Ok(p)) if p == free));
    let flag = Arc::clone(&slot.running.as_ref().unwrap().shutdown);
    assert!(slot.reconcile(None, bind_log, serve).is_none());
    assert!(slot.running.is_none());
    assert!(
        flag.load(Ordering::Relaxed),
        "the old listener was told to stop"
    );
}

#[test]
fn the_defaults_listen_for_contacts_on_2239_and_not_for_heard_calls() {
    let s = crate::settings::Settings::default();
    assert!(s.freedv_log);
    assert_eq!(s.freedv_log_port, 2239);
    assert!(!s.freedv_heard);
    assert_eq!(s.freedv_heard_port, 7177);
    // A settings file from before these fields existed reads the same.
    let old: crate::settings::Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(
        (
            old.freedv_log,
            old.freedv_log_port,
            old.freedv_heard,
            old.freedv_heard_port
        ),
        (true, 2239, false, 7177)
    );
}
