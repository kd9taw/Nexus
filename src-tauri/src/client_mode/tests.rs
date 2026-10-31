//! Client mode's rules, each shown doing what it does and refusing what it refuses: where every
//! command goes, the caps, the feeds, the ui-state split, this computer's own settings, the
//! revisions a save goes with, and the launch that opens no station here. The calls go through
//! Tauri's own IPC, on its mock runtime, as a window's do.

use super::*;
use crate::command_classes::registered;
use crate::command_dispatch::off_args;
use std::path::Path;
use tauri::test::MockRuntime;

/// Phone's Lock: the held press, bounded by its cap.
mod lock;
/// The held press against presence as heard here.
mod reassert;
/// A cold launch connects again to the station this computer last drove.
mod reconnect;
/// The recorder and the held press.
mod record;
/// What each call and each hold says of the Stops pressed before it.
mod stop_count;
/// Stop wins over a start that was in flight, at this computer.
mod stop_first;

// --- Scaffolding ---------------------------------------------------------------------------------

const STATION: &str = "0123abcd-0123-4567-89ab-0123456789ab";

/// A shack that records every call it is sent, can hold them until a test lets them go, and
/// answers each as the test set for its command (`{"ran": <command>}` otherwise).
struct FakeShack {
    sent: Mutex<Vec<(String, Value, Option<String>)>>,
    /// Each call's command and the count of Stops it carried, in the order sent.
    counted: Mutex<Vec<(String, u64)>>,
    answers: Mutex<HashMap<String, Answer>>,
    feeds: Mutex<HashMap<String, (u64, Value)>>,
    open: tokio::sync::watch::Sender<bool>,
    running: Mutex<HashMap<String, usize>>,
    /// The most calls running at once, in all and of one command, Stops left out.
    most: Mutex<(usize, usize)>,
    /// Its Stop road, when the test gives it one.
    road: Mutex<Option<Arc<StopRoad>>>,
}

impl FakeShack {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            sent: Mutex::new(Vec::new()),
            counted: Mutex::new(Vec::new()),
            answers: Mutex::new(HashMap::new()),
            feeds: Mutex::new(HashMap::new()),
            open: tokio::sync::watch::channel(true).0,
            running: Mutex::new(HashMap::new()),
            most: Mutex::new((0, 0)),
            road: Mutex::new(None),
        })
    }

    /// Holds every call until [`FakeShack::let_go`].
    fn hold(&self) {
        self.open.send_replace(false);
    }

    fn let_go(&self) {
        self.open.send_replace(true);
    }

    /// The commands sent, in order.
    fn sent(&self) -> Vec<String> {
        lock(&self.sent)
            .iter()
            .map(|(cmd, ..)| cmd.clone())
            .collect()
    }

    fn running(&self) -> usize {
        lock(&self.running).values().sum()
    }

    fn answers(&self, cmd: &str, answer: Answer) {
        lock(&self.answers).insert(cmd.to_string(), answer);
    }

    fn feeds(&self, feed: &str, number: u64, value: Value) {
        lock(&self.feeds).insert(feed.to_string(), (number, value));
    }
}

impl Shack for FakeShack {
    fn call(
        self: Arc<Self>,
        cmd: String,
        args: Value,
        base: Option<String>,
        stops: u64,
    ) -> Answering {
        Box::pin(async move {
            let stop = is_stop(&cmd, &args);
            lock(&self.counted).push((cmd.clone(), stops));
            lock(&self.sent).push((cmd.clone(), args, base));
            if !stop {
                let mut running = lock(&self.running);
                *running.entry(cmd.clone()).or_default() += 1;
                let total: usize = running.values().sum();
                let of_command = running[&cmd];
                let mut most = lock(&self.most);
                *most = (most.0.max(total), most.1.max(of_command));
            }
            let mut open = self.open.subscribe();
            let _ = open.wait_for(|open| *open).await;
            if !stop {
                if let Some(count) = lock(&self.running).get_mut(&cmd) {
                    *count -= 1;
                }
            }
            let set = lock(&self.answers).get(&cmd).cloned();
            let answer = set.unwrap_or_else(|| Answer::Value {
                value: json!({ "ran": cmd }),
                revision: None,
            });
            (0, answer)
        })
    }

    fn latest(&self, feed: &str) -> Option<(u64, Value)> {
        lock(&self.feeds).get(feed).cloned()
    }

    fn stop_road(&self) -> Option<Arc<StopRoad>> {
        lock(&self.road).clone()
    }
}

