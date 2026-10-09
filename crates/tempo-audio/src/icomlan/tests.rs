//! The daemon end to end: real UDP sockets on loopback, the simulated network radio
//! (`tempo_net::icom::sim`) answering the session, and behind its CI-V port the same `FakeRadio`
//! the serial engine's tests use. Made-up credentials and loopback addresses only.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tempo_net::icom::sim::{self, Entry, SimRadio, SocketRadio};
use tempo_net::icom::wire::{self, ControlOp, Packet, Role, TokenRequest};

use super::*;
use crate::civ::commands;
use crate::civ::engine::tests_support::{FakeRadio, Regs};
use crate::civ::frame::{freq_to_bcd, Frame, CONTROLLER};

/// The password the simulated radio's user has: an obvious fake.
fn creds(_profile: u32) -> Result<Option<String>, String> {
    Ok(Some(sim::PASSWORD.to_string()))
}

/// The CI-V frames in `bytes`, each `FE FE` to `FD`.
fn split_frames(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut cur = Vec::new();
    for &b in bytes {
        cur.push(b);
        if b == 0xFD {
            frames.push(std::mem::take(&mut cur));
        }
    }
    frames
}

/// A simulated network radio with the CI-V `FakeRadio` behind its CI-V port: the commands the
/// session delivers are written to the fake, its reply goes back as the radio's answer, and
/// anything the fake sends unprompted (a transceive push, a scope sweep) goes out as the radio's
/// own frames.
struct Bench {
    radio: SocketRadio,
    regs: Arc<Mutex<Regs>>,
    /// Bytes the fake sends without being asked, as its own transceive and scope traffic.
    push: Arc<Mutex<Vec<u8>>>,
}

fn bench_with(model: IcomModel, advertised: &str, tune: impl FnOnce(&mut SimRadio)) -> Bench {
    let addr = model.default_civ_addr();
    let (fake, push) = FakeRadio::new(addr);
    let regs = fake.regs();
    let fake = Arc::new(Mutex::new(fake));
    let queued: Arc<Mutex<VecDeque<Vec<u8>>>> = Arc::default();
    let mut radio = SimRadio::new();
    radio.radios = vec![Entry::new(advertised, addr)];
    let (f, q) = (fake.clone(), queued.clone());
    radio.responder = Some(Box::new(move |cmd: &[u8]| {
        let mut fake = f.lock().unwrap();
        fake.write_all(cmd).unwrap();
        let mut buf = [0u8; 4096];
        let n = fake.read(&mut buf).unwrap_or(0);
        let mut frames = split_frames(&buf[..n]);
        if frames.is_empty() {
            return None; // a lost reply
        }
        let reply = frames.remove(0);
        q.lock().unwrap().extend(frames);
        Some(reply)
    }));
    let (p, q) = (push.clone(), queued);
    radio.unprompted = Some(Box::new(move || {
        let mut out: Vec<Vec<u8>> = q.lock().unwrap().drain(..).collect();
        out.extend(split_frames(&std::mem::take(&mut *p.lock().unwrap())));
        out
    }));
    tune(&mut radio);
    Bench {
        radio: SocketRadio::start(radio),
        regs,
        push,
    }
}

fn bench(model: IcomModel) -> Bench {
    let name = net_model(model).expect("a network radio").radio_name();
    bench_with(model, name, |_| {})
}

fn target(b: &Bench, model: IcomModel) -> Target {
    Target {
        host: Ipv4Addr::LOCALHOST,
        control_port: b.radio.control_port,
        model,
        user: sim::USER.to_string(),
        profile_id: 7,
        rigctld_port: 0,
        data_mode: 1,
    }
}

fn start(t: &Target) -> Result<IcomLanDaemon, StartError> {
    IcomLanDaemon::start_with(t, &creds, Arc::new(now_ms))
}

/// Every datagram the simulated radio received, decoded.
fn received(b: &Bench, from: usize) -> Vec<(Role, Packet)> {
    b.radio.with(|r| {
        r.received[from..]
            .iter()
            .filter_map(|(role, bytes)| wire::decode(*role, bytes).ok().map(|p| (*role, p)))
            .collect()
    })
}

fn received_len(b: &Bench) -> usize {
    b.radio.with(|r| r.received.len())
}

/// The CI-V frames the radio was sent.
fn civ_frames(b: &Bench) -> Vec<Vec<u8>> {
    b.radio.with(|r| r.civ_commands.clone())
}

