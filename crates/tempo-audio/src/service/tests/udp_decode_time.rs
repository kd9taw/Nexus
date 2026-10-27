//! The WSJT-X UDP Decode message's `Time` is the START of the period the decode was heard in,
//! whenever the datagram goes out — a late full-pass decode included.
//!
//! WSJT-X 3.0.2 stamps every pass of a period with the period's start (`dec_data.params.nutc`,
//! `widgets/mainwindow.cpp:5397-5404`, truncated to the T/R period) and sends the decode line's
//! `hhmmss` as the Decode's `Time` (`postDecode`, :13409-13410; `Network/NetworkMessage.hpp`:
//! Decode, `Time QTime`), for a Replay too. JTAlert, GridTracker and loggers group decodes by it,
//! so the arrival time put a decode of the :15 period that landed at :31 under the :30 period.
//! ALL.TXT already stamps the period start; the datagram now says the same.

use super::decode_in_flight::ft8_frame;
use super::*;

/// 2023-11-14 22:13:15 UTC: the start of an FT8 period.
const PERIOD_START_S: u64 = 1_699_999_995;
/// The engine's index for that period's decodes: the audio's slot + 1, for both passes.
const SLOT: u64 = PERIOD_START_S / 15 + 1;
/// 22:13:15 as a Decode's `Time`: milliseconds since midnight UTC.
const PERIOD_START_MS_OF_DAY: u32 = (22 * 3600 + 13 * 60 + 15) * 1000;

/// W1AW's CQ heard in the 22:13:15 period, on an FT8 station.
fn heard_in_the_period() -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_tier(Tier::Ft8);
    assert!(
        e.ingest(&ft8_frame("CQ W1AW FN31"), SLOT) >= 1,
        "premise: W1AW's CQ is decoded"
    );
    e
}

/// `(New, Time, Message)` of one Decode datagram, read in `NetworkMessage.hpp`'s order.
fn decode_on_the_wire(datagram: &[u8]) -> (bool, u32, String) {
    let mut r = tempo_net::qds::QdsReader::new(datagram);
    assert_eq!(r.read_u32(), Some(tempo_net::wsjtx::MAGIC));
    r.read_u32().unwrap(); // schema
    assert_eq!(r.read_u32(), Some(tempo_net::wsjtx::msg_type::DECODE));
    r.read_utf8().unwrap(); // Id
    let new = r.read_bool().unwrap();
    let time = r.read_u32().unwrap(); // QTime: ms since midnight
    r.read_i32().unwrap(); // snr
    r.read_f64().unwrap(); // Delta time
    r.read_u32().unwrap(); // Delta frequency
    r.read_utf8().unwrap(); // Mode
    let message = r.read_utf8().unwrap().unwrap_or_default();
    (new, time, message)
}

/// A UDP consumer, and Nexus's WSJT-X server sending to it.
fn consumer() -> (std::net::UdpSocket, WsjtxServer) {
    let listener = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    listener
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let server = WsjtxServer::new(
        "127.0.0.1:0".parse().unwrap(),
        listener.local_addr().unwrap(),
    )
    .unwrap();
    (listener, server)
}

/// The next datagram the consumer receives.
fn received(listener: &std::net::UdpSocket) -> Vec<u8> {
    let mut buf = [0u8; 4096];
    let (n, _) = listener.recv_from(&mut buf).expect("a datagram");
    buf[..n].to_vec()
}

/// A decode of the 22:13:15 period goes out stamped 22:13:15 whether it is sent at :27 (the early
/// pass), at :31 or at :34 (a full pass that ended in the next period), and New, as WSJT-X sends
/// a decode as it completes (`postDecode (true, …)`, mainwindow.cpp 3416, 5676, 7383).
#[test]
fn a_decode_goes_out_stamped_with_its_period_start_whenever_it_is_sent() {
    let e = heard_in_the_period();
    let (listener, server) = consumer();
    let sinks = Sinks {
        wsjtx: Some(&server),
        psk: None,
        cfg_dial_hz: 14_074_000,
    };
    let (mut spots, mut cache) = (Vec::new(), SpotCache::default());
    for sent_at in [27.0, 31.0, 34.0] {
        let now = (PERIOD_START_S as f64 - 15.0 + sent_at) * 1000.0;
        emit_rx_decodes(&sinks, &e, &mut spots, &mut cache, now, 14_074_000);
        let (new, time, message) = decode_on_the_wire(&received(&listener));
        assert_eq!(message, "CQ W1AW FN31");
        assert_eq!(
            (new, time),
            (true, PERIOD_START_MS_OF_DAY),
            "sent at :{sent_at}: the Decode's New and Time"
        );
    }
}

