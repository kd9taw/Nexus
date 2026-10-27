//! Nexus's TCI client in the radio loop: which daemon a TCI radio gets and that nothing else serves
//! it, that every key goes through the client whatever PTT method is stored, that it never dials
//! Nexus's own TCI server, that it rebuilds on its own fields only, that the SDR program's receive
//! audio takes the sound card's place (the decoders, the waterfall, Remote Listen) with the receive
//! floor behind it, that Nexus's overs go out on its own audio once the SDR program has taken its
//! stream (the transmit tee: one route, never both) and none on the program's microphone, that a
//! Stop or a route lost part way through ends them, and what the client tells the operator about
//! the transmitter.
//!
//! The SDR program is the TCI simulator (`tempo-tcisim`) on loopback; the loop is the real
//! `RadioLoop::step`, driving the real `Rig` into the client's rigctld shim, as `run_radio` does.
//! The client's registry is process-wide and the simulators listen on ephemeral ports another
//! test's simulator may be given, so each scene forgets its server's entry first.

use super::*;
use crate::tci::{registry, Options as TciOptions, TciDaemon};
use tempo_net::tci::Trx;
use tempo_tcisim::{
    Config as SimConfig, Dialect as SimDialect, Event as SimEvent, Fault, Keyer, Logged, Meters,
    Mic, Simulator, Source as SimSource,
};

/// The longest any one expected thing may take here.
const WAIT: Duration = Duration::from_secs(10);

/// A TCI radio, as Settings stores one: model 7, the server on this computer.
fn tci(port: u16) -> Transport {
    let mut t = cat_transport(4534, Some(4532));
    t.rig_model = 7;
    t.rig_conn = "tci".into();
    t.tci_host = "127.0.0.1".into();
    t.tci_port = port;
    t.radio_id = 2;
    t
}

fn trx0() -> Trx {
    Trx::new(0).expect("trx 0")
}

// ── Which daemon, and how it keys ────────────────────────────────────────────────────────────

/// ⭐ THE TCI CONNECTION IS SERVED BY ITS OWN CLIENT AND KEYS THROUGH IT ALONE. The client's
/// target carries the profile's server, transceiver, dialect and audio. Nothing on this connection
/// opens the serial CI-V daemon or a keying line (a model and toggles left over from a USB setup
/// count for nothing), every PTT method keys through the client, and no Baud makes it a slow serial
/// link. Model 0 ("None / VOX") is still the no-CAT escape.
#[test]
fn a_tci_radio_is_served_by_its_own_client_and_keys_through_it_alone() {
    let mut t = tci(50001);
    t.tci_trx = 1;
    t.tci_dialect = "Thetis".into();
    t.tci_audio = false;
    assert_eq!(
        t.tci_target(),
        Some(crate::tci::Target {
            host: "127.0.0.1".into(),
            port: 50001,
            trx: 1,
            dialect: tempo_app::settings::TciDialect::Thetis,
            audio: false,
            rigctld_port: 4534,
        })
    );
    // A USB IC-9700's leftovers: the native toggle, its model, an RTS keying line on its port,
    // and a slow Baud.
    let mut t = tci(50001);
    t.rig_model = 3081;
    t.icom_native_cat = true;
    t.ptt_method = "rts".into();
    t.ptt_serial_port = t.serial_port.clone();
    t.baud = 9600;
    assert_eq!(native_civ_model(&t), None, "no serial CI-V daemon");
    assert!(!keys_on_the_cat_port(&t), "no keying line on a CAT port");
    assert_eq!(cat_port_ptt_line(&t), None);
    assert!(!t.is_slow_serial_link(), "no serial link at all");
    for method in ["cat", "rts", "dtr", "vox"] {
        t.ptt_method = method.into();
        assert!(
            matches!(ptt_mode_for(&t), PttMode::Cat),
            "{method}: keyed through the TCI client"
        );
    }
    // The controls: the same profile on its USB serial port.
    let mut usb = t.clone();
    usb.rig_conn = "serial".into();
    usb.ptt_method = "rts".into();
    assert!(native_civ_model(&usb).is_some(), "control: the CI-V daemon");
    assert!(keys_on_the_cat_port(&usb), "control: RTS on the CAT port");
    assert!(usb.is_slow_serial_link(), "control: 9600 Baud is slow");
    usb.ptt_method = "vox".into();
    assert!(matches!(ptt_mode_for(&usb), PttMode::Vox), "control: VOX");
    // Model 0 on TCI: no client, and no CAT keying either.
    let mut none = tci(50001);
    none.rig_model = 0;
    assert_eq!(none.tci_target(), None);
    assert!(matches!(ptt_mode_for(&none), PttMode::Vox));
}

/// ⛔ NO FALLBACK: a TCI connection the client cannot serve says why and starts nothing else,
/// rather than launch Hamlib at the serial port the profile last had.
#[test]
fn a_tci_connection_that_cannot_serve_says_why_and_starts_nothing_else() {
    let mut t = tci(50001);
    t.tci_host = " ".into();
    t.serial_port = "/dev/ttyUSB0".into();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(
            spawn_cat_daemon(&t, "/dev/ttyUSB0", false, None)
                .err()
                .map(|e| e.to_string()),
        );
    });
    let err = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("refused at once, with nothing launched")
        .expect("refused");
    assert!(
        err.starts_with("The TCI connection needs the SDR program's address"),
        "{err}"
    );
}

/// One change to a transport, for a table of them.
type Edit = fn(&mut Transport);

/// The TCI fields restart only a radio on the TCI connection, and every one of them does there.
#[test]
fn the_tci_fields_rebuild_only_the_tci_connection() {
    let a = tci(50001);
    let edits: [(&str, Edit); 5] = [
        ("host", |t| t.tci_host = "192.0.2.40".into()),
        ("port", |t| t.tci_port = 40001),
        ("transceiver", |t| t.tci_trx = 1),
        ("dialect", |t| t.tci_dialect = "pihpsdr".into()),
        ("audio", |t| t.tci_audio = false),
    ];
    for (what, edit) in edits {
        let mut b = a.clone();
        edit(&mut b);
        assert!(b.rig_differs(&a), "{what}: a TCI radio dials anew");
        let mut serial_a = a.clone();
        serial_a.rig_conn = "serial".into();
        serial_a.rig_model = 1035;
        let mut serial_b = serial_a.clone();
        edit(&mut serial_b);
        assert!(
            !serial_b.rig_differs(&serial_a),
            "{what}: a serial radio's leftover TCI fields restart nothing"
        );
    }
    // The dialect is read as the client reads it: a spelling of the same word is no change.
    let mut b = a.clone();
    b.tci_dialect = " AUTO ".into();
    assert!(!b.rig_differs(&a));
}

