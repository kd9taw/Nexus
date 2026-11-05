//! ⛔ A request from another program never overrides the operator's stop. A logger's WSJT-X UDP
//! Reply (GridTracker, JTAlert) works the station and arms TX only while no stop holds and no
//! watchdog trip stands; a FreeText with "send" never arms TX (WSJT-X's sets Tx5 and selects it,
//! `on_txb5_clicked`, `widgets/mainwindow.cpp:8797`); HaltTx always stops.
//!
//! The loop is the real `RadioLoop::step` on a VOX rig and a mock sound card, the CQ decoded from a
//! real FT8 capture; each datagram lands 14 s into an FT8 slot, too late for a first over to fit,
//! so nothing keys. The watchdog half is pinned in the engine (`stop_hold_tests`), which this
//! handler calls: a trip cannot be staged from outside the engine without minutes of wall clock.
//!
//! ⛔ From another computer, a request that shapes a transmission (Reply, FreeText, Location) is
//! refused unless the operator's setting lets it in. Which messages those are is pinned beside the
//! rule, `tempo_net::server::refused_from_other_computer`; here it is held on the real loop.
//! Those tests hold the remote-selection tests' lock: each decodes a CQ under the process-wide modem
//! mutex, which those tests need free.

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
    deliver_through(engine, &logger, server, datagrams);
}

/// The address "another computer" sends from here: the operator's UDP target, so the source check
/// admits it. (TEST-NET-1, and the sink's socket is on loopback: nothing leaves this box.)
const ANOTHER_COMPUTER: &str = "192.0.2.20";

/// [`deliver`], each datagram read as sent from [`ANOTHER_COMPUTER`], a target of the sink.
fn deliver_from_another_computer(engine: &Arc<Mutex<Engine>>, datagrams: &[Vec<u8>]) {
    let logger = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut server = WsjtxServer::new_multi(
        "127.0.0.1:0".parse().unwrap(),
        vec![
            logger.local_addr().unwrap(),
            format!("{ANOTHER_COMPUTER}:2237").parse().unwrap(),
        ],
    )
    .unwrap();
    server.read_sources_as(ANOTHER_COMPUTER.parse().unwrap());
    deliver_through(engine, &logger, server, datagrams);
}

