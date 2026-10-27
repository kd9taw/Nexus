//! Client mode: this computer's Nexus driving a paired station's over this network, with its own
//! station off. "Here" is the computer the operator sits at; "the shack" is the one with the radio.
//!
//! The operator picks a paired station in Stations on this network and presses Operate from this
//! computer. Nexus quits as itself, the way closing its window quits (the radio stopped first, the
//! logbook's last changes saved), and starts again with `--client <station id>` ([`ARG`]); Leave
//! starts it as itself again. A start, never a switch in place, so no half-stopped radio loop is
//! left behind.
//!
//! ## No station here
//!
//! A launch in client mode branches off in `run` (`lib.rs`) before anything of this computer's own
//! station is opened: no settings opened for writing, no logbook, no Engine, no radio loop, no
//! rigctld, no station audio device, no CAT broker, no WSJT-X UDP, no cluster or spot feed, no
//! upload and no listener. The app [`run`] builds manages no Engine at all, so a command that
//! reached this computer's own handler could find none: it would fail, and never run on a station
//! here. What it does open is this computer's: its windows, the keys of its own `ui-state.json`
//! that are not the station's, the two fields of its `settings.json` that belong to it, the update
//! channel and start at sign-in, and This computer ▸ Audio's choice in a file of its own,
//! `client-audio.json` ([`ThisComputer`]). Its headset (`crate::lan_client::audio_road`) opens this
//! computer's own output and microphone only while the shack's audio, a press, the meter or the
//! test wants them, and keys nothing. A test holds the module to the short list of the crate's own
//! functions it may call, each of which starts nothing of a station.
//!
//! ## Every command by its class, failing closed
//!
//! The app's own handler is wrapped ([`Client::handle`]), so every IPC call the screens make passes
//! here, whichever window made it. [`way`] decides, from `command_dispatch::route` and the class
//! table:
//! - **here**: windows, links, notifications, the files this computer saves, its updater and street
//!   map, through the app's own handler;
//! - **the shack**: every station command, over the command link, with the revision of the copy a
//!   save was made from; at most [`IN_FLIGHT`] at once and [`PER_COMMAND`] of one command, the rest
//!   waiting here for a slot ([`Caps`]);
//! - **answered here**: the snapshot, the waterfall's rows and the meters, from what the shack
//!   pushes, so the screens' polls never cross the network; the update channel and start at
//!   sign-in, which are this computer's; the launch picker's question, which is about which radio
//!   this Nexus drives, and so is never the shack's picker; and the voice keyer's recorder, which
//!   records this computer's microphone and sends the message to the shack's slot as an import
//!   (`import_voice_message`), where the shack's voice keyer plays it, and which is refused while
//!   a press is held, as the PTT is while a message is recorded;
//! - **split**: the ui-state pair, the station's keys at the shack and every other key here (the
//!   operator's ruling of 2026-10-07: the rail, its order, the UI scale and the window layouts stay
//!   on the computer the operator sits at);
//! - **refused here**, with nothing sent: a command with no class, one only the shack's own screen
//!   may give, a password typed here, and every command that starts a transmission but the few
//!   wired for this road (`command_dispatch::WIRED`). The shack refuses each of those again itself.
//!
//! ## A transmission started from here
//!
//! A wired start leaves only while both roads are up and presence is live as this computer's Stop
//! road last heard it; the shack judges it again by the stream's transmit rules, on its own clock.
//! The PTT is never the shack's `set_ptt`, which keys the shack's own audio source: each `set_ptt`
//! on from the screen, which re-asserts it every 100 ms while its PTT is held, is one hold of a
//! held press on Stop's road ([`Client::press`]) while presence is live as that road last heard it,
//! and its off half lets the press go first. A press that ended other than by the screen letting go
//! (a Stop, the station, a new link, presence lapsing here) never keys again: the screen must let
//! go and press again. So after a reconnect nothing transmits by
//! itself: the lease is taken again, and the shack's transmit stays off until it is armed.
//!
//! ## Stop is never held
//!
//! A Stop, and the off half of a transmit switch, takes no slot and waits for none, and the link
//! writes it ahead of everything waiting. With no link it cannot reach the shack, and says so at
//! once rather than waiting for one.
//!
//! ⛔ Stop wins over a start that was in flight. A start pressed here reads how many Stops (a Stop
//! press, or the off half of a switch) have been pressed so far; one still waiting here for a slot
//! when another is pressed is refused `stopped` and never sent ([`Client::at_the_shack`]). The shack
//! voids a start it read before a Stop the same way (`command_dispatch::still_bound`). And every
//! call carries that count, a Stop its own number (the one its second road carries too), and every
//! hold of a press the count it began under, so a Stop that overtakes a start on the way, by the
//! second road or ahead of it on the link, still wins: the shack refuses a start or a press made
//! before a Stop it has taken.
//!
//! A Stop press (a command of the stop class) also goes by Stop's second road
//! (`crate::lan_client::stop_road`): the handler writes it on that road's socket itself, before it
//! hands the call to the link, and returns. The road's own thread sends it again until the shack
//! acknowledges it. Each link's road outlives its link by a minute, so a Stop pressed as a link
//! drops still goes. The road's heartbeat names the last snapshot the main window fetched, kept here
//! per window, so a window that hangs lets presence lapse at the shack. The Stop press also ends
//! this computer's own press, after the Stop is written and with an atomic store, so the audio road
//! (`crate::lan_client::audio_road`) stops sending without the Stop ever waiting for it.
//!
//! ## The link and the lease
//!
//! [`Client::drive`] keeps the link: it connects, takes the lease when control is free, keeps it
//! with a heartbeat every [`HEARTBEAT`], watches the feeds, and hands the shack's events to the
//! screens. It connects where the station answered before, the last address that worked first,
//! and only when none answers (or another key does) where a look by name finds it, as the
//! stations window does. When the link drops, the screens keep the shack's last state ("Not live"), and it
//! connects again by itself. A call cut off by a drop is never sent again: the next link asks the
//! shack what became of it. A station kept by its service id (signed in to from elsewhere, ruling
//! 556) has no record on this network, so its internet road is its only road until this computer
//! first reaches it there and signs in on its row; the banner's status says so (`away`).
//!
//! ## A test build's road switch
//!
//! Built with the `bench-road-switch` feature only (`crate::bench_road_switch`): one switch here
//! blocks the command link or Stop's road at this computer's own transport, so the bench can try
//! each road alone. Every link [`Client::drive`] opens is opened under it, and the banner's status
//! says where it is. Nothing in this module decides anything by it: the rules above meet a blocked
//! road as they would meet a firewall on this computer.
//!
//! ## Signing in again
//!
//! A station added with the account takes this computer on its network for thirty days from the
//! sign-in that added it (`crate::lan_client::account`). In the last week of them, Operate's launch
//! signs in again by itself ([`sign_in_again_by`]): the browser opens for it while the link runs,
//! and the new proof goes to the station on a connection of its own, so the link is never closed for
//! it. The banner says so, and, when it did not happen, by what day to sign in again.
//!
//! ## What a refusal says
//!
//! A call refused here or at the shack fails with `nexusClient:<code>`, a refusal's code or one of
//! this module's (`notLive`, `gone`, `tooLarge`), and the screen words it in its own language
//! (`ui/src/api.ts`). A command's own error comes back as it would at the shack.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::net::SocketAddrV4;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::ipc::{Invoke, InvokeBody, InvokeError};
use tauri::{Emitter, Manager, Runtime};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::command_classes::{class_of, Class};
use crate::command_dispatch::{
    is_stop, join_ui_state, route, split_ui_state, store_access, transmit_from_afar, Refusal,
    Route, Store, Wired, IN_FLIGHT, PER_COMMAND,
};
use crate::lan_client::audio_road::{
    self, Headset, Recording, ThisHeadset, CUSHION_MS, INTERNET_CUSHION_MS,
};
use crate::lan_client::link::{run_id, Answer, Link};
use crate::lan_client::road::{self, Look};
use crate::lan_client::stop_road::{Over, StopRoad};
use crate::remote_service::vault::PairedStation;
use tempo_client::phone_lock::{self, Locked};
use tempo_client::still_there::{Idle, IdleWatch};
use tempo_stream::lan::{Lookup, Typed};

#[cfg(test)]
pub(crate) mod tests;

/// The argument that starts Nexus in client mode, with the paired station's id after it.
pub(crate) const ARG: &str = "--client";
/// The argument after [`ARG`] that gives the address the operator typed for the station, tried
/// first ([`typed_in`]).
const AT_ARG: &str = "--client-at";
/// How often the lease is renewed: well inside the shack's five seconds, so a heartbeat refused
/// once while the shack's engine is busy costs nothing.
const HEARTBEAT: Duration = Duration::from_secs(1);
/// How long a link that dropped waits before it is tried again, the first time.
const AGAIN: Duration = Duration::from_secs(2);
/// …and every time after that, until it opens: the shack takes at most ten new connections a
/// minute from one address.
const AGAIN_AFTER: Duration = Duration::from_secs(7);
/// How long a station that refused this computer for a reason that will not pass by itself (the
/// other build, a key that changed, a pairing it no longer knows) waits before it is asked again.
const AGAIN_REFUSED: Duration = Duration::from_secs(30);
/// The reasons a link does not open that only the operator can change.
const LASTING: [&str; 7] = [
    "updateStation",
    "updateComputer",
    "otherBuild",
    "keyChanged",
    "notPaired",
    "notThisStation",
    "notStation",
];
/// The refusals only a new pairing ends: the station no longer knows this computer, or answers
/// with another key. After one, a cold launch does not drive that station again by itself; it
/// offers to pair again with it instead ([`pair_again`]).
const PAIR_AGAIN: [&str; 2] = ["notPaired", "keyChanged"];
/// How long the main window waits for the shack's state before it shows without it.
const SHOW_BY: Duration = Duration::from_secs(10);
/// The feeds the screens' polls are answered from.
const FEEDS: [&str; 4] = ["snapshot/1", "spectrumRow/1", "meters/1", "rttyScope/1"];
/// The most ids one question after outcomes may carry (the shack's own bound).
const ASK_AT_ONCE: usize = 64;
/// How long a look for the station by name listens for answers: the stations window's own wait.
const LOOK_FOR: Duration = Duration::from_secs(3);
/// Where a settings read's revision goes to the screen, beside the settings it read; `api.ts`
/// takes it off and keeps it with that copy.
const READ_REVISION: &str = "nexusRevision";

/// The station this launch drives (`--client <id>` or `--client=<id>`), or none. Only an id a
/// station could have is taken: anything else starts Nexus as itself.
pub(crate) fn requested() -> Option<String> {
    requested_in(std::env::args().skip(1))
}

/// [`requested`], from the arguments given.
fn requested_in(mut args: impl Iterator<Item = String>) -> Option<String> {
    let mut found = None;
    while let Some(arg) = args.next() {
        if arg == ARG {
            found = args.next();
            break;
        } else if let Some(id) = arg.strip_prefix("--client=") {
            found = Some(id.to_string());
            break;
        }
    }
    found.filter(|id| tempo_stream::protocol::identifier(id))
}

/// The arguments that start Nexus driving the station `station`, at the address or host name
/// `typed` first when the operator typed one.
pub(crate) fn operate_args(station: String, typed: Option<Typed>) -> Vec<String> {
    let mut args = vec![ARG.into(), station];
    if let Some(at) = typed {
        args.extend([AT_ARG.into(), at.to_string()]);
    }
    args
}

/// The address or host name the operator typed for the station this launch drives
/// (`--client-at <address>` or `--client-at=<address>`), or none. Only what the stations window
/// would take is taken.
pub(crate) fn typed_in(mut args: impl Iterator<Item = String>) -> Option<Typed> {
    while let Some(arg) = args.next() {
        let typed = if arg == AT_ARG {
            args.next()
        } else if let Some(at) = arg.strip_prefix("--client-at=") {
            Some(at.to_string())
        } else {
            continue;
        };
        return typed.and_then(|text| Typed::read(&text));
    }
    None
}

// --- This computer's last client target ---------------------------------------------------------

/// This computer's last client target: the station it last drove and the address the operator
/// typed for it, kept so a COLD launch (the icon, with no `--client` argument) connects again to
/// that station, exactly as the warm `--client` relaunch does. A cold launch would otherwise boot
/// as this computer's own station, dropping the operator out of the shack they were driving.
///
/// It lives in a file of its own beside the DEFAULT profile's settings ([`target_path`]), as the
/// paired stations do ([`crate::lan_client::typed_path`]): the station it names is this computer's,
/// not one radio profile's, and it is read before any window or profile is opened. It is NOT a
/// `ui-state.json` "here" key — those are the rail, scale and window layout the operator sits in
/// front of (the ruling of 2026-10-07), a different kind of state read later and per profile.
const TARGET_FILE: &str = "client-target.json";

/// Where [`TARGET_FILE`] lives: beside the default profile's settings, where the paired stations
/// it names are kept.
pub(crate) fn target_path() -> PathBuf {
    crate::config_dir_for(None).join(TARGET_FILE)
}

/// The last client target as it is kept on disk.
#[derive(Debug, Serialize, Deserialize)]
struct Target {
    /// The station's id (a sound [`tempo_stream::protocol::identifier`], or the target is ignored).
    station: String,
    /// The address or host name the operator typed for it (`--client-at`), as [`Typed`] writes it;
    /// absent when none was typed (the station is found by its record or by name instead).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    at: Option<String>,
    /// Why the station last refused this computer, when that was one of [`PAIR_AGAIN`]; cleared by
    /// the next link that opens ([`note_refusal_at`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refused: Option<String>,
}

/// Read the kept target at `path`: `None` when there is no sound one — no file, one this build
/// cannot read, or an id no station could have.
fn read_target_at(path: &Path) -> Option<Target> {
    let text = std::fs::read_to_string(path).ok()?;
    let target: Target = serde_json::from_str(&text).ok()?;
    tempo_stream::protocol::identifier(&target.station).then_some(target)
}

