//! The rigctld shim for a FlexRadio: [`FlexShim`] serves one slice as the radio's dial, answering
//! reads from the session's status model and turning each write into ONE typed command or intent
//! of the protocol core. Every byte on the rigctld wire is still produced by
//! [`crate::rigctld_server::handle_command`], the encoder every backend shares.
//!
//! ## The served slice
//! Our transmit slice; else our lowest-numbered slice, receive only (admission refuses to key a
//! transmit slice that is not ours); else none, and the reads say so (`f` answers 0, which the
//! radio loop never takes for a dial). Another client's slice is never served. While the shim
//! holds a Rig split, the receive slice of its pair is served, though the flag is on the other.
//!
//! ## The verb map
//!
//! | rigctld | Typed command or intent | Gate |
//! |---|---|---|
//! | `T 1` (any non-zero) | `TxStart::Key` → `xmit 1` | the core's admission, after every engine gate the loop ran; with native audio on, a digital over only while the radio takes its audio from Nexus's DAX; with it off, never while Nexus's own write still has the radio on DAX; a voice over never while that write has the radio on DAX in place of the mic, unless the stream's browser voice rides it; a voice-keyer message only while the radio takes its audio from Nexus's DAX |
//! | `T 0` | everything of ours (`Connection::end_ours`): the open operation's own stop (`xmit 0`, `transmit tune 0`, `cwx clear`, or the ATU's `xmit 0` and `transmit tune 0`); with none open, every stop the radio's status shows an earlier session of ours may need | never gated; sent only for what is ours |
//! | `U TUNER <n≠0>` | `TxStart::AtuStart` → `atu start` | admission (refuses today: no readback); then a tuner the radio reports fitted, XIT off; a refusal, the client's or the radio's, says why on the ATU line ([`super::FlexDaemon::radio_atu`]) |
//! | `b <text>` | `TxStart::CwxSend` → `cwx send`, one word, with its keying time at the radio's speed | first, every character one Nexus can time; admission (refuses today: no readback); then the slice in CW, the radio's break-in on, Sync CWX off, XIT off; a refusal says why ([`super::FlexDaemon::key_refused_since`]) |
//! | `\stop_morse` | `TxStop::CwxClear` → `cwx clear` | never gated |
//! | `L KEYSPD <wpm>` | `Command::CwSpeed` → `cw wpm <5–100>` | keys nothing; the engine's WPM control decides the value |
//! | `F <hz>` | `slice tune <n> <MHz>`; on another band than a held split slice, `S 0`'s take-down first | the slice must be ours; refused if that take-down cannot be done now |
//! | `M <mode> <width>` | `slice set <n> mode=`, then `filt <n> <lo> <hi>` for a width | a mode the radio offers; a width placed on the mode's side |
//! | `L RFPOWER <0..1>` | `transmit set rfpower=<0–100>` | the engine's per-mode ceiling decides the value |
//! | `L AF <0..1>` | `slice set <n> audio_level=` | the slice must be ours |
//! | `U NB/NR/ANF <0/1>` | `slice set <n> nb=/nr=/anf=` | the slice must be ours |
//! | `f`, `m`, `t`, `v`, `s`, `l RFPOWER`, `l AF`, `l KEYSPD`, `u NB/NR/ANF` | read from the status model | — |
//! | `u TUNER` | the radio's `atu` status: `1` once a cycle has matched (`TUNE_SUCCESSFUL`, `TUNE_OK`), else `0` | only while the client runs the radio's ATU (its switch) and the radio reports a tuner fitted; otherwise not answered, so no ATU button is offered |
//! | `S 1 VFOB` | Rig split: `slice create` on the served slice's pan, frequency and mode, `slice set <new> audio_mute=1`, then `TxSlice` → `slice set <new> tx=1`; held, so a later `S 1` sends nothing while the flag is still there | the transmit slice's own admission (nothing of ours keyed, the interlock idle, the slice ours); a create or a move the radio refuses is `RPRT -1`, and a slice made for a split that did not happen is removed |
//! | `S 0` | `TxSlice` → `slice set <receive> tx=1`, then `slice remove <split slice>` | the same admission; nothing to undo is `RPRT 0`; one that cannot finish now is owed, finished at the radio loop's next quiet point; a receive slice closed outside Nexus lets the split go, the split slice left as the dial with its audio on |
//! | `I <hz>` | `slice tune <split slice> <MHz> autopan=0` | a split held; nothing of ours keyed and the interlock idle |
//! | `X <mode> <width>` | `slice set <split slice> mode=`, then `filt` for a width (`M`'s rules) | as `I` |
//! | `s`, `i`, `x` | the split flag and `VFOB` while the flag is on the pair's split slice (`TX` while an `S 0` is owed: Nexus's own leftover, never the operator's split); that slice's frequency and mode, else the served slice's | — |
//! | `\dump_caps` | the authored capabilities ([`authored_caps`]) | — |
//!
//! Everything else answers as the shared encoder answers a backend that lacks it (`RPRT -11`),
//! byte for byte. `T 1` is a key whatever its number (rigctld's `T 3` is a data-port key), as in
//! every other backend.
//!
//! ## A digital over never goes out on the radio's mic
//! While the operator's native audio is on and no other program feeds DAX, Nexus's over reaches
//! the radio only as DAX. The DAX source flag follows the TX slice's mode at quiet moments
//! (`super::routing`), so right after a mode change, or after the operator flips the flag by
//! hand, the radio can still be taking its audio from its mic input. Keying then would put
//! whatever that input hears on a digital frequency. So `T 1` is refused while the transmit
//! slice's mode is digital (the mode this shim just commanded counts, before the radio reports
//! it) and the radio does not report `transmit dax=1` on Nexus's own transmit stream. It only
//! refuses, sends nothing, and the loop reports a refused PTT, with the shim's reason
//! ([`super::FlexDaemon::key_refused_since`]: the rigctld answer, `RPRT -1`, carries none).
//!
//! ## Nor on a DAX nothing feeds
//! The other way round: native audio turned off (by the operator, or by the receive floor giving
//! up on DAX) hands Nexus's audio back to the sound card at once, but the operator's own setting,
//! the mic, comes back only at the next quiet point, after the boundary when the toggle fell
//! inside the guard before it. Keyed in between, the radio would take a digital over from the DAX
//! Nexus wrote, which nothing feeds: a silent over. So with native audio off, `T 1` is refused
//! for a digital transmit slice while Nexus's own write still has the radio on DAX
//! ([`super::routing::Routing::leaves_dax`]), the same way, with its own reason (operator ruling,
//! 2026-10-07, "Refuse that over").
//!
//! ## Nor a voice over on DAX
//! Phone the same way (operator ruling, 2026-10-08, "Same rule for Phone"): right after the
//! transmit slice leaves a digital mode for a voice mode, or native audio goes off, the radio
//! still takes its transmit audio from the DAX Nexus set until the routing's next quiet point puts
//! the operator's mic back, and a key held from then on holds that quiet point off, so the voice
//! would not reach the air for the whole over. `T 1` is refused for a voice transmit slice while
//! Nexus's own write still has the radio on DAX in place of the mic
//! ([`super::routing::Routing::leaves_voice_on_dax`]), with its own reason; not while the stream's
//! browser voice is live with native audio on, which rides DAX.
//!
//! ## A voice-keyer message only on DAX
//! The other way round again, for the voice keyer in Phone at the shack (operator ruling,
//! 2026-10-12, "Change it now"): its message is Nexus's own audio, which reaches the radio only as
//! DAX, while the radio keeps its mic between messages. The radio loop switches the radio to DAX
//! for the message and keys it once the radio reports it, and marks that one key
//! ([`super::FlexDaemon::set_voice_key`]). A marked `T 1` is refused, the digital over's way and
//! with its cause, in whatever mode the slice reports, unless the radio reports `transmit dax=1`
//! and Nexus's own transmit stream exists: the operator flipping SmartSDR's DAX button back
//! between the loop's look and the key must not put the room on the air in the message's place.
//! Its stop, `T 0`, is never marked.
//!
//! ## A CW word is timed at the radio's speed
//! The radio's own keyer sends each word (`cwx send`) and keys and unkeys itself with its
//! break-in; the readback proves the word ended once the radio holds idle past the word's
//! expected end, so the shim hands the session the word's keying time
//! ([`tempo_core::cw::morse_duration_ms`]) at the speed the radio reports, or at the speed the shim
//! has just set when that is slower and the radio has not reported it yet ([`word_wpm`]): a word
//! is never timed shorter than the radio keys it. A word with a character Nexus has no Morse for
//! is not sent: it cannot be timed (`morse_duration_ms` skips such a character, while the radio's
//! keyer may send it), so it could outlast its window and end in a false alarm. A word the client
//! does not send says why on the CW line, in place of the rigctld answer, `RPRT -1`.
//!
//! ## Rig split is a slice pair
//! FlexRadio's API has no split frequency: a slice is a receiver, and the slice whose `tx` flag is
//! set is where the radio transmits (`TCPIP-slice`: `slice create`, the `tx` parameter of
//! `slice set`, `slice remove`). So `S 1` makes a second slice of ours where the served one is,
//! mutes its audio, and moves the flag to it; `I` and `X` tune and set that slice; `S 0` moves the
//! flag back and removes it. The pair is held between overs: a later `S 1` finds the flag still on
//! the split slice and sends nothing, so overs never open a slice each against the radio's few.
//! Nothing moves the flag, tunes the split slice or changes its mode while anything of ours is
//! keyed or the interlock is not idle: each verb waits for that within its own answer's time and
//! is refused if it does not come, and the tune and mode of `I` and `X` are checked again where a
//! key of ours is admitted ([`Connection::request_quiet`]). The flag is never taken from another
//! client's slice. An `S 0` that cannot finish is owed, not forgotten: it is finished at the
//! radio loop's next quiet point ([`settle_split`]), and until then nothing is keyed while the flag
//! is on the split slice. A pair belongs to the session that made it, and a slice whose create
//! went unanswered is removed when it appears.
//!
//! Nexus's own design, not a port.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use tempo_app::dto::{FlexAudioCause, FlexAudioRefusal};
use tempo_net::flex::admission::{another_dax_feeder, transmitter_idle, Refusal};
use tempo_net::flex::encode::{
    Command, CwxText, Mode, SliceFunction, StartKind, TxSlice, TxStart, TxStop,
};
use tempo_net::flex::model::{owner_of, Owner, StatusModel};
use tempo_net::flex::session::{
    ConnError, Connection, EndOutcome, SendError, Snapshot, StopOutcome,
};
use tempo_net::flex::status::SliceDelta;
use tempo_net::flex::wire::Reply;