/// ⛔ THE TCI CLIENT NEVER DIALS NEXUS'S OWN TCI SERVER (the self-port guard): with the server on,
/// an address on this computer at its port is refused with words, and nothing connects. The same
/// address while the server is off is dialled (the control), and turning the server on or off
/// while a radio dials its port rebuilds that radio, never another.
#[test]
fn the_tci_client_never_dials_nexus_own_tci_server() {
    // What listens on "Nexus's own TCI server" port: count who connects.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    {
        let accepted = accepted.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                accepted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                drop(stream);
            }
        });
    }
    let open = |t: Transport| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(
                spawn_cat_daemon(&t, "", false, None)
                    .err()
                    .map(|e| e.to_string()),
            );
        });
        rx.recv_timeout(WAIT)
            .expect("the open answered")
            .expect("refused")
    };
    let mut t = tci(port);
    t.rigctld_port = 0;
    t.tci_server_self_port = Some(port);
    for host in ["127.0.0.1", "localhost", " 127.0.0.1 "] {
        t.tci_host = host.into();
        let err = open(t.clone());
        assert!(err.contains("Nexus's own TCI server"), "{host}: {err}");
    }
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        accepted.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "nothing dialled Nexus's own server"
    );
    // The control: the server off, the same address is dialled (it is not a TCI server, so the
    // start fails, after the connect).
    registry::forget((std::net::SocketAddr::from(([127, 0, 0, 1], port)), trx0()));
    let mut off = t.clone();
    off.tci_host = "127.0.0.1".into();
    off.tci_server_self_port = None;
    let err = open(off.clone());
    assert!(!err.contains("Nexus's own TCI server"), "{err}");
    assert!(
        accepted.load(std::sync::atomic::Ordering::SeqCst) >= 1,
        "control: the client dialled it ({err})"
    );
    registry::forget((std::net::SocketAddr::from(([127, 0, 0, 1], port)), trx0()));
    // Turning the server on at the port a TCI radio dials rebuilds it; at another port, or on a
    // radio that is not on TCI, nothing.
    let mut on = off.clone();
    on.tci_server_self_port = Some(port);
    assert!(on.rig_differs(&off), "the server on, where the radio dials");
    let mut elsewhere = off.clone();
    elsewhere.tci_server_self_port = Some(port.wrapping_add(1));
    assert!(!elsewhere.rig_differs(&off), "the server on another port");
    let mut serial_off = off.clone();
    serial_off.rig_conn = "serial".into();
    serial_off.rig_model = 1035;
    let mut serial_on = serial_off.clone();
    serial_on.tci_server_self_port = Some(port);
    assert!(!serial_on.rig_differs(&serial_off), "a radio not on TCI");
}

/// The launch dials the TCI server the settings name, with the port of Nexus's own TCI server known,
/// and so does every radio the monitor holds: neither waits a tick for the settings' rebuild, whose
/// first open would have dialled a default (and its close sent `trx:0,false;` to whatever listens
/// there).
#[test]
fn the_launch_and_the_monitor_dial_the_tci_server_the_settings_name() {
    let mut s = Settings {
        rig_model: 7,
        rig_conn: "tci".into(),
        tci_host: "192.0.2.40".into(),
        tci_port: 40001,
        tci_trx: 1,
        tci_server: true,
        tci_server_port: 40002,
        ..Settings::default()
    };
    let cfg = RadioConfig {
        rig_model: 7,
        rig_conn: "tci".into(),
        ..RadioConfig::default()
    };
    let launch = launch_transport(&cfg, &s, None);
    let want = Transport::from_settings(&s);
    assert!(launch.tci_target().is_some());
    assert_eq!(
        launch.tci_target(),
        want.tci_target(),
        "the launch dials elsewhere"
    );
    assert_eq!(launch.tci_server_self_port, Some(40002));
    // A radio the monitor holds.
    s.radios.push(tempo_app::settings::RadioProfile {
        id: 9,
        rig_model: 7,
        rig_conn: "tci".into(),
        tci_host: "192.0.2.41".into(),
        tci_port: 50001,
        ..tempo_app::settings::RadioProfile::default()
    });
    let (_, pool) = monitor_want(&s, None);
    let (_, t) = pool
        .iter()
        .find(|(id, _)| *id == 9)
        .expect("the monitor holds it");
    assert_eq!(
        t.tci_target().map(|t| (t.host, t.port)),
        Some(("192.0.2.41".to_string(), 50001))
    );
    assert_eq!(t.tci_server_self_port, Some(40002));
}

/// A TCI radio whose open failed is tried again when the client's reconnect ladder allows, never
/// while it waits for the operator, and on the ordinary backoff when the ladder has nothing to say.
#[test]
fn a_failed_tci_open_waits_on_the_clients_reconnect_ladder() {
    let key = (std::net::SocketAddr::from(([127, 0, 0, 1], 7)), trx0());
    registry::forget(key);
    assert_eq!(tci_reopen_at(key, 5_000.0, 99.0), 99.0, "nothing to say");
    let claim = registry::claim(key, crate::tci::now_ms()).expect("a claim");
    claim.ended(
        &tempo_net::tci::reconnect::End::Ended(tempo_net::tci::session::End::Lost),
        None,
        crate::tci::now_ms(),
    );
    // The ladder's first wait, 1 s, less what has run of it since.
    let at = tci_reopen_at(key, 5_000.0, 99.0);
    assert!(
        at > 5_000.0 && at <= 6_000.0,
        "the ladder's first wait: {at}"
    );
    // Three refusals in a row (not a TCI server) wait for the operator.
    let refused = tempo_net::tci::reconnect::End::Failed(
        tempo_net::tci::session::ConnectError::Handshake("not a TCI server".into()),
    );
    let mut at = crate::tci::now_ms();
    for _ in 0..3 {
        at += 1_000_000;
        registry::claim(key, at)
            .expect("past the ladder's wait")
            .ended(&refused, None, at);
    }
    assert!(registry::held(key));
    assert_eq!(tci_reopen_at(key, 5_000.0, 99.0), f64::INFINITY);
    registry::forget(key);
}

// ── The radio loop on a simulated SDR program ────────────────────────────────────────────────

/// The scene's sound card: it captures silence, which the SDR program's audio is told from, and
/// keeps what is played on it. As the sound card does, it hands an over to an installed transmit
/// tee instead, and a Stop's flush and a mid-over switch go to the tee too.
#[derive(Clone)]
struct Card {
    played: Arc<Mutex<Vec<f32>>>,
    /// The card's own ring on the receive tap.
    ring: Arc<crate::monitor::SpscRing>,
    tee: Arc<Mutex<Option<crate::backend::TxTeeHandle>>>,
}

impl Default for Card {
    fn default() -> Card {
        Card {
            played: Arc::default(),
            ring: Arc::new(crate::monitor::SpscRing::new(1 << 14)),
            tee: Arc::default(),
        }
    }
}

impl AudioBackend for Card {
    fn capture(&mut self) -> Vec<f32> {
        vec![0.0; 240]
    }
    fn play(&mut self, samples: &[f32]) {
        match self.tee.lock().unwrap().as_ref() {
            Some(tee) => tee.feed(samples),
            None => self.played.lock().unwrap().extend_from_slice(samples),
        }
    }
    fn spectrum_tap(&self) -> Option<(Arc<crate::monitor::SpscRing>, u32)> {
        Some((self.ring.clone(), 48_000))
    }
    fn set_tx_tee(&mut self, tee: Option<crate::backend::TxTeeHandle>) {
        *self.tee.lock().unwrap() = tee;
    }
    fn flush_output(&mut self) -> usize {
        self.tee.lock().unwrap().as_ref().map_or(0, |t| t.flush())
    }
    fn retune_tx(&mut self, tail: &mut dyn FnMut(usize, usize) -> Vec<f32>) -> bool {
        self.tee
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|t| t.retune(tail))
    }
}

/// What a rebuild of the scene's CAT link opens.
#[derive(Clone)]
enum Reopen {
    /// A rebuild fails the test: the client under test is gone.
    Never,
    /// A fresh client on the same server, as `open_cat` starts one.
    Client,
    /// A rigctld at this address, with no daemon of ours.
    Rigctld(String),
}

/// The radio loop on a radio of the TCI connection, its client on the simulated SDR program.
struct TciScene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    rig: Rig,
    card: Card,
    sim: Simulator,
    options: TciOptions,
    reopen: Reopen,
    rebuilds: Arc<std::sync::atomic::AtomicUsize>,
    /// The clock the loop is stepped on when it is not the real one: a made-up one from 100 ms, 20
    /// ms a step, for scenes whose slot boundary must fall on their first tick.
    made_up: Option<f64>,
}

impl TciScene {
    /// The scene on `faults`, its client's waits `options`, the engine set up by `setup` after the
    /// radio is put on the TCI connection.
    fn new(
        faults: impl IntoIterator<Item = Fault>,
        options: TciOptions,
        setup: impl FnOnce(&mut Engine),
    ) -> TciScene {
        TciScene::on_dialect(SimDialect::expert_sdr3(), faults, options, setup)
    }