fn until(max: Duration, mut done: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + max;
    while !done() {
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    true
}

struct Client {
    c: TcpStream,
    rd: BufReader<TcpStream>,
}

impl Client {
    fn to(addr: std::net::SocketAddr) -> Client {
        let c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(Duration::from_millis(400)))
            .unwrap();
        let rd = BufReader::new(c.try_clone().unwrap());
        Client { c, rd }
    }

    /// Sends one line and reads every reply line until the daemon goes quiet.
    fn ask(&mut self, line: &str) -> String {
        self.c.write_all(line.as_bytes()).unwrap();
        let mut out = String::new();
        loop {
            let mut l = String::new();
            match self.rd.read_line(&mut l) {
                Ok(0) | Err(_) => break,
                Ok(_) => out.push_str(&l),
            }
        }
        out
    }
}

// ── CAT over the network: the same daemon, the same answers ──────────────────────────────────

/// ⭐ THE SAME VERB TABLE GIVES THE SAME ANSWERS over a serial bus and over the network, against
/// the same fake radio: the network daemon is the native CI-V daemon, carried.
#[test]
fn the_same_verbs_answer_the_same_over_serial_and_over_the_network() {
    let model = IcomModel::Ic9700;
    let addr = model.default_civ_addr();
    let script = [
        "f\n",
        "F 14074000\n",
        "f\n",
        "m\n",
        "M USB 2400\n",
        "m\n",
        "l STRENGTH\n",
        "\\dump_state\n",
    ];
    let transcript = |client: &mut Client| -> Vec<String> {
        script.iter().map(|line| client.ask(line)).collect()
    };
    let (fake, _push) = FakeRadio::new(addr);
    let serial = CivDaemon::start_with_io(Box::new(fake), addr, 0, 1, Some(model)).unwrap();
    let over_serial = transcript(&mut Client::to(serial.local_addr()));
    let b = bench(model);
    let daemon = start(&target(&b, model)).expect("connects");
    let over_lan = transcript(&mut Client::to(daemon.local_addr()));
    assert_eq!(over_lan, over_serial);
    assert!(
        over_serial[2].starts_with("14074000"),
        "precondition: the table really moved the dial: {over_serial:?}"
    );
}

/// A front-panel dial change, pushed by the radio, reaches the dial with no poll.
#[test]
fn a_front_panel_dial_change_reaches_the_dial_with_no_poll() {
    let model = IcomModel::Ic7760;
    let b = bench(model);
    let daemon = start(&target(&b, model)).expect("connects");
    let sent = civ_frames(&b).len();
    let push = Frame {
        to: CONTROLLER,
        from: model.default_civ_addr(),
        cmd: 0x00,
        data: freq_to_bcd(7_074_000).to_vec(),
    };
    b.push.lock().unwrap().extend(push.to_bytes());
    let h = daemon.native().engine_handle();
    assert!(
        until(Duration::from_secs(3), || h.state().freq_hz
            == Some(7_074_000)),
        "the dial followed the radio"
    );
    assert_eq!(civ_frames(&b).len(), sent, "nothing was asked of the radio");
}

/// The scope over the network: the enable goes out (Main pinned on the 7760), and one frame is
/// one sweep, 689 points on the 7760's 0–200 scale.
#[test]
fn the_scope_streams_over_the_network_one_frame_a_sweep() {
    let model = IcomModel::Ic7760;
    let addr = model.default_civ_addr();
    let b = bench(model);
    let daemon = start(&target(&b, model)).expect("connects");
    daemon.native().set_scope_enabled(true);
    let enable = [
        vec![0xFE, 0xFE, addr, 0xE0, 0x27, 0x12, 0x00, 0xFD],
        vec![0xFE, 0xFE, addr, 0xE0, 0x27, 0x10, 0x01, 0xFD],
        vec![0xFE, 0xFE, addr, 0xE0, 0x27, 0x11, 0x01, 0xFD],
    ];
    assert!(until(Duration::from_secs(3), || {
        let sent = civ_frames(&b);
        enable.iter().all(|f| sent.contains(f))
    }));
    // One sweep as the radio sends it over the network: header and 689 points in one frame.
    let mut data = vec![0x00, 0x00, 0x01, 0x01, 0x00];
    data.extend_from_slice(&freq_to_bcd(14_100_000));
    data.extend_from_slice(&freq_to_bcd(50_000));
    data.push(0x00);
    data.extend((0..689usize).map(|i| (i * 200 / 688) as u8));
    let sweep = Frame {
        to: CONTROLLER,
        from: addr,
        cmd: 0x27,
        data,
    };
    b.push.lock().unwrap().extend(sweep.to_bytes());
    let mut row = None;
    assert!(until(Duration::from_secs(3), || {
        if row.is_none() {
            row = daemon.native().take_scope_row();
        }
        row.is_some()
    }));
    let row = row.unwrap();
    assert_eq!(row.row.len(), 689);
    assert_eq!(row.row.last().copied(), Some(1.0));
    assert_eq!((row.lo_hz, row.hi_hz), (14_050_000.0, 14_150_000.0));
}

