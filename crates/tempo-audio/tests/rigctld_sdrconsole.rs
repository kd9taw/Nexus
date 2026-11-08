//! FT8 through Hamlib's SDR Console backend (model 2056) sends SDR Console a mode it can take.
//!
//! The field report: SDR Console raised an error window at every FT8 key-down. Nexus commanded
//! `M PKTUSB` on the slot boundary just before `T 1`, and Hamlib's 2056 backend (a TS-2000
//! family one) put that on the wire as `MDD;`, which is not a TS-2000 mode. Its own caps dump
//! lists no PKT mode at all, and Nexus now reads that list (`tempo_app::rig_modes`) and commands
//! the plain sideband instead.
//!
//! This puts Hamlib's own `rigctld -m 2056` between `Rig` and a fake TS-2000, which answers the
//! reads Hamlib makes and answers `?;` (the Kenwood "bad command" reply) to a mode it does not
//! have. The mode list comes from the same binary, so the test is about that Hamlib, not a table.
//!
//! Skips without a `rigctld`, audibly, and never on GitHub Actions (`common::require_daemon`).
//! A Hamlib with no model 2056, or one whose 2056 lists a PKT mode, skips audibly too: the
//! premise is that backend's list. `NEXUS_RIGCTLD=/path/to/rigctld` picks the binary.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tempo_app::settings::{OperatingMode, Settings};
use tempo_audio::rig::Rig;

mod common;

const SDR_CONSOLE: u32 = 2056;