/// Keep `station` and the address typed for it as this computer's last client target, so a cold
/// launch connects to it again. Written as this computer ENTERS client mode ([`run`]), so it names
/// the station actually driven. A write that does not land is logged, not fatal: the only cost is
/// that a cold launch does not reconnect.
fn keep_target_at(path: &Path, station: &str, typed: Option<&Typed>) {
    write_target_at(
        path,
        &Target {
            station: station.to_string(),
            at: typed.map(Typed::to_string),
            refused: None,
        },
    );
}

fn write_target_at(path: &Path, target: &Target) {
    match serde_json::to_string_pretty(target) {
        Ok(text) if crate::write_json_atomic(path, &text) => {}
        _ => tempo_core::applog::warn(
            "client",
            "could not keep the last client target; a cold launch will not reconnect to it",
        ),
    }
}

/// Note beside the kept target at `path` why its station `station` last refused this computer,
/// when that is one of [`PAIR_AGAIN`], or that a link to it opened (`None`), which clears the note.
/// Any other refusal leaves the note as it is: it says nothing of the pairing. A target that names
/// another station is left as it is.
fn note_refusal_at(path: &Path, station: &str, refused: Option<&str>) {
    if refused.is_some_and(|reason| !PAIR_AGAIN.contains(&reason)) {
        return;
    }
    let Some(mut target) = read_target_at(path) else {
        return;
    };
    if target.station != station || target.refused.as_deref() == refused {
        return;
    }
    target.refused = refused.map(str::to_string);
    write_target_at(path, &target);
}

/// Forget this computer's last client target, so a cold launch boots as this computer's own
/// station. Called as Leave hands the station back ([`leave_client_mode`]).
fn clear_target_at(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tempo_core::applog::warn(
            "client",
            &format!("could not clear the last client target: {e}"),
        ),
    }
}

/// [`keep_target_at`] in this computer's own [`target_path`].
pub(crate) fn keep_target(station: &str, typed: Option<&Typed>) {
    keep_target_at(&target_path(), station, typed);
}

/// [`clear_target_at`] in this computer's own [`target_path`].
pub(crate) fn clear_target() {
    clear_target_at(&target_path());
}

/// Resolve a kept target to the station a cold launch drives: the station id and the typed address
/// when `known` says this computer still knows it, else `None` — and always `None` in a radio window
/// (`profile`), which drives its own radio. Split from the store read and the
/// knownness query so the by-value tests meet it here.
fn resolve_target(
    profile: Option<&str>,
    target: Option<Target>,
    known: impl FnOnce(&str) -> bool,
) -> Option<(String, Option<Typed>)> {
    // A radio window (`--profile`) is the operator choosing one of this computer's own radios from
    // the picker: it drives that radio, never the station the default window last drove.
    if profile.is_some() {
        return None;
    }
    let target = target?;
    // The station last refused this computer in a way only a new pairing ends: never driven again
    // by itself, so the operator is not dropped into a refusing client mode on every start.
    if target.refused.is_some() {
        return None;
    }
    known(&target.station).then(|| {
        let typed = target.at.as_deref().and_then(Typed::read);
        (target.station, typed)
    })
}

/// Whether this computer still knows station `id`: a sound LAN record (paired, or paired by code),
/// or a station kept by its service id (signed in to from elsewhere). These are the stations the
/// stations window lists, so a target whose station the operator removed resolves to none.
fn station_known(stations: &crate::lan_client::Stations, id: &str) -> bool {
    stations.get(id).ok().flatten().is_some() || stations.away(id).ok().flatten().is_some()
}

/// The station a COLD launch (no `--client`) drives, with the address typed for it: this computer's
/// last client target, when one is kept and its station is still known here. `None` — boot as this
/// computer's own station — when there is no kept target, or its station is no longer paired here
/// (the operator removed it, or Leave cleared the target).
pub(crate) fn auto_requested() -> Option<(String, Option<Typed>)> {
    let stations =
        crate::lan_client::Stations::new(Arc::new(crate::remote_service::vault::SystemVault))
            .typed_in(crate::lan_client::typed_path())
            .signed_in(crate::lan_client::signed_path());
    resolve_target(
        crate::active_profile(),
        read_target_at(&target_path()),
        |id| station_known(&stations, id),
    )
}

/// The station to offer to pair again with, once, on a launch that is not in client mode: the kept
/// target's, when its station last refused this computer in a way only a new pairing ends
/// ([`PAIR_AGAIN`]) and is still known here. The target is then forgotten, as Leave forgets it,
/// so the offer is made once and the next cold launch boots as this computer's own station. Never
/// in a radio window (`profile`).
fn pair_again_at(
    path: &Path,
    profile: Option<&str>,
    known: impl FnOnce(&str) -> bool,
) -> Option<String> {
    if profile.is_some() {
        return None;
    }
    let target = read_target_at(path).filter(|target| target.refused.is_some())?;
    if !known(&target.station) {
        return None;
    }
    clear_target_at(path);
    tempo_core::applog::info(
        "client",
        &format!(
            "the station this computer last drove refused it ({}): offering to pair again \
             instead of connecting",
            target.refused.as_deref().unwrap_or_default()
        ),
    );
    Some(target.station)
}

/// [`pair_again_at`] in this computer's own [`target_path`], for the window that opens: the
/// Stations on this network window, offering Pair again for that station.
pub(crate) fn pair_again() -> Option<String> {
    let stations =
        crate::lan_client::Stations::new(Arc::new(crate::remote_service::vault::SystemVault))
            .typed_in(crate::lan_client::typed_path())
            .signed_in(crate::lan_client::signed_path());
    pair_again_at(&target_path(), crate::active_profile(), |id| {
        station_known(&stations, id)
    })
}

// --- Where each command goes -------------------------------------------------------------------

/// Where a command goes from this computer in client mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Way {
    /// Run here, through the app's own handler.
    Here,
    /// Answered here by client mode itself.
    Local(Local),
    /// Sent to the shack. A Stop goes ahead of everything and takes no slot.
    Shack { stop: bool },
    /// A transmission start wired for this road: sent to the shack, under the caps, only while
    /// this computer's own roads say a start may go.
    Start,
    /// The PTT held: one hold of a held press on Stop's road, never the shack's `set_ptt`.
    Press,
    /// Phone's Lock: one hold of the same held press, bounded here by its cap
    /// (`tempo_client::phone_lock`).
    Lock,
    /// The ui-state pair: the station's keys at the shack, every other key here.
    Split,
    /// Refused here, and nothing is sent.
    Refused(Refusal),
}

/// What client mode answers itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Local {
    /// The shack's newest snapshot, from its feed.
    Snapshot,
    /// A waterfall row from its feed, each one once to each window.
    SpectrumRow,
    /// The shack's meters, from their feed.
    Meters,
    /// The shack's RTTY tuning scope, its newest frame from its feed (the frame's own `seq` tells
    /// the screen a frame it has drawn).
    RttyScope,
    /// This computer's update channel.
    BetaUpdates,
    /// Start this computer's Nexus at sign-in.
    LaunchAtLogin,
    /// Before this computer's updater hands over to its installer: nothing of a station's to
    /// save here, only this computer's window layouts and its diagnostic log.
    BeforeInstall,
    /// This computer's Nexus started again to finish its update, in client mode again.
    Restart,
    /// The launch picker's question: which radio this Nexus drives. A computer driving another
    /// station drives none of its own, so it never shows the shack's picker, whose choices would
    /// change the shack.
    RadioLaunchInfo,
    /// The voice keyer's Record: this computer's microphone, from now.
    RecordStart,
    /// The voice keyer's Stop & save: what was recorded here, sent to the shack's slot as an
    /// import, and the shack's answer.
    RecordStop,
    /// The voice keyer's recording thrown away: nothing is sent.
    RecordCancel,
}

/// The way for `cmd` invoked with `args`. `command_dispatch::route` first, deny by default: a
/// command it refuses never goes anywhere. Then what client mode does itself.
pub(crate) fn way(cmd: &str, args: &Value) -> Way {
    match route(cmd, args) {
        Route::Refused(refusal) => Way::Refused(refusal),
        Route::Split => Way::Split,
        Route::Here => match cmd {
            // This computer's, and kept in its own settings.json: the app's handler for them
            // reaches for an Engine, which client mode never has.
            "set_beta_updates" => Way::Local(Local::BetaUpdates),
            "set_launch_at_login" => Way::Local(Local::LaunchAtLogin),
            // The app's own handlers for these two save a logbook and stop a radio first.
            "prepare_update_install" => Way::Local(Local::BeforeInstall),
            "restart_app" => Way::Local(Local::Restart),
            // The app's own recorder records the station's audio input; here the voice keyer
            // records this computer's microphone.
            "start_voice_recording" => Way::Local(Local::RecordStart),
            "stop_voice_recording" => Way::Local(Local::RecordStop),
            "cancel_voice_recording" => Way::Local(Local::RecordCancel),
            _ => Way::Here,
        },
        Route::Shack => {
            let stop = is_stop(cmd, args);
            // A start leaves this computer only when it is wired for this road, and the shack
            // refuses every other one too, by the same switch. The off half of a switch is a Stop,
            // and goes.
            if !stop && class_of(cmd) == Some(Class::StartsTx) {
                return match transmit_from_afar(cmd) {
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
                };
            }
            match cmd {
                "get_snapshot" => Way::Local(Local::Snapshot),
                "get_spectrum_row" => Way::Local(Local::SpectrumRow),
                "get_meters" => Way::Local(Local::Meters),
                "get_rtty_scope" => Way::Local(Local::RttyScope),
                "radio_launch_info" => Way::Local(Local::RadioLaunchInfo),
                _ => Way::Shack { stop },
            }
        }
    }
}

/// A refusal, or another reason a call failed here, as the screen reads it.
pub(crate) fn word(code: &str) -> String {
    format!("nexusClient:{code}")
}

// --- The caps ----------------------------------------------------------------------------------

/// The caps on calls to the shack: [`IN_FLIGHT`] at once from this run, [`PER_COMMAND`] of one
/// command, as the shack's dispatcher counts them. A call over either waits here for a slot, in
/// the order the calls came, so the shack never has to answer `busy`. A Stop takes none.
pub(crate) struct Caps {
    in_flight: Arc<Semaphore>,
    per_command: Mutex<HashMap<String, Arc<Semaphore>>>,
}

/// A call's place under the caps, given back when it is dropped.
pub(crate) struct Slot {
    _of_command: OwnedSemaphorePermit,
    _in_flight: OwnedSemaphorePermit,
}

impl Caps {
    pub(crate) fn new() -> Self {
        Self {
            in_flight: Arc::new(Semaphore::new(IN_FLIGHT)),
            per_command: Mutex::new(HashMap::new()),
        }
    }

    /// A slot for one call of `cmd`, waited for: its command's first, so a call waiting behind
    /// others of its own command holds none of the slots every command shares.
    pub(crate) async fn slot(&self, cmd: &str) -> Slot {
        let mine = lock(&self.per_command)
            .entry(cmd.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(PER_COMMAND)))
            .clone();
        let of_command = mine.acquire_owned().await.expect("a cap is never closed");
        let in_flight = Arc::clone(&self.in_flight)
            .acquire_owned()
            .await
            .expect("a cap is never closed");
        Slot {
            _of_command: of_command,
            _in_flight: in_flight,
        }
    }
}

// --- The shack, as client mode reaches it ------------------------------------------------------

/// A call's answer, on its way.
pub(crate) type Answering = Pin<Box<dyn Future<Output = (u64, Answer)> + Send>>;

/// Where client mode finds the paired station's record by its id: this computer's store in the
/// app, the test's own in a test. The store can block, so it is asked off the async threads.
pub(crate) type Find =
    Arc<dyn Fn(&str) -> Result<Option<PairedStation>, &'static str> + Send + Sync>;

/// Where client mode reads, and keeps, the address or host name the operator typed for the station
/// once Operate opened there, so the next launch tries it first: this computer's typed file in the
/// app (`lan_client::Stations::typed`), never the credential entry that holds the station's keys.
/// Each can block, so it is asked off the async threads.
pub(crate) trait Keep: Send + Sync + 'static {
    /// What is kept for the station paired here as `id`, if anything.
    fn typed(&self, id: &str) -> Option<Typed>;
    /// Keep `typed` for the station paired here as `id`.
    fn keep(&self, id: &str, typed: &Typed) -> Result<(), &'static str>;
}

impl Keep for crate::lan_client::Stations {
    fn typed(&self, id: &str) -> Option<Typed> {
        crate::lan_client::Stations::typed(self, id)
    }
    fn keep(&self, id: &str, typed: &Typed) -> Result<(), &'static str> {
        self.keep_typed(id, Some(typed))
    }
}

/// The internet road, as client mode opens it when the station is not on this network: the Remote
/// service's pipe through the station's room and its relay in the app (`lan_client::internet`), a
/// stand-in in a test. Each is asked by the station's id here, and reads the road's record itself.
pub(crate) trait Afar: Send + Sync + 'static {
    /// Whether an internet road is kept for the station paired here as `id`. The store can block,
    /// so it is asked off the async threads.
    fn has_road(&self, id: &str) -> bool;
    /// The command link through the station's room, its internet roads beside it, its audio on
    /// `headset` when this computer has one: or why it did not open.
    fn open(
        &self,
        id: String,
        headset: Option<Arc<dyn Headset>>,
    ) -> Pin<Box<dyn Future<Output = Result<Link, &'static str>> + Send>>;
    /// The pipe's minute renewed, or why not.
    fn renew(&self, id: String) -> Pin<Box<dyn Future<Output = Result<(), &'static str>> + Send>>;
    /// The station paired here as `id` named `key` as its key for the internet road, on this
    /// network's road, which is pinned to its key here: a road kept for it pinned to another key is
    /// turned off. The store can block, so it is told off the async threads.
    fn heard_key(&self, id: &str, key: &str);
}