/// Control: ALL.TXT stamps that decode's line with the same second the datagram carries.
#[test]
fn all_txt_stamps_the_same_period_start() {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    let mut s = e.settings().clone();
    s.write_all_txt = true;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    assert!(e.ingest(&ft8_frame("CQ W1AW FN31"), SLOT) >= 1);
    let lines = e.take_all_txt_pending();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("231114_221315") && lines[0].ends_with("CQ W1AW FN31"),
        "{}",
        lines[0]
    );
}

/// A logger's Replay gets the period's decodes back with the period's start, not the moment it
/// asked, and not New: WSJT-X replays each old decode "with the 'New' field set to false"
/// (`Network/NetworkMessage.hpp` 221-226; `replayDecodes` → `postDecode (false, …)`,
/// mainwindow.cpp 13374-13396).
#[test]
fn a_replay_sends_the_period_start() {
    let engine = Arc::new(Mutex::new(heard_in_the_period()));
    let (listener, server) = consumer();
    let sinks = Sinks {
        wsjtx: Some(&server),
        psk: None,
        cfg_dial_hz: 14_074_000,
    };
    let mut replay = tempo_net::qds::QdsWriter::new();
    replay
        .put_u32(tempo_net::wsjtx::MAGIC)
        .put_u32(tempo_net::wsjtx::SCHEMA)
        .put_u32(tempo_net::wsjtx::msg_type::REPLAY)
        .put_utf8(Some("JTAlert"));
    listener
        .send_to(&replay.into_bytes(), server.local_addr().unwrap())
        .unwrap();
    std::thread::sleep(Duration::from_millis(50));

    let mut backend = MockBackend::new();
    let mut rig = Rig::vox();
    let mut state = loop_state_for(&engine);
    let (mut ra, mut reopen) = (mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    // Asked at 22:13:41, in the period after the one heard.
    let now = (PERIOD_START_S as f64 + 26.0) * 1000.0;
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
    // The loop also sends a Status; read until the replayed Decode.
    let decode = std::iter::repeat_with(|| received(&listener))
        .take(16)
        .find(|d| d.get(8..12) == Some(&tempo_net::wsjtx::msg_type::DECODE.to_be_bytes()[..]))
        .expect("the replayed Decode");
    let (new, time, message) = decode_on_the_wire(&decode);
    assert_eq!(message, "CQ W1AW FN31");
    assert_eq!(
        (new, time),
        (false, PERIOD_START_MS_OF_DAY),
        "the replayed Decode's New and Time"
    );
}

/// A Replay datagram from a logger.
fn replay_datagram() -> Vec<u8> {
    let mut w = tempo_net::qds::QdsWriter::new();
    w.put_u32(tempo_net::wsjtx::MAGIC)
        .put_u32(tempo_net::wsjtx::SCHEMA)
        .put_u32(tempo_net::wsjtx::msg_type::REPLAY)
        .put_utf8(Some("JTAlert"));
    w.into_bytes()
}

/// A logger's Clear of `window` (0 Band Activity, 1 Rx Frequency, 2 both).
fn clear_datagram(window: u8) -> Vec<u8> {
    let mut w = tempo_net::qds::QdsWriter::new();
    w.put_u32(tempo_net::wsjtx::MAGIC)
        .put_u32(tempo_net::wsjtx::SCHEMA)
        .put_u32(tempo_net::wsjtx::msg_type::CLEAR)
        .put_utf8(Some("JTAlert"))
        .put_u8(window);
    w.into_bytes()
}

/// The three FT8 periods heard, oldest first: each one's decode, and its start as a Decode's
/// `Time`.
const THREE_PERIODS: [(u64, &str, u32); 3] = [
    (SLOT - 2, "CQ W1AW FN31", PERIOD_START_MS_OF_DAY - 30_000),
    (SLOT - 1, "CQ K1ABC FN42", PERIOD_START_MS_OF_DAY - 15_000),
    (SLOT, "CQ W9XYZ EN37", PERIOD_START_MS_OF_DAY),
];

/// An FT8 station that heard [`THREE_PERIODS`], each folded as the live decode folds it (no
/// decoder runs: it would hold the process-wide modem lock other tests wait on).
fn heard_over_three_periods() -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_tier(Tier::Ft8);
    for (slot, msg, _) in THREE_PERIODS {
        assert_eq!(e.fold_heard_for_test(msg, slot), 1, "premise: {msg} folds");
    }
    e
}

/// What a replay of [`THREE_PERIODS`] sends: every decode, oldest first, not New.
fn three_periods_replayed() -> Vec<(bool, u32, String)> {
    THREE_PERIODS
        .iter()
        .map(|&(_, msg, time)| (false, time, msg.to_string()))
        .collect()
}

