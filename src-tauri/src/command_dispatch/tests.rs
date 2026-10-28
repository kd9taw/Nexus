//! The dispatcher's rules, each shown refusing what it refuses and admitting what it admits, and
//! the guards that hold its tables to the commands, the screen and the stores they describe.

use super::*;
use crate::command_classes::registered;
use serde_json::json;
use tempo_client::classes::CLASSES;

// --- Scaffolding ---------------------------------------------------------------------------------

/// Stores whose revisions and `ui-state.json` a test sets, and a file that can stop reading: at
/// once, or after a number of reads.
#[derive(Default)]
struct Fake {
    settings: Mutex<Option<Revision>>,
    ui: Mutex<BTreeMap<String, String>>,
    ui_unreadable: Mutex<bool>,
    ui_reads_left: Mutex<Option<usize>>,
}

impl Stores for Fake {
    fn revision(&self, store: Store) -> Option<Revision> {
        match store {
            Store::Settings => lock(&self.settings).clone(),
            Store::UiState => Revision::of(&split_ui_state(self.ui_state()?).0),
        }
    }

    fn ui_state(&self) -> Option<BTreeMap<String, String>> {
        let mut left = lock(&self.ui_reads_left);
        if let Some(reads) = left.as_mut() {
            if *reads == 0 {
                return None;
            }
            *reads -= 1;
        }
        (!*lock(&self.ui_unreadable)).then(|| lock(&self.ui).clone())
    }
}

fn rev(text: &str) -> Revision {
    Revision::new(text.to_string())
}

/// A shack whose settings stand at revision `r0`.
fn shack() -> Dispatcher<Fake> {
    let shack = Dispatcher::new(Fake::default());
    settings_now(&shack, Some("r0"));
    shack
}

fn settings_now(shack: &Dispatcher<Fake>, revision: Option<&str>) {
    *lock(&shack.inner.stores.settings) = revision.map(rev);
}

fn ui_now(shack: &Dispatcher<Fake>, pairs: &[(&str, &str)]) {
    *lock(&shack.inner.stores.ui) = map(pairs);
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn caller(run: u64, holds_lease: bool) -> Caller {
    Caller {
        key: CallerKey {
            device: "the operator's laptop".to_string(),
            run,
        },
        profile: Profile::Desktop,
        holds_lease,
        roads: Ok(()),
    }
}

/// The computer holding the lease, its roads saying `roads` of a start.
fn controller_whose_roads(roads: Result<(), Refusal>) -> Caller {
    Caller {
        roads,
        ..controller()
    }
}

fn controller() -> Caller {
    caller(1, true)
}

/// A paired computer that does not hold the lease.
fn other_device() -> Caller {
    caller(1, false)
}

fn call(id: u64, cmd: &str, args: Value) -> Call {
    Call {
        id,
        cmd: cmd.to_string(),
        args,
        base: None,
        stops: 0,
    }
}

fn save(id: u64, cmd: &str, args: Value, base: &str) -> Call {
    Call {
        base: Some(rev(base)),
        ..call(id, cmd, args)
    }
}

/// The admission in a word, for assertions that read plainly.
fn kind<S: Stores>(admission: &Admission<S>) -> String {
    match admission {
        Admission::Stop { .. } => "stop".to_string(),
        Admission::Run { .. } => "run".to_string(),
        Admission::Answered(outcome) => format!("answered {outcome:?}"),
        Admission::Refused(refusal) => format!("refused {}", refusal.code()),
    }
}

fn refused(refusal: Refusal) -> String {
    format!("refused {}", refusal.code())
}

/// The commands of one class, from the handler list.
fn of_class(class: Class) -> Vec<&'static str> {
    registered()
        .into_iter()
        .filter(|name| class_of(name) == Some(class))
        .collect()
}

/// Arguments that read as the on half of `cmd`, when it is a transmit switch; `{}` otherwise.
fn on_args(cmd: &str) -> Value {
    // `js8_arm` names its switch.
    if cmd == "js8_arm" {
        return json!({ "which": "autoreply", "on": true });
    }
    match OFF_HALVES.iter().find(|(name, ..)| *name == cmd) {
        Some((_, arg, Off::False)) => json!({ *arg: true }),
        Some((_, arg, Off::OneOf(_))) => json!({ *arg: "qso-run" }),
        None => json!({}),
    }
}

/// Every argument that reads as the off half of `cmd`.
fn off_args(cmd: &str) -> Vec<Value> {
    match OFF_HALVES.iter().find(|(name, ..)| *name == cmd) {
        Some((_, arg, Off::False)) => vec![json!({ *arg: false })],
        Some((_, arg, Off::OneOf(values))) => values.iter().map(|v| json!({ *arg: v })).collect(),
        None => panic!("{cmd} has no off half"),
    }
}

/// Fills every slot one computer has: [`IN_FLIGHT`] calls held running, from ids 1000 up.
fn fill_the_slots(shack: &Dispatcher<Fake>, who: &Caller) -> Vec<Admission<Fake>> {
    let held: Vec<_> = (0..IN_FLIGHT as u64)
        .map(|n| {
            let cmd = if n % 2 == 0 {
                "get_snapshot"
            } else {
                "get_band_plan"
            };
            shack.admit(who, call(1000 + n, cmd, json!({})))
        })
        .collect();
    assert!(held.iter().all(|a| kind(a) == "run"), "the slots filled");
    held
}

// --- Deny by default -----------------------------------------------------------------------------

#[test]
fn an_unclassed_command_is_refused_at_both_ends() {
    let shack = shack();
    for (id, name) in ["throwaway_unclassed_probe", "plugin:dialog|open", ""]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            route(name, &json!({})),
            Route::Refused(Refusal::Unclassed),
            "{name:?} here"
        );
        let admission = shack.admit(&controller(), call(id as u64 + 1, name, json!({})));
        assert_eq!(
            kind(&admission),
            refused(Refusal::Unclassed),
            "{name:?} at the shack"
        );
    }
}

#[test]
fn a_command_that_runs_here_runs_here_and_never_at_the_shack() {
    let shack = shack();
    let here = of_class(Class::RunsHere);
    assert!(here.len() >= 30, "the table's own count: {}", here.len());
    for (id, name) in here.into_iter().enumerate() {
        assert_eq!(route(name, &json!({})), Route::Here, "{name}");
        let admission = shack.admit(&controller(), call(id as u64 + 1, name, json!({})));
        assert_eq!(kind(&admission), refused(Refusal::RunsHere), "{name}");
    }
}

/// The Redo of one change's undo is no riskier than the undo a computer driving the station may
/// already ask for, so it runs at the shack from afar, as there; it refuses a run over more than
/// one contact itself (`undo_contact_run`), which stays `undo_history_run`'s, at the shack.
#[test]
fn one_contacts_redo_runs_from_afar_and_a_whole_runs_undo_does_not() {
    let shack = shack();
    for (id, name) in ["undo_qso_change", "redo_qso_change"]
        .into_iter()
        .enumerate()
    {
        assert_eq!(class_of(name), Some(Class::RunsAtShack), "{name}");
        let args = json!({ "run": "r" });
        assert_eq!(route(name, &args), Route::Shack, "{name} here");
        let admission = shack.admit(&controller(), call(id as u64 + 1, name, args));
        assert_eq!(kind(&admission), "run", "{name} at the shack");
    }
    let args = json!({ "run": "r" });
    assert_eq!(
        route("undo_history_run", &args),
        Route::Refused(Refusal::ShackOnly)
    );
    let admission = shack.admit(&controller(), call(9, "undo_history_run", args));
    assert_eq!(kind(&admission), refused(Refusal::ShackOnly));
}

#[test]
fn a_shack_only_command_is_refused_at_both_ends() {
    // The operator's rulings of 2026-10-07 by name: a settings restore and a factory reset, a
    // whole-log purge and the Connections log stay at the shack. The clock's repair waits on an
    // administrator prompt on the shack's own screen. A whole run's undo can delete as much of the
    // log as a purge (an import's undo), so it stays at the shack with it.
    for name in [
        "import_settings_bundle",
        "reset_settings",
        "purge_log",
        "undo_history_run",
        "get_connection_log",
        "repair_clock",
    ] {
        assert_eq!(class_of(name), Some(Class::RefusedHere), "{name}");
    }
    let shack = shack();
    for (id, name) in of_class(Class::RefusedHere).into_iter().enumerate() {
        assert_eq!(
            route(name, &json!({})),
            Route::Refused(Refusal::ShackOnly),
            "{name} here"
        );
        let admission = shack.admit(&controller(), call(id as u64 + 1, name, json!({})));
        assert_eq!(
            kind(&admission),
            refused(Refusal::ShackOnly),
            "{name} at the shack"
        );
    }
}

/// The desktop's FT buttons that start a transmission, as the operator signed them for a computer
/// driving the shack: Enable TX, CQ and directed CQ, a reply to a decode, Tx5 and the next-message
/// choice, resend, the Operate strip's Call CQ and contest S&P, and a run mode entered (a CQ run,
/// or Field Day's Running). Written here, not read from the table, so a change to the table cannot
/// move them.
const FT_STARTS: [&str; 10] = [
    "call_cq",
    "call_station",
    "override_next_tx",
    "qso_freetext",
    "qso_resend",
    "set_mode",
    "set_tx_enabled",
    "start_cq",
    "start_running",
    "start_search_and_pounce",
];

/// The JS8 cockpit's starts, allowed from afar as at the shack: a message, a directed command, CQ,
/// a heartbeat now, more text for the message going out, a `js8_arm` switch turned on (the
/// automatic replies, the heartbeat, relay, heartbeat acks, heartbeat networking), the repeating
/// CQ, and the yes to an automatic reply. Written here, as [`FT_STARTS`] is.
const JS8_STARTS: [&str; 8] = [
    "js8_answer_reply",
    "js8_arm",
    "js8_call_cq",
    "js8_cq_repeat",
    "js8_heartbeat_now",
    "js8_send",
    "js8_send_command",
    "js8_typeahead",
];

/// Keyboard CW and the CW macros (an F-key, or Enter Sends Message), which the shack's keyer
/// times. Written here, as [`FT_STARTS`] is.
const CW_STARTS: [&str; 2] = ["send_cw", "send_cw_armed"];

/// The tune carrier and the rig's own tuner. Written here, as [`FT_STARTS`] is.
const TUNE_STARTS: [&str; 2] = ["atu_tune", "set_tune"];

/// The RTTY cockpit's starts: a send, continuous TX switched on and typed into, and the
/// auto-sequencer's CQ and answer. Written here, as [`FT_STARTS`] is.
const RTTY_STARTS: [&str; 5] = [
    "rtty_auto_answer",
    "rtty_auto_cq",
    "rtty_send",
    "rtty_set_latched",
    "rtty_type",
];

/// The PSK cockpit's starts: a send, and continuous TX switched on and typed into. Written here, as
/// [`FT_STARTS`] is.
const PSK_STARTS: [&str; 3] = ["psk_send", "psk_set_latched", "psk_type"];

/// The JTTY cockpit's starts: a send, an F-key, live entry started and typed into, and the
/// sign-off. Written here, as [`FT_STARTS`] is.
const JTTY_STARTS: [&str; 5] = [
    "jtty_fkey",
    "jtty_live_commit",
    "jtty_live_start",
    "jtty_send",
    "jtty_sign_off",
];

/// APRS's starts, as at the shack: Monitor armed by hand, which lets the automatic ack key, a
/// beacon, and a message. Written here, as [`FT_STARTS`] is.
const APRS_STARTS: [&str; 3] = ["aprs_arm", "aprs_send_beacon", "aprs_send_message"];

/// An SSTV picture. Written here, as [`FT_STARTS`] is.
const SSTV_STARTS: [&str; 1] = ["sstv_send"];

/// Chat's starts, Nexus to Nexus: a message, a broadcast, a message sent again, the CQ run turned
/// on or resumed, and the presence beacon turned on. Written here, as [`FT_STARTS`] is.
const CHAT_STARTS: [&str; 6] = [
    "broadcast",
    "resend_chat",
    "resume_chat_cq",
    "send_message",
    "set_beacon",
    "set_chat_cq",
];

/// Every start that runs as a command from afar: [`FT_STARTS`], [`JS8_STARTS`], [`CW_STARTS`],
/// [`TUNE_STARTS`], [`RTTY_STARTS`], [`PSK_STARTS`], [`JTTY_STARTS`], [`APRS_STARTS`],
/// [`SSTV_STARTS`], [`CHAT_STARTS`] and the voice keyer's play.
fn starts_run_from_afar() -> Vec<&'static str> {
    FT_STARTS
        .into_iter()
        .chain(JS8_STARTS)
        .chain(CW_STARTS)
        .chain(TUNE_STARTS)
        .chain(RTTY_STARTS)
        .chain(PSK_STARTS)
        .chain(JTTY_STARTS)
        .chain(APRS_STARTS)
        .chain(SSTV_STARTS)
        .chain(CHAT_STARTS)
        .chain(["play_voice_message"])
        .collect()
}

/// The rest of those FT buttons: Halt, Clear DX, working a spot, Log QSO and the prompt-to-log answer, and
/// the operator's settings (the TX and RX offsets, Hold TX freq, even or odd, skip Tx1, auto cycle,
/// mode, band, frequency and decode depth). None keys anything.
const FT_KEYING_NOTHING: [&str; 15] = [
    "clear_dx_call",
    "confirm_pending_log",
    "halt_tx",
    "log_current_qso",
    "pick_band",
    "set_decode_depth",
    "set_frequency",
    "set_hold_tx_freq",
    "set_rx_offset",
    "set_skip_tx1",
    "set_tier",
    "set_tx_cycle_auto",
    "set_tx_even",
    "set_tx_offset",
    "work_spot",
];

/// ★ The starts wired from afar are exactly the FT buttons' starts, the JS8 sends, keyboard CW and
/// its macros, the tune carrier and the rig's tuner, the RTTY, PSK and JTTY starts, the voice keyer's
/// play, and the PTT as a held press; the table says so by value.
/// CONTROL: the table holds no row that is not a start.
#[test]
fn the_starts_wired_from_afar_are_the_ft_buttons_the_voice_keyer_and_the_held_press() {
    let mut wired: Vec<(&str, Wired)> = WIRED.to_vec();
    wired.sort_unstable_by_key(|(name, _)| *name);
    let mut expected: Vec<(&str, Wired)> = FT_STARTS.iter().map(|n| (*n, Wired::Ft)).collect();
    expected.extend(JS8_STARTS.iter().map(|n| (*n, Wired::Js8)));
    expected.extend(CW_STARTS.iter().map(|n| (*n, Wired::Cw)));
    expected.extend(TUNE_STARTS.iter().map(|n| (*n, Wired::Tune)));
    expected.extend(RTTY_STARTS.iter().map(|n| (*n, Wired::Rtty)));
    expected.extend(PSK_STARTS.iter().map(|n| (*n, Wired::Psk)));
    expected.extend(JTTY_STARTS.iter().map(|n| (*n, Wired::Jtty)));
    expected.extend(APRS_STARTS.iter().map(|n| (*n, Wired::Aprs)));
    expected.extend(SSTV_STARTS.iter().map(|n| (*n, Wired::Sstv)));
    expected.extend(CHAT_STARTS.iter().map(|n| (*n, Wired::Chat)));
    expected.push(("play_voice_message", Wired::VoiceKeyer));
    expected.push(("set_ptt", Wired::HeldPress));
    expected.push(("hold_ptt_lock", Wired::LockedPress));
    expected.sort_unstable_by_key(|(name, _)| *name);
    assert_eq!(wired, expected);
    assert!(
        WIRED.windows(2).all(|pair| pair[0].0 < pair[1].0),
        "name order"
    );
    for (name, _) in WIRED {
        assert_eq!(class_of(name), Some(Class::StartsTx), "{name}");
    }
}

/// ★ The FT buttons are admitted from the computer that holds the lease with its roads up, and
/// every other start is refused by name: the held press is never `set_ptt` from afar, and each start
/// not wired is `transmitNotYet`, at the shack and on the other computer alike. CONTROL: the FT
/// buttons that key nothing run, or stop, by their class.
#[test]
fn the_ft_buttons_are_admitted_and_every_other_start_is_refused_by_name() {
    let shack = shack();
    let starts = of_class(Class::StartsTx);
    assert!(
        starts.len() >= 30,
        "the table's own count: {}",
        starts.len()
    );
    for (id, name) in starts.into_iter().enumerate() {
        // The other computer's routing sends a start on, and its own refusal reads the same switch.
        assert_eq!(route(name, &on_args(name)), Route::Shack, "{name}");
        let admission = shack.admit(&controller(), call(id as u64 + 1, name, on_args(name)));
        let expected = match name {
            _ if starts_run_from_afar().contains(&name) => "run".to_string(),
            "set_ptt" | "hold_ptt_lock" => refused(Refusal::HeldPress),
            _ => refused(Refusal::TransmitNotYet),
        };
        assert_eq!(kind(&admission), expected, "{name}");
        assert_eq!(
            transmit_from_afar(name).err(),
            (expected == refused(Refusal::TransmitNotYet)).then_some(Refusal::TransmitNotYet),
            "{name}"
        );
    }
    for (id, name) in FT_KEYING_NOTHING.into_iter().enumerate() {
        let expected = match class_of(name) {
            Some(Class::RunsAtShack) => "run",
            Some(Class::StopsTx) => "stop",
            other => panic!("{name} is {other:?}: an FT button that keys nothing"),
        };
        let admission = shack.admit(&controller(), call(id as u64 + 500, name, json!({})));
        assert_eq!(kind(&admission), expected, "{name}");
    }
}

/// "Still activating" from afar goes to the shack as Stop does: the activation is the shack's,
/// taken up there paused at launch, and the operator may be at the park driving it from a laptop.
/// It keys nothing, so it is never a start.
#[test]
fn still_activating_from_afar_reaches_the_shack_as_stop_does() {
    let shack = shack();
    assert_eq!(class_of("clear_activation"), Some(Class::RunsAtShack));
    assert_eq!(class_of("resume_activation"), class_of("clear_activation"));
    for (id, name) in [(1, "clear_activation"), (2, "resume_activation")] {
        assert_eq!(route(name, &json!({})), Route::Shack, "{name}");
        match shack.admit(&controller(), call(id, name, json!({}))) {
            Admission::Run { .. } => {}
            other => panic!("{name}: {}", kind(&other)),
        }
    }
}

/// Skip Tx1 from afar is the station's own setting: the other computer sends the change on to the
/// shack, and the shack runs it with the value the operator picked, on and off alike, so the box
/// the client draws from the station's snapshot follows the click. It keys nothing, so it is never a
/// start. CONTROL: a refusal (no lease) is said by name, not dropped.
#[test]
fn skip_tx1_from_afar_reaches_the_stations_setting_with_its_value() {
    let shack = shack();
    assert_eq!(class_of("set_skip_tx1"), Some(Class::RunsAtShack));
    for (id, enabled) in [(1, true), (2, false)] {
        let args = json!({ "enabled": enabled });
        assert_eq!(route("set_skip_tx1", &args), Route::Shack, "{enabled}");
        match shack.admit(&controller(), call(id, "set_skip_tx1", args.clone())) {
            Admission::Run { args: ran, .. } => assert_eq!(ran, args, "the value is the click's"),
            other => panic!("{enabled}: {}", kind(&other)),
        }
    }
    let watcher = caller(2, false);
    assert!(
        kind(&shack.admit(
            &watcher,
            call(3, "set_skip_tx1", json!({ "enabled": true }))
        ))
        .starts_with("refused"),
        "a computer without the lease is refused"
    );
}