use crate::baud_ladder::{RigCaps, SplitDetect};
use crate::rigctld_server::{RigBackend, SetFault};

use super::routing::{mode_class, ModeClass};
use super::{effective_mode, routing_view, ClientState, COMMANDED_FOR};

/// How long a write waits for the radio's reply: well inside the radio loop's own CAT deadline,
/// so a radio that does not answer gets an answer of its own, not a dropped CAT connection. Every
/// write that waits on it tells that silence apart from a refusal ([`answered`]): the dial, the mode
/// and its width, the levels and the noise switches.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);

/// A write's reply as the rigctld answer it becomes: the radio's success is `Ok`; its error code,
/// or the session declining to send the command, is a refusal (`RPRT -1`); no reply within the
/// wait, or a session that has ended, is the radio not answering (`RPRT -5`), which the radio
/// loop words and counts as silence.
fn answered(reply: Result<Reply, ConnError>) -> Result<(), SetFault> {
    match reply {
        Ok(r) if r.code == 0 => Ok(()),
        Err(ConnError::Timeout | ConnError::Closed) => Err(SetFault::NoAnswer),
        _ => Err(SetFault::Refused),
    }
}

/// The modes a slice offers when the radio has not sent its `mode_list` (FlexRadio's API
/// documentation, `TCPIP-slice`).
const FLEX_MODES: [&str; 11] = [
    "USB", "LSB", "CW", "AM", "SAM", "FM", "NFM", "DFM", "DIGU", "DIGL", "RTTY",
];

/// The slice this shim serves as the radio's dial: our transmit slice, else our lowest-numbered
/// slice, else none.
pub(crate) fn served_slice(model: &StatusModel, ours: Option<u32>) -> Option<u8> {
    let owned =
        |i: &u8| owner_of(model.slices.get(i).and_then(|s| s.client_handle), ours) == Owner::Ours;
    if let [tx] = model.tx_slices().as_slice() {
        if owned(tx) {
            return Some(*tx);
        }
    }
    model.slices.keys().copied().find(|i| owned(i))
}

/// A Rig split's slice pair: the receive slice the shim keeps serving as the dial, and the slice
/// of ours it made for the split and moves the transmit flag to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SplitPair {
    pub(crate) rx: u8,
    pub(crate) tx: u8,
}

/// The Rig split the shim holds ([`ClientState::split`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SplitHold {
    pub(crate) pair: SplitPair,
    /// The client handle of the session that made the pair. Slice numbers mean nothing to another
    /// session, so a hold is used by its own session only ([`split_hold`]).
    pub(crate) handle: u32,
    /// `S 0` asked for the split to go and it has not gone yet: the flag could not move back while
    /// something transmitted, or the radio was slow. Finished at the radio loop's next quiet point
    /// ([`settle_split`]); until then `s` names the transmit VFO `TX`, which says the split is
    /// Nexus's own leftover and not the operator's, and nothing is keyed while the flag is still on
    /// the split slice.
    pub(crate) releasing: bool,
}

/// A split slice asked for whose answer never named it (no answer in time, or one that could not
/// be read): the radio may still make it. The slices of ours before the ask are kept, so a new one
/// can be told apart and removed ([`settle_split`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingCreate {
    handle: u32,
    known: Vec<u8>,
    at: Instant,
}

/// The last `I` and `X` the radio answered OK for a split slice, for the session that sent them.
/// The engine writes both before every over on a held split; one that would change nothing (this
/// record matches AND the radio reports the slice so) is answered at once with nothing sent. Each
/// write forgets its half before it goes out, so after a refusal, a timeout or another session
/// the next one is always a real write.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SplitWritten {
    handle: u32,
    slice: u8,
    /// `I`'s frequency.
    freq_hz: Option<u64>,
    /// `X`'s mode word (the Flex one) and width, as asked.
    mode: Option<(String, i32)>,
}

/// The split slice and a copy of the record for it, while the radio reports the flag on that
/// slice and nothing of ours is keyed: when an unchanged `I` or `X` may be answered at once.
/// Reads only; waits for nothing.
fn split_unchanged(conn: &Connection, state: &ClientState) -> Option<(Snapshot, u8, SplitWritten)> {
    let (snap, tx, releasing) = split_on_air(conn, state)?;
    if releasing || !quiet(&snap) {
        return None;
    }
    let written = lock(&state.split_written).clone()?;
    (Some(written.handle) == snap.handle && written.slice == tx).then_some((snap, tx, written))
}

/// Change the record for `slice` of this session's split by `f`, starting a fresh one when it is
/// another's.
fn note_written(
    state: &ClientState,
    snap: &Snapshot,
    slice: u8,
    f: impl FnOnce(&mut SplitWritten),
) {
    let Some(handle) = snap.handle else {
        return;
    };
    let mut written = lock(&state.split_written);
    let entry = written.get_or_insert_with(SplitWritten::default);
    if entry.handle != handle || entry.slice != slice {
        *entry = SplitWritten {
            handle,
            slice,
            ..SplitWritten::default()
        };
    }
    f(entry);
}

/// How long a slice asked for may still appear. A radio answers a create in milliseconds; past
/// this the record is dropped rather than kept as a reason to remove a slice made later.
const PENDING_CREATE_FOR: Duration = Duration::from_secs(10);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// [`served_slice`] while the shim may hold a Rig split: the receive slice of the pair while it
/// is still ours, else as with none.
pub(crate) fn served_slice_split(
    model: &StatusModel,
    ours: Option<u32>,
    split: Option<SplitPair>,
) -> Option<u8> {
    split
        .map(|p| p.rx)
        .filter(|rx| {
            owner_of(model.slices.get(rx).and_then(|s| s.client_handle), ours) == Owner::Ours
        })
        .or_else(|| served_slice(model, ours))
}

/// The split this session holds. A hold another session made is dropped here and never used:
/// its slice numbers may now be anyone's.
pub(crate) fn split_hold(state: &ClientState, handle: Option<u32>) -> Option<SplitHold> {
    let mut hold = lock(&state.split);
    match *hold {
        Some(h) if Some(h.handle) == handle => Some(h),
        Some(h) => {
            *hold = None;
            tempo_core::applog::info(
                "cat",
                &format!(
                    "Flex client: a Rig split made by an earlier session (0x{:08X}) is not used",
                    h.handle
                ),
            );
            None
        }
        None => None,
    }
}

fn set_hold(state: &ClientState, hold: Option<SplitHold>) {
    *lock(&state.split) = hold;
}

/// Whether `slice` is reported and ours.
fn is_ours(snap: &Snapshot, slice: u8) -> bool {
    owner_of(
        snap.model.slices.get(&slice).and_then(|s| s.client_handle),
        snap.handle,
    ) == Owner::Ours
}

