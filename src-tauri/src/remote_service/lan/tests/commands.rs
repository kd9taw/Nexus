//! Protocol 2's command lane at the shack, driven from a computer's side of the socket: the build
//! it opens on, the calls it admits and refuses, the journal across a reconnect, Stop ahead of
//! everything, calls and answers in pieces, the feeds and their numbers, and the events. The app's
//! handler is a stand-in that records every call it runs, so "nothing ran" is read off it.
use super::super::commands::{Commands, InTurn, PENDING, STARTED, STOP_RUNS};
use super::super::wire::{self, Assembly, Inflate, Whole};
use super::*;
use crate::command_classes::{class_of, Class};
use crate::command_dispatch::{Answer, Runner, IN_FLIGHT, PER_COMMAND};
use std::sync::Condvar;

/// The two commands the stand-in holds until the test lets them go: slow calls, enough of them
/// to fill every slot without passing the cap on one command.
const SLOW: [&str; 2] = ["ask_log", "search_parks"];

/// The app's own handler as a test stands it in. It records each call it runs, in order; holds
/// the [`SLOW`] ones while the test holds them; answers `get_snapshot` with a count of its calls,
/// `get_settings` with a fixed copy, `dxcc_entity_names` with 200 KB; and echoes anything else.
#[derive(Clone)]
pub(super) struct App {
    ran: Arc<Mutex<Vec<(String, Value)>>>,
    held: Arc<(Mutex<bool>, Condvar)>,
    snapshots: Arc<AtomicUsize>,
}

impl App {
    pub(super) fn new() -> Self {
        Self {
            ran: Default::default(),
            held: Arc::new((Mutex::new(false), Condvar::new())),
            snapshots: Default::default(),
        }
    }

    pub(super) fn holding() -> Self {
        let app = Self::new();
        *app.held.0.lock().unwrap() = true;
        app
    }

    pub(super) fn release(&self) {
        *self.held.0.lock().unwrap() = false;
        self.held.1.notify_all();
    }

    /// The commands it ran, in order, the snapshot polls left out.
    pub(super) fn ran(&self) -> Vec<String> {
        self.ran
            .lock()
            .unwrap()
            .iter()
            .map(|(cmd, _)| cmd.clone())
            .filter(|cmd| cmd != "get_snapshot")
            .collect()
    }

    fn runner(&self) -> Runner {
        let app = self.clone();
        Arc::new(move |cmd: String, args: Value, answer: Answer| {
            app.ran.lock().unwrap().push((cmd.clone(), args.clone()));
            let value = match cmd.as_str() {
                slow if SLOW.contains(&slow) => {
                    let (held, let_go) = &*app.held;
                    let mut held = held.lock().unwrap();
                    while *held {
                        held = let_go.wait(held).unwrap();
                    }
                    json!("answered")
                }
                "get_snapshot" => json!({"n": app.snapshots.fetch_add(1, Ordering::SeqCst) + 1}),
                "get_settings" => json!({"settings":"as read"}),
                "dxcc_entity_names" => json!("x".repeat(200_000)),
                _ => json!({"ran": cmd, "args": args}),
            };
            answer(Ok(value))
        })
    }
}

/// Lets the slow calls go when the test ends, however it ends: a thread left waiting would hold
/// the test's runtime open.
pub(super) struct LetGo(pub(super) App);
impl Drop for LetGo {
    fn drop(&mut self) {
        self.0.release();
    }
}

/// A shack whose calls run on `app`.
pub(super) fn running(app: &App) -> Shack {
    let mut s = shack(home());
    let voided = s.shared.engine.lock().unwrap().remote_starts_voided();
    s.shared.commands = Commands::new(app.runner(), voided);
    s
}

/// A whole message from the shack, its pieces put together, the status line left out.
#[derive(Debug)]
enum Got {
    Text(Value),
    Feed(Vec<u8>),
    Closed,
    Silent,
}

/// The next whole message within `wait`.
async fn got_within(socket: &mut Client, pieces: &mut Assembly, wait: Duration) -> Got {
    loop {
        let whole = match tokio::time::timeout(wait, socket.next()).await {
            Err(_) => return Got::Silent,
            Ok(Some(Ok(Message::Text(text)))) => Whole::Text(text.to_string()),
            Ok(Some(Ok(Message::Binary(bytes)))) if wire::is_piece(&bytes) => {
                match pieces.piece(&bytes) {
                    Ok(Some(whole)) => whole,
                    Ok(None) => continue,
                    Err(()) => panic!("a piece no shack sends"),
                }
            }
            Ok(Some(Ok(Message::Binary(bytes)))) => Whole::Binary(bytes.to_vec()),
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
            Ok(_) => return Got::Closed,
        };
        match whole {
            Whole::Text(text) => {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["type"] != "status" {
                    return Got::Text(value);
                }
            }
            Whole::Binary(bytes) => return Got::Feed(bytes),
        }
    }
}

async fn got(socket: &mut Client, pieces: &mut Assembly) -> Got {
    got_within(socket, pieces, Duration::from_secs(5)).await
}

/// The answer to call `id`, whatever comes before it.
async fn answer(socket: &mut Client, pieces: &mut Assembly, id: u64) -> Value {
    loop {
        match got(socket, pieces).await {
            Got::Text(value) if value["type"] == "result" && value["id"] == id => return value,
            Got::Text(_) | Got::Feed(_) => {}
            other => panic!("no answer to call {id}: {other:?}"),
        }
    }
}

