//! The TCI server's app side against a real engine and the real server on loopback: what a program
//! is refused, what its stops do, the dial, mode and power rules, the receive audio it is sent,
//! TX_CHRONO, the manager's switch and port, and the server's line in the snapshot.

use std::net::{Ipv4Addr, TcpListener, TcpStream};

use tempo_app::engine::TxOwner;
use tempo_app::mic::MicTick;
use tempo_app::settings::{OperatingMode, Settings};
use tempo_audio::receive_audio::ReceiveSource;
use tempo_net::tci::frame::{Channels, RawHeader, SampleFormat, HEADER_LEN};
use tokio_tungstenite::tungstenite::{self, Message, WebSocket};

use super::*;

const WAIT: Duration = Duration::from_secs(5);

/// A station on 20 m FT8 with transmit armed and allowed: every gate a program could pass is open,
/// so whatever refuses it is this file's.
fn armed() -> SharedEngine {
    let mut e = Engine::with_settings(Settings {
        mycall: "KD9TAW".into(),
        mygrid: "EN52".into(),
        operating_mode: OperatingMode::Digital,
        band: "20m".into(),
        dial_mhz: 14.074,
        sideband: "USB".into(),
        ..Settings::default()
    });
    e.set_tx_enabled(true);
    assert!(
        e.tx_enabled() && e.tx_allowed() && !e.on_air(),
        "premise: transmit armed and allowed on 14.074, nothing on the air"
    );
    Arc::new(Mutex::new(e))
}

/// The manager serving on a port the system picks, with the receive feed it was given.
fn serving(engine: &SharedEngine) -> (Manager, Arc<ReceiveAudioFeed>) {
    let feed = Arc::new(ReceiveAudioFeed::default());
    let mut m = Manager::new(engine.clone(), feed.clone(), MeterFeed::default());
    m.apply(Some(0));
    assert!(m.running.is_some(), "the server starts on a free port");
    (m, feed)
}

fn port(m: &Manager) -> u16 {
    m.running.as_ref().expect("serving").server.port()
}

fn handle(m: &Manager) -> Handle {
    m.running.as_ref().expect("serving").server.handle()
}

/// A message from the server.
#[derive(Debug)]
enum Got {
    Text(String),
    Binary(Vec<u8>),
    Close(Option<u16>),
}

/// A program on this computer: a plain WebSocket client, as WSJT-X's is (no `Origin`).
struct Client {
    ws: WebSocket<TcpStream>,
}

impl Client {
    fn connect(port: u16) -> Client {
        let tcp = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("the server accepts");
        let (ws, _) =
            tungstenite::client(format!("ws://127.0.0.1:{port}/"), tcp).expect("the handshake");
        ws.get_ref()
            .set_read_timeout(Some(Duration::from_millis(20)))
            .expect("a read timeout");
        Client { ws }
    }

    fn send(&mut self, line: &str) {
        self.ws.send(Message::text(line)).expect("sent");
    }

    fn send_binary(&mut self, bytes: Vec<u8>) {
        self.ws.send(Message::binary(bytes)).expect("sent");
    }