/// What is left of a verb's time until `deadline`.
fn left(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

/// Nothing of ours keyed and the interlock idle: when the transmit slice may be moved, tuned or
/// re-moded.
fn quiet(snap: &Snapshot) -> bool {
    !snap.keyed && transmitter_idle(&snap.model).is_ok()
}

/// Why a split verb was refused, for the log: the rigctld answer carries none.
fn refuse_split(why: &str) -> SetFault {
    tempo_core::applog::info("cat", &format!("Flex client: Rig split refused: {why}"));
    SetFault::Refused
}

/// Wait until `deadline` for [`quiet`]; whether it came.
fn wait_quiet(conn: &Connection, deadline: Instant) -> bool {
    conn.wait_until(left(deadline), quiet)
}

/// Move the transmit flag to `slice` and wait until the radio reports it there.
fn move_flag(conn: &Connection, slice: u8, deadline: Instant) -> Result<(), SetFault> {
    match conn.move_tx(TxSlice { slice }, left(deadline)) {
        // Another station's transmitter is never taken: say so in plain words.
        Err(ConnError::Refused(SendError::NotAdmitted(Refusal::TxSliceNotOurs {
            slice: holder,
            owner,
        }))) => {
            return Err(refuse_split(&match owner {
                Owner::Foreign(h) => format!(
                    "another client (0x{h:08X}) has the transmitter on its slice {holder}, and \
                     Nexus does not take it away"
                ),
                _ => format!(
                    "the transmitter is on slice {holder}, which the radio has not said is \
                     Nexus's, and Nexus does not take it away"
                ),
            }))
        }
        reply => answered(reply)?,
    }
    if conn.wait_until(left(deadline), |s| s.model.tx_slices() == [slice]) {
        Ok(())
    } else {
        Err(SetFault::NoAnswer)
    }
}

/// Take a split down: the transmit flag back on the receive slice and the split slice removed.
/// Marks the hold releasing first, so a take-down that cannot finish now stays owed (see
/// [`SplitHold::releasing`]); clears it once nothing of the split is left. Nothing to take down is
/// success.
pub(crate) fn release_split(
    conn: &Connection,
    state: &ClientState,
    deadline: Instant,
) -> Result<(), SetFault> {
    let snap = conn.snapshot();
    let Some(hold) = split_hold(state, snap.handle) else {
        return Ok(());
    };
    let pair = hold.pair;
    set_hold(
        state,
        Some(SplitHold {
            releasing: true,
            ..hold
        }),
    );
    let tx_slices = snap.model.tx_slices();
    if is_ours(&snap, pair.tx) {
        if tx_slices.contains(&pair.tx) {
            if !is_ours(&snap, pair.rx) {
                // The receive slice was closed outside Nexus: the flag has no slice of ours to go
                // back to, so the split slice stays, as a plain slice with the flag, and is the
                // dial from now on (as `S 1` drops a pair whose receive slice has gone). Its
                // audio comes back, so the operator hears it.
                if !wait_quiet(conn, deadline) {
                    return Err(refuse_split("a transmission has not ended"));
                }
                let _ = conn.request(
                    Command::SliceAudioMute {
                        slice: pair.tx,
                        mute: false,
                    },
                    left(deadline),
                );
                tempo_core::applog::warn(
                    "cat",
                    &format!(
                        "Flex client: Rig split is off. Nexus's receive slice was closed outside \
                         Nexus, so the transmitter stays on slice {}, which is now the dial",
                        pair.tx
                    ),
                );
                let mut current = lock(&state.split);
                if current.is_some_and(|h| h.pair == pair && h.handle == hold.handle) {
                    *current = None;
                }
                return Ok(());
            }
            if !wait_quiet(conn, deadline) {
                return Err(refuse_split("a transmission has not ended"));
            }
            move_flag(conn, pair.rx, deadline)?;
        }
        answered(conn.request(Command::SliceRemove { slice: pair.tx }, left(deadline)))?;
    } else if tx_slices.is_empty() && is_ours(&snap, pair.rx) && quiet(&snap) {
        // The split slice was removed outside Nexus and took the flag with it: the flag goes back
        // to the receive slice, as `S 0` would have put it. Best effort: with no transmit slice
        // the next key is refused, and says so.
        let _ = move_flag(conn, pair.rx, deadline);
    }
    let mut current = lock(&state.split);
    if current.is_some_and(|h| h.pair == pair && h.handle == hold.handle) {
        *current = None;
    }
    Ok(())
}

/// Remove a split slice the radio made after its create went unanswered or unread
/// ([`PendingCreate`]): a slice of ours the shim did not know before the ask and that is not part
/// of a pair held now. A slice that holds the transmit flag is left (the radio gives a new slice
/// the flag only when it has none). Each removal waits for its answer until `deadline` at most.
fn reconcile_create(conn: &Connection, state: &ClientState, deadline: Instant) {
    let snap = conn.snapshot();
    let pair = split_hold(state, snap.handle).map(|h| h.pair);
    let strays: Vec<u8> = {
        let mut pending = lock(&state.split_create);
        let Some(p) = pending.as_ref() else {
            return;
        };
        if Some(p.handle) != snap.handle || p.at.elapsed() >= PENDING_CREATE_FOR {
            *pending = None;
            return;
        }
        let tx = snap.model.tx_slices();
        let strays: Vec<u8> = snap
            .model
            .slices
            .keys()
            .copied()
            .filter(|i| {
                is_ours(&snap, *i)
                    && !p.known.contains(i)
                    && !tx.contains(i)
                    && pair.is_none_or(|pr| pr.rx != *i && pr.tx != *i)
            })
            .collect();
        if strays.is_empty() {
            return;
        }
        *pending = None;
        strays
    };
    for slice in strays {
        tempo_core::applog::info(
            "cat",
            &format!(
                "Flex client: removing slice {slice}, made for a Rig split that did not happen"
            ),
        );
        let _ = conn.request(Command::SliceRemove { slice }, left(deadline));
    }
}

/// The Rig split's own quiet-point work, called by the radio loop where it carries out the
/// transmit audio routing ([`super::FlexDaemon::sync_tx_routing`]): never with anything of ours
/// keyed or about to be. Removes a split slice a lost create left behind, and finishes a
/// take-down `S 0` could not, or one the radio's status calls for (the split slice removed in
/// SmartSDR). Waits for nothing it cannot have at once, and only when there is something to do:
/// all of it shares one [`REQUEST_TIMEOUT`], so the radio loop is held at most that long. What
/// the time does not cover stays owed for the next quiet point.
pub(crate) fn settle_split(conn: &Connection, state: &ClientState) {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    reconcile_create(conn, state, deadline);
    let snap = conn.snapshot();
    let Some(hold) = split_hold(state, snap.handle) else {
        return;
    };
    // Owed: a take-down `S 0` asked for, or a slice of the pair the radio no longer reports as
    // ours (removed in SmartSDR).
    let owed = hold.releasing || !is_ours(&snap, hold.pair.tx) || !is_ours(&snap, hold.pair.rx);
    if !owed || !quiet(&snap) {
        return;
    }
    if release_split(conn, state, deadline).is_ok() {
        tempo_core::applog::info(
            "cat",
            "Flex client: Rig split is off; the transmit flag is back on the receive slice",
        );
    }
}

/// The split slice while the radio reports the transmit flag on it, and whether that split is
/// being let go ([`SplitHold::releasing`]).
fn split_on_air(conn: &Connection, state: &ClientState) -> Option<(Snapshot, u8, bool)> {
    let snap = conn.snapshot();
    let hold = split_hold(state, snap.handle)?;
    let pair = hold.pair;
    (is_ours(&snap, pair.rx) && is_ours(&snap, pair.tx) && snap.model.tx_slices() == [pair.tx])
        .then_some((snap, pair.tx, hold.releasing))
}

/// Why nothing may be keyed now, if the transmit flag is still on a split slice `S 0` asked to
/// take down: an over there would go out at an old transmit frequency, after a band change on the
/// old band.
pub(crate) fn split_blocks_key(conn: &Connection, state: &ClientState) -> Option<String> {
    let (snap, tx, releasing) = split_on_air(conn, state)?;
    if !releasing {
        return None;
    }
    let at = snap
        .model
        .slices
        .get(&tx)
        .and_then(|s| s.frequency_mhz)
        .map_or(String::new(), |mhz| format!(" at {mhz:.6} MHz"));
    Some(format!(
        "the transmitter is still on Nexus's split slice{at}, and turning split off has not \
         finished yet; Nexus finishes it as soon as the radio is idle"
    ))
}

/// A rigctld mode word as a slice's: the data modes have Flex names of their own, the rest are
/// the same word. `None` when the radio does not offer it (its `mode_list`, or the documented
/// modes when it has not sent one) — refused, never approximated: rigctld's `CWR` or `PKTFM`
/// have no Flex mode.
pub(crate) fn flex_mode_for(rigctld: &str, offered: Option<&[String]>) -> Option<String> {
    let word = match rigctld.trim().to_ascii_uppercase().as_str() {
        "PKTUSB" => "DIGU".to_string(),
        "PKTLSB" => "DIGL".to_string(),
        other => other.to_string(),
    };
    let ok = match offered {
        Some(list) => list.contains(&word),
        None => FLEX_MODES.contains(&word.as_str()),
    };
    ok.then_some(word)
}

/// A slice's mode as the rigctld word Nexus uses for every radio; the radio's own word where
/// rigctld has none.
pub(crate) fn rigctld_mode(flex: &str) -> String {
    match flex {
        "DIGU" => "PKTUSB",
        "DIGL" => "PKTLSB",
        other => other,
    }
    .to_string()
}

/// Which side of the slice's frequency a mode's passband lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Upper,
    Lower,
    Centred,
}

