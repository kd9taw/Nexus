//! A raw Yaesu set must not hold a REAL rigctld, so the unkey behind it goes out at once.
//!
//! The in-process rigctlds elsewhere answer a raw set however the test says Hamlib does. This one
//! puts Hamlib's own `rigctld` between `Rig` and a fake radio, so the timing is Hamlib's.
//!
//! Hamlib's `w` (send_cmd) writes the string and then reads the radio's reply for the radio's
//! whole read timeout, and a Yaesu never answers a set. The FT-710's timeout is 1000 ms, so each
//! raw set held the port that long, and a `T 0` sent right after two of them (Stop TX on an
//! FT-710: the `KY00;` `KY10;` playback stops, then the over's unkey) reached the radio about 2 s
//! later. `Rig::send_raw_set` now sends `W <cmd> 0`, which Hamlib does not wait on.
//!
//! The fake radio is a TCP listener: Hamlib opens a serial rig's `-r host:port` as a network
//! port with the same timeout handling. It answers the reads Hamlib makes at open and on a key,
//! never a set, and logs when each command arrived.
//!
//! Skips without a `rigctld`, audibly, and never on GitHub Actions (`common::require_daemon`).
//! `NEXUS_RIGCTLD=/path/to/rigctld` picks the binary.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tempo_audio::rig::Rig;

mod common;

type Log = Arc<Mutex<Vec<(Instant, String)>>>;

/// A fake FT-710 (Yaesu ID 0800) on `listener`. Every command is logged with its arrival time.
fn fake_ft710(listener: TcpListener) -> Log {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let log2 = Arc::clone(&log);
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { break };
            let mut tx = '0';
            let mut buf = Vec::new();
            let mut chunk = [0u8; 256];
            loop {
                let n = match conn.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                buf.extend_from_slice(&chunk[..n]);
                while let Some(end) = buf.iter().position(|&b| b == b';') {
                    let cmd: String = String::from_utf8_lossy(&buf[..end]).trim().to_uppercase();
                    buf.drain(..=end);
                    if cmd.is_empty() {
                        continue;
                    }
                    log2.lock()
                        .unwrap()
                        .push((Instant::now(), format!("{cmd};")));
                    let reply = match cmd.as_str() {
                        "ID" => "ID0800;".to_string(),
                        "FA" | "FB" => format!("{cmd}014074000;"),
                        "MD0" => "MD0C;".to_string(),
                        "TX" => format!("TX{tx};"),
                        "IF" => "IF000014074000+000000C00000;".to_string(),
                        "AI" => "AI0;".to_string(),
                        "FT" => "FT0;".to_string(),
                        "ST" => "ST0;".to_string(),
                        "PS" => "PS1;".to_string(),
                        "VS" => "VS0;".to_string(),
                        c if c.len() == 3 && c.starts_with("TX") => {
                            tx = c.chars().nth(2).unwrap_or('0');
                            String::new() // a set: no reply, as on the radio
                        }
                        c if c.len() > 2
                            && c.chars().take(2).all(|ch| ch.is_ascii_alphabetic()) =>
                        {
                            String::new() // any other set: no reply
                        }
                        _ => "?;".to_string(),
                    };
                    if !reply.is_empty() && conn.write_all(reply.as_bytes()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    log
}

struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// ⭐ STOP TX ON AN FT-710: THE UNKEY BEHIND THE TWO RAW PLAYBACK STOPS REACHES THE RADIO AT
/// ONCE, AND IS ANSWERED WITHIN THE PTT DEADLINE.
#[test]
fn an_unkey_after_two_raw_yaesu_sets_reaches_the_radio_at_once() {
    let Some(bin) = common::require_daemon("rigctld", "NEXUS_RIGCTLD") else {
        return;
    };
    let radio = TcpListener::bind("127.0.0.1:0").unwrap();
    let radio_addr = radio.local_addr().unwrap().to_string();
    let log = fake_ft710(radio);
    let port = free_port();
    let _daemon = Daemon(
        Command::new(&bin)
            .args(["-m", "1049", "-r", &radio_addr, "-T", "127.0.0.1"])
            .args(["-t", &port.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn rigctld"),
    );
    let addr = format!("127.0.0.1:{port}");
    let ready = Instant::now() + Duration::from_secs(15);
    while TcpStream::connect(&addr).is_err() {
        assert!(Instant::now() < ready, "rigctld never listened on {addr}");
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut rig = Rig::rigctld(&addr);
    // POSITIVE CONTROL: Hamlib opened the fake radio and reads through it.
    let mut freq = None;
    while freq.is_none() && Instant::now() < ready {
        freq = rig.read_freq().ok();
    }
    assert_eq!(
        freq,
        Some(14_074_000),
        "control: the dial read through rigctld"
    );
    rig.ptt(true).expect("control: the key");

    let started = Instant::now();
    assert!(rig.send_raw_set("KY00;"), "rigctld took the first stop");
    assert!(rig.send_raw_set("KY10;"), "rigctld took the second stop");
    let unkey = rig.ptt(false);
    let answered = started.elapsed();
    // Wait for the unkey to reach the radio however late it is, so a late one fails on its
    // timing below rather than on the control.
    let landed = Instant::now() + Duration::from_secs(5);
    while Instant::now() < landed
        && !log
            .lock()
            .unwrap()
            .iter()
            .any(|(at, c)| *at >= started && c == "TX0;")
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    let seen: Vec<(Duration, String)> = log
        .lock()
        .unwrap()
        .iter()
        .filter(|(at, _)| *at >= started)
        .map(|(at, c)| (*at - started, c.clone()))
        .collect();
    let at = |cmd: &str| seen.iter().find(|(_, c)| c == cmd).map(|(t, _)| *t);
    // POSITIVE CONTROL: both raw stops reached the radio, in order, before the unkey.
    let (Some(ky0), Some(ky1), Some(tx0)) = (at("KY00;"), at("KY10;"), at("TX0;")) else {
        panic!("the radio must see KY00; KY10; TX0;: {seen:?}");
    };
    assert!(ky0 <= ky1 && ky1 <= tx0, "the order on the wire: {seen:?}");
    assert!(
        unkey.is_ok(),
        "the unkey must be answered within the PTT deadline ({answered:?}): {unkey:?}, {seen:?}"
    );
    assert!(
        tx0 < Duration::from_millis(500),
        "TX0; reached the radio {tx0:?} after the first raw stop: {seen:?}"
    );
}
