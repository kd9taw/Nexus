//! ⛔ A request from another program never overrides the operator's stop. A logger's WSJT-X UDP
//! Reply (GridTracker, JTAlert) works the station and arms TX only while no stop holds and no
//! watchdog trip stands; a FreeText with "send" never arms TX (WSJT-X's sets Tx5 and selects it,
//! `on_txb5_clicked`, `widgets/mainwindow.cpp:8797`); HaltTx always stops.
//!
//! The loop is the real `RadioLoop::step` on a VOX rig and a mock sound card, the CQ decoded from a
//! real FT8 capture; each datagram lands 14 s into an FT8 slot, too late for a first over to fit,
//! so nothing keys. The watchdog half is pinned in the engine (`stop_hold_tests`), which this
//! handler calls: a trip cannot be staged from outside the engine without minutes of wall clock.

use super::decode_in_flight::{ft8_frame, reply_datagram};
use super::*;

/// The head every datagram shares: magic, schema, `kind`, the sender's id.
fn head(kind: u32) -> tempo_net::qds::QdsWriter {
    let mut w = tempo_net::qds::QdsWriter::new();
    w.put_u32(tempo_net::wsjtx::MAGIC)
        .put_u32(tempo_net::wsjtx::SCHEMA)
        .put_u32(kind)
        .put_utf8(Some("JTAlert"));
    w
}

/// A logger's FreeText (WSJT-X UDP message type 9).
fn free_text_datagram(text: &str, send: bool) -> Vec<u8> {
    let mut w = head(tempo_net::wsjtx::msg_type::FREE_TEXT);
    w.put_utf8(Some(text)).put_bool(send);
    w.into_bytes()
}

/// A logger's Clear (WSJT-X UDP message type 3) of `window`: 0 Band Activity, 1 Rx Frequency, 2 both.
fn clear_datagram(window: u8) -> Vec<u8> {
    let mut w = head(tempo_net::wsjtx::msg_type::CLEAR);
    w.put_u8(window);
    w.into_bytes()
}

/// A logger's HaltTx (WSJT-X UDP message type 8), auto-TX-only off: halt now.
pub(super) fn halt_tx_datagram() -> Vec<u8> {
    let mut w = head(tempo_net::wsjtx::msg_type::HALT_TX);
    w.put_bool(false);
    w.into_bytes()
}

/// A station on FT8 QSO mode that has decoded W1AW's CQ. "Double-click on call sets Tx enable" is
/// off: a logger's Reply arms TX whatever it says.
fn station() -> Arc<Mutex<Engine>> {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    let mut s = e.settings().clone();
    s.double_click_sets_tx = false;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.set_mode("qso-monitor").expect("FT8 QSO mode");
    assert!(
        e.ingest(&ft8_frame("CQ W1AW FN31"), 3) >= 1,
        "premise: W1AW's CQ is decoded"
    );
    Arc::new(Mutex::new(e))
}