/// The answers to every one of `ids`, in whatever order they come.
async fn answers(
    socket: &mut Client,
    pieces: &mut Assembly,
    ids: impl IntoIterator<Item = u64>,
) -> std::collections::BTreeMap<u64, Value> {
    let mut waiting: std::collections::BTreeSet<u64> = ids.into_iter().collect();
    let mut answered = std::collections::BTreeMap::new();
    while !waiting.is_empty() {
        match got(socket, pieces).await {
            Got::Text(value) if value["type"] == "result" => {
                if let Some(id) = value["id"].as_u64().filter(|id| waiting.remove(id)) {
                    answered.insert(id, value);
                }
            }
            Got::Text(_) | Got::Feed(_) => {}
            other => panic!("no answer to {waiting:?}: {other:?}"),
        }
    }
    answered
}

async fn send_call(socket: &mut Client, id: u64, cmd: &str, args: Value) {
    send(socket, json!({"type":"call","id":id,"cmd":cmd,"args":args})).await;
}

/// One call and its answer.
async fn call(
    socket: &mut Client,
    pieces: &mut Assembly,
    id: u64,
    cmd: &str,
    args: Value,
) -> Value {
    send_call(socket, id, cmd, args).await;
    answer(socket, pieces, id).await
}

/// The lane opened with `profile`, `app` and `contract` for the run `run`: what the shack answered.
async fn open(socket: &mut Client, profile: &str, app: &str, contract: &str, run: u64) -> Value {
    send(
        socket,
        json!({"type":"open","profile":profile,"app":app,"contract":contract,"run":run}),
    )
    .await;
    next(socket).await
}

/// Welcomed, in control, and the lane opened with this build for the run `run`.
async fn opened(s: &Shack, socket: &mut Client, run: u64) {
    in_control(s, socket).await;
    assert_eq!(
        open(socket, "desktop", wire::APP, wire::contract(), run).await,
        json!({"type":"opened"})
    );
}

fn refused(id: u64, code: &str) -> Value {
    json!({"type":"result","id":id,"refused":code})
}

/// ★ The same build at both ends (ruled 2026-10-05): the welcome names the shack's build, and the
/// lane opens only for that app and contract. A newer app is told to update the station, an older
/// one the computer, another build of the same version is `otherBuild`, and a kind of client this
/// build does not serve (a phone, known by name, or a kind it does not know) is told to update the
/// station. A call sent straight behind a refused open never runs, and the connection closes.
/// CONTROL: this build opens.
#[tokio::test]
async fn the_lane_opens_only_on_the_same_build_and_says_which_side_to_update() {
    let app = App::new();
    let s = running(&app);
    let (_stop, stop) = watch::channel(false);
    let another = "0".repeat(64);
    for (profile, version, contract, side) in [
        ("desktop", "999.0.0", wire::contract(), "updateStation"),
        ("desktop", "0.0.1", wire::contract(), "updateComputer"),
        ("desktop", wire::APP, another.as_str(), "otherBuild"),
        ("mobile", wire::APP, wire::contract(), "updateStation"),
        ("tablet", wire::APP, wire::contract(), "updateStation"),
    ] {
        let (task, socket) = dial(&s, &s.computer.key, PEER, stop.clone()).await;
        let mut socket = socket.unwrap();
        let welcome = hello(&mut socket, VERSIONS).await;
        assert_eq!(
            (welcome["app"].as_str(), welcome["contract"].as_str()),
            (Some(wire::APP), Some(wire::contract())),
            "{welcome}"
        );
        send(
            &mut socket,
            json!({"type":"open","profile":profile,"app":version,"contract":contract,"run":1}),
        )
        .await;
        send_call(&mut socket, 1, "get_settings", json!({})).await;
        let mut pieces = Assembly::new(wire::ANSWER_BYTES);
        match got(&mut socket, &mut pieces).await {
            Got::Text(value) => assert_eq!(
                value,
                json!({"type":"refused","reason":side}),
                "{profile} {version}"
            ),
            other => panic!("{profile} {version}: {other:?}"),
        }
        assert!(
            matches!(got(&mut socket, &mut pieces).await, Got::Closed),
            "{profile} {version}: left open"
        );
        task.await.unwrap();
    }
    assert!(
        app.ran().is_empty(),
        "ran on a refused lane: {:?}",
        app.ran()
    );
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    opened(&s, &mut socket, 1).await;
}

/// ★ A window's session never reaches the command lane: before the lane is opened, a call, a
/// question after outcomes and a watch each close the connection, and nothing runs. CONTROL: on
/// an opened lane the same call runs.
#[tokio::test]
async fn a_session_that_never_opened_the_lane_runs_nothing() {
    let app = App::new();
    let s = running(&app);
    let (_stop, stop) = watch::channel(false);
    for message in [
        json!({"type":"call","id":1,"cmd":"get_settings","args":{}}),
        json!({"type":"outcomes","ids":[1]}),
        json!({"type":"watch","feeds":["snapshot/1"]}),
    ] {
        let (task, socket) = dial(&s, &s.computer.key, PEER, stop.clone()).await;
        let mut socket = socket.unwrap();
        in_control(&s, &mut socket).await;
        send(&mut socket, message.clone()).await;
        assert!(
            next(&mut socket).await["closed"].is_string(),
            "taken: {message}"
        );
        task.await.unwrap();
    }
    assert!(app.ran().is_empty(), "ran: {:?}", app.ran());
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    opened(&s, &mut socket, 1).await;
    let mut pieces = Assembly::new(wire::ANSWER_BYTES);
    let answered = call(&mut socket, &mut pieces, 1, "get_settings", json!({})).await;
    assert_eq!(
        answered["value"],
        json!({"settings":"as read"}),
        "{answered}"
    );
    assert_eq!(app.ran(), ["get_settings"]);
}

