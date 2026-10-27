//! The bench's road switch, end to end over loopback (test builds only): this computer in client
//! mode, through Tauri's own IPC on its mock runtime, driving the shack's real channel, command
//! lane and Stop road, with the switch moved from the screen as the operator moves it. Nothing of
//! the shack knows the switch: it is the roadside every other road test drives.
//!
//! What it shows, by value: with Stop's road blocked, a Stop goes by the command link, is
//! acknowledged there and stops each kind of over started from afar (a held press ends by itself
//! first, its holds going by that road); with the link blocked, Stop's road alone stops each one,
//! and the link then goes on where it stopped; and a start is refused, in its words, while either
//! road is blocked.
use super::super::computer::{behind_a_port, paired_record};
use super::*;
use crate::bench_road_switch::{with_its_command, Position};
use crate::client_mode::tests::{ask, eventually, scratch, send};
use crate::client_mode::{Client, Find, Phase, ThisComputer};
use crate::remote_service::vault::PairedStation;
use std::sync::mpsc::Receiver;
use tauri::ipc::Invoke;
use tauri::test::MockRuntime;
use tauri::Manager;

/// A screen's answer, on its way.
type Answering = Receiver<Result<Value, Value>>;

/// Waits up to `within` for `done`.
fn within(what: &str, within: Duration, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + within;
    while !done() {
        assert!(Instant::now() < deadline, "never: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The shack's handler for these tests, on its own engine, as `on_the_engine`'s: the starts wired
/// from afar and the cockpits' stops, each recorded in order (the feeds' own polls left out). A
/// Stop on the command link (Stop TX, the keyer's Stop) waits at `held` while it is shut: the
/// stand-in for a link Stop that has reached the shack and not yet run, so whatever stops the over
/// meanwhile came by Stop's road.
fn holding_the_link_stops(r: &mut Roadside, held: Gate) -> Arc<Mutex<Vec<String>>> {
    let ran = Arc::new(Mutex::new(Vec::new()));
    let (engine, record) = (r.s.shared.engine.clone(), ran.clone());
    let runner: crate::command_dispatch::Runner = Arc::new(
        move |cmd: String, args: Value, answer: crate::command_dispatch::Answer| {
            // Every feed's poll, read from the feeds' own table so a feed added later is left out
            // too: the RTTY scope's was recorded as a press once it came.
            if tempo_client::wire::FEEDS
                .iter()
                .any(|feed| feed.command == cmd)
            {
                return answer(Ok(json!({})));
            }
            record.lock().unwrap().push(cmd.clone());
            if matches!(cmd.as_str(), "halt_tx" | "stop_voice") {
                let (shut, opened) = &*held;
                let mut shut = shut.lock().unwrap();
                while *shut {
                    shut = opened.wait(shut).unwrap();
                }
            }
            let mut e = engine.lock().unwrap();
            let done = match cmd.as_str() {
                "set_tx_enabled" => {
                    e.set_tx_enabled(args["enabled"].as_bool() == Some(true));
                    Ok(())
                }
                "call_cq" => e.start_cq(None),
                "play_voice_message" => e.send_voice(vec![0.1; 12_000]),
                "halt_tx" => {
                    e.halt_tx();
                    Ok(())
                }
                "stop_voice" => {
                    e.stop_voice();
                    Ok(())
                }
                _ => Ok(()),
            };
            answer(done.map(|()| json!({ "ran": cmd })))
        },
    );
    let voided = r.s.shared.engine.lock().unwrap().remote_starts_voided();
    r.s.shared.commands = crate::remote_service::lan::commands::Commands::new(runner, voided);
    ran
}

/// A computer in client mode driving the roadside's station over loopback, with the bench's switch
/// among its screen's commands. The screen's own poll runs every 100 ms, as the main window's does,
/// so presence is live at the shack until the drive is over.
struct Driving {
    client: Arc<Client>,
    main: tauri::WebviewWindow<MockRuntime>,
    driving: tauri::async_runtime::JoinHandle<()>,
    polling: Arc<AtomicBool>,
    dir: std::path::PathBuf,
    _app: tauri::App<MockRuntime>,
    _listening: watch::Sender<bool>,
}

impl Drop for Driving {
    fn drop(&mut self) {
        self.polling.store(false, Ordering::SeqCst);
        self.driving.abort();
        tauri::async_runtime::block_on(self.client.let_go());
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn driving(r: &Roadside, name: &str) -> Driving {
    let (at, listening) = tauri::async_runtime::block_on(async {
        r.s.shared
            .authority
            .permit_station(&r.s.device, true)
            .unwrap();
        let (stopper, stop) = watch::channel(false);
        (behind_a_port(&r.s, stop).await, stopper)
    });
    let record = paired_record(&r.s, at);
    let dir = scratch(name);
    let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
    let client = Arc::new(Client::new(STATION.to_string(), this));
    // The app's own handler here: the switch's command, in front of one that runs nothing else.
    let local = with_its_command(Box::new(|invoke: Invoke<MockRuntime>| {
        invoke.resolver.resolve("ran here");
        true
    }));
    let routed = Arc::clone(&client);
    let app = tauri::test::mock_builder()
        .manage(Arc::clone(&client))
        .invoke_handler(move |invoke| routed.handle(invoke, &*local))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("a mock app");
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("the main window");
    let (device, key, pkcs8) = (
        record.device_id.clone(),
        record.station_key.clone(),
        record.pkcs8.clone(),
    );
    let find: Find = Arc::new(move |_| {
        Ok(Some(PairedStation {
            station_id: STATION.into(),
            device_id: device.clone(),
            station_key: key.clone(),
            pkcs8: pkcs8.clone(),
            addresses: vec![at.to_string()],
        }))
    });
    let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
        main.app_handle().clone(),
        find,
        Arc::new(|_| Ok(Vec::new())),
        Some(tempo_stream::lan::Typed::At(at)),
    ));
    let polling = Arc::new(AtomicBool::new(true));
    {
        let (main, polling) = (main.clone(), polling.clone());
        std::thread::spawn(move || {
            while polling.load(Ordering::SeqCst) {
                let _ = send(&main, "get_snapshot", json!({})).recv_timeout(Duration::from_secs(1));
                std::thread::sleep(Duration::from_millis(100));
            }
        });
    }
    eventually("control taken", || client.status().phase == Phase::Live);
    within("presence live at the shack", Duration::from_secs(5), || {
        r.s.shared
            .engine
            .lock()
            .unwrap()
            .remote_presence_live(Instant::now())
    });
    Driving {
        client,
        main,
        driving,
        polling,
        dir,
        _app: app,
        _listening: listening,
    }
}

/// The switch moved from the screen, as its control moves it: where it is now.
fn switch_to(d: &Driving, position: Position) -> Result<Value, Value> {
    ask(&d.main, "bench_road_switch", json!({ "to": position }))
}

/// The PTT held on the screen: re-asserted every 100 ms, as Phone's screen does, until let go.
struct Holding {
    on: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Holding {
    fn press(main: &tauri::WebviewWindow<MockRuntime>) -> Self {
        let on = Arc::new(AtomicBool::new(true));
        let thread = {
            let (main, on) = (main.clone(), on.clone());
            std::thread::spawn(move || {
                while on.load(Ordering::SeqCst) {
                    let _ = send(&main, "set_ptt", json!({ "on": true }))
                        .recv_timeout(Duration::from_secs(1));
                    std::thread::sleep(Duration::from_millis(100));
                }
            })
        };
        Self {
            on,
            thread: Some(thread),
        }
    }
}

impl Drop for Holding {
    fn drop(&mut self) {
        self.on.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Enable TX from the screen, once this computer has heard its presence on Stop's road: the first
/// start a cockpit makes from afar. Until then the start is refused here, and nothing is sent.
fn enable_tx(d: &Driving) {
    within(
        "Enable TX runs at the shack",
        Duration::from_secs(5),
        || {
            ask(&d.main, "set_tx_enabled", json!({ "enabled": true }))
                == Ok(json!({ "ran": "set_tx_enabled" }))
        },
    );
}

/// What the screen starts from afar, each kind as its cockpit starts it: FT armed and a CQ called,
/// the voice keyer's message, or the PTT held. Each is on the air at the shack, or armed, before
/// this returns.
fn start(r: &Roadside, d: &Driving, kind: &str) -> Option<Holding> {
    enable_tx(d);
    match kind {
        "ft" => {
            assert_eq!(
                ask(&d.main, "call_cq", json!({})),
                Ok(json!({ "ran": "call_cq" }))
            );
            assert!(r.s.shared.engine.lock().unwrap().tx_enabled(), "{kind}");
            None
        }
        "voice" => {
            assert_eq!(
                ask(&d.main, "play_voice_message", json!({ "slot": 1 })),
                Ok(json!({ "ran": "play_voice_message" }))
            );
            assert!(
                r.s.shared.engine.lock().unwrap().tx_owner().is_some(),
                "{kind}"
            );
            None
        }
        _ => {
            let holding = Holding::press(&d.main);
            within(
                "the press armed at the shack",
                Duration::from_secs(5),
                || r.s.shared.engine.lock().unwrap().mic_armed(),
            );
            Some(holding)
        }
    }
}

/// Whether `kind`, started from afar, is still on the air (or, for the press, armed) at the shack.
fn on_the_air(r: &Roadside, kind: &str) -> bool {
    let e = r.s.shared.engine.lock().unwrap();
    match kind {
        "ft" => e.tx_enabled(),
        "voice" => e.tx_owner().is_some(),
        _ => e.mic_armed(),
    }
}

/// The cockpit's stop for `kind`, pressed on the screen: each command sent as its button sends it
/// (a Stop goes by both roads), and where each answer will come.
fn stop_from_the_screen(d: &Driving, kind: &str) -> Vec<(&'static str, Answering)> {
    cockpit_stop(kind)
        .into_iter()
        .map(|cmd| (cmd, send(&d.main, cmd, json!({}))))
        .collect()
}

/// What the shack's handler ran after the first `before` it had run, in name order: the stops of
/// one press arrive together and run on threads of their own, in either order.
fn ran_since(record: &Arc<Mutex<Vec<String>>>, before: usize) -> Vec<String> {
    let mut since = ran(record).split_off(before);
    since.sort();
    since
}

/// The stops `kind`'s cockpit sends, in name order.
fn stops_of(kind: &str) -> Vec<String> {
    let mut stops: Vec<String> = cockpit_stop(kind).into_iter().map(str::to_string).collect();
    stops.sort();
    stops
}

/// Each kind stopped as the local Stop TX leaves it.
fn stopped(r: &Roadside, kind: &str) {
    let mut e = r.s.shared.engine.lock().unwrap();
    assert!(!e.mic_armed(), "{kind}: the microphone over");
    match kind {
        "ft" | "voice" => assert_stopped(&mut e, kind),
        _ => assert!(!e.tx_enabled(), "{kind}"),
    }
}

/// ★ With Stop's road blocked from the screen, a Stop goes by the command link, is acknowledged
/// there and stops each kind of over started from afar. The Stop pressed went by both roads: while
/// the link's copy waits at the shack, the FT over and the voice keyer stay on the air, so the road
/// carried nothing; let through, it runs, the screen has its answer, and the over is off. A held
/// press ends by itself first, 200 ms after the last hold reached the shack, since its holds go by
/// the blocked road. CONTROL: at Normal, with the link's Stop held the same way, the road stops the
/// FT over at once.
#[test]
fn with_the_stop_road_blocked_a_stop_on_the_link_stops_each_kind_of_over() {
    let _alone = alone();
    for (kind, position) in [
        ("ft", Position::Normal),
        ("ft", Position::BlockStopRoad),
        ("voice", Position::BlockStopRoad),
        ("press", Position::BlockStopRoad),
    ] {
        let app = App::new();
        let mut r = roadside(&app);
        let held = gate(true);
        let record = holding_the_link_stops(&mut r, held.clone());
        if kind == "press" {
            r.s.shared
                .engine
                .lock()
                .unwrap()
                .set_operating_mode("phone", false);
        }
        let d = driving(&r, "bench-stop-road-blocked");
        let done = Arc::new(AtomicBool::new(false));
        let radio = radio_loop(
            r.s.shared.engine.clone(),
            Over::Voice,
            done.clone(),
            Instant::now() + 4 * PRESENCE,
        );
        let holding = start(&r, &d, kind);
        assert_eq!(switch_to(&d, position), Ok(json!(position)), "{kind}");
        if holding.is_some() {
            let blocked = Instant::now();
            within("the press ended by itself", Duration::from_secs(2), || {
                !on_the_air(&r, kind)
            });
            let took = blocked.elapsed();
            eprintln!("bench: a held press with Stop's road blocked ended after {took:?}");
            assert!(
                took < PRESENCE / 5,
                "{took:?}: not the held press's own 200 ms but presence's lapse"
            );
        }
        let pressed = Instant::now();
        let before = ran(&record).len();
        let stops = stop_from_the_screen(&d, kind);
        if position == Position::Normal {
            assert!(
                tauri::async_runtime::block_on(came_to_a_stop(&r.s)),
                "CONTROL: {kind} stopped by the road while the link's Stop waits"
            );
            eprintln!(
                "bench: at Normal the road stopped {kind} in {:?}",
                pressed.elapsed()
            );
        } else if holding.is_none() {
            std::thread::sleep(NOTHING);
            assert!(
                on_the_air(&r, kind),
                "{kind}: off while the link's Stop waited, so Stop's road carried it"
            );
            assert_eq!(
                ran_since(&record, before),
                stops_of(kind),
                "{kind}: the link's Stop reached the shack, and waits there"
            );
        }
        set_gate(&held, false);
        for (cmd, answer) in stops {
            assert_eq!(
                answer.recv_timeout(Duration::from_secs(5)),
                Ok(Ok(json!({ "ran": cmd }))),
                "{kind}: the link acknowledged {cmd}"
            );
        }
        assert!(
            tauri::async_runtime::block_on(came_to_a_stop(&r.s)),
            "{kind} stopped"
        );
        drop(holding);
        done.store(true, Ordering::SeqCst);
        radio.join().unwrap();
        stopped(&r, kind);
        assert_eq!(ran_since(&record, before), stops_of(kind), "{kind}");
    }
}

/// ★ With the command link blocked from the screen, Stop's road alone stops each kind of over
/// started from afar, at once, nowhere near presence's lapse. Nothing reaches the shack's handler
/// while the link is blocked, and the screen's own Stop call waits unanswered; opened again, the
/// link goes on where it stopped: the Stop it held runs at the shack, and the screen has its answer.
/// CONTROL: before the block, every start went by the link and ran.
#[test]
fn with_the_link_blocked_a_stop_on_the_stop_road_stops_each_kind_of_over() {
    let _alone = alone();
    for kind in ["ft", "voice", "press"] {
        let app = App::new();
        let mut r = roadside(&app);
        let record = holding_the_link_stops(&mut r, gate(false));
        if kind == "press" {
            r.s.shared
                .engine
                .lock()
                .unwrap()
                .set_operating_mode("phone", false);
        }
        let d = driving(&r, "bench-link-blocked");
        let done = Arc::new(AtomicBool::new(false));
        let radio = radio_loop(
            r.s.shared.engine.clone(),
            Over::Voice,
            done.clone(),
            Instant::now() + 4 * PRESENCE,
        );
        let holding = start(&r, &d, kind);
        let before = ran(&record);
        assert!(
            !before.is_empty(),
            "CONTROL: {kind}'s start ran by the link"
        );
        assert_eq!(switch_to(&d, Position::BlockLink), Ok(json!("blockLink")));
        let pressed = Instant::now();
        let stops = stop_from_the_screen(&d, kind);
        assert!(
            tauri::async_runtime::block_on(came_to_a_stop(&r.s)),
            "{kind}: stopped by the road"
        );
        let took = pressed.elapsed();
        eprintln!("bench: with the link blocked the road stopped {kind} in {took:?}");
        assert!(
            took < Duration::from_secs(1),
            "{kind}: {took:?}, so not the road at once"
        );
        for (cmd, answer) in &stops {
            assert!(
                answer.recv_timeout(NOTHING).is_err(),
                "{kind}: {cmd} was answered while the link was blocked"
            );
        }
        assert_eq!(
            ran(&record),
            before,
            "{kind}: the shack heard the link while it was blocked"
        );
        drop(holding);
        assert_eq!(switch_to(&d, Position::Normal), Ok(json!("normal")));
        for (cmd, answer) in stops {
            assert_eq!(
                answer.recv_timeout(Duration::from_secs(5)),
                Ok(Ok(json!({ "ran": cmd }))),
                "{kind}: the link went on where it stopped"
            );
        }
        done.store(true, Ordering::SeqCst);
        radio.join().unwrap();
        stopped(&r, kind);
    }
}

/// ★ A start is refused, in its words, while either road is blocked from the screen: the design's
/// rules meet a blocked road as they would meet a firewall here. With Stop's road blocked, presence
/// as this computer last heard it lapses within five seconds; with the link blocked, the state the
/// road's heartbeat names stops moving, so the shack stops renewing presence two seconds after it
/// and it lapses within about seven. Then each start the screen can make from afar (Enable TX, a
/// CQ, the keyer's message, the PTT) is refused `noPresence`, and nothing runs at the shack.
/// CONTROL: the same Enable TX ran before the block, and runs again once the switch is at Normal.
#[test]
fn a_start_is_refused_while_either_road_is_blocked() {
    let _alone = alone();
    for (position, lapses_within) in [
        (Position::BlockStopRoad, PRESENCE),
        (Position::BlockLink, FRESH + PRESENCE),
    ] {
        let app = App::new();
        let mut r = roadside(&app);
        let record = holding_the_link_stops(&mut r, gate(false));
        r.s.shared
            .engine
            .lock()
            .unwrap()
            .set_operating_mode("phone", false);
        let d = driving(&r, "bench-starts");
        enable_tx(&d);
        assert_eq!(switch_to(&d, position), Ok(json!(position)));
        std::thread::sleep(lapses_within + Duration::from_millis(500));
        let before = ran(&record);
        for (cmd, args) in [
            ("set_tx_enabled", json!({ "enabled": true })),
            ("call_cq", json!({})),
            ("play_voice_message", json!({ "slot": 1 })),
            ("set_ptt", json!({ "on": true })),
        ] {
            assert_eq!(
                ask(&d.main, cmd, args),
                Err(json!("nexusClient:noPresence")),
                "{position:?}: {cmd}"
            );
        }
        assert_eq!(
            ran(&record),
            before,
            "{position:?}: a start ran at the shack"
        );
        assert_eq!(switch_to(&d, Position::Normal), Ok(json!("normal")));
        within(
            "CONTROL: a start runs again",
            Duration::from_secs(15),
            || {
                ask(&d.main, "set_tx_enabled", json!({ "enabled": true }))
                    == Ok(json!({ "ran": "set_tx_enabled" }))
            },
        );
    }
}