/// ⛔ From afar `js8_arm` turns on every switch the shack's own screen does: the automatic
/// replies, the heartbeat, relay, heartbeat acks and heartbeat networking each pass the other
/// computer's routing and the shack's admission, and run at the shack. Their off halves, every
/// switch, go as a Stop. A `which` that names no switch is the shack handler's own to refuse, as
/// from its own screen (`a_js8_switch_from_afar_turns_on_only_under_the_stand_down_it_was_read_under`).
/// CONTROL: `js8_arm` is still a start, and a start's own refusals still hold
/// (`a_wired_start_needs_the_lease_and_its_roads`).
#[test]
fn js8_arm_from_afar_turns_on_every_switch_the_shack_does() {
    let shack = shack();
    let mut id = 0;
    let mut admit = |args: Value| {
        id += 1;
        kind(&shack.admit(&controller(), call(id, "js8_arm", args)))
    };
    for which in ["autoreply", "hb", "relay", "hback", "hbnet"] {
        let on = json!({ "which": which, "on": true });
        assert_eq!(route("js8_arm", &on), Route::Shack, "{which} on");
        assert!(!is_stop("js8_arm", &on), "{which} on");
        assert_eq!(admit(on), "run", "{which} on");
        let off = json!({ "which": which, "on": false });
        assert_eq!(route("js8_arm", &off), Route::Shack, "{which} off");
        assert!(is_stop("js8_arm", &off), "{which} off");
        assert_eq!(admit(off), "stop", "{which} off");
    }
    assert_eq!(class_of("js8_arm"), Some(Class::StartsTx), "CONTROL");
}

/// ★ A wired start needs the lease on its connection and then its computer's roads: the second
/// computer, without the lease, is refused `notController` whatever its roads say; the lease holder
/// is refused with no Stop road, with no live presence, and with stale state, each by its own code.
/// CONTROL: the lease and the roads both up, it runs; and a Stop and the PTT's off half from a
/// computer whose roads are down still go.
#[test]
fn a_wired_start_needs_the_lease_and_its_roads() {
    let shack = shack();
    let mut id = 0;
    for name in starts_run_from_afar() {
        // A station of its own for each start: six asks for each of every start wired would
        // otherwise run into the computer's command rate, which is not what this test asks.
        let shack = self::shack();
        let mut admit = |who: &Caller| {
            id += 1;
            kind(&shack.admit(who, call(id, name, on_args(name))))
        };
        assert_eq!(
            admit(&other_device()),
            refused(Refusal::NotController),
            "{name}"
        );
        assert_eq!(
            admit(&Caller {
                roads: Err(Refusal::NoPresence),
                ..other_device()
            }),
            refused(Refusal::NotController),
            "{name}: the lease first"
        );
        for refusal in [
            Refusal::NoStopRoad,
            Refusal::NoPresence,
            Refusal::StaleState,
        ] {
            assert_eq!(
                admit(&controller_whose_roads(Err(refusal))),
                refused(refusal),
                "{name}"
            );
        }
        assert_eq!(admit(&controller()), "run", "{name}");
    }
    for (n, (name, args)) in [("halt_tx", json!({})), ("set_ptt", json!({"on": false}))]
        .into_iter()
        .enumerate()
    {
        let admission = shack.admit(
            &Caller {
                roads: Err(Refusal::NoStopRoad),
                ..other_device()
            },
            call(900 + n as u64, name, args),
        );
        assert_eq!(kind(&admission), "stop", "{name}");
    }
}

/// Whether a command's parameter reads as a time: one a clock or a slot could ride in.
fn time_like(name: &str, ty: &str) -> bool {
    const WORDS: [&str; 22] = [
        "time",
        "timestamp",
        "clock",
        "utc",
        "at",
        "ms",
        "millis",
        "micros",
        "nanos",
        "secs",
        "seconds",
        "slot",
        "period",
        "deadline",
        "epoch",
        "when",
        "until",
        "since",
        "delay",
        "fires",
        "boundary",
        "instant",
    ];
    const TYPES: [&str; 6] = [
        "Instant",
        "SystemTime",
        "Duration",
        "DateTime",
        "NaiveDateTime",
        "chrono::",
    ];
    let mut words = Vec::new();
    let mut word = String::new();
    for c in name.chars() {
        if (c.is_ascii_uppercase() || c == '_') && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        if c != '_' {
            word.push(c.to_ascii_lowercase());
        }
    }
    words.push(word);
    words.iter().any(|w| WORDS.contains(&w.as_str())) || TYPES.iter().any(|t| ty.contains(t))
}

/// ★ No FT command from afar carries a time, nor any other start run from afar (the JS8 starts,
/// keyboard CW and its macros, the tune carrier and the rig's tuner, the RTTY and PSK starts): every parameter of each one's command, read off its own signature
/// in `lib.rs`, names no time and has no time type, so slot timing, JS8's periods, the keyer's
/// timing, the clock and the sequencer's timeouts stay at the shack. Two are allowed, by name and
/// by type, as neither decides when anything transmits: the prompt-to-log answer's record is the
/// contact as the station proposed it, its logged times among its fields, what is written in the
/// log; and the yes to a JS8 automatic reply names that reply by the shack's own time for it, a
/// yes to any other refused. A new parameter that reads as a time fails this. CONTROL: the reader finds the times it must
/// (JS8's reply's `firesAtMs`, and made-up names and types), and passes plain ones.
#[test]
fn no_ft_command_from_afar_carries_a_time() {
    for (name, ty) in [
        ("firesAtMs", "u64"),
        ("slot", "u32"),
        ("txAt", "u64"),
        ("deadline", "Instant"),
        ("whenever", "Duration"),
        ("startUtc", "String"),
        ("periodIndex", "i64"),
    ] {
        assert!(time_like(name, ty), "the guard misses {name}: {ty}");
    }
    assert!(
        parameters("js8_answer_reply")
            .iter()
            .any(|(name, ty)| time_like(name, ty)),
        "a time in a real signature"
    );
    for (name, ty) in [("call", "String"), ("enabled", "bool"), ("hz", "f32")] {
        assert!(!time_like(name, ty), "{name}: {ty}");
    }
    // The voice keyer's play names its message by `slot`, a keyer slot, not a time: it plays as
    // soon as the shack takes it.
    for cmd in FT_STARTS
        .into_iter()
        .chain(JS8_STARTS)
        .chain(CW_STARTS)
        .chain(TUNE_STARTS)
        .chain(RTTY_STARTS)
        .chain(PSK_STARTS)
        .chain(JTTY_STARTS)
        .chain(APRS_STARTS)
        .chain(SSTV_STARTS)
        .chain(CHAT_STARTS)
        .chain(FT_KEYING_NOTHING)
    {
        for (name, ty) in parameters(cmd) {
            if cmd == "confirm_pending_log" && name == "record" {
                assert_eq!(ty, "LoggedQso", "the record is the logged contact");
                continue;
            }
            // The automatic reply the shack proposed, named by the shack's own time for it, as
            // the screen was shown it: a Yes to any other is refused, and the reply goes in the
            // shack's queue for its next period. It moves nothing.
            if cmd == "js8_answer_reply" && name == "firesAtMs" {
                assert_eq!(ty, "u64", "the reply's name, as the shack gave it");
                continue;
            }
            assert!(
                !time_like(&name, &ty),
                "{cmd} carries a time from afar: {name}: {ty}"
            );
        }
    }
}

/// ⛔ A settings save from afar turns on nothing that transmits by itself and only the settings
/// form writes: JS8's APRS-IS relay and the WSPR/FST4W beacon's share of intervals keep the
/// shack's values, both ways. CONTROL: the same save from the shack's own screen takes them, and a
/// field beside them is taken from afar too.
#[test]
fn a_settings_save_from_afar_keeps_the_shacks_transmit_settings() {
    let shack = tempo_app::settings::Settings::default();
    assert!(
        !shack.js8_aprs_relay && shack.beacon_tx_percent == 0,
        "premise: off"
    );
    let mut asked = shack.clone();
    asked.js8_aprs_relay = true;
    asked.beacon_tx_percent = 20;
    asked.cw_wpm = 31;
    let afar = Afar {
        stand_down: 3,
        starts_voided: 0,
    };
    let mut save = asked.clone();
    keep_the_shacks_transmit_settings(&shack, &mut save, Some(&afar));
    assert!(
        !save.js8_aprs_relay,
        "from afar, the APRS-IS relay turned on"
    );
    assert_eq!(save.beacon_tx_percent, 0, "from afar, the beacon turned on");
    assert_eq!(save.cw_wpm, 31, "CONTROL: the rest of the save");
    // …and off: a save from afar does not turn the shack's own off either.
    let mut save = shack.clone();
    keep_the_shacks_transmit_settings(&asked, &mut save, Some(&afar));
    assert!(
        save.js8_aprs_relay && save.beacon_tx_percent == 20,
        "turned off from afar"
    );
    let mut save = asked.clone();
    keep_the_shacks_transmit_settings(&shack, &mut save, None);
    assert!(
        save.js8_aprs_relay && save.beacon_tx_percent == 20,
        "CONTROL: the shack's own form"
    );
    assert!(
        takes_afar("set_settings"),
        "the lane says where it came from"
    );
}