/// ★ Deny by default at the shack: a command with no class, one given only at the shack, one that
/// runs on the computer, two that start a transmission (one on a connection with no Stop road, and
/// PTT's on half, which from afar is only ever a held press), a typed Winlink password and
/// arguments that are no object are each refused with the dispatcher's code, and none runs.
/// CONTROL: a classed read runs, and answers with its store's revision.
#[tokio::test]
async fn an_unclassed_or_refused_command_is_refused_with_its_code_and_never_runs() {
    let app = App::new();
    let s = running(&app);
    let (_stop, stop) = watch::channel(false);
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    opened(&s, &mut socket, 1).await;
    let mut pieces = Assembly::new(wire::ANSWER_BYTES);
    for (id, cmd, args, code) in [
        (1, "no_such_command", json!({}), "unclassed"),
        (2, "repair_clock", json!({}), "shackOnly"),
        (3, "check_for_update", json!({}), "runsHere"),
        (4, "call_cq", json!({}), "noStopRoad"),
        (5, "set_ptt", json!({"on":true}), "heldPress"),
        (
            6,
            "winlink_connect",
            json!({"password":"typed here"}),
            "typedPassword",
        ),
        (7, "add_radio", json!([1]), "badArguments"),
    ] {
        assert_eq!(
            call(&mut socket, &mut pieces, id, cmd, args).await,
            refused(id, code),
            "{cmd}"
        );
    }
    assert!(app.ran().is_empty(), "ran: {:?}", app.ran());
    let read = call(&mut socket, &mut pieces, 8, "get_settings", json!({})).await;
    assert_eq!(read["value"], json!({"settings":"as read"}), "{read}");
    assert_eq!(read["revision"].as_str().map(str::len), Some(64), "{read}");
    assert_eq!(app.ran(), ["get_settings"]);
}

/// ★ Only the computer in control runs a call: on an opened lane without the lease, a call is
/// refused `notController` and nothing runs. CONTROL: once it takes the lease, the same call runs.
#[tokio::test]
async fn a_call_runs_only_for_the_computer_in_control() {
    let app = App::new();
    let s = running(&app);
    let (_stop, stop) = watch::channel(false);
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    s.shared.authority.permit_station(&s.device, true).unwrap();
    assert_eq!(hello(&mut socket, VERSIONS).await["type"], "welcome");
    assert_eq!(
        open(&mut socket, "desktop", wire::APP, wire::contract(), 1).await,
        json!({"type":"opened"})
    );
    let mut pieces = Assembly::new(wire::ANSWER_BYTES);
    assert_eq!(
        call(&mut socket, &mut pieces, 1, "get_settings", json!({})).await,
        refused(1, "notController")
    );
    assert!(app.ran().is_empty(), "ran: {:?}", app.ran());
    let state = ask(&mut socket, json!({"type":"state","requestId":id()})).await;
    let acquired = ask(
        &mut socket,
        json!({"type":"acquire","requestId":id(),"stationBootId":state["value"]["stationBootId"]}),
    )
    .await;
    assert_eq!(acquired["value"]["phase"], "controlling", "{acquired}");
    let read = call(&mut socket, &mut pieces, 2, "get_settings", json!({})).await;
    assert_eq!(read["value"], json!({"settings":"as read"}), "{read}");
}

/// ★ A stale save refused on the wire (ruled 2026-10-05): a read answers with its store's
/// revision, a whole save names the revision of the copy it was made from, and a save made from a
/// copy the shack has changed since, or naming none, is refused `changedAtShack` and does not run.
/// CONTROL: a save from the current copy runs.
#[tokio::test]
async fn a_stale_whole_save_is_refused_over_the_link() {
    let app = App::new();
    let s = running(&app);
    let (_stop, stop) = watch::channel(false);
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    opened(&s, &mut socket, 1).await;
    let mut pieces = Assembly::new(wire::ANSWER_BYTES);
    let read = call(&mut socket, &mut pieces, 1, "get_settings", json!({})).await;
    let copy = read["revision"].as_str().unwrap().to_string();
    let save = |id: u64, base: Option<&str>| {
        let mut message =
            json!({"type":"call","id":id,"cmd":"set_settings","args":{"settings":{}}});
        if let Some(base) = base {
            message["base"] = json!(base);
        }
        message
    };
    send(&mut socket, save(2, Some(&copy))).await;
    let fresh = answer(&mut socket, &mut pieces, 2).await;
    assert!(
        fresh.get("value").is_some(),
        "made from the current copy: {fresh}"
    );
    // The shack's own screen saves a change.
    {
        let mut engine = s.shared.engine.lock().unwrap();
        let mut changed = engine.settings().clone();
        changed.mycall = "K9ABC".to_string();
        engine.apply_settings(changed);
    }
    send(&mut socket, save(3, Some(&copy))).await;
    assert_eq!(
        answer(&mut socket, &mut pieces, 3).await,
        refused(3, "changedAtShack")
    );
    send(&mut socket, save(4, None)).await;
    assert_eq!(
        answer(&mut socket, &mut pieces, 4).await,
        refused(4, "changedAtShack")
    );
    assert_eq!(app.ran(), ["get_settings", "set_settings"]);
}

