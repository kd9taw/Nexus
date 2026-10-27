//! The SDRconnect scope against a fake SDRconnect on loopback ([`super::fake`]), written from the
//! published "SDRconnect WebSocket API 1.0.3": where a carrier is drawn, how the receiver follows
//! the dial, what the scope says when SDRconnect is away or not streaming, and that nothing it
//! sends reaches past the receive side.

use super::fake::{Config, Fake};
use super::*;
use tempo_app::dto::Spectrum;
use tungstenite::Message;

/// The longest any one expected thing may take here.
const WAIT: Duration = Duration::from_secs(10);

/// Short windows, so a stall, a start window and a reconnect are seen in a test's time.
fn short() -> Times {
    Times {
        start_window_ms: 600,
        stall_ms: 300,
        ladder_ms: [50, 50, 100, 100],
    }
}

fn scope_on(fake: &Fake, feed: &SpectrumFeed) -> SdrconnectScope {
    SdrconnectScope::start_with("127.0.0.1", fake.addr().port(), feed, short())
        .expect("the scope starts")
}

/// Wait until `done`, or fail saying `what` and where the scope is.
fn until(scope: &SdrconnectScope, what: &str, done: impl Fn(&Status) -> bool) {
    let deadline = Instant::now() + WAIT;
    loop {
        let status = scope.status();
        if done(&status) {
            return;
        }
        assert!(Instant::now() < deadline, "{what}: {status:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The RF row in the feed, when it is this source's.
fn rf(feed: &SpectrumFeed) -> Option<Spectrum> {
    feed.row()
        .filter(|r| r.source == SOURCE && !r.row.is_empty())
}

/// A fresh row of ours whose span is centred on `centre`.
fn row_around(feed: &SpectrumFeed, centre: u64) -> Spectrum {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(row) = rf(feed) {
            let mid = (row.lo_hz + row.hi_hz) / 2.0;
            // The edges sit half a bin low (`Geometry::edges`).
            if (mid - centre as f64).abs() < 200.0 {
                return row;
            }
        }
        assert!(Instant::now() < deadline, "no row around {centre} Hz");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The RF frequency the scope puts the middle of bin `j` at.
fn bin_hz(row: &Spectrum, j: usize) -> f64 {
    row.lo_hz + (j as f64 + 0.5) * (row.hi_hz - row.lo_hz) / row.row.len() as f64
}

/// The loudest bin within 2 kHz of `hz`.
fn peak_near(row: &Spectrum, hz: f64) -> f64 {
    let width = (row.hi_hz - row.lo_hz) / row.row.len() as f64;
    let at = ((hz - row.lo_hz) / width) as usize;
    let reach = (2_000.0 / width) as usize;
    let lo = at.saturating_sub(reach);
    let hi = (at + reach).min(row.row.len() - 1);
    let j = (lo..=hi)
        .max_by(|a, b| row.row[*a].total_cmp(&row.row[*b]))
        .expect("bins");
    bin_hz(row, j)
}

/// A row value in dB below full scale.
fn db(v: f32) -> f32 {
    crate::tci::iq::LO_DB + v * (crate::tci::iq::HI_DB - crate::tci::iq::LO_DB)
}

/// ⭐ A CARRIER IS DRAWN AT ITS OWN RF FREQUENCY: two carriers either side of the receiver's
/// centre land in their own bins, by value, on a span of the receiver's rate around its centre.
#[test]
fn a_carrier_is_drawn_at_its_own_rf_frequency() {
    let fake = Fake::start(Config {
        rate: 192_000,
        centre: 14_074_000,
        carriers: vec![14_104_000, 14_050_000],
        ..Config::default()
    });
    let feed = SpectrumFeed::default();
    let scope = scope_on(&fake, &feed);
    until(&scope, "the scope draws", |s| {
        s.state == State::Drawing && s.rows > 2
    });
    let row = row_around(&feed, 14_074_000);
    let bin = 192_000.0 / row.row.len() as f64;
    assert!(
        (row.hi_hz - row.lo_hz - 192_000.0).abs() < 1.0,
        "the span is the rate"
    );
    for carrier in [14_104_000.0, 14_050_000.0] {
        let at = peak_near(&row, carrier);
        assert!(
            (at - carrier).abs() <= bin,
            "the carrier at {carrier} Hz is drawn at {at} Hz"
        );
        let j = ((carrier - row.lo_hz) / bin) as usize;
        assert!(
            db(row.row[j]) > -60.0,
            "the carrier stands out: {} dB",
            db(row.row[j])
        );
    }
    let quiet = ((14_000_000.0 + 10_000.0 - row.lo_hz) / bin) as usize;
    assert!(db(row.row[quiet]) < -100.0, "an empty bin is the floor");
    let status = scope.status();
    assert_eq!(status.api_version.as_deref(), Some("1.0.3"));
    assert_eq!(
        status.geometry,
        Some(Geometry {
            rate: 192_000,
            centre: 14_074_000
        })
    );
}

/// ⭐ THE RECEIVER FOLLOWS THE DIAL: a dial change retunes the receiver to it and the rows move
/// with it; a knob spun fast is followed at most ten times a second and the last dial is the one
/// the receiver lands on.
#[test]
fn the_centre_follows_the_dial_and_lands_on_the_last_one() {
    let fake = Fake::start(Config {
        carriers: vec![7_074_000],
        ..Config::default()
    });
    let feed = SpectrumFeed::default();
    let scope = scope_on(&fake, &feed);
    scope.want(7_074_000, Follow::Centre, false);
    until(&scope, "the first retune", |s| s.centre == Some(7_074_000));
    assert_eq!(fake.centre(), 7_074_000);
    let row = row_around(&feed, 7_074_000);
    assert!(
        (peak_near(&row, 7_074_000.0) - 7_074_000.0).abs() <= 100.0,
        "the carrier sits on the dial at the centre"
    );
    let before = scope.status().retunes;
    let spun = Instant::now();
    for k in 0..50u64 {
        scope.want(7_074_000 + k * 500, Follow::Centre, false);
        std::thread::sleep(Duration::from_millis(10));
    }
    scope.want(7_100_000, Follow::Centre, false);
    let elapsed = spun.elapsed().as_millis() as u64;
    until(&scope, "the last dial", |s| s.centre == Some(7_100_000));
    assert_eq!(fake.centre(), 7_100_000);
    let retunes = scope.status().retunes - before;
    assert!(
        retunes <= elapsed / RETUNE_MS + 2,
        "{retunes} retunes in {elapsed} ms: at most one per {RETUNE_MS} ms"
    );
    row_around(&feed, 7_100_000);
    let sets: Vec<String> = fake
        .received()
        .into_iter()
        .filter(|m| m.contains("set_property"))
        .collect();
    assert!(
        sets.last().is_some_and(|m| m.contains("\"7100000\"")),
        "the last retune is the last dial: {sets:?}"
    );
}

/// The two follow modes, by value: Centre retunes to the dial; Scroll leaves the receiver alone
/// while the dial is in the middle 80 % of its span and retunes to the dial once it leaves it.
#[test]
fn scroll_retunes_only_when_the_dial_leaves_the_middle_of_the_span() {
    let (c, r) = (Some(14_074_000), Some(192_000));
    // The middle 80 % of 192 kHz: 76.8 kHz either side.
    assert_eq!(target(Follow::Scroll, 14_074_000 + 76_800, c, r), c);
    assert_eq!(target(Follow::Scroll, 14_074_000 - 76_800, c, r), c);
    assert_eq!(
        target(Follow::Scroll, 14_074_000 + 76_801, c, r),
        Some(14_150_801)
    );
    assert_eq!(target(Follow::Scroll, 13_990_000, c, r), Some(13_990_000));
    assert_eq!(target(Follow::Centre, 14_074_100, c, r), Some(14_074_100));
    assert_eq!(
        target(Follow::Scroll, 14_074_100, None, r),
        Some(14_074_100),
        "before the receiver says where it is, the dial"
    );
    assert_eq!(target(Follow::Centre, 0, c, r), None, "no dial, no retune");
}

/// A retune in SDRconnect's own window is drawn where it went and not fought: the receiver is
/// retuned again only when the rig's dial moves.
#[test]
fn a_retune_in_sdrconnects_own_window_is_not_fought() {
    let fake = Fake::start(Config::default());
    let feed = SpectrumFeed::default();
    let scope = scope_on(&fake, &feed);
    scope.want(14_074_000, Follow::Centre, false);
    until(&scope, "drawing", |s| s.state == State::Drawing);
    fake.retune_from_its_window(14_200_000);
    until(&scope, "the window's centre", |s| {
        s.centre == Some(14_200_000)
    });
    let retunes = scope.status().retunes;
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        fake.centre(),
        14_200_000,
        "Nexus left the window's retune alone"
    );
    assert_eq!(scope.status().retunes, retunes);
    row_around(&feed, 14_200_000);
    scope.want(14_075_000, Follow::Centre, false);
    until(&scope, "the dial again", |s| s.centre == Some(14_075_000));
}

/// SDRconnect not running is said on the status line, and the scope connects by itself once it
/// starts.
#[test]
fn sdrconnect_not_running_is_said_and_the_scope_connects_once_it_starts() {
    let port = {
        let probe = Fake::start(Config::default());
        probe.addr().port()
    };
    let feed = SpectrumFeed::default();
    let scope = SdrconnectScope::start_with("127.0.0.1", port, &feed, short()).expect("starts");
    until(&scope, "not answering", |s| {
        matches!(s.state, State::NotAnswering(_))
    });
    let at = format!("127.0.0.1:{port}");
    let said = scope.status().state.problem(&at).expect("a problem");
    assert!(
        said.starts_with(&format!("SDRconnect is not answering at {at}")),
        "{said}"
    );
    let fake = Fake::start_on(port, Config::default()).expect("the port is free again");
    until(&scope, "drawing once SDRconnect starts", |s| {
        s.state == State::Drawing
    });
    assert_eq!(scope.status().state.problem(&at), None);
    assert_eq!(fake.connections(), 1);
}

/// A receiver that sends no IQ is said, once the start window has passed.
#[test]
fn a_receiver_that_sends_no_iq_is_said() {
    let fake = Fake::start(Config {
        sends_iq: false,
        ..Config::default()
    });
    let feed = SpectrumFeed::default();
    let scope = scope_on(&fake, &feed);
    until(&scope, "no IQ", |s| s.state == State::NoIq);
    assert!(scope
        .status()
        .state
        .problem("x")
        .is_some_and(|p| p.contains("sends no IQ")));
    assert!(rf(&feed).is_none());
}

/// IQ that stops is said and empties the RF slot, so the scope never holds an old row as live;
/// IQ that comes back is drawn again.
#[test]
fn iq_that_stops_is_said_and_empties_the_slot_and_comes_back() {
    let fake = Fake::start(Config::default());
    let feed = SpectrumFeed::default();
    let scope = scope_on(&fake, &feed);
    until(&scope, "drawing", |s| {
        s.state == State::Drawing && s.rows > 0
    });
    assert!(rf(&feed).is_some());
    fake.pause(true);
    until(&scope, "stalled", |s| s.state == State::Stalled);
    assert!(rf(&feed).is_none(), "the slot is empty once the IQ stopped");
    assert!(scope
        .status()
        .state
        .problem("x")
        .is_some_and(|p| p.contains("stopped sending IQ")));
    fake.pause(false);
    until(&scope, "drawing again", |s| s.state == State::Drawing);
}

/// A transmission is not a stall: a receiver on RX OUT may go quiet under the rig's carrier.
#[test]
fn a_transmission_is_not_a_stall() {
    let fake = Fake::start(Config::default());
    let feed = SpectrumFeed::default();
    let scope = scope_on(&fake, &feed);
    until(&scope, "drawing", |s| s.state == State::Drawing);
    scope.want(14_074_000, Follow::Centre, true);
    fake.pause(true);
    std::thread::sleep(Duration::from_millis(3 * short().stall_ms));
    assert_eq!(
        scope.status().state,
        State::Drawing,
        "no stall under a transmission"
    );
    scope.want(14_074_000, Follow::Centre, false);
    until(&scope, "stalled after the transmission", |s| {
        s.state == State::Stalled
    });
}

/// IQ that arrives at a quarter of the rate SDRconnect reports (decimated without saying so) is
/// never drawn, since every station would land at the wrong frequency, and the status line says
/// both rates.
#[test]
fn iq_slower_than_its_reported_rate_is_not_drawn_and_said() {
    let fake = Fake::start(Config {
        rate: 192_000,
        pace_rate: Some(48_000),
        pairs_per_message: 960,
        carriers: vec![14_100_000],
        ..Config::default()
    });
    let feed = SpectrumFeed::default();
    let scope = scope_on(&fake, &feed);
    until(&scope, "the rates disagree", |s| {
        matches!(s.state, State::RateMismatch { .. })
    });
    let status = scope.status();
    let State::RateMismatch { reported, measured } = status.state.clone() else {
        unreachable!()
    };
    assert_eq!(reported, 192_000);
    assert!(
        (40_000..=56_000).contains(&measured),
        "measured {measured} pairs a second"
    );
    assert!(
        status
            .state
            .problem("x")
            .is_some_and(|p| p.contains("wrong frequencies")),
        "{status:?}"
    );
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(scope.status().rows, 0, "nothing was ever drawn");
    assert!(rf(&feed).is_none());
}

/// The rate meter, by value: a stream at its reported rate agrees, one at half does not, a pause
/// starts the window again rather than reading as a slow rate, and a new reported rate is
/// measured again before it is trusted.
#[test]
fn the_rate_meter_measures_the_arrival_rate() {
    let mut m = RateMeter::default();
    let mut now = 1_000;
    for _ in 0..=50 {
        m.push(3_840, Some(192_000), now);
        now += 20;
    }
    assert_eq!(m.measured(), Some(192_000));
    assert_eq!(m.agrees(), Some(true));
    now += 2 * METER_PAUSE_MS;
    for _ in 0..=50 {
        m.push(1_920, Some(192_000), now);
        now += 20;
    }
    assert_eq!(m.measured(), Some(96_000));
    assert_eq!(m.agrees(), Some(false));
    m.push(1_920, Some(96_000), now);
    assert_eq!(m.agrees(), None, "a new reported rate is measured again");
    now += 3 * METER_PAUSE_MS;
    m.push(1_920, Some(96_000), now);
    assert_eq!(m.measured(), None, "a pause is not a measurement");
}

/// Headless SDRconnect starts with its receiver stopped: the scope starts it, once, and draws.
#[test]
fn a_stopped_receiver_is_started_once() {
    let fake = Fake::start(Config {
        started: false,
        ..Config::default()
    });
    let feed = SpectrumFeed::default();
    let scope = scope_on(&fake, &feed);
    until(&scope, "drawing", |s| s.state == State::Drawing);
    let starts = fake
        .received()
        .iter()
        .filter(|m| m.contains("device_stream_enable"))
        .count();
    assert_eq!(starts, 1);
}

/// SDRconnect saying this client may not control the receiver: no retune is sent, the scope
/// draws where the receiver is.
#[test]
fn no_retune_when_sdrconnect_says_this_client_may_not_control() {
    let fake = Fake::start(Config {
        can_control: false,
        ..Config::default()
    });
    let feed = SpectrumFeed::default();
    let scope = scope_on(&fake, &feed);
    until(&scope, "drawing", |s| {
        s.state == State::Drawing && s.can_control == Some(false)
    });
    scope.want(7_074_000, Follow::Centre, false);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(scope.status().retunes, 0);
    assert!(!fake.received().iter().any(|m| m.contains("set_property")));
    row_around(&feed, 14_074_000);
}

/// ⭐ RECEIVE ONLY ON THE WIRE: through a session with retunes, a stopped receiver and teardown,
/// every message SDRconnect got from Nexus is a property read, the centre frequency, the IQ stream
/// or starting the receiver's stream; teardown stops the IQ and hands the RF slot back.
#[test]
fn every_message_sent_is_a_receive_side_message() {
    let fake = Fake::start(Config {
        started: false,
        ..Config::default()
    });
    let feed = SpectrumFeed::default();
    let scope = scope_on(&fake, &feed);
    scope.want(14_074_000, Follow::Centre, false);
    until(&scope, "drawing", |s| s.state == State::Drawing);
    scope.want(14_080_000, Follow::Scroll, false);
    scope.want(14_300_000, Follow::Scroll, false);
    until(&scope, "the scroll retune", |s| {
        s.centre == Some(14_300_000)
    });
    drop(scope);
    let deadline = Instant::now() + WAIT;
    while !fake
        .received()
        .iter()
        .any(|m| m.contains("\"iq_stream_enable\"") && m.contains("\"false\""))
    {
        assert!(
            Instant::now() < deadline,
            "teardown stops the IQ: {:?}",
            fake.received()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let received = fake.received();
    assert!(received.len() >= 8, "{received:?}");
    for m in &received {
        let v: serde_json::Value = serde_json::from_str(m).expect("JSON");
        let (event, property) = (
            v["event_type"].as_str().unwrap_or_default(),
            v["property"].as_str().unwrap_or_default(),
        );
        let allowed = match event {
            "get_property" => Prop::ALL.iter().any(|p| p.wire() == property),
            "set_property" => property == "device_center_frequency",
            "iq_stream_enable" | "device_stream_enable" => property.is_empty(),
            _ => false,
        };
        assert!(allowed, "Nexus sent {m}");
    }
    feed.publish_rf_frame(
        Spectrum {
            row: vec![0.5],
            lo_hz: 0.0,
            hi_hz: 1.0,
            source: "civ".into(),
        },
        tempo_app::dto::SpectrumScale::Relative,
        None,
    );
    assert_eq!(
        feed.row().map(|r| r.source),
        Some("civ".to_string()),
        "the RF slot is the radio's again"
    );
}

/// An address that answers but is not a WebSocket server is said as such.
#[test]
fn an_address_that_is_not_sdrconnect_is_said() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
    listener.set_nonblocking(true).expect("non-blocking");
    let port = listener.local_addr().expect("addr").port();
    let done = Arc::new(AtomicBool::new(false));
    let server = {
        let done = done.clone();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            while !done.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut tcp, _)) => {
                        let _ = tcp.set_nonblocking(false);
                        let mut buf = [0u8; 1024];
                        let _ = tcp.read(&mut buf);
                        let _ =
                            tcp.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        })
    };
    let feed = SpectrumFeed::default();
    let scope = SdrconnectScope::start_with("127.0.0.1", port, &feed, short()).expect("starts");
    until(&scope, "not SDRconnect", |s| {
        matches!(s.state, State::NotSdrconnect(_))
    });
    assert!(scope
        .status()
        .state
        .problem("x")
        .is_some_and(|p| p.contains("not as SDRconnect's WebSocket API")));
    drop(scope);
    done.store(true, Ordering::Release);
    let _ = server.join();
}

/// A new address is taken without waiting for the old connection: teardown, which the radio loop
/// runs when the operator changes the host or port, returns at once though the scope's thread is
/// still in a connect that hangs. That thread still ends by itself and closes its socket, and it
/// never connects again.
#[test]
fn teardown_does_not_wait_for_a_connect_that_hangs() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
    let port = listener.local_addr().expect("addr").port();
    let feed = SpectrumFeed::default();
    let scope = SdrconnectScope::start_with("127.0.0.1", port, &feed, short()).expect("starts");
    // The stand-in takes the connection and says nothing: the scope waits on its handshake.
    let (mut held, _) = listener.accept().expect("the scope connects");
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(scope.status().connections, 0, "still in the connect");
    let started = Instant::now();
    drop(scope);
    let took = started.elapsed();
    assert!(
        took < Duration::from_millis(50),
        "teardown took {took:?}: it must not wait on the connect"
    );
    // The old thread gives up its connect by itself and closes the socket.
    use std::io::Read;
    held.set_read_timeout(Some(tempo_net::sdrconnect::CONNECT_TIMEOUT + WAIT))
        .expect("timeout");
    let mut buf = [0u8; 4096];
    let closed = loop {
        match held.read(&mut buf) {
            Ok(0) => break true,
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => break true,
            Err(_) => break false,
        }
    };
    assert!(closed, "the old thread closes its socket");
    listener.set_nonblocking(true).expect("non-blocking");
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        listener.accept().is_err(),
        "the old thread does not connect again"
    );
}