/// One field of the settings, set off its default: a change a save could carry.
type Change = (&'static str, fn(&mut tempo_app::settings::Settings));

/// ⛔ What lets the station transmit with no press at it: a switch that transmits by itself, or
/// one that lets a program at the shack, or a browser, start a transmission there.
const TRANSMITS_BY_ITSELF: &[Change] = &[
    ("js8_aprs_relay", |s| s.js8_aprs_relay = true),
    ("beacon_tx_percent", |s| s.beacon_tx_percent = 20),
    ("cq_pause_secs", |s| s.cq_pause_secs = Some(5)),
    ("wsjtx_udp", |s| s.wsjtx_udp = !s.wsjtx_udp),
    ("wsjtx_udp_addr", |s| {
        s.wsjtx_udp_addr = "203.0.113.9:2237".into()
    }),
    ("cat_broker", |s| s.cat_broker = !s.cat_broker),
    ("cat_broker_ptt", |s| s.cat_broker_ptt = !s.cat_broker_ptt),
    ("js8_api_tcp", |s| s.js8_api_tcp = !s.js8_api_tcp),
    ("js8_api_udp", |s| s.js8_api_udp = !s.js8_api_udp),
    ("js8_api_tcp_accept", |s| {
        s.js8_api_tcp_accept = !s.js8_api_tcp_accept
    }),
    ("js8_api_udp_accept", |s| {
        s.js8_api_udp_accept = !s.js8_api_udp_accept
    }),
    ("remote_stream", |s| s.remote_stream = !s.remote_stream),
];

/// ⛔ The guards on a transmission: what stops or caps one by itself, the licence privileges, and
/// the line that keys the transmitter.
const TX_GUARDS: &[Change] = &[
    ("tx_watchdog_min", |s| s.tx_watchdog_min = 0),
    ("js8_idle_watchdog_min", |s| s.js8_idle_watchdog_min = 0),
    ("tune_timeout_secs", |s| s.tune_timeout_secs = 600),
    ("parsec_presence_stop", |s| {
        s.parsec_presence_stop = !s.parsec_presence_stop
    }),
    ("disable_tx_after_73", |s| {
        s.disable_tx_after_73 = !s.disable_tx_after_73
    }),
    ("cq_max_calls", |s| s.cq_max_calls = None),
    ("directed_max_calls", |s| s.directed_max_calls = None),
    ("chat_max_cycles", |s| s.chat_max_cycles = Some(99)),
    ("rtty_auto_repeats", |s| s.rtty_auto_repeats = 9),
    ("swr_stop_enabled", |s| {
        s.swr_stop_enabled = !s.swr_stop_enabled
    }),
    ("swr_stop_threshold", |s| s.swr_stop_threshold = 9.0),
    ("max_power_phone", |s| s.max_power_phone = Some(0.9)),
    ("max_power_cw", |s| s.max_power_cw = Some(0.9)),
    ("max_power_digital", |s| s.max_power_digital = Some(0.9)),
    ("max_power_am", |s| s.max_power_am = Some(0.9)),
    ("license_class", |s| {
        s.license_class = tempo_app::settings::LicenseClass::Extra
    }),
    ("ptt_method", |s| s.ptt_method = "rts".into()),
    ("ptt_serial_port", |s| s.ptt_serial_port = "COM9".into()),
    ("cat_rts_state", |s| s.cat_rts_state = "high".into()),
    ("cat_dtr_state", |s| s.cat_dtr_state = "high".into()),
    ("cat_rts_keys_ptt", |s| {
        s.cat_rts_keys_ptt = !s.cat_rts_keys_ptt
    }),
    ("cat_ptt_line_state", |s| {
        s.cat_ptt_line_state = "high".into()
    }),
    ("cat_serial_handshake", |s| {
        s.cat_serial_handshake = "hardware".into()
    }),
];

/// ⛔ The one setting that names a program for the shack to run.
const RUNS_A_PROGRAM: &[Change] = &[("tqsl_path", |s| {
    s.tqsl_path = r"C:\Users\Public\evil.exe".into()
})];

/// ⛔ The TCI server, the station's and set at the shack only: the server itself and its port,
/// "Programs on TCI may transmit", and the CW and the spots it takes from programs.
const THE_TCI_SERVER: &[Change] = &[
    ("tci_server", |s| s.tci_server = !s.tci_server),
    ("tci_server_port", |s| s.tci_server_port = 40002),
    ("tci_server_tx", |s| s.tci_server_tx = !s.tci_server_tx),
    ("tci_server_cw", |s| s.tci_server_cw = !s.tci_server_cw),
    ("tci_server_spots", |s| {
        s.tci_server_spots = !s.tci_server_spots
    }),
];

/// What neither starts a transmission nor guards one, among them the fields beside the guards that
/// read most like one: a save from afar takes each.
const NEITHER: &[Change] = &[
    ("cw_wpm", |s| s.cw_wpm = 31),
    ("tx_level", |s| s.tx_level = 0.25),
    ("rated_watts", |s| s.rated_watts = 10),
    ("serial_port", |s| s.serial_port = "COM7".into()),
    ("cq_stall_overs", |s| s.cq_stall_overs = Some(0)),
    ("js8_hb_interval_min", |s| s.js8_hb_interval_min = 1),
    ("js8_hb_networking", |s| {
        s.js8_hb_networking = !s.js8_hb_networking
    }),
    ("tune_power_pct", |s| s.tune_power_pct = Some(50)),
    ("band_edge_tones", |s| {
        s.band_edge_tones = !s.band_edge_tones
    }),
    ("n1mm_addr", |s| s.n1mm_addr = "127.0.0.1:12060".into()),
];

/// The settings fields, by their JSON name, whose values differ between `a` and `b`: a whole
/// save compared by value.
fn moved(a: &tempo_app::settings::Settings, b: &tempo_app::settings::Settings) -> Vec<String> {
    let (a, b) = (
        serde_json::to_value(a).unwrap(),
        serde_json::to_value(b).unwrap(),
    );
    let (a, b) = (a.as_object().unwrap(), b.as_object().unwrap());
    a.keys()
        .chain(b.keys())
        .filter(|k| a.get(*k) != b.get(*k))
        .cloned()
        .collect()
}

/// ⛔ A settings save from afar changes no setting that lets the station transmit by itself, none
/// that guards a transmission, not the program the shack runs and none of the TCI server's: for
/// each, the save from afar comes out as the shack's settings exactly, and the same save from the
/// shack's own screen takes it.
#[test]
fn a_settings_save_from_afar_keeps_every_transmit_setting_at_the_shacks_value() {
    let shack = tempo_app::settings::Settings::default();
    let afar = Afar {
        stand_down: 3,
        starts_voided: 0,
    };
    for (name, change) in TRANSMITS_BY_ITSELF
        .iter()
        .chain(TX_GUARDS)
        .chain(RUNS_A_PROGRAM)
        .chain(THE_TCI_SERVER)
    {
        let mut asked = shack.clone();
        change(&mut asked);
        assert!(!moved(&asked, &shack).is_empty(), "premise: {name} moved");
        let mut save = asked.clone();
        keep_the_shacks_transmit_settings(&shack, &mut save, Some(&afar));
        assert_eq!(
            moved(&save, &shack),
            Vec::<String>::new(),
            "from afar, {name} changed"
        );
        let mut save = asked.clone();
        keep_the_shacks_transmit_settings(&shack, &mut save, None);
        assert_eq!(
            moved(&save, &asked),
            Vec::<String>::new(),
            "CONTROL: at the shack, {name}"
        );
    }
}

/// CONTROL: a save from afar still takes what neither starts nor guards a transmission.
#[test]
fn a_settings_save_from_afar_takes_what_neither_transmits_nor_guards() {
    let shack = tempo_app::settings::Settings::default();
    let afar = Afar {
        stand_down: 3,
        starts_voided: 0,
    };
    for (name, change) in NEITHER {
        let mut asked = shack.clone();
        change(&mut asked);
        assert!(!moved(&asked, &shack).is_empty(), "premise: {name} moved");
        let mut save = asked.clone();
        keep_the_shacks_transmit_settings(&shack, &mut save, Some(&afar));
        assert_eq!(
            moved(&save, &asked),
            Vec::<String>::new(),
            "from afar, {name} was kept"
        );
    }
}

/// A shack of two radios that key differently, the live one radio 0.
fn two_radio_shack() -> tempo_app::settings::Settings {
    let mut shack = tempo_app::settings::Settings {
        radios: vec![
            tempo_app::settings::RadioProfile {
                id: 0,
                ptt_method: "cat".into(),
                ptt_serial_port: String::new(),
                ..Default::default()
            },
            tempo_app::settings::RadioProfile {
                id: 1,
                ptt_method: "dtr".into(),
                ptt_serial_port: "COM4".into(),
                ..Default::default()
            },
        ],
        active_radio: 0,
        ..Default::default()
    };
    shack.sync_flat_from_active();
    shack
}

/// ⛔ The PTT fields of a save describe the radio its form was editing, which a switch since may
/// have left not the live one: from afar they come back as the shack's own for THAT radio, never
/// the live radio's (which the save would then write into the other's profile).
#[test]
fn a_save_from_afar_keeps_the_ptt_of_the_radio_its_form_edited() {
    let shack = two_radio_shack();
    let afar = Afar {
        stand_down: 3,
        starts_voided: 0,
    };
    let mut save = shack.clone();
    save.active_radio = 1;
    save.ptt_method = "rts".into();
    save.ptt_serial_port = "COM9".into();
    keep_the_shacks_transmit_settings(&shack, &mut save, Some(&afar));
    assert_eq!(
        (save.ptt_method.as_str(), save.ptt_serial_port.as_str()),
        ("dtr", "COM4"),
        "from afar, radio 1's PTT"
    );
    // A save with no roster describes the live radio.
    let mut save = shack.clone();
    save.radios.clear();
    save.active_radio = 1;
    save.ptt_method = "rts".into();
    keep_the_shacks_transmit_settings(&shack, &mut save, Some(&afar));
    assert_eq!(
        save.ptt_method, "cat",
        "from afar, with no roster, the live radio's PTT"
    );
}

/// ⛔ The per-radio edit from afar (`update_radio_profile`) keeps that radio's PTT method and port;
/// the rest of its patch is taken, and the shack's own edit takes them.
#[test]
fn a_radio_edit_from_afar_keeps_that_radios_ptt() {
    let shack = two_radio_shack();
    let afar = Afar {
        stand_down: 3,
        starts_voided: 0,
    };
    let mut asked: tempo_app::settings::RadioProfilePatch =
        serde_json::from_value(serde_json::to_value(&shack.radios[1]).unwrap()).unwrap();
    asked.ptt_method = "rts".into();
    asked.ptt_serial_port = "COM9".into();
    asked.tx_level = 0.25;
    let mut patch = asked.clone();
    keep_the_shacks_radio_ptt(&shack, 1, &mut patch, Some(&afar));
    assert_eq!(
        (patch.ptt_method.as_str(), patch.ptt_serial_port.as_str()),
        ("dtr", "COM4"),
        "from afar, radio 1's PTT changed"
    );
    assert_eq!(patch.tx_level, 0.25, "CONTROL: the rest of the patch");
    let mut patch = asked.clone();
    keep_the_shacks_radio_ptt(&shack, 1, &mut patch, None);
    assert_eq!(patch.ptt_method, "rts", "CONTROL: the shack's own edit");
    assert!(
        takes_afar("update_radio_profile"),
        "the lane says where it came from"
    );
}

/// What `read` finds in a whole save made from `shack` with `change` alone: as the shack has it,
/// once the save from afar has gone through the keeps, and once the shack's own screen's has.
fn kept_from_afar<T>(
    shack: &tempo_app::settings::Settings,
    change: impl Fn(&mut tempo_app::settings::Settings),
    read: impl Fn(&tempo_app::settings::Settings) -> T,
) -> [T; 3] {
    let mut asked = shack.clone();
    change(&mut asked);
    let mut from_afar = asked.clone();
    keep_the_shacks_settings(
        shack,
        &mut from_afar,
        Some(&Afar {
            stand_down: 3,
            starts_voided: 0,
        }),
    );
    let mut at_the_shack = asked;
    keep_the_shacks_settings(shack, &mut at_the_shack, None);
    [read(shack), read(&from_afar), read(&at_the_shack)]
}

/// The shack's own keying lines: a serial keyline for CW on its own port, and true FSK for RTTY on
/// its own port and line, beside a CAT radio on COM4.
fn shack_that_keys_by_lines() -> tempo_app::settings::Settings {
    tempo_app::settings::Settings {
        serial_port: "COM4".into(),
        cw_keyer: tempo_app::settings::CwKeyerBackend::Serial,
        cw_key_port: "COM7".into(),
        cw_key_line: "dtr".into(),
        winkeyer_port: "COM6".into(),
        rtty_backend: "fsk".into(),
        rtty_fsk_port: "COM8".into(),
        rtty_fsk_line: "dtr".into(),
        ..Default::default()
    }
}

/// ⛔ How CW keys the transmitter is set at the station only: a save from afar keeps the shack's
/// keyer. CONTROL: the shack's own save takes it.
#[test]
fn a_save_from_afar_keeps_the_cw_keyer() {
    use tempo_app::settings::CwKeyerBackend;
    assert_eq!(
        kept_from_afar(
            &shack_that_keys_by_lines(),
            |s| s.cw_keyer = CwKeyerBackend::Soundcard,
            |s| s.cw_keyer
        ),
        [
            CwKeyerBackend::Serial,
            CwKeyerBackend::Serial,
            CwKeyerBackend::Soundcard
        ]
    );
}

/// ⛔ The CW keyline's port is set at the station only: a save from afar cannot point it at the CAT
/// port. CONTROL: the shack's own save takes it.
#[test]
fn a_save_from_afar_keeps_the_cw_key_port() {
    assert_eq!(
        kept_from_afar(
            &shack_that_keys_by_lines(),
            |s| s.cw_key_port = "COM4".into(),
            |s| s.cw_key_port.clone()
        ),
        ["COM7", "COM7", "COM4"]
    );
}

/// ⛔ Which line of that port keys CW is set at the station only. CONTROL: the shack's own save
/// takes it.
#[test]
fn a_save_from_afar_keeps_the_cw_key_line() {
    assert_eq!(
        kept_from_afar(
            &shack_that_keys_by_lines(),
            |s| s.cw_key_line = "rts".into(),
            |s| s.cw_key_line.clone()
        ),
        ["dtr", "dtr", "rts"]
    );
}

/// ⛔ The WinKeyer's port is set at the station only. CONTROL: the shack's own save takes it.
#[test]
fn a_save_from_afar_keeps_the_winkeyer_port() {
    assert_eq!(
        kept_from_afar(
            &shack_that_keys_by_lines(),
            |s| s.winkeyer_port = "COM4".into(),
            |s| s.winkeyer_port.clone()
        ),
        ["COM6", "COM6", "COM4"]
    );
}

/// ⛔ How RTTY keys the transmitter (AFSK, or true FSK on a serial line) is set at the station
/// only. CONTROL: the shack's own save takes it.
#[test]
fn a_save_from_afar_keeps_the_rtty_keying() {
    assert_eq!(
        kept_from_afar(
            &shack_that_keys_by_lines(),
            |s| s.rtty_backend = "afsk".into(),
            |s| s.rtty_backend.clone()
        ),
        ["fsk", "fsk", "afsk"]
    );
}

/// ⛔ The FSK keyline's port is set at the station only: a save from afar cannot point it at the
/// CAT port. CONTROL: the shack's own save takes it.
#[test]
fn a_save_from_afar_keeps_the_rtty_fsk_port() {
    assert_eq!(
        kept_from_afar(
            &shack_that_keys_by_lines(),
            |s| s.rtty_fsk_port = "COM4".into(),
            |s| s.rtty_fsk_port.clone()
        ),
        ["COM8", "COM8", "COM4"]
    );
}

/// ⛔ Which line of that port carries the FSK data is set at the station only. CONTROL: the shack's
/// own save takes it.
#[test]
fn a_save_from_afar_keeps_the_rtty_fsk_line() {
    assert_eq!(
        kept_from_afar(
            &shack_that_keys_by_lines(),
            |s| s.rtty_fsk_line = "rts".into(),
            |s| s.rtty_fsk_line.clone()
        ),
        ["dtr", "dtr", "rts"]
    );
}

/// The two-radio shack with the Flex client's Beta for CW, Tune and the ATU on for radio 1 alone.
fn shack_with_one_flex_beta() -> tempo_app::settings::Settings {
    let mut shack = two_radio_shack();
    shack.radios[1].flex_native_unbenched = true;
    shack.sync_flat_from_active();
    shack
}

/// ⛔ The Flex client's Beta that lets it start CW, Tune and the ATU no bench has confirmed is set at
/// the station only, and it is the radio's own, as its PTT is: a save from afar keeps the shack's
/// value for the radio its form was editing, either way. CONTROL: the shack's own save takes it.
#[test]
fn a_save_from_afar_keeps_the_flex_beta_of_the_radio_its_form_edited() {
    let shack = shack_with_one_flex_beta();
    assert!(!shack.flex_native_unbenched, "premise: radio 0 is live");
    assert_eq!(
        kept_from_afar(
            &shack,
            |s| s.flex_native_unbenched = true,
            |s| s.flex_native_unbenched
        ),
        [false, false, true],
        "radio 0's form, turned on"
    );
    assert_eq!(
        kept_from_afar(
            &shack,
            |s| {
                s.active_radio = 1;
                s.flex_native_unbenched = false;
            },
            |s| s.flex_native_unbenched
        ),
        [false, true, false],
        "radio 1's form, turned off"
    );
}

/// ⛔ …and a radio's edit from afar keeps that radio's Beta as the shack has it. CONTROL: the
/// shack's own edit takes it, and the rest of the edit is taken from afar.
#[test]
fn a_radio_edit_from_afar_keeps_that_radios_flex_beta() {
    let shack = shack_with_one_flex_beta();
    let mut asked: tempo_app::settings::RadioProfilePatch =
        serde_json::from_value(serde_json::to_value(&shack.radios[1]).unwrap()).unwrap();
    asked.flex_native_unbenched = false;
    asked.tx_level = 0.25;
    let mut patch = asked.clone();
    keep_the_shacks_radio_ptt(
        &shack,
        1,
        &mut patch,
        Some(&Afar {
            stand_down: 3,
            starts_voided: 0,
        }),
    );
    assert!(patch.flex_native_unbenched, "from afar, radio 1's Beta");
    assert_eq!(patch.tx_level, 0.25, "CONTROL: the rest of the edit");
    let mut patch = asked.clone();
    keep_the_shacks_radio_ptt(&shack, 1, &mut patch, None);
    assert!(
        !patch.flex_native_unbenched,
        "CONTROL: the shack's own edit"
    );
}

/// ⛔ "PROGRAMS ON TCI MAY TRANSMIT" IS SET AT THE SHACK ONLY. A whole save from afar neither
/// turns it on nor off, nor the server under it; a radio's edit from afar cannot name it, since it
/// is the station's and no radio's; and the read from afar says it is set at the station.
/// CONTROL: the same save from the shack's own screen takes both, and a field beside them is taken
/// from afar.
#[test]
fn a_save_from_afar_never_lets_programs_on_tci_transmit() {
    let shack = two_radio_shack();
    assert!(!shack.tci_server && !shack.tci_server_tx, "premise: off");
    let afar = Afar {
        stand_down: 3,
        starts_voided: 0,
    };
    let on = |s: &tempo_app::settings::Settings| (s.tci_server, s.tci_server_tx);
    let mut asked = shack.clone();
    asked.tci_server = true;
    asked.tci_server_tx = true;
    asked.cw_wpm = 31;
    let mut save = asked.clone();
    keep_the_shacks_settings(&shack, &mut save, Some(&afar));
    assert_eq!(
        on(&save),
        (false, false),
        "from afar, programs on TCI may transmit"
    );
    assert_eq!(save.cw_wpm, 31, "CONTROL: the rest of the save");
    // …and off: a save from afar does not turn the shack's own off either.
    let mut save = shack.clone();
    keep_the_shacks_settings(&asked, &mut save, Some(&afar));
    assert_eq!(on(&save), (true, true), "turned off from afar");
    let mut save = asked.clone();
    keep_the_shacks_settings(&shack, &mut save, None);
    assert_eq!(on(&save), (true, true), "CONTROL: the shack's own form");

    // A radio's edit from afar that names them anyway changes neither.
    let mut patch = serde_json::to_value(&shack.radios[1]).unwrap();
    patch["tciServer"] = json!(true);
    patch["tciServerTx"] = json!(true);
    let mut patch: tempo_app::settings::RadioProfilePatch =
        serde_json::from_value(patch).expect("a radio's edit");
    keep_the_shacks_radio_ptt(&shack, 1, &mut patch, Some(&afar));
    keep_the_shacks_radio_icom_address(&shack, 1, &mut patch, Some(&afar));
    let mut e = tempo_app::engine::Engine::with_settings(shack.clone());
    e.update_radio_profile(1, patch).expect("the edit is taken");
    assert_eq!(
        on(e.settings()),
        (false, false),
        "a radio's edit from afar, naming them"
    );

    let flagged = station_only_settings(Some(&afar), &shack).unwrap();
    assert!(
        flagged.contains(&"tciServerTx") && flagged.contains(&"tciServer"),
        "the read from afar does not say they are set at the station: {flagged:?}"
    );
}

/// The two-radio shack, its radios on the Icom network at their own addresses, a Cloudlog server
/// and a club host set. Fake addresses: no test here holds or sends a key.
fn shack_that_sends_keys() -> tempo_app::settings::Settings {
    let mut shack = two_radio_shack();
    shack.radios[0].icom_lan_host = "192.168.1.50".into();
    shack.radios[1].icom_lan_host = "192.168.1.51".into();
    shack.radios[1].icom_lan_port = 50011;
    shack.sync_flat_from_active();
    shack.cloudlog_url = "https://log.shack.example".into();
    shack.fd_join_addr = "192.168.1.10:42073".into();
    shack
}

/// A save naming other servers for every address the shack sends a stored key or password to,
/// its form editing radio `edited`.
fn save_pointing_elsewhere(
    shack: &tempo_app::settings::Settings,
    edited: u32,
) -> tempo_app::settings::Settings {
    let mut save = shack.clone();
    save.active_radio = edited;
    save.cloudlog_url = "https://collector.example".into();
    save.fd_join_addr = "203.0.113.9:42073".into();
    save.icom_lan_host = "203.0.113.7".into();
    save.icom_lan_port = 50099;
    save
}

/// Where the shack's stored keys and passwords would go: the Cloudlog URL, the club host, and the
/// Icom network address of the save's radio.
fn key_destinations(s: &tempo_app::settings::Settings) -> (String, String, String, u16) {
    (
        s.cloudlog_url.clone(),
        s.fd_join_addr.clone(),
        s.icom_lan_host.clone(),
        s.icom_lan_port,
    )
}

/// ⛔ A settings save from afar cannot send the shack's stored keys and passwords anywhere new: the
/// Cloudlog URL (the API key), the club host (the position's club key) and the Icom network
/// address (the radio's password) come back as the shack has them, the Icom one for the radio the
/// form edited. The same save at the shack takes them all.
#[test]
fn a_settings_save_from_afar_keeps_where_the_shack_sends_its_keys() {
    let shack = shack_that_sends_keys();
    let afar = Afar {
        stand_down: 3,
        starts_voided: 0,
    };
    let mut save = save_pointing_elsewhere(&shack, 1);
    keep_the_shacks_credential_addresses(&shack, &mut save, Some(&afar));
    assert_eq!(
        key_destinations(&save),
        (
            "https://log.shack.example".to_string(),
            "192.168.1.10:42073".to_string(),
            "192.168.1.51".to_string(),
            50011
        ),
        "from afar, where the shack sends its keys changed"
    );
    // A save with no roster describes the live radio.
    let mut save = save_pointing_elsewhere(&shack, 1);
    save.radios.clear();
    keep_the_shacks_credential_addresses(&shack, &mut save, Some(&afar));
    assert_eq!(
        (save.icom_lan_host.as_str(), save.icom_lan_port),
        ("192.168.1.50", tempo_app::settings::ICOM_LAN_DEFAULT_PORT),
        "from afar, with no roster, the live radio's Icom address"
    );
    // CONTROL: the same save at the shack.
    let mut save = save_pointing_elsewhere(&shack, 1);
    keep_the_shacks_credential_addresses(&shack, &mut save, None);
    assert_eq!(
        key_destinations(&save),
        (
            "https://collector.example".to_string(),
            "203.0.113.9:42073".to_string(),
            "203.0.113.7".to_string(),
            50099
        ),
        "CONTROL: at the shack, the save is taken"
    );
}

/// CONTROL: a save from afar still takes every address and name that carries no stored secret: the
/// user names (each connector's password goes to its own fixed host whatever the name), the
/// Cloudlog station and switch (the key still goes only to the shack's server), and the hosts a
/// program or a feed is reached at with no credential.
#[test]
fn a_settings_save_from_afar_takes_the_addresses_that_carry_no_key() {
    let shack = shack_that_sends_keys();
    let afar = Afar {
        stand_down: 3,
        starts_voided: 0,
    };
    let mut asked = shack.clone();
    asked.cloudlog_station_id = "7".into();
    asked.cloudlog_upload = !shack.cloudlog_upload;
    asked.icom_lan_user = "op2".into();
    asked.qrz_username = "N0CALL".into();
    asked.lotw_username = "n0call".into();
    asked.clublog_email = "op@example.com".into();
    asked.n3fjp_host = "192.168.1.20".into();
    asked.dxkeeper_host = "192.168.1.21".into();
    asked.n1mm_addr = "192.168.1.22:12060".into();
    asked.hrd_udp_addr = "192.168.1.23:2333".into();
    asked.aprs_is_host = "noam.aprs2.net".into();
    asked.cluster_host = "dxc.example:7300".into();
    asked.rotator_host = "192.168.1.24:4533".into();
    asked.flex_radio_ip = "192.168.1.25".into();
    let mut save = asked.clone();
    keep_the_shacks_credential_addresses(&shack, &mut save, Some(&afar));
    assert_eq!(
        moved(&save, &asked),
        Vec::<String>::new(),
        "from afar, an address with no key was kept"
    );
}

/// ⛔ The per-radio edit from afar (`update_radio_profile`) keeps the Icom network address that
/// radio's stored password goes to; the rest of its patch is taken, the user name among it, and the
/// shack's own edit takes the address.
#[test]
fn a_radio_edit_from_afar_keeps_that_radios_icom_address() {
    let shack = shack_that_sends_keys();
    let afar = Afar {
        stand_down: 3,
        starts_voided: 0,
    };
    let mut asked: tempo_app::settings::RadioProfilePatch =
        serde_json::from_value(serde_json::to_value(&shack.radios[1]).unwrap()).unwrap();
    asked.icom_lan_host = "203.0.113.7".into();
    asked.icom_lan_port = 50099;
    asked.icom_lan_user = "op2".into();
    let mut patch = asked.clone();
    keep_the_shacks_radio_icom_address(&shack, 1, &mut patch, Some(&afar));
    assert_eq!(
        (patch.icom_lan_host.as_str(), patch.icom_lan_port),
        ("192.168.1.51", 50011),
        "from afar, radio 1's Icom address changed"
    );
    assert_eq!(patch.icom_lan_user, "op2", "CONTROL: the rest of the patch");
    let mut patch = asked.clone();
    keep_the_shacks_radio_icom_address(&shack, 1, &mut patch, None);
    assert_eq!(
        (patch.icom_lan_host.as_str(), patch.icom_lan_port),
        ("203.0.113.7", 50099),
        "CONTROL: the shack's own edit"
    );
}

/// ⛔ The licence class, which decides where the station may transmit, is set at the shack only:
/// its own command is refused from afar, as a save from afar keeps it.
#[test]
fn the_licence_class_is_set_at_the_shack_only() {
    assert_eq!(class_of("set_license_class"), Some(Class::RefusedHere));
}

/// ★ The command lane writes `afar` into exactly the calls whose handler takes it: every command
/// the app registers takes `afar` in its signature in `lib.rs` exactly when `takes_afar` says the
/// lane writes it. A handler that took it unwritten would never hear a Stop came after its start; a
/// call written with it and a handler that took none would run unbound. CONTROL: a wired start
/// takes it and a station read does not.
#[test]
fn the_lane_writes_afar_into_exactly_the_calls_whose_handler_takes_it() {
    let lib = include_str!("../lib.rs");
    let mut read = 0;
    for cmd in crate::command_classes::registered() {
        if !lib.contains(&format!("fn {cmd}(")) {
            continue;
        }
        read += 1;
        let takes = parameters(cmd).iter().any(|(name, _)| name == AFAR);
        assert_eq!(takes, takes_afar(cmd), "{cmd}: takes `afar` {takes}");
    }
    assert!(read > 400, "the signatures read: {read}");
    assert!(
        takes_afar("call_cq") && !takes_afar("get_snapshot"),
        "CONTROL"
    );
    assert!(
        parameters("call_cq").iter().any(|(name, _)| name == AFAR),
        "CONTROL: the reader sees it"
    );
}

#[test]
fn only_the_computer_holding_the_lease_runs_a_station_command() {
    let shack = shack();
    let refused_here = shack.admit(&other_device(), call(1, "get_snapshot", json!({})));
    assert_eq!(kind(&refused_here), refused(Refusal::NotController));
    let admitted = shack.admit(&controller(), call(2, "get_snapshot", json!({})));
    assert_eq!(kind(&admitted), "run");
}

#[test]
fn arguments_that_are_not_an_object_are_refused() {
    let shack = shack();
    for (id, args) in [json!(null), json!([1, 2]), json!("get_snapshot")]
        .into_iter()
        .enumerate()
    {
        let admission = shack.admit(&controller(), call(id as u64 + 1, "get_snapshot", args));
        assert_eq!(kind(&admission), refused(Refusal::BadArguments));
    }
}

// --- Stop's rules --------------------------------------------------------------------------------

/// Each `args` for `cmd` is admitted as a Stop under every condition that refuses any other call:
/// from a device without the lease, with every slot full, past the rate cap, as a repeated id, and
/// with the shack's stores unreadable.
fn admitted_as_a_stop_whatever_else(cmd: &str, args: &[Value]) {
    let shack = shack();
    settings_now(&shack, None);
    let held = fill_the_slots(&shack, &controller());
    let now = Instant::now();
    for n in 0..=PER_SECOND as u64 {
        // Past the cap: these are refused, and the stops below still are not.
        let _ = shack.admit_at(
            &controller(),
            call(2000 + n, "get_snapshot", json!({})),
            now,
        );
    }
    assert_eq!(
        kind(&shack.admit_at(&controller(), call(3000, "get_snapshot", json!({})), now)),
        refused(Refusal::RateLimited),
        "the rate cap is reached"
    );
    for args in args {
        for who in [controller(), other_device()] {
            for id in [9000, 9000, 1, 0] {
                match shack.admit_at(&who, call(id, cmd, args.clone()), now) {
                    Admission::Stop { args: run } => {
                        assert_eq!(&run, args, "{cmd} runs with its own arguments")
                    }
                    other => panic!("{cmd} {args} id {id}: {}", kind(&other)),
                }
            }
        }
    }
    drop(held);
}

#[test]
fn every_stop_is_admitted_whatever_else() {
    let stops = of_class(Class::StopsTx);
    assert_eq!(stops.len(), 9, "the table's own count: {stops:?}");
    for name in stops {
        assert_eq!(route(name, &json!({})), Route::Shack, "{name}");
        // A stop takes no arguments the dispatcher reads, so even malformed ones do not refuse it.
        admitted_as_a_stop_whatever_else(name, &[json!({}), json!(null)]);
    }
}

#[test]
fn the_off_half_of_every_transmit_switch_is_never_refused() {
    // The operator's ruling of 2026-10-07: the dispatcher runs the off half under Stop's rules,
    // reading the command's own on/off argument.
    for (name, ..) in OFF_HALVES {
        admitted_as_a_stop_whatever_else(name, &off_args(name));
    }
}

#[test]
fn the_on_half_of_a_transmit_switch_is_its_class() {
    let shack = shack();
    for (id, (name, ..)) in OFF_HALVES.iter().enumerate() {
        let admission = shack.admit(&controller(), call(id as u64 + 1, name, on_args(name)));
        let expected = match (class_of(name), transmit_from_afar(name)) {
            (
                Some(Class::StartsTx),
                Ok(
                    Wired::Ft
                    | Wired::Js8
                    | Wired::Tune
                    | Wired::Rtty
                    | Wired::Psk
                    | Wired::Aprs
                    | Wired::Chat,
                ),
            ) => "run".to_string(),
            (Some(Class::StartsTx), Ok(Wired::HeldPress | Wired::LockedPress)) => {
                refused(Refusal::HeldPress)
            }
            (Some(Class::StartsTx), _) => refused(Refusal::TransmitNotYet),
            // Turning the RTTY sequencer on keys nothing, so it runs at the shack.
            (Some(Class::RunsAtShack), _) => "run".to_string(),
            (other, _) => panic!("{name} is {other:?}"),
        };
        assert_eq!(kind(&admission), expected, "{name}");
    }
}

#[test]
fn set_mode_is_a_stop_only_into_a_passive_mode() {
    let shack = shack();
    for mode in ["chat", "qso-monitor", "fieldday-sp"] {
        let admission = shack.admit(
            &other_device(),
            call(1, "set_mode", json!({ "mode": mode })),
        );
        assert_eq!(kind(&admission), "stop", "{mode}");
    }
    // A run mode calls CQ: a start, run from afar under the start's rules. A mode this table does
    // not know is never read as off: it goes as a start too, and the shack's own handler refuses it.
    for (id, mode) in ["qso-run", "fieldday-run", "bogus", "QSO-MONITOR"]
        .into_iter()
        .enumerate()
    {
        let args = json!({ "mode": mode });
        assert!(!is_stop("set_mode", &args), "{mode}");
        let admission = shack.admit(
            &controller(),
            call(id as u64 + 10, "set_mode", args.clone()),
        );
        assert_eq!(kind(&admission), "run", "{mode}");
        let admission = shack.admit(&other_device(), call(id as u64 + 20, "set_mode", args));
        assert_eq!(kind(&admission), refused(Refusal::NotController), "{mode}");
    }
}

#[test]
fn an_argument_that_does_not_read_as_off_is_the_on_half() {
    // Fail-safe for a start: only the argument's real "off" is a stop, and the guard below holds
    // each argument's name to the command's signature, so the screen's real off half reads. The
    // tune carrier is wired from afar, so each of these is admitted as its on half, a start.
    let shack = shack();
    for (id, args) in [
        json!({}),
        json!({ "on": "false" }),
        json!({ "on": 0 }),
        json!({ "on": null }),
        json!({ "enabled": false }),
    ]
    .into_iter()
    .enumerate()
    {
        let admission = shack.admit(&controller(), call(id as u64 + 1, "set_tune", args.clone()));
        assert_eq!(kind(&admission), "run", "{args}");
        assert!(!is_stop("set_tune", &args), "{args}");
    }
}

#[test]
fn a_stop_takes_no_slot_and_leaves_no_outcome() {
    let shack = shack();
    for id in 1..=(2 * IN_FLIGHT as u64) {
        assert_eq!(
            kind(&shack.admit(&controller(), call(id, "halt_tx", json!({})))),
            "stop"
        );
    }
    let next = shack.admit(&controller(), call(100, "get_snapshot", json!({})));
    assert_eq!(kind(&next), "run", "no slot was taken");
    assert_eq!(
        shack.outcomes(&controller().key, &[1, 2]),
        vec![Outcome::Unknown, Outcome::Unknown],
        "a stop is answered when it runs, and never from the journal"
    );
}

// --- The rulings by name -------------------------------------------------------------------------

#[test]
fn a_whole_settings_save_from_afar_saves_every_field() {
    // The operator's ruling of 2026-10-07: "Every field saves", the stream window's behaviour, so
    // the connector fields, the stream switch and the two listener switches pass untouched.
    let shack = shack();
    let settings = json!({ "settings": {
        "remoteStream": true,
        "connectWeb": true,
        "fdScoreboard": true,
        "cloudlogUrl": "https://log.example/index.php",
        "tqslPath": "/opt/tqsl/bin/tqsl",
        "lotwUsername": "KD9TAW",
    }});
    match shack.admit(
        &controller(),
        save(1, "set_settings", settings.clone(), "r0"),
    ) {
        Admission::Run { args, .. } => assert_eq!(args, settings),
        other => panic!("{}", kind(&other)),
    }
}

#[test]
fn the_rigs_vox_switch_runs_at_the_shack_vox_on_included() {
    // The operator's ruling of 2026-10-07: "Allow VOX too".
    assert_eq!(class_of("set_rig_func"), Some(Class::RunsAtShack));
    let shack = shack();
    for (id, (func, on)) in [("VOX", true), ("VOX", false), ("NB", true)]
        .into_iter()
        .enumerate()
    {
        let args = json!({ "func": func, "on": on });
        assert_eq!(route("set_rig_func", &args), Route::Shack);
        let admission = shack.admit(&controller(), call(id as u64 + 1, "set_rig_func", args));
        assert_eq!(kind(&admission), "run", "{func} {on}");
    }
}

#[test]
fn a_winlink_password_typed_here_is_refused_at_both_ends() {
    // The operator's ruling of 2026-10-07: "Shack's stored password". An empty one is the
    // command's own "use the keychain's", so the session runs at the shack.
    let shack = shack();
    let stored = json!({ "callsign": "KD9TAW", "password": "" });
    assert_eq!(route("winlink_connect", &stored), Route::Shack);
    assert_eq!(
        kind(&shack.admit(&controller(), call(1, "winlink_connect", stored))),
        "run"
    );
    for (id, typed) in [
        json!({ "callsign": "KD9TAW", "password": "not-a-real-one" }),
        json!({ "callsign": "KD9TAW", "password": " " }),
        json!({ "callsign": "KD9TAW", "password": null }),
        json!({ "callsign": "KD9TAW" }),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            route("winlink_connect", &typed),
            Route::Refused(Refusal::TypedPassword),
            "never sent: {}",
            typed["password"]
        );
        let admission = shack.admit(&controller(), call(id as u64 + 2, "winlink_connect", typed));
        assert_eq!(kind(&admission), refused(Refusal::TypedPassword));
    }
}