// ── Nothing transmits ────────────────────────────────────────────────────────────────────────

fn is_key(frame: &[u8]) -> bool {
    let body = &frame[4..frame.len() - 1];
    body == [0x1C, 0x00, 0x01]
        || (body.first() == Some(&0x17) && body != [0x17, 0xFF])
        || body == [0x16, 0x46, 0x01]
}

/// ⭐ `T 1`, `b CQ` and `U VOX 1` are refused and nothing reaches the radio; `T 0`, `\stop_morse`
/// and `U VOX 0` go out.
#[test]
fn keying_is_refused_over_the_network_and_nothing_reaches_the_radio() {
    let model = IcomModel::Ic7760;
    let addr = model.default_civ_addr();
    let b = bench(model);
    let daemon = start(&target(&b, model)).expect("connects");
    let mut c = Client::to(daemon.local_addr());
    for line in ["T 1\n", "b CQ\n", "U VOX 1\n"] {
        assert_eq!(c.ask(line), "RPRT -1\n", "{line:?}");
    }
    let keys: Vec<Vec<u8>> = civ_frames(&b).into_iter().filter(|f| is_key(f)).collect();
    assert_eq!(keys, Vec::<Vec<u8>>::new(), "no key reached the radio");
    c.ask("T 0\n");
    c.ask("\\stop_morse\n");
    c.ask("U VOX 0\n");
    let sent = civ_frames(&b);
    for want in [
        vec![0xFE, 0xFE, addr, 0xE0, 0x1C, 0x00, 0x00, 0xFD],
        vec![0xFE, 0xFE, addr, 0xE0, 0x17, 0xFF, 0xFD],
        vec![0xFE, 0xFE, addr, 0xE0, 0x16, 0x46, 0x00, 0xFD],
    ] {
        assert!(sent.contains(&want), "{want:02X?} went out");
    }
}

/// Dropping the daemon sends the key-up, then gives the token back, then disconnects, in that
/// order.
#[test]
fn dropping_the_daemon_unkeys_then_gives_the_token_back_then_disconnects() {
    let model = IcomModel::Ic7760;
    let addr = model.default_civ_addr();
    let b = bench(model);
    let daemon = start(&target(&b, model)).expect("connects");
    let from = received_len(&b);
    drop(daemon);
    // The radio reads its sockets on its own thread: give it the datagrams in flight.
    until(Duration::from_secs(2), || {
        b.radio.with(|r| r.ctrl_disconnects > 0)
    });
    let after = received(&b, from);
    let unkey = vec![0xFE, 0xFE, addr, 0xE0, 0x1C, 0x00, 0x00, 0xFD];
    let at = |what: &dyn Fn(&(Role, Packet)) -> bool| after.iter().position(what);
    let key_up = at(&|(_, p)| matches!(p, Packet::Civ(c) if c.frame == unkey)).expect("key-up");
    let token = at(&|(role, p)| {
        *role == Role::Control
            && matches!(p, Packet::Token(t) if t.request() == Some(TokenRequest::Remove))
    })
    .expect("token remove");
    let bye = at(&|(role, p)| {
        *role == Role::Control
            && matches!(
                p,
                Packet::Control {
                    op: ControlOp::Disconnect,
                    ..
                }
            )
    })
    .expect("disconnect");
    assert!(key_up < token && token < bye, "{key_up} < {token} < {bye}");
}

// ── One session per radio, and the ladder ────────────────────────────────────────────────────