/// ★ At most once, across a reconnect: a repeated id is answered with what became of it and does
/// not run again, on its own connection and on the next one from the same run of Nexus, which can
/// also ask after it; ids that come out of order (calls admitted together, a call sent in pieces)
/// each run once; and a new run of Nexus starts a journal of its own. CONTROL: a new id runs.
#[tokio::test]
async fn a_call_runs_at_most_once_across_a_reconnect() {
    let app = App::new();
    let s = running(&app);
    let (_stop, stop) = watch::channel(false);
    let added = |app: &App| app.ran().iter().filter(|cmd| *cmd == "add_radio").count();
    let mut pieces = Assembly::new(wire::ANSWER_BYTES);
    {
        let (task, socket) = dial(&s, &s.computer.key, PEER, stop.clone()).await;
        let mut socket = socket.unwrap();
        opened(&s, &mut socket, 1).await;
        let first = call(
            &mut socket,
            &mut pieces,
            7,
            "add_radio",
            json!({"name":"FT-991A"}),
        )
        .await;
        assert!(first.get("value").is_some(), "{first}");
        assert_eq!(
            call(
                &mut socket,
                &mut pieces,
                7,
                "add_radio",
                json!({"name":"FT-991A"})
            )
            .await,
            json!({"type":"result","id":7,"outcome":"done"})
        );
        assert_eq!(added(&app), 1);
        drop(socket);
        task.await.unwrap();
    }
    {
        let (task, socket) = dial(&s, &s.computer.key, PEER, stop.clone()).await;
        let mut socket = socket.unwrap();
        opened(&s, &mut socket, 1).await;
        send(&mut socket, json!({"type":"outcomes","ids":[7, 8]})).await;
        let asked = loop {
            match got(&mut socket, &mut pieces).await {
                Got::Text(value) if value["type"] == "outcomes" => break value,
                Got::Text(_) | Got::Feed(_) => {}
                other => panic!("no outcomes: {other:?}"),
            }
        };
        assert_eq!(
            asked,
            json!({"type":"outcomes","ids":[7, 8],"outcomes":[{"outcome":"done"},{"outcome":"unknown"}]})
        );
        assert_eq!(
            call(
                &mut socket,
                &mut pieces,
                7,
                "add_radio",
                json!({"name":"FT-991A"})
            )
            .await,
            json!({"type":"result","id":7,"outcome":"done"})
        );
        assert_eq!(added(&app), 1, "a repeat ran again");
        let fresh = call(
            &mut socket,
            &mut pieces,
            9,
            "add_radio",
            json!({"name":"IC-9700"}),
        )
        .await;
        assert!(fresh.get("value").is_some(), "{fresh}");
        let later = call(
            &mut socket,
            &mut pieces,
            12,
            "add_radio",
            json!({"name":"IC-705"}),
        )
        .await;
        let earlier = call(
            &mut socket,
            &mut pieces,
            11,
            "add_radio",
            json!({"name":"IC-905"}),
        )
        .await;
        assert!(later.get("value").is_some() && earlier.get("value").is_some());
        assert_eq!(
            call(
                &mut socket,
                &mut pieces,
                11,
                "add_radio",
                json!({"name":"IC-905"})
            )
            .await,
            json!({"type":"result","id":11,"outcome":"done"})
        );
        assert_eq!(added(&app), 4);
        drop(socket);
        task.await.unwrap();
    }
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    opened(&s, &mut socket, 2).await;
    let fresh = call(
        &mut socket,
        &mut pieces,
        7,
        "add_radio",
        json!({"name":"FTDX10"}),
    )
    .await;
    assert!(
        fresh.get("value").is_some(),
        "a new run's id 7 is its own: {fresh}"
    );
    assert_eq!(added(&app), 5);
}

/// ★ Stop is behind nothing: with every call slot held by slow calls, the operations authority
/// held so that as many calls again wait to be admitted, the next ordinary call refused `busy` on
/// the reading task, and the Engine held, a Stop and the off half of PTT are admitted, run and
/// answered at once, while the slow calls still run; and a Stop runs for a computer that holds no
/// lease. CONTROL: once let go, the slow calls are answered, and the calls that waited are
/// admitted and refused `busy` by the dispatcher, the slots being full.
#[tokio::test]
async fn a_stop_is_admitted_and_run_at_once_whatever_else_is_waiting() {
    let app = App::holding();
    let _let_go = LetGo(app.clone());
    let s = running(&app);
    let (_stop, stop) = watch::channel(false);
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop.clone()).await;
    let mut socket = socket.unwrap();
    opened(&s, &mut socket, 1).await;
    // A second connection of the computer's, which never takes the lease.
    let (_other_task, other) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut other = other.unwrap();
    assert_eq!(hello(&mut other, VERSIONS).await["type"], "welcome");
    assert_eq!(
        open(&mut other, "desktop", wire::APP, wire::contract(), 2).await,
        json!({"type":"opened"})
    );
    assert!(SLOW
        .iter()
        .all(|cmd| class_of(cmd) == Some(Class::RunsAtShack)));
    assert_eq!(
        IN_FLIGHT,
        SLOW.len() * PER_COMMAND,
        "premise: the slow calls fill every slot"
    );
    for id in 1..=IN_FLIGHT as u64 {
        send_call(&mut socket, id, SLOW[(id % 2) as usize], json!({})).await;
    }
    let mut pieces = Assembly::new(wire::ANSWER_BYTES);
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.ran().len() < IN_FLIGHT {
        assert!(Instant::now() < deadline, "only ran {:?}", app.ran());
        if let Got::Text(early) =
            got_within(&mut socket, &mut pieces, Duration::from_millis(20)).await
        {
            panic!("a slow call was answered before it was let go: {early}");
        }
    }
    // The authority held, as a slow request holds it: every ordinary call now waits to be admitted.
    let authority = s.shared.authority.clone();
    let held = authority.held();
    // And the Engine held, as the radio loop holds it across a slow CAT exchange.
    let engine = s.shared.engine.clone();
    let (holding, engine_held) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        let _engine = engine.lock().unwrap();
        holding.send(()).unwrap();
        let _ = released.recv();
    });
    engine_held.recv().unwrap();
    let waiting = (PENDING - IN_FLIGHT) as u64;
    for id in 0..waiting {
        send_call(&mut socket, 100 + id, "get_settings", json!({})).await;
    }
    assert_eq!(
        call(
            &mut socket,
            &mut pieces,
            200,
            "add_radio",
            json!({"name":"one more"})
        )
        .await,
        refused(200, "busy")
    );
    let started = Instant::now();
    let stopped = call(&mut socket, &mut pieces, 201, "halt_tx", json!({})).await;
    assert!(stopped.get("value").is_some(), "{stopped}");
    let unkeyed = call(
        &mut socket,
        &mut pieces,
        202,
        "set_ptt",
        json!({"on":false}),
    )
    .await;
    assert!(unkeyed.get("value").is_some(), "{unkeyed}");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    let ran = app.ran();
    assert_eq!(
        ran[IN_FLIGHT..],
        ["halt_tx", "set_ptt"],
        "only the Stops ran past the full slots: {ran:?}"
    );
    let mut other_pieces = Assembly::new(wire::ANSWER_BYTES);
    let stopped = call(&mut other, &mut other_pieces, 1, "halt_tx", json!({})).await;
    assert!(
        stopped.get("value").is_some(),
        "no lease, still a Stop: {stopped}"
    );
    // Let everything go, the authority first: the waiting calls find every slot still full.
    drop(held);
    for (id, answered) in answers(&mut socket, &mut pieces, 100..100 + waiting).await {
        assert_eq!(answered, refused(id, "busy"));
    }
    release.send(()).unwrap();
    holder.join().unwrap();
    app.release();
    for (_, answered) in answers(&mut socket, &mut pieces, 1..=IN_FLIGHT as u64).await {
        assert_eq!(answered["value"], json!("answered"), "{answered}");
    }
}

