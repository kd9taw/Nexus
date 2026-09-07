//! The WHOLE Winlink internet path, over a real socket, with no radio and no live network.
//!
//! A fake CMS on `127.0.0.1:0` speaks the telnet pre-login and then replays the inbound side of
//! tempo-core's golden B2F transcript. `wl2k::run` drives a real `tempo_app::winlink::Driver`
//! into a temp mailbox, so this exercises every layer the batch built at once: the pre-login and
//! its leftover handover, the pump, the `ByteSession` seam, the B2F engine, LZHUF, the message
//! parser, the blob store, the journal and the ordered restore.
//!
//! ⚠️ **What it does NOT prove.** The transcript is a CONSTRUCTED one (its own header says so), so
//! this shows the parts agree with each other, not that any of them agrees with a real CMS. That
//! only a live capture settles.
//!
//! Nothing here keys a radio, opens an audio device, or touches the transmit path.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;

use tempo_core::winlink::{journal, ClientConfig};
use tempo_net::wl2k::{self, Login, Outcome, SessionState, CMS_TELNET_PASSWORD};

/// The inbound side of the golden transcript, concatenated.
fn fixture_inbound() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tempo-core/tests/fixtures/winlink/session1.trace"
    );
    let text = std::fs::read_to_string(path).expect("fixture is missing");
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if !line.starts_with('<') {
            continue;
        }
        let hex = line[1..].trim();
        out.extend(
            (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("fixture hex")),
        );
    }
    assert!(!out.is_empty(), "fixture parsed to no inbound bytes");
    out
}

#[test]
fn the_whole_internet_path_delivers_a_message_into_the_mailbox() {
    let root = std::env::temp_dir().join(format!("wl-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("mkdir");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let inbound = fixture_inbound();
    let server = std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().expect("accept");
        let mut got = Vec::new();
        let mut buf = [0u8; 4096];
        sock.write_all(b"[WL2K Central CMS]\r\nCallsign :").unwrap();
        let n = sock.read(&mut buf).unwrap();
        got.extend_from_slice(&buf[..n]);
        // The password prompt AND the CMS's whole opening in ONE write. That is the boundary the
        // pre-login exists to get right: the SID shares the prompt's read, and a pre-login that
        // swallowed it would fail the session as "FBB command before the peer's SID".
        let mut second = Vec::from(&b"\r\nPassword :"[..]);
        second.extend_from_slice(&inbound);
        sock.write_all(&second).unwrap();
        loop {
            match sock.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => got.extend_from_slice(&buf[..n]),
            }
        }
        got
    });

    let cfg = ClientConfig {
        callsign: "N0CALL".into(),
        // The transcript's stated parameter. Not a credential for anything.
        password: "NEXUSTEST".into(),
    };
    let mut driver =
        tempo_app::winlink::Driver::new(&cfg, &root, || 1_700_000_000).expect("driver");
    let stop = AtomicBool::new(false);
    let state = SessionState::default();
    let outcome = wl2k::run(
        &addr.ip().to_string(),
        addr.port(),
        Login::new("N0CALL", CMS_TELNET_PASSWORD),
        &mut driver,
        &stop,
        &state,
        None,
    );

    assert_eq!(outcome, Outcome::Complete, "traces: {:?}", driver.log());
    assert_eq!(
        driver.log().failed,
        None,
        "traces: {:?}",
        driver.log().traces
    );
    assert_eq!(
        driver.log().received.len(),
        1,
        "no message reached the mailbox: {:?}",
        driver.log().traces
    );

    let sent = String::from_utf8_lossy(&server.join().expect("server")).into_owned();
    assert!(sent.contains("N0CALL\r"), "callsign not sent: {sent:?}");
    assert!(
        sent.contains(&format!("{CMS_TELNET_PASSWORD}\r")),
        "telnet doorway token not sent: {sent:?}"
    );
    assert!(
        sent.contains(";FW: N0CALL\r"),
        "the B2F greeting never reached the wire: {sent:?}"
    );
    assert!(
        !sent.contains("NEXUSTEST"),
        "the account password reached the wire in the clear"
    );

    // The blob is on disk, under its MID, and the journal knows when it arrived.
    let mid = String::from_utf8_lossy(&driver.log().received[0]).into_owned();
    let blob = root.join("messages").join(format!("{mid}.b2f"));
    assert!(blob.is_file(), "no blob at {}", blob.display());
    let events = journal::open(&root).replay().expect("replay").events;
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        events[0],
        journal::Event::Received {
            mid: driver.log().received[0].clone(),
            at: 1_700_000_000
        }
    );

    // And a fresh restore agrees: one message, one accountable arrival, one unread badge.
    let restored = tempo_core::winlink::restore::restore(&root).expect("restore");
    assert_eq!(restored.index.entries.len(), 1);
    assert_eq!(
        restored.arrived.get(driver.log().received[0].as_slice()),
        Some(&1_700_000_000)
    );
    assert_eq!(restored.unread.len(), 1);
    assert_eq!(restored.repairs.journal_lines_skipped, 0);
    assert_eq!(restored.repairs.journal_rows_without_blob, 0);
    assert_eq!(restored.repairs.blobs_without_journal, 0);

    let _ = std::fs::remove_dir_all(&root);
}