/// `ticks` of datagrams, each tick's reaching the real radio loop before one step of it at
/// 22:13:41; every Decode the consumer receives, as `(New, Time, Message)`.
fn decodes_after(engine: Engine, ticks: &[&[Vec<u8>]]) -> Vec<(bool, u32, String)> {
    let engine = Arc::new(Mutex::new(engine));
    let (listener, server) = consumer();
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
    let mut now = (PERIOD_START_S as f64 + 26.0) * 1000.0;
    for datagrams in ticks {
        for d in *datagrams {
            listener.send_to(d, server.local_addr().unwrap()).unwrap();
        }
        std::thread::sleep(Duration::from_millis(50));
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
        now += 20.0;
    }
    listener
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    let mut buf = [0u8; 4096];
    std::iter::from_fn(|| {
        listener
            .recv_from(&mut buf)
            .ok()
            .map(|(n, _)| buf[..n].to_vec())
    })
    .filter(|d| d.get(8..12) == Some(&tempo_net::wsjtx::msg_type::DECODE.to_be_bytes()[..]))
    .map(|d| decode_on_the_wire(&d))
    .collect()
}

/// ⭐ A Replay re-sends every decode Band Activity holds, each period's decodes stamped with that
/// period's start, oldest first, none New. WSJT-X walks its whole Band Activity window
/// (`replayDecodes`, `widgets/mainwindow.cpp:13374-13396`); Nexus sent the last period only.
#[test]
fn a_replay_sends_every_decode_band_activity_holds() {
    assert_eq!(
        decodes_after(heard_over_three_periods(), &[&[replay_datagram()]]),
        three_periods_replayed(),
        "the replay of three periods: (New, Time, Message)"
    );
}

/// A burst of Replays in one tick is answered once (each re-sends all of Band Activity with the
/// engine lock held), and so is one in each of two ticks: a logger is answered at most once a
/// second.
#[test]
fn a_burst_of_replays_in_one_tick_is_answered_once() {
    let once = decodes_after(
        heard_over_three_periods(),
        &[&[replay_datagram(), replay_datagram(), replay_datagram()]],
    );
    let twice = decodes_after(
        heard_over_three_periods(),
        &[&[replay_datagram()], &[replay_datagram()]],
    );
    assert_eq!(
        (once.len(), twice.len()),
        (3, 3),
        "Decodes sent for (three Replays in one tick, one in each of two ticks)"
    );
}

/// A logger's Clear of Band Activity (window 0, or 2 for both) leaves a Replay nothing to send; a
/// Clear of Rx Frequency alone (1) leaves Band Activity, as WSJT-X's handler reads the window
/// (`++window; if (window & 1)` erases Band Activity, `widgets/mainwindow.cpp:667-674`).
#[test]
fn a_loggers_clear_of_band_activity_leaves_a_replay_nothing() {
    let after_clear = |window: u8| {
        decodes_after(
            heard_over_three_periods(),
            &[&[clear_datagram(window), replay_datagram()]],
        )
    };
    assert_eq!(after_clear(0), Vec::new(), "after a Clear of Band Activity");
    assert_eq!(after_clear(2), Vec::new(), "after a Clear of both windows");
    assert_eq!(
        after_clear(1),
        three_periods_replayed(),
        "after a Clear of Rx Frequency"
    );
}

/// The operator's Erase of Band Activity leaves a Replay nothing to send; an Erase of Rx Frequency
/// leaves Band Activity whole.
#[test]
fn the_operators_erase_of_band_activity_leaves_a_replay_nothing() {
    let after_erase = |window: u8| {
        let mut e = heard_over_three_periods();
        e.notify_erase(window);
        decodes_after(e, &[&[replay_datagram()]])
    };
    assert_eq!(after_erase(0), Vec::new(), "after Erase of Band Activity");
    assert_eq!(
        after_erase(1),
        three_periods_replayed(),
        "after Erase of Rx Frequency"
    );
}

/// A band change wipes Band Activity, and a Replay then sends only what the new band has given.
#[test]
fn after_a_band_change_a_replay_sends_only_the_new_bands_decodes() {
    let mut e = heard_over_three_periods();
    e.observe_rig_freq(7_074_000);
    assert_eq!(e.settings().band, "40m", "premise: the dial moved to 40 m");
    let left = decodes_after(e, &[&[replay_datagram()]]);
    let mut e = heard_over_three_periods();
    e.observe_rig_freq(7_074_000);
    assert_eq!(e.fold_heard_for_test("CQ JA1ABC PM95", SLOT + 1), 1);
    let heard_there = decodes_after(e, &[&[replay_datagram()]]);
    assert_eq!(
        (left, heard_there),
        (
            Vec::new(),
            vec![(
                false,
                PERIOD_START_MS_OF_DAY + 15_000,
                "CQ JA1ABC PM95".to_string()
            )]
        ),
        "(replayed after the band change, replayed after a 40 m decode)"
    );
}