pub(crate) fn scratch(label: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "nexus-client-{label}-{nanos}-{}",
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A client whose own files are in `dir`, on `shack`.
fn client_in(dir: &Path, shack: &Arc<FakeShack>) -> Arc<Client> {
    let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
    let client = Arc::new(Client::new(STATION.to_string(), this));
    client.install(Some(Arc::clone(shack) as Arc<dyn Shack>));
    client
}

/// The screens: a mock app whose handler is `client`'s around a local handler that records what
/// it ran here, with a main window and a pop-out.
pub(crate) struct Screens {
    _app: tauri::App<MockRuntime>,
    pub(crate) main: tauri::WebviewWindow<MockRuntime>,
    pub(crate) popout: tauri::WebviewWindow<MockRuntime>,
    here: Arc<Mutex<Vec<String>>>,
}

impl Screens {
    pub(crate) fn ran_here(&self) -> Vec<String> {
        lock(&self.here).clone()
    }
}

pub(crate) fn screens(client: &Arc<Client>) -> Screens {
    let here = Arc::new(Mutex::new(Vec::new()));
    let (routed, ran) = (Arc::clone(client), Arc::clone(&here));
    let local = move |invoke: Invoke<MockRuntime>| {
        lock(&ran).push(invoke.message.command().to_string());
        invoke.resolver.resolve("ran here");
        true
    };
    let app = tauri::test::mock_builder()
        .invoke_handler(move |invoke| routed.handle(invoke, &local))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("a mock app");
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("the main window");
    let popout = tauri::WebviewWindowBuilder::new(&app, "panel-ft8", Default::default())
        .build()
        .expect("a pop-out");
    Screens {
        _app: app,
        main,
        popout,
        here,
    }
}

/// One IPC call from `window`, as its screen makes it: the answer comes on the receiver.
pub(crate) fn send(
    window: &tauri::WebviewWindow<MockRuntime>,
    cmd: &str,
    args: Value,
) -> std::sync::mpsc::Receiver<Result<Value, Value>> {
    use tauri::ipc::{CallbackFn, InvokeResponse, InvokeResponseBody};
    let (answer, answered) = std::sync::mpsc::channel();
    let request = tauri::webview::InvokeRequest {
        cmd: cmd.to_string(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: window.url().expect("the window's address"),
        body: InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: window.app_handle().invoke_key().to_string(),
    };
    window.as_ref().clone().on_message(
        request,
        Box::new(move |_, _, response, _, _| {
            let _ = answer.send(match response {
                InvokeResponse::Ok(InvokeResponseBody::Json(json)) => {
                    Ok(serde_json::from_str(&json).unwrap_or(Value::Null))
                }
                InvokeResponse::Ok(InvokeResponseBody::Raw(_)) => Ok(Value::Null),
                InvokeResponse::Err(error) => Err(error.0),
            });
        }),
    );
    answered
}

/// [`send`], and its answer.
pub(crate) fn ask(
    window: &tauri::WebviewWindow<MockRuntime>,
    cmd: &str,
    args: Value,
) -> Result<Value, Value> {
    send(window, cmd, args)
        .recv_timeout(Duration::from_secs(5))
        .expect("an answer")
}

fn refused(code: &str) -> Result<Value, Value> {
    Err(json!(word(code)))
}

/// Waits up to five seconds for `done`.
pub(crate) fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "never: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The way a command of each class takes, for the table test: what it should be.
fn expected(cmd: &str, args: &Value) -> Way {
    let class = class_of(cmd).expect("every registered command is classed");
    let stop = is_stop(cmd, args);
    match class {
        Class::RefusedHere => Way::Refused(Refusal::ShackOnly),
        Class::RunsHere => match cmd {
            "set_beta_updates" => Way::Local(Local::BetaUpdates),
            "set_launch_at_login" => Way::Local(Local::LaunchAtLogin),
            "prepare_update_install" => Way::Local(Local::BeforeInstall),
            "restart_app" => Way::Local(Local::Restart),
            "start_voice_recording" => Way::Local(Local::RecordStart),
            "stop_voice_recording" => Way::Local(Local::RecordStop),
            "cancel_voice_recording" => Way::Local(Local::RecordCancel),
            _ => Way::Here,
        },
        Class::StopsTx => Way::Shack { stop: true },
        Class::StartsTx if stop => Way::Shack { stop: true },
        Class::StartsTx if tempo_client::dispatch::argument_refusal(cmd, args).is_some() => {
            Way::Refused(Refusal::TransmitNotYet)
        }
        Class::StartsTx => match transmit_from_afar(cmd) {
            Err(refusal) => Way::Refused(refusal),
            Ok(Wired::HeldPress) => Way::Press,
            Ok(Wired::LockedPress) => Way::Lock,
            Ok(
                Wired::Ft
                | Wired::VoiceKeyer
                | Wired::Js8
                | Wired::Cw
                | Wired::Tune
                | Wired::Rtty
                | Wired::Psk
                | Wired::Jtty
                | Wired::Aprs
                | Wired::Sstv
                | Wired::Chat,
            ) => Way::Start,
        },
        Class::RunsAtShack => match cmd {
            "ui_state_load" | "ui_state_save" => Way::Split,
            "get_snapshot" => Way::Local(Local::Snapshot),
            "get_spectrum_row" => Way::Local(Local::SpectrumRow),
            "get_meters" => Way::Local(Local::Meters),
            "get_rtty_scope" => Way::Local(Local::RttyScope),
            "radio_launch_info" => Way::Local(Local::RadioLaunchInfo),
            _ => Way::Shack { stop },
        },
    }
}

// --- Where each command goes ---------------------------------------------------------------------

/// ★ Every registered command, on and off, goes where its class says: "runs here" here (but the
/// four this computer answers itself), "refused here" refused, a station command to the shack, a
/// start wired for this road to the shack as a start, the PTT as a held press, every other start
/// refused here, and a Stop or the off half of a switch to the shack under Stop's rules.
/// Polls of the three feeds are answered here, and the ui-state pair splits. CONTROL: a command
/// with no class, and a password typed here, go nowhere.
#[test]
fn every_command_goes_where_its_class_says() {
    let mut cases = 0;
    for cmd in registered() {
        let mut argument_sets = vec![json!({})];
        argument_sets.extend(off_args(cmd));
        if cmd == "winlink_connect" {
            argument_sets = vec![json!({ "password": "" })];
        }
        for args in argument_sets {
            assert_eq!(way(cmd, &args), expected(cmd, &args), "{cmd} {args}");
            cases += 1;
        }
    }
    assert!(cases > registered().len(), "the off halves were tried too");
    assert_eq!(
        way("no_such_command", &json!({})),
        Way::Refused(Refusal::Unclassed)
    );
    assert_eq!(
        way("winlink_connect", &json!({ "password": "typed here" })),
        Way::Refused(Refusal::TypedPassword)
    );
    // Spot checks, so the table above cannot pass by mirroring a broken rule.
    assert_eq!(way("call_cq", &json!({})), Way::Start);
    assert_eq!(way("play_voice_message", &json!({ "slot": 1 })), Way::Start);
    assert_eq!(way("set_ptt", &json!({ "on": true })), Way::Press);
    assert_eq!(way("send_cw", &json!({ "text": "CQ" })), Way::Start);
    assert_eq!(way("start_running", &json!({})), Way::Start);
    assert_eq!(way("js8_send", &json!({ "text": "HI" })), Way::Start);
    assert_eq!(
        way("js8_arm", &json!({ "which": "autoreply", "on": true })),
        Way::Start
    );
    assert_eq!(
        way("js8_typeahead", &json!({ "sentSeen": 0, "unsent": "MORE" })),
        Way::Start
    );
    for which in ["hb", "relay", "hback", "hbnet"] {
        assert_eq!(
            way("js8_arm", &json!({ "which": which, "on": true })),
            Way::Start,
            "{which}"
        );
    }
    assert_eq!(
        way("js8_arm", &json!({ "which": "hb", "on": false })),
        Way::Shack { stop: true }
    );
    assert_eq!(way("set_tune", &json!({ "on": true })), Way::Start);
    assert_eq!(
        way("set_tune", &json!({ "on": false })),
        Way::Shack { stop: true }
    );
    assert_eq!(way("atu_tune", &json!({})), Way::Start);
    assert_eq!(way("rtty_set_latched", &json!({ "on": true })), Way::Start);
    assert_eq!(
        way("rtty_set_latched", &json!({ "on": false })),
        Way::Shack { stop: true }
    );
    assert_eq!(way("rtty_auto_cq", &json!({})), Way::Start);
    assert_eq!(way("psk_type", &json!({ "text": "HI" })), Way::Start);
    for (cmd, args) in [
        ("aprs_arm", json!({ "on": true })),
        ("aprs_send_beacon", json!({ "comment": "HI" })),
        (
            "aprs_send_message",
            json!({ "addressee": "W1AW", "text": "HI" }),
        ),
        ("sstv_send", json!({ "mode": "robot36" })),
        ("broadcast", json!({ "text": "HI" })),
        ("send_message", json!({ "peer": "W1AW", "text": "HI" })),
        ("resend_chat", json!({ "peer": "W1AW" })),
        ("set_chat_cq", json!({ "on": true })),
        ("resume_chat_cq", json!({})),
        ("set_beacon", json!({ "on": true })),
        ("set_mode", json!({ "mode": "qso-run" })),
    ] {
        assert_eq!(way(cmd, &args), Way::Start, "{cmd} {args}");
    }
    for (cmd, args) in [
        ("aprs_arm", json!({ "on": false })),
        ("set_chat_cq", json!({ "on": false })),
        ("set_beacon", json!({ "on": false })),
        ("set_mode", json!({ "mode": "qso-monitor" })),
    ] {
        assert_eq!(way(cmd, &args), Way::Shack { stop: true }, "{cmd} {args}");
    }
    assert_eq!(
        way("set_ptt", &json!({ "on": false })),
        Way::Shack { stop: true }
    );
    assert_eq!(way("halt_tx", &json!({})), Way::Shack { stop: true });
    assert_eq!(way("get_band_plan", &json!({})), Way::Shack { stop: false });
    assert_eq!(way("open_external_url", &json!({})), Way::Here);
    assert_eq!(
        way("set_lotw_password", &json!({})),
        Way::Refused(Refusal::ShackOnly)
    );
}

/// ★ Deny by default at this end, through the IPC every window uses: a command with no class, one
/// only the shack's own screen may give, a wired start with no Stop road, and a password typed
/// here are refused here with their codes, and nothing reaches the shack or this computer's own
/// handler. CONTROL: a station command reaches the shack, a command of
/// this computer's runs here, and a Stop and the off half of a switch reach the shack.
#[test]
fn a_command_refused_here_fails_closed_and_nothing_is_sent() {
    let dir = scratch("closed");
    let shack = FakeShack::new();
    let client = client_in(&dir, &shack);
    let s = screens(&client);
    for (cmd, args, code) in [
        ("no_such_command", json!({}), "unclassed"),
        ("repair_clock", json!({}), "shackOnly"),
        ("clear_lotw_password", json!({}), "shackOnly"),
        ("send_cw", json!({ "text": "CQ" }), "noStopRoad"),
        ("set_tune", json!({ "on": true }), "noStopRoad"),
        ("rtty_send", json!({ "text": "CQ" }), "noStopRoad"),
        ("psk_set_latched", json!({ "on": true }), "noStopRoad"),
        ("call_cq", json!({}), "noStopRoad"),
        ("set_tx_enabled", json!({ "enabled": true }), "noStopRoad"),
        ("set_ptt", json!({ "on": true }), "noStopRoad"),
        (
            "winlink_connect",
            json!({ "password": "typed" }),
            "typedPassword",
        ),
    ] {
        assert_eq!(ask(&s.main, cmd, args), refused(code), "{cmd}");
    }
    assert!(shack.sent().is_empty(), "{:?}", shack.sent());
    assert!(s.ran_here().is_empty(), "{:?}", s.ran_here());
    assert_eq!(
        ask(&s.main, "get_band_plan", json!({})),
        Ok(json!({ "ran": "get_band_plan" }))
    );
    assert_eq!(
        ask(&s.main, "app_version", json!({})),
        Ok(json!("ran here"))
    );
    assert!(ask(&s.main, "halt_tx", json!({})).is_ok());
    assert!(ask(&s.main, "set_tx_enabled", json!({ "enabled": false })).is_ok());
    assert_eq!(shack.sent(), ["get_band_plan", "halt_tx", "set_tx_enabled"]);
    assert_eq!(s.ran_here(), ["app_version"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ With no link to the station, a call says so at once, a Stop included: nothing waits for a
/// link that may never come, and nothing runs here in its place.
#[test]
fn with_no_link_a_call_says_so_at_once() {
    let dir = scratch("nolink");
    let shack = FakeShack::new();
    let client = client_in(&dir, &shack);
    client.install(None);
    let s = screens(&client);
    let started = std::time::Instant::now();
    assert_eq!(ask(&s.main, "get_band_plan", json!({})), refused("notLive"));
    assert_eq!(ask(&s.main, "halt_tx", json!({})), refused("notLive"));
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(shack.sent().is_empty() && s.ran_here().is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

// --- The caps ------------------------------------------------------------------------------------

/// ★ The caps, at this end: with the shack holding every call, ten calls of one command put four
/// at the shack and wait here, and calls of other commands fill the rest of the eight; a Stop then
/// reaches the shack at once all the same. Let go, every call is answered, and never were more
/// than eight running at once, nor more than four of one command.
#[test]
fn calls_over_the_caps_wait_here_and_a_stop_takes_no_slot() {
    let dir = scratch("caps");
    let shack = FakeShack::new();
    let client = client_in(&dir, &shack);
    let s = screens(&client);
    shack.hold();
    let mut waiting = Vec::new();
    for _ in 0..10 {
        waiting.push(send(&s.main, "get_band_plan", json!({})));
    }
    eventually("four of one command at the shack", || {
        shack.running() == PER_COMMAND
    });
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        shack.running(),
        PER_COMMAND,
        "no more than four of one command"
    );
    for cmd in [
        "ask_log",
        "get_propagation",
        "get_openings_log",
        "get_band_outlook",
    ] {
        for _ in 0..3 {
            waiting.push(send(&s.main, cmd, json!({})));
        }
    }
    eventually("the eight slots full", || shack.running() == IN_FLIGHT);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(shack.running(), IN_FLIGHT, "no more than eight at once");
    let stop = send(&s.main, "halt_tx", json!({}));
    eventually("the Stop at the shack", || {
        shack.sent().iter().any(|cmd| cmd == "halt_tx")
    });
    shack.let_go();
    assert!(stop.recv_timeout(Duration::from_secs(5)).unwrap().is_ok());
    for answer in waiting {
        assert!(answer.recv_timeout(Duration::from_secs(5)).unwrap().is_ok());
    }
    assert_eq!(*lock(&shack.most), (IN_FLIGHT, PER_COMMAND));
    assert_eq!(shack.sent().len(), 10 + 12 + 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

// --- The feeds -----------------------------------------------------------------------------------

/// ★ The screens' polls are answered here, from what the shack pushes, and never cross the
/// network: the snapshot and the meters are the newest values; each window gets each waterfall row
/// once, and "nothing new" (the empty row) after it; with the link down the panes keep the shack's
/// last state and the waterfall stops. Before anything has arrived, a poll says so.
#[test]
fn the_screens_polls_are_answered_here_from_the_feeds() {
    let dir = scratch("feeds");
    let shack = FakeShack::new();
    let client = client_in(&dir, &shack);
    let s = screens(&client);
    assert_eq!(ask(&s.main, "get_snapshot", json!({})), refused("notLive"));
    let empty = json!({ "row": [], "loHz": 0.0, "hiHz": 0.0, "source": "" });
    assert_eq!(
        ask(&s.main, "get_spectrum_row", json!({})),
        Ok(empty.clone())
    );
    shack.feeds("snapshot/1", 3, json!({ "n": 3 }));
    assert_eq!(
        ask(&s.main, "get_rtty_scope", json!({})),
        refused("notLive")
    );
    shack.feeds("meters/1", 9, json!({ "rxLevel": 0.5 }));
    let scope = json!({ "live": true, "seq": 40, "peak": 0.4, "xy": [127, 3], "offsetHz": -12.0 });
    shack.feeds("rttyScope/1", 4, scope.clone());
    let row = json!({ "row": [0.25, 0.5], "loHz": 0.0, "hiHz": 4000.0, "source": "" });
    shack.feeds("spectrumRow/1", 7, row.clone());
    assert_eq!(
        ask(&s.main, "get_snapshot", json!({})),
        Ok(json!({ "n": 3 }))
    );
    assert_eq!(
        ask(&s.main, "get_meters", json!({})),
        Ok(json!({ "rxLevel": 0.5 }))
    );
    // The tuning scope: the newest frame to every window that asks, from the feed.
    assert_eq!(ask(&s.main, "get_rtty_scope", json!({})), Ok(scope.clone()));
    assert_eq!(ask(&s.popout, "get_rtty_scope", json!({})), Ok(scope));
    assert_eq!(ask(&s.main, "get_spectrum_row", json!({})), Ok(row.clone()));
    assert_eq!(
        ask(&s.main, "get_spectrum_row", json!({})),
        Ok(empty.clone())
    );
    assert_eq!(
        ask(&s.popout, "get_spectrum_row", json!({})),
        Ok(row.clone())
    );
    shack.feeds("spectrumRow/1", 8, json!({ "row": [1.0] }));
    assert_eq!(
        ask(&s.main, "get_spectrum_row", json!({})),
        Ok(json!({ "row": [1.0] }))
    );
    // Nor does the launch picker's question: this computer drives no radio of its own.
    assert_eq!(
        ask(&s.main, "radio_launch_info", json!({})),
        Ok(json!({ "showPicker": false, "radios": [] }))
    );
    assert!(
        shack.sent().is_empty(),
        "no poll crossed: {:?}",
        shack.sent()
    );
    client.install(None);
    assert_eq!(
        ask(&s.main, "get_snapshot", json!({})),
        Ok(json!({ "n": 3 }))
    );
    assert_eq!(ask(&s.main, "get_spectrum_row", json!({})), Ok(empty));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A new link numbers its feeds from 1 again: each window's rows start again with it, so the
/// waterfall does not wait for the new numbers to pass the old.
#[test]
fn a_new_link_starts_each_windows_rows_again() {
    let dir = scratch("rows");
    let shack = FakeShack::new();
    let client = client_in(&dir, &shack);
    let s = screens(&client);
    shack.feeds("spectrumRow/1", 500, json!({ "row": [0.5] }));
    assert_eq!(
        ask(&s.main, "get_spectrum_row", json!({})),
        Ok(json!({ "row": [0.5] }))
    );
    let next = FakeShack::new();
    next.feeds("spectrumRow/1", 1, json!({ "row": [0.75] }));
    client.install(Some(next as Arc<dyn Shack>));
    assert_eq!(
        ask(&s.main, "get_spectrum_row", json!({})),
        Ok(json!({ "row": [0.75] }))
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

// --- This computer's own -------------------------------------------------------------------------

/// ★ The ui-state pair splits by key (the operator's ruling of 2026-10-07: the rail, its order,
/// the UI scale and the window layouts stay on the computer the operator sits at). A save sends
/// the shack the station's keys and nothing else, with the revision of the copy this window
/// loaded; this computer's own keys go to its own file, where its own station's memories and watch
/// list stay as they were. A load is the shack's station keys and this computer's own, and a key
/// of this computer's in the shack's answer is not taken.
#[test]
fn this_computers_keys_of_the_ui_state_never_reach_the_shack() {
    let dir = scratch("split");
    let file = dir.join("ui-state.json");
    std::fs::write(
        &file,
        json!({
            "nexus.watchlist": "[\"MINE\"]",
            "nexus.navOrder": "[\"cw\"]",
        })
        .to_string(),
    )
    .unwrap();
    let shack = FakeShack::new();
    shack.answers(
        "ui_state_load",
        Answer::Value {
            value: json!({ "nexus.watchlist": "[\"SHACK\"]", "nexus-ui-scale-cap": "9" }),
            revision: Some("r1".to_string()),
        },
    );
    shack.answers(
        "ui_state_save",
        Answer::Value {
            value: json!(true),
            revision: Some("r2".to_string()),
        },
    );
    let client = client_in(&dir, &shack);
    let s = screens(&client);
    assert_eq!(
        ask(&s.main, "ui_state_load", json!({})),
        Ok(json!({ "nexus.watchlist": "[\"SHACK\"]", "nexus.navOrder": "[\"cw\"]" }))
    );
    let own = [
        ("nexus.navOrder", "[\"ft8\",\"cw\"]"),
        ("nexus-ui-scale-cap", "1.5"),
        ("nexus.features.v1", "{\"cw\":false}"),
        ("nexus.panels.ft8.main", "{}"),
        ("nexus.connect.switch.main", "kept"),
    ];
    let mut state: serde_json::Map<String, Value> =
        own.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    state.insert("nexus.watchlist".into(), json!("[\"SHACK\",\"W1AW\"]"));
    state.insert("nexus.memory.bank.v2".into(), json!("[1]"));
    assert_eq!(
        ask(&s.main, "ui_state_save", json!({ "state": state })),
        Ok(json!(true))
    );
    let saved = lock(&shack.sent)
        .iter()
        .find(|(cmd, ..)| cmd == "ui_state_save")
        .cloned()
        .expect("a save at the shack");
    assert_eq!(
        saved.1,
        json!({ "state": {
            "nexus.watchlist": "[\"SHACK\",\"W1AW\"]",
            "nexus.memory.bank.v2": "[1]",
        }}),
        "only the station's keys went"
    );
    assert_eq!(
        saved.2.as_deref(),
        Some("r1"),
        "with the revision this window loaded"
    );
    let here: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let mut expected: serde_json::Map<String, Value> =
        own.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    expected.insert("nexus.watchlist".into(), json!("[\"MINE\"]"));
    assert_eq!(
        here,
        Value::Object(expected),
        "this computer's file: its own keys, as saved"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ The update channel and start at sign-in are this computer's: the shack's settings are shown
/// with this computer's two in place of the shack's, and turning the update channel on writes this
/// computer's own settings and sends the shack nothing. CONTROL: every other field is the shack's.
#[test]
fn this_computers_update_channel_and_start_at_sign_in_stay_here() {
    let dir = scratch("this");
    std::fs::write(
        dir.join("settings.json"),
        json!({ "betaUpdates": false, "launchAtLogin": false }).to_string(),
    )
    .unwrap();
    let shack = FakeShack::new();
    shack.answers(
        "get_settings",
        Answer::Value {
            value: json!({ "mycall": "KD9TAW", "betaUpdates": true, "launchAtLogin": true }),
            revision: Some("r1".to_string()),
        },
    );
    shack.feeds("snapshot/1", 1, json!({ "n": 1 }));
    let client = client_in(&dir, &shack);
    let s = screens(&client);
    assert_eq!(
        ask(&s.main, "get_settings", json!({})),
        Ok(
            json!({ "mycall": "KD9TAW", "betaUpdates": false, "launchAtLogin": false,
            "nexusRevision": "r1" })
        )
    );
    assert_eq!(
        ask(&s.main, "set_beta_updates", json!({ "on": true })),
        Ok(json!({ "n": 1 }))
    );
    let file: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
    assert_eq!(
        file["betaUpdates"],
        json!(true),
        "kept in this computer's settings"
    );
    assert_eq!(
        ask(&s.main, "get_settings", json!({}))
            .unwrap()
            .get("betaUpdates"),
        Some(&json!(true))
    );
    assert_eq!(
        shack.sent(),
        ["get_settings", "get_settings"],
        "nothing else went"
    );
    assert!(
        s.ran_here().is_empty(),
        "client mode answered it, not the app's handler"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

// --- Revisions -----------------------------------------------------------------------------------

/// ★ A save goes with the revision of the copy its own window holds: each window's last read of
/// the store, or its last save the shack took. CONTROL: a read sends none, and a window that never
/// read the store sends none, which the shack refuses as it should.
#[test]
fn a_save_goes_with_the_revision_of_the_copy_its_window_holds() {
    let dir = scratch("revisions");
    let shack = FakeShack::new();
    let client = client_in(&dir, &shack);
    let s = screens(&client);
    let answer = |revision: &str| Answer::Value {
        value: json!({}),
        revision: Some(revision.to_string()),
    };
    shack.answers("get_settings", answer("rMain"));
    ask(&s.main, "get_settings", json!({})).unwrap();
    shack.answers("get_settings", answer("rPopout"));
    ask(&s.popout, "get_settings", json!({})).unwrap();
    shack.answers("set_settings", answer("rSaved"));
    ask(&s.main, "set_settings", json!({ "settings": {} })).unwrap();
    ask(&s.popout, "set_rtty_macros", json!({ "macros": {} })).unwrap();
    ask(&s.main, "set_blocked_calls", json!({ "calls": [] })).unwrap();
    let bases: Vec<(String, Option<String>)> = lock(&shack.sent)
        .iter()
        .map(|(cmd, _, base)| (cmd.clone(), base.clone()))
        .collect();
    assert_eq!(
        bases,
        [
            ("get_settings".to_string(), None),
            ("get_settings".to_string(), None),
            ("set_settings".to_string(), Some("rMain".to_string())),
            ("set_rtty_macros".to_string(), Some("rPopout".to_string())),
            ("set_blocked_calls".to_string(), Some("rSaved".to_string())),
        ]
    );
    let fresh = client_in(&dir, &shack);
    let other = screens(&fresh);
    ask(&other.main, "set_settings", json!({ "settings": {} })).unwrap();
    assert_eq!(lock(&shack.sent).last().unwrap().2, None);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ A call the link cut off is said to be gone, never sent again, and kept for the next link to
/// ask after. A refusal at the shack comes back with its code, and a command's own error as it is.
#[test]
fn a_call_cut_off_is_said_and_asked_after_and_a_refusal_keeps_its_code() {
    let dir = scratch("gone");
    let shack = FakeShack::new();
    shack.answers("ask_log", Answer::Gone);
    shack.answers("set_settings", Answer::Refused(Refusal::ChangedAtShack));
    shack.answers("add_radio", Answer::Error("no room".to_string()));
    let client = client_in(&dir, &shack);
    let s = screens(&client);
    assert_eq!(ask(&s.main, "ask_log", json!({})), refused("gone"));
    assert_eq!(lock(&client.gone).len(), 1);
    assert_eq!(
        ask(&s.main, "set_settings", json!({})),
        refused("changedAtShack")
    );
    assert_eq!(ask(&s.main, "add_radio", json!({})), Err(json!("no room")));
    assert_eq!(shack.sent(), ["ask_log", "set_settings", "add_radio"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

// --- No station here -----------------------------------------------------------------------------

/// Everything of the crate's own this module may reach, each read here to start nothing of a
/// station: the dispatcher's tables and the link; this computer's own files; the windows and
/// their layouts; the app's command list and context; and the state the "runs here" commands read.
const MAY_REACH: &[&str] = &[
    "crate::command_classes",
    "crate::command_dispatch",
    "crate::lan_client::link",
    // The road's order of addresses, and what a look by name may try: it reads a record and an
    // advert, and starts nothing.
    "crate::lan_client::road",
    "crate::lan_client::stop_road",
    // This computer's headset for the shack's audio: its own output and microphone, opened only
    // while wanted. It plays and captures, and starts nothing of a station.
    "crate::lan_client::audio_road",
    "crate::lan_client::Stations",
    // Where the typed file is: a path beside the default profile's settings. It opens nothing.
    "crate::lan_client::typed_path",
    // Where the sign-in times are, beside it; and this computer's name. They open nothing.
    "crate::lan_client::signed_path",
    "crate::lan_client::computer_name",
    // Signing in again for the station this launch drives: the system browser, this computer's
    // own requests to the Remote service and its issuer, and its enrolment on a connection of
    // its own to the station, which keeps it. It keys nothing and starts nothing of a station.
    "crate::lan_client::account",
    // The internet road: this computer's own requests to the Remote service (a pipe, its relay, a
    // renewal) and the road's record in this computer's store. It opens a link and starts nothing
    // of a station.
    "crate::lan_client::internet",
    // A test build's road switch: this computer's own roads to the shack, blocked one at a time.
    // It opens and starts nothing.
    "crate::bench_road_switch",
    // WebView2's browser keys off in each window as Tauri makes it: one setting of the webview's
    // own, and nothing else.
    "crate::browser_keys::init",
    "crate::remote_service::vault::PairedStation",
    "crate::remote_service::vault::SystemVault",
    "crate::ui_state_read",
    "crate::ui_state_path",
    "crate::settings_path",
    // Where the last client target is kept, beside the default profile's settings: a path. It
    // resolves a directory and opens nothing.
    "crate::config_dir_for",
    "crate::write_json_atomic",
    "crate::active_profile",
    "crate::send_main_window_to_the_app",
    "crate::app_commands",
    "crate::context",
    "crate::BetaUpdateState",
    "crate::street_map::StreetMapState",
    "crate::remote_window::LanOrigin",
    "crate::RESOURCE_DIR",
    "crate::DISPLAY_DPI",
    "crate::probe_physical_dpi",
    "crate::DOCUMENTS_DIR",
    "crate::PICTURES_DIR",
    "crate::window_state::install",
    "crate::capture_all_window_geometry",
    "crate::capture_bandmap_window",
    "crate::show_startup_failure",
    // The panic hook's flag that the window is up. It sets one flag and starts nothing.
    "crate::startup_reached_window",
];

/// What starts a station, or holds one: none of it may be named in client mode.
const A_STATION: &[&str] = &[
    "SharedEngine",
    "engine_lock",
    "Engine::",
    "launch_engine",
    "start_on_the_logbook",
    "finish_launch",
    "remote_service_for",
    "build_app",
    "quit_cleanup",
    "tempo_audio",
    "start_cluster_feeds",
    "start_pskr_feed",
    "start_wspr_feed",
    "start_pskr_region_feed",
    "fdsync",
    "fd_scoreboard",
    "connect_web",
    "rigctld",
    "pouncer",
    "Settings::load",
    "logbook",
];

/// The module's code: its source with every comment taken out, so its prose (which says what it
/// never opens) is not read as code. Its tests are in this file, not in it.
fn client_source() -> String {
    include_str!("../client_mode.rs")
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Client mode's window is up once its setup runs, and it says so: the panic hook shows "Nexus could
/// not start" only while no window has been reached, and only the station's own setup used to say
/// it, so on a computer driving a station every later panic raised that box over a running app.
/// CONTROL: the scan finds the setup it reads.
#[test]
fn client_mode_marks_its_window_reached() {
    let source = client_source();
    let run = &source[source
        .find("pub(crate) fn run(station: String")
        .expect("run")..];
    let setup = &run[run.find(".setup(move |app|").expect("the setup")..];
    let setup = &setup[..setup.find(".on_window_event(").expect("the setup's end")];
    assert!(
        setup.contains("crate::window_state::install"),
        "the scan reads the setup"
    );
    assert!(
        setup.contains("crate::startup_reached_window()"),
        "client mode's setup marks the window reached"
    );
}

/// ★ No station here, by construction: client mode names nothing that starts or holds a station
/// (no Engine, no logbook, no radio or daemon, no feed, listener or uploader), and of the crate's
/// own it reaches only what [`MAY_REACH`] lists, each of which starts none. The app it builds
/// manages client mode's state and the state of this computer's own commands, and no Engine.
/// CONTROL: the scan finds the module's own calls, and names it was planted with.
#[test]
fn client_mode_reaches_nothing_of_a_station() {
    let source = client_source();
    let source = source.as_str();
    for name in A_STATION {
        assert!(!source.contains(name), "client mode names `{name}`");
    }
    let mut reached: Vec<&str> = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find("crate::") {
        let path: &str = &rest[at..];
        let end = path
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
            .unwrap_or(path.len());
        reached.push(path[..end].trim_end_matches(':'));
        rest = &rest[at + end..];
    }
    assert!(
        reached.contains(&"crate::app_commands"),
        "the scan reads the calls: {reached:?}"
    );
    for path in &reached {
        assert!(
            MAY_REACH.iter().any(|allowed| path.starts_with(allowed)),
            "client mode reaches `{path}`: read it, and list it in MAY_REACH if it starts nothing"
        );
    }
    let run = &source[source
        .find("pub(crate) fn run(station: String")
        .expect("run")..];
    let managed: Vec<&str> = run
        .match_indices(".manage(")
        .map(|(at, _)| {
            let inner = &run[at + ".manage(".len()..];
            &inner[..inner.find(")\n").expect("one line")]
        })
        .collect();
    assert_eq!(
        managed,
        [
            "Arc::clone(&client)",
            "crate::BetaUpdateState::default()",
            "crate::street_map::StreetMapState::default()",
            "crate::remote_window::LanOrigin::default()",
        ]
    );
    // The scan is not blind: a planted name is found.
    let planted = format!("{source}\n    crate::start_cluster_feeds(&spots);");
    assert!(planted.contains("start_cluster_feeds"));
}

/// ★ A launch in client mode branches off before anything of this computer's own station opens:
/// `run` asks for client mode before it reads its settings, looks at the logbook, takes its
/// profile's lock, loads the country file, builds an Engine, starts a feed or builds the station's
/// app.
#[test]
fn run_branches_off_before_anything_of_the_station_opens() {
    let lib = include_str!("../lib.rs");
    let run = &lib[lib.find("\npub fn run() {").expect("run")..];
    let run = &run[..run.find("\n}\n").expect("the end of run")];
    let branch = run
        .find("let client = client_mode::requested()")
        .expect("the branch");
    for opens in [
        "converting_elsewhere(",
        "hold_profile_lock()",
        "Settings::load(",
        "cty_load_from_disk()",
        "launch_engine(",
        "start_cluster_feeds(",
        "start_pskr_feed(",
        "build_app(",
    ] {
        let at = run
            .find(opens)
            .unwrap_or_else(|| panic!("run() has `{opens}`"));
        assert!(branch < at, "client mode branches off before `{opens}`");
    }
    assert!(
        run[branch..].starts_with(
            "let client = client_mode::requested()\n        .map(|station| (station, client_mode::typed_in(std::env::args().skip(1))))\n        .or_else(client_mode::auto_requested);\n    if let Some((station, typed)) = client {\n        client_mode::run(station, typed);\n        return;"
        ),
        "and goes no further into run()"
    );
}

/// ★ Leave, and Operate from this computer, quit through the main window's close, which is the
/// station's own quit (its radio stopped first, its logbook saved), and the next Nexus starts only
/// once this one has gone. CONTROL: the station's own run loop starts it at its very end.
#[test]
fn starting_again_waits_for_the_quit() {
    let source = client_source();
    let again = &source[source
        .find("pub(crate) fn start_again")
        .expect("start_again")..];
    let again = &again[..again.find("\n}\n").expect("its end")];
    assert!(again.contains("main.close()"), "{again}");
    assert!(!again.contains("spawn"), "nothing starts before the quit");
    let lib = include_str!("../lib.rs");
    let looped = &lib[lib
        .find("    app.run(|app_handle, event| {")
        .expect("the run loop")..];
    let exit = &looped[looped.find("tauri::RunEvent::Exit => {").expect("the exit")..];
    let exit = &exit[..exit.find('}').expect("its end")];
    let cleaned = exit
        .find("quit_cleanup(app_handle);")
        .expect("the quit's cleanup");
    let again = exit
        .find("client_mode::started_again();")
        .expect("the start again");
    assert!(cleaned < again, "{exit}");
}

/// ★ The launch's argument names the station by its id, either way it is written; anything that
/// is not an id a station could have starts Nexus as itself. CONTROL: no argument, no station.
#[test]
fn the_launch_names_the_station_it_drives() {
    let args = |list: &[&str]| {
        list.iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    };
    assert_eq!(
        requested_in(args(&["--client", STATION])),
        Some(STATION.to_string())
    );
    assert_eq!(
        requested_in(args(&["--profile", "r1", &format!("--client={STATION}")])),
        Some(STATION.to_string())
    );
    for not_an_id in [
        "",
        "shack",
        "0123ABCD-0123-4567-89AB-0123456789AB",
        "../../etc",
    ] {
        assert_eq!(
            requested_in(args(&["--client", not_an_id])),
            None,
            "{not_an_id:?}"
        );
    }
    assert_eq!(requested_in(args(&["--client"])), None);
    assert_eq!(requested_in(args(&["--profile", "r1"])), None);
}

/// ★ Operate at an address the operator typed: the launch the stations window starts names the
/// station and the address or host name, and this launch reads both back, as typed. CONTROL: with
/// none typed the launch is the station alone and nothing is read; what the window would not take
/// (a loopback address, a port below 1024, a name no host could have) is never read from the
/// launch either.
#[test]
fn a_typed_address_goes_with_the_launch() {
    for typed in [
        Typed::At("192.168.1.30:42075".parse().unwrap()),
        Typed::Name {
            host: "shack-pc.local".into(),
            port: 42080,
        },
    ] {
        let launch = operate_args(STATION.into(), Some(typed.clone()));
        assert_eq!(
            requested_in(launch.clone().into_iter()),
            Some(STATION.into())
        );
        assert_eq!(typed_in(launch.into_iter()), Some(typed));
    }
    let launch = operate_args(STATION.into(), None);
    assert_eq!(launch, vec![ARG.to_string(), STATION.to_string()]);
    assert_eq!(typed_in(launch.into_iter()), None);
    let args = |list: &[&str]| {
        list.iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    };
    assert_eq!(
        typed_in(args(&["--client", STATION, "--client-at=192.168.1.30"])),
        Some(Typed::At("192.168.1.30:42075".parse().unwrap()))
    );
    assert_eq!(
        typed_in(args(&["--client", STATION, "--client-at", "shack.local"])),
        Some(Typed::Name {
            host: "shack.local".into(),
            port: tempo_stream::lan::PORT
        })
    );
    for not_taken in [
        "127.0.0.1:42075",
        "192.168.1.30:80",
        "shack.local:80",
        "shack_pc",
        "",
    ] {
        assert_eq!(
            typed_in(args(&["--client", STATION, "--client-at", not_taken])),
            None,
            "{not_taken:?}"
        );
    }
    assert_eq!(typed_in(args(&["--client", STATION, "--client-at"])), None);
}

// --- Stop's second road --------------------------------------------------------------------------

/// The shack's end of a Stop road, for these tests: a socket that a thread of its own reads as
/// fast as it can, noting when each datagram arrived and what it was, and that answers the road's
/// computer as the shack would ([`RoadEnd::say`]).
struct RoadEnd {
    heard: Arc<
        Mutex<
            Vec<(
                crate::remote_service::lan::stop_road::Up,
                std::time::Instant,
            )>,
        >,
    >,
    listening: Arc<AtomicBool>,
    socket: std::net::UdpSocket,
    send: crate::remote_service::lan::datagram::Sealer,
    /// Where the road's computer last spoke from.
    peer: Arc<Mutex<Option<std::net::SocketAddr>>>,
}

impl RoadEnd {
    /// `down`, as the shack says it, to where the computer last spoke from.
    fn say(&self, down: &crate::remote_service::lan::stop_road::Down) {
        let peer = lock(&self.peer).expect("the computer has spoken");
        let datagram = self.send.seal(down.kind(), &down.body()).unwrap();
        self.socket.send_to(&datagram, peer).unwrap();
    }

    /// What the road sent, in order.
    fn ups(&self) -> Vec<crate::remote_service::lan::stop_road::Up> {
        lock(&self.heard).iter().map(|(up, _)| up.clone()).collect()
    }

    /// The holds and releases the road sent, in order.
    fn presses(&self) -> Vec<crate::remote_service::lan::stop_road::Up> {
        use crate::remote_service::lan::stop_road::Up;
        self.ups()
            .into_iter()
            .filter(|up| matches!(up, Up::Hold { .. } | Up::Release { .. }))
            .collect()
    }
}

impl Drop for RoadEnd {
    fn drop(&mut self) {
        self.listening.store(false, Ordering::SeqCst);
    }
}

/// A road's tag that no other road in this run has, as the shack names its roads from a random
/// start. These tests' roads share their key material, and a road outlives its test (the mock app
/// keeps its client, and an unacknowledged Stop is sent again for five seconds): under one tag for
/// all, a road an earlier test left running spoke on a later test's road whenever that test's end
/// was handed the same port. Its datagrams verified there: they drew the end's answers to the
/// wrong road, and could carry its replay window past the later road's own.
fn a_tag_of_its_own() -> crate::remote_service::lan::datagram::Tag {
    crate::remote_service::lan::stop_road::first_tag().to_be_bytes()
}

/// A shack with a Stop road to an end of the test's own.
fn shack_with_a_road() -> (Arc<FakeShack>, RoadEnd) {
    use crate::remote_service::lan::datagram::{End, Keys, EXPORTED};
    use crate::remote_service::lan::stop_road::Up;
    let exported = [5; EXPORTED];
    let tag = a_tag_of_its_own();
    let socket = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket.set_nonblocking(true).unwrap();
    let road =
        StopRoad::open(socket.local_addr().unwrap(), tag, &exported, Arc::default()).unwrap();
    let shack = FakeShack::new();
    *lock(&shack.road) = Some(road);
    let Keys { send, mut receive } = Keys::new(&exported, tag, End::Shack).unwrap();
    let heard = Arc::new(Mutex::new(Vec::new()));
    let listening = Arc::new(AtomicBool::new(true));
    let peer = Arc::new(Mutex::new(None));
    let answering = socket.try_clone().unwrap();
    {
        let (heard, listening, peer) = (heard.clone(), listening.clone(), peer.clone());
        std::thread::spawn(move || {
            let mut buffer = [0u8; 256];
            while listening.load(Ordering::SeqCst) {
                if let Ok((length, from)) = socket.recv_from(&mut buffer) {
                    let at = std::time::Instant::now();
                    let up = receive
                        .open(&buffer[..length])
                        .and_then(|(kind, body)| Up::read(kind, &body));
                    if let Some(up) = up {
                        *lock(&peer) = Some(from);
                        lock(&heard).push((up, at));
                    }
                }
            }
        });
    }
    (
        shack,
        RoadEnd {
            heard,
            listening,
            socket: answering,
            send,
            peer,
        },
    )
}

/// A computer on a shack with a road whose end says presence is live for `live`: its lease handed
/// to the road, the main window's snapshot fetched, so the road's heartbeat lets the end learn
/// where it is, and then the end's word.
fn present_for(live: Duration) -> (Arc<FakeShack>, RoadEnd, Arc<Client>, Screens, PathBuf) {
    use crate::remote_service::lan::stop_road::Down;
    let (shack, end) = shack_with_a_road();
    shack.feeds("snapshot/1", 3, json!({"n": 3}));
    let dir = scratch("present");
    let client = client_in(&dir, &shack);
    let s = screens(&client);
    let road = lock(&shack.road).clone().unwrap();
    road.lease(Some("30000000-0000-4000-8000-000000000001".into()));
    assert_eq!(ask(&s.main, "get_snapshot", json!({})), Ok(json!({"n": 3})));
    eventually("a heartbeat", || lock(&end.peer).is_some());
    end.say(&Down::Presence {
        left_ms: u32::try_from(live.as_millis()).unwrap(),
    });
    eventually("presence heard", || {
        road.presence_live(std::time::Instant::now())
    });
    (shack, end, client, s, dir)
}

/// ★ A wired start leaves only while both roads are up and presence is live as the road last heard
/// it: refused with no road, then with a road whose computer has heard no presence (a press too,
/// with no hold sent), each with its code and nothing sent; sent once presence is heard; and refused
/// again once that deadline has passed with no renewal (stale presence). CONTROL: a station command
/// goes all the while.
#[test]
fn a_start_leaves_only_with_both_roads_up_and_live_presence() {
    let dir = scratch("start-roads");
    let bare = FakeShack::new();
    let client = client_in(&dir, &bare);
    let s = screens(&client);
    assert_eq!(ask(&s.main, "call_cq", json!({})), refused("noStopRoad"));
    assert!(bare.sent().is_empty(), "{:?}", bare.sent());
    let (shack, end) = shack_with_a_road();
    client.install(Some(Arc::clone(&shack) as Arc<dyn Shack>));
    assert_eq!(ask(&s.main, "call_cq", json!({})), refused("noPresence"));
    assert_eq!(
        ask(&s.main, "set_ptt", json!({"on": true})),
        refused("noPresence"),
        "a press is a start too"
    );
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        end.presses().is_empty(),
        "no hold went: {:?}",
        end.presses()
    );
    assert!(shack.sent().is_empty(), "{:?}", shack.sent());
    drop(end);
    std::fs::remove_dir_all(&dir).unwrap();

    let (shack, _end, _client, s, dir) = present_for(Duration::from_millis(600));
    let heard = std::time::Instant::now();
    assert_eq!(
        ask(&s.main, "call_cq", json!({})),
        Ok(json!({"ran": "call_cq"}))
    );
    assert_eq!(
        ask(&s.main, "play_voice_message", json!({"slot": 1})),
        Ok(json!({"ran": "play_voice_message"}))
    );
    assert_eq!(shack.sent(), ["call_cq", "play_voice_message"]);
    std::thread::sleep(
        (heard + Duration::from_millis(650)).saturating_duration_since(std::time::Instant::now()),
    );
    assert_eq!(ask(&s.main, "start_cq", json!({})), refused("noPresence"));
    assert_eq!(
        ask(&s.main, "get_band_plan", json!({})),
        Ok(json!({"ran": "get_band_plan"}))
    );
    assert_eq!(
        shack.sent(),
        ["call_cq", "play_voice_message", "get_band_plan"]
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ The PTT here is a held press on Stop's road, never the shack's `set_ptt`: each re-assertion
/// from the screen is one hold of one press, written as it is asked and never on a timer of this
/// computer's own; letting go is its release on the road and the off half on the link; the next
/// press is a new one. CONTROL: no hold goes while the screen asks nothing.
#[test]
fn the_ptt_here_is_a_held_press_on_stop_road() {
    use crate::remote_service::lan::stop_road::Up;
    let (shack, end, _client, s, dir) = present_for(Duration::from_secs(5));
    for _ in 0..3 {
        assert!(ask(&s.main, "set_ptt", json!({"on": true})).is_ok());
        std::thread::sleep(Duration::from_millis(100));
    }
    eventually("three holds", || end.presses().len() >= 3);
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        end.presses(),
        [
            Up::Hold { press: 1, stops: 0 },
            Up::Hold { press: 1, stops: 0 },
            Up::Hold { press: 1, stops: 0 }
        ],
        "one hold a re-assertion, none of its own"
    );
    assert!(
        shack.sent().is_empty(),
        "the PTT never reached the link: {:?}",
        shack.sent()
    );
    assert!(ask(&s.main, "set_ptt", json!({"on": false})).is_ok());
    eventually("the release", || end.presses().len() >= 4);
    assert_eq!(end.presses()[3], Up::Release { press: 1 });
    assert_eq!(shack.sent(), ["set_ptt"], "the off half on the link");
    assert_eq!(lock(&shack.sent)[0].1, json!({"on": false}));
    assert!(ask(&s.main, "set_ptt", json!({"on": true})).is_ok());
    eventually("the next press", || end.presses().len() >= 5);
    assert_eq!(
        end.presses()[4],
        Up::Hold { press: 2, stops: 1 },
        "a new press, begun after the off half, which counts as a Stop"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ A press that ended other than by the screen letting go never keys again: after a Stop pressed
/// here, after the station says the press ended, and after a new link, every re-assertion is
/// refused `pressEnded` and no hold goes, on the old road or the new one. A press held when its link
/// changed is released on the old road. CONTROL: once the screen lets go, the next press goes, on
/// the link there is now.
#[test]
fn an_ended_press_never_keys_again_until_the_screen_lets_go() {
    use crate::remote_service::lan::stop_road::{Down, Up};
    use tempo_app::remote_control::ptt_hold::HoldEnd;
    let (shack, end, client, s, dir) = present_for(Duration::from_secs(5));
    let road = lock(&shack.road).clone().unwrap();
    let holds = |end: &RoadEnd| {
        end.presses()
            .into_iter()
            .filter(|up| matches!(up, Up::Hold { .. }))
            .count()
    };
    // A Stop pressed here.
    assert!(ask(&s.main, "set_ptt", json!({"on": true})).is_ok());
    assert!(ask(&s.main, "halt_tx", json!({})).is_ok());
    eventually("the Stop on the road", || {
        end.ups().iter().any(|up| matches!(up, Up::Stop { .. }))
    });
    assert_eq!(
        ask(&s.main, "set_ptt", json!({"on": true})),
        refused("pressEnded")
    );
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(holds(&end), 1, "no hold after the Stop");
    assert!(ask(&s.main, "set_ptt", json!({"on": false})).is_ok());
    // The station says the press ended.
    assert!(ask(&s.main, "set_ptt", json!({"on": true})).is_ok());
    eventually("the second press", || holds(&end) == 2);
    end.say(&Down::Press {
        press: 2,
        keyed: false,
        end: Some(HoldEnd::Lapsed),
    });
    eventually("the station's word heard", || {
        road.heard()
            .presses
            .iter()
            .any(|down| matches!(down, Down::Press { press: 2, .. }))
    });
    assert_eq!(
        ask(&s.main, "set_ptt", json!({"on": true})),
        refused("pressEnded")
    );
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(holds(&end), 2, "no hold after the station's word");
    assert!(ask(&s.main, "set_ptt", json!({"on": false})).is_ok());
    // A new link while a press is held.
    assert!(ask(&s.main, "set_ptt", json!({"on": true})).is_ok());
    eventually("the third press", || holds(&end) == 3);
    let (next, next_end) = shack_with_a_road();
    next.feeds("snapshot/1", 1, json!({"n": 1}));
    client.install(Some(Arc::clone(&next) as Arc<dyn Shack>));
    eventually("released on the old road", || {
        end.presses().last() == Some(&Up::Release { press: 3 })
    });
    let next_road = lock(&next.road).clone().unwrap();
    next_road.lease(Some("30000000-0000-4000-8000-000000000001".into()));
    assert_eq!(ask(&s.main, "get_snapshot", json!({})), Ok(json!({"n": 1})));
    eventually("a heartbeat on the new road", || {
        lock(&next_end.peer).is_some()
    });
    next_end.say(&Down::Presence { left_ms: 5000 });
    eventually("presence on the new road", || {
        next_road.presence_live(std::time::Instant::now())
    });
    assert_eq!(
        ask(&s.main, "set_ptt", json!({"on": true})),
        refused("pressEnded")
    );
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        next_end.presses().is_empty(),
        "nothing keyed on the new link"
    );
    assert_eq!(holds(&end), 3);
    assert!(ask(&s.main, "set_ptt", json!({"on": false})).is_ok());
    assert!(ask(&s.main, "set_ptt", json!({"on": true})).is_ok());
    eventually("a press on the new link", || !next_end.presses().is_empty());
    assert!(matches!(next_end.presses()[0], Up::Hold { .. }));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ No queue at this end. A Stop press leaves on the road within a
/// millisecond while the command link is stuck (the shack answers nothing): the handler writes it
/// itself, before it hands the call to the link. Five presses, each its own Stop: every one leaves,
/// and their median within the millisecond. A queue here (the road's thread sending it, or the link
/// first) delays every press, while a receiving thread descheduled on a loaded box delays one; a
/// single press measured 1.1 ms once beside the whole suite, and 54–99 µs alone. CONTROL: the calls
/// went to the link too, and are still waiting there.
#[test]
fn a_stop_press_leaves_on_the_road_within_a_millisecond_with_the_link_stuck() {
    use crate::remote_service::lan::stop_road::Up;
    let (shack, end) = shack_with_a_road();
    shack.hold();
    let dir = scratch("road-stop");
    let client = client_in(&dir, &shack);
    let screens = screens(&client);
    let mut took = Vec::new();
    let mut answers = Vec::new();
    for stop in 1..=5u64 {
        let first = |heard: &[(Up, std::time::Instant)]| {
            heard
                .iter()
                .find(|(up, _)| *up == Up::Stop { stop })
                .map(|(_, at)| *at)
        };
        let pressed = std::time::Instant::now();
        answers.push(send(&screens.main, "halt_tx", json!({})));
        eventually("the Stop on the road", || {
            first(&lock(&end.heard)[..]).is_some()
        });
        took.push(first(&lock(&end.heard)[..]).unwrap() - pressed);
    }
    eprintln!("measured: the Stops left on the road {took:?} after their presses");
    let mut sorted = took.clone();
    sorted.sort();
    assert!(sorted[2] < Duration::from_millis(1), "the median: {took:?}");
    eventually("the calls on the link", || shack.sent() == ["halt_tx"; 5]);
    assert!(
        answers.iter().all(|answered| answered.try_recv().is_err()),
        "the link answers nothing"
    );
    shack.let_go();
    for answered in &answers {
        assert!(answered.recv_timeout(Duration::from_secs(5)).is_ok());
    }
}

/// ★ No queue at this end, with the audio road's sender stuck as well. The five presses of the
/// test above, with the link stuck (the shack answers nothing) and the audio road's sender held
/// mid-send by another thread while a press is live: every Stop leaves on Stop's road at once,
/// their median within the millisecond, and each ends this computer's press without waiting for
/// the sender. CONTROL: the press was live before each Stop, and the sender was still held when
/// the Stops had gone.
#[test]
fn a_stop_press_leaves_on_the_road_within_a_millisecond_with_the_audio_sender_blocked_too() {
    use crate::lan_client::audio_road::{AudioRoad, StandIn};
    use crate::lan_client::stop_road::OnAir;
    use crate::remote_service::lan::datagram::{End, Keys, EXPORTED};
    use crate::remote_service::lan::stop_road::Up;
    let on_air = Arc::new(OnAir::default());
    let exported = [5; EXPORTED];
    let tag = a_tag_of_its_own();
    let socket = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket.set_nonblocking(true).unwrap();
    let road =
        StopRoad::open(socket.local_addr().unwrap(), tag, &exported, on_air.clone()).unwrap();
    let station = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let _audio = AudioRoad::open(
        station.local_addr().unwrap(),
        [3; 8],
        &[6; EXPORTED],
        StandIn::new(),
        on_air.clone(),
    )
    .unwrap();
    let shack = FakeShack::new();
    *lock(&shack.road) = Some(road);
    shack.hold();
    let mut keys = Keys::new(&exported, tag, End::Shack).unwrap();
    let dir = scratch("road-stop-audio");
    let client = client_in(&dir, &shack);
    let screens = screens(&client);
    on_air.pressing(Some(9));
    std::thread::sleep(Duration::from_millis(50));
    // The sender stuck: held by a thread of its own, as a send that will not return holds it.
    let holder = {
        let on_air = on_air.clone();
        std::thread::spawn(move || {
            let _held = lock(&on_air.sender);
            std::thread::sleep(Duration::from_millis(600));
        })
    };
    eventually("the sender held", || on_air.sender.try_lock().is_err());
    let mut took = Vec::new();
    let mut answers = Vec::new();
    let mut buffer = [0u8; 256];
    for stop in 1..=5u64 {
        on_air.pressing(Some(9));
        let pressed = std::time::Instant::now();
        answers.push(send(&screens.main, "halt_tx", json!({})));
        let deadline = pressed + Duration::from_secs(1);
        let left = loop {
            if let Ok(length) = socket.recv(&mut buffer) {
                let at = std::time::Instant::now();
                let up = keys
                    .receive
                    .open(&buffer[..length])
                    .and_then(|(kind, body)| Up::read(kind, &body));
                if up == Some(Up::Stop { stop }) {
                    break at;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Stop {stop} never left"
            );
        };
        took.push(left - pressed);
        eventually("the press ended", || on_air.press().is_none());
    }
    let held_throughout = on_air.sender.try_lock().is_err();
    eprintln!("measured: with the audio sender held, the Stops left on the road {took:?}");
    assert!(
        took.iter().all(|t| *t < Duration::from_millis(50)),
        "every Stop left at once: {took:?}"
    );
    let mut sorted = took.clone();
    sorted.sort();
    assert!(sorted[2] < Duration::from_millis(1), "the median: {took:?}");
    assert!(held_throughout, "premise: the sender was held throughout");
    holder.join().unwrap();
    shack.let_go();
    for answered in &answers {
        assert!(answered.recv_timeout(Duration::from_secs(5)).is_ok());
    }
}

/// ★ This computer ▸ Audio is this computer's: its four commands run here and never reach the
/// shack, and the headset chosen is kept in this computer's own file beside its settings, read back
/// as client mode starts. CONTROL: nothing else of this computer's is written, its settings file
/// included.
#[test]
fn this_computers_audio_is_kept_here_and_never_sent() {
    for cmd in [
        "client_audio",
        "set_client_audio",
        "client_audio_level",
        "client_audio_test",
    ] {
        assert_eq!(way(cmd, &json!({})), Way::Here, "{cmd}");
    }
    let dir = scratch("audio-here");
    let read = || ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
    let mut this = read();
    assert_eq!((this.output.as_str(), this.input.as_str()), ("", ""));
    assert_eq!(this.volume_db, 24.0, "the hosted page's default volume");
    this.keep_audio("Headset Earphone", "Headset Microphone", 30.0)
        .unwrap();
    let again = read();
    assert_eq!(
        (again.output.as_str(), again.input.as_str()),
        ("Headset Earphone", "Headset Microphone")
    );
    assert_eq!(again.volume_db, 30.0, "the volume is kept with the devices");
    let written: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(written, ["client-audio.json"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ Only a Stop press takes the road: the off half of a switch (TX off, PTT up with no press held)
/// goes by the link alone, as it always has, since TX off lets an FT over finish and a Stop does
/// not. CONTROL: each stop command does take it.
#[test]
fn only_a_stop_press_takes_the_road() {
    let (shack, end) = shack_with_a_road();
    let dir = scratch("road-only-stops");
    let client = client_in(&dir, &shack);
    let screens = screens(&client);
    for (cmd, args) in [
        ("set_tx_enabled", json!({"enabled": false})),
        ("set_ptt", json!({"on": false})),
        ("set_tune", json!({"on": false})),
    ] {
        assert!(ask(&screens.main, cmd, args).is_ok(), "{cmd}");
    }
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        lock(&end.heard).is_empty(),
        "an off half takes the link alone"
    );
    let stops: Vec<&str> = crate::command_classes::classes()
        .filter(|(_, class)| *class == Class::StopsTx)
        .map(|(cmd, _)| cmd)
        .collect();
    assert_eq!(stops.len(), 9);
    for cmd in &stops {
        assert!(ask(&screens.main, cmd, json!({})).is_ok(), "{cmd}");
    }
    eventually("a Stop on the road for each", || {
        lock(&end.heard).len() >= stops.len()
    });
}

/// ★ The heartbeat's echo is the main window's own poll: what a pop-out fetches never moves it,
/// so a main window that hangs lets presence lapse at the shack. CONTROL: the main window's poll
/// moves it, to the number it fetched.
#[test]
fn the_heartbeat_names_what_the_main_window_fetched() {
    use crate::remote_service::lan::stop_road::Up;
    let (shack, end) = shack_with_a_road();
    shack.feeds("snapshot/1", 5, json!({"n": 5}));
    let dir = scratch("road-echo");
    let client = client_in(&dir, &shack);
    let screens = screens(&client);
    lock(&shack.road)
        .as_ref()
        .unwrap()
        .lease(Some("30000000-0000-4000-8000-000000000001".into()));
    assert_eq!(
        ask(&screens.popout, "get_snapshot", json!({})),
        Ok(json!({"n": 5}))
    );
    std::thread::sleep(Duration::from_millis(700));
    assert!(
        lock(&end.heard).is_empty(),
        "a pop-out's poll names nothing"
    );
    assert_eq!(
        ask(&screens.main, "get_snapshot", json!({})),
        Ok(json!({"n": 5}))
    );
    eventually("a heartbeat", || !lock(&end.heard).is_empty());
    assert!(matches!(
        &lock(&end.heard)[0].0,
        Up::Heartbeat { state: 5, .. }
    ));
}

/// ★ A voice message recorded here goes to the shack's slot as an import. The keyer's Record takes
/// this computer's microphone; its Stop & save sends what was recorded as `import_voice_message`
/// with the slot and its label, a 12 kHz WAV that the shack's own reader reads back as the voice
/// that went in, and the keyer is given the shack's answer. While it records, a press is refused.
/// A cancel sends nothing, and a save with nothing recorded says so in the station's own words.
/// CONTROL: nothing reached the shack before the save, and this computer's own handler (the
/// station's recorder) was never asked.
#[test]
fn a_voice_message_recorded_here_goes_to_the_shacks_slot_as_an_import() {
    use crate::lan_client::audio_road::StandIn;
    let dir = scratch("record");
    let shack = FakeShack::new();
    let kept = json!([{ "slot": 2, "label": "CQ" }]);
    shack.answers(
        "import_voice_message",
        Answer::Value {
            value: kept.clone(),
            revision: None,
        },
    );
    let client = client_in(&dir, &shack);
    let mic = StandIn::new();
    client.record_from(mic.clone());
    let s = screens(&client);
    let tone = |block: usize| {
        (0..960).map(move |i| {
            let t = (block * 960 + i) as f32 / 48_000.0;
            0.3 * (2.0 * std::f32::consts::PI * 800.0 * t).sin()
        })
    };
    assert!(ask(&s.main, "start_voice_recording", json!({})).is_ok());
    for block in 0..50 {
        mic.rings.captured(tone(block));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        ask(&s.main, "set_ptt", json!({"on": true})),
        refused("recording")
    );
    assert!(shack.sent().is_empty(), "{:?}", shack.sent());
    assert_eq!(
        ask(
            &s.main,
            "stop_voice_recording",
            json!({"slot": 2, "label": "CQ"})
        ),
        Ok(kept)
    );
    let (cmd, args) = {
        let sent = lock(&shack.sent);
        assert_eq!(sent.len(), 1, "one import");
        (sent[0].0.clone(), sent[0].1.clone())
    };
    assert_eq!(cmd, "import_voice_message");
    assert_eq!(
        (args["slot"].clone(), args["label"].clone()),
        (json!(2), json!("CQ"))
    );
    let bytes: Vec<u8> = serde_json::from_value(args["bytes"].clone()).unwrap();
    let file = dir.join("sent.wav");
    std::fs::write(&file, &bytes).unwrap();
    let voice = tempo_audio::voice::read_wav_12k(&file).unwrap();
    assert!(
        (10_000..=13_200).contains(&voice.len()),
        "about the second recorded, at 12 kHz: {}",
        voice.len()
    );
    let rms = (voice.iter().map(|x| x * x).sum::<f32>() / voice.len() as f32).sqrt();
    assert!(rms > 0.15, "the voice that went in: {rms}");
    assert!(ask(&s.main, "start_voice_recording", json!({})).is_ok());
    mic.rings.captured(tone(0));
    std::thread::sleep(Duration::from_millis(50));
    assert!(ask(&s.main, "cancel_voice_recording", json!({})).is_ok());
    assert_eq!(
        ask(
            &s.main,
            "stop_voice_recording",
            json!({"slot": 3, "label": ""})
        ),
        Err(json!("Nothing was recorded — check your input device"))
    );
    assert_eq!(
        shack.sent(),
        ["import_voice_message"],
        "a cancel sends nothing"
    );
    assert!(s.ran_here().is_empty(), "{:?}", s.ran_here());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ The volume as kept or asked for is whole dB inside the hosted page's 0 to 42, and anything
/// unreadable is its default 24, never silence or a blast. CONTROL: a volume inside the range is
/// kept as it is.
#[test]
fn a_volume_is_whole_db_inside_the_pages_range() {
    assert_eq!(volume(Some(30.0)), 30.0);
    assert_eq!(volume(Some(23.6)), 24.0);
    assert_eq!(volume(Some(-3.0)), 0.0);
    assert_eq!(volume(Some(60.0)), 42.0);
    assert_eq!(volume(Some(f64::NAN)), 24.0);
    assert_eq!(volume(None), 24.0);
}

/// ★ A save goes with the revision of the read its copy was made from, when the screen names it:
/// a settings read takes its revision to the screen beside the settings it read, a later read in
/// the same window does not move what a save made from the earlier copy goes with, and the `base`
/// the screen named goes no further than here. A ui-state read carries no revision to the screen.
/// CONTROL: a save that names no read goes with the window's last read, as before.
#[test]
fn a_save_goes_with_the_revision_of_the_read_its_copy_was_made_from() {
    let dir = scratch("read-revisions");
    let shack = FakeShack::new();
    let client = client_in(&dir, &shack);
    let s = screens(&client);
    let answer = |revision: &str| Answer::Value {
        value: json!({ "mycall": "KD9TAW" }),
        revision: Some(revision.to_string()),
    };
    shack.answers("get_settings", answer("rFirst"));
    let first = ask(&s.main, "get_settings", json!({})).unwrap();
    assert_eq!(first[READ_REVISION], "rFirst");
    assert_eq!(first["mycall"], "KD9TAW");
    shack.answers("get_settings", answer("rLater"));
    ask(&s.main, "get_settings", json!({})).unwrap();
    shack.answers("set_settings", answer("rSaved"));
    ask(
        &s.main,
        "set_settings",
        json!({ "settings": {}, "base": "rFirst" }),
    )
    .unwrap();
    {
        let sent = lock(&shack.sent);
        let (cmd, args, base) = sent.last().unwrap();
        assert_eq!(
            (cmd.as_str(), base.as_deref()),
            ("set_settings", Some("rFirst"))
        );
        assert_eq!(*args, json!({ "settings": {} }), "the base goes no further");
    }
    shack.answers("get_settings", answer("rAgain"));
    ask(&s.main, "get_settings", json!({})).unwrap();
    ask(&s.main, "set_settings", json!({ "settings": {} })).unwrap();
    assert_eq!(
        lock(&shack.sent).last().unwrap().2.as_deref(),
        Some("rAgain"),
        "CONTROL: no read named, the window's last"
    );
    shack.answers(
        "ui_state_load",
        Answer::Value {
            value: json!({}),
            revision: Some("rUi".to_string()),
        },
    );
    let ui = ask(&s.main, "ui_state_load", json!({})).unwrap();
    assert!(ui.get(READ_REVISION).is_none(), "{ui}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ A gallery picture arrives in client mode: the gallery and the viewer pop-out ask the station
/// for it by the path its gallery names, and each window here is handed the station's bytes as the
/// station read them. Saving a piece runs on this computer and sends nothing to the station.
/// CONTROL: Reveal, which opens the station's own file manager, is still refused and never sent.
#[test]
fn a_gallery_picture_arrives_in_client_mode() {
    let dir = scratch("gallery");
    let gallery = dir.join("Nexus SSTV");
    std::fs::create_dir_all(&gallery).unwrap();
    let picture: Vec<u8> = (0..4096u32).map(|i| (i * 13) as u8).collect();
    let path = gallery.join("20261017_120000_PD120.png");
    std::fs::write(&path, &picture).unwrap();
    let at_the_shack =
        crate::shack_files::gallery_image_in(&gallery, path.to_str().unwrap()).unwrap();

    let shack = FakeShack::new();
    shack.answers(
        "shack_gallery_image",
        Answer::Value {
            value: json!(at_the_shack),
            revision: None,
        },
    );
    let client = client_in(&dir, &shack);
    let s = screens(&client);
    let args = json!({ "path": path.to_str().unwrap() });
    for window in [&s.main, &s.popout] {
        let got = ask(window, "shack_gallery_image", args.clone()).expect("the picture");
        let got = crate::b64_decode(got.as_str().expect("base64 text")).unwrap();
        assert!(got == picture, "the station's bytes, as it read them");
    }
    assert_eq!(
        shack.sent(),
        vec!["shack_gallery_image", "shack_gallery_image"]
    );
    assert_eq!(lock(&shack.sent)[0].1, args);

    let save = json!({ "filename": "a.png", "base64": "AA==", "offset": 0 });
    assert_eq!(ask(&s.main, "shack_file_save", save), Ok(json!("ran here")));
    assert_eq!(s.ran_here(), vec!["shack_file_save"]);
    assert_eq!(
        ask(&s.main, "reveal_sstv_gallery", json!({})),
        refused("shackOnly")
    );
    assert_eq!(shack.sent().len(), 2, "nothing more crossed");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ Operate's launch signs in again for a station added with the account in the last week before
/// the station stops taking this computer on its network, and not before; never for a station
/// paired with a code. The banner's status names the day while it is still to be done, and says
/// while the sign-in waits in the browser. CONTROL: one day earlier, nothing is due.
#[test]
fn the_launch_signs_in_again_in_the_last_week_before_the_station_stops_taking_this_computer() {
    use crate::lan_client::tests::{record, StationStore};
    use tempo_client::account::{PROOF_MAX_AGE_S, RENEW_BEFORE_S};
    let dir = scratch("sign-in-again");
    let stations = crate::lan_client::Stations::new(Arc::new(StationStore::default()))
        .signed_in(dir.join("lan-account.json"));
    let coded = "60000000-0000-4000-8000-0000000000c0";
    stations
        .keep(&record(STATION, &["192.168.1.20:42075"]))
        .unwrap();
    stations
        .keep(&record(coded, &["192.168.1.21:42075"]))
        .unwrap();
    let issued_at = 1_800_000_000;
    stations.keep_signed(STATION, issued_at).unwrap();
    let until = issued_at + PROOF_MAX_AGE_S;
    let start = until - RENEW_BEFORE_S;
    assert_eq!(sign_in_again_by(&stations, STATION, start - 86_400), None);
    assert_eq!(sign_in_again_by(&stations, STATION, start - 1), None);
    assert_eq!(sign_in_again_by(&stations, STATION, start), Some(until));
    assert_eq!(sign_in_again_by(&stations, STATION, until + 1), Some(until));
    assert_eq!(
        sign_in_again_by(&stations, coded, until + 1),
        None,
        "paired with a code"
    );

    let client = client_in(&dir, &FakeShack::new());
    assert_eq!(
        (client.status().sign_in_by, client.status().signing_again),
        (None, false)
    );
    client.signing_again(Some(until), true);
    let status = serde_json::to_value(client.status()).unwrap();
    assert_eq!(
        (&status["signInBy"], &status["signingAgain"]),
        (&json!(until), &json!(true))
    );
    client.signing_again(None, false);
    assert_eq!(
        (client.status().sign_in_by, client.status().signing_again),
        (None, false)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ The banner's status carries the station's `radio.txHold` beside `pttRefusal`, as the station
/// sent it, in any section: with its seconds left, without them, and absent when the station says
/// none (or is a build that does not say).
#[test]
fn the_status_carries_the_stations_transmit_hold() {
    let dir = scratch("tx-hold");
    let shack = FakeShack::new();
    let client = client_in(&dir, &shack);
    let hold = |client: &Client| serde_json::to_value(client.status()).unwrap()["txHold"].clone();
    shack.feeds("snapshot/1", 1, json!({ "radio": {} }));
    assert_eq!(hold(&client), Value::Null, "no hold said");
    shack.feeds(
        "snapshot/1",
        2,
        json!({ "radio": { "txHold": { "ownerNotified": true, "leftSecs": 540 } } }),
    );
    assert_eq!(
        hold(&client),
        json!({ "ownerNotified": true, "leftSecs": 540 })
    );
    shack.feeds(
        "snapshot/1",
        3,
        json!({ "radio": { "txHold": { "ownerNotified": false } } }),
    );
    assert_eq!(hold(&client), json!({ "ownerNotified": false }));
    std::fs::remove_dir_all(&dir).unwrap();
}