    /// [`TciScene::new`] on a server speaking `dialect`.
    fn on_dialect(
        dialect: SimDialect,
        faults: impl IntoIterator<Item = Fault>,
        options: TciOptions,
        setup: impl FnOnce(&mut Engine),
    ) -> TciScene {
        let sim = Simulator::start(SimConfig::dialect(dialect).with(faults))
            .expect("the simulator starts");
        registry::forget((sim.addr(), trx0()));
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            let mut s = e.settings().clone();
            s.rig_model = 7;
            s.rig_conn = "tci".into();
            s.tci_host = "127.0.0.1".into();
            s.tci_port = sim.addr().port();
            s.ptt_method = "cat".into();
            e.apply_settings(s);
            setup(&mut e);
        }
        // The transport the loop compares its settings against: the same settings, so it never
        // rebuilds the link the scene does not mean it to.
        let applied = Transport::from_settings(engine.lock().unwrap().settings());
        let (daemon, rig) = TciScene::client(&applied, &options);
        let cfg = RadioConfig {
            rig_model: 7,
            ..RadioConfig::default()
        };
        let state = RadioLoop::new(applied, Some(CatDaemon::Tci(daemon)), &cfg);
        TciScene {
            engine,
            state,
            rig,
            card: Card::default(),
            sim,
            options,
            reopen: Reopen::Never,
            rebuilds: Arc::default(),
            made_up: None,
        }
    }

    /// A client for `t` on an OS-chosen rigctld port, and the loop's `Rig` on it, as `open_cat`
    /// makes them.
    fn client(t: &Transport, options: &TciOptions) -> (TciDaemon, Rig) {
        let mut target = t.tci_target().expect("a TCI radio");
        target.rigctld_port = 0;
        let daemon =
            TciDaemon::start_receiving(&target, options.clone()).expect("the client starts");
        let mut rig = Rig::rigctld(&format!("127.0.0.1:{}", daemon.local_addr().port()));
        rig.set_slow_transport(true);
        (daemon, rig)
    }

    /// The scene on a made-up clock from 100 ms (see [`TciScene::made_up`]).
    fn on_a_made_up_clock(mut self) -> TciScene {
        self.made_up = Some(100.0);
        self
    }

    fn step(&mut self) {
        let now = self.made_up.unwrap_or_else(now_unix_ms);
        let card = self.card.clone();
        let mut reopen_audio = move |_t: &Transport| Ok::<_, String>(card.clone());
        let (reopen, options, rebuilds) = (
            self.reopen.clone(),
            self.options.clone(),
            self.rebuilds.clone(),
        );
        let mut reopen_rig = move |t: &Transport, _coexist: bool| -> RigOpen {
            rebuilds.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match &reopen {
                Reopen::Never => (Rig::vox(), None, CatProbe::status(None, "")),
                Reopen::Client => {
                    let (daemon, mut rig) = TciScene::client(t, &options);
                    let probe = finish_cat_open(&mut rig, t);
                    (rig, Some(CatDaemon::Tci(daemon)), probe)
                }
                Reopen::Rigctld(addr) => (
                    Rig::rigctld(addr),
                    None,
                    CatProbe::status(Some(true), "reopened"),
                ),
            }
        };
        let mut station = StationSinks::new();
        self.state
            .step(
                &self.engine,
                &mut self.card,
                &mut self.rig,
                &no_sinks(),
                now,
                &mut reopen_audio,
                &mut reopen_rig,
                &mut station,
            )
            .unwrap();
        if let Some(t) = self.made_up.as_mut() {
            *t += 20.0;
        }
        assert!(
            !matches!(self.reopen, Reopen::Never)
                || self.rebuilds.load(std::sync::atomic::Ordering::SeqCst) == 0,
            "the loop rebuilt its transport: the client under test is gone"
        );
    }

    /// Run the loop a step then 20 ms, as `run_radio` does, for `ms` of real time.
    fn run(&mut self, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            self.step();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Run the loop until `done`, or fail saying `what`.
    fn run_until(&mut self, what: &str, done: impl Fn(&TciScene) -> bool) {
        let deadline = Instant::now() + WAIT;
        while !done(self) {
            assert!(Instant::now() < deadline, "{what}");
            self.step();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn daemon(&self) -> &TciDaemon {
        self.state
            .rigctld_proc
            .as_ref()
            .and_then(CatDaemon::tci)
            .expect("the TCI client")
    }

    fn engine(&self) -> std::sync::MutexGuard<'_, Engine> {
        self.engine.lock().unwrap()
    }

    /// What keyed the simulated radio, in order.
    fn keys(&self) -> Vec<(Keyer, SimSource)> {
        keys(&self.sim.log())
    }

    /// How many times the server read `text` from connection `conn`.
    fn sent(&self, conn: usize, text: &str) -> usize {
        self.sim
            .commands_from(conn)
            .iter()
            .filter(|c| *c == text)
            .count()
    }

    /// Whether anything keyed the radio for the scene's client on any source.
    fn any_key_asked(&self) -> bool {
        self.sim
            .commands_from(0)
            .iter()
            .any(|c| c.starts_with("trx:0,true") || c.starts_with("tune:0,true"))
    }
}

/// What keyed the radio, in order.
fn keys(log: &[Logged]) -> Vec<(Keyer, SimSource)> {
    log.iter()
        .filter_map(|l| match l.event {
            SimEvent::Keyed { by, source, .. } => Some((by, source)),
            _ => None,
        })
        .collect()
}

/// The Phone section on 20 m, inside an Extra's phone privileges.
fn phone(e: &mut Engine) {
    e.set_operating_mode("phone", true);
    e.set_frequency(14.250, "20m", "USB");
}

/// Listening on the simulated radio's own dial, 14.074 MHz USB: its carrier, 1 kHz above the dial,
/// is a 1 kHz tone in the receive audio.
fn listen(e: &mut Engine) {
    e.set_operating_mode("phone", true);
    e.set_frequency(14.074, "20m", "USB");
}

fn waits() -> TciOptions {
    TciOptions::default()
}

/// The production waits, but a session that keeps a stop the server never confirms open for the
/// reading rather than escalating after two seconds.
fn patient() -> TciOptions {
    TciOptions {
        adjust: |c| c.unkey_deadline_ms = 30_000,
        ..TciOptions::default()
    }
}

/// The peak of `samples`.
fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// ⭐ A TCI RADIO RUNS ON ITS CLIENT, POLL AFTER POLL, WITH NO REBUILD: the heavy poll's reads of
/// what TCI does not carry (`RPRT -11`) neither cost the loop its CAT connection nor read as a
/// dead link, and the S-meter comes from the SDR program's own report.
#[test]
fn a_tci_radio_runs_on_its_client_poll_after_poll() {
    let mut s = TciScene::new([], waits(), phone);
    s.sim.set_meters(Meters {
        smeter_dbm: -53.0,
        mic_dbm: -30.0,
        max_power_w: 100.0,
        swr: 1.5,
    });
    s.run_until("the S-meter never reached the engine", |s| {
        s.engine().snapshot().radio.smeter_db == Some(20)
    });
    s.run(4_000);
    assert_eq!(s.state.cat_ok, Some(true), "the link reads as healthy");
    assert_eq!(
        s.rebuilds.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "no rebuild"
    );
    assert_eq!(
        s.sim.connections(),
        vec![0],
        "one session, never dialled again"
    );
    assert_eq!(
        s.sim.radio().trx[0].vfo[0],
        14_250_000,
        "the dial reached the SDR program"
    );
}

/// ⭐ THE SDR PROGRAM'S RECEIVE AUDIO TAKES THE SOUND CARD'S PLACE: the decoders hear the carrier the
/// simulated radio receives (1 kHz above the dial) where the card captures silence, and the receive
/// tap (the waterfall, Remote Listen) is fed the same audio at the modem rate, from a ring of the
/// session's own, not the card's.
#[test]
fn the_sdr_programs_receive_audio_takes_the_sound_cards_place() {
    let mut s = TciScene::new([], waits(), listen);
    s.run_until("TCI audio never became the receive source", |s| {
        s.state.tci_rx.is_some()
    });
    s.run(1_500);
    let heard = s.state.rx.tail_window(2_400, 2_400);
    assert!(
        peak(&heard) > 0.01,
        "the decoders heard the card's silence, not the SDR program (peak {})",
        peak(&heard)
    );
    let tap = s.state.rx_tap.current().expect("a source on the tap");
    assert!(
        !Arc::ptr_eq(&tap.ring, &s.card.ring),
        "the tap is still the sound card's"
    );
    assert_eq!(tap.rate, crate::tci::audio::MODEM_RATE);
    assert!(!tap.ring.is_empty(), "nothing reached the tap's ring");
    // Audio over TCI off: a new session that asks for none, and the card is back on the tap.
    {
        let mut e = s.engine();
        let mut settings = e.settings().clone();
        settings.tci_audio = false;
        e.apply_settings(settings);
    }
    s.reopen = Reopen::Client;
    s.run_until("the card never came back on the tap", |s| {
        s.state
            .rx_tap
            .current()
            .is_some_and(|t| Arc::ptr_eq(&t.ring, &s.card.ring))
    });
    assert!(s.state.tci_rx.is_none());
    assert!(
        !s.sim
            .commands_from(1)
            .iter()
            .any(|c| c.starts_with("audio_start")),
        "the new session asked for no audio"
    );
}

/// ⭐ RECEIVE AUDIO THAT STOPS FALLS BACK TO THE SOUND CARD, AND SAYS WHY (the receive floor): the
/// SDR program's frames stop, and past the floor's window the decoders and the tap are the sound
/// card's again, the operator is told, and the session stays the radio's CAT.
#[test]
fn receive_audio_that_stops_falls_back_to_the_sound_card_and_says_why() {
    let mut s = TciScene::new(
        [Fault::RxAudioStall {
            after: Duration::from_millis(500),
        }],
        waits(),
        listen,
    );
    s.run_until("TCI audio never became the receive source", |s| {
        s.state.tci_rx.is_some()
    });
    let deadline = Instant::now() + DAX_STARVE_AFTER + WAIT;
    while s.state.tci_rx.is_some() {
        assert!(Instant::now() < deadline, "the floor never gave up on it");
        s.run(100);
    }
    s.run(100);
    assert!(
        s.state
            .rx_tap
            .current()
            .is_some_and(|t| Arc::ptr_eq(&t.ring, &s.card.ring)),
        "the card is back on the tap"
    );
    let banner = s.engine().snapshot().radio.audio_error.unwrap_or_default();
    assert!(
        banner.starts_with("Audio over TCI is on for this radio but no audio is arriving"),
        "{banner}"
    );
    assert!(
        s.daemon().is_alive(),
        "the session is still the radio's CAT"
    );
    assert_eq!(s.state.cat_ok, Some(true));
    s.run(500);
    assert!(
        s.state.tci_rx.is_none(),
        "not taken again from this session"
    );
}

/// The receive floor does not give up on TCI audio that stops under an over of ours: a server may
/// pause its receive stream while it transmits, and that is no failure. The window runs from the
/// unkey.
#[test]
fn receive_audio_that_stops_under_our_own_over_is_not_given_up_on() {
    let mut s = TciScene::new(
        [Fault::RxAudioStall {
            after: Duration::from_millis(1_000),
        }],
        waits(),
        phone,
    );
    s.run_until("TCI audio never became the receive source", |s| {
        s.state.tci_rx.is_some()
    });
    s.engine().set_ptt(true);
    s.run_until("the PTT never keyed", |s| s.sent(0, "trx:0,true") == 1);
    s.run((DAX_STARVE_AFTER + Duration::from_secs(2)).as_millis() as u64);
    assert!(s.state.tci_rx.is_some(), "given up on under our own over");
    assert_eq!(s.engine().snapshot().radio.audio_error, None);
    s.engine().set_ptt(false);
    s.run_until("the release never unkeyed", |s| {
        s.sent(0, "trx:0,false") >= 1
    });
}

// ── Transmit: Nexus's audio on its own route, never the SDR's microphone ────────────────────

/// An engine set up for a scene.
type Setup = fn(&mut Engine);

/// FT8 CQ armed for the boundary (`refused_key.rs`'s `ft8_cq_at_the_boundary`).
fn ft8_cq_at_the_boundary(e: &mut Engine) {
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.broadcast("CQ W9XYZ EN37");
    let _ = e.take_immediate_tx();
}

impl TciScene {
    /// Wait until the SDR program has taken Nexus's audio stream and its receive audio flows: the
    /// session's transmit route is ready, and a key of Nexus's audio would be admitted.
    fn routed(&self, what: &str) {
        assert!(
            self.daemon()
                .session()
                .wait_until(WAIT, |snap| snap.rx_audio_fresh
                    && snap.model.trx(trx0()).audio.is_true()),
            "{what}: the SDR program never took Nexus's audio stream"
        );
        assert!(
            self.daemon().tx_route_ready(),
            "{what}: premise: the route is ready"
        );
    }

    /// Wait until some of Nexus's audio (a sample that is not silence) has reached the SDR program.
    fn audio_reached(&self, what: &str) {
        let deadline = Instant::now() + WAIT;
        while !self.sim.tx_audio(0).iter().any(|s| *s != 0.0) {
            assert!(
                Instant::now() < deadline,
                "{what}: none of Nexus's audio reached the SDR program"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The client's transmit route.
    fn route(&self) -> Arc<crate::tci::audio::TciTx> {
        self.daemon().tx().expect("the transmit route").clone()
    }
}

/// ⛔⭐ EVERY OVER OF NEXUS'S GOES OUT ON ITS OWN AUDIO, AND NONE ON THE SDR PROGRAM'S MICROPHONE.
/// Once the SDR program has taken Nexus's audio stream, the loop installs the client's transmit tee:
/// an FT8 over in a data mode, and a voice-keyer message and an APRS frame in a voice mode (where an
/// unmarked key would take the program's own microphone), each key `trx:0,true,tci` once, their
/// audio reaches the program as it pulls it, and none of it is played on the sound card (one route,
/// never both). A program that never takes the stream (it does not acknowledge `audio_start`) gives
/// no route: each over is kept off the air instead, nothing is played anywhere, and the slot over's
/// refusal halts TX, as WSJT-X halts on a key the rig refuses. The operator's own PTT, the control,
/// keys that microphone, as the Flex client's ruling has it.
#[test]
fn every_over_of_nexus_goes_out_on_its_own_audio_and_none_on_the_sdr_microphone() {
    let overs: [(&str, Setup); 3] = [
        ("an FT8 over", ft8_cq_at_the_boundary),
        ("a voice-keyer message", |e| {
            phone(e);
            e.send_voice(vec![0.05; 12_000]).unwrap();
        }),
        ("an APRS frame", |e| {
            phone(e);
            e.aprs_beacon(41.88, -87.63, '/', '>', "", &[])
                .expect("the beacon is queued");
        }),
    ];
    for (what, setup) in overs {
        let mut s = TciScene::new([], waits(), setup).on_a_made_up_clock();
        s.routed(what);
        // A step a tick, as `run` paces them: the made-up clock never runs ahead of the real one.
        // Unpaced, 50 steps took about 20 ms, and the APRS frame (0.49 s and its tail) had ended
        // and its audio gone with the unkey before the program pulled its first frame, when it
        // was slow to.
        for _ in 0..50 {
            s.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        s.audio_reached(what);
        assert_eq!(
            s.keys(),
            [(Keyer::Client(0), SimSource::Tci)],
            "{what}: keyed on Nexus's audio"
        );
        assert_eq!(s.sent(0, "trx:0,true,tci"), 1, "{what}");
        assert!(
            s.card.played.lock().unwrap().is_empty(),
            "{what}: played on the sound card as well"
        );
        // No route: the program never takes the stream.
        let mut s = TciScene::new([Fault::NoAudioAck], waits(), setup).on_a_made_up_clock();
        assert!(
            s.sim.wait_for(WAIT, |log| log.iter().any(|l| matches!(
                &l.event,
                SimEvent::Command { conn: 0, text, .. } if text == "audio_start:0"
            ))),
            "{what}: premise: the stream asked for"
        );
        assert!(
            !s.daemon()
                .session()
                .wait_until(Duration::from_millis(300), |snap| snap
                    .model
                    .trx(trx0())
                    .audio
                    .is_true()),
            "{what}: premise: never taken"
        );
        for _ in 0..50 {
            s.step();
        }
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !s.any_key_asked(),
            "{what}: keyed {:?}",
            s.sim.commands_from(0)
        );
        assert!(s.keys().is_empty(), "{what}: the radio keyed");
        assert!(s.card.played.lock().unwrap().is_empty(), "{what}: played");
        assert!(s.sim.tx_audio(0).is_empty(), "{what}: sent");
        if what == "an FT8 over" {
            assert!(!s.engine().tx_enabled(), "TX is still on");
            assert!(s.engine().snapshot().radio.slot_key_refused.is_some());
        }
    }
    // The control: the operator's own PTT in Phone keys the SDR program's microphone.
    let mut s = TciScene::new([], waits(), phone);
    s.run(300);
    s.engine().set_ptt(true);
    s.run_until("the operator's PTT never keyed", |s| !s.keys().is_empty());
    assert_eq!(
        s.keys()[0],
        (Keyer::Client(0), SimSource::Mic(Mic::Selected))
    );
    assert_eq!(s.sent(0, "trx:0,true"), 1);
    s.engine().set_ptt(false);
    s.run_until("the release never unkeyed", |s| {
        s.sent(0, "trx:0,false") >= 1
    });
}

/// ⛔ A SLOT OVER IS NEXUS'S AUDIO EVEN ON A SERVER THAT OFFERS NO DATA MODE. There the loop's PKTUSB
/// is refused and the SDR program stays in USB, a voice mode, where an unmarked key takes the SDR
/// program's microphone: an FT8 over, at its boundary and as a late start, keys Nexus's audio
/// instead (`trx:0,true,tci`), its audio reaches the program, and nothing is played on the sound
/// card.
#[test]
fn a_slot_over_never_keys_the_sdr_microphone_on_a_server_with_no_data_mode() {
    let no_data_mode = || {
        let mut d = SimDialect::expert_sdr3();
        d.modulations.retain(|m| m != "DIGU" && m != "DIGL");
        d
    };
    let on_its_own_audio = |s: &mut TciScene, what: &str| {
        for _ in 0..50 {
            s.step();
        }
        s.audio_reached(what);
        assert_eq!(
            s.keys(),
            [(Keyer::Client(0), SimSource::Tci)],
            "{what}: keyed on Nexus's audio, not the microphone"
        );
        assert_eq!(
            s.sim.radio().trx[0].modulation,
            "USB",
            "{what}: premise: a voice mode"
        );
        assert!(s.card.played.lock().unwrap().is_empty(), "{what}: played");
    };
    let mut s = TciScene::on_dialect(no_data_mode(), [], waits(), ft8_cq_at_the_boundary)
        .on_a_made_up_clock();
    s.routed("at the boundary");
    on_its_own_audio(&mut s, "at the boundary");
    // A late start: slot 0's boundary consumed first, as a running station's is.
    let mut s = TciScene::on_dialect(no_data_mode(), [], waits(), |e| e.set_tier(Tier::Ft8))
        .on_a_made_up_clock();
    s.routed("a late start");
    for _ in 0..2 {
        s.step();
    }
    assert_eq!(s.state.last_slot, Some(0), "scene guard: slot 0's boundary");
    ft8_cq(&mut s.engine());
    on_its_own_audio(&mut s, "a late start");
}

/// Phone on 20 m with "Programs on TCI may transmit" on, and program 1 on Nexus's own TCI server a
/// program that asked `usb` (a program's own audio goes out only after it asks a voice mode): it
/// may key.
fn programs_may_transmit(e: &mut Engine) {
    let mut settings = e.settings().clone();
    settings.tci_server_tx = true;
    e.apply_settings(settings);
    phone(e);
    let usb = tempo_net::tci::wire::Word::new("usb").expect("a mode word");
    e.tci_note_modulation(1, &usb);
}

impl TciScene {
    /// Run the loop on the real clock until `done`, a program on Nexus's own TCI server sending 20 ms
    /// of its audio at `level` before each step, as the server hands it over; or fail saying `what`.
    fn run_with_program_audio(&mut self, level: f32, what: &str, done: impl Fn(&TciScene) -> bool) {
        let deadline = Instant::now() + WAIT;
        while !done(self) {
            assert!(Instant::now() < deadline, "{what}");
            let _ = self.engine().tci_tx_audio(1, &[level; 240], Instant::now());
            self.step();
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// ⛔ A PROGRAM'S AUDIO OVER THROUGH NEXUS'S OWN TCI SERVER GOES TO THE SDR PROGRAM AS NEXUS'S AUDIO,
/// NEVER ON ITS MICROPHONE. With the station's radio on the TCI connection in a voice mode (where an
/// unmarked key would take the SDR program's microphone), a program's over keys `trx:0,true,tci`
/// once, its audio reaches the SDR program and none of it is played on the sound card. With an SDR
/// program that never takes Nexus's audio stream, nothing keys at all and nothing is sent anywhere:
/// the over ends, refused, and is never put on the microphone.
#[test]
fn a_tci_server_programs_over_goes_out_as_nexus_audio_never_on_the_sdr_microphone() {
    use tempo_net::tci::server::KeyKind;
    let mut s = TciScene::new([], waits(), programs_may_transmit);
    s.routed("the program's over");
    s.run(200);
    assert_eq!(
        s.engine().tci_key(1, KeyKind::Tci, true),
        Ok(()),
        "premise: the program's key is admitted"
    );
    s.run_with_program_audio(0.25, "the program's over never keyed", |s| {
        !s.keys().is_empty()
    });
    assert_eq!(s.sent(0, "trx:0,true"), 0, "the SDR program's microphone");
    assert_eq!(
        s.keys(),
        [(Keyer::Client(0), SimSource::Tci)],
        "keyed on Nexus's audio"
    );
    s.run_with_program_audio(
        0.25,
        "the program's audio never reached the SDR program",
        |s| s.sim.tx_audio(0).iter().any(|x| *x != 0.0),
    );
    assert_eq!(s.sent(0, "trx:0,true,tci"), 1);
    assert!(
        s.card.played.lock().unwrap().is_empty(),
        "played on the sound card as well"
    );
    // No route: the SDR program never takes Nexus's stream.
    let mut s = TciScene::new([Fault::NoAudioAck], waits(), programs_may_transmit);
    assert!(
        s.sim.wait_for(WAIT, |log| log.iter().any(|l| matches!(
            &l.event,
            SimEvent::Command { conn: 0, text, .. } if text == "audio_start:0"
        ))),
        "premise: the stream asked for"
    );
    s.run(300);
    assert_eq!(
        s.engine().tci_key(1, KeyKind::Tci, true),
        Ok(()),
        "premise: the program's key is admitted"
    );
    s.run_with_program_audio(0.25, "the refused over never ended", |s| {
        !s.engine().tci_over_armed()
    });
    std::thread::sleep(Duration::from_millis(300));
    assert!(!s.any_key_asked(), "keyed {:?}", s.sim.commands_from(0));
    assert!(s.keys().is_empty(), "the radio keyed");
    assert!(s.card.played.lock().unwrap().is_empty(), "played");
    assert!(s.sim.tx_audio(0).is_empty(), "sent");
}

/// ⭐ WITH AUDIO OVER TCI OFF, A TCI RADIO KEYS LIKE ANY CAT RADIO (the operator's rule): an over of
/// Nexus's keys the SDR program's own input, which the operator routes from Nexus's sound card (a
/// virtual cable), and Nexus plays it to the sound card. No transmit tee goes in, no audio stream is
/// asked for, and nothing of it reaches the program over TCI: one route.
#[test]
fn with_audio_over_tci_off_an_over_keys_the_programs_input_and_plays_on_the_sound_card() {
    let message = vec![0.05; 12_000];
    let mut s = TciScene::new([], waits(), |e| {
        let mut settings = e.settings().clone();
        settings.tci_audio = false;
        e.apply_settings(settings);
        phone(e);
        e.send_voice(vec![0.05; 12_000]).unwrap();
    });
    s.run_until("the message never keyed", |s| !s.keys().is_empty());
    s.run(300);
    assert_eq!(
        s.keys(),
        [(Keyer::Client(0), SimSource::Mic(Mic::Selected))],
        "the program's own input"
    );
    assert_eq!(
        *s.card.played.lock().unwrap(),
        message,
        "the message on the sound card"
    );
    assert_eq!(s.state.dax_tee_id, None, "no transmit tee");
    assert!(!s.daemon().tx_route_ready());
    assert!(
        !s.sim
            .commands_from(0)
            .iter()
            .any(|c| c.starts_with("audio_start")),
        "no stream asked for"
    );
    assert!(s.sim.tx_audio(0).is_empty(), "nothing over TCI");
}

/// ⭐ AND AN FT8 OVER, WITH AUDIO OVER TCI OFF (NEEDS-BENCH): the slot over keys the SDR program's
/// own input at its boundary, as any CAT radio's does, and the whole over plays on the sound card,
/// which the operator routes to that input. No transmit tee goes in, no audio stream is asked for,
/// and nothing of it reaches the program over TCI: one route.
#[test]
fn with_audio_over_tci_off_an_ft8_over_keys_the_programs_input_and_plays_on_the_sound_card() {
    let mut s = TciScene::new([], waits(), |e| {
        let mut settings = e.settings().clone();
        settings.tci_audio = false;
        e.apply_settings(settings);
        ft8_cq_at_the_boundary(e);
    })
    .on_a_made_up_clock();
    assert!(
        s.daemon().session().wait_until(WAIT, |snap| snap.phase
            == tempo_net::tci::session::Phase::Ready),
        "premise: the SDR program is ready"
    );
    for _ in 0..50 {
        s.step();
    }
    assert!(
        s.sim.wait_for(WAIT, |log| !keys(log).is_empty()),
        "the FT8 over never keyed: {:?}",
        s.sim.commands_from(0)
    );
    assert_eq!(
        s.keys(),
        [(Keyer::Client(0), SimSource::Mic(Mic::Selected))],
        "the program's own input"
    );
    assert_eq!(s.sent(0, "trx:0,true"), 1);
    let played = s.card.played.lock().unwrap().clone();
    assert!(
        played.len() > 12 * 12_000 && peak(&played) > 0.0,
        "the over on the sound card: {} samples, peak {}",
        played.len(),
        peak(&played)
    );
    assert_eq!(s.state.dax_tee_id, None, "no transmit tee");
    assert!(!s.daemon().tx_route_ready());
    assert!(
        !s.sim
            .commands_from(0)
            .iter()
            .any(|c| c.starts_with("audio_start")),
        "no stream asked for"
    );
    assert!(s.sim.tx_audio(0).is_empty(), "nothing over TCI");
}

/// ⛔ STOP TX MID-OVER ON TCI SENDS NO MORE OF IT. A voice-keyer message going out
/// on Nexus's audio is stopped part way: the loop's hard stop empties the transmit route, so nothing
/// of the message is left to send, and the over ends. Here the SDR program does not confirm the
/// unkey and goes on pulling: what it gets after our stop is silence. None of the message goes to
/// the sound card.
#[test]
fn stop_tx_mid_over_on_tci_sends_no_more_of_it() {
    let mut s = TciScene::new([Fault::StuckTrx], patient(), phone);
    s.routed("the scene");
    s.run(100);
    let tone: Vec<f32> = (0..36_000)
        .map(|i| 0.5 * (std::f32::consts::TAU * 1_000.0 * i as f32 / 12_000.0).sin())
        .collect();
    s.engine().send_voice(tone).unwrap();
    s.run_until("the message never keyed", |s| !s.keys().is_empty());
    s.audio_reached("the message");
    s.run(300);
    s.engine().halt_tx();
    s.run_until("the stop never reached the SDR program", |s| {
        s.sent(0, "trx:0,false") >= 1
    });
    assert_eq!(s.route().queued(), 0, "the rest of the message left queued");
    assert_eq!(s.state.tx_until_ms, None, "the over goes on");
    let stop = s
        .sim
        .log()
        .iter()
        .find_map(|l| match &l.event {
            SimEvent::Command { conn: 0, text, .. } if text == "trx:0,false" => Some(l.at),
            _ => None,
        })
        .expect("our stop");
    let answered = |s: &TciScene, before: Duration| {
        s.sim
            .log()
            .iter()
            .filter(|l| l.at < before && matches!(l.event, SimEvent::FrameIn { conn: 0, .. }))
            .count()
    };
    let before = answered(&s, stop);
    s.run_until("ten answers after the stop", |s| {
        answered(s, Duration::MAX) >= before + 10
    });
    let sent = s.sim.tx_audio(0);
    assert!(
        sent[before * 1_024..].iter().all(|v| *v == 0.0),
        "the message after the stop"
    );
    assert!(s.card.played.lock().unwrap().is_empty(), "played");
}

/// ⭐⭐ NEVER A SILENT OVER, PART WAY THROUGH EITHER, ON TCI. The SDR program stops pulling Nexus's
/// audio part way through a voice-keyer message and stays keyed: the client ends the over, and the
/// loop halts transmit with the reason on the CAT line, as it does for the session's own halts. The
/// rest of the message goes, none of it to the sound card.
#[test]
fn a_tci_over_the_sdr_program_stops_pulling_is_ended_and_says_why() {
    let mut s = TciScene::new(
        [Fault::ChronoStall {
            after: Duration::from_millis(400),
        }],
        patient(),
        phone,
    );
    s.routed("the scene");
    s.run(100);
    s.engine().send_voice(vec![0.05; 36_000]).unwrap();
    s.run_until("the message never keyed", |s| !s.keys().is_empty());
    s.run_until("the halt never reached the engine", |s| {
        s.engine().snapshot().radio.cat_detail
            == "TCI: the SDR program stopped taking Nexus's audio part way through the over, so \
                Nexus ended it."
    });
    s.run_until("the over goes on", |s| s.state.tx_until_ms.is_none());
    assert!(s.sent(0, "trx:0,false") >= 1, "our stop");
    assert_eq!(s.route().queued(), 0, "the rest of the message left queued");
    assert!(!s.engine().tx_enabled(), "TX is still on");
    assert!(s.card.played.lock().unwrap().is_empty(), "played");
    s.run(500);
    assert_eq!(s.sent(0, "trx:0,true,tci"), 1, "keyed again by itself");
}

/// ⭐ THE TRANSMIT TEE GOES WITH ITS SESSION, AND AN OVER IT CARRIED ENDS: the SDR program's
/// connection drops part way through a voice-keyer message on Nexus's audio. The loop takes the tee
/// out, the message ends there and does not come back on the next session (whose first command is
/// the unkey), and none of it goes to the sound card.
#[test]
fn a_tci_route_lost_mid_message_ends_the_message() {
    let mut s = TciScene::new(
        [Fault::DisconnectMidOver {
            after: Duration::from_millis(400),
            radio_stays_keyed: false,
        }],
        waits(),
        phone,
    );
    let key = s.daemon().key();
    s.reopen = Reopen::Client;
    s.routed("the scene");
    s.run(100);
    s.engine().send_voice(vec![0.05; 36_000]).unwrap();
    s.run_until("the message never keyed", |s| !s.keys().is_empty());
    assert!(s.state.dax_tee_id.is_some(), "premise: on the tee");
    s.run_until("no next session", |s| !s.sim.commands_from(1).is_empty());
    s.run_until("the message goes on", |s| s.state.tx_until_ms.is_none());
    assert_eq!(
        s.sim.commands_from(1)[0],
        "trx:0,false",
        "the next session's first command"
    );
    s.run(1_000);
    assert!(
        !s.sim
            .commands_from(1)
            .iter()
            .any(|c| c.starts_with("trx:0,true")),
        "the message came back on the next session"
    );
    assert!(s.card.played.lock().unwrap().is_empty(), "played");
    registry::forget(key);
}

/// The TCI radio a switch hands to the monitor leaves the loop's transmit tee behind, so its client
/// keys none of Nexus's audio from there: a stray key in a data mode is refused, and says the route
/// is not connected. While it was the loop's, the same key went out (the control).
#[test]
fn a_tci_radio_handed_to_the_monitor_keys_none_of_nexus_audio() {
    let mut s = TciScene::new([], waits(), phone);
    s.routed("the scene");
    s.run(200);
    assert!(s.state.dax_tee_id.is_some(), "premise: the tee in");
    let probe = |d: &TciDaemon| {
        use std::io::{BufRead, Write};
        let mut writer = std::net::TcpStream::connect(d.local_addr()).unwrap();
        writer.set_read_timeout(Some(WAIT)).unwrap();
        let mut reader = std::io::BufReader::new(writer.try_clone().unwrap());
        let mut ask = |verb: &str| {
            writeln!(writer, "{verb}").unwrap();
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            line
        };
        assert_eq!(ask("M PKTUSB 0"), "RPRT 0\n");
        let asked = Instant::now();
        let keyed = ask("T 1");
        let why = d.key_refused_since(asked);
        assert_eq!(ask("T 0"), "RPRT 0\n");
        (keyed, why)
    };
    let (keyed, why) = probe(s.daemon());
    assert_eq!((keyed.as_str(), why), ("RPRT 0\n", None), "control: keyed");
    s.run_until("the unkey never reported", |s| !s.daemon().keyed());
    let incoming = MonitorConn {
        id: 9,
        transport: cat_transport(4600, None),
        rig: Rig::vox(),
        rigctld_proc: None,
        last_poll: 0.0,
        ticks: 0,
        smeter_supported: None,
        freq_misses: 0,
        open_failures: 0,
        retry_after_ms: 0.0,
    };
    let want = incoming.transport.clone();
    let demoted = s
        .state
        .install_handoff_connection(&mut s.rig, incoming, &want, 2);
    let d = demoted
        .rigctld_proc
        .as_ref()
        .and_then(CatDaemon::tci)
        .expect("the TCI client in the monitor");
    let (keyed, why) = probe(d);
    assert_eq!(
        keyed, "RPRT -1\n",
        "a key of Nexus's audio from the monitor"
    );
    assert!(
        why.is_some_and(|w| w.contains("transmit audio is not connected")),
        "the route is not up there"
    );
}

/// An FT8 CQ, armed in the Digital section the way Call CQ arms it.
fn ft8_cq(e: &mut Engine) {
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.broadcast("CQ W9XYZ EN37");
}

/// ⭐ THE LOOP'S OWN CAT LINK DROPPING UNDER ITS OVER DOES NOT UNKEY IT. The loop tells the client it
/// is transmitting (the TX intent), so the shim's disconnect fail-safe stands down for a reconnect of
/// Nexus's own `Rig`, as on the CI-V and Flex daemons. The shim hangs up on a connection after three
/// lines in a row that are not rigctld protocol at all (every rigctld backend's rule against a peer
/// not speaking the protocol). The loop's own client never sends one, so three ride on the loop's
/// own connection here, behind a raw command.
#[test]
fn the_loops_own_link_dropping_under_its_over_does_not_unkey_it() {
    let mut s = TciScene::new([], waits(), phone);
    s.run(300);
    s.engine().set_ptt(true);
    s.run_until("the PTT never keyed", |s| s.sent(0, "trx:0,true") == 1);
    s.run(200);
    // The lines ride behind a raw set's verb (`W X`); what the shim answers it is beside the
    // point, and the hang-up below is the premise that they arrived.
    let _ = s.rig.send_raw_set("X\nHELP\nHELP\nHELP");
    // A read may still take an answer to a line before the hang-up as its own, an error that is no
    // link fault; the drive the shim does answer reads only on a link that is still up.
    let hung_up = (0..10).any(|_| {
        matches!(
            s.rig.read_level("RFPOWER").map_err(|e| e.kind()),
            Err(std::io::ErrorKind::UnexpectedEof
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::BrokenPipe)
        )
    });
    assert!(
        hung_up,
        "premise: the shim hung up on the loop's connection"
    );
    s.run(500);
    assert_eq!(
        s.sent(0, "trx:0,false"),
        0,
        "the reconnect unkeyed the over"
    );
    assert!(s.daemon().keyed());
    s.engine().set_ptt(false);
    s.run_until("the release never unkeyed", |s| {
        s.sent(0, "trx:0,false") >= 1
    });
}

/// ⛔ THE LOOP'S OWN KEYS ARE HELD TO THE SDR PROGRAM'S ADMISSION: on a server that does not let this
/// transmitter transmit (`tx_enable:0,false`), the operator's PTT and Tune put nothing on the wire
/// that keys, and the PTT line says the key was refused. No key of the loop's reaches the SDR
/// program but through the TCI client's admission.
#[test]
fn the_loops_keys_are_held_to_the_sdr_programs_admission() {
    let mut s = TciScene::new([Fault::TxEnableFalse], waits(), phone);
    s.run(300);
    s.engine().set_ptt(true);
    s.run(500);
    assert_eq!(s.sent(0, "trx:0,true"), 0, "the PTT keyed");
    assert!(
        s.engine().snapshot().radio.audio_error.is_some(),
        "the PTT line said nothing"
    );
    s.engine().set_ptt(false);
    s.run(200);
    s.engine().set_tune(true);
    s.run(500);
    assert_eq!(s.sent(0, "tune:0,true"), 0, "the tune keyed");
    let line = s.engine().snapshot().radio.audio_error.unwrap_or_default();
    assert!(line.starts_with("Tune not keyed:"), "{line}");
    assert!(s.keys().is_empty(), "the radio keyed");
}

/// ⭐ THE OPERATOR'S PTT KEYS THE SDR PROGRAM'S MICROPHONE, AND ITS TRANSMIT METERS REACH THE ENGINE:
/// SWR and the peak watts from the program's `tx_sensors`, while keyed.
#[test]
fn the_operators_ptt_is_metered_from_the_sdr_programs_sensors() {
    let mut s = TciScene::new([], waits(), phone);
    s.sim.set_meters(Meters {
        smeter_dbm: -73.0,
        mic_dbm: -30.0,
        max_power_w: 100.0,
        swr: 1.5,
    });
    s.run(300);
    s.engine().set_ptt(true);
    s.run_until("no SWR while keyed", |s| {
        s.engine().snapshot().radio.tx_swr == Some(1.5)
    });
    s.run_until("no power while keyed", |s| {
        s.engine().snapshot().radio.tx_po_w.is_some_and(|w| w > 0.0)
    });
    s.engine().set_ptt(false);
    s.run_until("the release never unkeyed", |s| {
        s.sent(0, "trx:0,false") >= 1
    });
}

/// ⭐ TUNE ON TCI IS THE SDR PROGRAM'S OWN CARRIER (`tune:0,true`), not a tone of Nexus's: nothing is
/// played, the PTT is never asked, and the release ends it (`tune:0,false`).
#[test]
fn tune_is_the_sdr_programs_own_carrier() {
    let mut s = TciScene::new([], waits(), phone);
    s.run(300);
    s.engine().set_tune(true);
    s.run_until("the tune never keyed", |s| !s.keys().is_empty());
    assert_eq!(s.keys()[0], (Keyer::Client(0), SimSource::Tune));
    assert_eq!(s.sent(0, "tune:0,true"), 1);
    assert_eq!(s.sent(0, "trx:0,true"), 0, "the PTT was asked");
    s.run(300);
    assert!(
        s.card.played.lock().unwrap().is_empty(),
        "a tone was played"
    );
    s.engine().set_tune(false);
    s.run_until("the release never ended the carrier", |s| {
        s.sent(0, "tune:0,false") >= 1
    });
}

// ── What the client knows that `rig` does not ────────────────────────────────────────────────

/// ⭐ STOP TX SENDS THE UNKEY AGAIN WHILE THE SDR PROGRAM HAS NOT CONFIRMED OURS: the loop counts the
/// client's belief as `rig.keyed` (`tci_keyed`), so a stop the program has not reported taken is
/// sent once more.
#[test]
fn stop_tx_unkeys_again_while_the_sdr_program_has_not_confirmed_our_unkey() {
    let mut s = TciScene::new([Fault::StuckTrx], patient(), phone);
    s.run(300);
    s.engine().set_ptt(true);
    s.run_until("the PTT never keyed", |s| s.sent(0, "trx:0,true") == 1);
    s.engine().set_ptt(false);
    s.run_until("the release never unkeyed", |s| {
        s.sent(0, "trx:0,false") == 1
    });
    s.run(300);
    assert!(
        s.daemon().keyed(),
        "premise: the program did not confirm it"
    );
    assert!(!s.rig.keyed, "premise: the loop's rig believes it unkeyed");
    s.engine().halt_tx();
    s.run_until("Stop TX sent no second unkey", |s| {
        s.sent(0, "trx:0,false") >= 2
    });
}

/// ⭐ A REBUILD WHILE THE SDR PROGRAM HAS NOT CONFIRMED OUR UNKEY CARRIES THE KEYED BELIEF onto the
/// fresh rig (the stuck-TX invariant), so the idle self-heal keeps sending the unkey where the new
/// channel did not take it. And the old session is sent a stop of the teardown's own before its
/// close: the loop's unkey through the still-live rig, the daemon's stops, or both; the release's
/// earlier stop does not count.
#[test]
fn a_rebuild_while_our_unkey_is_unconfirmed_carries_the_keyed_belief() {
    let mut s = TciScene::new([Fault::StuckTrx], patient(), phone);
    let key = s.daemon().key();
    s.run(300);
    s.engine().set_ptt(true);
    s.run_until("the PTT never keyed", |s| s.sent(0, "trx:0,true") == 1);
    s.engine().set_ptt(false);
    s.run_until("the release never unkeyed", |s| {
        s.sent(0, "trx:0,false") == 1
    });
    s.run(300);
    assert!(
        s.daemon().keyed(),
        "premise: the program did not confirm it"
    );
    assert!(!s.rig.keyed, "premise: the loop's rig believes it unkeyed");
    // A saved change to the connection, and a new channel that does not take the unkey.
    let (addr, _log) = refusing_unkey_rigctld();
    s.reopen = Reopen::Rigctld(addr);
    let stops_before = s.sent(0, "trx:0,false");
    {
        let mut e = s.engine();
        let mut settings = e.settings().clone();
        settings.tci_dialect = "expertsdr3".into();
        e.apply_settings(settings);
    }
    s.step();
    assert_eq!(s.rebuilds.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(
        s.rig.keyed,
        "the fresh rig forgot a transmitter the SDR program never reported unkeyed"
    );
    // The old session: a stop of the teardown's own before its close.
    assert!(
        s.sim.wait_for(WAIT, |log| log
            .iter()
            .any(|l| matches!(l.event, SimEvent::Closed { conn: 0, .. }))),
        "the old session never closed"
    );
    let log = s.sim.log();
    let closed = log
        .iter()
        .find_map(|l| match l.event {
            SimEvent::Closed { conn: 0, .. } => Some(l.at),
            _ => None,
        })
        .expect("the close");
    let stops = log
        .iter()
        .filter(|l| {
            l.at < closed
                && matches!(&l.event, SimEvent::Command { conn: 0, text, .. } if text == "trx:0,false")
        })
        .count();
    assert!(
        stops > stops_before,
        "no stop between the change and the close ({stops} before the close, {stops_before} before \
         the change)"
    );
    registry::forget(key);
}

/// A rigctld that refuses the unkey (`T 0` → `RPRT -1`) and answers everything else, logging what
/// it was sent: a new channel the post-rebuild unkey does not land on.
fn refusing_unkey_rigctld() -> (String, Arc<Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let log2 = log.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let Ok(read) = stream.try_clone() else {
                continue;
            };
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let l = line.trim().to_string();
                log2.lock().unwrap().push(l.clone());
                let reply = match l.as_str() {
                    "T 0" => "RPRT -1\n".to_string(),
                    "f" => "14074000\n".to_string(),
                    "t" => "0\n".to_string(),
                    _ => "RPRT 0\n".to_string(),
                };
                if stream.write_all(reply.as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    (addr, log)
}

/// ⭐ THE LINK LOST MID-OVER IS THE ALARM, AND THE NEXT SESSION STOPS FIRST: the SDR program's
/// connection drops while our key is up and the radio stays keyed; the operator is told the radio
/// may still be transmitting, the loop starts a new session at once, and that session's first
/// command is the unkey.
#[test]
fn a_link_lost_mid_over_is_the_alarm_and_the_next_session_stops_first() {
    let mut s = TciScene::new(
        [Fault::DisconnectMidOver {
            after: Duration::from_millis(300),
            radio_stays_keyed: true,
        }],
        waits(),
        phone,
    );
    let key = s.daemon().key();
    s.reopen = Reopen::Client;
    s.run(300);
    s.engine().set_ptt(true);
    s.run_until("no alarm", |s| !s.engine().snapshot().tx_alarms.is_empty());
    let alarm = s.engine().snapshot().tx_alarms[0].text.clone();
    assert!(
        alarm.starts_with(
            "the connection to the SDR program was lost during a transmission — it may still be \
             transmitting"
        ),
        "{alarm}"
    );
    s.engine().set_ptt(false);
    s.run_until("no next session", |s| !s.sim.commands_from(1).is_empty());
    assert_eq!(
        s.sim.commands_from(1)[0],
        "trx:0,false",
        "the next session's first command"
    );
    registry::forget(key);
}

/// ⭐ AN OVER THE SDR PROGRAM ENDS BY ITSELF HALTS TRANSMIT AND SAYS WHY: the program stops mid-over;
/// the session sends its stops, and the loop halts the engine's transmission (the operator's PTT
/// released, nothing re-keys by itself) with the reason on the transmit notices' lane.
#[test]
fn an_over_the_sdr_program_ends_by_itself_halts_transmit_and_says_why() {
    let mut s = TciScene::new(
        [Fault::ServerStop {
            after: Duration::from_millis(300),
        }],
        patient(),
        phone,
    );
    s.run(300);
    s.engine().set_ptt(true);
    s.run_until("the PTT never keyed", |s| s.sent(0, "trx:0,true") == 1);
    s.run_until("the halt never reached the engine", |s| {
        !s.engine().manual_ptt()
    });
    assert_eq!(
        s.engine().snapshot().radio.cat_detail,
        "TCI: the SDR program stopped, so Nexus ended the over."
    );
    s.run(500);
    assert_eq!(s.sent(0, "trx:0,true"), 1, "nothing keyed again by itself");
}
