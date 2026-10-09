//! ⛔ THE ICOM NETWORK PASSWORD NEVER REACHES A LOG. A whole connect, CAT over the session and the
//! teardown, run against the simulated radio with both of Nexus's logs open: the app's diagnostic
//! log and the CI-V diagnostic file the operator is asked to send in. Neither may hold the
//! password, the network user, or the bytes the login encodes them as, in raw or hex form.
//!
//! Each search has its positive control: the same search over a copy seeded with the secret finds
//! it, and the logs are shown to hold this very session (its CI-V traffic, its connect line), so
//! an absence means something. Made-up credentials on loopback only.
//!
//! A test binary of its own because both logs are one per process (`tempo_core::applog::init` is a
//! one-shot, the CI-V tap is a global): opening them here can reach no other suite.
#![cfg(feature = "device")]

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tempo_audio::civ::commands::IcomModel;
use tempo_audio::civ::diag;
use tempo_audio::icomlan::{now_ms, IcomLanDaemon, Target};
use tempo_net::icom::sim::{SimRadio, SocketRadio, PASSWORD, USER};
use tempo_net::icom::wire;

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every form a credential could leak in: the text, its login encoding as raw bytes, and that
/// encoding as the CI-V log's hex.
fn forms(secret: &str) -> (Vec<u8>, Vec<u8>, String) {
    let encoded = wire::passcode(secret).expect("a sendable credential");
    let encoded = encoded[..secret.len()].to_vec();
    (secret.as_bytes().to_vec(), encoded.clone(), hex(&encoded))
}

fn holds(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// Which forms of `secret` the file at `path` holds.
fn leaks(path: &Path, secret: &str) -> Vec<&'static str> {
    let bytes = std::fs::read(path).expect("the log was written");
    let (text, encoded, encoded_hex) = forms(secret);
    let mut found = Vec::new();
    if holds(&bytes, &text) {
        found.push("the text");
    }
    if holds(&bytes, &encoded) {
        found.push("the encoded bytes");
    }
    if holds(&bytes, encoded_hex.as_bytes()) || holds(&bytes, encoded_hex.to_lowercase().as_bytes())
    {
        found.push("the encoded bytes in hex");
    }
    found
}

#[test]
fn the_password_and_the_user_reach_neither_log() {
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("icomlan-leak-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let app_log = dir.join("nexus-diag.log");
    let civ_log = dir.join("nexus-civ-diagnostic.log");
    tempo_core::applog::init(app_log.clone());
    assert_eq!(
        tempo_core::applog::path(),
        Some(app_log.as_path()),
        "precondition: the log this test reads is the one the daemon writes"
    );
    diag::start(&civ_log).expect("the CI-V log opens");

    // A connect, some CAT, and the teardown.
    let radio = SocketRadio::start(SimRadio::new()); // an IC-7610, as real hardware reports
    let target = Target {
        host: Ipv4Addr::LOCALHOST,
        control_port: radio.control_port,
        model: IcomModel::Ic7610,
        user: USER.to_string(),
        profile_id: 1,
        rigctld_port: 0,
        data_mode: 1,
    };
    let creds = |_: u32| -> Result<Option<String>, String> { Ok(Some(PASSWORD.to_string())) };
    let daemon = IcomLanDaemon::start_with(&target, &creds, Arc::new(now_ms)).expect("connects");
    let mut c = TcpStream::connect(daemon.local_addr()).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut rd = BufReader::new(c.try_clone().unwrap());
    c.write_all(b"f\n").unwrap();
    let mut line = String::new();
    rd.read_line(&mut line).unwrap();
    drop(daemon);
    std::thread::sleep(Duration::from_millis(200));
    tempo_core::applog::flush();
    diag::stop();

    // The logs hold this session, so an absence below is evidence.
    let civ = std::fs::read_to_string(&civ_log).unwrap();
    assert!(
        civ.contains("FE FE 98 E0 1A 05 00 31 FD") && civ.contains("FE FE 98 E0 1C 00 00 FD"),
        "the CI-V log recorded this session: its time-out timer read and its key-up: {civ}"
    );
    let app = std::fs::read_to_string(&app_log).unwrap();
    assert!(
        app.contains("Icom network: connected to IC-7610") && app.contains("network user: set"),
        "the app log recorded the connect: {app}"
    );

    for (log, path) in [("the app log", &app_log), ("the CI-V log", &civ_log)] {
        for (what, secret) in [("the password", PASSWORD), ("the network user", USER)] {
            assert_eq!(leaks(path, secret), Vec::<&str>::new(), "{what} in {log}");
        }
    }

    // The positive controls: the same search over a copy seeded with each form finds it.
    let (text, encoded, encoded_hex) = forms(PASSWORD);
    for (form, seed) in [
        ("the text", text),
        ("the encoded bytes", encoded),
        ("the encoded bytes in hex", encoded_hex.into_bytes()),
    ] {
        let seeded = dir.join("seeded.log");
        let mut bytes = std::fs::read(&civ_log).unwrap();
        bytes.extend_from_slice(&seed);
        std::fs::write(&seeded, bytes).unwrap();
        assert!(
            leaks(&seeded, PASSWORD).contains(&form),
            "the control finds {form}"
        );
    }
}

/// What the daemon says about a refused login names neither credential.
#[test]
fn a_refused_login_names_neither_credential() {
    let mut sim = SimRadio::new();
    sim.login_error = 0xFFFF_FFFF;
    let radio = SocketRadio::start(sim);
    let target = Target {
        host: Ipv4Addr::LOCALHOST,
        control_port: radio.control_port,
        model: IcomModel::Ic7610,
        user: USER.to_string(),
        profile_id: 2,
        rigctld_port: 0,
        data_mode: 1,
    };
    let creds = |_: u32| -> Result<Option<String>, String> { Ok(Some(PASSWORD.to_string())) };
    let e = IcomLanDaemon::start_with(&target, &creds, Arc::new(now_ms))
        .err()
        .expect("refused");
    let shown = format!("{e} {e:?} {target:?}");
    assert_eq!(e.status, "The radio refused the network user or password");
    assert!(!shown.contains(PASSWORD), "{shown}");
    // The user is in `Target`, which is the profile's own and not secret, but no message says it.
    assert!(!format!("{e} {e:?}").contains(USER));
}