fn side(flex_mode: &str) -> Option<Side> {
    match flex_mode {
        "USB" | "DIGU" => Some(Side::Upper),
        "LSB" | "DIGL" => Some(Side::Lower),
        "CW" | "AM" | "SAM" | "FM" | "NFM" | "DFM" => Some(Side::Centred),
        _ => None,
    }
}

/// The filter edges (Hz from the slice's frequency) for a `width_hz` passband in `mode`.
///
/// A slice's filter is two edges, and rigctld sends a width, so one edge has to be chosen. Where
/// the slice already sits on the same side, its own edge stays where the operator put it (the low
/// edge on the upper side, the high edge on the lower side, the centre for a centred mode);
/// otherwise the edge is the slice's frequency itself. `None` for a mode with no side to place a
/// width on (RTTY's passband sits around its mark and shift), or a width that is not one.
pub(crate) fn filter_for(
    mode: &str,
    width_hz: u32,
    current_mode: Option<&str>,
    current: Option<(i32, i32)>,
) -> Option<(i32, i32)> {
    let width = i32::try_from(width_hz)
        .ok()
        .filter(|w| (1..=100_000).contains(w))?;
    let side = side(mode)?;
    let anchor = current.filter(|_| current_mode.and_then(self::side) == Some(side));
    Some(match side {
        Side::Upper => {
            let low = anchor.map_or(0, |(low, _)| low);
            (low, low + width)
        }
        Side::Lower => {
            let high = anchor.map_or(0, |(_, high)| high);
            (high - width, high)
        }
        Side::Centred => {
            let centre = anchor.map_or(0, |(low, high)| (low + high) / 2);
            let low = centre - width / 2;
            (low, low + width)
        }
    })
}

/// A FlexRadio's capabilities, AUTHORED rather than read out of Hamlib (the spec's "authored
/// `RigCaps`"):
/// - **split by slice pair**: Nexus's own Rig split is a slice pair the shim makes (`S`), and the
///   split detect is still "cannot": the privilege gate never takes an operator's split from
///   here — silence is never permission;
/// - **RF power floor 0**: `transmit set rfpower=` runs 0–100, so a reading of zero is a real
///   zero;
/// - **native VFO read**: the VFO this shim answers is the served slice itself, never a cache of
///   what Nexus last set;
/// - **receive coverage unknown**: the `radio` status names the model and carries a `bands`
///   value whose format is not established, so no coverage is claimed, and unknown fails open.
///   The transmit side is the licence table's, as on every radio.
pub fn authored_caps() -> RigCaps {
    RigCaps {
        serial_rates: None,
        rx_coverage: Vec::new(),
        split_detect: SplitDetect::Absent,
        rfpower_floor_milli: Some(0),
        vfo_read_native: true,
    }
}

/// The rigctld backend over one Flex session.
pub struct FlexShim {
    /// Weak, so the daemon's drop closes the session whatever clients are still connected here.
    conn: Weak<Connection>,
    /// True while Nexus itself intends to transmit: the broker's disconnect fail-safe stands
    /// down then. The same contract as the CI-V and OmniRig daemons'.
    tx_intent: Arc<AtomicBool>,
    state: Arc<ClientState>,
}

impl FlexShim {
    pub(crate) fn new(
        conn: Weak<Connection>,
        tx_intent: Arc<AtomicBool>,
        state: Arc<ClientState>,
    ) -> FlexShim {
        FlexShim {
            conn,
            tx_intent,
            state,
        }
    }

    /// Why a key must not go out on the audio route as it stands, if it must not: a digital
    /// over while the radio would take its audio from its mic input, or, with native audio off,
    /// from a DAX nothing feeds, a voice over while it would take its audio from DAX in place
    /// of the mic, and a voice-keyer message while it would not take Nexus's DAX (see the module
    /// header). In the shim's words, for the log and the refusal on screen, and as a cause, for
    /// the UI's own.
    pub(crate) fn audio_refuses_key(&self) -> Option<(String, FlexAudioRefusal)> {
        let native = self.state.native_audio.load(Ordering::Relaxed);
        let conn = self.conn.upgrade()?;
        let snap = conn.snapshot();
        if another_dax_feeder(&snap.model, snap.handle, snap.dax_tx_stream).is_some() {
            // Another program's DAX carries Nexus's audio from the sound card: not ours to judge.
            return None;
        }
        // No single transmit slice: admission refuses the key itself.
        let [slice] = snap.model.tx_slices()[..] else {
            return None;
        };
        let mode = effective_mode(&snap.model, slice, &self.state);
        if self.state.voice_key.load(Ordering::SeqCst) {
            // A voice-keyer message: Nexus's own audio, which only DAX carries, in whatever mode
            // the slice reports. Native audio is not asked: the message is played to the DAX
            // route or not at all.
            let from_dax = snap.model.transmit.dax == Some(true);
            if from_dax && snap.dax_tx_stream.is_some() {
                return None;
            }
            let mode = mode.unwrap_or_default();
            let why = format!(
                "not keying the voice keyer's {}message: the radio takes its transmit audio \
                 from {}{}",
                if mode.is_empty() {
                    String::new()
                } else {
                    format!("{mode} ")
                },
                if from_dax { "DAX" } else { "its mic input" },
                if snap.dax_tx_stream.is_some() {
                    ""
                } else {
                    " and Nexus's DAX transmit stream does not exist"
                }
            );
            return Some((
                why,
                FlexAudioRefusal {
                    mode,
                    cause: FlexAudioCause::NotYetDax,
                },
            ));
        }
        let mode = mode?;
        let class = mode_class(&mode);
        if class == ModeClass::Phone {
            // The operator's mic carries a voice over, unless the browser voice rides DAX.
            let view = routing_view(&snap, &self.state);
            let routing = self
                .state
                .routing
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !routing.leaves_voice_on_dax(&view, native) {
                return None;
            }
            let why = format!(
                "not keying a {mode} over: the radio still takes its transmit audio from the DAX \
                 Nexus set, not its mic input, until Nexus puts the mic back"
            );
            return Some((
                why,
                FlexAudioRefusal {
                    mode,
                    cause: FlexAudioCause::MicNotBack,
                },
            ));
        }
        if class != ModeClass::Digital {
            return None;
        }
        if !native {
            // The over goes to the sound card: only a DAX Nexus itself left the radio on carries
            // nothing.
            let view = routing_view(&snap, &self.state);
            let routing = self
                .state
                .routing
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !routing.leaves_dax(&view) {
                return None;
            }
            let why = format!(
                "not keying a {mode} over: native audio is off, and the radio still takes its \
                 transmit audio from the DAX Nexus set, which nothing feeds until its mic input \
                 is back"
            );
            return Some((
                why,
                FlexAudioRefusal {
                    mode,
                    cause: FlexAudioCause::DaxUnfed,
                },
            ));
        }
        let from_dax = snap.model.transmit.dax == Some(true);
        if from_dax && snap.dax_tx_stream.is_some() {
            return None;
        }
        let why = format!(
            "not keying a {mode} over: the radio takes its transmit audio from {}{}",
            if from_dax { "DAX" } else { "its mic input" },
            if snap.dax_tx_stream.is_some() {
                ""
            } else {
                " and Nexus's DAX transmit stream does not exist yet"
            }
        );
        Some((
            why,
            FlexAudioRefusal {
                mode,
                cause: FlexAudioCause::NotYetDax,
            },
        ))
    }