#[test]
fn a_radios_icom_network_password_is_set_at_the_shack_only() {
    // Credentials are set at the shack only, as for every other service: storing and clearing
    // are each refused at both ends. A typed password is refused before it is sent.
    let shack = shack();
    for (id, (name, args)) in [
        (
            "set_icom_lan_password",
            json!({ "radioId": 1, "password": "not-a-real-one" }),
        ),
        ("clear_icom_lan_password", json!({ "radioId": 1 })),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(class_of(name), Some(Class::RefusedHere), "{name}");
        assert_eq!(
            route(name, &args),
            Route::Refused(Refusal::ShackOnly),
            "{name} is never sent"
        );
        let admission = shack.admit(&controller(), call(id as u64 + 1, name, args));
        assert_eq!(kind(&admission), "refused shackOnly", "{name} at the shack");
    }
}

#[test]
fn find_tci_server_runs_from_the_shacks_own_screen_only() {
    // Find TCI server dials whatever address the form holds, from the shack: from afar it would let
    // another computer make the station reach into its own network. Refused at both ends, with the
    // arguments it really takes.
    let shack = shack();
    let args = json!({ "host": "192.168.1.30", "port": 50001 });
    assert_eq!(class_of("tci_probe"), Some(Class::RefusedHere));
    assert_eq!(
        route("tci_probe", &args),
        Route::Refused(Refusal::ShackOnly),
        "it is never sent"
    );
    let admission = shack.admit(&controller(), call(1, "tci_probe", args));
    assert_eq!(
        kind(&admission),
        refused(Refusal::ShackOnly),
        "at the shack"
    );
}

/// From afar the Settings badges say whether each credential is saved, and a connector's Test
/// runs at the shack with the key stored there: each is sent, and admitted, as a station command.
/// What each answers carries no secret (`remote_credential_reply_tests` in `lib.rs`).
#[test]
fn a_credential_badge_and_a_connector_test_run_at_the_shack_from_afar() {
    let shack = shack();
    for (id, (name, args)) in [
        ("get_credentials_status", json!({})),
        ("icom_lan_password_saved", json!({ "radioId": 1 })),
        ("qrz_test_connection", json!({})),
        ("n3fjp_test_connection", json!({})),
        ("cloudlog_station_info", json!({})),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(class_of(name), Some(Class::RunsAtShack), "{name}");
        assert_eq!(
            route(name, &args),
            Route::Shack,
            "{name} is sent to the shack"
        );
        let admission = shack.admit(&controller(), call(id as u64 + 1, name, args));
        assert_eq!(kind(&admission), "run", "{name} at the shack");
    }
}

/// Typing or clearing a credential from afar stays refused: every command that stores or clears
/// one is refused at both ends, whatever the badges and Tests above now do.
#[test]
fn no_credential_is_typed_or_cleared_from_afar() {
    let setters: Vec<&str> = CLASSES
        .iter()
        .map(|(name, ..)| *name)
        .filter(|name| name.starts_with("set_") || name.starts_with("clear_"))
        .filter(|name| {
            ["password", "_key", "_code", "_token"]
                .iter()
                .any(|word| name.ends_with(word))
        })
        .collect();
    assert_eq!(
        setters.len(),
        23,
        "premise: the credential setters are found by name: {setters:?}"
    );
    for name in setters {
        assert_eq!(class_of(name), Some(Class::RefusedHere), "{name}");
    }
}

#[test]
fn a_cluster_spot_and_a_confirmation_check_run_at_the_shack_as_their_siblings_do() {
    // A spot to the DX cluster goes as the activation's own spot goes, by the shack's connections;
    // the shack still refuses it while a contest allows spotting only over RF. Starting Check
    // confirmations goes as the check's other steps go: at the shack, with the credentials stored
    // there, changing nothing until Apply.
    let shack = shack();
    for (id, (name, sibling, args)) in [
        (
            "post_spot",
            "self_spot_activation",
            json!({ "freqMhz": 14.074, "call": "KD9TAW", "comment": "FT8" }),
        ),
        ("start_confirmation_check", "confirmation_check", json!({})),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(class_of(sibling), Some(Class::RunsAtShack), "{sibling}");
        assert_eq!(class_of(name), Some(Class::RunsAtShack), "{name}");
        assert_eq!(route(name, &args), Route::Shack, "{name} is sent");
        match shack.admit(&controller(), call(id as u64 + 1, name, args.clone())) {
            Admission::Run { args: ran, .. } => assert_eq!(ran, args, "{name} at the shack"),
            other => panic!("{name} at the shack: {}", kind(&other)),
        }
    }
}

// --- At most once --------------------------------------------------------------------------------

#[test]
fn a_repeated_id_is_answered_and_never_run_twice() {
    let shack = shack();
    let Admission::Run { ticket, .. } =
        shack.admit(&controller(), call(5, "log_current_qso", json!({})))
    else {
        panic!("admitted")
    };
    assert_eq!(
        kind(&shack.admit(&controller(), call(5, "log_current_qso", json!({})))),
        "answered Running",
        "a repeat while it runs"
    );
    let _ = ticket.finish(Ok(json!({ "logged": true })));
    assert_eq!(
        kind(&shack.admit(&controller(), call(5, "log_current_qso", json!({})))),
        "answered Done",
        "a repeat once it ran"
    );
    assert_eq!(
        kind(&shack.admit(&controller(), call(3, "log_current_qso", json!({})))),
        "run",
        "an id below the highest, never seen, is new: calls admitted together, and one sent in \
         pieces, come in any order"
    );
}

#[test]
fn what_became_of_each_call_after_a_reconnect() {
    let shack = shack();
    let me = controller();
    let run = |id, cmd: &str| match shack.admit(&me, call(id, cmd, json!({}))) {
        Admission::Run { ticket, .. } => ticket,
        other => panic!("{id}: {}", kind(&other)),
    };
    let _ = run(1, "log_current_qso").finish(Ok(json!(null)));
    let _ = run(2, "upload_lotw_report").finish(Err("LoTW answered 503".to_string()));
    let running = run(3, "import_adif");
    let _ = shack.admit(&other_device(), call(4, "log_current_qso", json!({})));
    drop(run(5, "edit_qso"));
    assert_eq!(
        shack.outcomes(&me.key, &[1, 2, 3, 4, 5, 6]),
        vec![
            Outcome::Done,
            Outcome::Failed("LoTW answered 503".to_string()),
            Outcome::Running,
            Outcome::Refused(Refusal::NotController),
            Outcome::Lost,
            Outcome::Unknown,
        ]
    );
    drop(running);
    assert_eq!(
        shack.outcomes(&caller(2, true).key, &[1]),
        vec![Outcome::Unknown],
        "another run of Nexus has its own journal"
    );
}

#[test]
fn outcomes_are_kept_for_ten_minutes_and_an_old_id_is_never_run_again() {
    let shack = shack();
    let Admission::Run { ticket, .. } =
        shack.admit(&controller(), call(1, "log_current_qso", json!({})))
    else {
        panic!("admitted")
    };
    let _ = ticket.finish(Ok(json!(null)));
    let start = Instant::now();
    let key = controller().key;
    assert_eq!(
        shack.outcomes_at(&key, &[1], start + RESULT_AGE - Duration::from_secs(1)),
        vec![Outcome::Done]
    );
    let later = start + RESULT_AGE + Duration::from_secs(1);
    assert_eq!(shack.outcomes_at(&key, &[1], later), vec![Outcome::Unknown]);
    assert_eq!(
        kind(&shack.admit_at(&controller(), call(1, "log_current_qso", json!({})), later)),
        refused(Refusal::Replayed)
    );
}

#[test]
fn the_journal_keeps_the_newest_outcomes_and_never_reruns_an_older_id() {
    let shack = shack();
    let now = Instant::now();
    for id in 1..=(JOURNAL as u64 + 1) {
        // Spread across seconds so the rate cap stays out of it.
        let at = now + Duration::from_secs(id / u64::from(PER_SECOND) + 1);
        match shack.admit_at(&controller(), call(id, "get_snapshot", json!({})), at) {
            Admission::Run { ticket, .. } => drop(ticket.finish(Ok(json!(null)))),
            other => panic!("{id}: {}", kind(&other)),
        }
    }
    let key = controller().key;
    let last = JOURNAL as u64 + 1;
    assert_eq!(
        shack.outcomes(&key, &[1, 2, last]),
        vec![Outcome::Unknown, Outcome::Done, Outcome::Done]
    );
    assert_eq!(
        kind(&shack.admit(&controller(), call(1, "get_snapshot", json!({})))),
        refused(Refusal::Replayed)
    );
}

#[test]
fn a_new_run_of_nexus_starts_its_ids_again() {
    let shack = shack();
    drop(shack.admit(&caller(1, true), call(7, "get_snapshot", json!({}))));
    assert_eq!(
        kind(&shack.admit(&caller(2, true), call(1, "get_snapshot", json!({})))),
        "run"
    );
}

#[test]
fn a_long_error_is_kept_cut_on_a_character_boundary() {
    let long = "é".repeat(ERROR_BYTES);
    let kept = bounded(&long);
    assert!((ERROR_BYTES - 1..=ERROR_BYTES).contains(&kept.len()));
    assert!(long.starts_with(&kept));
    assert_eq!(bounded("short"), "short");
}

// --- Caps ----------------------------------------------------------------------------------------

#[test]
fn the_in_flight_caps_refuse_past_eight_calls_and_four_of_one_command() {
    let shack = shack();
    let me = controller();
    let same: Vec<_> = (1..=PER_COMMAND as u64)
        .map(|id| shack.admit(&me, call(id, "get_snapshot", json!({}))))
        .collect();
    assert!(same.iter().all(|a| kind(a) == "run"));
    assert_eq!(
        kind(&shack.admit(&me, call(10, "get_snapshot", json!({})))),
        refused(Refusal::Busy),
        "a fifth of one command"
    );
    let others: Vec<_> = (11..)
        .take(IN_FLIGHT - PER_COMMAND)
        .map(|id| shack.admit(&me, call(id, "get_band_plan", json!({}))))
        .collect();
    assert!(others.iter().all(|a| kind(a) == "run"));
    assert_eq!(
        kind(&shack.admit(&me, call(20, "get_settings", json!({})))),
        refused(Refusal::Busy),
        "a ninth call"
    );
    assert_eq!(
        kind(&shack.admit(&caller(9, true), call(1, "get_settings", json!({})))),
        "run",
        "the caps are each run's own"
    );
    drop(others);
    assert_eq!(
        kind(&shack.admit(&me, call(21, "get_settings", json!({})))),
        "run",
        "a finished call frees its slot"
    );
    drop(same);
}

#[test]
fn the_rate_cap_refuses_past_its_count_in_a_second() {
    let shack = shack();
    let now = Instant::now();
    for id in 1..=u64::from(PER_SECOND) {
        match shack.admit_at(&controller(), call(id, "get_snapshot", json!({})), now) {
            Admission::Run { ticket, .. } => drop(ticket.finish(Ok(json!(null)))),
            other => panic!("{id}: {}", kind(&other)),
        }
    }
    assert_eq!(
        kind(&shack.admit_at(&controller(), call(1000, "get_snapshot", json!({})), now)),
        refused(Refusal::RateLimited)
    );
    let next = now + Duration::from_secs(1);
    assert_eq!(
        kind(&shack.admit_at(&controller(), call(1001, "get_snapshot", json!({})), next)),
        "run"
    );
}

// --- A stale whole save --------------------------------------------------------------------------

#[test]
fn a_whole_save_from_an_older_copy_is_refused() {
    let shack = shack();
    // A list save, too, is checked on the whole store where no copy of it is kept, as here.
    let saves: Vec<&str> = STORES
        .iter()
        .filter(|(_, store, access)| {
            *store == Store::Settings && matches!(access, Access::WholeSave | Access::PartSave)
        })
        .map(|(name, ..)| *name)
        .collect();
    assert_eq!(saves.len(), 7, "{saves:?}");
    let mut id = 0;
    let mut next = || {
        id += 1;
        id
    };
    for name in saves {
        settings_now(&shack, Some("r1"));
        for base in ["r0", "", "r10"] {
            let admission = shack.admit(&controller(), save(next(), name, json!({}), base));
            assert_eq!(
                kind(&admission),
                refused(Refusal::ChangedAtShack),
                "{name} {base:?}"
            );
        }
        let no_base = shack.admit(&controller(), call(next(), name, json!({})));
        assert_eq!(
            kind(&no_base),
            refused(Refusal::ChangedAtShack),
            "{name} names no copy"
        );
        settings_now(&shack, None);
        let unreadable = shack.admit(&controller(), save(next(), name, json!({}), "r1"));
        assert_eq!(
            kind(&unreadable),
            refused(Refusal::ChangedAtShack),
            "{name} unreadable"
        );
        settings_now(&shack, Some("r1"));
        let current = shack.admit(&controller(), save(next(), name, json!({}), "r1"));
        assert_eq!(kind(&current), "run", "{name} from the current copy");
    }
}

