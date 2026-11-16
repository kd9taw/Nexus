//! A decode WSJT-X prints with `?` is shown, sent to WSJT-X UDP consumers with the Decode
//! message's low-confidence flag set, and never uploaded to PSK Reporter: the maintainer's ruling
//! for the JTDX-style decoder's low-confidence decodes is to match WSJT-X.
//!
//! WSJT-X 3.0.2's rule for `?`: an a-priori decode (`nap != 0`) under 0.17 quality
//! (`lib/decoder.f90`, `ft8_decodedvar` :1641-1648, and the other decoders' callbacks: `if(nap.ne.0)`
//! ... `if(qual.lt.0.17) decoded0(37:37)='?'`). Its UDP Decode carries `low_confidence` = that `?`
//! (`MainWindow::postDecode`, `widgets/mainwindow.cpp:13399-13415`: `QChar {'?'} == decode.mid
//! (... + 36, 1)`, into `MessageClient::decode`, `Network/MessageClient.cpp:605-614`), and
//! `pskPost` skips it (`decodedtext.isLowConfidence ()`, mainwindow.cpp:7567;
//! `Decoder/decodedtext.cpp:131-134`).
//!
//! The JTDX-style decoder hands a `?` decode on with its a-priori type and quality and unmarked
//! (`crates/ft8/tests/ft8var.rs`, the `CQ N8DEK/R CN77` noise fixture); this is what the radio loop
//! then does with it.

use super::*;

/// The JTDX-style decoder's `CQ N8DEK/R CN77` from noise, as it hands it on: a-priori type 1,
/// quality under 0.17, not marked unchecked.
fn question_mark_decode() -> tempo_core::modes::Decode {
    tempo_core::modes::Decode {
        message: "CQ N8DEK/R CN77".into(),
        sync: 0.0,
        snr: -24,
        dt: -2.0,
        freq: 3506.0,
        nap: 1,
        qual: 0.10,
        rv: None,
        raw: None,
        unchecked: false,
        mode: Some(tempo_core::modes::ModeKind::Ft8),
    }
}

/// An FT8 station that has just heard `d`.
fn heard(d: tempo_core::modes::Decode) -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_tier(Tier::Ft8);
    assert_eq!(e.fold_decode_for_test(d, 1), 1, "premise: the decode folds");
    e
}

/// `(Message, low confidence)` of one Decode datagram, read in `NetworkMessage.hpp`'s order.
fn message_and_low_confidence(datagram: &[u8]) -> (String, bool) {
    let mut r = tempo_net::qds::QdsReader::new(datagram);
    assert_eq!(r.read_u32(), Some(tempo_net::wsjtx::MAGIC));
    r.read_u32().unwrap(); // schema
    assert_eq!(r.read_u32(), Some(tempo_net::wsjtx::msg_type::DECODE));
    r.read_utf8().unwrap(); // Id
    r.read_bool().unwrap(); // New
    r.read_u32().unwrap(); // Time
    r.read_i32().unwrap(); // snr
    r.read_f64().unwrap(); // Delta time
    r.read_u32().unwrap(); // Delta frequency
    r.read_utf8().unwrap(); // Mode
    let message = r.read_utf8().unwrap().unwrap_or_default();
    (message, r.read_bool().unwrap())
}

#[test]
fn a_question_mark_decode_is_shown_unmarked() {
    let e = heard(question_mark_decode());
    let rows = e.snapshot().recent_decodes;
    let row = rows
        .iter()
        .find(|r| r.message == "CQ N8DEK/R CN77")
        .expect("shown");
    assert!(!row.unchecked, "not marked `^`: WSJT-X shows it");
}

#[test]
fn a_question_mark_decode_reaches_udp_flagged_low_confidence() {
    let e = heard(question_mark_decode());
    let listener = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    listener
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let server = WsjtxServer::new(
        "127.0.0.1:0".parse().unwrap(),
        listener.local_addr().unwrap(),
    )
    .unwrap();
    let sinks = Sinks {
        wsjtx: Some(&server),
        psk: None,
    };
    let (mut spots, mut cache) = (Vec::new(), SpotCache::default());
    emit_rx_decodes(&sinks, &e, &mut spots, &mut cache, 1.7e12, 14_074_000);
    let mut buf = [0u8; 4096];
    let (n, _) = listener.recv_from(&mut buf).expect("a Decode datagram");
    assert_eq!(
        message_and_low_confidence(&buf[..n]),
        ("CQ N8DEK/R CN77".to_string(), true),
        "sent, with WSJT-X's low-confidence flag"
    );
}

#[test]
fn a_question_mark_decode_is_never_uploaded_to_psk_reporter() {
    let e = heard(question_mark_decode());
    let wire = e.wire_decodes();
    assert_eq!(wire.len(), 1, "premise: it is on the wire");
    let (mut spots, mut cache) = (Vec::new(), SpotCache::default());
    queue_spot(
        &mut spots,
        &mut cache,
        &wire[0],
        "FT8",
        14_074_000,
        1_700_000_000,
        ("K2DEF", "FN31"),
    );
    assert!(spots.is_empty(), "no PSK Reporter spot: {spots:?}");
    // Control: the same CQ at full quality is spotted.
    let e = heard(tempo_core::modes::Decode {
        qual: 0.9,
        ..question_mark_decode()
    });
    queue_spot(
        &mut spots,
        &mut cache,
        &e.wire_decodes()[0],
        "FT8",
        14_074_000,
        1_700_000_000,
        ("K2DEF", "FN31"),
    );
    assert_eq!(spots.len(), 1, "a confident CQ is spotted");
}