/// ★ A flood of Stops on the link: however many come, each Stop command has one blocking
/// task at work, never one per Stop, and every Stop is answered. The first Stop runs at once; a
/// different Stop runs at once beside it, never behind it; Stops of the same command that land
/// while it runs are run once more after it, on the same task, so none is lost. CONTROL: a Stop
/// sent once the burst is done runs again.
#[tokio::test]
async fn a_burst_of_stops_starts_one_task_per_stop_command_and_none_is_lost() {
    const BURST: u64 = 64;
    let gate = Arc::new((Mutex::new(true), Condvar::new()));
    let ran: Arc<Mutex<Vec<String>>> = Default::default();
    let (at_work, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let runner: Runner = {
        let (gate, ran, at_work, most) = (gate.clone(), ran.clone(), at_work.clone(), most.clone());
        Arc::new(move |cmd: String, _args: Value, answer: Answer| {
            if cmd == "halt_tx" || cmd == "set_ptt" {
                most.fetch_max(at_work.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                ran.lock().unwrap().push(cmd.clone());
                let (held, let_go) = &*gate;
                let mut held = held.lock().unwrap();
                while *held {
                    held = let_go.wait(held).unwrap();
                }
                at_work.fetch_sub(1, Ordering::SeqCst);
            }
            answer(Ok(json!({ "ran": cmd })))
        })
    };
    struct Open(Arc<(Mutex<bool>, Condvar)>);
    impl Drop for Open {
        fn drop(&mut self) {
            *self.0 .0.lock().unwrap() = false;
            self.0 .1.notify_all();
        }
    }
    let open = Open(gate.clone());
    let mut s = shack(home());
    let voided = s.shared.engine.lock().unwrap().remote_starts_voided();
    s.shared.commands = Commands::new(runner, voided);
    let (_stop, stop) = watch::channel(false);
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    opened(&s, &mut socket, 1).await;
    let mut pieces = Assembly::new(wire::ANSWER_BYTES);
    let ran_within = |count: usize| {
        let ran = ran.clone();
        async move {
            let deadline = Instant::now() + Duration::from_secs(2);
            while ran.lock().unwrap().len() < count && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            ran.lock().unwrap().clone()
        }
    };
    let started = STARTED.with(|started| started.get());
    // The first Stop halts at once.
    send_call(&mut socket, 1, "halt_tx", json!({})).await;
    assert_eq!(ran_within(1).await, ["halt_tx"]);
    // The burst, while it runs, and a different Stop among it.
    for id in 2..=BURST {
        send_call(&mut socket, id, "halt_tx", json!({})).await;
    }
    send_call(&mut socket, BURST + 1, "set_ptt", json!({"on": false})).await;
    // Read as far as here: a call after them is answered only once the reading task has read them.
    let read = call(
        &mut socket,
        &mut pieces,
        BURST + 2,
        "get_settings",
        json!({}),
    )
    .await;
    assert!(read.get("value").is_some(), "{read}");
    assert_eq!(
        STARTED.with(|now| now.get()) - started,
        2,
        "blocking tasks started for {BURST} halt_tx and one set_ptt"
    );
    // The different Stop did not wait behind the first.
    assert_eq!(ran_within(2).await, ["halt_tx", "set_ptt"]);
    drop(open);
    let answered = answers(&mut socket, &mut pieces, 1..=BURST + 1).await;
    for (id, answer) in &answered {
        assert!(answer.get("value").is_some(), "Stop {id}: {answer}");
    }
    let mut ran_all = ran.lock().unwrap().clone();
    ran_all.sort();
    assert_eq!(
        ran_all,
        ["halt_tx", "halt_tx", "set_ptt"],
        "the first, and once more for the Stops that landed while it ran"
    );
    assert_eq!(most.load(Ordering::SeqCst), 2, "at work at once");
    let again = call(&mut socket, &mut pieces, BURST + 3, "halt_tx", json!({})).await;
    assert!(again.get("value").is_some(), "{again}");
    assert_eq!(ran.lock().unwrap().len(), 4);
}

/// ⛔ A flood of Stops of one command, each with arguments of its own: the radio is halted by the
/// first, and what waits behind it is at most [`STOP_RUNS`] runs, never one per Stop, so neither
/// what the shack holds nor what it runs grows with the flood. A Stop past them is answered
/// `busy`, so no Stop is left unanswered. CONTROL: a Stop once the flood is done runs again.
#[tokio::test]
async fn stops_of_one_command_with_arguments_of_their_own_wait_as_a_few_runs() {
    const FLOOD: u64 = 64;
    let gate = Arc::new((Mutex::new(true), Condvar::new()));
    let ran: Arc<Mutex<Vec<Value>>> = Default::default();
    let runner: Runner = {
        let (gate, ran) = (gate.clone(), ran.clone());
        Arc::new(move |cmd: String, args: Value, answer: Answer| {
            if cmd == "halt_tx" {
                ran.lock().unwrap().push(args);
                let (held, let_go) = &*gate;
                let mut held = held.lock().unwrap();
                while *held {
                    held = let_go.wait(held).unwrap();
                }
            }
            answer(Ok(json!({ "ran": cmd })))
        })
    };
    struct Open(Arc<(Mutex<bool>, Condvar)>);
    impl Drop for Open {
        fn drop(&mut self) {
            *self.0 .0.lock().unwrap() = false;
            self.0 .1.notify_all();
        }
    }
    let open = Open(gate.clone());
    let mut s = shack(home());
    let voided = s.shared.engine.lock().unwrap().remote_starts_voided();
    s.shared.commands = Commands::new(runner, voided);
    let (_stop, stop) = watch::channel(false);
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    opened(&s, &mut socket, 1).await;
    let mut pieces = Assembly::new(wire::ANSWER_BYTES);
    // The first Stop halts at once.
    send_call(&mut socket, 1, "halt_tx", json!({})).await;
    let deadline = Instant::now() + Duration::from_secs(2);
    while ran.lock().unwrap().is_empty() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(*ran.lock().unwrap(), [json!({})], "halted by the first");
    for id in 2..=FLOOD + 1 {
        send_call(&mut socket, id, "halt_tx", json!({ "junk": id })).await;
    }
    send_call(&mut socket, FLOOD + 2, "get_settings", json!({})).await;
    // The Stops past the runs waiting are answered while the first still runs, never left waiting.
    let past = (2 + STOP_RUNS as u64)..=FLOOD + 1;
    let early = answers(&mut socket, &mut pieces, past.clone().chain([FLOOD + 2])).await;
    for id in past {
        assert_eq!(early[&id], refused(id, "busy"));
    }
    assert!(early[&(FLOOD + 2)].get("value").is_some(), "{early:?}");
    drop(open);
    let answered = answers(&mut socket, &mut pieces, 1..2 + STOP_RUNS as u64).await;
    for (id, answer) in &answered {
        assert!(answer.get("value").is_some(), "Stop {id}: {answer}");
    }
    assert_eq!(
        ran.lock().unwrap().len(),
        1 + STOP_RUNS,
        "the first, then the runs that waited behind it"
    );
    let again = call(
        &mut socket,
        &mut pieces,
        FLOOD + 3,
        "halt_tx",
        json!({"junk": 0}),
    )
    .await;
    assert!(again.get("value").is_some(), "{again}");
    assert_eq!(ran.lock().unwrap().len(), 2 + STOP_RUNS);
}

/// ⛔ A task at work that panics leaves no work behind it unstarted: the next work, a Stop on the
/// link, starts a task of its own. CONTROL: work after work that did not panic runs as well.
#[tokio::test]
async fn work_after_a_task_that_panicked_still_runs() {
    let turns: Arc<InTurn<u64>> = Default::default();
    let (done, ran) = std::sync::mpsc::channel();
    for n in 1..=3u64 {
        let done = done.clone();
        turns.start(
            |waiting| waiting.push(n),
            move |waiting| {
                for n in waiting {
                    assert_ne!(n, 2, "the work's own panic");
                    done.send(n).unwrap();
                }
            },
        );
        if n == 2 {
            // Its task has ended by panicking before the next is added.
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
    let got: Vec<u64> = (0..2)
        .map(|_| {
            ran.recv_timeout(Duration::from_secs(2))
                .expect("work after it ran")
        })
        .collect();
    assert_eq!(got, [1, 3]);
}

/// ★ A flood of `outcomes` questions: one blocking task answers them in turn, never one
/// per question, and every one is answered. The journals are held meanwhile, as a burst of calls
/// being admitted holds them. CONTROL: a question once they are let go is answered too.
#[tokio::test]
async fn a_burst_of_outcomes_questions_starts_one_task_and_each_is_answered() {
    const BURST: usize = 32;
    let app = App::new();
    let s = running(&app);
    let (_stop, stop) = watch::channel(false);
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    opened(&s, &mut socket, 1).await;
    let mut pieces = Assembly::new(wire::ANSWER_BYTES);
    let held = s.shared.dispatcher.hold_journals();
    let started = STARTED.with(|started| started.get());
    for n in 0..BURST as u64 {
        send(&mut socket, json!({"type":"outcomes","ids":[1000 + n]})).await;
    }
    // Answered on the reading task: once it is, every question before it has been read.
    send(&mut socket, json!({"type":"watch","feeds":[]})).await;
    loop {
        match got(&mut socket, &mut pieces).await {
            Got::Text(value) if value["type"] == "watching" => break,
            Got::Text(value) if value["type"] == "outcomes" => {
                panic!("answered while held: {value}")
            }
            Got::Text(_) | Got::Feed(_) => {}
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(
        STARTED.with(|now| now.get()) - started,
        1,
        "blocking tasks started for {BURST} questions"
    );
    drop(held);
    let mut asked: std::collections::BTreeSet<u64> = (0..BURST as u64).map(|n| 1000 + n).collect();
    while !asked.is_empty() {
        match got(&mut socket, &mut pieces).await {
            Got::Text(value) if value["type"] == "outcomes" => {
                assert_eq!(value["outcomes"], json!([{"outcome":"unknown"}]), "{value}");
                assert!(asked.remove(&value["ids"][0].as_u64().unwrap()), "{value}");
            }
            Got::Text(_) | Got::Feed(_) => {}
            other => panic!("unanswered {asked:?}: {other:?}"),
        }
    }
    send(&mut socket, json!({"type":"outcomes","ids":[7]})).await;
    loop {
        match got(&mut socket, &mut pieces).await {
            Got::Text(value) if value["type"] == "outcomes" => {
                assert_eq!(value["ids"], json!([7]));
                break;
            }
            Got::Text(_) | Got::Feed(_) => {}
            other => panic!("{other:?}"),
        }
    }
}

/// ★ Pieces: a call bigger than a frame crosses in pieces and runs with exactly its arguments, and
/// its answer, bigger than a frame, comes back whole; a Stop sent between a big call's pieces is
/// answered before the rest of them is sent; and a 200 KB answer comes back whole. CONTROL: a binary frame that is
/// no piece ends the connection.
#[tokio::test]
async fn big_calls_and_answers_cross_in_pieces_and_a_stop_goes_between_them() {
    let app = App::new();
    let s = running(&app);
    let (_stop, stop) = watch::channel(false);
    let (task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    opened(&s, &mut socket, 1).await;
    let big = "x".repeat(100_000);
    let text = json!({"type":"call","id":1,"cmd":"add_radio","args":{"name":big}}).to_string();
    let frames = wire::split(Whole::Text(text), 1);
    assert!(frames.len() > 10, "premise: many pieces");
    let (first, rest) = frames.split_at(frames.len() / 2);
    for frame in first {
        socket
            .send(Message::Binary(frame.clone().into()))
            .await
            .unwrap();
    }
    // Answered while the big call is half sent: the Stop waited for nothing.
    let mut pieces = Assembly::new(wire::ANSWER_BYTES);
    let stopped = call(&mut socket, &mut pieces, 2, "halt_tx", json!({})).await;
    assert!(stopped.get("value").is_some(), "{stopped}");
    assert_eq!(app.ran(), ["halt_tx"]);
    for frame in rest {
        socket
            .send(Message::Binary(frame.clone().into()))
            .await
            .unwrap();
    }
    let echoed = answer(&mut socket, &mut pieces, 1).await;
    assert_eq!(
        echoed["value"],
        json!({"ran":"add_radio","args":{"name":big}})
    );
    assert_eq!(app.ran(), ["halt_tx", "add_radio"]);
    let long = call(&mut socket, &mut pieces, 3, "dxcc_entity_names", json!({})).await;
    assert_eq!(long["value"].as_str().map(str::len), Some(200_000));
    socket
        .send(Message::Binary(vec![9, 0, 0, 0, 0, 1, 1].into()))
        .await
        .unwrap();
    assert!(matches!(got(&mut socket, &mut pieces).await, Got::Closed));
    task.await.unwrap();
}

/// ★ Calls left half-sent: a computer that starts call after call and never finishes
/// one is cut off once what the shack holds of them passes two calls of the largest size, where
/// before each of its 16 streams could hold 16 MiB. CONTROL: two calls of the largest size, both
/// half-sent at once, are held, and a Stop between them is still answered.
#[tokio::test]
async fn calls_left_half_sent_are_cut_off_at_the_connections_budget() {
    let app = App::new();
    let s = running(&app);
    let (_stop, stop) = watch::channel(false);
    let (task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    opened(&s, &mut socket, 1).await;
    let size = wire::FRAME - 6;
    let piece = |stream: u32, bytes: usize| {
        let mut frame = vec![1];
        frame.extend_from_slice(&stream.to_le_bytes());
        frame.push(0);
        frame.resize(6 + bytes, b'x');
        Message::Binary(frame.into())
    };
    for stream in [1, 2] {
        let mut left = wire::CALL_BYTES;
        while left > 0 {
            let bytes = left.min(size);
            socket.send(piece(stream, bytes)).await.unwrap();
            left -= bytes;
        }
    }
    let mut pieces = Assembly::new(wire::ANSWER_BYTES);
    let stopped = call(&mut socket, &mut pieces, 1, "halt_tx", json!({})).await;
    assert!(
        stopped.get("value").is_some(),
        "held two, still open: {stopped}"
    );
    socket.send(piece(3, 1)).await.unwrap();
    assert!(matches!(got(&mut socket, &mut pieces).await, Got::Closed));
    task.await.unwrap();
}

/// ★ The feeds' state numbers: a watched feed is pushed numbered 1, 2, 3 with no gap and in order,
/// the snapshot at least every 300 ms whether or not anything changed, each value as the app
/// answered it, deflated against the values before it; and a name that is no feed is told, and
/// the session goes on. CONTROL: while the computer holds no lease, nothing is pushed.
#[tokio::test]
async fn a_watched_feed_is_pushed_numbered_without_a_gap() {
    let app = App::new();
    let s = running(&app);
    let (_stop, stop) = watch::channel(false);
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    s.shared.authority.permit_station(&s.device, true).unwrap();
    assert_eq!(hello(&mut socket, VERSIONS).await["type"], "welcome");
    assert_eq!(
        open(&mut socket, "desktop", wire::APP, wire::contract(), 1).await,
        json!({"type":"opened"})
    );
    send(
        &mut socket,
        json!({"type":"watch","feeds":["snapshot/1","noSuchFeed/1"]}),
    )
    .await;
    let mut pieces = Assembly::new(wire::ANSWER_BYTES);
    match got(&mut socket, &mut pieces).await {
        Got::Text(value) => assert_eq!(
            value,
            json!({"type":"watching","feeds":["snapshot/1"],"unknown":["noSuchFeed/1"]})
        ),
        other => panic!("{other:?}"),
    }
    assert!(
        matches!(
            got_within(&mut socket, &mut pieces, Duration::from_millis(900)).await,
            Got::Silent
        ),
        "pushed to a computer without the lease"
    );
    let state = ask(&mut socket, json!({"type":"state","requestId":id()})).await;
    let acquired = ask(
        &mut socket,
        json!({"type":"acquire","requestId":id(),"stationBootId":state["value"]["stationBootId"]}),
    )
    .await;
    assert_eq!(acquired["value"]["phase"], "controlling", "{acquired}");
    let mut inflate = Inflate::default();
    let (mut numbers, mut values, mut arrivals) = (Vec::new(), Vec::new(), Vec::new());
    while numbers.len() < 8 {
        match got(&mut socket, &mut pieces).await {
            Got::Feed(frame) => {
                let (name, number, deflated) = wire::read_feed(&frame).expect("a feed frame");
                assert_eq!(name, "snapshot/1");
                let value: Value =
                    serde_json::from_slice(&inflate.value(deflated).expect("inflates")).unwrap();
                numbers.push(number);
                values.push(value["n"].as_u64().unwrap());
                arrivals.push(Instant::now());
            }
            Got::Text(_) => {}
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(numbers, (1..=8).collect::<Vec<u64>>(), "a gap or a reorder");
    assert!(values.windows(2).all(|w| w[0] < w[1]), "{values:?}");
    let longest = arrivals.windows(2).map(|w| w[1] - w[0]).max().unwrap();
    assert!(longest < Duration::from_millis(600), "{longest:?}");
    let read = call(&mut socket, &mut pieces, 1, "get_settings", json!({})).await;
    assert_eq!(
        read["value"],
        json!({"settings":"as read"}),
        "the session went on"
    );
}

/// ★ Events: an event the dispatcher forwards, raised at the shack, reaches the computer in control
/// numbered and timed, with its payload as raised; one it does not forward never crosses, and a
/// computer without the lease is sent none. CONTROL: the same pounce, raised again once the
/// computer holds the lease, arrives.
#[tokio::test]
async fn only_a_forwarded_event_reaches_only_the_computer_in_control() {
    let app = App::new();
    let s = running(&app);
    let (_stop, stop) = watch::channel(false);
    let (_task, socket) = dial(&s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    s.shared.authority.permit_station(&s.device, true).unwrap();
    assert_eq!(hello(&mut socket, VERSIONS).await["type"], "welcome");
    assert_eq!(
        open(&mut socket, "desktop", wire::APP, wire::contract(), 1).await,
        json!({"type":"opened"})
    );
    let mut pieces = Assembly::new(wire::ANSWER_BYTES);
    tokio::time::sleep(Duration::from_millis(100)).await;
    s.shared
        .commands
        .raised("alert", json!({"call":"VP8PJ"}).to_string());
    assert!(
        matches!(
            got_within(&mut socket, &mut pieces, Duration::from_millis(700)).await,
            Got::Silent
        ),
        "an event reached a computer without the lease"
    );
    let state = ask(&mut socket, json!({"type":"state","requestId":id()})).await;
    ask(
        &mut socket,
        json!({"type":"acquire","requestId":id(),"stationBootId":state["value"]["stationBootId"]}),
    )
    .await;
    s.shared
        .commands
        .raised("update-progress", json!({"done":1}).to_string());
    // An alert as the shack makes it: its id goes to the computer unchanged, so both screens name
    // the same alert.
    let raised = crate::alert::raised("pounce", json!({"call":"VP8PJ"}));
    s.shared
        .commands
        .raised("alert", serde_json::to_string(&raised).unwrap());
    let event = loop {
        match got(&mut socket, &mut pieces).await {
            Got::Text(value) if value["type"] == "event" => break value,
            Got::Text(_) => {}
            other => panic!("no event: {other:?}"),
        }
    };
    assert_eq!(event["name"], "alert", "{event}");
    assert_eq!(
        event["data"],
        serde_json::to_value(&raised).unwrap(),
        "{event}"
    );
    assert_eq!(event["data"]["id"], raised.id, "the same id: {event}");
    assert_eq!(event["data"]["alert"], json!({"call":"VP8PJ"}), "{event}");
    assert!(
        event["id"].as_u64().is_some() && event["at"].as_u64().is_some(),
        "{event}"
    );
    assert!(
        matches!(
            got_within(&mut socket, &mut pieces, Duration::from_millis(300)).await,
            Got::Silent
        ),
        "something else crossed"
    );
}
