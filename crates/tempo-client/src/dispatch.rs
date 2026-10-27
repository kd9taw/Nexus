//! What both ends of a command link route and admit a call by, read the same way at each: the
//! computer driving a station asks [`route`] before it sends a call, and the shack's dispatcher
//! (src-tauri's `command_dispatch`, whose header has the rules in full) looks the same tables up
//! again before it admits one. "Here" is the computer the operator sits at; "the shack" is the one
//! with the radio.
//!
//! - **The refusals** ([`Refusal`]), by their codes on the wire, and **what became of a call**
//!   ([`Outcome`]).
//! - **Where a command goes** from this computer ([`route`]), by its class ([`crate::classes`]):
//!   deny by default, a password typed here refused ([`argument_refusal`]), and the ui-state pair
//!   split by key ([`split_ui_state`], [`join_ui_state`], [`STATION_KEYS`]).
//! - **The starts wired from afar** ([`WIRED`], [`transmit_from_afar`]) and **Stop's rules**
//!   ([`is_stop`]: a command that stops a transmission, or the off half of a transmit switch,
//!   [`OFF_HALVES`]), and which of those only unkey ([`is_unkey`]).
//! - **The stores** a whole save replaces ([`STORES`]), **the events that cross** ([`forwards`]),
//!   **the kinds of client** ([`Profile`]), and all of it as **the contract's text**
//!   ([`contract_text`]), so two builds that differ in any table are two builds.

use crate::classes::{class_of, Class};
use serde_json::Value;
use std::collections::BTreeMap;

/// Why a call was refused. Its [`code`](Refusal::code) is what the other computer is told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The command has no class. Refused at both ends.
    Unclassed,
    /// The command runs on the computer the operator sits at, never at the shack.
    RunsHere,
    /// The command is given only from the shack's own screen.
    ShackOnly,
    /// The command starts a transmission that is not wired for another computer ([`WIRED`]).
    TransmitNotYet,
    /// `set_ptt` or `hold_ptt_lock` sent from another computer: its PTT, and its Lock, is a held
    /// press on Stop's road, never the command, which keys the shack's own audio source.
    HeldPress,
    /// A start from a connection with no Stop road: Stop would have one road only.
    NoStopRoad,
    /// A start while the computer's Stop road holds no live presence.
    NoPresence,
    /// A start while the newest heartbeat on the Stop road named shack state older than two seconds.
    StaleState,
    /// A password typed on the other computer: credentials are set and used at the shack.
    TypedPassword,
    /// Another device holds station control.
    NotController,
    /// Over a cap on calls in flight.
    Busy,
    /// Over the cap on calls a second.
    RateLimited,
    /// An id already used, whose outcome is no longer kept. It is never run again.
    Replayed,
    /// A whole save made from a copy older than the shack's, or naming no copy at all.
    ChangedAtShack,
    /// Arguments that are not what the command is invoked with.
    BadArguments,
    /// A start pressed before a Stop, or a halt at the station, that has since been taken: it never
    /// ran. Stop wins over a start that was in flight (the shack's `command_dispatch::still_bound`).
    Stopped,
}

impl Refusal {
    /// Every refusal, for reading one back from its code.
    pub const ALL: [Refusal; 16] = [
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

    /// The refusal a code names, as the other computer reads it back.
    pub fn named(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|refusal| refusal.code() == code)
    }

    /// The refusal's name on the wire.
    pub fn code(self) -> &'static str {
        match self {
            Refusal::Unclassed => "unclassed",
            Refusal::RunsHere => "runsHere",
            Refusal::ShackOnly => "shackOnly",
            Refusal::TransmitNotYet => "transmitNotYet",
            Refusal::HeldPress => "heldPress",
            Refusal::NoStopRoad => "noStopRoad",
            Refusal::NoPresence => "noPresence",
            Refusal::StaleState => "staleState",
            Refusal::TypedPassword => "typedPassword",
            Refusal::NotController => "notController",
            Refusal::Busy => "busy",
            Refusal::RateLimited => "rateLimited",
            Refusal::Replayed => "replayed",
            Refusal::ChangedAtShack => "changedAtShack",
            Refusal::BadArguments => "badArguments",
            Refusal::Stopped => "stopped",
        }
    }
}