#[test]
fn a_read_answers_with_the_revision_from_before_it_ran() {
    let shack = shack();
    let Admission::Run { ticket, .. } =
        shack.admit(&controller(), call(1, "get_settings", json!({})))
    else {
        panic!("admitted")
    };
    // A save at the shack lands while the read runs: the copy may or may not have it, so the
    // older revision is the one that can only refuse, never let a stale save through.
    settings_now(&shack, Some("r1"));
    let reply = ticket.finish(Ok(json!({ "mycall": "KD9TAW" })));
    assert_eq!(reply.result, Ok(json!({ "mycall": "KD9TAW" })));
    assert_eq!(reply.revision.as_ref().map(Revision::as_str), Some("r0"));
    assert_eq!(
        kind(&shack.admit(&controller(), save(2, "set_settings", json!({}), "r0"))),
        refused(Refusal::ChangedAtShack)
    );
}

#[test]
fn an_accepted_save_answers_with_the_revision_after_it() {
    let shack = shack();
    let Admission::Run { ticket, .. } =
        shack.admit(&controller(), save(1, "set_settings", json!({}), "r0"))
    else {
        panic!("admitted")
    };
    settings_now(&shack, Some("r1"));
    let reply = ticket.finish(Ok(json!({ "snapshot": true })));
    assert_eq!(reply.revision.as_ref().map(Revision::as_str), Some("r1"));
    // The panel saves the station first and the radio second, from one copy.
    let second = shack.admit(
        &controller(),
        save(2, "update_radio_profile", json!({}), "r1"),
    );
    assert_eq!(kind(&second), "run");
    // A failed save answers with no revision: the copy is what it was.
    let failed = match second {
        Admission::Run { ticket, .. } => ticket.finish(Err("serial port busy".to_string())),
        other => panic!("{}", kind(&other)),
    };
    assert_eq!(failed.revision, None);
}

// --- The one mixed store -------------------------------------------------------------------------

#[test]
fn a_load_from_afar_carries_only_the_station_keys() {
    let shack = shack();
    let whole = map(&[
        ("nexus.memory.bank.v2", "[1]"),
        ("nexus.watchlist", "[\"K1ABC\"]"),
        ("nexus-ui-scale-cap", "1.5"),
        ("nexus.panels.phone.main", "{}"),
    ]);
    ui_now(&shack, &[]);
    assert_eq!(route("ui_state_load", &json!({})), Route::Split);
    let Admission::Run { ticket, .. } =
        shack.admit(&controller(), call(1, "ui_state_load", json!({})))
    else {
        panic!("admitted")
    };
    let reply = ticket.finish(Ok(serde_json::to_value(&whole).unwrap()));
    assert_eq!(
        reply.result,
        Ok(json!({ "nexus.memory.bank.v2": "[1]", "nexus.watchlist": "[\"K1ABC\"]" }))
    );
    assert_eq!(
        reply.revision,
        Revision::of(&BTreeMap::<String, String>::new()),
        "the revision of the station's part, as it stood before the read"
    );
    let Admission::Run { ticket, .. } =
        shack.admit(&controller(), call(2, "ui_state_load", json!({})))
    else {
        panic!("admitted")
    };
    assert_eq!(
        ticket.finish(Ok(json!(["not", "a", "map"]))).result,
        Ok(json!({})),
        "a malformed answer sends nothing"
    );
}

#[test]
fn a_save_from_afar_writes_the_station_keys_and_keeps_the_shacks_own() {
    let shack = shack();
    ui_now(
        &shack,
        &[
            ("nexus-ui-scale-cap", "1.5"),
            ("nexus.navOrder", "[\"ft8\"]"),
            ("nexus.watchlist", "old"),
            ("nexus.sats.chasing", "[25544]"),
        ],
    );
    let base = shack.inner.stores.revision(Store::UiState).unwrap();
    let sent = json!({ "state": {
        "nexus.watchlist": "new",
        "nexus-ui-scale-cap": "3.0",
        "nexus.features.v1": "{}",
    }});
    assert_eq!(route("ui_state_save", &sent), Route::Split);
    match shack.admit(
        &controller(),
        save(1, "ui_state_save", sent.clone(), base.as_str()),
    ) {
        Admission::Run { args, .. } => assert_eq!(
            args,
            // The other computer's scale and modes stay there; the chase set it removed is gone.
            json!({ "state": {
                "nexus-ui-scale-cap": "1.5",
                "nexus.navOrder": "[\"ft8\"]",
                "nexus.watchlist": "new",
            }})
        ),
        other => panic!("{}", kind(&other)),
    }
    // A save made before the shack's station keys changed is refused like any stale save; one
    // that changed only the shack's own keys is not.
    ui_now(
        &shack,
        &[
            ("nexus-ui-scale-cap", "2.0"),
            ("nexus.watchlist", "old"),
            ("nexus.sats.chasing", "[25544]"),
        ],
    );
    assert_eq!(
        kind(&shack.admit(
            &controller(),
            save(2, "ui_state_save", sent.clone(), base.as_str())
        )),
        "run",
        "only the shack's own keys moved"
    );
    ui_now(&shack, &[("nexus.watchlist", "changed at the shack")]);
    assert_eq!(
        kind(&shack.admit(&controller(), save(3, "ui_state_save", sent, base.as_str()))),
        refused(Refusal::ChangedAtShack)
    );
    let base = shack.inner.stores.revision(Store::UiState).unwrap();
    assert_eq!(
        kind(&shack.admit(
            &controller(),
            save(4, "ui_state_save", json!({ "state": [1] }), base.as_str())
        )),
        refused(Refusal::BadArguments)
    );
}

/// ★ A `ui-state.json` that is there and does not read is never a store with nothing in it: a
/// save from afar onto it is refused, because merged into an empty map it would write the
/// station's keys and remove every key of the shack's own (its rail, its scale, its layouts).
/// CONTROL: the same save, onto the same file once it reads, runs with the shack's keys kept.
#[test]
fn a_save_from_afar_onto_an_unreadable_ui_state_is_refused() {
    let shack = shack();
    ui_now(
        &shack,
        &[("nexus.navOrder", "[\"ft8\"]"), ("nexus.watchlist", "old")],
    );
    let base = shack.inner.stores.revision(Store::UiState).unwrap();
    let sent = json!({ "state": { "nexus.watchlist": "new" } });
    *lock(&shack.inner.stores.ui_unreadable) = true;
    assert_eq!(shack.inner.stores.revision(Store::UiState), None);
    assert_eq!(
        kind(&shack.admit(
            &controller(),
            save(1, "ui_state_save", sent.clone(), base.as_str())
        )),
        refused(Refusal::ChangedAtShack)
    );
    *lock(&shack.inner.stores.ui_unreadable) = false;
    match shack.admit(&controller(), save(2, "ui_state_save", sent, base.as_str())) {
        Admission::Run { args, .. } => assert_eq!(
            args,
            json!({ "state": { "nexus.navOrder": "[\"ft8\"]", "nexus.watchlist": "new" } })
        ),
        other => panic!("{}", kind(&other)),
    }
}

/// ★ …and a file that stops reading between the save's revision check and its merge refuses the
/// save as well: merged then, the save would remove every key of the shack's own. CONTROL: with
/// one read more, the same save runs.
#[test]
fn a_ui_state_that_stops_reading_before_the_merge_refuses_the_save() {
    let shack = shack();
    ui_now(
        &shack,
        &[("nexus.navOrder", "[\"ft8\"]"), ("nexus.watchlist", "old")],
    );
    let base = shack.inner.stores.revision(Store::UiState).unwrap();
    let sent = json!({ "state": { "nexus.watchlist": "new" } });
    // The revision's read, and then nothing.
    *lock(&shack.inner.stores.ui_reads_left) = Some(1);
    assert_eq!(
        kind(&shack.admit(
            &controller(),
            save(1, "ui_state_save", sent.clone(), base.as_str())
        )),
        refused(Refusal::ChangedAtShack)
    );
    *lock(&shack.inner.stores.ui_reads_left) = Some(2);
    assert_eq!(
        kind(&shack.admit(&controller(), save(2, "ui_state_save", sent, base.as_str()))),
        "run"
    );
}

#[test]
fn the_other_computer_splits_and_joins_by_the_same_keys() {
    let whole = map(&[
        ("nexus.memory.bank.v2", "mine"),
        ("nexus.profiles", "mine"),
        ("nexus-ui-scale-cap", "1.25"),
        ("nexus.panels.cw.main", "{}"),
    ]);
    let (station, own) = split_ui_state(whole);
    assert_eq!(
        station,
        map(&[("nexus.memory.bank.v2", "mine"), ("nexus.profiles", "mine")])
    );
    assert_eq!(
        own,
        map(&[
            ("nexus-ui-scale-cap", "1.25"),
            ("nexus.panels.cw.main", "{}")
        ])
    );
    let from_shack = map(&[
        ("nexus.memory.bank.v2", "shack's"),
        ("nexus-ui-scale-cap", "9"),
    ]);
    assert_eq!(
        join_ui_state(from_shack, own),
        map(&[
            ("nexus.memory.bank.v2", "shack's"),
            ("nexus-ui-scale-cap", "1.25"),
            ("nexus.panels.cw.main", "{}"),
        ]),
        "the shack's station keys, this computer's own keys"
    );
}

// --- Events --------------------------------------------------------------------------------------