/// Whatever SDRconnect sends, the scope does not panic, counts what it cannot use, and draws on.
#[test]
fn whatever_sdrconnect_sends_never_panics_the_scope() {
    let fake = Fake::start(Config::default());
    let feed = SpectrumFeed::default();
    let scope = scope_on(&fake, &feed);
    until(&scope, "drawing", |s| s.state == State::Drawing);
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for k in 0..400 {
        let len = (next() % 40) as usize;
        let mut bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        if k % 3 == 0 && bytes.len() >= 2 {
            bytes[0] = 2;
            bytes[1] = 0;
        }
        fake.inject(Message::binary(bytes.clone()));
        let text = match k % 4 {
            0 => String::from_utf8_lossy(&bytes).into_owned(),
            1 => r#"{"event_type":"property_changed","property":"device_sample_rate","value":"-1"}"#
                .to_string(),
            2 => r#"{"event_type":"property_changed","property":"device_center_frequency","device":"secondary","value":"1"}"#
                .to_string(),
            _ => r#"{"event_type":"property_changed"}"#.to_string(),
        };
        fake.inject(Message::text(text));
    }
    std::thread::sleep(Duration::from_millis(300));
    let rows = scope.status().rows;
    until(&scope, "drawing on", |s| {
        s.state == State::Drawing && s.rows > rows + 2
    });
    assert!(
        scope.status().counts.malformed > 0,
        "the bad IQ was counted"
    );
    row_around(&feed, 14_074_000);
}