    /// The session, its state, and the slice served now.
    fn served(&self) -> Option<(Arc<Connection>, Snapshot, u8)> {
        let conn = self.conn.upgrade()?;
        let snap = conn.snapshot();
        let pair = split_hold(&self.state, snap.handle).map(|h| h.pair);
        let slice = served_slice_split(&snap.model, snap.handle, pair)?;
        Some((conn, snap, slice))
    }

    /// The split slice while the radio reports the transmit flag on it, and whether that split is
    /// being let go: what `s`, `i` and `x` read.
    fn split_tx(&self) -> Option<(Snapshot, u8, bool)> {
        let conn = self.conn.upgrade()?;
        split_on_air(&conn, &self.state)
    }

    /// `S 1 VFOB`: the transmit flag on a slice of ours beside the served one (module header).
    fn split_on(&self) -> Result<(), SetFault> {
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let Some(conn) = self.conn.upgrade() else {
            return Err(SetFault::Refused);
        };
        let snap = conn.snapshot();
        let Some(handle) = snap.handle else {
            return Err(refuse_split(
                "the radio gave this connection no client handle",
            ));
        };
        if let Some(hold) = split_hold(&self.state, snap.handle) {
            let pair = hold.pair;
            if is_ours(&snap, pair.rx) && is_ours(&snap, pair.tx) {
                if hold.releasing {
                    // Asked for again before it went: kept, and no longer owed a take-down.
                    set_hold(
                        &self.state,
                        Some(SplitHold {
                            releasing: false,
                            ..hold
                        }),
                    );
                }
                if snap.model.tx_slices() == [pair.tx] {
                    // Held from the last over: nothing to send.
                    return Ok(());
                }
                if !wait_quiet(&conn, deadline) {
                    return Err(refuse_split("a transmission has not ended"));
                }
                return move_flag(&conn, pair.tx, deadline);
            }
            // A pair whose receive slice has gone is not one: its split slice goes with it,
            // unless it is transmitting.
            set_hold(&self.state, None);
            if is_ours(&snap, pair.tx) && snap.model.tx_slices() != [pair.tx] {
                let _ = conn.request(Command::SliceRemove { slice: pair.tx }, left(deadline));
            }
        }
        let snap = conn.snapshot();
        // Another station's transmitter is never taken (the move's own admission refuses it too):
        // no slice is made for a split that could not happen.
        if let Some(holder) = snap
            .model
            .tx_slices()
            .into_iter()
            .find(|t| !is_ours(&snap, *t))
        {
            return Err(refuse_split(&format!(
                "the transmitter is on slice {holder}, which is not Nexus's, and Nexus does not \
                 take it away"
            )));
        }
        let Some(rx) = served_slice(&snap.model, snap.handle) else {
            return Err(refuse_split("no slice of ours"));
        };
        let Some(s) = snap.model.slices.get(&rx) else {
            return Err(refuse_split("no slice of ours"));
        };
        let Some(freq_hz) = s
            .frequency_mhz
            .filter(|mhz| mhz.is_finite() && *mhz > 0.0)
            .map(|mhz| (mhz * 1e6).round())
        else {
            return Err(refuse_split("the slice has reported no frequency"));
        };
        let create = Command::SliceCreate {
            pan: s.pan,
            freq_hz,
            mode: s.mode.as_deref().and_then(Mode::new),
        };
        if !wait_quiet(&conn, deadline) {
            return Err(refuse_split("a transmission has not ended"));
        }
        // A slice an earlier create left behind goes before another is asked for.
        reconcile_create(&conn, &self.state, Instant::now() + REQUEST_TIMEOUT);
        let snap = conn.snapshot();
        let known: Vec<u8> = snap
            .model
            .slices
            .keys()
            .copied()
            .filter(|i| is_ours(&snap, *i))
            .collect();
        // From here a slice the radio may have made without saying which is tracked, and removed
        // when it appears ([`settle_split`]).
        let lost = |fault: SetFault| {
            *lock(&self.state.split_create) = Some(PendingCreate {
                handle,
                known: known.clone(),
                at: Instant::now(),
            });
            fault
        };
        let reply = match conn.request(create, left(deadline)) {
            Ok(r) if r.code == 0 => r,
            Ok(r) => {
                return Err(refuse_split(&format!(
                    "the radio made no second slice (0x{:08X} {})",
                    r.code, r.message
                )))
            }
            Err(ConnError::Timeout) => {
                return Err(lost(refuse_split(
                    "the radio did not answer the slice create",
                )))
            }
            other => return answered(other),
        };
        // The reply names the new slice: `2`, or `2|<debug text>`.
        let Some(tx) = reply
            .message
            .split('|')
            .next()
            .and_then(|n| n.trim().parse::<u8>().ok())
        else {
            return Err(lost(refuse_split(&format!(
                "the radio's answer named no slice ({:?})",
                reply.message
            ))));
        };
        if !conn.wait_until(left(deadline), |s| is_ours(s, tx)) {
            return Err(lost(SetFault::NoAnswer));
        }
        let pair = SplitPair { rx, tx };
        // A new split slice: nothing written to it yet.
        *lock(&self.state.split_written) = None;
        set_hold(
            &self.state,
            Some(SplitHold {
                pair,
                handle,
                releasing: false,
            }),
        );
        // Its receive audio is the served slice's again; the operator hears one.
        let _ = conn.request(
            Command::SliceAudioMute {
                slice: tx,
                mute: true,
            },
            left(deadline),
        );
        let moved = move_flag(&conn, tx, deadline);
        if moved.is_err() && conn.snapshot().model.tx_slices() != [tx] {
            // No split came of it: the slice made for one goes.
            set_hold(&self.state, None);
            let _ = conn.request(Command::SliceRemove { slice: tx }, left(deadline));
        }
        moved
    }

    /// `S 0`: the transmit flag back on the receive slice and the split slice removed. Nothing to
    /// undo is success. One that cannot finish now is owed, and finished at the radio loop's next
    /// quiet point ([`SplitHold::releasing`]).
    fn split_off(&self) -> Result<(), SetFault> {
        let Some(conn) = self.conn.upgrade() else {
            return Err(SetFault::Refused);
        };
        release_split(&conn, &self.state, Instant::now() + REQUEST_TIMEOUT)
    }

    /// The split slice for `I` or `X`: held, the flag on it, and quiet within `deadline`. The
    /// write itself goes through [`Connection::request_quiet`], which checks again where a key of
    /// ours is admitted.
    fn split_target(&self, deadline: Instant) -> Result<(Arc<Connection>, Snapshot, u8), SetFault> {
        let Some(conn) = self.conn.upgrade() else {
            return Err(SetFault::Refused);
        };
        let Some((_, tx, _)) = split_on_air(&conn, &self.state) else {
            return Err(refuse_split("split is not on"));
        };
        if !wait_quiet(&conn, deadline) {
            return Err(refuse_split("a transmission has not ended"));
        }
        let snap = conn.snapshot();
        Ok((conn, snap, tx))
    }

    /// The served slice's status, for a read.
    fn read<T>(&self, f: impl FnOnce(&SliceDelta) -> Option<T>) -> Option<T> {
        let (_, snap, slice) = self.served()?;
        snap.model.slices.get(&slice).and_then(f)
    }

    /// Send `command` (built for the served slice) and say how the radio answered ([`answered`]).
    /// No served slice, or no command for it, is a refusal: nothing was sent.
    fn try_write(&self, command: impl FnOnce(u8) -> Option<Command>) -> Result<(), SetFault> {
        let Some((conn, _, slice)) = self.served() else {
            return Err(SetFault::Refused);
        };
        let Some(command) = command(slice) else {
            return Err(SetFault::Refused);
        };
        answered(conn.request(command, REQUEST_TIMEOUT))
    }

    /// Keep why a key was kept off the air, for the radio loop to put on screen
    /// ([`super::FlexDaemon::key_refused_since`]): the rigctld answer it reads is only `RPRT -1`.
    fn refuse(&self, why: String, cause: Option<FlexAudioRefusal>) {
        *self
            .state
            .refused
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some((Instant::now(), why, cause));
    }

    /// Send one CW word to the radio's keyer, timed at the radio's speed, or say why not.
    fn send_word(&self, text: &str) -> Result<(), String> {
        let Some(text) = CwxText::new(text) else {
            return Err(
                "CW not sent: the text has a character the radio's CW keyer cannot \
                        take."
                    .to_string(),
            );
        };
        if let Some(c) = text
            .as_str()
            .chars()
            .find(|c| *c != ' ' && tempo_core::cw::morse_code(*c).is_none())
        {
            return Err(format!(
                "CW not sent: Nexus has no Morse for \"{c}\", so it cannot tell how long the \
                 radio takes to send it. Take it out of the message, or use another keyer."
            ));
        }
        let conn = self
            .conn
            .upgrade()
            .ok_or("CW not sent: the connection to the radio is closed.")?;
        if let Some(why) = split_blocks_key(&conn, &self.state) {
            return Err(format!("CW not sent: {why}."));
        }
        let wpm = word_wpm(&conn.snapshot().model, &self.state);
        let word_ms = tempo_core::cw::morse_duration_ms(text.as_str(), wpm).ceil() as u64;
        match conn.start_lasting(TxStart::CwxSend(text), Some(word_ms)) {
            Ok(_) => Ok(()),
            Err(Some(refusal)) => Err(cw_refusal_words(&refusal)),
            Err(None) => Err("CW not sent: the connection to the radio is closed.".to_string()),
        }
    }