#[test]
fn the_alert_is_the_one_event_that_crosses_and_both_screens_use_its_name() {
    assert!(forwards("alert"));
    for other in [
        "",
        "pounce",
        "alert2",
        "Alert",
        "tauri://close-requested",
        "remote-log-notice",
    ] {
        assert!(!forwards(other), "{other:?}");
    }
    assert!(
        include_str!("../lib.rs").contains(r#".emit(alert::EVENT, &alert::raised("pounce", "#),
        "the shack raises Pounce's as an alert made where it is raised"
    );
    assert!(
        include_str!("../../../ui/src/usePounce.ts").contains("listen<RaisedAlert>('alert'"),
        "the screen listens for it by this name"
    );
    // The station's decode alerts and its new parks are raised the same way, and heard by name.
    let lib = include_str!("../lib.rs");
    assert!(
        lib.contains(r#"app.emit(alert::EVENT, &alert::raised("needed", one))"#),
        "the needed-station alerts"
    );
    assert!(
        lib.contains(r#"alert::raised("parks", "#),
        "the parks newly on the air"
    );
    assert!(
        include_str!("../../../ui/src/useStationAlerts.ts").contains("listen<RaisedAlert>('alert'"),
        "the screen listens for them by this name"
    );
}

/// The station decides the needed-station alerts on each look at the decode window it serves,
/// under the Engine lock, and raises them after the lock is released, so no window's listener ever
/// runs under it.
#[test]
fn every_snapshot_served_decides_the_needed_alerts_and_raises_them_unlocked() {
    let lib = include_str!("../lib.rs");
    let start = lib.find("async fn get_snapshot(").expect("get_snapshot");
    let body = &lib[start..start + lib[start..].find("\n}\n").expect("its end")];
    let at = |what: &str| {
        body.find(what)
            .unwrap_or_else(|| panic!("{what} in get_snapshot"))
    };
    assert!(at("let snap = eng.snapshot();") < at("eng.needed_alerts(&snap,"));
    assert!(at("eng.needed_alerts(&snap,") < at("drop(eng);"));
    assert!(at("drop(eng);") < at("raise_needed(&app, &needed);"));
}

#[test]
fn every_refusal_has_its_own_code() {
    let all = [
        Refusal::Unclassed,
        Refusal::RunsHere,
        Refusal::ShackOnly,
        Refusal::TransmitNotYet,
        Refusal::HeldPress,
        Refusal::NoStopRoad,
        Refusal::NoPresence,
        Refusal::StaleState,
        Refusal::TypedPassword,
        Refusal::NotController,
        Refusal::Busy,
        Refusal::RateLimited,
        Refusal::Replayed,
        Refusal::ChangedAtShack,
        Refusal::BadArguments,
        Refusal::Stopped,
    ];
    let mut codes: Vec<&str> = all.iter().map(|r| r.code()).collect();
    assert!(codes
        .iter()
        .all(|c| c.chars().all(|ch| ch.is_ascii_alphabetic())));
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), all.len());
}

// --- The tables, held to what they describe ------------------------------------------------------

/// The parameters the screen gives a command, as `(json key, Rust type)`, read from its
/// `#[tauri::command]` signature in `lib.rs`. Managed state, the app handle and the window are
/// Tauri's to supply and are left out. A key is the parameter's name in camelCase, as Tauri reads
/// it, or as written under `rename_all = "snake_case"`.
fn parameters(cmd: &str) -> Vec<(String, String)> {
    let lib = include_str!("../lib.rs");
    let needle = format!("fn {cmd}(");
    let (start, attribute) = lib
        .match_indices(&needle)
        .filter_map(|(at, _)| {
            let attribute = lib[..at].rfind("#[tauri::command")?;
            (!lib[attribute..at].contains("fn ")).then_some((at, attribute))
        })
        .next()
        .unwrap_or_else(|| panic!("no #[tauri::command] fn {cmd}( in lib.rs"));
    let snake = lib[attribute..start].contains("rename_all = \"snake_case\"");
    let list = &lib[start + needle.len()..];
    let (mut parens, mut angles, mut from) = (0usize, 0usize, 0usize);
    let mut pieces = Vec::new();
    for (at, c) in list.char_indices() {
        match c {
            '(' => parens += 1,
            ')' if parens == 0 => {
                pieces.push(&list[from..at]);
                break;
            }
            ')' => parens -= 1,
            '<' => angles += 1,
            '>' => angles = angles.saturating_sub(1),
            ',' if parens == 0 && angles == 0 => {
                pieces.push(&list[from..at]);
                from = at + 1;
            }
            _ => {}
        }
    }
    pieces
        .into_iter()
        .filter_map(|piece| {
            let (name, ty) = piece.split_once(':')?;
            let (name, ty) = (
                name.trim(),
                ty.split_whitespace().collect::<Vec<_>>().join(" "),
            );
            let tauris = ["State<", "AppHandle", "Window", "WebviewWindow"];
            if tauris
                .iter()
                .any(|t| ty.trim_start_matches("tauri::").starts_with(t))
            {
                return None;
            }
            let key = if snake {
                name.to_string()
            } else {
                name.split('_')
                    .enumerate()
                    .map(|(n, part)| {
                        let mut chars = part.chars();
                        match (n, chars.next()) {
                            (0, _) | (_, None) => part.to_string(),
                            (_, Some(first)) => first.to_uppercase().chain(chars).collect(),
                        }
                    })
                    .collect()
            };
            Some((key, ty))
        })
        .collect()
}

#[test]
fn the_parameter_reader_reads_what_tauri_reads() {
    assert_eq!(
        parameters("js8_answer_reply"),
        vec![
            ("yes".to_string(), "bool".to_string()),
            ("display".to_string(), "String".to_string()),
            ("firesAtMs".to_string(), "u64".to_string()),
            (
                "afar".to_string(),
                "Option<command_dispatch::Afar>".to_string()
            ),
        ]
    );
    assert_eq!(
        parameters("winlink_connect"),
        vec![
            ("callsign".to_string(), "String".to_string()),
            ("password".to_string(), "String".to_string()),
        ]
    );
}

#[test]
fn each_off_half_reads_an_argument_its_command_takes() {
    let registered = registered();
    for (name, arg, off) in OFF_HALVES {
        assert!(registered.contains(name), "{name} is not registered");
        let wanted = match off {
            Off::False => "bool",
            Off::OneOf(_) => "String",
        };
        assert!(
            parameters(name).contains(&(arg.to_string(), wanted.to_string())),
            "{name} takes no `{arg}: {wanted}`: {:?}",
            parameters(name)
        );
    }
}

/// The transmit commands with an on/off-looking argument that have no off half, and why.
const NO_OFF_HALF: &[(&str, &str)] = &[
    (
        "hold_ptt_lock",
        "Its bool is the answer to \"Still talking?\", not a switch: the Lock is let go by the PTT's \
         own off half, as a held press is.",
    ),
    (
        "js8_answer_reply",
        "Its No declines a reply that only a Yes would queue: nothing is on the air to stop.",
    ),
    (
        "jtty_live_commit",
        "Its `flush` says whether everything typed goes now or only the words already complete, \
         not a switch: a live session is stopped by `jtty_stop`.",
    ),
    (
        "send_cw",
        "Its `reply` says whether the message answers a station (the speed it may key at), not a \
         switch: the CW is stopped by `stop_cw`.",
    ),
    (
        "send_cw_armed",
        "Its `reply` says whether the message answers a station, not a switch: the CW is stopped \
         by `stop_cw`.",
    ),
    (
        "sstv_send",
        "Whether the call goes in the picture is part of what is sent, not a switch.",
    ),
];

#[test]
fn every_transmit_command_with_a_switch_argument_has_its_off_half() {
    for name in of_class(Class::StartsTx) {
        let switch = parameters(name)
            .iter()
            .any(|(_, ty)| ty == "bool" || ty == "Option<bool>");
        let listed = OFF_HALVES.iter().any(|(row, ..)| *row == name)
            || NO_OFF_HALF.iter().any(|(row, _)| *row == name);
        assert!(
            !switch || listed,
            "{name} starts a transmission and takes a bool: give its off half a row in \
             OFF_HALVES, or say in NO_OFF_HALF why it has none"
        );
    }
    for (name, why) in NO_OFF_HALF {
        assert_eq!(class_of(name), Some(Class::StartsTx), "{name}: {why}");
    }
}

#[test]
fn every_store_command_is_a_registered_station_command() {
    let registered = registered();
    for (name, ..) in STORES {
        assert!(registered.contains(name), "{name}");
        assert_eq!(class_of(name), Some(Class::RunsAtShack), "{name}");
    }
    for window in STORES.windows(2) {
        assert!(window[0].0 < window[1].0, "{} out of order", window[1].0);
    }
}

/// The durable keys that belong to the computer the operator sits at: the UI scale's cap, the
/// rail's order, which modes the rail shows, and the main window's one-time layout switch. The main
/// window's pane layouts (`nexus.panels.<view>.main`) are durable by prefix and stay there too.
const OWN_KEYS: &[&str] = &[
    "nexus-ui-scale-cap",
    "nexus.navOrder",
    "nexus.features.v1",
    "nexus.connect.switch.main",
];

#[test]
fn every_durable_key_is_in_exactly_one_half_of_the_split() {
    let store = include_str!("../../../ui/src/features/durableStore.ts");
    let list = store
        .split_once("export const DURABLE_KEYS: readonly string[] = [")
        .and_then(|(_, rest)| rest.split_once("\n]"))
        .map(|(list, _)| list)
        .expect("durableStore.ts has its DURABLE_KEYS list");
    let durable: Vec<&str> = list
        .lines()
        .map(|line| line.split("//").next().unwrap_or("").trim())
        .filter_map(|line| line.strip_prefix('\'')?.split_once('\''))
        .map(|(key, _)| key)
        .collect();
    assert!(durable.len() >= 14, "read the list: {durable:?}");
    for key in &durable {
        let station = STATION_KEYS.contains(key);
        let own = OWN_KEYS.contains(key);
        assert!(
            station != own,
            "{key}: say in STATION_KEYS or OWN_KEYS which computer it belongs to"
        );
    }
    for key in STATION_KEYS.iter().chain(OWN_KEYS) {
        assert!(durable.contains(key), "{key} is not a durable key");
    }
    assert!(store.contains("key.startsWith('nexus.panels.') && key.endsWith('.main')"));
    assert!(!STATION_KEYS
        .iter()
        .any(|key| key.starts_with("nexus.panels.")));
}

/// ★ The screen's own list of the station's keys is this one. On a computer driving the shack its
/// durable store keeps exactly those keys out of that computer's `localStorage`, and the split
/// sends exactly these to the shack: a key on one list and not the other would be the station's
/// data read from the wrong computer, or this computer's kept at the shack.
#[test]
fn the_screens_station_keys_are_the_dispatchers() {
    let store = include_str!("../../../ui/src/features/durableStore.ts");
    let list = store
        .split_once("export const STATION_KEYS: readonly string[] = [")
        .and_then(|(_, rest)| rest.split_once("\n]"))
        .map(|(list, _)| list)
        .expect("durableStore.ts has its STATION_KEYS list");
    let mut screens: Vec<&str> = list
        .lines()
        .map(|line| line.split("//").next().unwrap_or("").trim())
        .filter_map(|line| line.strip_prefix('\'')?.split_once('\''))
        .map(|(key, _)| key)
        .collect();
    screens.sort_unstable();
    let mut ours = STATION_KEYS.to_vec();
    ours.sort_unstable();
    assert_eq!(screens, ours);
}

// --- The shack's own stores ----------------------------------------------------------------------

#[test]
fn the_shacks_settings_revision_moves_with_a_change_and_only_with_one() {
    let engine: crate::SharedEngine = Arc::new(Mutex::new(tempo_app::engine::Engine::new(
        "KD9TAW", "EN52", 0,
    )));
    let stores = ShackStores {
        engine: Arc::clone(&engine),
    };
    let first = stores
        .revision(Store::Settings)
        .expect("the settings serialize");
    assert_eq!(first.as_str().len(), 64, "a SHA-256, in hex");
    assert_eq!(
        stores.revision(Store::Settings),
        Some(first.clone()),
        "nothing changed"
    );
    let mut changed = crate::engine_lock(&engine).settings().clone();
    changed.mycall = "W9XYZ".to_string();
    crate::engine_lock(&engine).apply_settings(changed);
    assert_ne!(stores.revision(Store::Settings), Some(first));
}

#[test]
fn a_stale_save_is_refused_against_the_shacks_real_settings() {
    let engine: crate::SharedEngine = Arc::new(Mutex::new(tempo_app::engine::Engine::new(
        "KD9TAW", "EN52", 0,
    )));
    let shack = Dispatcher::new(ShackStores {
        engine: Arc::clone(&engine),
    });
    let Admission::Run { ticket, .. } =
        shack.admit(&controller(), call(1, "get_settings", json!({})))
    else {
        panic!("admitted")
    };
    let copy = ticket
        .finish(Ok(json!({})))
        .revision
        .expect("a read answers with its revision");
    let fresh = shack.admit(
        &controller(),
        save(2, "set_settings", json!({}), copy.as_str()),
    );
    assert_eq!(kind(&fresh), "run", "made from the current copy");
    drop(fresh);
    // The shack's own screen saves a change.
    let mut changed = crate::engine_lock(&engine).settings().clone();
    changed.mycall = "W9XYZ".to_string();
    crate::engine_lock(&engine).apply_settings(changed);
    assert_eq!(
        kind(&shack.admit(
            &controller(),
            save(3, "set_settings", json!({}), copy.as_str())
        )),
        refused(Refusal::ChangedAtShack)
    );
}

// --- The command link's needs --------------------------------------------------------------------

/// ★ A forgotten outcome closes its id and every id below it, and nothing else: once what became
/// of id 5 is no longer kept, an unseen id 4 is refused `replayed` as 5 is, and an unseen id 6 is
/// new. CONTROL: before that, 4 would have been new.
#[test]
fn an_unseen_id_at_or_below_a_forgotten_one_is_refused_and_above_it_runs() {
    let shack = shack();
    let now = Instant::now();
    let Admission::Run { ticket, .. } =
        shack.admit_at(&controller(), call(5, "log_current_qso", json!({})), now)
    else {
        panic!("admitted")
    };
    let _ = ticket.finish(Ok(json!(null)));
    let journals = lock(&shack.inner.journals);
    let journal = journals.get(&controller().key).expect("a journal");
    assert!(matches!(journal.seen(4), Seen::New), "premise: 4 is new");
    drop(journals);
    let later = now + RESULT_AGE + Duration::from_secs(1);
    for id in [5, 4] {
        assert_eq!(
            kind(&shack.admit_at(&controller(), call(id, "log_current_qso", json!({})), later)),
            refused(Refusal::Replayed),
            "{id}"
        );
    }
    assert_eq!(
        kind(&shack.admit_at(&controller(), call(6, "log_current_qso", json!({})), later)),
        "run"
    );
}

/// ⛔ An unkey is told from a Stop the same way at both ends ([`is_unkey`]): the off half of every
/// transmit switch is an unkey; every command that stops a transmission (the voice keyer's own
/// Stop and `release_stream_mic` included) is a Stop, by both roads, which halts everything at the
/// shack; and the on half of a switch is neither. CONTROL: each unkey runs under Stop's rules, so
/// it is never refused.
#[test]
fn an_unkey_is_the_off_half_of_a_switch_and_nothing_else() {
    for (cmd, ..) in OFF_HALVES {
        for off in off_args(cmd) {
            assert!(is_unkey(cmd, &off), "{cmd} {off}");
            assert!(
                is_stop(cmd, &off),
                "CONTROL: {cmd} {off} runs under Stop's rules"
            );
        }
    }
    for (cmd, args) in [
        ("set_ptt", json!({"on": true})),
        ("set_tune", json!({"on": true})),
        ("set_tx_enabled", json!({"enabled": true})),
        ("set_mode", json!({"mode": "fieldday-run"})),
        ("set_ptt", json!({})),
    ] {
        assert!(!is_unkey(cmd, &args), "{cmd} {args}");
    }
    let stops: Vec<&str> = CLASSES
        .iter()
        .filter(|(_, class, _)| *class == Class::StopsTx)
        .map(|(cmd, ..)| *cmd)
        .collect();
    assert_eq!(stops.len(), 9, "{stops:?}");
    for cmd in &stops {
        assert!(!is_unkey(cmd, &json!({})), "{cmd} is a Stop");
    }
    assert!(stops.contains(&"stop_voice") && stops.contains(&"release_stream_mic"));
}

/// ⛔ A start from afar runs only while BOTH generations it was read under stand
/// ([`still_bound`]): an unkey moves the starts-voided generation and voids it; a Stop or any halt
/// moves the stand-down and voids it. Only the stand-down is what a start that ran is ended by: an
/// unkey taken after a start from afar ran leaves what it began on the radio loop's next tick, and
/// a stand-down revoked with no halt ends it. CONTROL: under the generations as they stand it runs,
/// and from the shack's own screen (`None`) it always runs.
#[test]
fn a_start_from_afar_is_voided_by_an_unkey_or_a_stop_and_ended_only_by_a_stop() {
    use tempo_app::remote_control::transmit::TransmitAuthority;
    let mut e = tempo_app::engine::Engine::new("W9XYZ", "EN52", 0);
    let transmit = TransmitAuthority::default();
    e.set_remote_transmit_revocation(transmit.revocation());
    let read = |e: &tempo_app::engine::Engine| Afar {
        stand_down: e.remote_transmit_stand_down_generation(),
        starts_voided: e.remote_starts_voided_generation(),
    };
    let stopped = Err(STOPPED_SINCE.to_string());

    let before_an_unkey = read(&e);
    assert_eq!(still_bound(&e, Some(&before_an_unkey)), Ok(()), "CONTROL");
    e.remote_starts_voided().revoke();
    assert_eq!(
        still_bound(&e, Some(&before_an_unkey)),
        stopped,
        "after an unkey"
    );
    assert_eq!(still_bound(&e, None), Ok(()), "CONTROL: the shack's own");

    let before_a_stop = read(&e);
    assert_eq!(still_bound(&e, Some(&before_a_stop)), Ok(()), "CONTROL");
    e.halt_tx();
    assert_eq!(
        still_bound(&e, Some(&before_a_stop)),
        stopped,
        "after a halt"
    );
    let before_a_revocation = read(&e);
    transmit.revoke();
    assert_eq!(
        still_bound(&e, Some(&before_a_revocation)),
        stopped,
        "after a Stop"
    );

    // TX On from afar, then an unkey: TX stays on, tick after tick.
    let afar = read(&e);
    still_bound(&e, Some(&afar)).unwrap();
    e.set_tx_enabled_by_operator(true);
    started_from_afar(&mut e, Some(&afar));
    e.remote_starts_voided().revoke();
    e.set_ptt(false);
    let now = Instant::now();
    assert!(!e.poll_remote_transmit(now), "an unkey halted the station");
    assert!(
        !e.poll_remote_transmit(now),
        "an unkey halted the station later"
    );
    assert!(e.tx_enabled(), "an unkey from afar turned TX off");
    // …and a revocation with no halt ends it.
    transmit.revoke();
    assert!(e.poll_remote_transmit(now), "a revoked stand-down halts");
    assert!(!e.tx_enabled(), "a revoked stand-down left TX on");
}

/// ★ The link's Stop admission: every command that stops a transmission, and the off half of every
/// transmit switch, is taken as it came, and agrees with [`is_stop`]; anything else, the on half of
/// the same switches included, is handed back untouched for the dispatcher.
#[test]
fn admit_stop_takes_every_stop_as_it_came_and_hands_back_everything_else() {
    let stops: Vec<Call> = crate::command_classes::classes()
        .filter(|(_, class)| *class == Class::StopsTx)
        .map(|(cmd, _)| call(1, cmd, json!({})))
        .chain(OFF_HALVES.iter().map(|(cmd, arg, off)| {
            let value = match off {
                Off::False => json!(false),
                Off::OneOf(values) => json!(values[0]),
            };
            let mut args = serde_json::Map::new();
            args.insert(arg.to_string(), value);
            call(1, cmd, Value::Object(args))
        }))
        .collect();
    assert!(
        stops.len() > OFF_HALVES.len(),
        "premise: some StopsTx commands"
    );
    for stop in stops {
        let (cmd, args) = (stop.cmd.clone(), stop.args.clone());
        assert!(is_stop(&cmd, &args), "{cmd} {args}");
        let taken = admit_stop(stop).unwrap_or_else(|_| panic!("{cmd} {args} handed back"));
        assert_eq!((taken.cmd, taken.args), (cmd, args));
    }
    for (cmd, args) in [
        ("set_ptt", json!({"on": true})),
        ("set_tx_enabled", json!({"enabled": true})),
        ("set_mode", json!({"mode": "qso-run"})),
        ("call_cq", json!({})),
        ("get_settings", json!({})),
        ("no_such_command", json!({})),
    ] {
        assert!(!is_stop(cmd, &args), "{cmd}");
        let back = admit_stop(call(9, cmd, args.clone()))
            .err()
            .expect("handed back");
        assert_eq!((back.id, back.cmd.as_str(), back.args), (9, cmd, args));
    }
}

/// ★ Every refusal reads back from its code, as the other computer reads it. CONTROL: a code no
/// refusal has reads as none.
#[test]
fn every_refusal_reads_back_from_its_code() {
    for refusal in Refusal::ALL {
        assert_eq!(Refusal::named(refusal.code()), Some(refusal));
    }
    assert_eq!(Refusal::named("noSuchRefusal"), None);
}

/// ★ The one profile this build serves is the desktop's (the mobile addendum's hook: a phone is a
/// later build's). A phone is known by name and admitted nothing: a call the desktop runs is
/// refused it, whatever its class. CONTROL: the same call from the desktop runs.
#[test]
fn the_desktop_is_the_only_profile_served() {
    assert_eq!(Profile::named("desktop"), Some(Profile::Desktop));
    assert_eq!(
        Profile::named(Profile::Desktop.name()),
        Some(Profile::Desktop)
    );
    assert_eq!(Profile::named("mobile"), Some(Profile::Mobile));
    assert!(!Profile::Mobile.served());
    let shack = shack();
    let phone = Caller {
        profile: Profile::Mobile,
        ..caller(2, true)
    };
    for (id, name) in ["get_snapshot", "get_band_plan"].into_iter().enumerate() {
        let id = id as u64 + 1;
        assert_eq!(
            kind(&shack.admit(&phone, call(id, name, json!({})))),
            refused(Refusal::Unclassed),
            "{name} from a phone"
        );
        assert_eq!(
            kind(&shack.admit(&controller(), call(id, name, json!({})))),
            "run",
            "{name} from the desktop"
        );
    }
}

// --- The shack's own handler, on Tauri's own dispatch --------------------------------------------

#[tauri::command]
fn runner_hello(name: String) -> String {
    format!("hello {name}")
}

#[tauri::command]
async fn runner_later(n: u32) -> Result<u32, String> {
    Ok(n + 1)
}

#[tauri::command]
fn runner_refuses() -> Result<(), String> {
    Err("not from here".to_string())
}

/// What `runner` answers `cmd` with `args`.
fn ask_runner(runner: &Runner, cmd: &str, args: Value) -> Result<Value, String> {
    let (answer, answered) = std::sync::mpsc::channel();
    runner(
        cmd.to_string(),
        args,
        Box::new(move |result| {
            let _ = answer.send(result);
        }),
    );
    answered
        .recv_timeout(Duration::from_secs(5))
        .expect("the handler answered")
}

/// ★ The shack's runner is Tauri's own dispatch for the main window: a sync command and an async
/// one run with their arguments as the screen sends them, a command's own error comes back as its
/// error, and the call carries the app's invoke key and the window's own address, so Tauri's
/// permission check takes it as the screen's own. CONTROL: with no main window it runs nothing,
/// and the same request from an address that is not the window's is refused by that check.
#[test]
fn the_runner_is_tauris_own_dispatch_for_the_main_window() {
    use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponse};
    let app = tauri::test::mock_builder()
        .invoke_handler(tauri::generate_handler![
            runner_hello,
            runner_later,
            runner_refuses
        ])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("a mock app");
    let runner = tauri_runner(app.handle().clone());
    assert_eq!(
        ask_runner(&runner, "runner_hello", json!({"name": "KD9TAW"})),
        Err("The station's main window is not open.".to_string())
    );
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("a main window");
    assert_eq!(
        ask_runner(&runner, "runner_hello", json!({"name": "KD9TAW"})),
        Ok(json!("hello KD9TAW"))
    );
    assert_eq!(
        ask_runner(&runner, "runner_later", json!({"n": 41})),
        Ok(json!(42))
    );
    assert_eq!(
        ask_runner(&runner, "runner_refuses", json!({})),
        Err("not from here".to_string())
    );
    let (answer, answered) = std::sync::mpsc::channel();
    window.as_ref().clone().on_message(
        tauri::webview::InvokeRequest {
            cmd: "runner_hello".to_string(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "https://example.invalid/".parse().expect("an address"),
            body: InvokeBody::Json(json!({"name": "KD9TAW"})),
            headers: Default::default(),
            invoke_key: app.invoke_key().to_string(),
        },
        Box::new(move |_, _, response, _, _| {
            let _ = answer.send(matches!(response, InvokeResponse::Ok(_)));
        }),
    );
    assert_eq!(
        answered.recv_timeout(Duration::from_secs(5)),
        Ok(false),
        "a request from another address is refused"
    );
}

// --- A list save is checked on its own list (the operator's ruling of 2026-10-07) ----------------

/// A shack on a real Engine with a roster of two radios, and the Engine.
fn real_shack() -> (Dispatcher<ShackStores>, crate::SharedEngine) {
    let mut engine = tempo_app::engine::Engine::new("KD9TAW", "EN52", 0);
    engine.add_radio();
    let engine: crate::SharedEngine = Arc::new(Mutex::new(engine));
    let shack = Dispatcher::new(ShackStores {
        engine: Arc::clone(&engine),
    });
    (shack, engine)
}

/// The shack's own screen does something, through the Engine's own verbs as its commands do.
fn at_the_shack(engine: &crate::SharedEngine, act: impl FnOnce(&mut tempo_app::engine::Engine)) {
    act(&mut crate::engine_lock(engine));
}

/// A radio's whole patch as the settings page sends it, with its serial port set to `port`.
fn patch_with_port(
    engine: &crate::SharedEngine,
    port: &str,
) -> (u32, tempo_app::settings::RadioProfilePatch) {
    let radio = crate::engine_lock(engine).settings().radios[0].clone();
    let mut patch: tempo_app::settings::RadioProfilePatch =
        serde_json::from_value(serde_json::to_value(&radio).unwrap()).unwrap();
    patch.serial_port = port.to_string();
    (radio.id, patch)
}

/// A computer reads the settings, as call `id`: the revision of its copy.
fn read_copy(shack: &Dispatcher<ShackStores>, id: u64) -> Revision {
    let Admission::Run { ticket, .. } =
        shack.admit(&controller(), call(id, "get_settings", json!({})))
    else {
        panic!("admitted")
    };
    ticket
        .finish(Ok(json!({})))
        .revision
        .expect("a read answers with its revision")
}

/// What becomes of a list or whole save as call `id`, made from the copy at `base`.
fn saving(
    shack: &Dispatcher<ShackStores>,
    id: u64,
    cmd: &str,
    args: Value,
    base: &Revision,
) -> String {
    kind(&shack.admit(&controller(), save(id, cmd, args, base.as_str())))
}

fn macros() -> Value {
    json!({"profiles": [], "active": "Everyday"})
}

/// ★ "Check only that list": after a QSY at the shack, a list save made from the copy loaded before
/// it is admitted (the RTTY macros, the blocked calls), since its own list is as the copy had it,
/// while the whole form from the same copy is still refused. CONTROL: once the shack changes the
/// blocked calls, the blocked-calls save from that copy is refused.
#[test]
fn a_list_save_is_checked_on_its_own_list() {
    let (shack, engine) = real_shack();
    let copy = read_copy(&shack, 1);
    at_the_shack(&engine, |e| e.set_frequency(14.080, "20m", "RTTY"));
    assert_ne!(
        shack.inner.stores.revision(Store::Settings),
        Some(copy.clone()),
        "premise: the QSY changed the settings"
    );
    assert_eq!(saving(&shack, 2, "set_rtty_macros", macros(), &copy), "run");
    assert_eq!(
        saving(&shack, 3, "set_blocked_calls", json!({"calls": []}), &copy),
        "run"
    );
    assert_eq!(
        saving(&shack, 4, "set_settings", json!({}), &copy),
        refused(Refusal::ChangedAtShack),
        "the whole form is all one part"
    );
    at_the_shack(&engine, |e| e.set_blocked_calls(vec!["K1ABC".to_string()]));
    assert_eq!(
        saving(&shack, 5, "set_blocked_calls", json!({"calls": []}), &copy),
        refused(Refusal::ChangedAtShack)
    );
}

/// ★ A radio's profile save is checked on the fields its patch writes: the radio's bands changed
/// at the shack leave it admitted, and its serial port changed there refuses it. The bands are a
/// part of their own. CONTROL: a radio neither the copy nor the shack has is no part to differ.
#[test]
fn a_radio_save_is_checked_on_what_it_writes() {
    let (shack, engine) = real_shack();
    let (radio, patch) = patch_with_port(&engine, "COM3");
    let copy = read_copy(&shack, 1);
    let profile_save = json!({"id": radio, "patch": patch});
    at_the_shack(&engine, |e| {
        e.set_radio_bands(radio, vec!["20m".to_string()])
    });
    assert_eq!(
        saving(
            &shack,
            2,
            "update_radio_profile",
            profile_save.clone(),
            &copy
        ),
        "run"
    );
    assert_eq!(
        saving(
            &shack,
            3,
            "set_radio_bands",
            json!({"id": radio, "bands": ["40m"]}),
            &copy
        ),
        refused(Refusal::ChangedAtShack),
        "the bands changed at the shack"
    );
    let (_, moved) = patch_with_port(&engine, "COM9");
    at_the_shack(&engine, |e| e.update_radio_profile(radio, moved).unwrap());
    assert_eq!(
        saving(&shack, 4, "update_radio_profile", profile_save, &copy),
        refused(Refusal::ChangedAtShack)
    );
    let elsewhere = json!({"id": radio + 1000, "patch": {"serialPort": "COM3"}});
    assert_eq!(
        saving(&shack, 5, "update_radio_profile", elsewhere, &copy),
        "run",
        "no such radio in the copy nor now: the save changes nothing"
    );
}

/// ★ The revision a list save answers with is the computer's copy with that list as saved, never
/// the shack's whole settings: a second list save from it is checked on its own list again, so
/// a list the shack changed meanwhile is refused, and one it did not is admitted. CONTROL: that
/// revision is not the shack's own, which has a QSY the copy never had.
#[test]
fn after_a_list_save_the_copy_is_its_own_with_that_list_saved() {
    let (shack, engine) = real_shack();
    let radio = crate::engine_lock(&engine).settings().radios[0].id;
    let copy = read_copy(&shack, 1);
    at_the_shack(&engine, |e| e.set_frequency(7.074, "40m", "FT8"));
    let calls = json!({"calls": ["N0CALL"]});
    let Admission::Run { ticket, .. } = shack.admit(
        &controller(),
        save(2, "set_blocked_calls", calls, copy.as_str()),
    ) else {
        panic!("admitted")
    };
    // The save runs: the shack's list is what it wrote.
    at_the_shack(&engine, |e| e.set_blocked_calls(vec!["N0CALL".to_string()]));
    let mine = ticket.finish(Ok(json!({}))).revision.expect("a revision");
    assert_ne!(
        Some(&mine),
        shack.inner.stores.revision(Store::Settings).as_ref()
    );
    at_the_shack(&engine, |e| {
        e.set_radio_bands(radio, vec!["2m".to_string()])
    });
    assert_eq!(
        saving(
            &shack,
            3,
            "set_radio_bands",
            json!({"id": radio, "bands": []}),
            &mine
        ),
        refused(Refusal::ChangedAtShack)
    );
    assert_eq!(
        saving(&shack, 4, "set_routing_rules", json!({"rules": []}), &mine),
        "run"
    );
    assert_eq!(
        saving(&shack, 5, "set_blocked_calls", json!({"calls": []}), &mine),
        "run",
        "its own list, as it saved it"
    );
}

/// ★ Every list save's part is where the settings keep it: each path names a value in the real
/// settings' JSON, and every field a radio's patch can write is a field of the radio's profile.
/// CONTROL: a path to a field the settings do not have names nothing.
#[test]
fn each_list_saves_part_is_where_the_settings_keep_it() {
    let mut engine = tempo_app::engine::Engine::new("KD9TAW", "EN52", 0);
    engine.add_radio();
    let settings = engine.settings().clone();
    let json = serde_json::to_value(&settings).unwrap();
    let radio = settings.radios[0].clone();
    let profile = serde_json::to_value(&radio).unwrap();
    let patch: tempo_app::settings::RadioProfilePatch =
        serde_json::from_value(profile.clone()).expect("a profile carries every patch field");
    let patch = serde_json::to_value(patch).unwrap();
    assert!(
        patch.as_object().unwrap().len() > 20,
        "premise: the whole patch"
    );
    let mut cases = vec![
        ("set_blocked_calls", json!({"calls": []})),
        ("set_psk_macros", macros()),
        ("set_radio_bands", json!({"id": radio.id, "bands": []})),
        ("set_routing_rules", json!({"rules": []})),
        ("set_rtty_macros", macros()),
        (
            "update_radio_profile",
            json!({"id": radio.id, "patch": patch}),
        ),
    ];
    let saves: Vec<&str> = STORES
        .iter()
        .filter(|(_, _, access)| *access == Access::PartSave)
        .map(|(cmd, ..)| *cmd)
        .collect();
    assert_eq!(
        saves,
        cases.iter().map(|(cmd, _)| *cmd).collect::<Vec<_>>(),
        "a list save with no case here"
    );
    for (cmd, args) in cases.drain(..) {
        let paths = parts(cmd, &args).expect(cmd);
        for path in paths {
            assert!(at(&json, &path).is_some(), "{cmd}: {path:?} names nothing");
        }
    }
    assert_eq!(at(&json, &[Step::Key("noSuchField".to_string())]), None);
    assert_eq!(
        parts("set_radio_bands", &json!({"bands": []})),
        None,
        "no radio named"
    );
}

/// ⛔ A settings save from afar cannot change the program the shack runs to sign a LoTW upload:
/// `tqsl_path` keeps the shack's value, set or empty, both ways. CONTROL: the same save from the
/// shack's own screen takes it, and the LoTW fields beside it are taken from afar.
#[test]
fn a_settings_save_from_afar_keeps_the_program_the_shack_runs() {
    let shack = tempo_app::settings::Settings {
        tqsl_path: r"C:\Program Files (x86)\TrustedQSL\tqsl.exe".to_string(),
        ..Default::default()
    };
    let mut asked = shack.clone();
    asked.tqsl_path = r"C:\Users\Public\evil.exe".to_string();
    asked.lotw_station_location = "Home".to_string();
    let afar = Afar {
        stand_down: 3,
        starts_voided: 0,
    };

    let mut save = asked.clone();
    keep_the_shacks_transmit_settings(&shack, &mut save, Some(&afar));
    assert_eq!(
        save.tqsl_path, shack.tqsl_path,
        "from afar, the program the shack runs changed"
    );
    assert_eq!(
        save.lotw_station_location, "Home",
        "CONTROL: the rest of the save"
    );

    // An empty path (the default: TQSL found where it installs) is not filled from afar either.
    let unset = tempo_app::settings::Settings::default();
    let mut save = asked.clone();
    keep_the_shacks_transmit_settings(&unset, &mut save, Some(&afar));
    assert_eq!(save.tqsl_path, "", "from afar, a program was named");

    let mut save = asked.clone();
    keep_the_shacks_transmit_settings(&shack, &mut save, None);
    assert_eq!(
        save.tqsl_path, asked.tqsl_path,
        "CONTROL: the shack's own form"
    );
}

/// ⛔ A save from afar can neither change, blank nor plant a stored key: each of
/// `SECRET_SETTINGS` comes back as the shack has it, saved or not. CONTROL: the shack's own save, of
/// a form read with its keys, is taken as it came. The read's "saved at the station" is not saved
/// into the file (at the shack: `a_shack_save_of_a_form_read_without_its_keys_keeps_them`).
#[test]
fn a_settings_save_from_afar_cannot_change_blank_or_plant_a_stored_key() {
    let afar = Afar {
        stand_down: 2,
        starts_voided: 0,
    };
    for (name, field) in SECRET_SETTINGS {
        // (what the shack holds, what the save asks for)
        for (held, asked) in [
            ("FAKE-SHACK-KEY-0001", "FAKE-OTHER-KEY-0002"),
            ("FAKE-SHACK-KEY-0001", ""),
            ("", "FAKE-PLANTED-KEY-0003"),
        ] {
            let mut shack = tempo_app::settings::Settings::default();
            *field(&mut shack) = held.into();
            let mut save = shack.clone();
            *field(&mut save) = asked.into();
            save.unknown.insert(
                SAVED_AT_THE_STATION.into(),
                json!({ "clublogApiKey": true }),
            );
            let mut from_afar = save.clone();
            keep_the_shacks_secret_settings(&shack, &mut from_afar, Some(&afar));
            assert_eq!(
                field(&mut from_afar).as_str(),
                held,
                "from afar, {name} asked {asked:?}"
            );
            assert!(
                !from_afar.unknown.contains_key(SAVED_AT_THE_STATION),
                "from afar, the read's flags would be saved"
            );
            let mut at_the_shack = save.clone();
            at_the_shack.unknown.remove(SAVED_AT_THE_STATION);
            keep_the_shacks_secret_settings(&shack, &mut at_the_shack, None);
            assert_eq!(
                field(&mut at_the_shack).as_str(),
                asked,
                "CONTROL: the shack's own save of {name}"
            );
        }
    }
}

/// ⛔ A save at the shack of a form read while a stream watched (its keys blanked, the read's
/// "saved at the station" riding back with it) keeps each stored key as the shack has it, so the
/// blank never reaches the file. CONTROL: a save from a form read with the keys is taken as it came.
#[test]
fn a_shack_save_of_a_form_read_without_its_keys_keeps_them() {
    for (name, field) in SECRET_SETTINGS {
        let mut shack = tempo_app::settings::Settings::default();
        *field(&mut shack) = "FAKE-SHACK-KEY-0001".into();
        let mut save = shack.clone();
        *field(&mut save) = String::new();
        let mut withheld = save.clone();
        withheld.unknown.insert(
            SAVED_AT_THE_STATION.into(),
            Value::Object([(name.into(), json!(true))].into_iter().collect()),
        );
        keep_the_shacks_secret_settings(&shack, &mut withheld, None);
        assert_eq!(
            field(&mut withheld).as_str(),
            "FAKE-SHACK-KEY-0001",
            "a form read without {name} blanked it"
        );
        assert!(!withheld.unknown.contains_key(SAVED_AT_THE_STATION));
        keep_the_shacks_secret_settings(&shack, &mut save, None);
        assert_eq!(
            field(&mut save).as_str(),
            "",
            "CONTROL: {name} cleared at the shack"
        );
    }
}

/// ⛔ While a stream watches, the shack's own read blanks each of `SECRET_SETTINGS` and says by
/// value whether each is saved, as a read from afar does. CONTROL: unwatched, nothing is blanked.
#[test]
fn a_shack_read_while_a_stream_watches_blanks_each_stored_key() {
    for (name, field) in SECRET_SETTINGS {
        let mut shack = tempo_app::settings::Settings::default();
        *field(&mut shack) = "FAKE-SHACK-KEY-0001".into();
        let mut read = shack.clone();
        let saved = withhold_the_shacks_secret_settings(&mut read, None, true);
        assert_eq!(
            field(&mut read).as_str(),
            "",
            "{name} sent to a watched screen"
        );
        assert_eq!(saved.and_then(|s| s.get(name).copied()), Some(true));
        let mut own = shack.clone();
        assert_eq!(
            withhold_the_shacks_secret_settings(&mut own, None, false),
            None
        );
        assert_eq!(
            field(&mut own).as_str(),
            "FAKE-SHACK-KEY-0001",
            "CONTROL: {name}"
        );
    }
}

/// ⛔ A read from afar blanks each of `SECRET_SETTINGS` and says, by value, whether the shack has
/// it saved; the shack's own read blanks nothing and says nothing.
#[test]
fn a_settings_read_from_afar_blanks_each_stored_key_and_says_whether_it_is_saved() {
    let afar = Afar {
        stand_down: 0,
        starts_voided: 0,
    };
    for (name, field) in SECRET_SETTINGS {
        for held in ["FAKE-SHACK-KEY-0001", "", "   "] {
            let mut shack = tempo_app::settings::Settings::default();
            *field(&mut shack) = held.into();
            let mut read = shack.clone();
            let saved = withhold_the_shacks_secret_settings(&mut read, Some(&afar), false);
            assert_eq!(field(&mut read).as_str(), "", "from afar, {name} blanked");
            assert_eq!(
                saved.as_ref().and_then(|s| s.get(name)).copied(),
                Some(!held.trim().is_empty()),
                "from afar, {name} held {held:?}"
            );
            let mut own = shack.clone();
            assert_eq!(
                withhold_the_shacks_secret_settings(&mut own, None, false),
                None
            );
            assert_eq!(
                field(&mut own).as_str(),
                held,
                "CONTROL: the shack's own read"
            );
        }
    }
}

/// ★ The one list of stored keys is the list: each name is the wire name of the field it reaches,
/// and every setting named for a key, password, token or secret is on it or named here as none.
/// CONTROL: the sweep finds the ClubLog key by its name.
#[test]
fn every_stored_key_in_a_settings_file_is_on_the_one_list() {
    let mut probe = tempo_app::settings::Settings::default();
    for (name, field) in SECRET_SETTINGS {
        *field(&mut probe) = format!("probe-{name}");
    }
    let wire = serde_json::to_value(&probe).expect("settings serialize");
    for (name, _) in SECRET_SETTINGS {
        assert_eq!(
            wire.get(name).and_then(Value::as_str),
            Some(format!("probe-{name}").as_str()),
            "{name} is not the wire name of the field it reaches"
        );
    }
    let secret_named = |key: &str| {
        let k = key.to_ascii_lowercase();
        [
            "key", "password", "passwd", "token", "secret", "passcode", "code",
        ]
        .iter()
        .any(|w| k.ends_with(w))
    };
    // Named for one, and none: what a field holds, said by its own doc comment.
    const NOT_SECRET: [&str; 2] = [
        // The APRS symbol's character, which every beacon sends.
        "aprsSymbolCode",
        // The postal code of the contest entry's address, sent with every log submitted.
        "contestAddressPostalcode",
    ];
    let found: Vec<&String> = wire
        .as_object()
        .expect("an object")
        .iter()
        .filter(|(key, value)| value.is_string() && secret_named(key))
        .map(|(key, _)| key)
        .collect();
    let on_neither: Vec<&&String> = found
        .iter()
        .filter(|key| {
            !SECRET_SETTINGS.iter().any(|(n, _)| *n == key.as_str())
                && !NOT_SECRET.contains(&key.as_str())
        })
        .collect();
    assert!(
        on_neither.is_empty(),
        "named for a secret and on neither list: {on_neither:?}"
    );
    assert!(
        found.iter().any(|k| *k == "clublogApiKey"),
        "CONTROL: the sweep sees the ClubLog key, found {found:?}"
    );
}

/// The settings read takes `afar`, so the lane writes it there, and only the read.
#[test]
fn the_lane_tells_the_settings_read_it_comes_from_afar() {
    assert!(takes_afar("get_settings"));
    assert!(!takes_afar("get_snapshot"), "CONTROL");
}

/// ⛔ The whole save from afar goes through every keep: a transmit guard, an address a key is sent
/// to and a stored key each come back as the shack has them. CONTROL: the shack's own save is
/// taken as it came, and the rest of the save from afar is taken.
#[test]
fn a_whole_save_from_afar_goes_through_every_keep() {
    let shack = tempo_app::settings::Settings {
        cq_max_calls: Some(5),
        cloudlog_url: "https://log.example.net".into(),
        clublog_api_key: "FAKE-SHACK-KEY-0001".into(),
        mycall: "W9OLD".into(),
        ..Default::default()
    };
    let asked = tempo_app::settings::Settings {
        cq_max_calls: Some(9),
        cloudlog_url: "https://elsewhere.example.org".into(),
        clublog_api_key: "FAKE-OTHER-KEY-0002".into(),
        mycall: "W9NEW".into(),
        ..shack.clone()
    };
    let shown = |s: &tempo_app::settings::Settings| {
        (
            s.cq_max_calls,
            s.cloudlog_url.clone(),
            s.clublog_api_key.clone(),
            s.mycall.clone(),
        )
    };
    let mut save = asked.clone();
    keep_the_shacks_settings(
        &shack,
        &mut save,
        Some(&Afar {
            stand_down: 4,
            starts_voided: 0,
        }),
    );
    assert_eq!(
        shown(&save),
        (
            Some(5),
            "https://log.example.net".into(),
            "FAKE-SHACK-KEY-0001".into(),
            "W9NEW".into()
        ),
        "from afar"
    );
    let mut save = asked.clone();
    keep_the_shacks_settings(&shack, &mut save, None);
    assert_eq!(shown(&save), shown(&asked), "CONTROL: the shack's own save");
}

/// ⛔ The saved config profiles are a whole Settings each, stored keys included: a read from afar
/// carries none of them, and a save from afar keeps each profile's as the shack has it and cannot
/// plant one. CONTROL: the rest of a profile, and the shack's own keys, go and come as they were.
#[test]
fn a_saved_profile_carries_no_stored_key_toward_afar() {
    let shack_profiles = json!([
        { "name": "Home", "settings": { "clublogApiKey": "FAKE-HOME-KEY-0001", "mygrid": "EN52" }, "schema": 2 },
        { "name": "Portable", "settings": { "clublogApiKey": "", "mygrid": "EN61" } },
    ])
    .to_string();
    let shack = map(&[
        ("nexus.profiles", shack_profiles.as_str()),
        ("nexus-ui-scale-cap", "1.25"),
    ]);
    let read = station_answer(serde_json::to_value(&shack).unwrap());
    let text = read["nexus.profiles"]
        .as_str()
        .expect("the profiles are read");
    assert!(!text.contains("FAKE-HOME-KEY-0001"), "from afar: {text}");
    let profiles: Value = serde_json::from_str(text).unwrap();
    assert_eq!(profiles[0]["settings"]["clublogApiKey"], "");
    assert_eq!(
        profiles[0]["settings"]["mygrid"], "EN52",
        "CONTROL: the rest"
    );
    assert_eq!(profiles[0]["schema"], 2, "CONTROL: the rest");

    let sent_profiles = json!([
        { "name": "Home", "settings": { "clublogApiKey": "", "mygrid": "EN53" } },
        { "name": "Portable", "settings": { "clublogApiKey": "FAKE-PLANTED-0002", "mygrid": "EN61" } },
        { "name": "New", "settings": { "clublogApiKey": "FAKE-PLANTED-0003" } },
    ])
    .to_string();
    let saved = merged_save(
        shack.clone(),
        &json!({ "state": { "nexus.profiles": sent_profiles } }),
    )
    .expect("a map of strings");
    let text = saved["state"]["nexus.profiles"]
        .as_str()
        .expect("the profiles are saved");
    let profiles: Value = serde_json::from_str(text).unwrap();
    assert_eq!(
        [0, 1, 2].map(|i| profiles[i]["settings"]["clublogApiKey"].clone()),
        [json!("FAKE-HOME-KEY-0001"), json!(""), json!("")],
        "from afar: kept, not planted, not planted"
    );
    assert_eq!(
        profiles[0]["settings"]["mygrid"], "EN53",
        "CONTROL: the rest of the save"
    );
    assert_eq!(
        saved["state"]["nexus-ui-scale-cap"], "1.25",
        "CONTROL: the shack's own keys"
    );
}

/// The saved profiles of a `ui_state_save` admitted from afar onto a shack whose `ui-state.json`
/// holds `shack_profiles`: what the shack's file is then written with.
fn profiles_saved_from_afar(shack_profiles: &Value, sent_profiles: &Value) -> Value {
    let shack = shack();
    let text = shack_profiles.to_string();
    ui_now(&shack, &[("nexus.profiles", text.as_str())]);
    let base = shack.inner.stores.revision(Store::UiState).unwrap();
    let sent = json!({ "state": { "nexus.profiles": sent_profiles.to_string() } });
    match shack.admit(&controller(), save(1, "ui_state_save", sent, base.as_str())) {
        Admission::Run { args, .. } => serde_json::from_str(
            args["state"]["nexus.profiles"]
                .as_str()
                .expect("the profiles are saved"),
        )
        .unwrap(),
        other => panic!("{}", kind(&other)),
    }
}

/// ⛔ A SAVED PROFILE CARRIES NO STATION-ONLY SETTING IN FROM AFAR. A profile is a whole Settings,
/// and the shack's own screen applies what it holds when it is loaded there, as if typed there. A
/// profile saved from afar keeps the TX watchdog as the shack's own profile of that name holds it,
/// and one new from afar carries none, so loading it keeps the station's own (an absent key keeps
/// the current value, `mergeProfile`). The same for the program the shack runs to sign a LoTW
/// upload and the address its Cloudlog key goes to. CONTROL: the rest of each profile is the save's.
#[test]
fn a_profile_saved_from_afar_keeps_the_shacks_watchdog() {
    let home = serde_json::to_value(tempo_app::settings::Settings {
        tx_watchdog_min: 3,
        mygrid: "EN52".into(),
        ..Default::default()
    })
    .unwrap();
    let mut planted = home.clone();
    planted["txWatchdogMin"] = json!(0);
    planted["tqslPath"] = json!("C:\\Users\\Public\\not-tqsl.exe");
    planted["cloudlogUrl"] = json!("https://collector.example");
    planted["mygrid"] = json!("EN53");
    let saved = profiles_saved_from_afar(
        &json!([{ "name": "Home", "settings": home, "schema": 2 }]),
        &json!([
            { "name": "Home", "settings": planted, "schema": 2 },
            { "name": "New", "settings": { "txWatchdogMin": 0, "mygrid": "EN54" } },
        ]),
    );
    let home = &saved[0]["settings"];
    assert_eq!(
        (
            &home["txWatchdogMin"],
            &home["tqslPath"],
            &home["cloudlogUrl"]
        ),
        (&json!(3), &json!(""), &json!("")),
        "from afar: the shack's Home"
    );
    assert_eq!(
        saved[1]["settings"].get("txWatchdogMin"),
        None,
        "from afar: a new profile names no watchdog"
    );
    assert_eq!(
        (&home["mygrid"], &saved[1]["settings"]["mygrid"]),
        (&json!("EN53"), &json!("EN54")),
        "CONTROL: the rest of the save"
    );
}

/// ⛔ Every setting a save from afar keeps as the shack has it ([`STATION_ONLY_SETTINGS`]), inside a
/// saved profile, by value: one the shack's profile of that name holds comes back as the shack's,
/// one it does not hold is left out, and a profile new from afar carries none of them. CONTROL: the
/// rest of each profile is the save's.
#[test]
fn a_profile_saved_from_afar_keeps_every_station_only_setting() {
    let tagged = |tag: &str, keys: &[&str]| -> serde_json::Map<String, Value> {
        keys.iter()
            .map(|key| (key.to_string(), json!(format!("{tag} {key}"))))
            .chain([("mygrid".to_string(), json!(format!("{tag} grid")))])
            .collect()
    };
    // The shack's Home holds every station-only setting but the first, which it predates.
    let (predated, held) = STATION_ONLY_SETTINGS.split_first().unwrap();
    let saved = profiles_saved_from_afar(
        &json!([{ "name": "Home", "settings": tagged("shack", held) }]),
        &json!([
            { "name": "Home", "settings": tagged("afar", &STATION_ONLY_SETTINGS) },
            { "name": "New", "settings": tagged("afar", &STATION_ONLY_SETTINGS) },
        ]),
    );
    for key in held {
        assert_eq!(
            saved[0]["settings"][key],
            json!(format!("shack {key}")),
            "from afar: {key}"
        );
    }
    assert_eq!(saved[0]["settings"].get(predated), None, "{predated}");
    for key in STATION_ONLY_SETTINGS {
        assert_eq!(saved[1]["settings"].get(key), None, "a new profile: {key}");
    }
    assert_eq!(
        [
            &saved[0]["settings"]["mygrid"],
            &saved[1]["settings"]["mygrid"]
        ],
        [&json!("afar grid"), &json!("afar grid")],
        "CONTROL: the rest of the save"
    );
}

/// A profile saved at the shack is read from afar and saved back as it was read: it comes back
/// whole, every field of a whole Settings, its stored key and its station-only settings included.
/// What the shack's own screen saves never passes here (`ui_state_save`, at the shack, writes the
/// map as it came).
#[test]
fn a_profile_saved_at_the_shack_comes_back_whole_through_a_save_from_afar() {
    let home = serde_json::to_value(tempo_app::settings::Settings {
        tx_watchdog_min: 3,
        clublog_api_key: "FAKE-HOME-KEY-0003".into(),
        tqsl_path: "C:\\TrustedQSL\\tqsl.exe".into(),
        ..Default::default()
    })
    .unwrap();
    let shack_profiles = json!([{ "name": "Home", "settings": home, "schema": 2 }]);
    let shack_map = map(&[("nexus.profiles", shack_profiles.to_string().as_str())]);
    let read = station_answer(serde_json::to_value(&shack_map).unwrap());
    let read: Value = serde_json::from_str(read["nexus.profiles"].as_str().unwrap()).unwrap();
    assert_eq!(
        read[0]["settings"]["clublogApiKey"], "",
        "premise: read blank"
    );
    let saved = profiles_saved_from_afar(&shack_profiles, &read);
    assert_eq!(saved, shack_profiles);
}

/// Values the shack's screen reads in a saved profile list (`JSON.parse`) and the station does not
/// (serde_json).
fn values_only_the_screen_reads() -> [(&'static str, String); 3] {
    [
        ("a number past the largest float", "1e400".to_string()),
        ("a lone half of a surrogate pair", r#""\ud800""#.to_string()),
        (
            "nesting deeper than 128",
            format!("{}{}", "[".repeat(129), "]".repeat(129)),
        ),
    ]
}

/// A saved profile list of one profile, "Home", holding `settings`, with `pad` beside its name.
fn profile_list_with(pad: &str, settings: &Value) -> String {
    format!(r#"[{{"name":"Home","pad":{pad},"settings":{settings}}}]"#)
}

/// ⛔ A saved profile list from afar that the station cannot read is refused, and with it the whole
/// save, so nothing of it is written. The shack's screen reads more than the station does: a list
/// kept as sent would load there with every setting it holds, the station-only ones included.
/// CONTROL: the same list, readable, is saved with the shack's own watchdog and the rest of the save.
#[test]
fn a_profile_list_from_afar_the_station_cannot_read_is_refused() {
    let shack = shack();
    let own = json!([{ "name": "Home", "settings": { "txWatchdogMin": 6, "mygrid": "EN52" } }]);
    ui_now(
        &shack,
        &[
            ("nexus.profiles", own.to_string().as_str()),
            ("nexus.watchlist", "old"),
        ],
    );
    let base = shack.inner.stores.revision(Store::UiState).unwrap();
    let planted = json!({
        "txWatchdogMin": 0,
        "tqslPath": "C:\\Users\\Public\\not-tqsl.exe",
        "cloudlogUrl": "https://collector.example",
        "mygrid": "EN53",
    });
    let sent = |profiles: String| json!({ "state": { "nexus.profiles": profiles, "nexus.watchlist": "new" } });
    for (id, (shape, pad)) in (1..).zip(values_only_the_screen_reads()) {
        let profiles = profile_list_with(&pad, &planted);
        assert!(
            serde_json::from_str::<Value>(&profiles).is_err(),
            "premise: the station cannot read {shape}"
        );
        assert_eq!(
            kind(&shack.admit(
                &controller(),
                save(id, "ui_state_save", sent(profiles), base.as_str())
            )),
            refused(Refusal::BadArguments),
            "{shape}"
        );
    }
    let readable = sent(profile_list_with("1", &planted));
    match shack.admit(
        &controller(),
        save(9, "ui_state_save", readable, base.as_str()),
    ) {
        Admission::Run { args, .. } => {
            let saved: Value =
                serde_json::from_str(args["state"]["nexus.profiles"].as_str().unwrap()).unwrap();
            let home = &saved[0]["settings"];
            assert_eq!(
                (
                    &home["txWatchdogMin"],
                    home.get("tqslPath"),
                    &home["mygrid"],
                    &args["state"]["nexus.watchlist"]
                ),
                (&json!(6), None, &json!("EN53"), &json!("new")),
                "CONTROL: the shack's watchdog, and the rest of the save"
            );
        }
        other => panic!("{}", kind(&other)),
    }
}

/// ⛔ The shack's own saved profile list, when the station cannot read it, is never replaced from
/// afar: the station could keep neither its stored keys nor its station-only settings, so a save
/// carrying another list is refused. CONTROL: the shack's own list, sent back as it is, is written
/// as it is, byte for byte, with the rest of the save.
#[test]
fn a_shack_profile_list_the_station_cannot_read_is_not_replaced_from_afar() {
    for (shape, pad) in values_only_the_screen_reads() {
        let shack = shack();
        let own = profile_list_with(
            &pad,
            &json!({ "txWatchdogMin": 6, "clublogApiKey": "FAKE-HOME-KEY-0004" }),
        );
        ui_now(
            &shack,
            &[("nexus.profiles", own.as_str()), ("nexus.watchlist", "old")],
        );
        let base = shack.inner.stores.revision(Store::UiState).unwrap();
        let sent = |profiles: &str| json!({ "state": { "nexus.profiles": profiles, "nexus.watchlist": "new" } });
        let other = json!([{ "name": "Home", "settings": { "txWatchdogMin": 0 } }]).to_string();
        assert_eq!(
            kind(&shack.admit(
                &controller(),
                save(1, "ui_state_save", sent(&other), base.as_str())
            )),
            refused(Refusal::BadArguments),
            "{shape}"
        );
        match shack.admit(
            &controller(),
            save(2, "ui_state_save", sent(&own), base.as_str()),
        ) {
            Admission::Run { args, .. } => assert_eq!(
                args,
                json!({ "state": { "nexus.profiles": own, "nexus.watchlist": "new" } }),
                "CONTROL, {shape}"
            ),
            other => panic!("{shape}: {}", kind(&other)),
        }
    }
}

/// ⛔ A saved profile list the station cannot read stays at the shack: its stored keys cannot be
/// blanked, so a read from afar answers without it. CONTROL: the station's other keys are read.
#[test]
fn a_profile_list_the_station_cannot_read_is_not_sent_afar() {
    for (shape, pad) in values_only_the_screen_reads() {
        let own = profile_list_with(&pad, &json!({ "clublogApiKey": "FAKE-HOME-KEY-0005" }));
        let shack = map(&[("nexus.profiles", own.as_str()), ("nexus.watchlist", "old")]);
        let read = station_answer(serde_json::to_value(&shack).unwrap());
        assert!(
            !read.to_string().contains("FAKE-HOME-KEY-0005"),
            "{shape}: {read}"
        );
        assert_eq!(
            read,
            json!({ "nexus.watchlist": "old" }),
            "CONTROL, {shape}"
        );
    }
}

/// ⛔ …and a save from afar made from that read, which then carries no profile list, leaves the
/// shack's as it is, byte for byte: the other computer never had it, so its save can neither remove
/// nor rewrite it. CONTROL: the rest of the save is the other computer's.
#[test]
fn a_save_from_afar_keeps_a_profile_list_its_read_left_out() {
    for (shape, pad) in values_only_the_screen_reads() {
        let shack = shack();
        let own = profile_list_with(&pad, &json!({ "clublogApiKey": "FAKE-HOME-KEY-0006" }));
        ui_now(
            &shack,
            &[("nexus.profiles", own.as_str()), ("nexus.watchlist", "old")],
        );
        let base = shack.inner.stores.revision(Store::UiState).unwrap();
        let mut sent =
            station_answer(serde_json::to_value(lock(&shack.inner.stores.ui).clone()).unwrap());
        sent["nexus.watchlist"] = json!("new");
        match shack.admit(
            &controller(),
            save(1, "ui_state_save", json!({ "state": sent }), base.as_str()),
        ) {
            Admission::Run { args, .. } => assert_eq!(
                args,
                json!({ "state": { "nexus.profiles": own, "nexus.watchlist": "new" } }),
                "{shape}"
            ),
            other => panic!("{shape}: {}", kind(&other)),
        }
    }
}

/// Each setting a whole save from afar keeps as the shack has it, found by trying them all: every
/// setting the shack's file carries is changed alone in a save from afar, and it is kept when it
/// comes back as the shack's. Also what could not be changed alone (a list or a table).
fn what_a_save_from_afar_keeps() -> (std::collections::BTreeSet<String>, Vec<String>) {
    // The pending Cloudlog key is written only while it holds one.
    what_a_save_from_afar_keeps_of(tempo_app::settings::Settings {
        cloudlog_key: "FAKE-PENDING-KEY-0001".into(),
        ..Default::default()
    })
}

/// [`what_a_save_from_afar_keeps`] of the shack `shack`.
fn what_a_save_from_afar_keeps_of(
    shack: tempo_app::settings::Settings,
) -> (std::collections::BTreeSet<String>, Vec<String>) {
    use tempo_app::settings::Settings;
    let file = serde_json::to_value(&shack).unwrap();
    let mut kept = std::collections::BTreeSet::new();
    let mut untried = Vec::new();
    for (name, value) in file.as_object().unwrap() {
        let tries: Vec<Value> = match value {
            Value::Bool(b) => vec![json!(!b)],
            Value::Number(n) => {
                let n = n.as_f64().unwrap();
                vec![json!(n + 1.0), json!(n - 1.0), json!(n * 2.0 + 3.0)]
                    .into_iter()
                    .chain([json!(n as u64 + 1), json!(n as u64 + 3)])
                    .collect()
            }
            Value::Null => vec![
                json!(1),
                json!(2.5),
                json!(true),
                json!("probe"),
                json!([]),
                json!({}),
            ],
            Value::String(s) => {
                // A choice of a few names says which, when it refuses another.
                let mut asked = file.clone();
                asked[name] = json!("probe");
                let others = match serde_json::from_value::<Settings>(asked) {
                    Err(e) => e
                        .to_string()
                        .split('`')
                        .skip(3)
                        .step_by(2)
                        .filter(|v| v != s)
                        .map(|v| json!(v))
                        .collect(),
                    Ok(_) => Vec::new(),
                };
                [json!(format!("{s}probe"))]
                    .into_iter()
                    .chain(others)
                    .collect()
            }
            Value::Array(_) | Value::Object(_) => Vec::new(),
        };
        let took = tries.into_iter().find_map(|changed| {
            let mut asked = file.clone();
            asked[name] = changed;
            let asked: Settings = serde_json::from_value(asked).ok()?;
            (serde_json::to_value(&asked).unwrap()[name] != *value).then_some(asked)
        });
        let Some(mut save) = took else {
            untried.push(name.clone());
            continue;
        };
        keep_the_shacks_settings(
            &shack,
            &mut save,
            Some(&Afar {
                stand_down: 1,
                starts_voided: 0,
            }),
        );
        if serde_json::to_value(&save).unwrap()[name] == *value {
            kept.insert(name.clone());
        }
    }
    (kept, untried)
}

/// ⛔ What the read from afar says is set at the station is exactly what the save from afar keeps:
/// every setting kept is flagged, so the form never takes an edit the save quietly puts back, and
/// nothing flagged is a setting the save takes. Only a list or a table goes untried, and none of
/// those is flagged.
#[test]
fn every_setting_a_save_from_afar_keeps_is_set_at_the_station() {
    let (kept, untried) = what_a_save_from_afar_keeps();
    let flagged: std::collections::BTreeSet<String> = station_only_settings(
        Some(&Afar {
            stand_down: 1,
            starts_voided: 0,
        }),
        &tempo_app::settings::Settings::default(),
    )
    .unwrap()
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(
        kept.difference(&flagged).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "kept, but not set at the station"
    );
    assert_eq!(
        flagged.difference(&kept).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "set at the station, but the save takes it"
    );
    assert!(
        kept.contains("txWatchdogMin") && kept.contains("clublogApiKey"),
        "CONTROL: the probe finds a guard and a stored key: {kept:?}"
    );
    assert!(!kept.contains("mycall"), "CONTROL: the call is the save's");
    let file = serde_json::to_value(tempo_app::settings::Settings::default()).unwrap();
    let untried_values: Vec<(&String, &Value)> = untried
        .iter()
        .filter(|name| !file[name.as_str()].is_array() && !file[name.as_str()].is_object())
        .map(|name| (name, &file[name.as_str()]))
        .collect();
    assert_eq!(untried_values, Vec::new(), "could not be changed alone");
    for name in &untried {
        assert!(!flagged.contains(name), "{name} is flagged but untried");
    }
    assert_eq!(
        station_only_settings(None, &tempo_app::settings::Settings::default()),
        None,
        "at the shack nothing is set elsewhere"
    );
}

/// ⛔ While the shack has "My station" profiles, its call is set at the station: a whole save from
/// afar keeps the shack's call, and the read from afar says so, beside every other setting it keeps.
/// The call it would have written is no edit of the profile the shack is on. CONTROL: a shack with
/// no profiles takes the call from afar as before (`every_setting_a_save_from_afar_keeps_is_set_at_the_station`).
#[test]
fn with_station_profiles_the_call_is_set_at_the_station() {
    // The pending Cloudlog key is written only while it holds one (`what_a_save_from_afar_keeps`).
    let mut shack = tempo_app::settings::Settings {
        mycall: "KD9TAW".into(),
        cloudlog_key: "FAKE-PENDING-KEY-0001".into(),
        ..Default::default()
    };
    shack.ensure_station_profiles();
    assert!(
        !shack.station_profiles.is_empty(),
        "premise: Home is written"
    );
    let (kept, _) = what_a_save_from_afar_keeps_of(shack.clone());
    let afar = Afar {
        stand_down: 1,
        starts_voided: 0,
    };
    let flagged: std::collections::BTreeSet<String> = station_only_settings(Some(&afar), &shack)
        .unwrap()
        .into_iter()
        .map(String::from)
        .collect();
    assert!(kept.contains("mycall"), "the save from afar keeps the call");
    assert_eq!(
        kept.symmetric_difference(&flagged).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "kept and flagged are the same"
    );
    let mut save = shack.clone();
    save.mycall = "W9ABC".into();
    save.mygrid = "EN53".into();
    keep_the_shacks_settings(&shack, &mut save, Some(&afar));
    let mut e = tempo_app::engine::Engine::with_settings(shack.clone());
    e.apply_settings(save);
    assert_eq!(
        (
            e.settings().mycall.as_str(),
            e.settings().station_profiles[0].call.as_str(),
            e.settings().mygrid.as_str(),
        ),
        ("KD9TAW", "KD9TAW", "EN53"),
        "the call stays the shack's; CONTROL: the grid is the save's"
    );
}

/// The list the UI's tests render the form with is the one the read from afar sends.
#[test]
fn the_ui_fixture_carries_the_station_only_list() {
    let fixture: Vec<String> = serde_json::from_str(include_str!(
        "../../../ui/src/components/__fixtures__/stationOnlySettings.json"
    ))
    .unwrap();
    let sent: Vec<String> = station_only_settings(
        Some(&Afar {
            stand_down: 1,
            starts_voided: 0,
        }),
        &tempo_app::settings::Settings::default(),
    )
    .unwrap()
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(fixture, sent);
}

/// The read's "set at the station" is no setting: a save sends it back, and it is never kept into
/// the file, from afar or at the shack.
#[test]
fn a_save_never_writes_the_set_at_the_station_list() {
    for afar in [
        Some(Afar {
            stand_down: 2,
            starts_voided: 0,
        }),
        None,
    ] {
        let shack = tempo_app::settings::Settings::default();
        let mut save = shack.clone();
        save.unknown
            .insert(SET_AT_THE_STATION.into(), json!(["txWatchdogMin"]));
        keep_the_shacks_settings(&shack, &mut save, afar.as_ref());
        assert!(save.unknown.get(SET_AT_THE_STATION).is_none(), "{afar:?}");
    }
}

#[test]
fn allow_to_transmit_is_refused_from_afar_so_a_held_device_never_allows_itself() {
    // A device added by its own sign-in is held from transmit until the operator allows it at the
    // shack. That press is `remote_station_action` with this action: from another computer it is
    // refused at both ends, by the controller too, so the held device can never lift its own hold.
    let args = json!({ "action": { "type": "allowTransmit", "deviceId": "dev-held" } });
    let action: crate::remote_service::Action =
        serde_json::from_value(args["action"].clone()).expect("the press as the screen sends it");
    assert!(
        matches!(&action, crate::remote_service::Action::AllowTransmit { device_id } if device_id == "dev-held"),
        "premise: these are the arguments of Allow to transmit"
    );
    assert_eq!(class_of("remote_station_action"), Some(Class::RefusedHere));
    assert_eq!(
        route("remote_station_action", &args),
        Route::Refused(Refusal::ShackOnly),
        "it is never sent"
    );
    assert_eq!(
        crate::client_mode::way("remote_station_action", &args),
        crate::client_mode::Way::Refused(Refusal::ShackOnly),
        "client mode refuses it on the other computer"
    );
    let shack = shack();
    let admission = shack.admit(&controller(), call(1, "remote_station_action", args));
    assert_eq!(
        kind(&admission),
        refused(Refusal::ShackOnly),
        "the controlling device at the shack"
    );
    // The control: the same controller is admitted for a command that runs at the shack, so the
    // refusal above is the class's, not a caller the dispatcher turns away anyway.
    let control = shack.admit(&controller(), call(2, "get_snapshot", json!({})));
    assert_eq!(kind(&control), "run");
}