// --- The other computer's end ------------------------------------------------------------------

/// Where the computer the operator sits at sends a command its screens make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// Run it on this computer.
    Here,
    /// Send it to the shack.
    Shack,
    /// The ui-state pair: the station's keys go to and come from the shack, every other key stays
    /// on this computer ([`split_ui_state`], [`join_ui_state`]).
    Split,
    /// Refuse it on this computer; nothing is sent.
    Refused(Refusal),
}

/// The route for `cmd`, invoked with `args`. A command with no class is refused, as is one only the
/// shack's own screen may give and an argument that may not leave this computer.
pub fn route(cmd: &str, args: &Value) -> Route {
    let Some(class) = class_of(cmd) else {
        return Route::Refused(Refusal::Unclassed);
    };
    match class {
        Class::RunsHere => Route::Here,
        Class::RefusedHere => Route::Refused(Refusal::ShackOnly),
        Class::RunsAtShack | Class::StartsTx | Class::StopsTx => {
            if let Some(refusal) = argument_refusal(cmd, args) {
                Route::Refused(refusal)
            } else if UI_STATE.contains(&cmd) {
                Route::Split
            } else {
                Route::Shack
            }
        }
    }
}

/// A refusal read off a command's arguments, made at both ends so that what it refuses never
/// leaves the other computer. One today: `winlink_connect` with a password typed on the other
/// computer (the operator's ruling of 2026-10-07, "Shack's stored password"). An empty password is
/// the command's own "use the one in the keychain", so the session still runs at the shack with the
/// password stored there; anything else is refused. The value is only tested for being empty:
/// never kept, logged, compared or echoed, and the refusal carries none of it.
pub fn argument_refusal(cmd: &str, args: &Value) -> Option<Refusal> {
    match cmd {
        "winlink_connect" => match args.get("password") {
            Some(Value::String(typed)) if typed.is_empty() => None,
            _ => Some(Refusal::TypedPassword),
        },
        _ => None,
    }
}

/// How a transmission start wired for another computer goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wired {
    /// One of the desktop's FT buttons. None carries a time: slot timing, the clock, sequencing,
    /// its timeouts and the auto-log stay at the shack.
    Ft,
    /// The shack's voice keyer playing one of its slots: a message recorded on the other computer
    /// reaches the slot as an import, and plays here as one recorded here does.
    VoiceKeyer,
    /// The PTT. From another computer never this command, which keys the shack's own audio
    /// source: a held press on Stop's road, re-asserted by that computer's screen, that arms its
    /// microphone over; its voice keys it.
    HeldPress,
    /// Phone's Lock (hands-free PTT): the same held press, re-asserted by that computer's screen
    /// with no finger on it, and ended there at a cap unless the operator answers "Still talking?"
    /// (`crate::phone_lock`). On Stop's road it is a held press and nothing else, so every rule of
    /// one holds at the shack.
    LockedPress,
    /// One of the JS8 cockpit's starts: a message, a command, a CQ or a heartbeat now; more text for
    /// the message going out; a switch (the automatic replies, the heartbeat, relay, heartbeat acks,
    /// heartbeat networking) or the repeating CQ turned on; and the yes to an automatic reply. None carries a time the shack keys by: its JS8 periods, its queue and
    /// every JS8 gate there decide when, and none of them turns transmit on.
    Js8,
    /// Keyboard CW or a CW macro, as text. The shack's keyer times every element; there is no
    /// paddle from afar.
    Cw,
    /// The tune carrier, or the rig's own tuner. The carrier keeps the shack's own ceiling, and its
    /// off half is a Stop ([`OFF_HALVES`]).
    Tune,
    /// One of the RTTY cockpit's starts: a send, continuous TX switched on and typed into, and the
    /// auto-sequencer's CQ and answer. The shack re-checks every transmit gate on each tick of
    /// continuous TX and holds it to its own ceiling; the sequencer's timing stays there.
    Rtty,
    /// One of the PSK cockpit's starts: a send, and continuous TX switched on and typed into, under
    /// the same per-tick gates as RTTY's.
    Psk,
    /// One of the JTTY cockpit's starts: a send, an F-key, live entry started and typed into, and
    /// the sign-off. The shack's JTTY gate decides each when it is planned and again when it is
    /// queued, and the radio loop re-checks every transmit gate on each frame; a live session keeps
    /// its own ceiling and the watchdog there.
    Jtty,
    /// APRS: a beacon or a message, which the shack renders and keys behind its own APRS gates, or
    /// Monitor armed by hand, which lets the shack's automatic ack key. That ack ends when the
    /// computer that armed it is no longer there (src-tauri's `command_dispatch::PRESENCE_BOUND`).
    Aprs,
    /// An SSTV picture, sent as its pixels. The shack burns its ID in, encodes it and keys it
    /// behind its own SSTV gates.
    Sstv,
    /// Chat, Nexus to Nexus: a message, a broadcast, a message sent again, the CQ run turned on or
    /// resumed, and the presence beacon turned on. The shack's slots decide when each keys. The CQ
    /// run and the beacon end when the computer that turned them on is no longer there
    /// (`command_dispatch::PRESENCE_BOUND`).
    Chat,
}