fn deliver_through(
    engine: &Arc<Mutex<Engine>>,
    logger: &std::net::UdpSocket,
    server: WsjtxServer,
    datagrams: &[Vec<u8>],
) {
    let sinks = Sinks {
        wsjtx: Some(&server),
        psk: None,
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

/// A logger's Location (WSJT-X UDP message type 11).
fn location_datagram(location: &str) -> Vec<u8> {
    let mut w = head(tempo_net::wsjtx::msg_type::LOCATION);
    w.put_utf8(Some(location));
    w.into_bytes()
}

/// Lets requests in from other computers, as the operator's setting does.
fn accept_other_computers(engine: &Arc<Mutex<Engine>>) {
    let mut e = engine.lock().unwrap();
    let mut s = e.settings().clone();
    s.wsjtx_udp_accept_other_computers = true;
    e.apply_settings(s);
}

/// ⛔ A Reply from another computer, the setting off as it ships: no station worked, TX stays off.
#[test]
fn a_reply_from_another_computer_works_nothing_and_arms_nothing() {
    let _modem = super::remote_radio::selection::selection_test_lock();
    let engine = station();
    assert!(
        !engine
            .lock()
            .unwrap()
            .settings()
            .wsjtx_udp_accept_other_computers,
        "off unless the operator turns it on"
    );
    deliver_from_another_computer(&engine, &[reply_datagram("CQ W1AW FN31")]);
    assert_eq!(answered(&engine), (None, false), "(station worked, armed)");
}

/// ⛔ A FreeText from another computer, sent or not, the setting off and TX on: Tx 5 holds nothing
/// and nothing is queued.
#[test]
fn a_free_text_from_another_computer_leaves_the_text_alone() {
    let _modem = super::remote_radio::selection::selection_test_lock();
    for send in [true, false] {
        let engine = station();
        engine.lock().unwrap().set_tx_enabled_by_operator(true);
        deliver_from_another_computer(&engine, &[free_text_datagram("TNX 73", send)]);
        let taken = engine.lock().unwrap().snapshot().udp_free_text;
        assert_eq!(
            (taken.is_none(), now_sending(&engine)),
            (true, None),
            "(Tx 5 untouched, now sending) with send {send}"
        );
    }
}

/// ⛔ A Location from another computer, the setting off, leaves the grid the station sends.
#[test]
fn a_location_from_another_computer_leaves_the_grid_alone() {
    let _modem = super::remote_radio::selection::selection_test_lock();
    let engine = station();
    deliver_from_another_computer(&engine, &[location_datagram("JO22")]);
    assert_eq!(engine.lock().unwrap().settings().mygrid, "FN31");
}

/// Control: a Location from this computer moves the grid, as before.
#[test]
fn a_location_from_this_computer_moves_the_grid() {
    let _modem = super::remote_radio::selection::selection_test_lock();
    let engine = station();
    deliver(&engine, &[location_datagram("JO22")]);
    assert_eq!(engine.lock().unwrap().settings().mygrid, "JO22");
}

/// With the setting on, another computer's requests are taken as this computer's are.
#[test]
fn with_the_setting_on_another_computers_requests_are_taken() {
    let _modem = super::remote_radio::selection::selection_test_lock();
    let engine = station();
    accept_other_computers(&engine);
    deliver_from_another_computer(&engine, &[reply_datagram("CQ W1AW FN31")]);
    assert_eq!(
        answered(&engine),
        (Some("W1AW".to_string()), true),
        "(station worked, armed)"
    );

    let engine = station();
    accept_other_computers(&engine);
    engine.lock().unwrap().set_tx_enabled_by_operator(true);
    deliver_from_another_computer(&engine, &[free_text_datagram("TNX 73", true)]);
    assert_eq!(now_sending(&engine), Some("TNX 73".to_string()));

    let engine = station();
    accept_other_computers(&engine);
    deliver_from_another_computer(&engine, &[location_datagram("JO22")]);
    assert_eq!(engine.lock().unwrap().settings().mygrid, "JO22");
}

/// ⛔ The setting opens no guard: after the operator's Stop TX, another computer's Reply still
/// works nothing and arms nothing. (The watchdog's half is pinned in the engine: the same check.)
#[test]
fn with_the_setting_on_a_reply_still_cannot_lift_a_stop() {
    let _modem = super::remote_radio::selection::selection_test_lock();
    let engine = station();
    accept_other_computers(&engine);
    {
        let mut e = engine.lock().unwrap();
        e.set_tx_enabled_by_operator(true);
        e.halt_tx(); // Stop TX
    }
    deliver_from_another_computer(&engine, &[reply_datagram("CQ W1AW FN31")]);
    assert_eq!(answered(&engine), (None, false), "(station worked, armed)");
}

/// HaltTx only ever stops: another computer's stops TX with the setting off, as it always has.
#[test]
fn a_halt_tx_from_another_computer_still_stops() {
    let _modem = super::remote_radio::selection::selection_test_lock();
    let engine = station();
    engine.lock().unwrap().set_tx_enabled_by_operator(true);
    deliver_from_another_computer(&engine, &[halt_tx_datagram()]);
    assert!(!engine.lock().unwrap().tx_enabled(), "TX still on");
}

/// Clear and HighlightCallsign change only what the panes show: another computer's are taken with
/// the setting off, as they always have been.
#[test]
fn a_clear_from_another_computer_is_still_taken() {
    let engine = station();
    deliver_from_another_computer(&engine, &[clear_datagram(2)]);
    let s = engine.lock().unwrap().snapshot();
    assert_eq!((s.clear_tick, s.rx_freq_clear_tick), (1, 1));
}

/// A request refused from another computer says so in the diagnostic log, naming the request and
/// the address and where the setting is; a flood of them is one line; this computer's and an
/// accepted computer's make none. The scene runs in a child process of this test binary, because
/// the diagnostic log is one per process (`applog::init` is a one-shot) and would otherwise open
/// for every other test here.
#[test]
fn a_refused_request_is_one_line_in_the_diagnostic_log() {
    const CHILD: &str = "NEXUS_TEST_UDP_REFUSED_LOG";
    if let Some(path) = std::env::var_os(CHILD) {
        tempo_core::applog::init(path.into());
        // From this computer, then from an accepted one: neither is refused.
        deliver(&station(), &[reply_datagram("CQ W1AW FN31")]);
        let accepted = station();
        accept_other_computers(&accepted);
        deliver_from_another_computer(&accepted, &[reply_datagram("CQ W1AW FN31")]);
        tempo_core::applog::flush();
        // The refused flood.
        let flood = vec![reply_datagram("CQ W1AW FN31"); 5];
        deliver_from_another_computer(&station(), &flood);
        tempo_core::applog::flush();
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "nexus-udp-refused-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("nexus-diag.log");
    let me = format!(
        "{}::a_refused_request_is_one_line_in_the_diagnostic_log",
        module_path!().split_once("::").map_or("", |(_, rest)| rest)
    );
    let out = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([me.as_str(), "--exact", "--test-threads=1"])
        .env(CHILD, &path)
        .output()
        .expect("run the child");
    let shown = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        shown.contains("running 1 test"),
        "control: the child ran no test, so it proved nothing:\n{shown}"
    );
    assert!(out.status.success(), "the child failed:\n{shown}");
    let log = std::fs::read_to_string(&path).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        log.contains("diagnostic log opened"),
        "control: the child wrote this log:\n{log}"
    );
    let lines: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("another computer"))
        .collect();
    assert_eq!(lines.len(), 1, "one line for the flood:\n{log}");
    for word in [
        "Reply",
        ANOTHER_COMPUTER,
        "Settings",
        "Integrations & Feeds",
    ] {
        assert!(lines[0].contains(word), "{word} in: {}", lines[0]);
    }
}