/// A second start for one radio fails at once and sends nothing. The control: once the first is
/// gone, the same start connects.
#[test]
fn a_second_start_for_one_radio_fails_at_once_and_sends_nothing() {
    let model = IcomModel::Ic705;
    let b = bench(model);
    let t = target(&b, model);
    let first = start(&t).expect("connects");
    let from = received_len(&b);
    let begun = Instant::now();
    let second = start(&t).err().expect("refused");
    assert_eq!(
        second.status,
        "Nexus already has this radio's network session"
    );
    assert!(begun.elapsed() < Duration::from_millis(200), "at once");
    assert_eq!(received_len(&b), from, "nothing was sent");
    drop(first);
    assert!(
        start(&t).is_ok(),
        "the control: the same start, alone, connects"
    );
}

/// ⭐ AFTER A LOST SESSION THE ATTEMPTS CLIMB THE LADDER, on a fake clock ticking like the radio
/// loop: 1, 2, 4, 8 and 16 s apart, then every 30 s, never once per tick; and every attempt that
/// fails part-way gives the token back and disconnects.
#[test]
fn after_a_loss_the_attempts_climb_the_ladder_on_a_fake_clock() {
    let model = IcomModel::Ic9700;
    // A radio that takes the login and refuses the connection request as busy, as one does while
    // it still holds a dead session's slot.
    let b = bench_with(model, "IC-9700", |r| r.status_error = 1);
    let t = target(&b, model);
    let clock = Arc::new(AtomicU64::new(0));
    let fake: Clock = {
        let clock = clock.clone();
        Arc::new(move || clock.load(Ordering::Relaxed))
    };
    // The session that was lost, at t = 0.
    let claim = registry::claim(t.key(), 0).expect("free");
    claim.connected();
    claim.ended(&End::Lost(LossReason::LinkTimeout), 0, "lost");
    let mut attempts = Vec::new();
    let mut tokens = Vec::new();
    // The radio loop's tick, 20 ms, for 92 s.
    for tick in 0..(92_000 / 20) {
        let now = tick * 20;
        clock.store(now, Ordering::Relaxed);
        let before = b.radio.with(|r| (r.token_removes, r.ctrl_disconnects));
        match IcomLanDaemon::start_with(&t, &creds, fake.clone()) {
            Ok(_) => panic!("the busy radio never connects"),
            Err(e) if e.status.starts_with("Reconnecting to the radio in") => {}
            Err(e) => {
                assert_eq!(e.status, "The radio is busy with another session");
                attempts.push(now);
                // The radio reads its sockets on its own thread: give it the datagrams in flight.
                let gave_back = || {
                    let after = b.radio.with(|r| (r.token_removes, r.ctrl_disconnects));
                    after.0 > before.0 && after.1 > before.1
                };
                until(Duration::from_secs(2), gave_back);
                let after = b.radio.with(|r| (r.token_removes, r.ctrl_disconnects));
                tokens.push((after.0 - before.0, after.1 > before.1));
            }
        }
    }
    assert_eq!(
        attempts,
        vec![1_000, 3_000, 7_000, 15_000, 31_000, 61_000, 91_000],
        "1, 2, 4, 8, 16, then 30 s apart"
    );
    assert!(
        tokens
            .iter()
            .all(|&(removed, disconnected)| removed == 1 && disconnected),
        "each failure gave the token back and disconnected: {tokens:?}"
    );
}

// ── What the connect reads ───────────────────────────────────────────────────────────────────

/// The `1A 05` frames the radio was sent, each as `(item, data bytes after it)`.
fn menu_frames(regs: &Arc<Mutex<Regs>>) -> Vec<Vec<u8>> {
    regs.lock()
        .unwrap()
        .wire
        .iter()
        .filter(|f| f.get(4..6) == Some(&[0x1A, 0x05]))
        .cloned()
        .collect()
}

