//! The satellite section over Remote: arm and stop a pass, pick a transponder, the Doppler
//! switch, the uplink mapping and its consent, peg-lock, and the manual element refresh.
//!
//! ⭐ **Every verb here is the DESKTOP's verb.** Each arm calls the same function the Tauri
//! command calls (`arm_sat_track`, `disarm_sat_track`, `pick_sat_transponder`, `write_sat_doppler`,
//! `write_sat_uplink`, `write_peg_lock`, `tle_refresh_flight_now`), so a browser gesture and a
//! click at the shack cannot drift apart — there is no second implementation to keep in step. The
//! consent gates those functions carry are the gates: a transponder pick still refuses a dead row
//! and a pinned mid-pass re-pick, an arm still refuses elements past the 30 d ceiling, and the
//! per-tick Doppler consent — a mapping confirmed FOR THE RADIO IN PLAY before the transmit leg is
//! ever written — is untouched by anything in this file.
//!
//! **Why every one of them runs on its own thread.** The dispatcher calls in holding the Engine
//! lock, and each of these verbs takes that same lock itself; a direct call would deadlock. So
//! each gesture is handed to a worker exactly as the rotator's is, and the receipt stays pending
//! until the worker answers. The worker rechecks the gesture's authority at its write boundary,
//! so a gesture whose lease lapsed between admission and execution writes nothing.
//!
//! **An operator gesture only.** Nothing here is reachable from a timer, a pass alarm or a spot,
//! and nothing here can arm, key or touch the transmit latch. Arming a track is the one gesture in
//! the app that makes the station steer the dial and the mast on its own for minutes, which is
//! exactly why it is a gesture and why the track it starts carries this browser's authority for
//! its whole life: see `arm_sat_track`, and `SatTrackLoss::RemoteAuthorityEnded` for what happens
//! to it when the browser goes away.
use std::time::Instant;
use tempo_app::remote_control::{Completion, Evidence, Outcome, Permit, Reason};
use tempo_app::settings::SatVfoMap;

/// One satellite gesture, resolved from the wire action before the worker starts.
pub enum Command {
    /// Arm auto-track for the pass whose AOS is this one.
    Track { name: String, aos_unix: i64 },
    /// Disarm auto-track and hand the dial back.
    StopTrack,
    /// Hold this transponder (`index`), or hand the dial back (`None`).
    Transponder {
        name: String,
        index: Option<usize>,
        auto: bool,
    },
    /// The `satDopplerOff` switch.
    Doppler { on: bool },
    /// The uplink mapping and its consent. `map` `None` = confirm the mapping already in force.
    UplinkMap {
        map: Option<SatVfoMap>,
        radio_id: Option<u32>,
    },
    /// Peg-lock the active radio.
    Peg { on: bool },
    /// One manual element-refresh attempt.
    Elements,
}

/// A bird name as the grammar admits it: the browser's own schedule row, bounded and printable.
/// The station resolves it against its TLE set (aliases included) and refuses anything it cannot
/// name, so this only keeps a wire value from being unbounded.
pub fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len()) && !name.bytes().any(|b| !(0x20..0x7f).contains(&b))
}

/// Admit one satellite gesture and hand it to its own worker thread. The Engine lock the caller
/// holds is never taken here — see the module header.
pub fn queue(
    command: Command,
    permit: Permit,
    engine: &crate::SharedEngine,
) -> Result<Completion, Reason> {
    if !permit.valid(Instant::now()) {
        return Err(Reason::AuthorityExpired);
    }
    if let Command::Track { name, .. } | Command::Transponder { name, .. } = &command {
        if !valid_name(name) {
            return Err(Reason::InvalidAction);
        }
    }
    // The authority this gesture was admitted under, with the execution deadline dropped: an armed
    // track outlives the permit by the length of a pass, and what it must NOT outlive is the
    // browser. Resolved here, before the worker, so the track carries the generation the gesture
    // was actually admitted under and not a later one.
    let standing = permit.standing();
    let completion = Completion::guarded(permit);
    let worker = completion.clone();
    let engine = engine.clone();
    std::thread::Builder::new()
        .name("remote-satellite".into())
        .spawn(move || run(&worker, &engine, command, standing))
        .map_err(|_| Reason::StationBusy)?;
    Ok(completion)
}