/// ⛔ THE TRANSMISSION STARTS WIRED FOR ANOTHER COMPUTER, and how each goes; every other start is
/// refused, by name, at both ends. In name order. The FT rows are exactly the desktop's FT buttons
/// that start a transmission, the Operate strip's Call CQ and contest S&P among them; its other FT
/// buttons key nothing and run at the shack by their class. The JS8 rows are the cockpit's sends,
/// more text for the message going out, every `js8_arm` switch and the repeating CQ turned on, and
/// the yes to an automatic reply. The CW rows are keyboard CW and the macros, sent as text for the
/// shack's keyer. The tune rows are the tune carrier and the rig's own tuner; the RTTY and PSK rows
/// are the two keyboard cockpits' sends, continuous TX switched on and typed into, and RTTY's auto
/// CQ and answer. The JTTY rows are its cockpit's send, F-keys, live entry started and typed into,
/// and sign-off. Continuous TX, JTTY's live entry and an auto run go on transmitting after the
/// press, so each ends with the rest when the computer's presence lapses (the shack's halt ends the
/// latch, the live session and the run).
/// The APRS, SSTV and chat rows are each cockpit's sends and what it turns on that transmits by
/// itself, which ends with the presence of the computer that turned it on. Phone's PTT and its Lock
/// are each a held press on Stop's road, never a command at the shack.
pub const WIRED: &[(&str, Wired)] = &[
    ("aprs_arm", Wired::Aprs),
    ("aprs_send_beacon", Wired::Aprs),
    ("aprs_send_message", Wired::Aprs),
    ("atu_tune", Wired::Tune),
    ("broadcast", Wired::Chat),
    ("call_cq", Wired::Ft),
    ("call_station", Wired::Ft),
    ("hold_ptt_lock", Wired::LockedPress),
    ("js8_answer_reply", Wired::Js8),
    ("js8_arm", Wired::Js8),
    ("js8_call_cq", Wired::Js8),
    ("js8_cq_repeat", Wired::Js8),
    ("js8_heartbeat_now", Wired::Js8),
    ("js8_send", Wired::Js8),
    ("js8_send_command", Wired::Js8),
    ("js8_typeahead", Wired::Js8),
    ("jtty_fkey", Wired::Jtty),
    ("jtty_live_commit", Wired::Jtty),
    ("jtty_live_start", Wired::Jtty),
    ("jtty_send", Wired::Jtty),
    ("jtty_sign_off", Wired::Jtty),
    ("override_next_tx", Wired::Ft),
    ("play_voice_message", Wired::VoiceKeyer),
    ("psk_send", Wired::Psk),
    ("psk_set_latched", Wired::Psk),
    ("psk_type", Wired::Psk),
    ("qso_freetext", Wired::Ft),
    ("qso_resend", Wired::Ft),
    ("resend_chat", Wired::Chat),
    ("resume_chat_cq", Wired::Chat),
    ("rtty_auto_answer", Wired::Rtty),
    ("rtty_auto_cq", Wired::Rtty),
    ("rtty_send", Wired::Rtty),
    ("rtty_set_latched", Wired::Rtty),
    ("rtty_type", Wired::Rtty),
    ("send_cw", Wired::Cw),
    ("send_cw_armed", Wired::Cw),
    ("send_message", Wired::Chat),
    ("set_beacon", Wired::Chat),
    ("set_chat_cq", Wired::Chat),
    ("set_mode", Wired::Ft),
    ("set_ptt", Wired::HeldPress),
    ("set_tune", Wired::Tune),
    ("set_tx_enabled", Wired::Ft),
    ("sstv_send", Wired::Sstv),
    ("start_cq", Wired::Ft),
    ("start_running", Wired::Ft),
    ("start_search_and_pounce", Wired::Ft),
];

