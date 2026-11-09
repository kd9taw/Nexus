//! What Hamlib's TS-2000-family backends put on the wire for Nexus's FT8 DATA word, and what they
//! read back: the facts the radio loop's judgement (`RadioLoop::judge_data_word`,
//! `tempo_app::rig_modes`) stands on.
//!
//! The field report: SDR Console raised an error window at every FT8 key-down. Nexus commanded
//! `M PKTUSB` on the slot boundary just before `T 1`, and Hamlib's 2056 backend put it on the wire
//! as `MDD;`, which is not a TS-2000 mode. Hamlib's mode lists cannot tell that apart from a radio
//! that does take the word under another name, which is decided at open from the radio's own `ID`.
//! So Nexus judges by what happens on the link: a refusal, or a radio that reads back as plain USB.
//!
//! These put Hamlib's own `rigctld` between `Rig` and a fake TS-2000 that answers the reads Hamlib
//! makes and either refuses (`?;`, the Kenwood "bad command" answer) or ignores a mode outside 1-9.
//! Its `IF` answer is the 37 characters Hamlib expects; a wrong length makes Hamlib's VFO restore
//! after the set fail, and every set then reads as `RPRT -1`. They pin, per backend:
//! - 2056 SDR Console: `PKTUSB` goes out as `MDD;`, Hamlib (4.6.5) reports the set as taken
//!   whether the radio answers `?;` or nothing, and the radio reads back `USB`;
//! - 2014 TS-2000 whose radio answers `ID904` (SmartSDR's emulation): `MD9;` (DIGU), read back
//!   `PKTUSB`;
//! - 2040 piHPSDR: `MD9;`, read back `PKTUSB`.
//!
//! Skips without a `rigctld`, audibly, and never on GitHub Actions (`common::require_daemon`). A
//! Hamlib without one of these models skips that case audibly. `NEXUS_RIGCTLD` picks the binary.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tempo_audio::rig::Rig;

mod common;

/// How the fake answers a mode set outside the TS-2000's 1-9.
#[derive(Clone, Copy)]
enum BadMode {
    Refuse,
    Ignore,
}

/// A fake TS-2000 answering `ID` with `id`, on 20 m in USB, logging every command it gets.
fn fake_ts2000(listener: TcpListener, id: &'static str, bad: BadMode) -> Arc<Mutex<Vec<String>>> {
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
                        "ID" => format!("ID{id};"),
                        "FA" | "FB" => format!("{cmd}00014074000;"),
                        "MD" => format!("MD{mode};"),
                        // The TS-2000 IF answer: dial, step, RIT, flags, memory, TX, mode, VFO,
                        // scan, split, tone, tone number.
                        "IF" => format!("IF00014074000     +0000000000{mode}0000000;"),
                        "AI" => "AI0;".to_string(),
                        "PS" => "PS1;".to_string(),
                        "FR" | "FT" => format!("{cmd}0;"),
                        "TX" | "RX" => String::new(), // a set: no reply
                        c if c.len() == 3 && c.starts_with("MD") => {
                            let m = c.chars().nth(2).unwrap_or(' ');
                            if ('1'..='9').contains(&m) {
                                mode = m;
                                String::new()
                            } else {
                                match bad {
                                    BadMode::Refuse => "?;".to_string(),
                                    BadMode::Ignore => String::new(),
                                }
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

/// The skip notice, on the real stderr (libtest captures `eprintln!` from a passing test).
fn not_run(why: &str) {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "!! NOT RUN, and NOT A PASS: {why}");
}

/// What one DATA-word set did through `rigctld -m <model>` in front of the fake.
struct Outcome {
    /// What reached the radio for the set.
    wire: Vec<String>,
    /// Whether Hamlib answered the set as taken.
    taken: bool,
    /// What the radio reads back as afterwards, past Hamlib's own cache.
    reads: Option<String>,
}

/// Set `PKTUSB` through `bin -m model`, in front of a fake answering `ID` with `id`. `None` when
/// this Hamlib has no such model.
fn pktusb_through(bin: &str, model: u32, id: &'static str, bad: BadMode) -> Option<Outcome> {
    let known = Command::new(bin)
        .args(["-m", &model.to_string(), "-u"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !known {
        return None;
    }
    let radio = TcpListener::bind("127.0.0.1:0").unwrap();
    let radio_addr = radio.local_addr().unwrap().to_string();
    let log = fake_ts2000(radio, id, bad);
    let port = free_port();
    let _daemon = Daemon(
        Command::new(bin)
            .args(["-m", &model.to_string(), "-r", &radio_addr])
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
        "model {model}: the dial read through rigctld"
    );
    let from = log.lock().unwrap().len();
    let set = rig.set_mode("PKTUSB", -1);
    eprintln!("model {model}: set_mode answered {set:?}");
    let taken = set.is_ok();
    let wire = log.lock().unwrap()[from..].to_vec();
    // Past Hamlib's mode cache, as the radio loop's receive-time read always is.
    std::thread::sleep(Duration::from_millis(1500));
    let reads = rig.read_mode();
    eprintln!("model {model}, ID{id}: wire {wire:?}, taken {taken}, reads back {reads:?}");
    Some(Outcome { wire, taken, reads })
}

/// ⭐ SDR CONSOLE: `PKTUSB` reaches it as `MDD;`, and Hamlib reports the set as TAKEN whether the
/// radio answers it with `?;` or ignores it. So the refusal never shows up as an error. What does
/// show is the radio reading back as plain `USB`, which is what the loop's two-read judgement acts
/// on.
#[test]
fn sdr_console_gets_mdd_reported_as_taken_and_reads_back_usb() {
    let Some(bin) = common::require_daemon("rigctld", "NEXUS_RIGCTLD") else {
        return;
    };
    for bad in [BadMode::Refuse, BadMode::Ignore] {
        let Some(o) = pktusb_through(&bin, 2056, "019", bad) else {
            return not_run("this Hamlib has no model 2056 (SDR Console)");
        };
        assert!(o.wire.iter().any(|c| c == "MDD;"), "{:?}", o.wire);
        assert_eq!(
            o.reads.as_deref(),
            Some("USB"),
            "the radio reads back as the plain twin"
        );
        eprintln!("SDR Console, taken by Hamlib: {}", o.taken);
    }
}

/// CONTROLS: the same word on backends whose radio has a DATA mode Hamlib's list does not name.
/// A TS-2000 entry on SmartSDR's emulation (`ID904`) and piHPSDR: `MD9;` (DIGU), taken, and read
/// back as `PKTUSB`, so the loop keeps the DATA word.
#[test]
fn an_emulation_and_pihpsdr_take_pktusb_as_digu_and_read_it_back() {
    let Some(bin) = common::require_daemon("rigctld", "NEXUS_RIGCTLD") else {
        return;
    };
    for (model, id, what) in [
        (2014, "904", "TS-2000 entry on SmartSDR's emulation"),
        (2040, "019", "piHPSDR"),
    ] {
        let Some(o) = pktusb_through(&bin, model, id, BadMode::Refuse) else {
            not_run(&format!("this Hamlib has no model {model} ({what})"));
            continue;
        };
        assert!(o.wire.iter().any(|c| c == "MD9;"), "{what}: {:?}", o.wire);
        assert!(o.taken, "{what}: taken");
        assert_eq!(o.reads.as_deref(), Some("PKTUSB"), "{what}: read back");
    }
}
