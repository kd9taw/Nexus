//! A logger's WSJT-X UDP Reply (GridTracker, JTAlert) that arrives while a decode is in flight.
//!
//! The radio loop handles the Reply itself, on its own thread with the Engine lock held, and the
//! radio loop is what drops PTT. Working the station the Reply names reset the decoder's IR-HARQ
//! buffers, and that reset waited for the decode to let go of the modem: the whole loop stood
//! still for the rest of the decode. WSJT-X's Reply (`replyToCQ`, `widgets/mainwindow.cpp:13279`)
//! waits for nothing; its decoder is another process.
//!
//! The loop is the real `RadioLoop::step` on a VOX rig and a mock sound card. The Reply lands 14 s
//! into an FT8 slot, too late for the first over to fit, so nothing keys.

use super::*;
use std::sync::mpsc;
use std::time::Instant;

/// A logger's Reply (WSJT-X UDP message type 4) to the decoded line `message`.
fn reply_datagram(message: &str) -> Vec<u8> {
    let mut w = tempo_net::qds::QdsWriter::new();
    w.put_u32(tempo_net::wsjtx::MAGIC)
        .put_u32(tempo_net::wsjtx::SCHEMA)
        .put_u32(tempo_net::wsjtx::msg_type::REPLY)
        .put_utf8(Some("GridTracker"))
        .put_u32(0)
        .put_i32(-5)
        .put_f64(0.1)
        .put_u32(1500)
        .put_utf8(Some("~"))
        .put_utf8(Some(message))
        .put_bool(false)
        .put_u8(0);
    w.into_bytes()
}

/// A 15 s FT8 capture of `msg` at 1500 Hz, at the engine's capture scale.
fn ft8_frame(msg: &str) -> Vec<f32> {
    use tempo_core::modes::{make_mode, ModeKind};
    let mode = make_mode(ModeKind::Ft8);
    let wave = mode.gen_wave(&mode.encode(msg), tempo_fast::SAMPLE_RATE, 1500.0);
    let mut frame = vec![0f32; mode.frame_samples()];
    for (i, &v) in wave.iter().enumerate() {
        if let Some(s) = frame.get_mut(6_000 + i) {
            *s = v * 0.0305;
        }
    }
    frame
}

#[test]
fn a_loggers_reply_during_a_decode_does_not_stall_the_radio_loop() {
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    {
        let mut e = engine.lock().unwrap();
        e.set_tier(Tier::Ft8);
        assert!(
            e.ingest(&ft8_frame("CQ W1AW FN31"), 3) >= 1,
            "premise: W1AW's CQ is decoded, so the Reply has a line to answer"
        );
    }
    let logger = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let server =
        WsjtxServer::new("127.0.0.1:0".parse().unwrap(), logger.local_addr().unwrap()).unwrap();
    let sinks = Sinks {
        wsjtx: Some(&server),
        psk: None,
        cfg_dial_hz: 14_074_000,
    };
    let mut backend = MockBackend::new();
    let mut rig = Rig::vox();
    let mut state = loop_state_for(&engine);
    let (mut ra, mut reopen) = (mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    let slot = ((now_unix_ms() + 30_000.0) / 15_000.0).ceil() * 15_000.0;
    let mut tick = |now: f64| {
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                now,
                &mut ra,
                &mut reopen,
                &mut station,
            )
            .unwrap();
    };
    // The loop's first ticks, with their boundary work, before anything is in flight.
    tick(slot + 13_900.0);
    tick(slot + 13_920.0);

    // A decode in flight holds the modem lock (the a7 reset guard is that lock) for its whole
    // length; this one holds it until the tick is done, or 600 ms.
    let (holding, held) = mpsc::channel();
    let (go, wait) = mpsc::channel::<()>();
    let decode = std::thread::spawn(move || {
        let _modem = loop {
            match tempo_core::modes::Ft8A7ResetGuard::try_acquire() {
                Some(modem) => break modem,
                None => std::thread::sleep(Duration::from_millis(1)),
            }
        };
        holding.send(()).unwrap();
        let _ = wait.recv_timeout(Duration::from_millis(600));
    });
    held.recv().unwrap();
    logger
        .send_to(
            &reply_datagram("CQ W1AW FN31"),
            server.local_addr().unwrap(),
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(20));

    let t0 = Instant::now();
    tick(slot + 13_940.0);
    let took = t0.elapsed();
    let _ = go.send(());
    decode.join().unwrap();

    let e = engine.lock().unwrap();
    assert_eq!(e.qso_dxcall(), Some("W1AW"), "the Reply started the QSO");
    assert!(e.tx_enabled(), "and armed TX, as WSJT-X's Reply does");
    assert!(
        took < Duration::from_millis(100),
        "the radio loop's tick took {took:?} handling the Reply while a decode ran"
    );
}