/// The internet road's clocks. A test shortens them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Timing {
    /// How long an internet link's Stop road may take to find a pair before this network is said
    /// to block the datagram roads.
    pub(crate) udp_grace: Duration,
    /// How often, while on the internet, the station's addresses on this network are tried.
    pub(crate) lan_probe: Duration,
    /// "Still there?" and the end after it, by the operator's input (N5).
    pub(crate) idle_prompt: Duration,
    pub(crate) idle_end: Duration,
    /// Phone's Lock: its cap, and how long before it "Still talking?" is asked.
    pub(crate) lock_cap: Duration,
    pub(crate) lock_ask: Duration,
    /// How often the pipe is renewed.
    pub(crate) renew: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            udp_grace: UDP_GRACE,
            lan_probe: LAN_PROBE,
            idle_prompt: tempo_client::still_there::PROMPT,
            idle_end: tempo_client::still_there::END,
            lock_cap: tempo_client::phone_lock::CAP,
            lock_ask: tempo_client::phone_lock::ASK_BEFORE,
            renew: crate::lan_client::internet::RENEW_EVERY,
        }
    }
}

/// How long an internet Stop road has to find a pair: ICE's own checks finish well inside it.
const UDP_GRACE: Duration = Duration::from_secs(10);
/// How often the station's addresses on this network are tried while on the internet.
const LAN_PROBE: Duration = Duration::from_secs(30);
/// The longest wait between those tries, after one that found the station but could not open.
const LAN_PROBE_MOST: u32 = 10;
/// How long each of the station's addresses has to answer a try.
const LAN_PROBE_WAIT: Duration = Duration::from_secs(1);

/// The shack, as client mode reaches it: the command link in the app, a stand-in in a test.
pub(crate) trait Shack: Send + Sync + 'static {
    /// One call, with the revision of the copy a save was made from and how many Stops had been
    /// pressed here when it was made, a Stop counting itself: its id, and what became of it.
    fn call(
        self: Arc<Self>,
        cmd: String,
        args: Value,
        base: Option<String>,
        stops: u64,
    ) -> Answering;
    /// A feed's newest value, with its number.
    fn latest(&self, feed: &str) -> Option<(u64, Value)>;
    /// This link's Stop road, when the shack opened one.
    fn stop_road(&self) -> Option<Arc<StopRoad>> {
        None
    }
}

impl Shack for Link {
    fn call(
        self: Arc<Self>,
        cmd: String,
        args: Value,
        base: Option<String>,
        stops: u64,
    ) -> Answering {
        Box::pin(async move { Link::call(&self, &cmd, args, base.as_deref(), stops).await })
    }

    fn latest(&self, feed: &str) -> Option<(u64, Value)> {
        Link::latest(self, feed)
    }

    fn stop_road(&self) -> Option<Arc<StopRoad>> {
        Link::stop_road(self)
    }
}

// --- This computer's own ------------------------------------------------------------------------

/// What client mode keeps of this computer's own: the two settings that are this computer's
/// (`betaUpdates`, `launchAtLogin`), shown in place of the shack's and changed only here; This
/// computer ▸ Audio's headset; and where its own files are.
pub(crate) struct ThisComputer {
    /// This computer's `settings.json`. Only the two fields are ever written to it, and only when
    /// the operator changes one.
    settings: PathBuf,
    /// This computer's `ui-state.json`: the keys that are not the station's.
    ui_state: PathBuf,
    beta_updates: bool,
    launch_at_login: bool,
    /// This computer's `client-audio.json`, beside its `settings.json`: the headset for the shack's
    /// audio, written only when the operator picks one, and never sent anywhere.
    audio: PathBuf,
    /// The output and the microphone chosen there, "" for the system's default.
    output: String,
    input: String,
    /// The station's audio here, in dB over its own level: the hosted page's volume.
    volume_db: f32,
}

impl ThisComputer {
    /// This computer's, read from its own files. The settings are read as JSON and never loaded
    /// whole: a load could set a file it cannot read aside, and client mode writes nothing it was
    /// not asked to.
    pub(crate) fn read(settings: PathBuf, ui_state: PathBuf) -> Self {
        let json = |path: &PathBuf| -> Value {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default()
        };
        let read = json(&settings);
        let audio = settings.with_file_name(AUDIO_FILE);
        let chosen = json(&audio);
        let device = |which: &str| {
            chosen[which]
                .as_str()
                .filter(|name| name.len() <= DEVICE_NAME)
                .unwrap_or_default()
                .to_string()
        };
        Self {
            beta_updates: read["betaUpdates"].as_bool().unwrap_or(false),
            launch_at_login: read["launchAtLogin"].as_bool().unwrap_or(false),
            output: device("output"),
            input: device("input"),
            volume_db: volume(chosen["volumeDb"].as_f64()),
            settings,
            ui_state,
            audio,
        }
    }

    /// Keeps This computer ▸ Audio's choice in this computer's own file.
    fn keep_audio(&mut self, output: &str, input: &str, volume_db: f32) -> Result<(), String> {
        let chosen = json!({ "output": output, "input": input, "volumeDb": volume_db });
        let text = serde_json::to_string_pretty(&chosen).map_err(|e| e.to_string())?;
        if !crate::write_json_atomic(&self.audio, &text) {
            return Err("audioNotKept".to_string());
        }
        self.output = output.to_string();
        self.input = input.to_string();
        self.volume_db = volume_db;
        Ok(())
    }

    /// The shack's settings with this computer's own two in place of the shack's.
    fn over(&self, settings: &mut Value) {
        if let Some(fields) = settings.as_object_mut() {
            fields.insert("betaUpdates".into(), json!(self.beta_updates));
            fields.insert("launchAtLogin".into(), json!(self.launch_at_login));
        }
    }

    /// Keeps one of the two in this computer's `settings.json`, through the file's own writer.
    fn keep(&mut self, which: Local, on: bool) -> Result<(), String> {
        tempo_app::settings::writer::SettingsWriter::of(&self.settings)
            .rewrite(|s| {
                match which {
                    Local::BetaUpdates => s.beta_updates = on,
                    _ => s.launch_at_login = on,
                }
                true
            })
            .map_err(|e| e.to_string())?;
        match which {
            Local::BetaUpdates => self.beta_updates = on,
            _ => self.launch_at_login = on,
        }
        Ok(())
    }
}

/// This computer ▸ Audio's own file, beside this computer's `settings.json`.
const AUDIO_FILE: &str = "client-audio.json";

/// A volume as kept or asked for, in whole dB inside the hosted page's range; anything unreadable
/// is the page's default, never silence or a blast.
fn volume(db: Option<f64>) -> f32 {
    match db.filter(|db| db.is_finite()) {
        Some(db) => (db.round() as f32).clamp(audio_road::VOLUME_DB_MIN, audio_road::VOLUME_DB_MAX),
        None => audio_road::VOLUME_DB,
    }
}
/// The longest device name kept: a system's own names are far shorter.
const DEVICE_NAME: usize = 512;

// --- The client ----------------------------------------------------------------------------------

/// How the link to the station stands, for the screens' banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Phase {
    /// Not reached yet.
    Connecting,
    /// This computer holds control and the shack's state is arriving.
    Live,
    /// The link dropped; the screens hold the shack's last state while it connects again.
    NotLive,
    /// Another device holds control of the station.
    Busy,
    /// The station would not take this computer: the reason says why.
    Refused,
}

/// The link's state as the banner shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    /// Where the station answered, or was last known to: its address and port.
    pub(crate) station: String,
    pub(crate) phase: Phase,
    /// For a refusal, or a link that did not open: its code.
    pub(crate) reason: Option<String>,
    /// This computer's over, as the station last said it on Stop's road: on the air with its holes,
    /// ended and why, or no word while a press is held.
    pub(crate) over: Option<Over>,
    /// Why a PTT press would not key the station right now, lifted from the station's snapshot
    /// (`radio.pttRefusal`, the first gate `Engine::arm_remote_mic` would hit): `None` when a press
    /// would, or did, key. DISPLAY ONLY — the Phone PTT shows it so a press that did nothing is
    /// explained; nothing here keys, refuses or releases.
    pub(crate) ptt_refusal: Option<String>,
    /// The station holds this computer from transmit, in any mode, as its snapshot says it
    /// (`radio.txHold`): with the time left only when the station would lift the hold by itself.
    /// DISPLAY ONLY — the banner says so; the station's own authority decides every transmission.
    pub(crate) tx_hold: Option<tempo_app::dto::TxHold>,
    /// The link runs over the internet (through the station's room), not this network.
    pub(crate) internet: bool,
    /// "Still there?": nothing from the operator for a quarter of an hour on the internet.
    pub(crate) still_there: bool,
    /// On the internet, this network lets no datagram road through: watch, tune and log only.
    pub(crate) udp_blocked: bool,
    /// A test build's road switch: Normal, or the road this computer blocks.
    #[cfg(feature = "bench-road-switch")]
    pub(crate) bench: crate::bench_road_switch::Position,
    /// This computer's Nexus version.
    pub(crate) computer_build: &'static str,
    /// The station's Nexus version, as its last welcome said it, the link opened or refused for
    /// another build: `None` until a welcome has said it.
    pub(crate) station_build: Option<String>,
    /// The day the station stops taking this computer on its network, seconds since 1970, while
    /// that is within a week and this computer has not signed in again ([`sign_in_again_by`]).
    pub(crate) sign_in_by: Option<u64>,
    /// Operate's launch is signing in again for the station: the sign-in waits in the browser.
    pub(crate) signing_again: bool,
    /// The station is kept by its service id until this computer first reaches it on its network
    /// (`crate::lan_client::Stations::keep_away`): the internet road is its only road from here.
    pub(crate) away: bool,
}

/// This computer's PTT between the screen's re-assertions.
enum Press {
    /// Not pressed.
    Up,
    /// Pressed: a held press numbered `press`, on the Stop road of the link it was made on. Held by
    /// Phone's Lock, `locked` is when its cap's count began: the press, or the operator's last
    /// answer to "Still talking?".
    Held {
        press: u64,
        road: Arc<StopRoad>,
        locked: Option<Instant>,
    },
    /// Ended other than by the screen letting go: a Stop, the station's word, or a new link. It
    /// never keys again; the screen must let go and press again.
    Ended,
}

/// Client mode's state: the link, the caps, each window's revisions and rows, and this computer's
/// own settings.
pub(crate) struct Client {
    station_id: String,
    status: Mutex<Status>,
    shack: Mutex<Option<Arc<dyn Shack>>>,
    caps: Caps,
    /// How many Stops this run has pressed, a Stop press or the off half of a switch: a start reads
    /// it when it is pressed, and one that waited for a slot while it moved never leaves.
    stops: AtomicU64,
    /// The revision of the copy each window holds of each store: the answer to its last read of
    /// it, or to its last save the shack took. A save goes with it. The screen does not say which
    /// read a save was made from, so a window that read the store again between making a copy and
    /// saving it sends the newer revision.
    revisions: Mutex<HashMap<(String, Store), String>>,
    /// The number of the last waterfall row each window was given, on this link.
    rows: Mutex<HashMap<String, u64>>,
    /// Each feed's last value, kept for the screens while the link is down.
    last: Mutex<HashMap<&'static str, Value>>,
    this: Mutex<ThisComputer>,
    /// Calls cut off by a link that dropped, asked after on the next one.
    gone: Mutex<Vec<u64>>,
    /// The shack's state has reached this computer since this run began.
    live_once: AtomicBool,
    /// The link and the lease this computer holds on it, handed back as Nexus quits.
    held: Mutex<Option<(Arc<Link>, String)>>,
    /// The operator asked to take control from the device holding it ([`Client::take_control`]):
    /// done on the lease's next turn, and asked again while the shack is only busy a moment.
    take: AtomicBool,
    /// Another device took control from this computer, so the banner says so until this one holds
    /// control again.
    taken_over: AtomicBool,
    /// Stop's second roads: the link's now, and each earlier link's for its last minute.
    roads: Mutex<Vec<Arc<StopRoad>>>,
    /// The number of the last snapshot each window's poll fetched, on the link there is now.
    fetched: Mutex<HashMap<String, u64>>,
    /// What the update hand-off saves of this computer's own (its window layouts and log).
    before_install: OnceLock<Box<dyn Fn() + Send + Sync>>,
    /// This computer's headset, for the shack's audio and the microphone: given once, as the app
    /// starts, and never in a test that has no use for one.
    headset: OnceLock<Arc<ThisHeadset>>,
    /// A test's stand-in for it on the links, opening no device.
    #[cfg(test)]
    stand_in: OnceLock<Arc<dyn Headset>>,
    /// The PTT, and the number the next press takes: rising for this run, so no two presses on
    /// one road are ever one press at the station.
    press: Mutex<Press>,
    next_press: AtomicU64,
    /// The microphone the voice keyer records from here (this computer's headset), and the
    /// recording under way, if one is.
    mic: OnceLock<Arc<dyn Headset>>,
    recording: Mutex<Option<Recording>>,
    /// A test build's road switch over every link this computer opens to the shack: in memory
    /// only, so every launch starts with both roads open.
    #[cfg(feature = "bench-road-switch")]
    road_switch: Arc<crate::bench_road_switch::Switch>,
    /// The internet road, when this computer may open one ([`Afar`]), and its clocks.
    afar: OnceLock<Arc<dyn Afar>>,
    /// Where a typed host name is looked up; the system's resolver when none was given.
    lookup: OnceLock<Lookup>,
    /// Where an address the operator typed is kept once Operate opened there; nowhere when none
    /// was given.
    keep: OnceLock<Arc<dyn Keep>>,
    /// The kept client target, where why the station last refused this computer is noted
    /// ([`note_refusal_at`]); nowhere when none was given.
    target: OnceLock<PathBuf>,
    timing: Mutex<Timing>,
    /// The link there is now runs over the internet.
    on_internet: AtomicBool,
    /// On the internet, this network has let no datagram road through ([`Timing::udp_grace`]).
    udp_blocked: AtomicBool,
    /// The operator's input, for "Still there?" (N5); and whether nobody answered it, so nothing
    /// connects again until the operator does ([`Client::still_here`]).
    idle: Mutex<IdleWatch>,
    idle_ended: AtomicBool,
    answered: tokio::sync::Notify,
    /// How many times longer than [`Timing::lan_probe`] the next try of this network waits: it
    /// doubles each time the station was found here but the link would not open.
    lan_backoff: std::sync::atomic::AtomicU32,
}