    /// Run a start through the core's admission; `false` for any refusal.
    fn start(&self, start: TxStart) -> bool {
        self.conn
            .upgrade()
            .is_some_and(|conn| conn.start(start).is_ok())
    }

    /// `u TUNER`: whether the radio's tuner is in line, answered only while this client runs the
    /// radio's ATU and the radio reports a tuner fitted; otherwise not at all, so no ATU button is
    /// offered, as before the switch. In line once the radio's last cycle matched
    /// (`TUNE_SUCCESSFUL`, `TUNE_OK`); every other status is the tuner bypassed, failed or not used
    /// yet. ⚠️ Unconfirmed until a tester's bench, as is the key that says a tuner is fitted.
    fn tuner(&self) -> Option<bool> {
        let conn = self.conn.upgrade()?;
        if !conn.switched_on(StartKind::Atu) {
            return None;
        }
        conn.read(|s| {
            let matched = matches!(
                s.model.atu.status.as_deref(),
                Some("TUNE_SUCCESSFUL" | "TUNE_OK")
            );
            s.model.atu_fitted().then_some(matched)
        })
    }

    /// `U TUNER 2`: one ATU cycle, through the core's admission. The ATU line's reason is cleared
    /// before the start goes out (the radio's refusal of it arrives later, through the watcher),
    /// and a start the client refuses keeps its own words there.
    fn start_atu(&self) -> bool {
        *self
            .state
            .atu_refused
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        // Refused, as `T 1` is, while the flag is still on a split slice being taken down.
        let started = self
            .conn
            .upgrade()
            .map(|conn| match split_blocks_key(&conn, &self.state) {
                Some(why) => Err(Some(Refusal::SplitTakeDown { why })),
                None => conn.start(TxStart::AtuStart),
            });
        let refused = match started {
            Some(Ok(_)) => return true,
            Some(Err(refusal)) => super::atu_refusal_words(refusal.as_ref()),
            None => super::atu_refusal_words(None),
        };
        tempo_core::applog::info("cat", &format!("Flex client: {refused}"));
        *self
            .state
            .atu_refused
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(refused);
        false
    }

    /// Run a stop, which is never gated. Nothing of ours to stop is success: there is nothing on
    /// the air of ours, and another client's transmission is never stopped from here.
    fn stop(&self, stop: TxStop) -> bool {
        self.conn.upgrade().is_some_and(|conn| {
            matches!(
                conn.stop(stop),
                StopOutcome::Sent { .. } | StopOutcome::NothingOfOurs
            )
        })
    }

    /// End everything of ours, never gated: what `T 0` means here. Success as for [`Self::stop`].
    fn end_ours(&self) -> bool {
        self.conn
            .upgrade()
            .is_some_and(|conn| matches!(conn.end_ours(), EndOutcome::Sent(_)))
    }
}

/// A 0..1 level as the radio's 0–100.
fn percent(value: &str) -> Option<i32> {
    let v: f64 = value.trim().parse().ok()?;
    (v.is_finite() && (0.0..=1.0).contains(&v)).then(|| (v * 100.0).round() as i32)
}

/// The radio keyer's slowest speed (`cw wpm` takes 5–100).
const SLOWEST_WPM: u32 = 5;

/// The speed a CW word is timed at: the radio's reported speed, or the speed the shim set within
/// [`COMMANDED_FOR`] when that is slower (the radio may not have reported it yet); the one the shim
/// set when the radio reports none; with neither, the keyer's slowest. Never faster than the radio
/// keys it.
fn word_wpm(model: &StatusModel, state: &ClientState) -> u32 {
    let reported = model.transmit.cw_speed.and_then(|w| u32::try_from(w).ok());
    let commanded = *state.cw_wpm.lock().unwrap_or_else(PoisonError::into_inner);
    match (reported, commanded) {
        (Some(r), Some((c, at))) if at.elapsed() < COMMANDED_FOR => r.min(c),
        (Some(r), _) => r,
        (None, Some((c, _))) => c,
        (None, None) => SLOWEST_WPM,
    }
}

/// The CW line's words for a word the client did not send: what kept it back, and what to do.
pub(crate) fn cw_refusal_words(refusal: &Refusal) -> String {
    match refusal {
        Refusal::NoReadback(_) => format!(
            "CW not sent: CW through the Flex native client is not bench-tested yet. To try it, \
             turn on {}, or turn the Flex native client off (SmartSDR CAT sends CW), or use the \
             WinKeyer or Soundcard keyer.",
            super::BETA_SWITCH
        ),
        Refusal::NotCw { mode } if mode.is_empty() => "CW not sent: the radio has not said which \
                                                        mode its transmit slice is in."
            .to_string(),
        Refusal::NotCw { mode } => format!(
            "CW not sent: the radio's transmit slice is in {mode}, not CW. Nexus sends CW to the \
             radio's keyer only in CW."
        ),
        Refusal::BreakInOff => "CW not sent: break-in is off on the radio. Turn on break-in in \
                                SmartSDR, or use another keyer."
            .to_string(),
        Refusal::SyncCwx => "CW not sent: Sync CWX is on in SmartSDR. Turn it off, or use another \
                             keyer."
            .to_string(),
        Refusal::XitOn => "CW not sent: XIT is on for the radio's transmit slice, so the radio \
                           would send off the frequency Nexus checked. Turn XIT off."
            .to_string(),
        other => format!("CW not sent: {other}."),
    }
}

fn slice_function(token: &str) -> Option<SliceFunction> {
    match token {
        "NB" => Some(SliceFunction::Nb),
        "NR" => Some(SliceFunction::Nr),
        "ANF" => Some(SliceFunction::Anf),
        _ => None,
    }
}

impl RigBackend for FlexShim {
    fn owner_transmitting(&self) -> bool {
        self.tx_intent.load(Ordering::Relaxed)
    }

    fn freq_hz(&self) -> u64 {
        // 0 is "no honest reading", the answer the CI-V and OmniRig backends give a dead link,
        // and one the radio loop never takes for a dial.
        self.read(|s| s.frequency_mhz)
            .filter(|mhz| mhz.is_finite() && *mhz > 0.0)
            .map_or(0, |mhz| (mhz * 1e6).round() as u64)
    }

    fn mode(&self) -> (String, u32) {
        // An unknown mode is an empty word, never a guessed "USB": a partial answer is not a
        // reading.
        self.read(|s| {
            let mode = rigctld_mode(s.mode.as_deref()?);
            let width = match (s.filter_low, s.filter_high) {
                (Some(low), Some(high)) if high > low => u32::try_from(high - low).unwrap_or(0),
                _ => 0,
            };
            Some((mode, width))
        })
        .unwrap_or_default()
    }

    fn ptt(&self) -> bool {
        // What the radio's interlock says, whoever keyed it: Nexus must see another client's
        // transmission too. NOT the session's own "keyed", which stays set until the readback has
        // proved an unkey (a few hundred milliseconds after `READY`); the radio loop reads `t`
        // only while Nexus is idle, to spot SOMEONE ELSE keying, and would take that tail of our
        // own over for exactly that. With no whole interlock sample, our own unconfirmed start
        // is the only thing known.
        self.conn.upgrade().is_some_and(|conn| {
            let snap = conn.snapshot();
            match &snap.model.interlock.sample {
                Some(s) => matches!(
                    s.state.as_str(),
                    "PTT_REQUESTED" | "TRANSMITTING" | "UNKEY_REQUESTED"
                ),
                None => snap.keyed,
            }
        })
    }

    fn vfo(&self) -> String {
        "VFOA".to_string()
    }

    fn split(&self) -> bool {
        self.split_tx().is_some()
    }

    /// `VFOB` for a split Nexus holds; `TX` while that split is being let go
    /// ([`SplitHold::releasing`]): the split is on, but it is Nexus's own leftover, never the
    /// operator's to put back. `S 1 TX` is refused, so a restore of it changes nothing.
    fn split_vfo(&self) -> String {
        match self.split_tx() {
            Some((_, _, true)) => "TX",
            Some(_) => "VFOB",
            None => "VFOA",
        }
        .to_string()
    }