/// The words of the transmit and CAT paths, and the SDRconnect requests the scope must never make.
/// Read as whole identifiers in the code, comments aside.
const FORBIDDEN: [&str; 16] = [
    "Rig",
    "CatDaemon",
    "TciDaemon",
    "Connection",
    "Engine",
    "engine_lock",
    "ptt",
    "key",
    "keyed",
    "tune_on",
    "set_freq",
    "set_ptt",
    "transmit",
    "audio_stream_enable",
    "start_recording",
    "apply_device_profile",
];

/// The forbidden words `source` names in its code (comment lines and the test module left out).
fn forbidden_in(source: &str) -> Vec<String> {
    let code = source
        .split("#[cfg(test)]\nmod tests;")
        .next()
        .unwrap_or(source);
    code.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .flat_map(|l| {
            l.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .filter(|w| FORBIDDEN.contains(w))
                .map(move |w| format!("{w}: {}", l.trim()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// ⭐ THE SCOPE CANNOT REACH THE TRANSMITTER OR THE RIG: its code names no part of the transmit or
/// CAT paths, no engine (whose verbs key), and no SDRconnect request outside the receive side.
/// Sends go only through `Link::send`, which takes only `Outbound` (tempo-net's own test holds that
/// list to the receive side). The guard is shown to fire on a planted line first.
#[test]
fn the_scope_never_names_a_transmit_or_cat_path() {
    assert_eq!(
        forbidden_in("fn f(r: &mut Rig) { r.set_ptt(true) }\n// a Rig in a comment\n"),
        vec![
            "Rig: fn f(r: &mut Rig) { r.set_ptt(true) }".to_string(),
            "set_ptt: fn f(r: &mut Rig) { r.set_ptt(true) }".to_string(),
        ],
        "the guard fires on a planted line, and not on a comment"
    );
    let found = forbidden_in(include_str!("../sdrconnect.rs"));
    assert!(found.is_empty(), "the scope names {found:?}");
    let sends: Vec<&str> = include_str!("../sdrconnect.rs")
        .lines()
        .filter(|l| l.contains(".send("))
        .collect();
    assert!(
        sends.iter().all(|l| l.contains("link.send(&")),
        "every send goes through the link: {sends:?}"
    );
}

/// CPU time (ms) the threads whose name starts with `name` have used, from /proc (Linux, USER_HZ
/// 100).
#[cfg(target_os = "linux")]
fn thread_cpu_ms(name: &str) -> f64 {
    let mut total = 0.0;
    for task in std::fs::read_dir("/proc/self/task")
        .expect("/proc")
        .flatten()
    {
        let comm = std::fs::read_to_string(task.path().join("comm")).unwrap_or_default();
        if !comm.trim().starts_with(name) {
            continue;
        }
        let stat = std::fs::read_to_string(task.path().join("stat")).unwrap_or_default();
        // After the ")" that closes the name: state is field 3, utime 14 and stime 15.
        let rest = stat.rsplit_once(')').map_or("", |(_, r)| r);
        let f: Vec<&str> = rest.split_whitespace().collect();
        let ticks = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        total += (ticks(11) + ticks(12)) * 10.0;
    }
    total
}

/// The ingest's cost at SDRconnect's rates (run by hand; prints, and asserts only a bound far
/// above what was measured): the scope against the fake streaming canned IQ for three seconds at
/// each rate, the scope's own thread's CPU time over that, read from /proc.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "a measurement, run by hand: cargo test -p tempo-audio --features device ingest_cost -- --ignored --nocapture"]
fn ingest_cost_at_sdrconnects_rates() {
    for rate in [192_000u32, 2_000_000, 6_000_000, 10_000_000] {
        let fake = Fake::start(Config {
            rate,
            carriers: vec![14_100_000],
            pairs_per_message: (rate / 100) as usize,
            canned: true,
            ..Config::default()
        });
        let feed = SpectrumFeed::default();
        let scope = scope_on(&fake, &feed);
        until(&scope, "drawing", |s| s.state == State::Drawing);
        let (t0, c0, f0, r0) = (
            Instant::now(),
            thread_cpu_ms("sdrconnect-sco"),
            thread_cpu_ms("fake-sdrconnect"),
            scope.status().rows,
        );
        std::thread::sleep(Duration::from_secs(3));
        let wall = t0.elapsed().as_secs_f64() * 1_000.0;
        let cpu = thread_cpu_ms("sdrconnect-sco") - c0;
        let fake_cpu = thread_cpu_ms("fake-sdrconnect") - f0;
        let status = scope.status();
        println!(
            "{rate:>9} Hz: the scope's thread {:.1} % of one core (the fake {:.1} %), {} rows in \
             3 s, {:?}, {} windows skipped, {} pairs dropped behind, {} gaps",
            100.0 * cpu / wall,
            100.0 * fake_cpu / wall,
            status.rows - r0,
            status.state,
            status.skipped,
            status.counts.overflow,
            status.counts.gaps,
        );
        assert!(cpu / wall < 0.5, "under half a core at {rate} Hz");
    }
}

/// The UI reads this source's rows as RF: `isRfScopeSource` in `ui/src/waterfall.ts` names
/// [`SOURCE`]. A label it does not know is drawn as audio on a 0–4000 Hz axis, with nothing
/// erroring, so the two are held together here.
#[test]
fn the_ui_draws_this_sources_rows_as_rf() {
    let ui = include_str!("../../../../ui/src/waterfall.ts");
    let start = ui
        .find("export function isRfScopeSource(")
        .expect("isRfScopeSource is in waterfall.ts");
    let body = &ui[start..start + ui[start..].find("\n}").expect("its end")];
    assert!(
        body.contains("'tci'"),
        "the parse found the predicate: {body}"
    );
    assert!(
        body.contains(&format!("'{SOURCE}'")),
        "isRfScopeSource does not name '{SOURCE}': {body}"
    );
}