impl Client {
    pub(crate) fn new(station_id: String, this: ThisComputer) -> Self {
        Self {
            status: Mutex::new(Status {
                station: station_id.clone(),
                phase: Phase::Connecting,
                reason: None,
                over: None,
                ptt_refusal: None,
                tx_hold: None,
                internet: false,
                still_there: false,
                udp_blocked: false,
                #[cfg(feature = "bench-road-switch")]
                bench: Default::default(),
                computer_build: tempo_client::wire::APP,
                station_build: None,
                sign_in_by: None,
                signing_again: false,
                away: false,
            }),
            station_id,
            shack: Mutex::new(None),
            caps: Caps::new(),
            stops: AtomicU64::new(0),
            revisions: Mutex::new(HashMap::new()),
            rows: Mutex::new(HashMap::new()),
            last: Mutex::new(HashMap::new()),
            this: Mutex::new(this),
            gone: Mutex::new(Vec::new()),
            live_once: AtomicBool::new(false),
            held: Mutex::new(None),
            take: AtomicBool::new(false),
            taken_over: AtomicBool::new(false),
            roads: Mutex::new(Vec::new()),
            fetched: Mutex::new(HashMap::new()),
            before_install: OnceLock::new(),
            headset: OnceLock::new(),
            #[cfg(test)]
            stand_in: OnceLock::new(),
            press: Mutex::new(Press::Up),
            next_press: AtomicU64::new(1),
            mic: OnceLock::new(),
            recording: Mutex::new(None),
            #[cfg(feature = "bench-road-switch")]
            road_switch: Arc::default(),
            afar: OnceLock::new(),
            lookup: OnceLock::new(),
            keep: OnceLock::new(),
            target: OnceLock::new(),
            timing: Mutex::new(Timing::default()),
            on_internet: AtomicBool::new(false),
            udp_blocked: AtomicBool::new(false),
            idle: Mutex::new({
                let timing = Timing::default();
                IdleWatch::bounded(timing.idle_prompt, timing.idle_end, Instant::now())
            }),
            idle_ended: AtomicBool::new(false),
            answered: tokio::sync::Notify::new(),
            lan_backoff: std::sync::atomic::AtomicU32::new(1),
        }
    }

    /// The internet road this computer may open when the station is not on this network. Given
    /// once, before [`Client::drive`]; without it, this network's road alone.
    pub(crate) fn reach_afar(&self, afar: Arc<dyn Afar>) {
        let _ = self.afar.set(afar);
    }

    /// Where a typed host name is looked up, for a test with names of its own. Given once, before
    /// [`Client::drive`].
    #[cfg(test)]
    pub(crate) fn look_up_with(&self, lookup: Lookup) {
        let _ = self.lookup.set(lookup);
    }

    /// Where an address the operator typed is kept once Operate opened there, and read back on
    /// the next launch. Given once, before [`Client::drive`].
    pub(crate) fn keep_typed_with(&self, keep: Arc<dyn Keep>) {
        let _ = self.keep.set(keep);
    }

    /// Where the kept client target is, so a refusal only a new pairing ends is noted beside it,
    /// and cleared by the next link that opens. Given once, before [`Client::drive`].
    pub(crate) fn note_refusals_in(&self, target: PathBuf) {
        let _ = self.target.set(target);
    }

    /// [`note_refusal_at`] in the target given, if one was.
    fn note_refusal(&self, refused: Option<&str>) {
        if let Some(path) = self.target.get() {
            note_refusal_at(path, &self.station_id, refused);
        }
    }

    /// Where `typed` is now: an address as it is, a host name looked up again, each time it is
    /// asked. None for none, and for a name that resolves to no address on a private network.
    async fn typed_at(&self, typed: Option<Typed>) -> Option<SocketAddrV4> {
        let lookup = self
            .lookup
            .get()
            .cloned()
            .unwrap_or_else(|| Arc::new(tempo_stream::lan::lookup));
        let typed = typed?;
        tokio::task::spawn_blocking(move || typed.resolve(&lookup))
            .await
            .ok()
            .flatten()
    }

    /// The internet road's clocks, for a test that cannot wait for them.
    #[cfg(test)]
    pub(crate) fn time_with(&self, timing: Timing) {
        let idle = IdleWatch::bounded(timing.idle_prompt, timing.idle_end, Instant::now());
        *lock(&self.idle) = idle;
        *lock(&self.timing) = timing;
    }

    /// The operator did something here: a click, a key or the wheel in a window (the banner says
    /// so, [`client_still_here`]). It answers "Still there?", and after a link nobody answered it
    /// for, lets this computer connect again.
    pub(crate) fn still_here(&self) {
        lock(&self.idle).active(Instant::now());
        if self.idle_ended.swap(false, Ordering::SeqCst) {
            self.set(Phase::Connecting, None);
            self.answered.notify_one();
        }
    }

    /// The operator confirmed Take control in the banner while another device holds the station:
    /// the lease's next turn takes it, ending that device's over as its Stop would, and this
    /// computer starts with TX off. Nothing while this computer is not kept out by another.
    pub(crate) fn take_control(&self) {
        if lock(&self.status).phase == Phase::Busy {
            self.take.store(true, Ordering::SeqCst);
        }
    }

    /// The lease this computer holds now, if it holds one: for a test that watches it.
    #[cfg(test)]
    pub(crate) fn lease_held(&self) -> Option<String> {
        lock(&self.held).as_ref().map(|(_, lease)| lease.clone())
    }

    /// The link's state, for the banner, with this computer's over as the link's Stop road last
    /// heard it.
    pub(crate) fn status(&self) -> Status {
        let over = self
            .shack()
            .and_then(|shack| shack.stop_road())
            .and_then(|road| road.over(std::time::Instant::now()));
        // Why a press would not key, as the station's snapshot says it (`radio.pttRefusal`).
        let snap = self.fed("snapshot/1").ok();
        let radio = snap.as_ref().and_then(|snap| snap.get("radio"));
        let ptt_refusal = radio
            .and_then(|radio| radio.get("pttRefusal"))
            .and_then(|refusal| refusal.as_str())
            .map(str::to_string);
        // The hold beside it, the same way (`radio.txHold`); one the banner cannot read is none.
        let tx_hold = radio
            .and_then(|radio| radio.get("txHold"))
            .and_then(|hold| serde_json::from_value(hold.clone()).ok());
        let internet = self.on_internet.load(Ordering::SeqCst);
        Status {
            over,
            ptt_refusal,
            tx_hold,
            internet,
            still_there: internet && lock(&self.idle).state(Instant::now()) == Idle::Prompt,
            udp_blocked: internet && self.udp_blocked.load(Ordering::SeqCst),
            #[cfg(feature = "bench-road-switch")]
            bench: self.road_switch.position(),
            ..lock(&self.status).clone()
        }
    }

    /// Signing in again for the station: the day the station stops taking this computer on its
    /// network while that is still to be done (`by`), and whether the sign-in is under way.
    pub(crate) fn signing_again(&self, by: Option<u64>, under_way: bool) {
        let mut status = lock(&self.status);
        status.sign_in_by = by;
        status.signing_again = under_way;
    }

    /// A test build's road switch.
    #[cfg(feature = "bench-road-switch")]
    pub(crate) fn road_switch(&self) -> &crate::bench_road_switch::Switch {
        &self.road_switch
    }

    /// This computer's headset, given once as the app starts.
    pub(crate) fn hear_with(&self, headset: Arc<ThisHeadset>) {
        let _ = self.mic.set(Arc::clone(&headset) as Arc<dyn Headset>);
        let _ = self.headset.set(headset);
    }

    /// A stand-in headset for a test's links: the station's audio and the microphone over.
    #[cfg(test)]
    pub(crate) fn hear_in_a_test(&self, headset: Arc<dyn Headset>) {
        let _ = self.stand_in.set(headset);
    }

    /// The headset a link opens with, if this computer has one.
    fn link_headset(&self) -> Option<Arc<dyn Headset>> {
        #[cfg(test)]
        if let Some(stand_in) = self.stand_in.get() {
            return Some(Arc::clone(stand_in));
        }
        self.headset
            .get()
            .map(|headset| Arc::clone(headset) as Arc<dyn Headset>)
    }

    /// The microphone the voice keyer records from, for a test with no headset.
    #[cfg(test)]
    pub(crate) fn record_from(&self, mic: Arc<dyn Headset>) {
        let _ = self.mic.set(mic);
    }

    fn set(&self, phase: Phase, reason: Option<&str>) {
        let mut status = lock(&self.status);
        status.phase = phase;
        status.reason = reason.map(str::to_string);
    }

    fn at(&self, station: String) {
        lock(&self.status).station = station;
    }

    /// The shack as it is reached now, if it is.
    fn shack(&self) -> Option<Arc<dyn Shack>> {
        lock(&self.shack).clone()
    }

    /// A new link to the shack, or none: each window's rows and snapshot numbers start again with
    /// its numbering, and its Stop road joins those a Stop goes on. The roads past their minute go.
    /// A press held on the link before it is let go there, and ended: it never keys on the next.
    pub(crate) fn install(&self, shack: Option<Arc<dyn Shack>>) {
        {
            let mut held = lock(&self.press);
            if let Press::Held { press, road, .. } = &*held {
                road.release(*press);
                *held = Press::Ended;
            }
        }
        lock(&self.rows).clear();
        lock(&self.fetched).clear();
        let now = std::time::Instant::now();
        let mut roads = lock(&self.roads);
        roads.retain(|road| !road.gone(now));
        if let Some(road) = shack.as_ref().and_then(|shack| shack.stop_road()) {
            if !roads.iter().any(|kept| Arc::ptr_eq(kept, &road)) {
                roads.push(road);
            }
        }
        drop(roads);
        *lock(&self.shack) = shack;
    }

    /// The Stop press numbered `stop`, on every Stop road that may still reach the shack: written
    /// now, on this thread, beside the Stop the command link carries under the same number. The
    /// roads past their minute go.
    fn stop_on_the_roads(&self, stop: u64) {
        let now = std::time::Instant::now();
        let mut roads = lock(&self.roads);
        roads.retain(|road| !road.gone(now));
        for road in roads.iter() {
            road.stop(stop);
        }
    }