/// ⛔ THE ONE SWITCH FOR A TRANSMISSION STARTED FROM ANOTHER COMPUTER: how the start `cmd` goes
/// from afar, or the refusal of one that is not wired. Every refusal of one reads it: the shack's
/// own class check (`command_dispatch::Dispatcher::admit`), the other computer's routing
/// (`client_mode::way`), a held press on Stop's second road (`remote_service::lan::stop_road`), and
/// a press's microphone on the audio road (`remote_service::lan::audio_road`), the last two asking
/// for `set_ptt`'s held press. Past it, a start still needs the stream's transmit rules
/// (`command_dispatch::Caller::roads`).
pub fn transmit_from_afar(cmd: &str) -> Result<Wired, Refusal> {
    WIRED
        .iter()
        .find(|(name, _)| *name == cmd)
        .map(|&(_, wired)| wired)
        .ok_or(Refusal::TransmitNotYet)
}

/// Whether the held press is wired from afar: what Stop's road asks of a hold, and the audio road
/// of a press's microphone.
pub fn press_from_afar() -> bool {
    transmit_from_afar("set_ptt") == Ok(Wired::HeldPress)
}

// --- Stop's rules --------------------------------------------------------------------------------

/// How the off half of a transmit switch reads in its argument.
#[derive(Clone, Copy, Debug)]
pub enum Off {
    /// A `bool` argument that is `false`.
    False,
    /// A string argument naming one of these values.
    OneOf(&'static [&'static str]),
}

/// The off half of each transmit switch: the command, the argument the app invokes it with that
/// says on or off, and how "off" reads in it. The off half runs under Stop's rules; anything else in
/// that argument, a missing one included, is the on half, under the command's class. A test holds
/// each argument to the command's own signature, and fails on a transmit command with an on/off
/// argument and no row here.
pub const OFF_HALVES: &[(&str, &str, Off)] = &[
    ("aprs_arm", "on", Off::False),
    ("js8_arm", "on", Off::False),
    ("js8_cq_repeat", "on", Off::False),
    ("psk_set_latched", "on", Off::False),
    // It runs at the shack either way: turning the sequencer on keys nothing, but turning it off
    // ends a live session, so the off half is a stop.
    ("rtty_set_auto", "on", Off::False),
    ("rtty_set_latched", "on", Off::False),
    ("set_beacon", "on", Off::False),
    ("set_chat_cq", "on", Off::False),
    // A run mode ("qso-run", "fieldday-run") calls CQ; these three are passive on entry, and
    // entering one ends a run and drops the queue.
    (
        "set_mode",
        "mode",
        Off::OneOf(&["chat", "qso-monitor", "fieldday-sp"]),
    ),
    ("set_ptt", "on", Off::False),
    ("set_tune", "on", Off::False),
    ("set_tx_enabled", "enabled", Off::False),
];

/// Arguments that read as the off half of `cmd`, when it is a transmit switch: for the other
/// computer's tests, which hold its routing to Stop's rules.
#[cfg(any(test, feature = "test-util"))]
pub fn off_args(cmd: &str) -> Option<Value> {
    OFF_HALVES
        .iter()
        .find(|(name, ..)| *name == cmd)
        .map(|(_, arg, off)| match off {
            Off::False => serde_json::json!({ *arg: false }),
            Off::OneOf(values) => serde_json::json!({ *arg: values[0] }),
        })
}