    /// The next message, if one comes within `within`.
    fn next(&mut self, within: Duration) -> Option<Got> {
        let until = Instant::now() + within;
        loop {
            match self.ws.read() {
                Ok(Message::Text(t)) => return Some(Got::Text(t.as_str().to_string())),
                Ok(Message::Binary(b)) => return Some(Got::Binary(b.to_vec())),
                Ok(Message::Close(frame)) => {
                    return Some(Got::Close(frame.map(|f| u16::from(f.code))))
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => return None,
            }
            if Instant::now() >= until {
                return None;
            }
        }
    }

    /// Every message up to and including the text line `last`.
    fn until(&mut self, last: &str) -> Vec<Got> {
        let until = Instant::now() + WAIT;
        let mut got = Vec::new();
        while let Some(m) = self.next(until.saturating_duration_since(Instant::now())) {
            let done = matches!(&m, Got::Text(t) if t == last);
            got.push(m);
            if done {
                return got;
            }
        }
        panic!("no {last:?} within {WAIT:?}; got {got:?}");
    }

    /// The text lines up to and including `last`.
    fn lines_until(&mut self, last: &str) -> Vec<String> {
        self.until(last)
            .into_iter()
            .filter_map(|m| match m {
                Got::Text(t) => Some(t),
                _ => None,
            })
            .collect()
    }

    /// Whatever comes within `within`.
    fn drain(&mut self, within: Duration) -> Vec<Got> {
        let until = Instant::now() + within;
        let mut got = Vec::new();
        while let Some(m) = self.next(until.saturating_duration_since(Instant::now())) {
            got.push(m);
        }
        got
    }
}

/// A program connected and greeted (the init burst ends with `start;`), and its number.
fn program(handle: &Handle, port: u16) -> (Client, ClientId, Vec<String>) {
    let before: Vec<ClientId> = handle.programs().iter().map(|p| p.id).collect();
    let mut client = Client::connect(port);
    let burst = client.lines_until("start;");
    let id = handle
        .programs()
        .iter()
        .map(|p| p.id)
        .find(|id| !before.contains(id))
        .expect("the server lists the program");
    (client, id, burst)
}

/// Whether this lane of the connection log has a line holding `needle`, as the Connections panel
/// reads the log.
fn logged(needle: &str) -> bool {
    crate::get_connection_log()
        .iter()
        .any(|ev| ev.connector == LANE && ev.message.contains(needle))
}

/// Wait for `f` on the engine, or `WAIT`.
fn eventually(engine: &SharedEngine, f: impl Fn(&Engine) -> bool) -> bool {
    let until = Instant::now() + WAIT;
    while Instant::now() < until {
        if f(&engine_lock(engine)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// ⛔ THE CLOSED DEFAULT: "Programs on TCI may transmit" is off on a station that never turned it
/// on. With every Nexus gate open (armed, allowed, nothing on the air), every way a program can ask
/// to transmit is refused, on the wire as TCI refuses a key and at the backend as the station's
/// switch; `tx_enable` says false from the same verdict; and nothing in the engine moves. CONTROL:
/// the transmit latch is still up afterwards, so nothing here disarmed the station to get its
/// refusals.
#[test]
fn with_the_transmit_switch_at_its_default_every_transmit_a_program_asks_for_is_refused() {
    let engine = armed();
    let (mut m, _feed) = serving(&engine);
    let h = handle(&m);
    let (mut client, id, burst) = program(&h, port(&m));
    assert!(
        burst.contains(&"tx_enable:0,false;".to_string()),
        "the burst tells the program it may not transmit: {burst:?}"
    );
    for asked in [
        // No receive source on this station: refused, and a key that needs one is refused anyway.
        "audio_start:0;",
        "trx:0,true,tci;",
        "trx:0,true;",
        "trx:0,true,mic1;",
        "tune:0,true;",
        "cw_macros:0,CQ TEST;",
    ] {
        client.send(asked);
    }
    // A read the server answers in order, after everything above.
    client.send("modulation:0;");
    let lines = client.lines_until("modulation:0,digu;");
    m.publish();
    client.send("modulation:0;");
    let mut lines_after = client.lines_until("modulation:0,digu;");
    let mut all = lines.clone();
    all.append(&mut lines_after);
    for keyed in [
        "trx:0,true;",
        "tune:0,true;",
        "tx_enable:0,true;",
        "audio_start:0;",
    ] {
        assert!(
            !all.iter().any(|l| l == keyed),
            "a program was told {keyed} though programs may not transmit: {all:?}"
        );
    }
    assert_eq!(
        lines.iter().filter(|l| *l == "trx:0,false;").count(),
        3,
        "each of the three keys is answered as a key that did not take: {lines:?}"
    );
    assert!(lines.contains(&"tune:0,false;".to_string()), "{lines:?}");

    let tci = m.backend.clone();
    for kind in [
        KeyKind::Tci,
        KeyKind::Mic(tempo_net::tci::server::Mic::Selected),
    ] {
        assert_eq!(tci.key(id, kind, true), Err(Refusal::Switch), "{kind:?}");
    }
    assert_eq!(tci.tune_on(id), Err(Refusal::Switch));
    let text = tempo_net::tci::wire::Text::new("CQ").expect("a CW text");
    assert_eq!(tci.cw(id, Cw::Macro(text)), Err(Refusal::Switch));

    {
        let e = engine_lock(&engine);
        assert!(
            e.tx_enabled(),
            "CONTROL: the station stays armed; the refusals were the TCI side's"
        );
        assert!(!e.on_air() && !e.tuning() && e.tx_owner().is_none());
    }
    let status = engine_lock(&engine).snapshot().radio.tci_server;
    let refused = status
        .and_then(|s| s.programs.into_iter().find(|p| p.id == id.get()))
        .and_then(|p| p.refused);
    assert_eq!(
        refused.as_deref(),
        Some(Refusal::Switch.text()),
        "the status names the last refusal"
    );
    assert!(logged(&format!("refused {id}: {}", Refusal::Switch.text())));
    assert!(logged(&format!(
        "refused {id}: {}",
        Refusal::NoSource.text()
    )));
}

/// A program's `trx:0,false` stops what is on the air, as WSJT-X's Halt Tx does: the
/// over is cut at the loop's next tick (the abort armed, the slot flag down), transmit disarms, and
/// the stop HOLDS, so entering a section does not re-arm. CONTROL: with nothing on the air it
/// changes nothing at all.
#[test]
fn a_program_s_trx_false_stops_what_is_on_the_air_and_the_stop_holds() {
    for on_air in [true, false] {
        let engine = armed();
        if on_air {
            engine_lock(&engine).app.set_transmitting(true);
        }
        let (m, _feed) = serving(&engine);
        let h = handle(&m);
        let (mut client, _, _) = program(&h, port(&m));
        client.send("trx:0,false;");
        client.send("modulation:0;");
        client.lines_until("modulation:0,digu;");
        let mut e = engine_lock(&engine);
        if on_air {
            assert!(
                !e.tx_enabled() && !e.on_air(),
                "a program's trx:0,false stops the over and disarms"
            );
            assert!(
                e.take_slot_tx_abort(),
                "the loop cuts the over at its next tick"
            );
            e.set_operating_mode("phone", false);
            assert!(!e.tx_enabled(), "the stop holds until the operator arms");
        } else {
            assert!(
                e.tx_enabled(),
                "CONTROL: nothing on the air, nothing stopped"
            );
            assert!(!e.take_slot_tx_abort(), "CONTROL: no abort armed");
        }
    }
}

/// The dial is the operator's while anything is on the air: a program's write is
/// refused and the program told the dial in force. Off the air it retunes through the CAT broker's
/// own rule, band and all. CONTROL: the same write lands once the over ends.
#[test]
fn a_program_s_dial_is_refused_while_anything_is_on_the_air() {
    let engine = armed();
    let (m, _feed) = serving(&engine);
    let h = handle(&m);
    let (mut client, id, _) = program(&h, port(&m));
    let tci = m.backend.clone();
    engine_lock(&engine).app.set_transmitting(true);
    assert_eq!(
        tci.set_dial(id, hz(7_074_000)),
        Err(Refusal::OnAir),
        "a retune under an over"
    );
    client.send("vfo:0,0,7074000;");
    let lines = client.lines_until("tx_frequency:14074000;");
    assert!(
        lines.contains(&"vfo:0,0,14074000;".to_string()),
        "{lines:?}"
    );
    assert_eq!(dial_hz(&engine_lock(&engine)), 14_074_000);

    engine_lock(&engine).app.set_transmitting(false);
    assert_eq!(tci.set_dial(id, hz(7_074_000)), Ok(hz(7_074_000)));
    let e = engine_lock(&engine);
    assert_eq!(dial_hz(&e), 7_074_000);
    assert_eq!(
        e.settings().band,
        "40m",
        "the band follows, as for the CAT broker"
    );
}

/// The spec's §3.5: a change made in the radio program holds the parameter for 200 ms. A dial the
/// operator just moved refuses a program's write, whether the manager or the write saw the move
/// first; 200 ms on, the same write lands. The hold runs from the move as the manager saw it, not
/// from a program's first write after it: a write long after the move lands at once. CONTROL: a
/// program's own retune holds nothing.
#[test]
fn a_program_s_dial_waits_out_200_ms_after_the_operator_moves_it() {
    let engine = armed();
    let (mut m, _feed) = serving(&engine);
    let h = handle(&m);
    let (_client, id, _) = program(&h, port(&m));
    let tci = m.backend.clone();
    assert_eq!(tci.set_dial(id, hz(14_075_000)), Ok(hz(14_075_000)));
    assert_eq!(
        tci.set_dial(id, hz(14_076_000)),
        Ok(hz(14_076_000)),
        "CONTROL: a program's own retune holds nothing"
    );
    let wait_out = || std::thread::sleep(OPERATOR_HOLD + Duration::from_millis(50));
    for manager_sees_it_first in [false, true] {
        engine_lock(&engine).set_frequency(14.080, "20m", "USB");
        if manager_sees_it_first {
            m.publish();
        }
        assert_eq!(
            tci.set_dial(id, hz(14_077_000)),
            Err(Refusal::OperatorChange),
            "manager first: {manager_sees_it_first}"
        );
        assert_eq!(dial_hz(&engine_lock(&engine)), 14_080_000);
        wait_out();
        assert_eq!(tci.set_dial(id, hz(14_077_000)), Ok(hz(14_077_000)));
    }
    engine_lock(&engine).set_frequency(14.081, "20m", "USB");
    m.publish();
    wait_out();
    assert_eq!(
        tci.set_dial(id, hz(14_078_000)),
        Ok(hz(14_078_000)),
        "a write well after the move the manager saw is not held"
    );
}

/// Power only down, through the one chokepoint, so the operator's ceiling holds. A
/// level Nexus does not know cannot be shown lower and is refused. Tune power: never up; down is
/// not offered.
#[test]
fn a_program_may_lower_power_and_never_raise_it() {
    let engine = armed();
    let (m, _feed) = serving(&engine);
    let h = handle(&m);
    let (_client, id, _) = program(&h, port(&m));
    let tci = m.backend.clone();
    assert_eq!(
        tci.set_drive(id, 10),
        Err(Refusal::Raise),
        "no level known: refused"
    );
    engine_lock(&engine).set_rf_power(0.5);
    assert_eq!(
        tci.set_drive(id, 60),
        Err(Refusal::Raise),
        "a raise is refused"
    );
    assert_eq!(engine_lock(&engine).effective_rf_power(), Some(0.5));
    assert_eq!(tci.set_drive(id, 50), Ok(50), "the level in force");
    assert_eq!(tci.set_drive(id, 40), Ok(40), "a program may lower it");
    assert_eq!(engine_lock(&engine).effective_rf_power(), Some(0.4));

    // The ceiling holds: the radio read back above the digital cap, a program lowers to a level
    // still above it, and the cap is what is commanded.
    let capped = armed();
    {
        let mut e = engine_lock(&capped);
        let mut s = e.settings().clone();
        s.max_power_digital = Some(0.3);
        e.apply_settings(s);
        e.observe_rig_power(0.5);
    }
    let (m2, _feed2) = serving(&capped);
    let h2 = handle(&m2);
    let (_c2, id2, _) = program(&h2, port(&m2));
    assert_eq!(m2.backend.set_drive(id2, 40), Ok(30));

    {
        let mut e = engine_lock(&engine);
        let mut s = e.settings().clone();
        s.tune_power_pct = Some(20);
        e.apply_settings(s);
    }
    assert_eq!(tci.set_tune_drive(id, 20), Ok(20));
    assert_eq!(tci.set_tune_drive(id, 30), Err(Refusal::Raise));
    assert_eq!(tci.set_tune_drive(id, 10), Err(Refusal::Tier));
}

/// The mode is the section's: the CAT broker's rule moves only the side inside its family, and
/// nothing moves while anything is on the air.
#[test]
fn a_program_s_mode_follows_the_cat_broker_s_rule() {
    let engine = armed();
    let (m, _feed) = serving(&engine);
    let h = handle(&m);
    let (_client, id, _) = program(&h, port(&m));
    let tci = m.backend.clone();
    let word = |w: &str| Word::new(w).expect("a mode word");
    assert_eq!(tci.set_mode(id, &word("digl")), Ok(word("digl")));
    assert_eq!(engine_lock(&engine).rig_mode_effective(), "PKTLSB");
    assert_eq!(
        tci.set_mode(id, &word("usb")),
        Err(Refusal::Family),
        "SSB is not the Digital section's"
    );
    engine_lock(&engine).app.set_transmitting(true);
    assert_eq!(tci.set_mode(id, &word("digu")), Err(Refusal::OnAir));
    assert_eq!(engine_lock(&engine).rig_mode_effective(), "PKTLSB");
}

/// `tune:0,false` ends Tune whoever started it, and `cw_macros_stop` ends CW from any origin.
/// CONTROL: `cw_macros_stop` with an FT over on the air and no CW leaves the over
/// alone (`stop_cw` would also cut it).
#[test]
fn a_program_s_tune_and_cw_stops_end_only_what_they_name() {
    let engine = armed();
    let (m, _feed) = serving(&engine);
    let h = handle(&m);
    let (_client, id, _) = program(&h, port(&m));
    let tci = m.backend.clone();

    engine_lock(&engine).set_tune(true);
    assert!(engine_lock(&engine).tuning(), "premise: the operator tunes");
    tci.tune_off(id);
    assert!(
        !engine_lock(&engine).tuning(),
        "a program's tune:0,false ends it"
    );

    {
        let mut e = engine_lock(&engine);
        e.set_operating_mode("cw", false);
        e.send_cw("CQ TEST");
        assert_eq!(e.tx_owner(), Some(TxOwner::Cw), "premise: CW is sending");
    }
    tci.cw_stop(id);
    assert_eq!(
        engine_lock(&engine).tx_owner(),
        None,
        "cw_macros_stop ends it"
    );

    let ft = armed();
    let (m2, _feed2) = serving(&ft);
    let h2 = handle(&m2);
    let (_c2, id2, _) = program(&h2, port(&m2));
    {
        let mut e = engine_lock(&ft);
        e.app.set_transmitting(true);
        let _ = e.take_slot_tx_abort();
    }
    m2.backend.cw_stop(id2);
    let mut e = engine_lock(&ft);
    assert!(
        !e.take_slot_tx_abort() && e.on_air(),
        "CONTROL: an FT over is not CW, and goes on"
    );
}

/// What programs are told is the engine's: the dial, the mode as a TCI word, power and Tune power,
/// `trx` for anything on the air (Tune included), the S-meter in dBm on each side of 30 MHz.
#[test]
fn the_station_programs_are_told_is_the_engine_s() {
    let engine = armed();
    let mut e = engine_lock(&engine);
    e.set_rf_power(0.4);
    let s = station(&e, &[], Some(6));
    assert_eq!(s.dial, hz(14_074_000));
    assert_eq!(s.mode.as_str(), "digu");
    assert_eq!((s.drive, s.tune_drive), (40, 40));
    assert!(!s.trx && !s.tune && !s.receive_only);
    assert_eq!(s.rx_dbm, Some(-67.0), "S9+6 dB on 20 m");
    assert_eq!(s.tx_buffering_ms, 100);
    e.set_tune(true);
    let tuned = station(&e, &[], None);
    assert!(tuned.trx && tuned.tune, "a tune carrier is on the air too");
    e.set_tune(false);
    e.set_frequency(144.174, "2m", "USB");
    assert_eq!(station(&e, &[], Some(0)).rx_dbm, Some(-93.0), "S9 on 2 m");
}

/// Receive audio goes to each program that started it, in that program's own stream (rate,
/// format, channels, frame length), and to no other; once none listens, the pump lets its reader go,
/// so the feed copies nothing.
#[test]
fn receive_audio_goes_to_each_program_that_started_it_in_its_own_stream() {
    let engine = armed();
    let feed = Arc::new(ReceiveAudioFeed::default());
    let source = ReceiveSource {
        epoch: 1,
        rate: 48_000,
    };
    feed.replace_source(Some(source));
    let backend = Arc::new(EngineTci::new(engine.clone(), feed.clone()));
    let first = station(&engine_lock(&engine), &[], None);
    let server = Server::start(
        Config {
            port: 0,
            limits: Limits::default(),
        },
        backend,
        first,
    )
    .expect("a loopback port");
    let h = server.handle();
    let (mut wide, wide_id, _) = program(&h, server.port());
    let (mut narrow, narrow_id, _) = program(&h, server.port());
    let (mut deaf, _, _) = program(&h, server.port());
    wide.send("audio_start:0;");
    wide.lines_until("audio_start:0;");
    for line in [
        "audio_samplerate:12000;",
        "audio_stream_channels:1;",
        "audio_stream_sample_type:int16;",
        "audio_stream_samples:256;",
        "audio_start:0;",
    ] {
        narrow.send(line);
    }
    narrow.lines_until("audio_start:0;");

    let mut pump = PumpState::default();
    pump.turn(&h, &feed, None, Instant::now());
    // A subscription drops the first batch it sees; then 120 ms of a steady level.
    for _ in 0..7 {
        feed.publish(source, Instant::now(), &[0.25; 960]);
        pump.turn(&h, &feed, None, Instant::now());
    }

    let frames = |client: &mut Client| -> Vec<Vec<u8>> {
        client
            .drain(Duration::from_millis(300))
            .into_iter()
            .filter_map(|m| match m {
                Got::Binary(b) => Some(b),
                _ => None,
            })
            .collect()
    };
    let wide_frames = frames(&mut wide);
    assert!(wide_frames.len() >= 4, "{} frames", wide_frames.len());
    for f in &wide_frames {
        let head = RawHeader::read(f).expect("a header");
        assert_eq!(
            (
                head.kind,
                head.receiver,
                head.sample_rate,
                head.format,
                head.channels,
                head.length
            ),
            (
                BlockKind::RxAudio.code(),
                0,
                48_000,
                SampleFormat::Float32.code(),
                2,
                2048
            ),
            "the default stream: 48 kHz float32 stereo, 2048 values"
        );
        assert_eq!(f.len(), HEADER_LEN + 2048 * 4);
        assert!(
            f[HEADER_LEN..]
                .chunks_exact(4)
                .all(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) == 0.25),
            "48 kHz to 48 kHz is the audio itself"
        );
    }
    let narrow_frames = frames(&mut narrow);
    assert!(
        !narrow_frames.is_empty(),
        "the 12 kHz program is sent audio"
    );
    for f in &narrow_frames {
        let head = RawHeader::read(f).expect("a header");
        assert_eq!(
            (
                head.kind,
                head.sample_rate,
                head.format,
                head.channels,
                head.length
            ),
            (
                BlockKind::RxAudio.code(),
                12_000,
                SampleFormat::Int16.code(),
                1,
                256
            ),
            "its own stream: 12 kHz int16 mono, 256 values"
        );
        assert_eq!(f.len(), HEADER_LEN + 256 * 2);
    }
    assert!(
        frames(&mut deaf).is_empty(),
        "a program that did not start audio is sent none"
    );

    wide.send("audio_stop:0;");
    wide.lines_until("audio_stop:0;");
    narrow.send("audio_stop:0;");
    narrow.lines_until("audio_stop:0;");
    pump.turn(&h, &feed, None, Instant::now());
    assert!(
        feed.subscribe(source).is_ok(),
        "with no program listening the pump holds no reader"
    );
    let _ = (wide_id, narrow_id);
}

/// A backend that admits every key, so the server holds a `tci` key and would pass a TX_CHRONO
/// (tempo-net's own guard): the pump's own rule is all that is left to test.
struct Admits;

impl TciBackend for Admits {
    fn disconnected(&self, _: ClientId) {}
    fn set_dial(&self, _: ClientId, hz: Hz) -> Result<Hz, Refusal> {
        Ok(hz)
    }
    fn set_mode(&self, _: ClientId, mode: &Word) -> Result<Word, Refusal> {
        Ok(mode.clone())
    }
    fn set_drive(&self, _: ClientId, level: u32) -> Result<u32, Refusal> {
        Ok(level)
    }
    fn set_tune_drive(&self, _: ClientId, level: u32) -> Result<u32, Refusal> {
        Ok(level)
    }
    fn key(&self, _: ClientId, _: KeyKind, _: bool) -> Result<(), Refusal> {
        Ok(())
    }
    fn release(&self, _: ClientId) {}
    fn tune_on(&self, _: ClientId) -> Result<(), Refusal> {
        Ok(())
    }
    fn tune_off(&self, _: ClientId) {}
    fn end_own(&self, _: ClientId) {}
    fn tx_audio(&self, _: ClientId, _: AudioFormat, _: &[u8]) {}
    fn audio_start(&self, _: ClientId, _: AudioFormat) -> Result<(), Refusal> {
        Ok(())
    }
    fn audio_stop(&self, _: ClientId) {}
    fn cw_stop(&self, _: ClientId) {}
}

fn chronos(client: &mut Client, within: Duration) -> Vec<RawHeader> {
    client
        .drain(within)
        .into_iter()
        .filter_map(|m| match m {
            Got::Binary(b) => RawHeader::read(&b).filter(|h| h.kind == 3),
            _ => None,
        })
        .collect()
}

/// TX_CHRONO goes only to the program whose over the engine says is armed, and only while it
/// holds a `tci` key: none with no armed over, none for an over armed for someone who holds no key.
/// CONTROL: armed for the key holder, the requests come, in its stream.
#[test]
fn tx_chrono_goes_only_to_the_program_whose_over_is_armed() {
    let feed = Arc::new(ReceiveAudioFeed::default());
    let server = Server::start(
        Config {
            port: 0,
            limits: Limits::default(),
        },
        Arc::new(Admits),
        station(&engine_lock(&armed()), &[], None),
    )
    .expect("a loopback port");
    let h = server.handle();
    let (mut keyed, keyed_id, _) = program(&h, server.port());
    let (mut other, other_id, _) = program(&h, server.port());
    keyed.send("trx:0,true,tci;");
    keyed.lines_until("trx:0,true;");
    assert_eq!(
        h.programs()
            .iter()
            .find(|p| p.id == keyed_id)
            .and_then(|p| p.key),
        Some(KeyKind::Tci),
        "premise: the server holds the program's tci key"
    );

    let mut pump = PumpState::default();
    for (armed, case) in [
        (None, "no over armed"),
        (Some(other_id), "an over armed for a program with no key"),
    ] {
        for _ in 0..10 {
            pump.turn(&h, &feed, armed, Instant::now());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            chronos(&mut keyed, Duration::from_millis(100)).is_empty(),
            "{case}: no TX_CHRONO to the key holder"
        );
        assert!(
            chronos(&mut other, Duration::from_millis(50)).is_empty(),
            "{case}"
        );
    }

    for _ in 0..10 {
        pump.turn(&h, &feed, Some(keyed_id), Instant::now());
        std::thread::sleep(Duration::from_millis(10));
    }
    let asked = chronos(&mut keyed, Duration::from_millis(200));
    assert!(
        asked.len() >= 4,
        "CONTROL: the armed over's program is asked for audio: {}",
        asked.len()
    );
    assert!(asked
        .iter()
        .all(|c| (c.receiver, c.sample_rate, c.length) == (0, 48_000, 2048)));
    assert!(chronos(&mut other, Duration::from_millis(50)).is_empty());
}

/// The pacer asks for the over's lead at the arm, then one request per frame of real time, and a
/// late turn never makes a burst: after a stall it asks for the lead again, not for the time it lost.
#[test]
fn the_pacer_asks_for_the_lead_then_real_time_and_never_a_burst() {
    let server = Server::start(
        Config {
            port: 0,
            limits: Limits::default(),
        },
        Arc::new(Admits),
        station(&engine_lock(&armed()), &[], None),
    )
    .expect("a loopback port");
    let (_client, id, _) = program(&server.handle(), server.port());
    let t0 = Instant::now();
    let mut pacer = Pacer::new(id, AudioFormat::default(), t0).expect("the default stream");
    // 48 kHz, 1024 frames a request (2048 values stereo); the lead is 100 ms, 4800 frames.
    assert_eq!(pacer.due(t0), 4, "the lead at the arm");
    let mut asked = 4;
    for ms in (10..=1000).step_by(10) {
        asked += pacer.due(t0 + Duration::from_millis(ms));
    }
    let frames = asked * 1024;
    assert!(
        (48_000 + 4_800 - 1_024..=48_000 + 4_800).contains(&frames),
        "a second of real time and the lead: {frames} frames"
    );
    let late = pacer.due(t0 + Duration::from_millis(3000));
    assert!(
        late <= 5,
        "after a 2 s stall, the lead again, not a burst: {late}"
    );
}

/// The manager follows the switch and the port, as the CAT broker's does: on, a program connects
/// and the snapshot names the port and the program; a new port moves the server (the program on the
/// old one is told `trx:0,false;` and closed, and the old port is free); off, nothing listens and
/// the snapshot's line is gone.
#[test]
fn the_server_follows_its_switch_and_its_port() {
    let engine = armed();
    let (mut m, _feed) = serving(&engine);
    let first = port(&m);
    let addrs = m
        .running
        .as_ref()
        .expect("serving")
        .server
        .local_addrs()
        .to_vec();
    assert!(
        !addrs.is_empty() && addrs.iter().all(|a| a.ip().is_loopback()),
        "this computer only: {addrs:?}"
    );
    let h = handle(&m);
    let (mut client, id, burst) = program(&h, first);
    assert!(burst.contains(&"device:Nexus;".to_string()));
    m.publish();
    let status = engine_lock(&engine).snapshot().radio.tci_server;
    let status = status.expect("the snapshot carries the server");
    assert_eq!(status.port, Some(first));
    assert_eq!(status.programs.len(), 1);
    assert_eq!(status.programs[0].id, id.get());
    assert!(status.programs[0].peer.starts_with("127.0.0.1:"));

    let quiet = crate::lan_client::tests::listener_below_the_ephemeral_range(Ipv4Addr::LOCALHOST);
    let moved_to = quiet.local_addr().expect("a port").port();
    drop(quiet);
    m.apply(Some(moved_to));
    let second = port(&m);
    assert!(
        second == moved_to || second == moved_to + 1,
        "the new port, or the next: {second}"
    );
    let told = client.drain(Duration::from_millis(500));
    assert!(
        told.iter()
            .any(|g| matches!(g, Got::Text(t) if t == "trx:0,false;")),
        "the program on the old port is told nothing is on the air: {told:?}"
    );
    assert!(
        told.iter().any(|g| matches!(g, Got::Close(Some(1001)))),
        "…and closed as going away: {told:?}"
    );
    assert!(
        TcpStream::connect((Ipv4Addr::LOCALHOST, first)).is_err(),
        "the old port is free"
    );
    let (_c2, _, _) = program(&handle(&m), second);
    m.publish();
    let status = engine_lock(&engine).snapshot().radio.tci_server;
    assert_eq!(status.and_then(|s| s.port), Some(second));

    m.apply(None);
    assert!(m.running.is_none());
    assert!(
        TcpStream::connect((Ipv4Addr::LOCALHOST, second)).is_err(),
        "switched off, nothing listens"
    );
    m.publish();
    assert_eq!(
        engine_lock(&engine).snapshot().radio.tci_server,
        None,
        "switched off, the snapshot has no line for it"
    );
}

/// A port that will not bind is said once, in the snapshot and the log, and retried quietly; the
/// server starts once the port is free and the retry is due.
#[test]
fn a_port_that_will_not_bind_is_reported_once_and_retried() {
    let engine = armed();
    let feed = Arc::new(ReceiveAudioFeed::default());
    let mut m = Manager::new(engine.clone(), feed, MeterFeed::default());
    let held = crate::lan_client::tests::listener_below_the_ephemeral_range(Ipv4Addr::LOCALHOST);
    let taken = held.local_addr().expect("a port").port();
    let next = TcpListener::bind((Ipv4Addr::LOCALHOST, taken + 1));
    m.apply(Some(taken));
    assert!(m.running.is_none(), "both ports are taken");
    m.publish();
    let status = engine_lock(&engine).snapshot().radio.tci_server;
    let status = status.expect("switched on, the snapshot says why it is not serving");
    assert_eq!(status.port, None);
    let why = status.error.expect("a reason");
    assert!(
        why.contains(&format!("ports {taken} and {}", taken + 1)),
        "{why}"
    );
    let said = |why: &str| {
        crate::get_connection_log()
            .iter()
            .filter(|ev| ev.connector == LANE && ev.message == why)
            .count()
    };
    assert_eq!(said(&why), 1, "said in the connection log");
    m.apply(Some(taken));
    assert!(m.running.is_none(), "not retried before the retry is due");
    m.retry = Duration::ZERO;
    m.apply(Some(taken));
    assert!(m.running.is_none(), "still taken at the retry");
    assert_eq!(said(&why), 1, "said once, however often it is retried");

    drop(held);
    drop(next);
    m.apply(Some(taken));
    assert!(m.running.is_some(), "the free port is taken at the retry");
    m.publish();
    let status = engine_lock(&engine).snapshot().radio.tci_server;
    assert_eq!(status.and_then(|s| s.error), None);
}

/// The operator's Disconnect closes the program (1000) and it leaves the list; a number no program
/// has does nothing.
#[test]
fn the_operator_s_disconnect_closes_the_program() {
    let engine = armed();
    let (m, _feed) = serving(&engine);
    let h = handle(&m);
    let (mut client, id, _) = program(&h, port(&m));
    assert!(!m.serving.disconnect(id.get() + 100), "no such program");
    assert!(m.serving.disconnect(id.get()));
    let told = client.drain(Duration::from_millis(500));
    assert!(
        told.iter().any(|g| matches!(g, Got::Close(Some(1000)))),
        "{told:?}"
    );
    let until = Instant::now() + WAIT;
    while !h.programs().is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(h.programs().is_empty(), "the program left the list");
}

/// A web page is refused at the handshake (tempo-net's gate), and the operator reads which page in
/// the connection log.
#[test]
fn a_web_page_s_connection_is_refused_and_named_in_the_log() {
    let engine = armed();
    let (m, _feed) = serving(&engine);
    let p = port(&m);
    let origin = "https://tci-page.example";
    let request = tungstenite::handshake::client::Request::builder()
        .uri(format!("ws://127.0.0.1:{p}/"))
        .header("Host", format!("127.0.0.1:{p}"))
        .header("Origin", origin)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tungstenite::handshake::client::generate_key(),
        )
        .body(())
        .expect("a request");
    let tcp = TcpStream::connect((Ipv4Addr::LOCALHOST, p)).expect("the server accepts");
    assert!(
        tungstenite::client(request, tcp).is_err(),
        "a handshake carrying Origin is refused"
    );
    assert!(eventually(&engine, |_| logged(&format!(
        "refused a web page ({origin})"
    ))));
}

// ── Programs' transmit requests through the engine's TCI rules ─────────────────────────────────

/// A station in Phone on 20 m with every privilege, transmit armed as entering Phone arms it, and
/// "Programs on TCI may transmit" on: every gate in front of a program's over or microphone key is
/// open, so whatever refuses one is the rule a test is about.
fn phone() -> SharedEngine {
    let mut e = Engine::with_settings(Settings {
        mycall: "KD9TAW".into(),
        mygrid: "EN52".into(),
        tci_server_tx: true,
        ..Settings::default()
    });
    e.set_license_class("extra");
    e.set_operating_mode("phone", false);
    e.set_frequency(14.250, "20m", "USB");
    assert!(
        e.tx_enabled() && e.tx_allowed() && e.tci_transmit() && !e.on_air(),
        "premise: Phone armed and allowed on 14.250, programs may transmit, nothing on the air"
    );
    Arc::new(Mutex::new(e))
}

/// The operator's settings save turning "Programs on TCI may transmit" on or off.
fn programs_may_transmit(engine: &SharedEngine, on: bool) {
    let mut e = engine_lock(engine);
    let mut s = e.settings().clone();
    s.tci_server_tx = on;
    e.apply_settings(s);
}

/// The manager serving with a receive source, so a program's `audio_start` is taken.
fn serving_with_audio(engine: &SharedEngine) -> (Manager, Arc<ReceiveAudioFeed>) {
    let (m, feed) = serving(engine);
    feed.replace_source(Some(ReceiveSource {
        epoch: 1,
        rate: 48_000,
    }));
    (m, feed)
}

/// A program connected and greeted, its audio started as WSJT-X starts it: a `tci` key needs its
/// stream (the verdict's item 9).
fn listening(h: &Handle, port: u16) -> (Client, ClientId) {
    let (mut client, id, _) = program(h, port);
    client.send("audio_start:0;");
    client.lines_until("audio_start:0;");
    (client, id)
}

/// A program as [`listening`] that has also asked a voice mode, as a logger's voice keyer or
/// WSJT-X set to Radio mode USB asks one (`modulation:0,usb;`): a `tci` key is refused without it.
/// The server runs a program's commands in order, so the engine has the ask when its audio start
/// is answered.
fn voice_program(h: &Handle, port: u16) -> (Client, ClientId) {
    let (mut client, id, _) = program(h, port);
    client.send("modulation:0,usb;");
    client.send("audio_start:0;");
    client.lines_until("audio_start:0;");
    (client, id)
}

/// One transmit frame in a program's default stream (48 kHz float32 stereo): `frames` frames of
/// `level` on both channels, its `length` in values.
fn tx_frame(frames: usize, level: f32) -> Vec<u8> {
    let header = BlockHeader {
        trx: Trx::new(0).expect("receiver 0"),
        kind: BlockKind::TxAudio,
        sample_rate: 48_000,
        format: SampleFormat::Float32,
        channels: Channels::Two,
    };
    frame::encode_block(&header, LengthMeaning::Values, &vec![level; frames * 2])
        .expect("a transmit frame")
}

/// The radio loop's tick for programs' overs, as the loop's own TCI block makes it: what the loop
/// is told to do. The transmit route never changes here.
fn loop_tick(engine: &SharedEngine) -> MicTick {
    engine_lock(engine).poll_tci(Instant::now(), 40.0, 1)
}

/// `client`'s over: its key admitted and the station published (as the manager publishes it every
/// 50 ms), one frame of its audio sent, and the radio loop's tick keying on it.
fn key_over(engine: &SharedEngine, m: &mut Manager, client: &mut Client) {
    client.send("trx:0,true,tci;");
    client.lines_until("trx:0,true;");
    m.publish();
    // 2048 frames at 48 kHz: past what the resampler keeps back, at least one 20 ms frame of the
    // over's at 12 kHz.
    client.send_binary(tx_frame(2048, 0.5));
    let until = Instant::now() + WAIT;
    loop {
        match loop_tick(engine) {
            MicTick::Key => return,
            MicTick::Armed if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(2));
            }
            other => {
                panic!("premise: the program's first audio keys its over; the loop: {other:?}")
            }
        }
    }
}