    /// Whether a transmission start may leave this computer now: a link, its Stop road, and
    /// presence live as that road last heard it. The road, or the code of the first that is not
    /// there. The shack judges the start again, by its own clock.
    fn ready_to_start(&self, now: Instant) -> Result<Arc<StopRoad>, &'static str> {
        let shack = self.shack().ok_or("notLive")?;
        // ⛔ On the internet with no datagram road through this network, Stop would have the link
        // alone: nothing starts here (watch, tune and log only). Presence, below, would refuse it
        // too, since presence comes only over Stop's road; this says why.
        if self.on_internet.load(Ordering::SeqCst) && self.udp_blocked.load(Ordering::SeqCst) {
            return Err("udpBlocked");
        }
        let road = shack.stop_road().ok_or(Refusal::NoStopRoad.code())?;
        if !road.presence_live(now) {
            return Err(Refusal::NoPresence.code());
        }
        Ok(road)
    }

    /// The screen re-asserted its PTT (`set_ptt` on), or its Lock (`hold_ptt_lock`, `locking` the
    /// operator's answer to "Still talking?"). The first starts a held press on Stop's road, once a
    /// start may leave this computer; each one after it holds that press again, one hold each, never
    /// on a timer of this computer's own, on the same link and while presence is live as its road
    /// last heard it. A press whose presence lapsed here is let go on its road and ends. A press that
    /// ended other than by the screen letting go is refused until the screen lets go: it never keys
    /// again.
    ///
    /// ⛔ A LOCK ENDS AT ITS CAP. Held by the Lock, a press is let go on its road and ends once its
    /// cap has passed since it began or since the operator last answered "Still talking?", whatever
    /// re-asserts it after: no finger bounds it, so the cap does. An answer moves the count on only
    /// while the cap has not passed, and the Lock goes on under a new press, since the station ends
    /// any one press from afar at the same cap on its own clock.
    fn press(&self, locking: Option<bool>) -> Result<Value, String> {
        let now = Instant::now();
        // A held PTT is the operator at work, for as long as the screen re-asserts it.
        lock(&self.idle).active(now);
        let timing = *lock(&self.timing);
        let mut held = lock(&self.press);
        match &mut *held {
            Press::Held {
                press,
                road,
                locked,
            } => {
                let on_this_link = self
                    .shack()
                    .and_then(|shack| shack.stop_road())
                    .is_some_and(|now| Arc::ptr_eq(&now, road));
                let capped = locked.is_some_and(|since| {
                    phone_lock::locked(since, timing.lock_cap, timing.lock_ask, now)
                        == Locked::Capped
                });
                if (on_this_link && !road.presence_live(now)) || capped {
                    // Presence as this computer last heard it has lapsed (its Stop road gone quiet,
                    // say), or the Lock's cap has passed: the press ends here, its microphone
                    // stops, and the station is told.
                    road.release(*press);
                } else if on_this_link && road.hold(*press) {
                    if let Some(answered) = locking {
                        let since = locked.get_or_insert(now);
                        if answered {
                            *since = now;
                            // The station holds one press for at most the cap on its own clock, so
                            // the Lock goes on under a new press: the key handed straight over
                            // there, the microphone carried on here.
                            let renewed = self.next_press.fetch_add(1, Ordering::SeqCst);
                            if road.renew(*press, renewed, self.stops.load(Ordering::SeqCst)) {
                                *press = renewed;
                            }
                        }
                    }
                    return Ok(self.held_answer(locking, *locked, timing, now));
                }
                *held = Press::Ended;
                Err(word(if capped { "lockCapped" } else { "pressEnded" }))
            }
            Press::Ended => Err(word("pressEnded")),
            Press::Up => {
                // The microphone is the recorder's while a voice message is recorded here.
                if lock(&self.recording).is_some() {
                    return Err(word("recording"));
                }
                let road = self.ready_to_start(now).map_err(word)?;
                let press = self.next_press.fetch_add(1, Ordering::SeqCst);
                // Begun under the Stops pressed so far: a Stop pressed after it that reaches the
                // shack first keeps it from ever keying there.
                road.press(press, self.stops.load(Ordering::SeqCst));
                let locked = locking.map(|_| now);
                *held = Press::Held {
                    press,
                    road,
                    locked,
                };
                Ok(self.held_answer(locking, locked, timing, now))
            }
        }
    }

    /// What a hold is answered: the PTT's, the station's snapshot; the Lock's, whether "Still
    /// talking?" is asked and how long is left before its cap.
    fn held_answer(
        &self,
        locking: Option<bool>,
        locked: Option<Instant>,
        timing: Timing,
        now: Instant,
    ) -> Value {
        match (locking, locked) {
            (Some(_), Some(since)) => {
                match phone_lock::locked(since, timing.lock_cap, timing.lock_ask, now) {
                    Locked::Ask { left } => {
                        json!({ "ask": true, "leftMs": left.as_millis() as u64 })
                    }
                    _ => json!({ "ask": false }),
                }
            }
            _ => self.fed("snapshot/1").unwrap_or_else(|_| json!({})),
        }
    }

    /// The screen let go of its PTT: a press held is released on its road, at once, and the next
    /// press is a new one.
    fn let_go_the_press(&self) {
        let mut held = lock(&self.press);
        if let Press::Held { press, road, .. } = &*held {
            road.release(*press);
        }
        *held = Press::Up;
    }

    /// One IPC call from a window: routed by [`way`], answered here, sent to the shack, or refused.
    /// `local` is the app's own handler.
    pub(crate) fn handle<R: Runtime>(
        self: &Arc<Self>,
        invoke: Invoke<R>,
        local: &dyn Fn(Invoke<R>) -> bool,
    ) -> bool {
        let cmd = invoke.message.command().to_string();
        let args = match invoke.message.payload() {
            InvokeBody::Json(args) => args.clone(),
            // A raw body cannot be routed by its arguments, so it goes nowhere.
            InvokeBody::Raw(_) => {
                invoke.resolver.reject(word(Refusal::BadArguments.code()));
                return true;
            }
        };
        let window = invoke.message.webview_ref().label().to_string();
        match way(&cmd, &args) {
            Way::Here => local(invoke),
            Way::Refused(refusal) => {
                invoke.resolver.reject(word(refusal.code()));
                true
            }
            Way::Local(Local::LaunchAtLogin) => {
                let app = invoke.message.webview_ref().app_handle().clone();
                let me = Arc::clone(self);
                invoke.resolver.respond_async(async move {
                    me.launch_at_login(&app, &args).map_err(InvokeError::from)
                });
                true
            }
            Way::Local(Local::Restart) => {
                // Tauri starts this same binary again with the arguments it was started with, so
                // in client mode again; the quit hands the lease back first.
                invoke.message.webview_ref().app_handle().request_restart();
                invoke.resolver.resolve(());
                true
            }
            Way::Local(Local::BeforeInstall) => {
                if let Some(save) = self.before_install.get() {
                    save();
                }
                invoke.resolver.resolve(());
                true
            }
            Way::Local(Local::RecordStop) => {
                let me = Arc::clone(self);
                invoke.resolver.respond_async(async move {
                    me.record_stop(&args, &window)
                        .await
                        .map_err(InvokeError::from)
                });
                true
            }
            Way::Local(what @ Local::BetaUpdates) => {
                let me = Arc::clone(self);
                invoke.resolver.respond_async(async move {
                    me.local(what, &args, &window).map_err(InvokeError::from)
                });
                true
            }
            Way::Local(what) => {
                let answer = self.local(what, &args, &window);
                invoke
                    .resolver
                    .respond_async(async move { answer.map_err(InvokeError::from) });
                true
            }
            Way::Split => {
                let me = Arc::clone(self);
                invoke.resolver.respond_async(async move {
                    me.split(&cmd, &args, &window)
                        .await
                        .map_err(InvokeError::from)
                });
                true
            }
            Way::Start => {
                // Only while this computer's own roads say a start may go; the shack judges it
                // again, by its own clock.
                if let Err(code) = self.ready_to_start(Instant::now()) {
                    invoke.resolver.reject(word(code));
                    return true;
                }
                let pressed = self.stops.load(Ordering::SeqCst);
                let me = Arc::clone(self);
                invoke.resolver.respond_async(async move {
                    me.at_the_shack(&cmd, args, &window, false, Some(pressed), pressed)
                        .await
                        .map_err(InvokeError::from)
                });
                true
            }
            Way::Press => {
                let answer = self.press(None);
                invoke
                    .resolver
                    .respond_async(async move { answer.map_err(InvokeError::from) });
                true
            }
            Way::Lock => {
                let answer = self.press(Some(args["stillTalking"].as_bool() == Some(true)));
                invoke
                    .resolver
                    .respond_async(async move { answer.map_err(InvokeError::from) });
                true
            }
            Way::Shack { stop } => {
                // ⛔ STOP WINS: counted first, an atomic step, so a start pressed before it and
                // still waiting here for a slot never leaves. Its count is its number on both
                // roads, so the shack can tell which starts were made before it.
                let stops = if stop {
                    self.stops.fetch_add(1, Ordering::SeqCst) + 1
                } else {
                    self.stops.load(Ordering::SeqCst)
                };
                // A Stop press goes by the second road first, written here before the call is
                // handed to the link, so a link that is stuck or gone holds nothing back. The off
                // half of a switch is not a Stop press: it goes by the link alone, but the PTT's
                // off half lets the held press go on its road first.
                if stop && class_of(&cmd) == Some(Class::StopsTx) {
                    self.stop_on_the_roads(stops);
                }
                if stop && cmd == "set_ptt" {
                    self.let_go_the_press();
                }
                let me = Arc::clone(self);
                invoke.resolver.respond_async(async move {
                    me.at_the_shack(&cmd, args, &window, stop, None, stops)
                        .await
                        .map_err(InvokeError::from)
                });
                true
            }
        }
    }

    /// What client mode answers itself, but start at sign-in ([`Client::launch_at_login`]).
    fn local(&self, what: Local, args: &Value, window: &str) -> Result<Value, String> {
        match what {
            Local::Snapshot => self.polled(window),
            Local::Meters => self.fed("meters/1"),
            Local::RttyScope => self.fed("rttyScope/1"),
            Local::SpectrumRow => Ok(self.row(window)),
            Local::BetaUpdates | Local::LaunchAtLogin => {
                let on = args["on"]
                    .as_bool()
                    .ok_or_else(|| word(Refusal::BadArguments.code()))?;
                lock(&self.this).keep(what, on)?;
                // The screen reads a falsy answer as a write that failed.
                Ok(self.fed("snapshot/1").unwrap_or_else(|_| json!({})))
            }
            Local::RadioLaunchInfo => Ok(json!({ "showPicker": false, "radios": [] })),
            Local::BeforeInstall | Local::Restart => Ok(Value::Null),
            Local::RecordStart => {
                let mic = self
                    .mic
                    .get()
                    .ok_or("No microphone on this computer to record from")?;
                // The microphone is the over's while a press is held, as it is the recorder's
                // while a message is recorded: refused, and nothing recorded. Held while the
                // recording starts (a thread, at once), so no press begins between the two.
                let held = lock(&self.press);
                if matches!(*held, Press::Held { .. }) {
                    return Err(word("pressHeld"));
                }
                let mut recording = lock(&self.recording);
                // A recording already under way starts afresh, as the station's own does.
                recording.take();
                *recording = Some(
                    Recording::start(Arc::clone(mic))
                        .ok_or("No microphone on this computer to record from")?,
                );
                Ok(self.fed("snapshot/1").unwrap_or_else(|_| json!({})))
            }
            Local::RecordCancel => {
                lock(&self.recording).take();
                Ok(self.fed("snapshot/1").unwrap_or_else(|_| json!({})))
            }
            // Answered by [`Client::record_stop`].
            Local::RecordStop => Err(word(Refusal::BadArguments.code())),
        }
    }

    /// The voice keyer's Stop & save: the recording made here, finished, written as the keyer's
    /// 12 kHz WAV and sent to the shack's slot (`import_voice_message`, as an imported file goes),
    /// with the shack's answer: its slots as they are now. Nothing recorded is the station's own
    /// words for it.
    async fn record_stop(self: Arc<Self>, args: &Value, window: &str) -> Result<Value, String> {
        let slot = args["slot"]
            .as_u64()
            .and_then(|slot| u8::try_from(slot).ok())
            .ok_or_else(|| word(Refusal::BadArguments.code()))?;
        let label = args["label"].as_str().unwrap_or_default().to_string();
        let recording = lock(&self.recording).take();
        let voice = match recording {
            Some(recording) => tokio::task::spawn_blocking(move || recording.finish())
                .await
                .unwrap_or_default(),
            None => Vec::new(),
        };
        if voice.is_empty() {
            return Err("Nothing was recorded — check your input device".to_string());
        }
        let bytes = audio_road::voice_message(&voice)
            .map_err(|e| format!("Could not save recording: {e}"))?;
        let message = json!({ "slot": slot, "label": label, "bytes": bytes });
        let stops = self.stops.load(Ordering::SeqCst);
        self.at_the_shack("import_voice_message", message, window, false, None, stops)
            .await
    }

    /// What the update hand-off saves of this computer's own, once the app is built.
    pub(crate) fn before_install(&self, save: Box<dyn Fn() + Send + Sync>) {
        let _ = self.before_install.set(save);
    }

    /// Start at sign-in, on this computer: the operating system's login entry first, and the
    /// choice kept only once it took, as the app's own command does.
    fn launch_at_login<R: Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        args: &Value,
    ) -> Result<Value, String> {
        use tauri_plugin_autostart::ManagerExt;
        let on = args["on"]
            .as_bool()
            .ok_or_else(|| word(Refusal::BadArguments.code()))?;
        let launcher = app.autolaunch();
        let changed = if on {
            launcher.enable()
        } else {
            launcher.disable()
        };
        changed.map_err(|_| "launchAtLoginUnsupported".to_string())?;
        self.local(Local::LaunchAtLogin, args, "")
    }

    /// A feed's newest value, or its last while the link is down; nothing before it first came.
    fn fed(&self, feed: &'static str) -> Result<Value, String> {
        self.newest(self.shack().as_ref(), feed)
            .map(|(_, value)| value)
    }

    /// [`Client::fed`] from `shack`, with the value's number when it came from the link now.
    fn newest(
        &self,
        shack: Option<&Arc<dyn Shack>>,
        feed: &'static str,
    ) -> Result<(Option<u64>, Value), String> {
        let live = shack.and_then(|shack| shack.latest(feed));
        let mut last = lock(&self.last);
        if let Some((number, value)) = live {
            last.insert(feed, value.clone());
            return Ok((Some(number), value));
        }
        last.get(feed)
            .cloned()
            .map(|value| (None, value))
            .ok_or_else(|| word("notLive"))
    }

    /// A window's snapshot poll: [`Client::fed`], and the number of what it fetched kept for that
    /// window. The main window's goes to the link's Stop road, whose heartbeat names it: the screen's
    /// own poll, so a main window that hangs stops it moving.
    fn polled(&self, window: &str) -> Result<Value, String> {
        let shack = self.shack();
        let (number, value) = self.newest(shack.as_ref(), "snapshot/1")?;
        if let (Some(shack), Some(number)) = (&shack, number) {
            lock(&self.fetched).insert(window.to_string(), number);
            if window == "main" {
                if let Some(road) = shack.stop_road() {
                    road.fetched(number);
                }
            }
        }
        Ok(value)
    }

    /// The newest waterfall row, once to each window: a window that has it already, or a link
    /// with none, gets the empty row every display reads as "nothing new this tick", so the
    /// waterfall never scrolls a copy.
    fn row(&self, window: &str) -> Value {
        let latest = self.shack().and_then(|shack| shack.latest("spectrumRow/1"));
        let mut rows = lock(&self.rows);
        match latest {
            Some((number, row)) if rows.get(window).is_none_or(|seen| number > *seen) => {
                rows.insert(window.to_string(), number);
                row
            }
            _ => serde_json::to_value(tempo_app::dto::Spectrum {
                row: Vec::new(),
                lo_hz: 0.0,
                hi_hz: 0.0,
                source: String::new(),
            })
            .unwrap_or_default(),
        }
    }

    /// One call at the shack, under the caps unless it is a Stop. A start comes with the count of
    /// Stops pressed when it was (`pressed`): if one was pressed here while it waited for its slot,
    /// it is refused `stopped` and never sent. Stop wins over a start that was in flight. `stops` is
    /// the count the call carries to the shack: a start's `pressed`, a Stop's own number.
    async fn at_the_shack(
        self: Arc<Self>,
        cmd: &str,
        args: Value,
        window: &str,
        stop: bool,
        pressed: Option<u64>,
        stops: u64,
    ) -> Result<Value, String> {
        let _slot = if stop {
            None
        } else {
            Some(self.caps.slot(cmd).await)
        };
        if pressed.is_some_and(|pressed| pressed != self.stops.load(Ordering::SeqCst)) {
            return Err(word(Refusal::Stopped.code()));
        }
        let mut value = self.send(cmd, args, window, stops).await?;
        if cmd == "get_settings" {
            lock(&self.this).over(&mut value);
        }
        Ok(value)
    }

    /// Sends one call on the link there is now, with the base revision when it saves a store, and
    /// keeps the revision the shack answers with for the store it read or saved. The base is the
    /// read the saved copy was made from when the screen names it (`base`, which `api.ts` keeps
    /// with each settings read, and which goes no further than here), else the window's last read
    /// or accepted save. A settings read takes its revision to the screen ([`READ_REVISION`]).
    async fn send(
        &self,
        cmd: &str,
        mut args: Value,
        window: &str,
        stops: u64,
    ) -> Result<Value, String> {
        let shack = self.shack().ok_or_else(|| word("notLive"))?;
        let access = store_access(cmd);
        let base = match access {
            Some((store, true)) => {
                let named = args
                    .as_object_mut()
                    .and_then(|fields| fields.remove("base"))
                    .and_then(|base| base.as_str().map(str::to_string));
                named.or_else(|| {
                    lock(&self.revisions)
                        .get(&(window.to_string(), store))
                        .cloned()
                })
            }
            _ => None,
        };
        let (id, answer) = shack.call(cmd.to_string(), args, base, stops).await;
        match answer {
            Answer::Value {
                mut value,
                revision,
            } => {
                if let (Some((store, saved)), Some(revision)) = (access, revision) {
                    if store == Store::Settings && !saved {
                        if let Some(fields) = value.as_object_mut() {
                            fields.insert(READ_REVISION.to_string(), json!(revision));
                        }
                    }
                    lock(&self.revisions).insert((window.to_string(), store), revision);
                }
                Ok(value)
            }
            Answer::Error(error) => Err(error),
            Answer::Refused(refusal) => Err(word(refusal.code())),
            // A fresh id is never a repeat; if one were, it did not run again.
            Answer::Before(_) => Err(word(Refusal::Replayed.code())),
            Answer::TooLarge => Err(word("tooLarge")),
            Answer::Gone => {
                lock(&self.gone).push(id);
                Err(word("gone"))
            }
        }
    }

    /// The ui-state pair, split by key: the station's keys from and to the shack, every other key
    /// from and to this computer's own file, where this computer's own station's keys stay as they
    /// are. Both halves are this window's, so the revision of the shack's half is kept for its
    /// next save as any store's is.
    async fn split(&self, cmd: &str, args: &Value, window: &str) -> Result<Value, String> {
        let path = lock(&self.this).ui_state.clone();
        match cmd {
            "ui_state_load" => {
                let strict = args["strict"].as_bool().unwrap_or(false);
                // The station's half strictly, always: a file at the shack that does not read is
                // no station data, never an empty list of it.
                let _slot = self.caps.slot(cmd).await;
                let stops = self.stops.load(Ordering::SeqCst);
                let station = self
                    .send("ui_state_load", json!({ "strict": true }), window, stops)
                    .await?;
                let station: BTreeMap<String, String> = serde_json::from_value(station)
                    .map_err(|_| word(Refusal::BadArguments.code()))?;
                let here = crate::ui_state_read(&path, strict)?;
                serde_json::to_value(join_ui_state(station, here)).map_err(|e| e.to_string())
            }
            _ => {
                let sent: BTreeMap<String, String> = serde_json::from_value(args["state"].clone())
                    .map_err(|_| word(Refusal::BadArguments.code()))?;
                let (station, own) = split_ui_state(sent);
                let kept = crate::ui_state_read(&path, true).unwrap_or_default();
                let here = serde_json::to_string_pretty(&join_ui_state(kept, own))
                    .map(|text| crate::write_json_atomic(&path, &text))
                    .unwrap_or(false);
                let _slot = self.caps.slot(cmd).await;
                let there = matches!(
                    self.send(
                        "ui_state_save",
                        json!({ "state": station }),
                        window,
                        self.stops.load(Ordering::SeqCst),
                    )
                    .await,
                    Ok(Value::Bool(true))
                );
                Ok(json!(here && there))
            }
        }
    }

    /// Keeps the link to the station `id` names: connects, takes and keeps the lease, watches the
    /// feeds, hands the shack's events to the screens, and connects again when it drops. Shows the
    /// main window once the shack's state is here, or after [`SHOW_BY`] without it; a window shown
    /// without it is loaded again the first time it comes, so every screen starts from the shack's.
    pub(crate) async fn drive<R: Runtime>(
        self: Arc<Self>,
        app: tauri::AppHandle<R>,
        find: Find,
        look: Look,
        typed: Option<Typed>,
    ) {
        let shown = Arc::new(AtomicBool::new(false));
        {
            let (app, me, shown) = (app.clone(), Arc::clone(&self), Arc::clone(&shown));
            tauri::async_runtime::spawn(async move {
                let deadline = tokio::time::Instant::now() + SHOW_BY;
                while !me.live_once.load(Ordering::SeqCst) && tokio::time::Instant::now() < deadline
                {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                shown.store(!me.live_once.load(Ordering::SeqCst), Ordering::SeqCst);
                show_the_app(&app).await;
            });
        }
        loop {
            // Nobody answered "Still there?": nothing connects again until the operator does.
            while self.idle_ended.load(Ordering::SeqCst) {
                self.answered.notified().await;
            }
            let id = self.station_id.clone();
            let found = find.clone();
            let record = tokio::task::spawn_blocking(move || found(&id))
                .await
                .unwrap_or(Err("storeUnavailable"));
            let record = match record {
                Ok(Some(record)) => Some(record),
                // Kept by its service id until this computer first reaches it on its network:
                // no record of it here, so its internet road alone.
                Ok(None) if self.road_kept().await => None,
                Ok(None) => {
                    // Forgotten here, or never paired: nothing will change that but the operator.
                    self.set(Phase::Refused, Some("unknownStation"));
                    return;
                }
                Err(reason) => {
                    self.set(Phase::Refused, Some(reason));
                    tokio::time::sleep(AGAIN_AFTER).await;
                    continue;
                }
            };
            lock(&self.status).away = record.is_none();
            if let Some(first) = record.as_ref().and_then(|record| record.addresses.first()) {
                self.at(first.clone());
            }
            // What the operator typed for this launch, or else what was kept for the station when
            // Operate last opened there: tried first, a host name looked up again on every try.
            let kept = match (&typed, self.keep.get()) {
                (None, Some(keep)) => {
                    let (keep, id) = (Arc::clone(keep), self.station_id.clone());
                    tokio::task::spawn_blocking(move || keep.typed(&id))
                        .await
                        .ok()
                        .flatten()
                }
                _ => None,
            };
            let typing = typed.clone().or(kept);
            let typed_at = self.typed_at(typing.clone()).await;
            let unresolved = typing.is_some() && typed_at.is_none();
            let connecting = record
                .as_ref()
                .map(|record| self.connect(record, typed_at, &look));
            // In a test build, what this link opens is under the road switch.
            #[cfg(feature = "bench-road-switch")]
            let connecting = connecting.map(|connecting| self.road_switch.over(connecting));
            let probed = self.lan_backoff.load(Ordering::SeqCst) > 1;
            // The station's version, heard from its welcome on either road, for a refusal to name.
            let (reached, heard) = crate::lan_client::link::hearing_build(async {
                let Some(connecting) = connecting else {
                    // No record of it on this network to try.
                    let internet = self.over_the_internet("unknownStation").await;
                    return internet.map(|link| (link, None));
                };
                match connecting.await {
                    Ok((link, at)) => Ok((link, Some(at))),
                    // Not on this network (nothing answered, or not the station): the internet
                    // road, when one is kept for this station. LAN first, always.
                    Err(why) if road::look_further(why) || why == "notThisStation" => {
                        self.over_the_internet(why).await.map(|link| (link, None))
                    }
                    Err(why) => Err(why),
                }
            })
            .await;
            if heard.is_some() {
                lock(&self.status).station_build = heard;
            }
            if let (true, Ok((_, None))) = (probed, &reached) {
                // This network's road was found by a try but would not open: wait longer next time.
                let backoff = self.lan_backoff.load(Ordering::SeqCst);
                self.lan_backoff
                    .store((backoff * 2).min(LAN_PROBE_MOST), Ordering::SeqCst);
            } else if let Ok((_, Some(_))) = &reached {
                self.lan_backoff.store(1, Ordering::SeqCst);
            }
            let wait = match reached {
                // The station proved the key pinned for this pairing; one that then names itself
                // another station is not the one asked for, and runs nothing from here. Over the
                // internet the station proved the key the service listed for it, which is the
                // identity there.
                Ok((link, Some(_))) if link.ids.station != self.station_id => {
                    link.close();
                    self.set(Phase::Refused, Some("notThisStation"));
                    AGAIN_REFUSED
                }
                Ok((link, at)) => {
                    let internet = at.is_none();
                    if let Some(at) = at {
                        self.at(at.to_string());
                    }
                    // On this network's road, pinned to the station's key here, the station names
                    // its key for the internet road: a road kept for it pinned to another is
                    // turned off.
                    if let (true, Some(key), Some(afar)) = (
                        !internet,
                        link.station_key.clone(),
                        self.afar.get().cloned(),
                    ) {
                        let id = self.station_id.clone();
                        let _ =
                            tokio::task::spawn_blocking(move || afar.heard_key(&id, &key)).await;
                    }
                    // Operate opened where the operator typed: kept with the station, so the next
                    // launch tries it first. A store that will not keep it changes nothing now.
                    if let (Some(typed), Some(keep)) = (&typed, self.keep.get()) {
                        if at.is_some() && at == typed_at {
                            let (keep, id, typed) =
                                (Arc::clone(keep), self.station_id.clone(), typed.clone());
                            tokio::task::spawn_blocking(move || keep.keep(&id, &typed));
                        }
                    }
                    self.note_refusal(None);
                    self.on_the_link(&app, Arc::new(link), &shown, internet, (&find, typing))
                        .await;
                    if !self.idle_ended.load(Ordering::SeqCst) {
                        self.set(Phase::NotLive, None);
                    }
                    AGAIN
                }
                Err(reason) => {
                    // Nothing answered anywhere, and the name typed for the station resolves to
                    // nothing: the name is what to check.
                    let reason = if unresolved && road::UNREACHED.contains(&reason) {
                        "nameUnresolved"
                    } else {
                        reason
                    };
                    let lasting =
                        LASTING.contains(&reason) || crate::lan_client::internet::lasting(reason);
                    // What the station told this computer, once each time it changes: the
                    // reason alone, never a key.
                    if lock(&self.status).reason.as_deref() != Some(reason) {
                        tempo_core::applog::warn(
                            "client",
                            &format!(
                                "client mode: the station did not take this computer ({reason})"
                            ),
                        );
                    }
                    self.note_refusal(Some(reason));
                    let phase = if !lasting && self.live_once.load(Ordering::SeqCst) {
                        Phase::NotLive
                    } else {
                        Phase::Refused
                    };
                    self.set(phase, Some(reason));
                    if lasting {
                        AGAIN_REFUSED
                    } else {
                        AGAIN_AFTER
                    }
                }
            };
            tokio::time::sleep(wait).await;
        }
    }

    /// The link to the station `record` names: at the address the operator typed, then at the
    /// remembered ones, and only when none of them welcomed this computer because nothing answered
    /// or another key did, where a look by name finds the station now (`road::found_for`). The
    /// link and the address that welcomed it, or the most telling reason none did (`road::told`).
    async fn connect(
        &self,
        record: &PairedStation,
        typed: Option<SocketAddrV4>,
        look: &Look,
    ) -> Result<(Link, SocketAddrV4), &'static str> {
        let headset = self.link_headset();
        match Link::connect(record, typed, run_id(), headset.clone()).await {
            Err(why) if road::look_further(why) => {
                let look = Arc::clone(look);
                let found = tokio::task::spawn_blocking(move || look(LOOK_FOR))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or_default();
                let found = road::found_for(record, &road::order(record, typed), found);
                Link::connect_at(record, &found, run_id(), headset)
                    .await
                    .map_err(|later| road::told(why, later, !found.is_empty()))
            }
            reached => reached,
        }
    }

    /// The link over the internet road, when one is kept for this station: or `why`, this
    /// network's reason, when none is, or the internet road's own reason it did not open.
    async fn over_the_internet(&self, why: &'static str) -> Result<Link, &'static str> {
        let Some(afar) = self.afar.get().cloned() else {
            return Err(why);
        };
        if !self.road_kept().await {
            return Err(why);
        }
        let headset = self.link_headset();
        afar.open(self.station_id.clone(), headset).await
    }

    /// Whether an internet road is kept for this station, asked off the async threads (the store
    /// can block). Never without the internet road ([`Client::reach_afar`]).
    async fn road_kept(&self) -> bool {
        let Some(afar) = self.afar.get().cloned() else {
            return false;
        };
        let id = self.station_id.clone();
        tokio::task::spawn_blocking(move || afar.has_road(&id))
            .await
            .unwrap_or(false)
    }

    /// One link, open, until it ends: the lease, the feeds, the events, and the calls the last
    /// link left unanswered. On the internet (`internet`), also: its pipe renewed, "Still there?",
    /// whether this network lets the datagram roads through, and a try of the station's addresses
    /// on this network now and then (`lan`: the record `find` reads, and the address the operator
    /// typed), which ends this link for that road when one answers and nothing is on the air.
    async fn on_the_link<R: Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        link: Arc<Link>,
        shown: &AtomicBool,
        internet: bool,
        lan: (&Find, Option<Typed>),
    ) {
        let timing = *lock(&self.timing);
        let opened = Instant::now();
        self.on_internet.store(internet, Ordering::SeqCst);
        self.udp_blocked.store(false, Ordering::SeqCst);
        if internet {
            // The operator's time away began when this link did.
            lock(&self.idle).active(opened);
        }
        // The cushion for this road: twice the LAN's on the internet, where jitter is larger.
        if let Some(headset) = self.mic.get() {
            let cushion = if internet {
                INTERNET_CUSHION_MS
            } else {
                CUSHION_MS
            };
            headset.rings().set_cushion_ms(cushion);
        }
        let renewing = match (internet, self.afar.get()) {
            (true, Some(afar)) => Some(self.renew_the_pipe(Arc::clone(afar), &link, timing.renew)),
            _ => None,
        };
        let lan_found = Arc::new(AtomicBool::new(false));
        let probing = internet.then(|| self.try_this_network(lan, timing, &lan_found));
        self.install(Some(link.clone() as Arc<dyn Shack>));
        let forwarding = {
            let (app, mut events) = (app.clone(), link.events());
            tauri::async_runtime::spawn(async move {
                use tokio::sync::broadcast::error::RecvError;
                // Only what the dispatcher lets cross ever arrives (`Link::events`). A screen that
                // fell behind misses the oldest, and goes on hearing the rest.
                loop {
                    match events.recv().await {
                        Ok(event) => {
                            let _ = app.emit(&event.name, event.data);
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            })
        };
        self.ask_after_the_gone(&link).await;
        let mut lease: Option<String> = None;
        let mut said_unwatched = false;
        let mut every = tokio::time::interval(HEARTBEAT);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = link.closed() => break,
                _ = every.tick() => {}
            }
            lease = self.keep_the_lease(&link, lease).await;
            *lock(&self.held) = lease.clone().map(|lease| (Arc::clone(&link), lease));
            // The Stop road's heartbeat names the lease held now, or none, and the audio road
            // listens only while this computer holds it.
            if let Some(road) = link.stop_road() {
                road.lease(lease.clone());
            }
            if let Some(audio) = link.audio_road() {
                audio.lease(lease.clone());
            }
            if !said_unwatched {
                if let Some(watching) = link.watching() {
                    said_unwatched = true;
                    let unknown = &watching["unknown"];
                    if unknown.as_array().is_some_and(|names| !names.is_empty()) {
                        tempo_core::applog::warn(
                            "client",
                            &format!("the station has no feed for {unknown}; those screens wait"),
                        );
                    }
                }
            }
            let here = link.latest("snapshot/1").is_some();
            if here && !self.live_once.swap(true, Ordering::SeqCst) && shown.load(Ordering::SeqCst)
            {
                // The shack's state is here for the first time, and the main window was shown
                // without it: it starts again, so every screen starts from the shack's.
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.eval("window.location.reload()");
                }
            }
            if !internet {
                continue;
            }
            let now = Instant::now();
            let blocked =
                now >= opened + timing.udp_grace && link.internet_stop_road() != Some(true);
            self.udp_blocked.store(blocked, Ordering::SeqCst);
            if lock(&self.idle).state(now) == Idle::End {
                // Nobody answered "Still there?": the station is let go, as Leave lets it go.
                self.idle_ended.store(true, Ordering::SeqCst);
                if let Some(held) = lease.take() {
                    let release = operate(&link, json!({"type":"release","leaseId":held}));
                    let _ = tokio::time::timeout(Duration::from_secs(1), release).await;
                }
                link.close();
                self.set(Phase::Refused, Some("idle"));
                break;
            }
            if lan_found.load(Ordering::SeqCst) && self.nothing_on_the_air(now) {
                // The station answers on this network again: its road is free, and comes first.
                tempo_core::applog::info(
                    "client",
                    "the station answers on this network again: leaving the internet road for it",
                );
                link.close();
                break;
            }
        }
        if let Some(renewing) = renewing {
            renewing.abort();
        }
        if let Some(probing) = probing {
            probing.abort();
        }
        forwarding.abort();
        *lock(&self.held) = None;
        self.install(None);
        self.udp_blocked.store(false, Ordering::SeqCst);
    }

    /// Renews the pipe under `link` every `every` while it lives; a refusal that asking again will
    /// not change ends the link, as the room will itself within its minute.
    fn renew_the_pipe(
        &self,
        afar: Arc<dyn Afar>,
        link: &Arc<Link>,
        every: Duration,
    ) -> tauri::async_runtime::JoinHandle<()> {
        let (id, link) = (self.station_id.clone(), Arc::clone(link));
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(every).await;
                match afar.renew(id.clone()).await {
                    Err(why) if crate::lan_client::internet::lasting(why) => {
                        tempo_core::applog::warn(
                            "client",
                            &format!("the internet road's pipe was not renewed ({why})"),
                        );
                        link.close();
                        return;
                    }
                    _ => {}
                }
            }
        })
    }

    /// Tries the station's addresses on this network, the typed one first, every [`Timing::lan_probe`] (longer after a
    /// try that found it but could not open, [`Client::lan_backoff`]) until one answers, then
    /// says so in `found`.
    fn try_this_network(
        &self,
        (find, typed): (&Find, Option<Typed>),
        timing: Timing,
        found: &Arc<AtomicBool>,
    ) -> tauri::async_runtime::JoinHandle<()> {
        let (find, id, found) = (find.clone(), self.station_id.clone(), Arc::clone(found));
        let lookup = self
            .lookup
            .get()
            .cloned()
            .unwrap_or_else(|| Arc::new(tempo_stream::lan::lookup));
        let every = timing.lan_probe * self.lan_backoff.load(Ordering::SeqCst);
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(every).await;
                let asked = (find.clone(), id.clone(), typed.clone(), lookup.clone());
                // A typed host name is looked up again on every try.
                let addresses = tokio::task::spawn_blocking(move || match (asked.0)(&asked.1) {
                    Ok(Some(record)) => {
                        road::order(&record, asked.2.and_then(|typed| typed.resolve(&asked.3)))
                    }
                    _ => Vec::new(),
                })
                .await
                .unwrap_or_default();
                for at in addresses {
                    let answered =
                        tokio::time::timeout(LAN_PROBE_WAIT, tokio::net::TcpStream::connect(at))
                            .await;
                    if matches!(answered, Ok(Ok(_))) {
                        found.store(true, Ordering::SeqCst);
                        return;
                    }
                }
            }
        })
    }

    /// Nothing is on the air: no press is held, this computer's over is not on the air as its
    /// Stop road last heard it, and the station's newest snapshot does not say it is transmitting.
    /// A link may then be left for another road without cutting an over. A courtesy, not a guard:
    /// leaving a link mid-over ends the over, which is the safe way to be wrong.
    fn nothing_on_the_air(&self, now: Instant) -> bool {
        let pressed = matches!(&*lock(&self.press), Press::Held { .. });
        let shack = self.shack();
        let over = shack
            .as_ref()
            .and_then(|shack| shack.stop_road())
            .and_then(|road| road.over(now));
        let transmitting = shack
            .and_then(|shack| shack.latest("snapshot/1"))
            .is_some_and(|(_, snapshot)| snapshot["radio"]["transmitting"] == true);
        !pressed && !transmitting && !matches!(over, Some(Over::OnAir { .. } | Over::NoWord))
    }

    /// One turn of the lease: renewed while held, taken when control is free, and the banner told
    /// how it stands. A shack busy for a moment answers `stationBusy` (its engine) or `remoteBusy`
    /// (its authority's lock, held by another request that moment); the lease outlasts several of
    /// those, so it is simply asked again next turn.
    async fn keep_the_lease(&self, link: &Link, lease: Option<String>) -> Option<String> {
        if let Some(held) = lease {
            return match operate(link, json!({"type":"heartbeat","leaseId":held})).await {
                Ok(_) => {
                    self.set(Phase::Live, None);
                    Some(held)
                }
                Err(code) if busy(code) => Some(held),
                // Another device holds it now: it took control from this one.
                Err("notController") => {
                    self.taken_over.store(true, Ordering::SeqCst);
                    self.set(Phase::Busy, Some(TAKEN_OVER));
                    None
                }
                // Gone, or taken back by the station: taken again next turn, if control is free.
                Err(_) => None,
            };
        }
        let state = match operate(link, json!({"type":"state"})).await {
            Ok(state) => state,
            Err(_) => return None,
        };
        let taking = self.take.swap(false, Ordering::SeqCst) && state["phase"] == "occupied";
        match state["phase"].as_str() {
            Some("available") | Some("controlling") => {}
            Some("occupied") if taking => {}
            Some("occupied") => {
                self.set(Phase::Busy, self.taken_over());
                return None;
            }
            _ => {
                self.set(Phase::Refused, Some("localPermissionRequired"));
                return None;
            }
        }
        let acquire = if taking {
            json!({"type":"takeControl","stationBootId":state["stationBootId"]})
        } else {
            json!({"type":"acquire","stationBootId":state["stationBootId"]})
        };
        match operate(link, acquire).await {
            Ok(taken) => {
                let lease = taken["leaseId"].as_str().map(str::to_string);
                if lease.is_some() {
                    self.taken_over.store(false, Ordering::SeqCst);
                    link.watch(&FEEDS);
                    self.set(Phase::Live, None);
                }
                lease
            }
            Err("controllerBusy") => {
                self.set(Phase::Busy, self.taken_over());
                None
            }
            Err(code) => {
                // The shack was busy a moment: the take is asked again next turn.
                if taking && busy(code) {
                    self.take.store(true, Ordering::SeqCst);
                }
                None
            }
        }
    }

    /// The banner's reason while another device holds control: that it took it from this one.
    fn taken_over(&self) -> Option<&'static str> {
        self.taken_over.load(Ordering::SeqCst).then_some(TAKEN_OVER)
    }

    /// Asks the shack what became of the calls a dropped link left unanswered, and says so in the
    /// diagnostic log. They are never sent again.
    async fn ask_after_the_gone(&self, link: &Link) {
        let gone: Vec<u64> = std::mem::take(&mut *lock(&self.gone));
        for ids in gone.chunks(ASK_AT_ONCE) {
            let outcomes = link.outcomes(ids).await;
            tempo_core::applog::info(
                "client",
                &format!(
                    "calls cut off by the dropped link: {ids:?}, at the station: {outcomes:?}"
                ),
            );
        }
    }

    /// Lets the station go: the lease handed back, so control is free at once for the shack's own
    /// screen or another device, and the link closed. Bounded, since it runs as Nexus quits.
    pub(crate) async fn let_go(&self) {
        let held = lock(&self.held).take();
        if let Some((link, lease)) = held {
            let release = operate(&link, json!({"type":"release","leaseId":lease}));
            let _ = tokio::time::timeout(Duration::from_secs(1), release).await;
            link.close();
        }
        self.install(None);
    }
}