    /// Where the radio transmits: the split slice while split is on, else the served dial.
    fn split_freq_hz(&self) -> Option<u64> {
        match self.split_tx() {
            Some((snap, tx, _)) => snap
                .model
                .slices
                .get(&tx)
                .and_then(|s| s.frequency_mhz)
                .filter(|mhz| mhz.is_finite() && *mhz > 0.0)
                .map(|mhz| (mhz * 1e6).round() as u64),
            None => Some(self.freq_hz()),
        }
    }

    fn split_mode(&self) -> Option<(String, u32)> {
        match self.split_tx() {
            Some((snap, tx, _)) => {
                let s = snap.model.slices.get(&tx)?;
                let width = match (s.filter_low, s.filter_high) {
                    (Some(low), Some(high)) if high > low => u32::try_from(high - low).unwrap_or(0),
                    _ => 0,
                };
                Some((rigctld_mode(s.mode.as_deref()?), width))
            }
            None => Some(self.mode()),
        }
    }

    fn set_split(&self, on: bool, tx_vfo: &str) -> Option<bool> {
        self.try_set_split(on, tx_vfo).map(|r| r.is_ok())
    }

    /// `S`: on only with the transmit VFO B (the split slice); off from any.
    fn try_set_split(&self, on: bool, tx_vfo: &str) -> Option<Result<(), SetFault>> {
        Some(match (on, tx_vfo) {
            (true, "VFOB" | "Sub" | "SUB") => self.split_on(),
            (true, other) => Err(refuse_split(&format!("transmit VFO {other}"))),
            (false, _) => self.split_off(),
        })
    }

    fn set_split_freq(&self, hz: u64) -> Option<bool> {
        self.try_set_split_freq(hz).map(|r| r.is_ok())
    }

    /// `I`: tune the split slice, never while anything transmits. The frequency the slice is
    /// already on, by the radio's report and by the last `I` it took, is answered at once with
    /// nothing sent ([`SplitWritten`]).
    fn try_set_split_freq(&self, hz: u64) -> Option<Result<(), SetFault>> {
        if let Some(conn) = self.conn.upgrade() {
            if let Some((snap, slice, written)) = split_unchanged(&conn, &self.state) {
                let reported = snap
                    .model
                    .slices
                    .get(&slice)
                    .and_then(|s| s.frequency_mhz)
                    .map(|mhz| (mhz * 1e6).round());
                if written.freq_hz == Some(hz) && reported == Some(hz as f64) {
                    return Some(Ok(()));
                }
            }
        }
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        Some(self.split_target(deadline).and_then(|(conn, snap, slice)| {
            note_written(&self.state, &snap, slice, |w| w.freq_hz = None);
            answered(conn.request_quiet(
                Command::SliceTune {
                    slice,
                    freq_hz: hz as f64,
                    keep_pan: true,
                },
                left(deadline),
            ))?;
            note_written(&self.state, &snap, slice, |w| w.freq_hz = Some(hz));
            Ok(())
        }))
    }

    fn set_split_mode(&self, mode: &str, passband_hz: i32) -> Option<bool> {
        self.try_set_split_mode(mode, passband_hz)
            .map(|r| r.is_ok())
    }

    /// `X`: the split slice's mode, and a width as `M` places one (zero or less keeps the
    /// filter), never while anything transmits.
    /// The mode and width the slice already has, by the radio's report and by the last `X` it
    /// took, are answered at once with nothing sent ([`SplitWritten`]).
    fn try_set_split_mode(&self, mode: &str, passband_hz: i32) -> Option<Result<(), SetFault>> {
        if let Some(conn) = self.conn.upgrade() {
            if let Some((snap, slice, written)) = split_unchanged(&conn, &self.state) {
                let unchanged = snap.model.slices.get(&slice).is_some_and(|s| {
                    flex_mode_for(mode, s.mode_list.as_deref()).is_some_and(|word| {
                        let filter_kept = match u32::try_from(passband_hz) {
                            Ok(width) if width > 0 => {
                                let current = s.filter_low.zip(s.filter_high);
                                current.is_some()
                                    && filter_for(&word, width, s.mode.as_deref(), current)
                                        == current
                            }
                            _ => true,
                        };
                        s.mode.as_deref() == Some(word.as_str())
                            && filter_kept
                            && written.mode == Some((word, passband_hz))
                    })
                });
                if unchanged {
                    return Some(Ok(()));
                }
            }
        }
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        Some(self.split_target(deadline).and_then(|(conn, snap, slice)| {
            note_written(&self.state, &snap, slice, |w| w.mode = None);
            let Some(s) = snap.model.slices.get(&slice) else {
                return Err(SetFault::Refused);
            };
            let Some(word) = flex_mode_for(mode, s.mode_list.as_deref()) else {
                return Err(SetFault::Refused);
            };
            let filter = match u32::try_from(passband_hz) {
                Ok(width) if width > 0 => Some(
                    filter_for(
                        &word,
                        width,
                        s.mode.as_deref(),
                        s.filter_low.zip(s.filter_high),
                    )
                    .ok_or(SetFault::Refused)?,
                ),
                _ => None,
            };
            let mode = Mode::new(&word).ok_or(SetFault::Refused)?;
            let asked = (word.clone(), passband_hz);
            answered(conn.request_quiet(Command::SliceMode { slice, mode }, left(deadline)))?;
            // The over goes out in this mode from now, whether or not the radio has said so yet.
            *self
                .state
                .commanded
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some((slice, word, Instant::now()));
            if let Some((low_hz, high_hz)) = filter {
                answered(conn.request_quiet(
                    Command::SliceFilter {
                        slice,
                        low_hz,
                        high_hz,
                    },
                    left(deadline),
                ))?;
            }
            note_written(&self.state, &snap, slice, |w| w.mode = Some(asked));
            Ok(())
        }))
    }

    fn set_freq(&self, hz: u64) -> bool {
        self.try_set_freq(hz).is_ok()
    }

    /// The dial write: a tune the radio does not answer is silence, not a refusal ([`answered`]).
    /// A dial on another band than a split slice of ours takes the split down first, so the
    /// transmitter never stays on the old band; refused, with the dial left, if that cannot be
    /// done now (it stays owed: [`SplitHold::releasing`]).
    fn try_set_freq(&self, hz: u64) -> Result<(), SetFault> {
        if let Some(conn) = self.conn.upgrade() {
            let snap = conn.snapshot();
            if let Some(hold) = split_hold(&self.state, snap.handle) {
                let band = |mhz: f64| tempo_app::bandplan::band_for_dial(mhz);
                let split_band = snap
                    .model
                    .slices
                    .get(&hold.pair.tx)
                    .and_then(|s| s.frequency_mhz)
                    .and_then(band);
                let dial_band = band(hz as f64 / 1e6);
                if split_band.is_none() || split_band != dial_band {
                    release_split(&conn, &self.state, Instant::now() + REQUEST_TIMEOUT)
                        .inspect_err(|_| {
                            tempo_core::applog::info(
                                "cat",
                                "Flex client: the dial was not moved to another band while \
                                 Nexus's split slice still has the transmitter",
                            );
                        })?;
                }
            }
        }
        self.try_write(|slice| {
            Some(Command::SliceTune {
                slice,
                freq_hz: hz as f64,
                keep_pan: false,
            })
        })
    }

    fn set_mode(&self, mode: &str, passband_hz: u32) -> bool {
        self.try_set_mode(mode, passband_hz).is_ok()
    }

    /// The mode write, with its width: a mode or a filter the radio does not answer is silence,
    /// not a refusal ([`answered`]).
    fn try_set_mode(&self, mode: &str, passband_hz: u32) -> Result<(), SetFault> {
        let Some((conn, snap, slice)) = self.served() else {
            return Err(SetFault::Refused);
        };
        let Some(s) = snap.model.slices.get(&slice) else {
            return Err(SetFault::Refused);
        };
        let Some(word) = flex_mode_for(mode, s.mode_list.as_deref()) else {
            return Err(SetFault::Refused);
        };
        // Decide the whole change before sending any of it: a width that cannot be placed refuses
        // the verb without moving the mode.
        let filter = match passband_hz {
            0 => None,
            width => match filter_for(
                &word,
                width,
                s.mode.as_deref(),
                s.filter_low.zip(s.filter_high),
            ) {
                Some(edges) => Some(edges),
                None => return Err(SetFault::Refused),
            },
        };
        let Some(mode) = Mode::new(&word) else {
            return Err(SetFault::Refused);
        };
        answered(conn.request(Command::SliceMode { slice, mode }, REQUEST_TIMEOUT))?;
        // The over goes out in this mode from now, whether or not the radio has said so yet.
        *self
            .state
            .commanded
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some((slice, word, Instant::now()));
        match filter {
            None => Ok(()),
            Some((low_hz, high_hz)) => answered(conn.request(
                Command::SliceFilter {
                    slice,
                    low_hz,
                    high_hz,
                },
                REQUEST_TIMEOUT,
            )),
        }
    }