/// The worker body: act only while the gesture's authority still holds, then finish the receipt.
fn run(
    completion: &Completion,
    engine: &crate::SharedEngine,
    command: Command,
    standing: tempo_app::remote_control::Standing,
) {
    if !completion.begin_write(Instant::now()) {
        // Lapsed before the write: nothing at the station moved, so this is a refusal.
        completion.refuse(Reason::AuthorityExpired);
        return;
    }
    completion.finish(match command {
        Command::Track { name, aos_unix } => match crate::arm_sat_track(
            engine,
            name,
            Some(aos_unix),
            // ⭐ THE TRACK CARRIES THIS BROWSER'S AUTHORITY. A remotely armed pass that outlived
            // the browser that armed it would be the station steering the radio and the mast with
            // nobody watching. The loop re-reads this every tick and ends the pass — dial handback
            // and all — the moment it stops being held.
            Some(standing),
        ) {
            // The station would not arm: no grid, a bird it cannot name, no matching pass in the
            // next 48 h, or elements past the 30 d acting ceiling. Each is the station declining a
            // gesture, and the reason vocabulary has one word for that.
            Ok(None) | Err(_) => Outcome::Rejected {
                reason: Reason::InvalidAction,
            },
            Ok(Some(_)) => Outcome::Applied {
                evidence: Evidence::StationState,
            },
        },
        Command::StopTrack => {
            stop_track(engine);
            Outcome::Applied {
                evidence: Evidence::StationState,
            }
        }
        Command::Transponder { name, index, auto } => {
            match crate::pick_sat_transponder(engine, name, index, auto) {
                // A dead row, an index the bird does not have, a transponder with no downlink to
                // tune, or an auto re-pick against the row a pass is being worked on.
                Err(_) => Outcome::Rejected {
                    reason: Reason::InvalidAction,
                },
                // Whether the radio actually MOVED is not claimed here: the pick sets the hold and
                // asks for the tune, and the honest answer rides the binding the rail already
                // polls (`get_sat_transponder`), exactly as it does on the desktop.
                Ok(()) => Outcome::Applied {
                    evidence: Evidence::StationState,
                },
            }
        }
        Command::Doppler { on } => match crate::write_sat_doppler(engine, on) {
            Ok(()) => Outcome::Applied {
                evidence: Evidence::SettingsSaved,
            },
            // A save that failed part way cannot say what the file holds.
            Err(Reason::PersistenceFailed) => Outcome::Unknown {
                reason: Reason::PersistenceFailed,
            },
            Err(reason) => Outcome::Rejected { reason },
        },
        Command::UplinkMap { map, radio_id } => {
            crate::write_sat_uplink(engine, map, radio_id);
            Outcome::Applied {
                evidence: Evidence::SettingsSaved,
            }
        }
        Command::Peg { on } => {
            crate::write_peg_lock(engine, on);
            Outcome::Applied {
                evidence: Evidence::SettingsSaved,
            }
        }
        Command::Elements => match crate::tle_refresh_flight_now() {
            // A refresh already in flight, or an unreadable cache: the desktop's own "could not
            // attempt at all".
            Err(_) => Outcome::Rejected {
                reason: Reason::StationBusy,
            },
            // The decision function said not now. Nothing was attempted and nothing is wrong: the
            // elements the station holds are the answer, and the rail already shows their age.
            Ok(None) => Outcome::Applied {
                evidence: Evidence::StationState,
            },
            // ⚠️ The single-flight latch is taken by the call above and released inside the
            // flight, so the flight MUST run — never skip it on a late authority check.
            Ok(Some(flight)) => {
                flight();
                Outcome::Applied {
                    evidence: Evidence::StationState,
                }
            }
        },
    });
}

/// Disarm auto-track and halt the mast. Split out because the remote Stop runs it too, from a
/// place that has no worker of its own — see `transmit_stop::stop_station`.
pub fn stop_track(engine: &crate::SharedEngine) {
    if let Some(addr) = crate::disarm_sat_track(engine) {
        // The halt is a blocking socket write and the Engine lock is already released, so it runs
        // here rather than on a thread of its own.
        let _ = tempo_audio::rotator::stop(&addr);
    }
}