/// Whether a call runs under Stop's rules, so the other computer sends it ahead of everything.
pub fn is_stop(cmd: &str, args: &Value) -> bool {
    runs_as_stop(class_of(cmd), cmd, args)
}

/// Whether a call runs under Stop's rules: its command stops a transmission, or it is the off half
/// of a transmit switch.
pub fn runs_as_stop(class: Option<Class>, cmd: &str, args: &Value) -> bool {
    class == Some(Class::StopsTx) || is_unkey(cmd, args)
}

/// ⛔ Whether a call under Stop's rules only unkeys: the off half of a transmit switch
/// ([`OFF_HALVES`]). An unkey goes by the link alone, ahead of everything, and ends its own over at
/// once, as at the shack's own screen. It voids every start from afar the station took before it,
/// whichever computer made it (the counter is the station's), as a Stop does, but halts nothing
/// else: what other starts from afar began goes on. A command that stops a transmission is a Stop,
/// by both roads, and halts everything at the station.
pub fn is_unkey(cmd: &str, args: &Value) -> bool {
    OFF_HALVES.iter().any(|(name, arg, off)| {
        *name == cmd
            && match (off, args.get(*arg)) {
                (Off::False, Some(Value::Bool(false))) => true,
                (Off::OneOf(values), Some(Value::String(value))) => {
                    values.contains(&value.as_str())
                }
                _ => false,
            }
    })
}

// --- Stores and their revisions ------------------------------------------------------------------

/// A store a whole save replaces, and whose reads answer with its revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Store {
    /// The shack's settings.
    Settings,
    /// The station's keys of `ui-state.json` ([`STATION_KEYS`]).
    UiState,
}

/// How a command meets its store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// It reads the store whole.
    Read,
    /// It replaces the store whole: what it sends is what the other computer's copy held, so a
    /// copy older than the shack's would put back what changed since.
    WholeSave,
    /// It replaces one part of the store ([`parts`]) with what the other computer's copy held, and
    /// is checked on that part alone.
    PartSave,
}

/// The commands that read a store whole or replace it, whole or a part. The list saves each replace
/// one part of the settings with a list the screen took from them.
pub const STORES: &[(&str, Store, Access)] = &[
    ("get_settings", Store::Settings, Access::Read),
    ("set_blocked_calls", Store::Settings, Access::PartSave),
    ("set_psk_macros", Store::Settings, Access::PartSave),
    ("set_radio_bands", Store::Settings, Access::PartSave),
    ("set_routing_rules", Store::Settings, Access::PartSave),
    ("set_rtty_macros", Store::Settings, Access::PartSave),
    ("set_settings", Store::Settings, Access::WholeSave),
    ("ui_state_load", Store::UiState, Access::Read),
    ("ui_state_save", Store::UiState, Access::WholeSave),
    ("update_radio_profile", Store::Settings, Access::PartSave),
];

/// The store `cmd` reads or saves, and whether it saves it: what the other computer keeps the
/// revision of, and sends a save with.
pub fn store_access(cmd: &str) -> Option<(Store, bool)> {
    STORES
        .iter()
        .find(|(name, ..)| *name == cmd)
        .map(|&(_, store, access)| (store, access != Access::Read))
}

/// The ui-state pair, which splits by key at both ends.
const UI_STATE: &[&str] = &["ui_state_load", "ui_state_save"];

// --- The one mixed store -------------------------------------------------------------------------

/// The keys of `ui-state.json` that hold the station's own data: its memory channels, its watch
/// list, the satellite and DXpedition chase sets and their alarms, and its settings profiles. They
/// live at the shack. Every other key (the UI scale's cap, the rail's order, which modes the rail
/// shows, the main window's pane layout) belongs to the computer the operator sits at and stays
/// there. This is `DURABLE_KEYS` in `ui/src/features/durableStore.ts` split in two, and a test fails
/// on a durable key that is in neither half.
pub const STATION_KEYS: &[&str] = &[
    "nexus.dxped.alarms",
    "nexus.dxped.alarms.fired",
    "nexus.dxped.chasing",
    "nexus.memory.bank.v1",
    "nexus.memory.bank.v2",
    "nexus.profiles",
    "nexus.sats.alarms",
    "nexus.sats.alarms.fired",
    "nexus.sats.chasing",
    "nexus.watchlist",
];