/// Each datagram in turn reaches the real radio loop, one tick apiece.
fn deliver(engine: &Arc<Mutex<Engine>>, datagrams: &[Vec<u8>]) {
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
    let mut state = loop_state_for(engine);
    let (mut ra, mut reopen) = (mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    let slot = ((now_unix_ms() + 30_000.0) / 15_000.0).ceil() * 15_000.0;
    let mut now = slot + 13_900.0;
    let mut tick = |now: f64| {
        state
            .step(
                engine,
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
    tick(now);
    for d in datagrams {
        logger.send_to(d, server.local_addr().unwrap()).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        now += 20.0;
        tick(now);
    }
}

/// The station the engine works, and whether TX is armed.
fn answered(engine: &Arc<Mutex<Engine>>) -> (Option<String>, bool) {
    let e = engine.lock().unwrap();
    (e.qso_dxcall().map(str::to_string), e.tx_enabled())
}

/// Control: with nothing stopped, a logger's Reply works the station and arms TX, as before.
#[test]
fn a_loggers_reply_with_nothing_stopped_works_the_station_and_arms() {
    let engine = station();
    deliver(&engine, &[reply_datagram("CQ W1AW FN31")]);
    assert_eq!(answered(&engine), (Some("W1AW".to_string()), true));
}

/// ⛔ After the operator's Stop TX, a logger's Reply works nothing and arms nothing.
#[test]
fn a_loggers_reply_after_stop_tx_arms_nothing() {
    let engine = station();
    {
        let mut e = engine.lock().unwrap();
        e.set_tx_enabled_by_operator(true);
        e.halt_tx(); // Stop TX
    }
    deliver(&engine, &[reply_datagram("CQ W1AW FN31")]);
    assert_eq!(
        answered(&engine),
        (None, false),
        "(station worked, TX armed)"
    );
}

/// ⛔ A logger's FreeText with "send" set never arms TX.
#[test]
fn a_loggers_free_text_send_never_arms() {
    let engine = station();
    deliver(&engine, &[free_text_datagram("TNX 73", true)]);
    assert!(
        !engine.lock().unwrap().tx_enabled(),
        "the FreeText armed TX"
    );
}

/// ⛔ A logger's HaltTx stops TX, and its stop holds against the logger's own Reply after it.
#[test]
fn a_loggers_halt_tx_stops_and_holds_against_a_reply() {
    let engine = station();
    engine.lock().unwrap().set_tx_enabled_by_operator(true);
    deliver(&engine, &[halt_tx_datagram()]);
    let halted = engine.lock().unwrap().tx_enabled();
    deliver(&engine, &[reply_datagram("CQ W1AW FN31")]);
    assert_eq!(
        (halted, answered(&engine)),
        (false, (None, false)),
        "(armed after HaltTx, (station worked, armed after the Reply))"
    );
}

/// The "Now sending" readout.
fn now_sending(engine: &Arc<Mutex<Engine>>) -> Option<String> {
    engine.lock().unwrap().snapshot().qso.and_then(|q| q.tx_now)
}

/// ⭐ A logger's FreeText with "send", TX on: the text is the next over, as WSJT-X's click on Tx5
/// makes it (`widgets/mainwindow.cpp:692-709`). Nexus queued a chat broadcast that the FT8
/// screen's sequencer never sent.
#[test]
fn a_loggers_free_text_with_send_goes_out_as_tx5_while_tx_is_on() {
    let engine = station();
    engine.lock().unwrap().set_tx_enabled_by_operator(true);
    deliver(&engine, &[free_text_datagram("TNX 73", true)]);
    assert_eq!(
        (now_sending(&engine), engine.lock().unwrap().tx_enabled()),
        (Some("TNX 73".to_string()), true),
        "(now sending, TX on)"
    );
}

/// Without "send" the logger's text only fills Tx5: nothing is queued.
#[test]
fn a_loggers_free_text_without_send_fills_tx5_only() {
    let engine = station();
    engine.lock().unwrap().set_tx_enabled_by_operator(true);
    deliver(&engine, &[free_text_datagram("TNX 73", false)]);
    let taken = engine.lock().unwrap().snapshot().udp_free_text;
    assert_eq!(
        (taken.and_then(|t| t.fill), now_sending(&engine)),
        (Some("TNX 73".to_string()), None),
        "(Tx 5, now sending)"
    );
}

/// A logger's Clear erases the panes its window names, as WSJT-X's does (`++window`; `& 1` Band
/// Activity, `& 2` Rx Frequency; `widgets/mainwindow.cpp:667-674`): 0 Band Activity only, 1 Rx
/// Frequency only, 2 both, 3 neither. Each pane has its own tick in the snapshot, which the window
/// showing it erases on.
#[test]
fn a_loggers_clear_erases_only_the_panes_its_window_names() {
    let cleared = |window: u8| {
        let engine = Arc::new(Mutex::new(Engine::new("K2DEF", "FN31", 0)));
        deliver(&engine, &[clear_datagram(window)]);
        let s = engine.lock().unwrap().snapshot();
        (s.clear_tick, s.rx_freq_clear_tick)
    };
    assert_eq!(
        [0, 1, 2, 3].map(cleared),
        [(1, 0), (0, 1), (1, 1), (0, 0)],
        "(Band Activity, Rx Frequency) ticks after a Clear of window 0, 1, 2, 3"
    );
}