/// Program `id` as the server lists it.
fn listed(h: &Handle, id: ClientId) -> Program {
    h.programs()
        .into_iter()
        .find(|p| p.id == id)
        .expect("the program is connected")
}

/// What the station tells program `id` as `tx_enable`, from the engine now.
fn told_tx_enable(engine: &SharedEngine, h: &Handle, id: ClientId) -> bool {
    station(&engine_lock(engine), &h.programs(), None)
        .tx_enable
        .into_iter()
        .any(|(c, on)| c == id && on)
}

/// A read the server answers in order: everything `client` sent before it has been run.
fn in_order(client: &mut Client, mode: &str) {
    client.send("modulation:0;");
    client.lines_until(&format!("modulation:0,{mode};"));
}

/// ⛔ A PROGRAM'S KEY IS THE ENGINE'S VERDICT, AND ITS OVER IS ITS OWN. In Phone with the switch on,
/// a program whose audio is started is told `tx_enable:0,true;`, its key is admitted (`trx:0,true;`),
/// it owns the transmitter in the engine and in the server (which keeps its key through the
/// manager's publish), TX_CHRONO asks it for its audio, and its first frame keys the over. Another
/// program is refused while it holds it, as TCI tells a key that did not take, and told
/// `tx_enable:0,false;`. The owner's own `trx:0,false` lets its over play out: nothing halted, and
/// nothing flushed.
#[test]
fn a_programs_key_is_the_engines_verdict_and_its_over_is_its_own() {
    let engine = phone();
    let (mut m, _feed) = serving_with_audio(&engine);
    let h = handle(&m);
    let (mut a, a_id) = voice_program(&h, port(&m));
    let (mut b, b_id, _) = program(&h, port(&m));
    m.publish();
    a.lines_until("tx_enable:0,true;");
    assert!(
        !told_tx_enable(&engine, &h, b_id),
        "a program with no audio stream told it may transmit"
    );

    key_over(&engine, &mut m, &mut a);
    {
        let e = engine_lock(&engine);
        assert_eq!(e.tci_owner(), Some(a_id.get()), "the engine's owner");
        assert_eq!(e.tx_owner(), Some(TxOwner::Tci));
        assert!(e.tci_over_keyed(), "the over keyed on its audio");
    }
    assert_eq!(
        listed(&h, a_id).key,
        Some(KeyKind::Tci),
        "the server let go of the key the engine holds"
    );
    assert_eq!(*lock(&m.armed), Some(a_id), "the pump's armed over");
    let asked = chronos(&mut a, Duration::from_millis(300));
    assert!(
        !asked.is_empty(),
        "no TX_CHRONO asked the over's owner for audio"
    );
    assert!(asked
        .iter()
        .all(|c| (c.receiver, c.sample_rate, c.length) == (0, 48_000, 2048)));

    b.send("trx:0,true;");
    b.lines_until("trx:0,false;");
    assert_eq!(
        m.backend.key(
            b_id,
            KeyKind::Mic(tempo_net::tci::server::Mic::Selected),
            false
        ),
        Err(Refusal::Owner),
        "another program keyed under the over"
    );
    assert!(logged(&format!(
        "refused {b_id}: {}",
        Refusal::Owner.text()
    )));
    assert!(!told_tx_enable(&engine, &h, b_id));
    assert!(
        told_tx_enable(&engine, &h, a_id),
        "the owner's re-assert would pass"
    );

    a.send("trx:0,false;");
    in_order(&mut a, "usb");
    {
        let mut e = engine_lock(&engine);
        assert!(e.tx_enabled(), "the owner's release halted transmit");
        assert!(
            e.tci_over_keyed(),
            "the owner's release cut its over at once"
        );
        assert!(!e.take_tci_abort(), "the owner's release flushed its over");
    }
    let until = Instant::now() + WAIT;
    while loop_tick(&engine) != MicTick::Idle {
        assert!(Instant::now() < until, "the released over never played out");
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut e = engine_lock(&engine);
    assert!(
        !e.take_tci_abort() && e.tci_owner().is_none() && e.tx_enabled(),
        "the over did not play out to its graceful end"
    );
}

/// ⛔ NOTHING A PROGRAM SENDS ARMS TRANSMIT, LETS GO OF THE OPERATOR'S STOP OR A WATCHDOG TRIP,
/// RAISES THE POWER, OR KEYS OUTSIDE THE LICENCE, through the backend
/// the server calls: every call a program's commands make, in a row, from every station state a
/// program can find, each checked against the one before. CONTROL: from the open station a key is
/// admitted and the power goes down, so the sweep is not vacuous.
#[test]
fn nothing_a_program_sends_arms_lets_go_of_a_stop_or_trip_raises_power_or_keys_outside_the_licence()
{
    use tempo_net::tci::server::Mic;
    use tempo_net::tci::wire::Text;
    type Step = (&'static str, fn(&EngineTci, ClientId, ClientId));
    let steps: Vec<Step> = vec![
        ("A's voice mode", |t, a, _| {
            let _ = t.set_mode(a, &Word::new("usb").expect("a mode word"));
        }),
        ("A's tci key", |t, a, _| {
            let _ = t.key(a, KeyKind::Tci, true);
        }),
        ("A's audio", |t, a, _| {
            t.tx_audio(a, AudioFormat::default(), &tx_frame(1024, 0.5));
        }),
        ("B's microphone key", |t, _, b| {
            let _ = t.key(b, KeyKind::Mic(Mic::Mic1), false);
        }),
        ("A's microphone key", |t, a, _| {
            let _ = t.key(a, KeyKind::Mic(Mic::Selected), false);
        }),
        ("A's tune", |t, a, _| {
            let _ = t.tune_on(a);
        }),
        ("B's tune", |t, _, b| {
            let _ = t.tune_on(b);
        }),
        ("A's drive 100", |t, a, _| {
            let _ = t.set_drive(a, 100);
        }),
        ("A's tune drive 100", |t, a, _| {
            let _ = t.set_tune_drive(a, 100);
        }),
        ("A's drive 10", |t, a, _| {
            let _ = t.set_drive(a, 10);
        }),
        ("A's dial on the band", |t, a, _| {
            let _ = t.set_dial(a, hz(14_230_000));
        }),
        ("A's dial to 40 m", |t, a, _| {
            let _ = t.set_dial(a, hz(7_150_000));
        }),
        ("A's mode", |t, a, _| {
            let _ = t.set_mode(a, &Word::new("lsb").expect("a mode word"));
        }),
        ("A's CW", |t, a, _| {
            let _ = t.cw(a, Cw::Macro(Text::new("CQ").expect("a CW text")));
        }),
        ("A's audio stopped and started", |t, a, _| {
            t.audio_stop(a);
            let _ = t.audio_start(a, AudioFormat::default());
        }),
        ("B's trx:0,false", |t, _, b| t.release(b)),
        ("A's trx:0,false", |t, a, _| t.release(a)),
        ("B's tune:0,false", |t, _, b| t.tune_off(b)),
        ("A's cw_macros_stop", |t, a, _| t.cw_stop(a)),
        ("A's stop;", |t, a, _| t.end_own(a)),
        ("B lost", |t, _, b| t.disconnected(b)),
    ];
    type Build = fn() -> SharedEngine;
    let states: Vec<(&str, Build)> = vec![
        ("at launch: Digital, transmit off", || {
            Arc::new(Mutex::new(Engine::with_settings(Settings {
                mycall: "KD9TAW".into(),
                mygrid: "EN52".into(),
                tci_server_tx: true,
                ..Settings::default()
            })))
        }),
        ("after Stop TX", || {
            let s = phone();
            engine_lock(&s).halt_tx();
            s
        }),
        ("after TX Off", || {
            let s = phone();
            engine_lock(&s).set_tx_enabled_by_operator(false);
            s
        }),
        ("after a watchdog trip", || {
            let s = phone();
            engine_lock(&s).trip_tx_watchdog_for_test();
            s
        }),
        ("after a band change", || {
            let s = phone();
            engine_lock(&s).set_frequency(7.200, "40m", "LSB");
            s
        }),
        ("outside the licence", || {
            let s = phone();
            engine_lock(&s).set_frequency(14.349, "20m", "USB");
            s
        }),
        ("open, at half power", || {
            let s = phone();
            engine_lock(&s).set_rf_power(0.5);
            s
        }),
    ];
    // What no step may move upward.
    let seen = |e: &Engine| {
        (
            e.tx_enabled(),
            e.snapshot().radio.tx_watchdog,
            e.effective_rf_power(),
        )
    };
    let (mut wrong, mut admitted, mut lowered) = (Vec::new(), 0, 0);
    for (state, build) in states {
        let engine = build();
        let (m, _feed) = serving_with_audio(&engine);
        let h = handle(&m);
        let (_a, a) = listening(&h, port(&m));
        let (_b, b, _) = program(&h, port(&m));
        let tci = m.backend.clone();
        for (name, step) in &steps {
            let before = seen(&engine_lock(&engine));
            step(&tci, a, b);
            let e = engine_lock(&engine);
            let after = seen(&e);
            if !before.0 && after.0 {
                wrong.push(format!("{state}: {name} armed transmit"));
            }
            if before.1 && !after.1 {
                wrong.push(format!("{state}: {name} let go of the watchdog trip"));
            }
            if after.2.unwrap_or(0.0) > before.2.unwrap_or(0.0) {
                wrong.push(format!(
                    "{state}: {name} raised the power: {before:?} -> {after:?}"
                ));
            }
            let keyed =
                e.manual_ptt() || e.tuning() || e.tci_over_armed() || e.tci_owner().is_some();
            if !e.tx_allowed() && keyed {
                wrong.push(format!(
                    "{state}: {name} left a program keyed outside the licence"
                ));
            }
            admitted += usize::from(e.tci_owner().is_some());
            lowered += usize::from(after.2 < before.2);
        }
        let mut e = engine_lock(&engine);
        match state {
            "after Stop TX" | "after TX Off" => {
                e.set_operating_mode("phone", false);
                if e.tx_enabled() {
                    wrong.push(format!("{state}: the operator's Stop no longer holds"));
                }
            }
            "after a band change" => {
                drop(e);
                let probe = tci.key(a, KeyKind::Tci, true);
                if probe != Err(Refusal::ContextHold) {
                    wrong.push(format!("{state}: the band change's hold let go: {probe:?}"));
                }
            }
            _ => {}
        }
    }
    assert!(
        wrong.is_empty(),
        "a program moved them:\n{}",
        wrong.join("\n")
    );
    assert!(admitted > 0, "control: no program's key was ever admitted");
    assert!(lowered > 0, "control: no program ever lowered the power");
}

/// ⛔ STOP IN NEXUS ALWAYS WINS. Stop TX, TX Off and the switch going off
/// each end a program's keyed over at once (the abort armed, so the loop flushes and unkeys); at
/// the manager's next publish the server lets go of its key, tells it `trx:0,false;`, takes no more
/// of its audio and asks for none; its next key is refused, as TCI tells a key that did not take,
/// until the operator's own arm, which then admits it.
#[test]
fn every_stop_in_nexus_ends_a_programs_over_and_the_server_takes_no_more_of_its_audio() {
    type Stop = fn(&SharedEngine);
    type Arm = fn(&SharedEngine);
    let stops: [(&str, Stop, Refusal, Arm); 3] = [
        (
            "Stop TX",
            |s| engine_lock(s).halt_tx(),
            Refusal::StopHold,
            |s| engine_lock(s).set_tx_enabled_by_operator(true),
        ),
        (
            "TX Off",
            |s| engine_lock(s).set_tx_enabled_by_operator(false),
            Refusal::StopHold,
            |s| engine_lock(s).set_tx_enabled_by_operator(true),
        ),
        (
            "the switch off",
            |s| programs_may_transmit(s, false),
            Refusal::Switch,
            |s| programs_may_transmit(s, true),
        ),
    ];
    for (name, stop, refusal, arm) in stops {
        let engine = phone();
        let (mut m, _feed) = serving_with_audio(&engine);
        let h = handle(&m);
        let (mut a, a_id) = voice_program(&h, port(&m));
        key_over(&engine, &mut m, &mut a);
        stop(&engine);
        {
            let mut e = engine_lock(&engine);
            assert!(!e.tci_over_armed(), "{name}: the over survived");
            assert!(e.take_tci_abort(), "{name}: no abort to flush and unkey");
            assert_eq!(e.tci_owner(), None, "{name}: still the owner");
        }
        m.publish();
        assert_eq!(
            listed(&h, a_id).key,
            None,
            "{name}: the server still holds the program's key"
        );
        assert_eq!(
            *lock(&m.armed),
            None,
            "{name}: the pump still asks for audio"
        );
        a.lines_until("trx:0,false;");
        let before = listed(&h, a_id).counts;
        a.send_binary(tx_frame(1024, 0.5));
        in_order(&mut a, "usb");
        let after = listed(&h, a_id).counts;
        assert_eq!(
            (after.tx_frames, after.tx_dropped),
            (before.tx_frames, before.tx_dropped + 1),
            "{name}: the server took the program's audio after the stop"
        );
        assert_eq!(loop_tick(&engine), MicTick::Idle, "{name}: something keyed");
        let _ = chronos(&mut a, Duration::from_millis(100));
        assert!(
            chronos(&mut a, Duration::from_millis(200)).is_empty(),
            "{name}: TX_CHRONO went on after the stop"
        );

        a.send("trx:0,true,tci;");
        a.lines_until("trx:0,false;");
        assert_eq!(
            m.backend.key(a_id, KeyKind::Tci, true),
            Err(refusal),
            "{name}: a key after it"
        );
        assert!(
            !engine_lock(&engine).tci_over_armed(),
            "{name}: a key after the stop armed an over"
        );
        arm(&engine);
        a.send("trx:0,true,tci;");
        a.lines_until("trx:0,true;");
        assert_eq!(
            engine_lock(&engine).tci_owner(),
            Some(a_id.get()),
            "{name}: control, after the operator's own arm"
        );
    }
}

/// ⛔ A PROGRAM'S LOST CONNECTION ENDS WHAT IT HELD BEFORE THE RADIO LOOP'S NEXT TICK:
/// its keyed over is gone with the abort armed by the server's own report of the loss,
/// so that tick flushes and unkeys; a lost program's microphone key is down at once. CONTROL:
/// another program's loss leaves the over on the air.
#[test]
fn a_programs_lost_connection_ends_what_it_held_before_the_next_tick() {
    let engine = phone();
    let (mut m, _feed) = serving_with_audio(&engine);
    let h = handle(&m);
    let (mut a, _) = voice_program(&h, port(&m));
    let (other, other_id, _) = program(&h, port(&m));
    key_over(&engine, &mut m, &mut a);
    drop(other);
    let until = Instant::now() + WAIT;
    while h.programs().iter().any(|p| p.id == other_id) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    {
        let mut e = engine_lock(&engine);
        assert!(
            e.tci_over_keyed() && !e.take_tci_abort(),
            "control: another program's loss ended the over"
        );
    }
    drop(a);
    assert!(
        eventually(&engine, |e| e.tci_owner().is_none()),
        "the lost program's over was not ended"
    );
    {
        let mut e = engine_lock(&engine);
        assert!(
            !e.tci_over_armed(),
            "the lost program's over is still armed"
        );
        assert!(e.take_tci_abort(), "no abort for the loop's next tick");
    }
    assert_eq!(loop_tick(&engine), MicTick::Idle, "the next tick keyed");

    let (mut c, _) = listening(&h, port(&m));
    c.send("trx:0,true;");
    c.lines_until("trx:0,true;");
    assert!(
        engine_lock(&engine).manual_ptt(),
        "premise: its microphone keys the radio"
    );
    drop(c);
    assert!(
        eventually(&engine, |e| !e.manual_ptt() && e.tci_owner().is_none()),
        "the lost program's microphone key is still held"
    );
}

/// A program's Tune is Nexus's own tune carrier, through the engine's verdict: told `tune:0,true;`,
/// the program owns it; `tune:0,false;` from any program ends it, and so does the program's own
/// `stop;`. Refused (the switch off), it is told as TCI tells a Tune that did not take.
#[test]
fn a_programs_tune_is_nexus_tune_through_the_engines_verdict() {
    let engine = phone();
    let (mut m, _feed) = serving_with_audio(&engine);
    let h = handle(&m);
    let (mut a, a_id) = listening(&h, port(&m));
    let (mut b, _, _) = program(&h, port(&m));
    a.send("tune:0,true;");
    a.lines_until("tune:0,true;");
    {
        let e = engine_lock(&engine);
        assert!(e.tuning(), "the program's Tune is not up");
        assert_eq!(e.tci_owner(), Some(a_id.get()), "not the program's Tune");
    }
    b.send("tune:0,false;");
    in_order(&mut b, "usb");
    assert!(
        !engine_lock(&engine).tuning(),
        "another program's tune:0,false left it up"
    );
    m.publish();
    a.lines_until("tune:0,false;");
    a.send("tune:0,true;");
    a.lines_until("tune:0,true;");
    a.send("stop;");
    in_order(&mut a, "usb");
    assert!(!engine_lock(&engine).tuning(), "its own stop; left it up");
    m.publish();
    a.lines_until("tune:0,false;");

    programs_may_transmit(&engine, false);
    a.send("tune:0,true;");
    a.lines_until("tune:0,false;");
    let e = engine_lock(&engine);
    assert!(
        !e.tuning() && e.tci_owner().is_none(),
        "switched off, a program's Tune went up"
    );
}

/// `stop;` ends the sender's own key and nothing else (the spec's §4.2): another program's `stop;`
/// leaves a program's microphone key held; its own ends it at once.
#[test]
fn a_programs_stop_ends_its_own_key_and_no_other() {
    let engine = phone();
    let (m, _feed) = serving_with_audio(&engine);
    let h = handle(&m);
    let (mut a, a_id) = listening(&h, port(&m));
    let (mut b, _, _) = program(&h, port(&m));
    a.send("trx:0,true;");
    a.lines_until("trx:0,true;");
    b.send("stop;");
    in_order(&mut b, "usb");
    assert_eq!(
        engine_lock(&engine).tci_owner(),
        Some(a_id.get()),
        "control: another program's stop; ended the key"
    );
    a.send("stop;");
    in_order(&mut a, "usb");
    let e = engine_lock(&engine);
    assert_eq!(e.tci_owner(), None, "its own stop; left its key");
    assert!(!e.manual_ptt(), "its own stop; left the radio keyed");
    assert!(
        e.tx_enabled(),
        "a program's stop; is not the station's Stop"
    );
}

/// Run `f` on its own thread: its answer, or `None` if it has not answered within `bound` (it is
/// left running: a wait on a lock its own caller holds never ends).
fn within<T: Send + 'static>(bound: Duration, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(bound).ok()
}

/// ⛔ THE LOCK RULE (tempo-net's): the server's stop and the operator's Disconnect each end a
/// program's transmissions through this backend, which takes the engine lock, so neither may run
/// under it. The manager's tick that finds "Share by TCI" off stops the server, and the operator's
/// Disconnect closes a program, each while a program holds a key: each returns, and the key is
/// ended. A stop under the engine lock would wait on itself for ever.
#[test]
fn the_servers_stop_and_the_disconnect_end_a_key_without_the_engine_lock() {
    let engine = phone();
    let (m, _feed) = serving_with_audio(&engine);
    let h = handle(&m);
    let (mut a, a_id) = listening(&h, port(&m));
    a.send("trx:0,true;");
    a.lines_until("trx:0,true;");
    let serving = m.serving.clone();
    assert_eq!(
        within(WAIT, move || serving.disconnect(a_id.get())),
        Some(true),
        "the operator's Disconnect did not return"
    );
    assert_eq!(
        engine_lock(&engine).tci_owner(),
        None,
        "the Disconnect left the program's key"
    );

    let engine = phone();
    assert!(
        !engine_lock(&engine).settings().tci_server,
        "premise: Share by TCI is off"
    );
    let (m, _feed) = serving_with_audio(&engine);
    let h = handle(&m);
    let (mut a, a_id) = listening(&h, port(&m));
    a.send("trx:0,true;");
    a.lines_until("trx:0,true;");
    assert_eq!(
        engine_lock(&engine).tci_owner(),
        Some(a_id.get()),
        "premise"
    );
    let mut m = m;
    assert_eq!(
        within(WAIT, move || m.tick()),
        Some(false),
        "the manager's stop did not return, or left the server running"
    );
    assert_eq!(
        engine_lock(&engine).tci_owner(),
        None,
        "the server's stop left the program's key"
    );
}

/// "Share by TCI" and its port are the station's settings: off (its default) the manager serves
/// nothing; on, it serves on the port asked; off again, it stops.
#[test]
fn the_server_follows_share_by_tci_and_its_port_in_the_settings() {
    let engine = armed();
    let mut m = Manager::new(
        engine.clone(),
        Arc::new(ReceiveAudioFeed::default()),
        MeterFeed::default(),
    );
    assert!(!m.tick(), "a station that never turned it on serves");
    let quiet = crate::lan_client::tests::listener_below_the_ephemeral_range(Ipv4Addr::LOCALHOST);
    let asked = quiet.local_addr().expect("a port").port();
    drop(quiet);
    let share = |on: bool| {
        let mut e = engine_lock(&engine);
        let mut s = e.settings().clone();
        s.tci_server = on;
        s.tci_server_port = asked;
        e.apply_settings(s);
    };
    share(true);
    assert!(m.tick(), "switched on, it does not serve");
    let at = port(&m);
    assert!(
        at == asked || at == asked + 1,
        "the port asked, or the next: {at}"
    );
    share(false);
    assert!(!m.tick(), "switched off, it serves");
    assert!(
        TcpStream::connect((Ipv4Addr::LOCALHOST, at)).is_err(),
        "switched off, something listens"
    );
}

/// ⛔ THE OPERATOR'S RULING OF 2026-10-19, THROUGH THE SERVER: FT8, FT4 and the other slot-timed
/// modes stay refused until the loop bench. In Digital on FT8, armed, with the switch on, a
/// program's over is answered `trx:0,false;`, the log says why, its `tx_enable` stays false and
/// nothing is armed. CONTROL: the same program in Phone is admitted (it asked a voice mode).
#[test]
fn slot_timed_modes_stay_refused_through_the_server() {
    let engine = armed();
    programs_may_transmit(&engine, true);
    let (m, _feed) = serving_with_audio(&engine);
    let h = handle(&m);
    let (mut a, a_id) = voice_program(&h, port(&m));
    a.send("trx:0,true,tci;");
    a.lines_until("trx:0,false;");
    assert_eq!(
        m.backend.key(a_id, KeyKind::Tci, true),
        Err(Refusal::SlotTimed)
    );
    assert!(logged(&format!(
        "refused {a_id}: {}",
        Refusal::SlotTimed.text()
    )));
    assert!(
        !told_tx_enable(&engine, &h, a_id),
        "told it may transmit FT8"
    );
    {
        let e = engine_lock(&engine);
        assert!(
            !e.tci_over_armed() && e.tx_owner().is_none(),
            "an FT8 over armed"
        );
    }
    {
        let mut e = engine_lock(&engine);
        e.set_operating_mode("phone", false);
        e.set_frequency(14.250, "20m", "USB");
    }
    a.send("trx:0,true,tci;");
    a.lines_until("trx:0,true;");
    assert_eq!(
        engine_lock(&engine).tci_owner(),
        Some(a_id.get()),
        "control: Phone admits it"
    );
}

/// ⛔ THE OPERATOR'S RULING OF 2026-10-20, THROUGH THE SERVER: a data-mode over from a program is
/// held like FT8, in any section. WSJT-X's shape, `modulation:0,digu;` and then `trx:0,true,tci;`,
/// with Nexus on the Phone screen at 14.074 and the licence class Open, where no other gate
/// refuses a phone emission: answered `trx:0,false;`, the program's last refusal is the slot-timed
/// one, it is not told it may transmit, and nothing is armed. CONTROLS: another program that asked
/// `modulation:0,usb;` keys as before; and the first program's later `modulation:0,usb;` lets its
/// next voice over key.
#[test]
fn a_programs_data_mode_over_is_held_like_ft8_through_the_server() {
    let engine = phone();
    {
        let mut e = engine_lock(&engine);
        e.set_license_class("open");
        e.set_frequency(14.074, "20m", "USB");
        assert!(
            e.tx_allowed(),
            "premise: the Open class keys on 14.074 in Phone"
        );
    }
    let (mut m, _feed) = serving_with_audio(&engine);
    let h = handle(&m);
    let (mut a, a_id) = listening(&h, port(&m));
    // The server runs a program's commands in order: its key is judged after its modulation.
    a.send("modulation:0,digu;");
    a.send("trx:0,true,tci;");
    a.lines_until("trx:0,false;");
    m.publish();
    let refused = engine_lock(&engine)
        .snapshot()
        .radio
        .tci_server
        .and_then(|s| s.programs.into_iter().find(|p| p.id == a_id.get()))
        .and_then(|p| p.refused);
    assert_eq!(
        refused.as_deref(),
        Some(Refusal::SlotTimed.text()),
        "the program's last refusal"
    );
    assert!(
        !told_tx_enable(&engine, &h, a_id),
        "told it may transmit a data-mode over"
    );
    assert!(
        !engine_lock(&engine).tci_over_armed(),
        "a data-mode over armed"
    );
    // CONTROL: another program that asked usb keys as before.
    let (mut b, b_id) = listening(&h, port(&m));
    b.send("modulation:0,usb;");
    b.send("trx:0,true,tci;");
    assert!(
        eventually(&engine, |e| e.tci_owner() == Some(b_id.get())),
        "control: a program that asked usb was not admitted"
    );
    b.send("trx:0,false;");
    assert!(
        eventually(&engine, |e| e.tci_owner().is_none()),
        "premise: B let go"
    );
    // CONTROL: the first program's later usb lets its next voice over key.
    a.send("modulation:0,usb;");
    a.send("trx:0,true,tci;");
    assert!(
        eventually(&engine, |e| e.tci_owner() == Some(a_id.get())),
        "control: after its usb, its voice over was not admitted"
    );
}

/// ⛔ THE OPERATOR'S RULING OF 2026-10-20, THROUGH THE SERVER: a program's own audio goes out only
/// after it asks a voice mode. WSJT-X's default shape (its Radio mode "None": no `modulation:` at
/// all), its audio started and then `trx:0,true,tci;`, with Nexus on the Phone screen at 14.074 and
/// the licence class Open, where no other gate refuses a phone emission: answered `trx:0,false;`,
/// the program's last refusal says it must ask a voice mode, it is not told it may transmit, and
/// nothing is armed. CONTROL: the same program's later `modulation:0,usb;` lets its next over key.
#[test]
fn a_programs_over_with_no_voice_mode_asked_is_refused_through_the_server() {
    let engine = phone();
    {
        let mut e = engine_lock(&engine);
        e.set_license_class("open");
        e.set_frequency(14.074, "20m", "USB");
        assert!(
            e.tx_allowed(),
            "premise: the Open class keys on 14.074 in Phone"
        );
    }
    let (mut m, _feed) = serving_with_audio(&engine);
    let h = handle(&m);
    let (mut a, a_id) = listening(&h, port(&m));
    a.send("trx:0,true,tci;");
    a.lines_until("trx:0,false;");
    m.publish();
    let refused = engine_lock(&engine)
        .snapshot()
        .radio
        .tci_server
        .and_then(|s| s.programs.into_iter().find(|p| p.id == a_id.get()))
        .and_then(|p| p.refused);
    assert_eq!(
        refused.as_deref(),
        Some(Refusal::VoiceMode.text()),
        "the program's last refusal"
    );
    assert!(
        !told_tx_enable(&engine, &h, a_id),
        "told it may transmit with no voice mode asked"
    );
    assert!(
        !engine_lock(&engine).tci_over_armed(),
        "an over with no voice mode asked armed"
    );
    // CONTROL: its later usb lets its next over key.
    a.send("modulation:0,usb;");
    a.send("trx:0,true,tci;");
    assert!(
        eventually(&engine, |e| e.tci_owner() == Some(a_id.get())),
        "control: after its usb, its voice over was not admitted"
    );
}

/// A station in the CW section on 40 m with every privilege, transmit armed by the operator after
/// the move from 20 m (a band change holds programs' keys until that arm), the operator's own CW at
/// 20 WPM, and "Programs on TCI may transmit" on.
fn cw_station() -> SharedEngine {
    let mut e = Engine::with_settings(Settings {
        mycall: "KD9TAW".into(),
        mygrid: "EN52".into(),
        tci_server_tx: true,
        ..Settings::default()
    });
    e.set_license_class("extra");
    e.set_operating_mode("cw", false);
    e.set_frequency(7.030, "40m", "CW");
    e.set_tx_enabled_by_operator(true);
    // The band change's halt left its one-shot cuts; taken, as the radio loop's next tick would,
    // so a cut a test sees is its own.
    let _ = (e.take_cw_abort(), e.take_slot_tx_abort());
    e.set_cw_wpm(20);
    assert!(
        e.tx_enabled() && e.tci_transmit() && !e.on_air(),
        "premise: CW armed on 7.030, programs may transmit, nothing on the air"
    );
    Arc::new(Mutex::new(e))
}

/// The radio loop takes the next CW word and reports the keyer sending it, for a minute: the
/// arbiter holds it on the air until something ends it. The word and its speed.
fn cw_word_on_the_air(engine: &SharedEngine) -> (String, u32) {
    let mut e = engine_lock(engine);
    let (word, wpm, _) = e.poll_cw_word().expect("premise: a CW word to key");
    e.observe_cw_keying(Some(60_000.0));
    (word, wpm)
}

/// Every CW word the radio loop would key from now on, with its speed.
fn cw_words_left(engine: &SharedEngine) -> Vec<(String, u32)> {
    let mut e = engine_lock(engine);
    std::iter::from_fn(|| e.poll_cw_word().map(|(word, wpm, _)| (word, wpm))).collect()
}

/// ⛔ A PROGRAM'S CW THROUGH THE SERVER, on the wire against a real engine: in the CW
/// section with the switch on, `cw_macros_speed:30;` is answered and sets the programs' speed only
/// (the operator's stays 20, and another program reads 30); `cw_macros` goes to Nexus's CW keyer at
/// that speed, as that program's CW, and programs are told `trx:0,true;` while it is on the air;
/// another program's `cw_macros_stop;` cuts it mid-word; a text holding a `;` (sent as `*`) is
/// refused whole and named in the log; a program's lost connection cuts its own CW mid-word.
/// CONTROL: with the switch off, a macro queues nothing.
#[test]
fn a_programs_cw_goes_through_the_engine_at_its_own_speed_and_its_stops_cut_it() {
    let engine = cw_station();
    let (mut m, _feed) = serving(&engine);
    let h = handle(&m);
    let (mut a, a_id, _) = program(&h, port(&m));
    let (mut b, _, _) = program(&h, port(&m));

    a.send("cw_macros_speed:30;");
    a.lines_until("cw_macros_speed:30;");
    assert_eq!(
        engine_lock(&engine).cw_wpm(),
        20,
        "the operator's speed moved"
    );
    m.publish();
    b.send("cw_macros_speed;");
    b.lines_until("cw_macros_speed:30;");

    a.send("cw_macros:0,cq test de kd9taw;");
    in_order(&mut a, "cw");
    assert_eq!(
        engine_lock(&engine).tci_owner(),
        Some(a_id.get()),
        "the program's CW is its own"
    );
    m.publish();
    a.lines_until("trx:0,true;");
    assert_eq!(cw_word_on_the_air(&engine), ("CQ".to_string(), 30));
    b.send("cw_macros_stop;");
    in_order(&mut b, "cw");
    {
        let mut e = engine_lock(&engine);
        assert!(
            e.take_cw_abort(),
            "cw_macros_stop: the word on the air keys on"
        );
        assert_eq!(e.tx_owner(), None, "cw_macros_stop: still owned");
    }
    assert_eq!(cw_words_left(&engine), Vec::new(), "words still to go");

    a.send("cw_macros:0,TEST*FA00007000000;");
    in_order(&mut a, "cw");
    assert_eq!(cw_words_left(&engine), Vec::new(), "a ; reached the keyer");
    assert!(
        logged(Refusal::CwText.text()),
        "the refused text is not in the connection log"
    );

    a.send("cw_macros:0,CQ TEST;");
    in_order(&mut a, "cw");
    assert_eq!(cw_word_on_the_air(&engine), ("CQ".to_string(), 30));
    drop(a);
    // The server tells the backend of the loss before it frees the program's slot.
    let until = Instant::now() + WAIT;
    while h.programs().iter().any(|p| p.id == a_id) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    {
        let mut e = engine_lock(&engine);
        assert!(e.take_cw_abort(), "the lost program's word keys on");
        assert_eq!(
            e.tx_owner(),
            None,
            "the lost program's CW is still on the air"
        );
    }
    assert_eq!(
        cw_words_left(&engine),
        Vec::new(),
        "the lost program's words to go"
    );

    programs_may_transmit(&engine, false);
    b.send("cw_macros:0,CQ;");
    in_order(&mut b, "cw");
    assert_eq!(
        cw_words_left(&engine),
        Vec::new(),
        "control: the switch off"
    );
}

// ── Programs' spots, and the operator's clicks told to programs ─────────────────────────────

/// A station sharing by TCI with "Spots from programs" on (as it comes) and TX off, as at launch,
/// on 20 m. Its port is the one `serving` asks for (any free one), so the manager's tick keeps the
/// server where it is.
fn spotting() -> SharedEngine {
    let e = Engine::with_settings(Settings {
        mycall: "KD9TAW".into(),
        mygrid: "EN52".into(),
        operating_mode: OperatingMode::Digital,
        band: "20m".into(),
        dial_mhz: 14.074,
        sideband: "USB".into(),
        tci_server: true,
        tci_server_port: 0,
        ..Settings::default()
    });
    assert!(
        e.tci_server_spots_on() && !e.tx_enabled(),
        "premise: programs' spots on as they come, TX off"
    );
    Arc::new(Mutex::new(e))
}

/// Everything the program sent before this has run: a program's commands run in order.
fn settled(client: &mut Client) {
    client.send("trx:0;");
    client.lines_until("trx:0,false;");
}

/// `(program, call, Hz, text)` of each program's spot the band map reads, in that order.
fn on_the_map(engine: &SharedEngine) -> Vec<(u64, String, i64, String)> {
    let mut spots: Vec<_> = engine_lock(engine)
        .tci_program_spots(crate::now_unix())
        .into_iter()
        .map(|p| (p.program, p.spot.call, p.spot.hz, p.spot.text))
        .collect();
    spots.sort();
    spots
}

/// Change the station's settings as a save does.
fn set(engine: &SharedEngine, f: impl FnOnce(&mut Settings)) {
    let mut e = engine_lock(engine);
    let mut s = e.settings().clone();
    f(&mut s);
    e.apply_settings(s);
}

/// The last thing the station refused program `id`, as the server's line in the snapshot says it
/// once published.
fn last_refused(engine: &SharedEngine, m: &mut Manager, id: ClientId) -> Option<String> {
    m.publish();
    engine_lock(engine)
        .snapshot()
        .radio
        .tci_server
        .and_then(|s| s.programs.into_iter().find(|p| p.id == id.get()))
        .and_then(|p| p.refused)
}

/// The server's generation for programs' spots, as the snapshot carries it once published.
fn spots_generation(engine: &SharedEngine, m: &mut Manager) -> u64 {
    m.publish();
    engine_lock(engine)
        .snapshot()
        .radio
        .tci_server
        .expect("serving")
        .spots
}

/// ⛔ A PROGRAM'S SPOTS ARE MARKS ON NEXUS'S BAND MAP WITH THE PROGRAM THAT SENT THEM, and nothing
/// more: the band map reads each with its program's number, as a spot row whose `tci` names it
/// (`get_tci_spots`), its text as the comment, kept as the characters it is; the snapshot's
/// generation moves on so a window asks again; nothing tuned, nothing armed. CONTROL: before them
/// the band map reads none.
#[test]
fn programs_spots_reach_the_band_map_with_the_program_that_sent_them() {
    let engine = spotting();
    let (mut m, _feed) = serving(&engine);
    let h = handle(&m);
    let (mut a, a_id, _) = program(&h, port(&m));
    let (mut b, b_id, _) = program(&h, port(&m));
    let before = spots_generation(&engine, &mut m);
    assert!(on_the_map(&engine).is_empty(), "control: none yet");
    a.send("spot:K1ABC,CW,14025000,4294901760,CQ NA up 2;");
    a.send("spot:W1AW,CW,7030000,4278190335,<b>QST</b> <img src=x onerror=alert(1)>;");
    b.send("spot:k1abc,CW,14025500,4278255360,from the skimmer;");
    settled(&mut a);
    settled(&mut b);
    let (a_n, b_n) = (a_id.get(), b_id.get());
    assert_eq!(
        on_the_map(&engine),
        [
            (a_n, "K1ABC".into(), 14_025_000, "CQ NA up 2".into()),
            (
                a_n,
                "W1AW".into(),
                7_030_000,
                "<b>QST</b> <img src=x onerror=alert(1)>".into()
            ),
            (b_n, "K1ABC".into(), 14_025_500, "from the skimmer".into()),
        ]
    );
    assert!(
        spots_generation(&engine, &mut m) > before,
        "the snapshot says they changed"
    );
    let now = crate::now_unix();
    let rows: Vec<serde_json::Value> = engine_lock(&engine)
        .tci_program_spots(now)
        .into_iter()
        .filter(|p| p.program == a_n && p.spot.call == "K1ABC")
        .map(|p| {
            serde_json::to_value(crate::program_spot_row(
                p,
                tempo_app::settings::LicenseClass::Open,
                now,
            ))
            .expect("a row")
        })
        .collect();
    let [row] = &rows[..] else {
        panic!("one row: {rows:?}");
    };
    for (key, want) in [
        ("tci", serde_json::json!(a_n)),
        ("call", serde_json::json!("K1ABC")),
        ("band", serde_json::json!("20m")),
        ("freqMhz", serde_json::json!(14.025)),
        ("mode", serde_json::json!("CW")),
        ("comment", serde_json::json!("CQ NA up 2")),
        ("spotter", serde_json::json!("")),
    ] {
        assert_eq!(row[key], want, "{key} in {row}");
    }
    let e = engine_lock(&engine);
    assert!(
        (e.settings().dial_mhz - 14.074).abs() < 1e-9 && !e.tx_enabled() && !e.on_air(),
        "a spot tuned or armed"
    );
}

/// ⛔ `spot_delete` AND `spot_clear` ARE THE SENDER'S OWN: another program's delete of the same
/// callsign and its clear leave a program's spots on the map. CONTROL: the program's own delete
/// takes its own.
#[test]
fn another_programs_delete_and_clear_leave_a_programs_spots() {
    let engine = spotting();
    let (m, _feed) = serving(&engine);
    let h = handle(&m);
    let (mut a, a_id, _) = program(&h, port(&m));
    let (mut b, b_id, _) = program(&h, port(&m));
    for client in [&mut a, &mut b] {
        client.send("spot:K1ABC,CW,14025000,1,a;");
        client.send("spot:W1AW,CW,14030000,1,b;");
        settled(client);
    }
    assert_eq!(on_the_map(&engine).len(), 4, "premise: two each");
    b.send("spot_delete:K1ABC;");
    settled(&mut b);
    let calls = |n: u64| -> Vec<String> {
        on_the_map(&engine)
            .into_iter()
            .filter(|s| s.0 == n)
            .map(|s| s.1)
            .collect()
    };
    assert_eq!(calls(a_id.get()), ["K1ABC", "W1AW"], "B's delete took A's");
    assert_eq!(calls(b_id.get()), ["W1AW"]);
    b.send("spot_clear;");
    settled(&mut b);
    assert_eq!(calls(a_id.get()), ["K1ABC", "W1AW"], "B's clear took A's");
    assert!(calls(b_id.get()).is_empty());
    a.send("spot_delete:W1AW;");
    settled(&mut a);
    assert_eq!(calls(a_id.get()), ["K1ABC"], "control: A's own delete");
}

/// ⛔ A PROGRAM'S SPOTS GO WHEN IT GOES, whether its connection is lost or the operator disconnects
/// it, and another program's stay.
#[test]
fn a_programs_spots_go_when_it_disconnects_and_the_others_stay() {
    let engine = spotting();
    let (m, _feed) = serving(&engine);
    let h = handle(&m);
    let (mut a, a_id, _) = program(&h, port(&m));
    let (mut b, b_id, _) = program(&h, port(&m));
    let (mut c, c_id, _) = program(&h, port(&m));
    for client in [&mut a, &mut b, &mut c] {
        client.send("spot:K1ABC,CW,14025000,1,x;");
        settled(client);
    }
    assert_eq!(on_the_map(&engine).len(), 3, "premise");
    drop(a);
    let gone = |n: u64| {
        eventually(&engine, |e| {
            e.tci_program_spots(crate::now_unix())
                .iter()
                .all(|s| s.program != n)
        })
    };
    assert!(gone(a_id.get()), "the lost program's spot stayed");
    assert!(m.serving.disconnect(b_id.get()));
    assert!(gone(b_id.get()), "the disconnected program's spot stayed");
    assert_eq!(
        on_the_map(&engine),
        [(c_id.get(), "K1ABC".into(), 14_025_000, "x".into())],
        "the program still connected keeps its spot"
    );
    settled(&mut c);
}

/// ⛔ EVERY PROGRAM'S SPOTS GO WITH THE SWITCH AND WITH THE SERVER: "Spots from programs" off takes
/// them from the map at once and from the store at the manager's next tick, and refuses the next
/// one as not offered, as the program's last refusal says; on again, a program's spot is kept
/// again; the server stopping takes them all.
#[test]
fn every_programs_spots_go_with_the_switch_and_with_the_server() {
    let engine = spotting();
    let (mut m, _feed) = serving(&engine);
    let h = handle(&m);
    let (mut a, a_id, _) = program(&h, port(&m));
    a.send("spot:K1ABC,CW,14025000,1,x;");
    settled(&mut a);
    assert_eq!(on_the_map(&engine).len(), 1, "premise");
    set(&engine, |s| s.tci_server_spots = false);
    assert!(on_the_map(&engine).is_empty(), "the map reads none at once");
    assert!(m.tick(), "the server keeps serving");
    assert_eq!(
        engine_lock(&engine).tci_program_spot_store().iter().count(),
        0,
        "the tick took them"
    );
    a.send("spot:W1AW,CW,14030000,1,y;");
    settled(&mut a);
    assert!(on_the_map(&engine).is_empty(), "refused while off");
    assert_eq!(
        last_refused(&engine, &mut m, a_id).as_deref(),
        Some("not offered"),
        "the station says why"
    );
    set(&engine, |s| s.tci_server_spots = true);
    a.send("spot:W1AW,CW,14030000,1,y;");
    settled(&mut a);
    assert_eq!(on_the_map(&engine).len(), 1, "on again, kept");
    m.apply(None);
    assert_eq!(
        engine_lock(&engine).tci_program_spot_store().iter().count(),
        0,
        "the server's stop took them, the switches still on"
    );
}

/// ⛔ WHAT A PROGRAM SENDS IS DATA, CHECKED AND COUNTED: a callsign that is not one and a frequency
/// outside the range programs are told are refused, each said as the program's last refusal, and
/// counted; a text past 64 characters is cut and counted; a control character fails TCI's own
/// reading, so the command never arrives and the server counts it unreadable.
#[test]
fn a_spot_that_is_not_one_is_refused_counted_and_said() {
    let engine = spotting();
    let (mut m, _feed) = serving(&engine);
    let h = handle(&m);
    let (mut a, a_id, _) = program(&h, port(&m));
    let unreadable = |h: &Handle| {
        h.programs()
            .iter()
            .find(|p| p.id == a_id)
            .map(|p| p.counts.unreadable)
            .unwrap_or_default()
    };
    let before = unreadable(&h);
    a.send("spot:HELLO,CW,14025000,1,x;");
    settled(&mut a);
    assert_eq!(
        last_refused(&engine, &mut m, a_id).as_deref(),
        Some("a spot whose callsign is not one")
    );
    a.send("spot:K1ABC,CW,20000,1,x;");
    settled(&mut a);
    assert_eq!(
        last_refused(&engine, &mut m, a_id).as_deref(),
        Some("outside the radio's range")
    );
    a.send(&format!("spot:K2ABC,CW,14026000,1,{};", "X".repeat(100)));
    a.send("spot:K3ABC,CW,14027000,1,bell\u{7}here;");
    settled(&mut a);
    assert_eq!(
        on_the_map(&engine),
        [(a_id.get(), "K2ABC".into(), 14_026_000, "X".repeat(64))],
        "only the long text, cut"
    );
    let e = engine_lock(&engine);
    let store = e.tci_program_spot_store();
    assert_eq!(
        (store.refused(), store.cut()),
        (2, 1),
        "refused and cut, counted"
    );
    drop(e);
    assert_eq!(unreadable(&h), before + 1, "the control character, counted");
}

/// Every program's lines up to `rx_clicked_on_spot:0,0,<call>,<hz>;`.
fn told_click(client: &mut Client, call: &str, hz: i64) -> Vec<String> {
    client.lines_until(&format!("rx_clicked_on_spot:0,0,{call},{hz};"))
}

/// ⛔ A CLICK ON ANY SPOT IN NEXUS IS TOLD TO EVERY PROGRAM IN BOTH SPELLINGS, and nothing else
/// happens for it: a cluster spot worked (the Work's own section entry is the station's, not the
/// click's) and a program's spot clicked, each told at the manager's next tick as
/// `clicked_on_spot:<call>,<hz>;` and `rx_clicked_on_spot:0,0,<call>,<hz>;` at the spot's own
/// frequency. The program's spot's click tunes and arms nothing, starts no QSO and leaves the
/// section. CONTROL: with programs' spots off nothing is told, and a refused click tells nothing.
#[test]
fn a_click_on_any_spot_is_told_to_every_program_and_arms_nothing() {
    let engine = spotting();
    let (mut m, _feed) = serving(&engine);
    let h = handle(&m);
    let (mut a, _, _) = program(&h, port(&m));
    let (mut b, _, _) = program(&h, port(&m));
    {
        let mut e = engine_lock(&engine);
        e.work_spot_tiered(None, "digital", 14.074, "20m", None);
        e.note_work_call(Some("W1AW".into()));
    }
    m.publish();
    for client in [&mut a, &mut b] {
        let lines = told_click(client, "W1AW", 14_074_000);
        assert!(
            lines.contains(&"clicked_on_spot:W1AW,14074000;".to_string()),
            "both spellings: {lines:?}"
        );
    }
    let (tick, section) = {
        let e = engine_lock(&engine);
        (e.snapshot().work_tick, e.settings().operating_mode)
    };
    assert_eq!(
        engine_lock(&engine).tci_program_spot_clicked("K1ABC", 14_025_000),
        Ok(())
    );
    m.publish();
    for client in [&mut a, &mut b] {
        let lines = told_click(client, "K1ABC", 14_025_000);
        assert!(
            lines.contains(&"clicked_on_spot:K1ABC,14025000;".to_string()),
            "both spellings: {lines:?}"
        );
    }
    {
        let e = engine_lock(&engine);
        assert!(
            (e.settings().dial_mhz - 14.025).abs() < 1e-9,
            "control: tuned"
        );
        assert!(!e.tx_enabled() && !e.on_air(), "armed or keyed");
        assert_eq!(e.qso_dxcall(), None, "a QSO started");
        assert_eq!(e.settings().operating_mode, section, "the section moved");
        assert_eq!(e.snapshot().work_tick, tick, "a cockpit was asked to open");
    }
    assert!(
        engine_lock(&engine)
            .tci_program_spot_clicked("K1ABC", 15_000_000)
            .is_err(),
        "premise: off the bands"
    );
    set(&engine, |s| s.tci_server_spots = false);
    {
        let mut e = engine_lock(&engine);
        e.work_spot_tiered(None, "digital", 14.074, "20m", None);
        e.note_work_call(Some("W1AW".into()));
        assert_eq!(e.tci_program_spot_clicked("K1ABC", 14_026_000), Ok(()));
    }
    m.publish();
    for client in [&mut a, &mut b] {
        let told = client.drain(Duration::from_millis(300));
        assert!(
            !told
                .iter()
                .any(|g| matches!(g, Got::Text(t) if t.contains("clicked_on_spot"))),
            "control: told with spots off or refused: {told:?}"
        );
    }
}

/// A click made while no server runs (switched on, its port not bound yet) is dropped at the
/// manager's tick, never told to the programs that connect later. CONTROL: a click made once it
/// serves is told.
#[test]
fn a_click_made_while_no_server_runs_is_never_told_later() {
    let engine = spotting();
    let mut m = Manager::new(
        engine.clone(),
        Arc::new(ReceiveAudioFeed::default()),
        MeterFeed::default(),
    );
    let work = |call: &str| {
        let mut e = engine_lock(&engine);
        e.work_spot_tiered(None, "digital", 14.074, "20m", None);
        e.note_work_call(Some(call.into()));
    };
    work("W1AW");
    m.publish();
    m.apply(Some(0));
    let h = handle(&m);
    let (mut a, _, _) = program(&h, port(&m));
    m.publish();
    let told = a.drain(Duration::from_millis(300));
    assert!(
        !told
            .iter()
            .any(|g| matches!(g, Got::Text(t) if t.contains("clicked_on_spot"))),
        "a click made before the server ran was told: {told:?}"
    );
    work("K1ABC");
    m.publish();
    let lines = told_click(&mut a, "K1ABC", 14_074_000);
    assert!(
        lines.contains(&"clicked_on_spot:K1ABC,14074000;".to_string()),
        "control: {lines:?}"
    );
}