/// A ui-state map split in two: the station's keys, and every other key.
pub fn split_ui_state(
    map: BTreeMap<String, String>,
) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    map.into_iter()
        .partition(|(key, _)| STATION_KEYS.contains(&key.as_str()))
}

/// One map from two: the station's keys from `station`, every other key from `rest`. The other
/// computer's screen loads the shack's station keys joined to its own other keys, so a station key
/// in its own store, an old copy, gives way; and a save from afar writes the station keys it sent
/// joined to the shack's own other keys.
pub fn join_ui_state(
    station: BTreeMap<String, String>,
    rest: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut joined = split_ui_state(rest).1;
    joined.extend(split_ui_state(station).0);
    joined
}

// --- Events --------------------------------------------------------------------------------------

/// The shack's events that another computer is sent, and the only ones it takes in: the alert
/// (`crate::alert`), made once where it is raised (the pounce detector's, the decode alerts', the
/// new parks'), the app's one push (everything else polls), so both computers' screens hear it under
/// the same id. Any other event stays where it was raised.
pub const FORWARDED_EVENTS: &[&str] = &[crate::alert::EVENT];

/// Whether the event `name` crosses between the two computers.
pub fn forwards(name: &str) -> bool {
    FORWARDED_EVENTS.contains(&name)
}

/// The kind of client a computer's Nexus is, as its hello names it. This build serves the desktop
/// alone; a phone is a later build's, with what it may call written here first, so a new kind is a
/// new rule in the dispatcher and leaves how a computer is admitted as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// The desktop app, the same build at both ends: every classed command, by its class.
    Desktop,
    /// A phone's Nexus. Known by name, so a station recognises one, and refused: this build serves
    /// it nothing ([`Profile::served`]), and the shack tells it to update the station.
    Mobile,
}

impl Profile {
    /// The profile a hello names, or `None` for a name this build does not know.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "desktop" => Some(Profile::Desktop),
            "mobile" => Some(Profile::Mobile),
            _ => None,
        }
    }

    /// Its name in the hello.
    pub fn name(self) -> &'static str {
        match self {
            Profile::Desktop => "desktop",
            Profile::Mobile => "mobile",
        }
    }

    /// Whether this build opens a lane for it. Only the served profiles are in the build's
    /// contract, so naming a refused one changes nothing a desktop of another build compares.
    pub fn served(self) -> bool {
        match self {
            Profile::Desktop => true,
            Profile::Mobile => false,
        }
    }
}

/// What became of a call, as the journal keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Admitted, and not finished yet.
    Running,
    /// It ran, and succeeded.
    Done,
    /// It ran, and failed with the command's own error.
    Failed(String),
    /// It was refused, and nothing ran.
    Refused(Refusal),
    /// It was admitted, and its result never came back.
    Lost,
    /// Nothing is known of it: never seen, no longer kept, or the shack restarted since. The
    /// screen says to check, and never guesses.
    Unknown,
}

/// This dispatcher's tables as text, for the build's contract ([`crate::wire::contract`]): every
/// command's class, the starts wired from afar, the off halves, the stores and how each command
/// meets them, the station's ui-state keys, the events that cross and the profiles served.
pub fn contract_text() -> String {
    use std::fmt::Write;
    let mut text = String::new();
    for (cmd, class) in crate::classes::classes() {
        let _ = writeln!(text, "class {cmd} {class:?}");
    }
    for (cmd, wired) in WIRED {
        let _ = writeln!(text, "wired {cmd} {wired:?}");
    }
    for (cmd, arg, off) in OFF_HALVES {
        let _ = writeln!(text, "off {cmd} {arg} {off:?}");
    }
    for (cmd, store, access) in STORES {
        let _ = writeln!(text, "store {cmd} {store:?} {access:?}");
    }
    for key in STATION_KEYS {
        let _ = writeln!(text, "station {key}");
    }
    for event in FORWARDED_EVENTS {
        let _ = writeln!(text, "event {event}");
    }
    let _ = writeln!(text, "profile {}", Profile::Desktop.name());
    text
}