/// ⭐ THE CONNECT READS EACH RADIO'S TIME-OUT TIMER AND MOD INPUT, byte for byte from its guide,
/// and writes nothing: every `1A 05` frame on the wire is a read. OFF gives the warning.
#[test]
fn the_connect_reads_the_time_out_timer_and_mod_input_and_writes_nothing() {
    for model in [
        IcomModel::Ic7610,
        IcomModel::Ic9700,
        IcomModel::Ic705,
        IcomModel::Ic905,
        IcomModel::Ic7760,
        IcomModel::Ic7300Mk2,
    ] {
        let addr = model.default_civ_addr();
        let tot = commands::time_out_timer_item(model).unwrap();
        let mi = commands::mod_input(model).unwrap();
        let b = bench(model);
        {
            let mut r = b.regs.lock().unwrap();
            r.menus.insert(tot, 0x00); // OFF
            r.menus.insert(mi.data_off, 0x00); // MIC
            r.menus.insert(mi.data[0], mi.lan);
        }
        let daemon = start(&target(&b, model)).expect("connects");
        let read = |item: [u8; 2]| vec![0xFE, 0xFE, addr, 0xE0, 0x1A, 0x05, item[0], item[1], 0xFD];
        assert_eq!(
            menu_frames(&b.regs),
            vec![read(tot), read(mi.data_off), read(mi.data[0])],
            "{model:?}: three reads, no write"
        );
        assert_eq!(
            daemon.findings(),
            &probe::Findings {
                tot: probe::Tot::Off,
                data_off_mod: Some(0x00),
                data_mod: Some(mi.lan),
            },
            "{model:?}"
        );
        let status = daemon.status();
        assert!(
            status.contains("is OFF. Transmitting over the network"),
            "{model:?}: {status}"
        );
        assert!(status.contains("DATA OFF MOD: MIC."), "{model:?}: {status}");
    }
}

/// A read the radio refuses is "unknown", and said so.
#[test]
fn a_refused_menu_read_is_unknown() {
    let model = IcomModel::Ic7760;
    let b = bench(model); // no menu items: the fake refuses every read
    let daemon = start(&target(&b, model)).expect("connects");
    assert_eq!(
        daemon.findings(),
        &probe::Findings {
            tot: probe::Tot::Unknown,
            data_off_mod: None,
            data_mod: None,
        }
    );
    assert!(
        daemon.status().contains("Time-Out Timer (CI-V): unknown"),
        "{}",
        daemon.status()
    );
}

/// The operator's data mode picks the DATA MOD item read: DATA2 on a radio with three.
#[test]
fn the_operators_data_mode_picks_the_data_mod_item() {
    let mi = commands::mod_input(IcomModel::Ic7760).unwrap();
    assert_eq!(probe::data_mod_item(&mi, 2), [0x01, 0x31]);
    assert_eq!(probe::data_mod_name(IcomModel::Ic7760, 2), "DATA2 MOD");
    let one = commands::mod_input(IcomModel::Ic9700).unwrap();
    assert_eq!(
        probe::data_mod_item(&one, 3),
        [0x01, 0x16],
        "one DATA MOD item"
    );
    assert_eq!(probe::data_mod_name(IcomModel::Ic9700, 3), "DATA MOD");
}

/// Over the network, an over must take its audio from the network: the refusal names the menu.
#[test]
fn an_over_whose_audio_is_not_the_network_is_named_with_the_menu_that_fixes_it() {
    let lan = probe::Findings {
        tot: probe::Tot::Minutes(3),
        data_off_mod: Some(0x00),
        data_mod: Some(0x09),
    };
    let m = IcomModel::Ic7760;
    assert_eq!(
        probe::network_tx_audio_refusal(m, probe::OverClass::Digital, 1, &lan),
        None
    );
    assert_eq!(
        probe::network_tx_audio_refusal(m, probe::OverClass::Cw, 1, &lan),
        None
    );
    assert_eq!(
        probe::network_tx_audio_refusal(m, probe::OverClass::Phone, 1, &lan).as_deref(),
        Some(
            "The radio takes this over's audio from MIC (DATA OFF MOD), not from the network: set \
             MENU › SET › Connectors › MOD Input › DATA OFF MOD = LAN"
        )
    );
    let unknown = probe::Findings {
        data_mod: None,
        ..lan
    };
    assert!(
        probe::network_tx_audio_refusal(m, probe::OverClass::Digital, 1, &unknown)
            .is_some_and(|why| why.contains("could not read the radio's DATA1 MOD"))
    );
}

// ── The radio's own answers ──────────────────────────────────────────────────────────────────

/// One advertised radio with another name connects, with a warning.
#[test]
fn a_radio_that_advertises_another_name_connects_with_a_warning() {
    let model = IcomModel::Ic7760;
    let b = bench_with(model, "IC-7610", |_| {});
    let daemon = start(&target(&b, model)).expect("connects anyway");
    assert!(
        daemon
            .status()
            .contains("The radio says it is an IC-7610, not an IC-7760."),
        "{}",
        daemon.status()
    );
}