/// One of the lease's operation requests on `link`, with a fresh request id: its value, or the
/// station's error code (`linkEnded` once the link has gone).
async fn operate(link: &Link, mut request: Value) -> Result<Value, &'static str> {
    request["requestId"] = json!(request_id());
    let answer = link.operation(request).await.ok_or("linkEnded")?;
    match answer["error"].as_str() {
        Some(code) => Err(KNOWN
            .iter()
            .find(|known| **known == code)
            .copied()
            .unwrap_or("refused")),
        None => Ok(answer["value"].clone()),
    }
}

/// The banner's reason when another device took control from this computer.
const TAKEN_OVER: &str = "takenOver";

/// The operation errors client mode tells apart; any other reads as `refused`.
const KNOWN: [&str; 7] = [
    "stationBusy",
    "remoteBusy",
    "controllerBusy",
    "leaseExpired",
    "notController",
    "localPermissionRequired",
    "staleStation",
];

/// Whether an operation's error says only that the shack was busy a moment, its engine
/// (`stationBusy`) or its authority's lock (`remoteBusy`), and not that the request was refused.
fn busy(code: &str) -> bool {
    matches!(code, "stationBusy" | "remoteBusy")
}

/// A request id: 36 lowercase characters in the station's identifier shape, at random.
fn request_id() -> String {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; 16];
    let _ = ring::rand::SystemRandom::new().fill(&mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// The main window, sent from the page it waits on to the app, then shown, and the splash closed.
async fn show_the_app<R: Runtime>(app: &tauri::AppHandle<R>) {
    if crate::send_main_window_to_the_app(app).is_err() {
        tempo_core::applog::error("startup", "client mode: the main window could not be shown");
        return;
    }
    // The time the station's own launch gives the app to draw behind the splash.
    tokio::time::sleep(Duration::from_secs(2)).await;
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
    if let Some(splash) = app.get_webview_window("splashscreen") {
        let _ = splash.close();
    }
}

// --- Starting again ------------------------------------------------------------------------------

/// The arguments to start Nexus with once this one has quit, when it is to start again.
static AGAIN_WITH: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Starts this Nexus again with `args` once it has quit, and quits it: the main window is closed,
/// so the quit is the one a window close makes (at the shack's own station: the radio stopped
/// first and the logbook's changes saved, held on screen while they are). The profile it runs in
/// goes along.
pub(crate) fn start_again<R: Runtime>(app: &tauri::AppHandle<R>, mut args: Vec<String>) {
    if let Some(profile) = crate::active_profile() {
        args.splice(0..0, ["--profile".to_string(), profile.to_string()]);
    }
    *lock(&AGAIN_WITH) = Some(args);
    match app.get_webview_window("main") {
        Some(main) => {
            let _ = main.close();
        }
        None => app.exit(0),
    }
}

/// As the process ends: starts the Nexus [`start_again`] asked for, if it did.
pub(crate) fn started_again() {
    let args = lock(&AGAIN_WITH).take();
    let Some(args) = args else { return };
    let started = std::env::current_exe()
        .map_err(|e| e.to_string())
        .and_then(|exe| {
            tempo_core::process::command(exe)
                .args(&args)
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
    if let Err(why) = started {
        tempo_core::applog::error("startup", &format!("could not start Nexus again: {why}"));
    }
}

/// Whether this computer drives a paired station, and how its link stands: what the screens'
/// banner shows. `None` on a computer running its own station.
#[tauri::command]
pub(crate) fn client_status(app: tauri::AppHandle) -> Option<Status> {
    app.try_state::<Arc<Client>>().map(|client| client.status())
}

/// The operator did something in a window: a click, a key or the wheel. On the internet road it
/// answers "Still there?", and after a link nobody answered it for, connects again. Nothing on a
/// computer running its own station.
#[tauri::command]
pub(crate) fn client_still_here(app: tauri::AppHandle) {
    if let Some(client) = app.try_state::<Arc<Client>>() {
        client.still_here();
    }
}

/// The operator confirmed Take control in the banner: this computer takes control from the device
/// holding the station. Nothing on a computer running its own station.
#[tauri::command]
pub(crate) fn client_take_control(app: tauri::AppHandle) {
    if let Some(client) = app.try_state::<Arc<Client>>() {
        client.take_control();
    }
}

/// This computer ▸ Audio as it stands: the output and the microphone chosen ("" for the system's
/// default), this computer's devices to choose from, and why either would not open, if it would
/// not. Only on a computer driving a station.
#[tauri::command]
pub(crate) fn client_audio(app: tauri::AppHandle) -> Result<Value, String> {
    let client = app
        .try_state::<Arc<Client>>()
        .ok_or_else(|| "notClient".to_string())?;
    Ok(client.audio_view())
}

/// Picks the headset for the shack's audio, and how loud the station is heard here (the volume as
/// it was when none is given): kept in this computer's own file, and used from now on.
#[tauri::command]
pub(crate) fn set_client_audio(
    app: tauri::AppHandle,
    output: String,
    input: String,
    volume_db: Option<f64>,
) -> Result<Value, String> {
    let client = app
        .try_state::<Arc<Client>>()
        .ok_or_else(|| "notClient".to_string())?;
    if output.len() > DEVICE_NAME || input.len() > DEVICE_NAME {
        return Err(word(Refusal::BadArguments.code()));
    }
    let volume_db = match volume_db {
        Some(db) => volume(Some(db)),
        None => lock(&client.this).volume_db,
    };
    lock(&client.this).keep_audio(&output, &input, volume_db)?;
    if let Some(headset) = client.headset.get() {
        headset.devices(&output, &input);
        headset.rings().set_volume_db(volume_db);
    }
    Ok(client.audio_view())
}

/// The meter: the microphone's loudest moment since the last ask, `0` to `1`, keeping it open a
/// moment longer, and whether it is open.
#[tauri::command]
pub(crate) fn client_audio_level(app: tauri::AppHandle) -> Result<Value, String> {
    let client = app
        .try_state::<Arc<Client>>()
        .ok_or_else(|| "notClient".to_string())?;
    let headset = client
        .headset
        .get()
        .ok_or_else(|| "audioUnavailable".to_string())?;
    Headset::want_input(&**headset);
    let (_, open) = headset.open();
    let (_, trouble) = headset.trouble();
    Ok(json!({ "level": Headset::rings(&**headset).peak(), "open": open, "trouble": trouble }))
}

/// The test: a steady note in the output chosen, once it is open.
#[tauri::command]
pub(crate) async fn client_audio_test(app: tauri::AppHandle) -> Result<Value, String> {
    let client = app
        .try_state::<Arc<Client>>()
        .ok_or_else(|| "notClient".to_string())?
        .inner()
        .clone();
    let headset = client
        .headset
        .get()
        .ok_or_else(|| "audioUnavailable".to_string())?
        .clone();
    Headset::want_output(&*headset);
    // The output opens on the headset's own thread within a moment, unless it cannot.
    for _ in 0..50 {
        if let Some(rate) = Headset::rings(&*headset).out_rate() {
            Headset::rings(&*headset).sound(&audio_road::sound(audio_road::Sound::Test, rate));
            return Ok(json!({ "played": true }));
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (trouble, _) = headset.trouble();
    Ok(json!({ "played": false, "trouble": trouble }))
}

impl Client {
    /// This computer ▸ Audio as it stands.
    fn audio_view(&self) -> Value {
        let (output, input, volume_db) = {
            let this = lock(&self.this);
            (this.output.clone(), this.input.clone(), this.volume_db)
        };
        let (inputs, outputs) = audio_road::devices();
        let listed = |devices: audio_road::Listed| {
            devices
                .into_iter()
                .map(|(name, label)| json!({ "name": name, "label": label }))
                .collect::<Vec<_>>()
        };
        let (out_trouble, in_trouble) = self
            .headset
            .get()
            .map(|headset| headset.trouble())
            .unwrap_or_default();
        json!({
            "output": output,
            "input": input,
            "volumeDb": volume_db,
            "outputs": listed(outputs),
            "inputs": listed(inputs),
            "available": self.headset.get().is_some(),
            "trouble": { "output": out_trouble, "input": in_trouble },
        })
    }
}

/// Leave: this computer's Nexus starts again as itself, the station handed back first. With
/// `pair_again`, asked from the banner while the station refuses this computer in a way only a new
/// pairing ends ([`PAIR_AGAIN`]): the refusal stays noted beside the target, so the start that
/// follows opens Stations on this network offering Pair again for that station ([`pair_again`]).
#[tauri::command]
pub(crate) fn leave_client_mode(
    app: tauri::AppHandle,
    pair_again: Option<bool>,
) -> Result<(), String> {
    let Some(client) = app.try_state::<Arc<Client>>() else {
        return Err("notClient".to_string());
    };
    let refused = lock(&client.status)
        .reason
        .clone()
        .filter(|reason| PAIR_AGAIN.contains(&reason.as_str()));
    match refused {
        Some(reason) if pair_again == Some(true) => {
            note_refusal_at(&target_path(), &client.station_id, Some(&reason));
        }
        // The operator asked to come back to this computer's own station: forget the last client
        // target so the next cold launch boots as itself, not back into the station just left.
        _ => clear_target(),
    }
    start_again(&app, Vec::new());
    Ok(())
}

/// This run in client mode: the app with no station of its own, driving the station `station`.
/// Client mode's drive of the station, from this computer's store of paired stations: the
/// station's record read from the store each time, and the address or host name the operator typed
/// for it (`typed`, from `--client-at` on a warm launch or the kept target on a cold one) tried
/// first, then kept with the station once Operate opens there.
pub(crate) fn start_driving<R: Runtime>(
    client: Arc<Client>,
    app: tauri::AppHandle<R>,
    stations: Arc<crate::lan_client::Stations>,
    look: Look,
    typed: Option<Typed>,
) -> tauri::async_runtime::JoinHandle<()> {
    client.keep_typed_with(Arc::clone(&stations) as Arc<dyn Keep>);
    let find: Find = Arc::new(move |id: &str| stations.get(id));
    tauri::async_runtime::spawn(client.drive(app, find, look, typed))
}

/// When Operate's launch signs in again for station `id`: the day the station stops taking this
/// computer on its network, once that is within `tempo_client::account::RENEW_BEFORE_S` or past, for
/// a station added with the account. `None` for one paired with a code, or signed in to lately.
pub(crate) fn sign_in_again_by(
    stations: &crate::lan_client::Stations,
    id: &str,
    now_s: u64,
) -> Option<u64> {
    let issued_at = stations.signed(id)?;
    tempo_client::account::renew_due(issued_at, now_s)
        .then(|| tempo_client::account::admitted_until(issued_at))
}

/// Signs in again for the station `client` drives, through the system browser
/// (`lan_client::account::sign_in`), saying so on the banner; `by` stays there when it did not
/// happen. Nothing is picked and nobody is asked where the station is: it is the one this launch
/// drives, at the addresses it is known at.
fn sign_in_again<R: Runtime>(
    app: tauri::AppHandle<R>,
    client: Arc<Client>,
    stations: Arc<crate::lan_client::Stations>,
    by: u64,
) {
    use crate::lan_client::account::{Ask, Env};
    client.signing_again(Some(by), true);
    tauri::async_runtime::spawn(async move {
        use tauri_plugin_opener::OpenerExt;
        let env = Env::new(
            stations,
            Arc::new(tempo_stream::lan::dnssd::find),
            Arc::new(tempo_stream::lan::lookup),
            crate::lan_client::computer_name(),
        );
        let signed = match env {
            Ok(env) => {
                let open = |url: &str| {
                    app.opener()
                        .open_url(url, None::<&str>)
                        .map_err(|e| e.to_string())
                };
                let choose: crate::lan_client::internet::Choose =
                    Arc::new(|_| Box::pin(async { None }));
                let ask: Ask = Arc::new(|_, _| Box::pin(async { None }));
                let id = client.station_id.clone();
                crate::lan_client::account::sign_in(&env, Some(id), open, choose, ask).await
            }
            Err(why) => Err(why),
        };
        client.signing_again(signed.is_err().then_some(by), false);
    });
}

pub(crate) fn run(station: String, typed: Option<Typed>) {
    tempo_core::applog::info(
        "startup",
        "client mode: driving a paired station; this computer's own station stays off",
    );
    // This computer is now driving this station: keep it as the last client target so a later
    // cold launch connects to it again. Warm or cold, the target reflects the station and the
    // typed address this launch actually drives.
    keep_target(&station, typed.as_ref());
    let this = ThisComputer::read(crate::settings_path(), crate::ui_state_path());
    let headset = audio_road::headset(&this.output, &this.input);
    if let Some(headset) = &headset {
        headset.rings().set_volume_db(this.volume_db);
    }
    let client = Arc::new(Client::new(station, this));
    client.note_refusals_in(target_path());
    match headset {
        Some(headset) => client.hear_with(headset),
        None => tempo_core::applog::warn(
            "client",
            "this computer's headset did not start: the station's audio is not heard here",
        ),
    }
    let local = crate::app_commands();
    let routed = Arc::clone(&client);
    let driving = Arc::clone(&client);
    let built = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        // WebView2's browser keys off in every window here too, as in the station's own app.
        .plugin(crate::browser_keys::init())
        .manage(Arc::clone(&client))
        .manage(crate::BetaUpdateState::default())
        .manage(crate::street_map::StreetMapState::default())
        .manage(crate::remote_window::LanOrigin::default())
        .invoke_handler(move |invoke| routed.handle(invoke, &*local))
        .setup(move |app| {
            if let Ok(dir) = app.path().resource_dir() {
                let _ = crate::RESOURCE_DIR.set(dir);
            }
            let _ = crate::DISPLAY_DPI.set(crate::probe_physical_dpi());
            if let Ok(dir) = app.path().document_dir() {
                let _ = crate::DOCUMENTS_DIR.set(dir);
            }
            if let Ok(dir) = app.path().picture_dir() {
                let _ = crate::PICTURES_DIR.set(dir);
            }
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.set_min_size(Some(tauri::LogicalSize::new(900.0, 600.0)));
            }
            // This computer's window layouts, restored and kept here.
            crate::window_state::install(app.handle());
            // The window is up: a panic from here on is not a launch that failed to show one.
            crate::startup_reached_window();
            let handle = app.handle().clone();
            driving.before_install(Box::new(move || {
                crate::capture_all_window_geometry(&handle);
                tempo_core::applog::flush();
            }));
            let stations = Arc::new(
                crate::lan_client::Stations::new(Arc::new(
                    crate::remote_service::vault::SystemVault,
                ))
                .typed_in(crate::lan_client::typed_path())
                .signed_in(crate::lan_client::signed_path()),
            );
            // In the last week before the station stops taking this computer on its network, this
            // launch signs in again by itself (see the header).
            let now_s = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if let Some(by) = sign_in_again_by(&stations, &driving.station_id, now_s) {
                sign_in_again(
                    app.handle().clone(),
                    Arc::clone(&driving),
                    Arc::clone(&stations),
                    by,
                );
            }
            match crate::lan_client::internet::Roads::new(Arc::clone(&stations)) {
                Ok(roads) => driving.reach_afar(Arc::new(roads)),
                Err(why) => tempo_core::applog::warn(
                    "client",
                    &format!("no internet road from this computer ({why})"),
                ),
            }
            let look: Look = Arc::new(tempo_stream::lan::dnssd::find);
            start_driving(driving, app.handle().clone(), stations, look, typed);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let app = window.app_handle();
                if let Some(w) = app.get_webview_window(window.label()) {
                    crate::capture_bandmap_window(&w);
                }
                // Closing the main window ends the app, and its pop-outs with it.
                if window.label() == "main" {
                    for (label, w) in app.webview_windows() {
                        if label != "main" {
                            crate::capture_bandmap_window(&w);
                            let _ = w.close();
                        }
                    }
                }
            }
        })
        .build(crate::context());
    let app = match built {
        Ok(app) => app,
        Err(e) => {
            let why = format!("client mode could not start: {e}");
            tempo_core::applog::error("startup", &why);
            crate::show_startup_failure(&why);
            return;
        }
    };
    app.run(move |handle, event| match event {
        tauri::RunEvent::ExitRequested { .. } => {
            crate::capture_all_window_geometry(handle);
            tauri::async_runtime::block_on(client.let_go());
        }
        tauri::RunEvent::Exit => {
            crate::capture_all_window_geometry(handle);
            started_again();
        }
        _ => {}
    });
}

/// A lock, taken back from a panic that poisoned it: every value here is only ever replaced whole.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