    fn set_ptt(&self, on: bool) -> bool {
        if on {
            if let Some(why) = self
                .conn
                .upgrade()
                .and_then(|conn| split_blocks_key(&conn, &self.state))
            {
                let why = format!("not keying: {why}");
                tempo_core::applog::warn("cat", &format!("Flex client: {why}"));
                self.refuse(why, None);
                return false;
            }
            if let Some((why, cause)) = self.audio_refuses_key() {
                tempo_core::applog::warn("cat", &format!("Flex client: {why}"));
                self.refuse(why, Some(cause));
                return false;
            }
            self.start(TxStart::Key)
        } else {
            self.end_ours()
        }
    }

    fn set_vfo(&self, vfo: &str) -> bool {
        // The served slice is the only VFO: A, by name or as the current one.
        matches!(vfo, "VFOA" | "currVFO")
    }

    /// UNKNOWN: the client declares no receive range, so `\dump_state` carries an empty RX list,
    /// which Nexus's reader takes as unknown: every caller fails open, and the radio is tuned and
    /// answers for itself. Left at the trait's `None`, the reply carried the CAT broker's wide row
    /// for WSJT-X, and the app took 135.7 kHz to 1.3 GHz as this radio's range.
    fn rx_ranges(&self) -> Option<Vec<(u64, u64)>> {
        Some(Vec::new())
    }

    fn level(&self, name: &str) -> Option<String> {
        match name {
            "RFPOWER" => {
                let conn = self.conn.upgrade()?;
                let power = conn.snapshot().model.transmit.rf_power?;
                Some(format!("{:.6}", f64::from(power.clamp(0, 100)) / 100.0))
            }
            "AF" => self
                .read(|s| s.audio_level)
                .map(|level| format!("{:.6}", (level / 100.0).clamp(0.0, 1.0))),
            // The keyer's speed as the radio reports it, whoever set it.
            "KEYSPD" => {
                let conn = self.conn.upgrade()?;
                let wpm = conn.snapshot().model.transmit.cw_speed?;
                Some(wpm.to_string())
            }
            _ => None,
        }
    }

    fn set_level(&self, name: &str, value: &str) -> Option<bool> {
        self.try_set_level(name, value).map(|r| r.is_ok())
    }

    /// The level writes: a level the radio does not answer is silence, not a refusal
    /// ([`answered`]). A value the command cannot carry is refused before anything is sent.
    fn try_set_level(&self, name: &str, value: &str) -> Option<Result<(), SetFault>> {
        let request = |command| match self.conn.upgrade() {
            Some(conn) => answered(conn.request(command, REQUEST_TIMEOUT)),
            None => Err(SetFault::Refused),
        };
        match name {
            "RFPOWER" => Some(percent(value).map_or(Err(SetFault::Refused), |level| {
                request(Command::TransmitRfPower { level })
            })),
            "AF" => Some(percent(value).map_or(Err(SetFault::Refused), |level| {
                self.try_write(|slice| Some(Command::SliceAudioLevel { slice, level }))
            })),
            // The keyer's speed, which CWX keys at too: the one CW setting Nexus owns.
            "KEYSPD" => Some(
                value
                    .trim()
                    .parse::<u32>()
                    .map_or(Err(SetFault::Refused), |wpm| {
                        let took = request(Command::CwSpeed { wpm });
                        if took.is_ok() {
                            *self
                                .state
                                .cw_wpm
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner) =
                                Some((wpm, Instant::now()));
                        }
                        took
                    }),
            ),
            _ => None,
        }
    }

    fn func(&self, token: &str) -> Option<bool> {
        if token == "TUNER" {
            return self.tuner();
        }
        let function = slice_function(token)?;
        self.read(|s| match function {
            SliceFunction::Nb => s.nb,
            SliceFunction::Nr => s.nr,
            SliceFunction::Anf => s.anf,
        })
    }

    fn set_func(&self, token: &str, on: bool) -> Option<bool> {
        self.try_set_func(token, on).map(|r| r.is_ok())
    }

    /// The noise switches, told apart as the levels are ([`answered`]). The tuner's start goes
    /// through the core's admission, which answers yes or no.
    fn try_set_func(&self, token: &str, on: bool) -> Option<Result<(), SetFault>> {
        if token == "TUNER" {
            // The tuner function starts a tune cycle (`U TUNER 2`). Its off half has no sender
            // in Nexus and no meaning here: not implemented, as before.
            return on.then(|| {
                if self.start_atu() {
                    Ok(())
                } else {
                    Err(SetFault::Refused)
                }
            });
        }
        let function = slice_function(token)?;
        Some(self.try_write(|slice| {
            Some(Command::SliceDsp {
                slice,
                function,
                on,
            })
        }))
    }

    fn send_morse(&self, text: &str) -> Option<bool> {
        Some(match self.send_word(text) {
            Ok(()) => true,
            Err(why) => {
                tempo_core::applog::info("cat", &format!("Flex client: {why}"));
                self.refuse(why, None);
                false
            }
        })
    }

    fn stop_morse(&self) -> Option<bool> {
        Some(self.stop(TxStop::CwxClear))
    }

    fn caps(&self) -> Option<RigCaps> {
        Some(authored_caps())
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn the_data_modes_have_flex_names_and_the_rest_keep_theirs() {
        assert_eq!(flex_mode_for("PKTUSB", None).as_deref(), Some("DIGU"));
        assert_eq!(flex_mode_for("pktlsb", None).as_deref(), Some("DIGL"));
        assert_eq!(flex_mode_for("USB", None).as_deref(), Some("USB"));
        assert_eq!(flex_mode_for("CW", None).as_deref(), Some("CW"));
        assert_eq!(rigctld_mode("DIGU"), "PKTUSB");
        assert_eq!(rigctld_mode("DIGL"), "PKTLSB");
        assert_eq!(rigctld_mode("NFM"), "NFM");
    }

    /// A mode the radio does not offer is refused, never approximated.
    #[test]
    fn a_mode_the_radio_does_not_offer_is_refused() {
        assert_eq!(flex_mode_for("CWR", None), None);
        assert_eq!(flex_mode_for("PKTFM", None), None);
        assert_eq!(flex_mode_for("", None), None);
        let offered = vec!["USB".to_string(), "LSB".to_string()];
        assert_eq!(flex_mode_for("USB", Some(&offered)).as_deref(), Some("USB"));
        assert_eq!(
            flex_mode_for("PKTUSB", Some(&offered)),
            None,
            "this radio's list has no DIGU"
        );
    }

    /// A width keeps the operator's own edge on the side the slice already sits on, and starts
    /// from the slice's frequency on a new side.
    #[test]
    fn a_width_keeps_the_edge_the_slice_already_has_on_its_side() {
        // USB 100..2900 to DIGU 3000: the low edge stays.
        assert_eq!(
            filter_for("DIGU", 3000, Some("USB"), Some((100, 2900))),
            Some((100, 3100))
        );
        // LSB to DIGU: a new side starts at the slice's frequency.
        assert_eq!(
            filter_for("DIGU", 3000, Some("LSB"), Some((-2900, -100))),
            Some((0, 3000))
        );
        // The lower side keeps its high edge.
        assert_eq!(
            filter_for("DIGL", 3000, Some("LSB"), Some((-2900, -100))),
            Some((-3100, -100))
        );
        // AM is centred: a 6 kHz width around the slice's own centre.
        assert_eq!(
            filter_for("AM", 6000, Some("USB"), Some((100, 2900))),
            Some((-3000, 3000))
        );
        assert_eq!(
            filter_for("CW", 500, Some("CW"), Some((400, 1000))),
            Some((450, 950))
        );
        // Nothing to place a width on, or not a width.
        assert_eq!(filter_for("RTTY", 500, None, None), None);
        assert_eq!(filter_for("USB", 0, None, None), None);
        assert_eq!(filter_for("USB", 200_000, None, None), None);
    }

    #[test]
    fn levels_are_fractions_of_full_scale() {
        assert_eq!(percent("0.5"), Some(50));
        assert_eq!(percent("1"), Some(100));
        assert_eq!(percent("0"), Some(0));
        assert_eq!(percent("1.5"), None);
        assert_eq!(percent("-0.1"), None);
        assert_eq!(percent("NaN"), None);
        assert_eq!(percent("x"), None);
    }
}