/// With no password saved, nothing is sent and the status says so.
#[test]
fn with_no_password_nothing_is_sent_and_the_status_says_so() {
    let model = IcomModel::Ic7760;
    let b = bench(model);
    let none = |_: u32| -> Result<Option<String>, String> { Ok(None) };
    let e = IcomLanDaemon::start_with(&target(&b, model), &none, Arc::new(now_ms))
        .err()
        .expect("refused");
    assert_eq!(e.status, "No password is saved for this radio");
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(received_len(&b), 0, "not one datagram");
}

/// A refused login says so, gives nothing back to retry on, and waits for the operator; the
/// operator acting lets the next start try.
#[test]
fn a_refused_login_says_so_and_waits_for_the_operator() {
    let model = IcomModel::Ic7610;
    let b = bench_with(model, "IC-7610", |r| r.login_error = 0xFFFF_FFFF);
    let t = target(&b, model);
    let e = start(&t).err().expect("refused");
    assert_eq!(e.status, "The radio refused the network user or password");
    assert!(!e.status.contains(sim::PASSWORD) && !e.status.contains(sim::USER));
    assert!(registry::held(t.key()), "waits for the operator");
    let again = start(&t).err().expect("held");
    assert!(
        again.status.contains("Nexus will not reconnect by itself"),
        "{}",
        again.status
    );
    let logins = b.radio.with(|r| r.logins.len());
    registry::operator_acted(t.key());
    assert!(start(&t).is_err());
    assert_eq!(
        b.radio.with(|r| r.logins.len()),
        logins + 1,
        "the operator's retry logged in"
    );
}

/// The radio ending the session (another program took it) is a loss that waits for the operator.
#[test]
fn a_peer_disconnect_is_lost_and_waits_for_the_operator() {
    let model = IcomModel::Ic905;
    let b = bench(model);
    let t = target(&b, model);
    let daemon = start(&t).expect("connects");
    b.radio.with(|r| r.announce_disconnect = true);
    assert!(until(Duration::from_secs(3), || !daemon.is_alive()));
    assert_eq!(
        daemon.loss(),
        Some("Session lost: another program took the radio")
    );
    drop(daemon);
    assert!(registry::held(t.key()));
}

/// A radio that stops answering is lost within the liveness window, and the ladder retries it.
#[test]
fn a_silent_radio_is_lost_and_retried_on_the_ladder() {
    let model = IcomModel::Ic7300Mk2;
    let b = bench(model);
    let t = target(&b, model);
    let daemon = start(&t).expect("connects");
    let quiet = Instant::now();
    b.radio.with(|r| r.go_silent = true);
    assert!(until(Duration::from_secs(8), || !daemon.is_alive()));
    assert!(
        quiet.elapsed() >= Duration::from_millis(4_500),
        "{:?}",
        quiet.elapsed()
    );
    assert_eq!(
        daemon.loss(),
        Some("Session lost: the radio stopped answering")
    );
    drop(daemon);
    assert!(!registry::held(t.key()));
    let wait = registry::retry_in(t.key(), now_ms()).expect("a wait");
    assert!(wait <= 1_000, "the first step is 1 s: {wait}");
}

/// Nothing answering the first probe gives up after 3 s, not 10.
#[test]
fn no_answer_gives_up_after_three_seconds() {
    let model = IcomModel::Ic7760;
    let b = bench_with(model, "IC-7760", |r| r.go_silent = true);
    let begun = Instant::now();
    let e = start(&target(&b, model)).err().expect("no answer");
    let took = begun.elapsed();
    assert!(
        e.status.starts_with("No answer from 127.0.0.1"),
        "{}",
        e.status
    );
    assert!(
        took >= FIRST_ANSWER_TIMEOUT && took < FIRST_ANSWER_TIMEOUT + Duration::from_secs(2),
        "{took:?}"
    );
}

/// The model the daemon drives and the one the protocol core logs in as are the same six.
#[test]
fn the_six_network_models_map_to_the_protocol_cores() {
    for m in Model::ALL {
        let ours = crate::rigmodels::icom_lan_model(match m {
            Model::Ic7610 => 3078,
            Model::Ic9700 => 3081,
            Model::Ic705 => 3085,
            Model::Ic905 => 3090,
            Model::Ic7760 => 3092,
            Model::Ic7300Mk2 => 3094,
        })
        .expect("a network model");
        assert_eq!(net_model(ours), Some(m));
    }
    assert_eq!(net_model(IcomModel::Ic7300), None);
}