/// A fake TS-2000 on `listener`, on 20 m in USB, logging every command it gets.
fn fake_ts2000(listener: TcpListener) -> Arc<Mutex<Vec<String>>> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let log2 = Arc::clone(&log);
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { break };
            let mut mode = '2'; // USB
            let mut buf = Vec::new();
            let mut chunk = [0u8; 256];
            loop {
                let n = match conn.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                buf.extend_from_slice(&chunk[..n]);
                while let Some(end) = buf.iter().position(|&b| b == b';') {
                    let cmd = String::from_utf8_lossy(&buf[..end]).trim().to_uppercase();
                    buf.drain(..=end);
                    if cmd.is_empty() {
                        continue;
                    }
                    log2.lock().unwrap().push(format!("{cmd};"));
                    let reply = match cmd.as_str() {
                        "ID" => "ID019;".to_string(),
                        "FA" | "FB" => format!("{cmd}00014074000;"),
                        "MD" => format!("MD{mode};"),
                        // The TS-2000 IF answer: dial, step, RIT, flags, memory, TX, mode, VFO,
                        // scan, split, tone, tone number.
                        "IF" => format!("IF00014074000     +00000000000{mode}0000000;"),
                        "AI" => "AI0;".to_string(),
                        "PS" => "PS1;".to_string(),
                        "FR" | "FT" => format!("{cmd}0;"),
                        "TX" | "RX" => String::new(), // a set: no reply
                        // A mode set: 1-9 are TS-2000 modes; anything else is a bad command.
                        c if c.len() == 3 && c.starts_with("MD") => {
                            let m = c.chars().nth(2).unwrap_or(' ');
                            if ('1'..='9').contains(&m) {
                                mode = m;
                                String::new()
                            } else {
                                "?;".to_string()
                            }
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

/// A mode SET (`MD<x>;`) whose `x` is not one of the TS-2000's modes 1-9.
fn not_a_ts2000_mode(cmd: &str) -> bool {
    cmd.len() == 4 && cmd.starts_with("MD") && !(b'1'..=b'9').contains(&cmd.as_bytes()[2])
}

/// The skip notice, on the real stderr (libtest captures `eprintln!` from a passing test).
fn not_run(why: &str) {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "!! NOT RUN, and NOT A PASS: {why}");
}

/// ⭐ FT8 ON SDR CONSOLE: NEXUS'S MODE WORD REACHES THE RADIO AS A TS-2000 MODE, AND THE KEY
/// FOLLOWS. CONTROL: the DATA word Nexus used to send reaches it as `MDD;`, and is refused.
#[test]
fn ft8_through_hamlib_2056_sends_a_mode_sdr_console_takes_and_keys() {
    let Some(bin) = common::require_daemon("rigctld", "NEXUS_RIGCTLD") else {
        return;
    };
    let dump = Command::new(&bin)
        .args(["-m", &SDR_CONSOLE.to_string(), "-u"])
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    let Some(modes) = dump
        .as_deref()
        .and_then(tempo_audio::rigctld_proc::parse_mode_list)
    else {
        return not_run("this Hamlib has no model 2056 (SDR Console)");
    };
    if modes.iter().any(|m| m.starts_with("PKT")) {
        return not_run(&format!("this Hamlib's 2056 lists a DATA mode: {modes:?}"));
    }
    // What the radio loop does at the open (`learn_mode_caps`), and the word the policy gives.
    tempo_app::rig_modes::record(SDR_CONSOLE, Some(modes));
    let s = Settings {
        rig_model: SDR_CONSOLE,
        operating_mode: OperatingMode::Digital,
        dial_mhz: 14.074,
        ..Settings::default()
    };
    let md = s.rig_mode();
    assert_eq!(md, "USB", "the FT8 mode word for this backend");

    let radio = TcpListener::bind("127.0.0.1:0").unwrap();
    let radio_addr = radio.local_addr().unwrap().to_string();
    let log = fake_ts2000(radio);
    let port = free_port();
    let _daemon = Daemon(
        Command::new(&bin)
            .args(["-m", &SDR_CONSOLE.to_string(), "-r", &radio_addr])
            .args(["-T", "127.0.0.1", "-t", &port.to_string()])
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

    let from = log.lock().unwrap().len();
    rig.set_mode(&md, -1).expect("the mode set is taken");
    rig.ptt(true).expect("the key is taken");
    let _ = rig.ptt(false);
    let seen: Vec<String> = log.lock().unwrap()[from..].to_vec();
    eprintln!("Nexus's FT8 mode set and key, on the wire: {seen:?}");
    assert!(
        !seen.iter().any(|c| not_a_ts2000_mode(c)),
        "a mode SDR Console has no such thing as reached it: {seen:?}"
    );
    assert!(
        seen.iter().any(|c| c == "TX;"),
        "the key reached the radio: {seen:?}"
    );

    // CONTROL: the DATA word, as Nexus sent it before, reaches SDR Console as `MDD;`.
    let from = log.lock().unwrap().len();
    let _ = rig.set_mode("PKTUSB", -1);
    let seen: Vec<String> = log.lock().unwrap()[from..].to_vec();
    eprintln!("CONTROL, PKTUSB on the wire: {seen:?}");
    assert!(
        seen.iter().any(|c| c == "MDD;"),
        "control: Hamlib's 2056 puts PKTUSB on the wire as MDD;: {seen:?}"
    );
}

/// The list the radio loop records, read from the installed Hamlib: SDR Console's has no PKT
/// mode; the FT-991's does; an IC-9700 (a CI-V backend, whose printed list leaves out the USB-D it
/// takes) and NET rigctl (no list of its own) are unknown, so they are commanded as before.
#[test]
fn the_mode_list_nexus_records_is_hamlibs_own_and_never_an_icoms() {
    if common::require_daemon("rigctld", "NEXUS_RIGCTLD").is_none() {
        return;
    }
    let list = tempo_audio::rigctld_proc::hamlib_mode_list;
    match list(SDR_CONSOLE) {
        Some(modes) => assert!(
            !modes.iter().any(|m| m.starts_with("PKT")) && modes.iter().any(|m| m == "USB"),
            "SDR Console: {modes:?}"
        ),
        None => not_run("this Hamlib has no model 2056 (SDR Console)"),
    }
    let ft991 = list(1035).expect("the FT-991's list");
    assert!(ft991.iter().any(|m| m == "PKTUSB"), "FT-991: {ft991:?}");
    assert_eq!(
        list(3081),
        None,
        "IC-9700: a CI-V backend is never recorded"
    );
    assert_eq!(
        list(3073),
        None,
        "IC-7300: nor is this one, PKT listed or not"
    );
    assert_eq!(list(2), None, "NET rigctl has no list of its own");
}
