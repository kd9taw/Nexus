//! The WSJT-X UDP API while the JTTY section is the operator's, as WSJT-X itself speaks it in
//! JTTY: the Status names the mode `JTTY` (its mode and TX mode are both `m_mode`,
//! `widgets/mainwindow.cpp` `statusUpdate`) with the JTTY Rx frequency as its RX offset and the
//! JTTY Tx frequency as its TX offset (WSJT-X keeps its two pairs of spin boxes in step,
//! `on_RxFreqSpinBox_2_valueChanged`, `on_TxFreqSpinBox_2_valueChanged`), and no Decode
//! datagram goes out: WSJT-X's JTTY receive path never calls its message client, and its FT
//! decoder is not running in JTTY.
//!
//! The Status is read off the real `RadioLoop::step` crossing a slot boundary with a WSJT-X sink
//! pointed at a socket of the test's own (`contest_status.rs`'s rig). The Decode half calls the
//! boundary's own emitter with a real FT8 decode in the engine.

use super::*;

/// What a Status datagram says about the mode: (mode, TX mode, RX offset, TX offset).
fn status_mode(listener: &std::net::UdpSocket) -> (String, String, u32, u32) {
    let mut buf = [0u8; 4096];
    loop {
        let (n, _) = listener.recv_from(&mut buf).expect("a Status datagram");
        let mut r = tempo_net::qds::QdsReader::new(&buf[..n]);
        let (_magic, _schema, kind) = (r.read_u32(), r.read_u32(), r.read_u32());
        if kind != Some(tempo_net::wsjtx::msg_type::STATUS) {
            continue;
        }
        r.read_utf8(); // id
        r.read_u64(); // dial
        let mode = r.read_utf8().flatten().expect("mode");
        r.read_utf8(); // DX call
        r.read_utf8(); // report
        let tx_mode = r.read_utf8().flatten().expect("TX mode");
        for _ in 0..3 {
            r.read_bool(); // TX enabled, transmitting, decoding
        }
        let rx_df = r.read_u32().expect("RX offset");
        let tx_df = r.read_u32().expect("TX offset");
        return (mode, tx_mode, rx_df, tx_df);
    }
}

/// The Status a station in `section` sends once the loop has crossed a slot boundary, with the
/// JTTY Rx and Tx frequencies set to 1234 and 1777 Hz and the FT offsets at the engine's own.
fn status_in(section: &str) -> (String, String, u32, u32) {
    let listener = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    listener
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let server = WsjtxServer::new(
        "127.0.0.1:0".parse().unwrap(),
        listener.local_addr().unwrap(),
    )
    .unwrap();
    let engine = Arc::new(Mutex::new(Engine::new("K1ABC", "FN42", 0)));
    {
        let mut eng = engine.lock().unwrap();
        eng.set_operating_mode(section, false);
        eng.set_jtty_rx(1234.0, 50.0).unwrap();
        eng.set_jtty_tx(1777.0).unwrap();
    }
    let mut backend = MockBackend::new();
    let mut rig = Rig::vox();
    let mut state = loop_state();
    let sinks = Sinks {
        wsjtx: Some(&server),
        psk: None,
        cfg_dial_hz: 14_090_000,
    };
    let (mut ra, mut rr) = (mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    for now in [0.0, 16_000.0] {
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                now,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
    }
    status_mode(&listener)
}

/// ⭐ In the JTTY section the Status says JTTY, with the JTTY Rx frequency and the JTTY Tx
/// frequency — the offset JTTY actually transmits on, not the FT one; in PSK, the same Keyboard
/// section with the other modem, it says what it always said.
#[test]
fn the_status_names_jtty_while_the_jtty_section_is_the_operators() {
    assert_eq!(
        status_in("jtty"),
        ("JTTY".into(), "JTTY".into(), 1234, 1777)
    );
    let (mode, tx_mode, rx_df, tx_df) = status_in("keyboard");
    assert_eq!(
        (mode.as_str(), tx_mode.as_str()),
        ("FT8", "FT8"),
        "control: PSK's Status names the tier, as before"
    );
    assert_ne!(rx_df, 1234, "control: and the FT RX offset");
    assert_ne!(tx_df, 1777, "control: and the FT TX offset");
}

/// Whether the boundary's emitter sends a Decode datagram for the FT8 line the engine decoded,
/// with the station in `section`.
fn decode_sent_in(section: &str) -> bool {
    let logger = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    logger
        .set_read_timeout(Some(std::time::Duration::from_millis(400)))
        .unwrap();
    let server =
        WsjtxServer::new("127.0.0.1:0".parse().unwrap(), logger.local_addr().unwrap()).unwrap();
    let sinks = Sinks {
        wsjtx: Some(&server),
        psk: None,
        cfg_dial_hz: 14_074_000,
    };
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_tier(Tier::Ft8);
    assert!(
        e.ingest(&super::decode_in_flight::ft8_frame("CQ W1AW FN31"), 3) >= 1,
        "premise: W1AW's CQ is decoded"
    );
    assert!(!e.wire_decodes().is_empty(), "premise: a line for the wire");
    e.set_operating_mode(section, false);
    emit_rx_decodes(
        &sinks,
        &e,
        &mut Vec::new(),
        &mut tempo_net::pskreporter::SpotCache::default(),
        1_700_000_000_000.0,
        14_074_000,
    );
    let mut buf = [0u8; 4096];
    while let Ok((n, _)) = logger.recv_from(&mut buf) {
        let mut r = tempo_net::qds::QdsReader::new(&buf[..n]);
        let (_magic, _schema, kind) = (r.read_u32(), r.read_u32(), r.read_u32());
        if kind == Some(tempo_net::wsjtx::msg_type::DECODE) {
            return true;
        }
    }
    false
}

/// ⭐ No Decode datagram goes out in the JTTY section, even for a line the FT decoder found; in
/// PSK the same line goes out, as it always has.
#[test]
fn no_decode_datagram_goes_out_in_the_jtty_section() {
    assert!(!decode_sent_in("jtty"));
    assert!(decode_sent_in("keyboard"), "control: PSK sends the line");
}
