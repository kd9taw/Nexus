//! A real serial port keeps its RTS/DTR holds on a Hamlib whose `rigctld -L` never exits.
//!
//! Hamlib 4.5.5 (Ubuntu 24.04's `libhamlib-utils`) prints the `-L` show-conf dump and then stays
//! up as a daemon. Nexus asks that dump which line holds this Hamlib accepts; bounded, a dump
//! that never exits used to answer nothing, and a real port then opened with NO hold, which on an
//! RTS- or DTR-keyed interface is a possible key-down at connect. The dump now comes from
//! `rigctl` instead, so the holds are passed.
//!
//! The port is a path to nothing: it reads as a real port (not a pseudo-terminal) and nothing is
//! opened. Run against 4.5.5 with `NEXUS_RIGCTLD=/path/to/4.5.5/rigctld` (its `rigctl` beside it,
//! `LD_LIBRARY_PATH` for its library); on any Hamlib the holds must be there.
#![cfg(unix)]

use std::time::{Duration, Instant};

use tempo_audio::rigctld_proc::{spawn_rigctld, ControlLines, KeyingFacts};

mod common;

/// The rigctld serving `-t <tcp>`, its whole command line, from the process list.
fn daemon_args(tcp: u16) -> Option<String> {
    let out = std::process::Command::new("ps")
        .args(["-axo", "args="])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|l| {
            l.contains("rigctld")
                && l.split_whitespace()
                    .collect::<Vec<_>>()
                    .windows(2)
                    .any(|w| w[0] == "-t" && w[1] == tcp.to_string())
        })
        .map(str::to_string)
}

#[test]
fn a_real_port_gets_the_hold_options_whatever_the_hamlib() {
    let Some(bin) = common::require_daemon("rigctld", "NEXUS_RIGCTLD") else {
        return;
    };
    if let Some(dir) = std::path::Path::new(&bin)
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
    {
        let path = format!(
            "{}:{}",
            dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        std::env::set_var("PATH", path);
    }
    let version = std::process::Command::new("rigctld")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    eprintln!("rigctld_holds_old_hamlib: {version}");
    let port = format!("/dev/nexus-test-no-such-port-{}", std::process::id());
    let tcp = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("free port");
    let started = Instant::now();
    let daemon = spawn_rigctld(
        3073, // IC-7300: both lines settable on every Hamlib here
        &port,
        115_200,
        tcp,
        false,
        KeyingFacts::default(),
        ControlLines::hold_low(),
    )
    .unwrap_or_else(|e| panic!("{version}: the launch failed: {e}"));
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "{version}: the launch took {:?}",
        started.elapsed()
    );
    let mut args = None;
    let until = Instant::now() + Duration::from_secs(5);
    while args.is_none() && Instant::now() < until {
        args = daemon_args(tcp);
        std::thread::sleep(Duration::from_millis(50));
    }
    let args = args.unwrap_or_else(|| panic!("{version}: no rigctld on -t {tcp}"));
    for hold in ["rts_state=OFF", "dtr_state=OFF"] {
        assert!(
            args.split_whitespace().any(|a| a == hold),
            "{version}: {hold} missing from the launch: {args}"
        );
    }
    drop(daemon);
}
