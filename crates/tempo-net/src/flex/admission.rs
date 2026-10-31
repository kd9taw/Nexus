//! Admission: the Flex-side checks a transmit start must pass. They can only refuse.
//!
//! This is not Nexus's transmit gate. The engine decides whether a transmission may happen at
//! all (the TX-enable latch, licence privileges on the emitted RF, identity, the watchdog, the
//! one transmitter arbiter), and nothing in the app calls this module yet; the change that wires
//! the Flex client into the engine puts those gates in front of it. Admission adds what only the
//! radio's own status can say, and every check is a reason to refuse:
//!
//! - the session is registered, on a TCP API version reviewed for transmit (1.4), and has a
//!   client handle;
//! - nothing of ours is keyed already;
//! - exactly one slice is the transmit slice, and it is ours;
//! - what the start's kind needs of the radio's settings ([`kind_checks`]): a CWX word only on a
//!   transmit slice in CW, with the radio's break-in on and its Sync CWX not on, so that the
//!   radio's own keyer keys and unkeys it; an ATU cycle only while the radio reports a tuner
//!   fitted; a tune, an ATU cycle or a CWX word never while the transmit slice's XIT is on, which
//!   would put the radio's own carrier off the dial the engine judged. Nexus reads these settings
//!   and never writes them;
//! - the operator's own key in the radio's key jack is not keying the radio, as the interlock
//!   reports it ([`super::ptt_evidence::OPERATOR_KEY`], named once a tester's bench has shown it):
//!   the radio is the operator's then, so nothing of ours starts over it;
//! - the interlock's last whole sample is `READY`, transmit allowed, no reason, no keying source
//!   (no mic, ACC or RCA PTT), and names no transmitting client, or names us;
//! - the unkey readback is idle: it has seen the radio idle on this connection, so the end of the
//!   transmission can be proven ([`super::ptt_evidence`]);
//! - and the start is of a kind whose readback a tester's bench has confirmed ([`BENCHED`]). Today
//!   that is `xmit` only. Tune, ATU and CWX have readback profiles built from AetherSDR's notes,
//!   but their starts are refused, before any other check, until the bench confirms them, unless
//!   the operator has turned on, for the radio in use, the Beta setting that lets Nexus start them
//!   before a bench has ([`BETA`], [`admit_beta`]; off by default). That setting passes this one
//!   check: every check above still decides each start, in the same order.
//!
//! **A CWX word while our own CWX operation is open** joins it: the two checks that describe that
//! operation (nothing of ours keyed, the readback idle) do not apply, nor does the interlock's
//! `READY` while the radio keys our CWX under the CWX source. Every other check does: the transmit
//! slice ours and alone, no other transmitting client, no hardware source, transmit allowed, no
//! reason.
//!
//! [`Admitted`] is the value a start must carry to be rendered ([`super::encode::render_start`]).
//! Its field is private to this module and [`admit`] is the only constructor (with
//! [`admit_beta`], the same checks under the operator's Beta setting), so a keying command cannot
//! be produced anywhere else. It is neither `Clone` nor `Copy`: one admission renders one start.
//!
//! **The transmitter's audio source** (`stream create type=dax_tx`, `transmit set dax`) has its
//! own admission, [`admit_tx_audio`], and its own value, [`AdmittedTxAudio`]: it keys nothing,
//! but `transmit set dax` is radio-wide and decides what an over carries. It is refused while
//! anything of ours is keyed or the interlock is not idle (no transmitting client, no keying
//! source, a receive-side state), and whenever another program feeds the radio's DAX transmit
//! audio ([`another_dax_feeder`]; operator ruling, 2026-10-03: "when SmartSDR's DAX is also
//! connected, never write the flag"). The DAX source is also refused unless the transmit slice is
//! ours. When to write it is the caller's: the ruling keeps it out of the keying path.
//!
//! **Which slice transmits** (`slice set <n> tx=1`, the slice pair a split is made of) has its own
//! admission too, [`admit_tx_slice`], and its own value, [`AdmittedTxSlice`]: it keys nothing, but
//! it moves every later transmission to another frequency, so it is refused while anything of
//! ours is keyed or the interlock is not idle, by the same checks as the audio source, and unless
//! the slice it names is ours.
//!
//! Nexus's own design, not a port.

use std::fmt;

use super::encode::{StartKind, TxAudio, TxSlice, TxStart};
use super::model::{owner_of, Owner, StatusModel};
use super::ptt_evidence;
use super::status::SliceDelta;

/// The kinds whose readback a tester's bench has confirmed on a real radio. Only these are
/// admitted; any other start is refused with [`Refusal::NoReadback`] before every other check. A
/// kind joins this list only after its readback profile ([`super::ptt_evidence`]) has passed the
/// bench, never on the strength of the notes it was built from.
pub const BENCHED: &[StartKind] = &[StartKind::Key];

/// The kinds the operator's Beta setting admits beside [`BENCHED`] on the radio it is on for
/// (`super::session::Config::beta`; the radio profile's `flex_native_unbenched` in tempo-app, off
/// by default): tune, ATU and CWX, whose readback profiles are built from AetherSDR's notes and
/// that no bench has confirmed yet. With the setting on they pass the [`Refusal::NoReadback`]
/// check and every other check still decides them, in the same order ([`admit_beta`]); with it
/// off they are refused as before. A kind moves from here to [`BENCHED`] once a bench confirms it.
pub const BETA: &[StartKind] = &[StartKind::Tune, StartKind::Atu, StartKind::Cwx];

/// Whether admission takes starts of `kind` at all, with the operator's Beta setting at `beta`: a
/// kind a bench has confirmed ([`BENCHED`]), or, with the setting on, one of [`BETA`]. A kind it
/// does not take is refused with [`Refusal::NoReadback`] before every other check.
pub fn switched_on(kind: StartKind, beta: bool) -> bool {
    BENCHED.contains(&kind) || (beta && BETA.contains(&kind))
}

// The door past [`BENCHED`] (`session::Config::unbenched`) is test support: this crate's tests,
// and tempo-audio's through the `flex-unbenched` feature. A release build never carries it.
#[cfg(all(feature = "flex-unbenched", not(debug_assertions)))]
compile_error!(
    "`flex-unbenched` admits the Flex starts no bench has confirmed: test support, never in a \
     release build"
);

/// A start that passed admission. Constructed only by [`admit`].
#[derive(Debug)]
pub struct Admitted(TxStart);

impl Admitted {
    /// The start, for the encoder. Consumes the admission.
    pub(super) fn into_start(self) -> TxStart {
        self.0
    }

    /// Mint an admission without the checks, for unit tests of the encoder only. Not compiled
    /// outside this crate's tests.
    #[cfg(test)]
    pub(super) fn for_test(start: TxStart) -> Admitted {
        Admitted(start)
    }
}

/// A change to the transmitter's audio source that passed admission. Constructed only by
/// [`admit_tx_audio`]; neither `Clone` nor `Copy`.
#[derive(Debug)]
pub struct AdmittedTxAudio(TxAudio);

impl AdmittedTxAudio {
    /// The change, for the encoder. Consumes the admission.
    pub(super) fn into_tx_audio(self) -> TxAudio {
        self.0
    }

    /// Mint an admission without the checks, for unit tests of the encoder only.
    #[cfg(test)]
    pub(super) fn for_test(audio: TxAudio) -> AdmittedTxAudio {
        AdmittedTxAudio(audio)
    }
}

/// A move of the transmit flag that passed admission. Constructed only by [`admit_tx_slice`];
/// neither `Clone` nor `Copy`.
#[derive(Debug)]
pub struct AdmittedTxSlice(TxSlice);

impl AdmittedTxSlice {
    /// The move, for the encoder. Consumes the admission.
    pub(super) fn into_tx_slice(self) -> TxSlice {
        self.0
    }

    /// Mint an admission without the checks, for unit tests of the encoder only.
    #[cfg(test)]
    pub(super) fn for_test(tx: TxSlice) -> AdmittedTxSlice {
        AdmittedTxSlice(tx)
    }
}

/// What the session knows about itself, for admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    /// Registered as a GUI client and subscribed; not closing.
    pub ready: bool,
    /// The prologue named a TCP API version reviewed for transmit.
    pub transmit_protocol: bool,
    /// This connection's client handle.
    pub handle: Option<u32>,
    /// A start of ours is unconfirmed: keyed until the readback proves otherwise.
    pub keyed: bool,
    /// The kind of our unconfirmed start, if any.
    pub open: Option<StartKind>,
    /// The readback has seen the radio idle and is ready to track a key.
    pub readback_idle: bool,
    /// Our own DAX transmit stream, from the reply to our create.
    pub dax_tx_stream: Option<u32>,
    /// The operator's own key as the radio reports it, when one is named
    /// ([`super::ptt_evidence::OPERATOR_KEY`]).
    pub operator_key: Option<ptt_evidence::OperatorKey>,
}

/// Why a start was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A kind whose readback no tester's bench has confirmed yet ([`BENCHED`]): tune, ATU, CWX,
    /// while the operator's Beta setting for the radio is off ([`BETA`]). Also a CWX word sent
    /// with no keying time, which its readback needs.
    NoReadback(StartKind),
    NotReady,
    /// A TCP API version not reviewed for transmit: receive only.
    ProtocolUnsupported,
    NoHandle,
    AlreadyKeyed,
    NoTxSlice,
    /// The radio reports more than one transmit slice.
    SeveralTxSlices(Vec<u8>),
    /// The transmit slice belongs to another client, or its owner is not known.
    TxSliceNotOurs {
        slice: u8,
        owner: Owner,
    },
    /// No whole interlock sample since the last partial line.
    InterlockUnknown,
    /// A mic, ACC or RCA PTT is keying the radio.
    HardwarePtt {
        source: String,
    },
    /// Something else is keying the radio.
    SourceActive {
        source: String,
    },
    /// Another client holds the transmitter.
    TransmitterHeld {
        by: u32,
    },
    /// The operator's own paddle or key is keying the radio, or has taken over our CW and our part
    /// has not yet ended ([`super::ptt_evidence::OPERATOR_KEY`]).
    OperatorKeying,
    /// The radio does not allow transmit here (`tx_allowed=0`, or a reason such as
    /// `OUT_OF_BAND`).
    TransmitNotAllowed {
        reason: String,
    },
    /// The interlock is not `READY`.
    InterlockNotReady {
        state: String,
    },
    /// A CWX word while the transmit slice is not in CW: its mode as reported, empty when it has
    /// reported none. The CW ID after an FT 73 is queued with the slice in DIGU, so it stays unsent.
    NotCw {
        mode: String,
    },
    /// A CWX word while the radio does not report its break-in on: nothing would key and unkey
    /// the radio around the word.
    BreakInOff,
    /// A CWX word while the radio reports Sync CWX on: it would hold transmit after the text until
    /// told (AetherSDR's notes), which no bench has measured.
    SyncCwx,
    /// A tune, an ATU cycle or a CWX word while the transmit slice reports XIT on: the radio's own
    /// carrier would sit off the dial the engine judged, which knows nothing of the slice's XIT.
    XitOn,
    /// An ATU cycle while the radio reports no antenna tuner fitted
    /// ([`StatusModel::atu_fitted`]).
    NoAtu,
    /// The readback has not seen the radio idle on this connection.
    ReadbackNotIdle,
    /// Another program feeds the radio's DAX transmit audio (SmartSDR's DAX, typically): Nexus
    /// neither writes the DAX source nor opens a transmit stream beside it.
    AnotherDaxFeeder {
        stream: u32,
        owner: Owner,
    },
    /// The client's own Rig split is still being taken down: the transmit flag is on the split
    /// slice turning split off has not removed yet, so a start would go out at that slice's old
    /// frequency. Refused by the client above this module, which holds the split; its words.
    SplitTakeDown {
        why: String,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NoReadback(kind) => {
                write!(f, "no unkey readback is established for {kind:?} yet")
            }
            Refusal::NotReady => f.write_str("not registered with the radio"),
            Refusal::ProtocolUnsupported => {
                f.write_str("the radio's API version is not reviewed for transmit")
            }
            Refusal::NoHandle => f.write_str("the radio gave this connection no client handle"),
            Refusal::AlreadyKeyed => f.write_str("a transmission of ours is still unconfirmed"),
            Refusal::NoTxSlice => f.write_str("the radio has no transmit slice"),
            Refusal::SeveralTxSlices(s) => write!(f, "the radio reports transmit slices {s:?}"),
            Refusal::TxSliceNotOurs { slice, owner } => {
                write!(f, "transmit slice {slice} is not ours ({owner:?})")
            }
            Refusal::InterlockUnknown => f.write_str("the interlock state is not known"),
            Refusal::HardwarePtt { source } => write!(f, "the radio is keyed by {source} PTT"),
            Refusal::SourceActive { source } => write!(f, "the radio is keyed ({source})"),
            Refusal::TransmitterHeld { by } => {
                write!(f, "another client (0x{by:08X}) holds the transmitter")
            }
            Refusal::OperatorKeying => f.write_str("your key or paddle is keying the radio"),
            Refusal::TransmitNotAllowed { reason } => {
                write!(f, "the radio does not allow transmit ({reason})")
            }
            Refusal::InterlockNotReady { state } => write!(f, "the interlock is {state}"),
            Refusal::NotCw { mode } if mode.is_empty() => {
                f.write_str("the transmit slice has not reported its mode")
            }
            Refusal::NotCw { mode } => write!(f, "the transmit slice is in {mode}, not CW"),
            Refusal::BreakInOff => f.write_str("the radio's break-in is off"),
            Refusal::SyncCwx => f.write_str("the radio's Sync CWX is on"),
            Refusal::XitOn => f.write_str("XIT is on for the transmit slice"),
            Refusal::NoAtu => f.write_str("the radio reports no antenna tuner"),
            Refusal::ReadbackNotIdle => {
                f.write_str("the radio has not been seen idle on this connection")
            }
            Refusal::AnotherDaxFeeder { stream, owner } => write!(
                f,
                "another program feeds the radio's DAX transmit audio (stream 0x{stream:08X}, \
                 {owner:?})"
            ),
            Refusal::SplitTakeDown { why } => f.write_str(why),
        }
    }
}

impl std::error::Error for Refusal {}

/// Run the Flex-side checks for `start`. Pure: no state changes, no I/O, and the only result
/// other than a refusal is the one value that can be rendered as a keying command.
pub fn admit(model: &StatusModel, facts: &Facts, start: TxStart) -> Result<Admitted, Refusal> {
    admit_kinds(model, facts, start, BENCHED)
}

/// [`admit`] on a radio whose operator has turned its Beta setting on ([`BETA`]): a tune, an ATU
/// cycle and a CWX word pass the [`Refusal::NoReadback`] check as a key does, and every other
/// check decides them, in the same order. With the setting off, [`admit`] runs, unchanged.
pub fn admit_beta(model: &StatusModel, facts: &Facts, start: TxStart) -> Result<Admitted, Refusal> {
    let kinds = if BETA.contains(&start.kind()) {
        BETA
    } else {
        BENCHED
    };
    admit_kinds(model, facts, start, kinds)
}

/// [`admit`] for every kind, also those whose readback no bench has confirmed: the tests' way to
/// run the readback per kind. Not compiled outside tests ([`BENCHED`]).
#[cfg(any(test, feature = "flex-unbenched"))]
pub(super) fn admit_unbenched(
    model: &StatusModel,
    facts: &Facts,
    start: TxStart,
) -> Result<Admitted, Refusal> {
    let all = [
        StartKind::Key,
        StartKind::Tune,
        StartKind::Atu,
        StartKind::Cwx,
    ];
    admit_kinds(model, facts, start, &all)
}

fn admit_kinds(
    model: &StatusModel,
    facts: &Facts,
    start: TxStart,
    kinds: &[StartKind],
) -> Result<Admitted, Refusal> {
    let kind = start.kind();
    if !kinds.contains(&kind) {
        return Err(Refusal::NoReadback(kind));
    }
    if !facts.ready {
        return Err(Refusal::NotReady);
    }
    if !facts.transmit_protocol {
        return Err(Refusal::ProtocolUnsupported);
    }
    let ours = facts.handle.ok_or(Refusal::NoHandle)?;
    // The operator's own key keying the radio: theirs, so nothing of ours starts over it.
    if let (Some(key), Some(sample)) = (facts.operator_key, model.interlock.sample.as_ref()) {
        if matches!(sample.state.as_str(), "PTT_REQUESTED" | "TRANSMITTING")
            && key.keys(&sample.source, sample.tx_client_handle, ours)
        {
            return Err(Refusal::OperatorKeying);
        }
    }
    // A CWX word while our own CWX operation is open joins it (see the module header).
    let append = kind == StartKind::Cwx && facts.open == Some(StartKind::Cwx);
    if facts.keyed && !append {
        return Err(Refusal::AlreadyKeyed);
    }
    let tx = match model.tx_slices().as_slice() {
        [] => return Err(Refusal::NoTxSlice),
        [slice] => {
            let tx = model.slices.get(slice);
            let owner = owner_of(tx.and_then(|s| s.client_handle), Some(ours));
            if owner != Owner::Ours {
                return Err(Refusal::TxSliceNotOurs {
                    slice: *slice,
                    owner,
                });
            }
            tx
        }
        several => return Err(Refusal::SeveralTxSlices(several.to_vec())),
    };
    kind_checks(model, tx, kind)?;
    let sample = model
        .interlock
        .sample
        .as_ref()
        .ok_or(Refusal::InterlockUnknown)?;
    if sample.tx_client_handle != 0 && sample.tx_client_handle != ours {
        return Err(Refusal::TransmitterHeld {
            by: sample.tx_client_handle,
        });
    }
    match sample.source.as_str() {
        "" => {}
        "MIC" | "ACC" | "RCA" => {
            return Err(Refusal::HardwarePtt {
                source: sample.source.clone(),
            })
        }
        // Our own CWX keying the radio.
        cwx if append && cwx == ptt_evidence::CWX.source => {}
        other => {
            return Err(Refusal::SourceActive {
                source: other.to_string(),
            })
        }
    }
    if !sample.tx_allowed || !sample.reason.is_empty() {
        return Err(Refusal::TransmitNotAllowed {
            reason: sample.reason.clone(),
        });
    }
    if sample.state != "READY" && !append {
        return Err(Refusal::InterlockNotReady {
            state: sample.state.clone(),
        });
    }
    if !facts.readback_idle && !append {
        return Err(Refusal::ReadbackNotIdle);
    }
    Ok(Admitted(start))
}

/// What only some kinds need of the radio's settings, refuse-only. A CWX word keys the radio's own
/// keyer, which keys and unkeys the radio with its own break-in: the transmit slice must be in CW,
/// break-in on, Sync CWX not on. An ATU cycle needs the radio's tuner. A tune, an ATU cycle and a
/// CWX word are carriers the radio makes itself where its transmit slice says, so the slice's XIT
/// must be off. A key is none of these.
fn kind_checks(
    model: &StatusModel,
    tx: Option<&SliceDelta>,
    kind: StartKind,
) -> Result<(), Refusal> {
    if kind == StartKind::Cwx {
        let mode = tx.and_then(|s| s.mode.clone()).unwrap_or_default();
        if !mode.eq_ignore_ascii_case("CW") {
            return Err(Refusal::NotCw { mode });
        }
        if model.transmit.cw_break_in != Some(true) {
            return Err(Refusal::BreakInOff);
        }
        if model.transmit.sync_cwx == Some(true) {
            return Err(Refusal::SyncCwx);
        }
    }
    if kind == StartKind::Atu && !model.atu_fitted() {
        return Err(Refusal::NoAtu);
    }
    if kind != StartKind::Key && tx.and_then(|s| s.xit_on) == Some(true) {
        return Err(Refusal::XitOn);
    }
    Ok(())
}

/// Another program's DAX transmit stream, if the radio reports one: a `dax_tx` stream that is not
/// ours, by its owner or by our own create's reply, and is not a dead orphan (owner zero and
/// endpoint `0.0.0.0`, the rule `super::ownership` applies to receive streams). An owner the
/// radio did not report counts as another program's: when that cannot be told, nothing is
/// written.
pub fn another_dax_feeder(
    model: &StatusModel,
    ours: Option<u32>,
    own_tx_stream: Option<u32>,
) -> Option<(u32, Owner)> {
    model.streams.iter().find_map(|(id, s)| {
        if s.kind.as_deref() != Some("dax_tx") || Some(*id) == own_tx_stream {
            return None;
        }
        let dead_orphan =
            s.client_handle == Some(0) && s.ip.as_deref().map(str::trim) == Some("0.0.0.0");
        match owner_of(s.client_handle, ours) {
            Owner::Ours => None,
            _ if dead_orphan => None,
            owner => Some((*id, owner)),
        }
    })
}

/// Run the checks for a change to the transmitter's audio source. Pure, like [`admit`].
pub fn admit_tx_audio(
    model: &StatusModel,
    facts: &Facts,
    audio: TxAudio,
) -> Result<AdmittedTxAudio, Refusal> {
    if !facts.ready {
        return Err(Refusal::NotReady);
    }
    if !facts.transmit_protocol {
        return Err(Refusal::ProtocolUnsupported);
    }
    let ours = facts.handle.ok_or(Refusal::NoHandle)?;
    if facts.keyed {
        return Err(Refusal::AlreadyKeyed);
    }
    if let Some((stream, owner)) = another_dax_feeder(model, facts.handle, facts.dax_tx_stream) {
        return Err(Refusal::AnotherDaxFeeder { stream, owner });
    }
    transmitter_idle(model)?;
    if let TxAudio::DaxSource { .. } = audio {
        // Radio-wide: written only while the transmit slice is ours.
        match model.tx_slices().as_slice() {
            [] => return Err(Refusal::NoTxSlice),
            [slice] => {
                let reported = model.slices.get(slice).and_then(|s| s.client_handle);
                let owner = owner_of(reported, Some(ours));
                if owner != Owner::Ours {
                    return Err(Refusal::TxSliceNotOurs {
                        slice: *slice,
                        owner,
                    });
                }
            }
            several => return Err(Refusal::SeveralTxSlices(several.to_vec())),
        }
    }
    Ok(AdmittedTxAudio(audio))
}

/// The interlock idle: a whole sample naming no transmitting client (ours neither), no keying
/// source, and a receive-side state.
pub fn transmitter_idle(model: &StatusModel) -> Result<(), Refusal> {
    let sample = model
        .interlock
        .sample
        .as_ref()
        .ok_or(Refusal::InterlockUnknown)?;
    if sample.tx_client_handle != 0 {
        // Ours too: until the readback has seen the transmitter released, it is not idle.
        return Err(Refusal::TransmitterHeld {
            by: sample.tx_client_handle,
        });
    }
    match sample.source.as_str() {
        "" => {}
        "MIC" | "ACC" | "RCA" => {
            return Err(Refusal::HardwarePtt {
                source: sample.source.clone(),
            })
        }
        other => {
            return Err(Refusal::SourceActive {
                source: other.to_string(),
            })
        }
    }
    // Receive-side states only: anything else is a transmission starting, running or ending.
    if !matches!(sample.state.as_str(), "READY" | "RECEIVE" | "NOT_READY") {
        return Err(Refusal::InterlockNotReady {
            state: sample.state.clone(),
        });
    }
    Ok(())
}

/// Run the checks for a move of the transmit flag to `tx.slice`. Pure, like [`admit`]: the
/// session registered on a version reviewed for transmit, nothing of ours keyed, the interlock
/// idle ([`admit_tx_audio`]'s checks), and the slice ours.
pub fn admit_tx_slice(
    model: &StatusModel,
    facts: &Facts,
    tx: TxSlice,
) -> Result<AdmittedTxSlice, Refusal> {
    if !facts.ready {
        return Err(Refusal::NotReady);
    }
    if !facts.transmit_protocol {
        return Err(Refusal::ProtocolUnsupported);
    }
    let ours = facts.handle.ok_or(Refusal::NoHandle)?;
    if facts.keyed {
        return Err(Refusal::AlreadyKeyed);
    }
    transmitter_idle(model)?;
    // The flag is taken from nobody: wherever it is now must be ours, or nowhere. On a multiFLEX
    // radio another station's slice may hold it, and moving it would take their transmitter.
    for holder in model.tx_slices() {
        let owner = owner_of(
            model.slices.get(&holder).and_then(|s| s.client_handle),
            Some(ours),
        );
        if owner != Owner::Ours {
            return Err(Refusal::TxSliceNotOurs {
                slice: holder,
                owner,
            });
        }
    }
    let reported = model.slices.get(&tx.slice).and_then(|s| s.client_handle);
    let owner = owner_of(reported, Some(ours));
    if owner != Owner::Ours {
        return Err(Refusal::TxSliceNotOurs {
            slice: tx.slice,
            owner,
        });
    }
    Ok(AdmittedTxSlice(tx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flex::encode::CwxText;
    use crate::flex::status::decode;
    use crate::flex::wire::{parse_line, Line};

    const OURS: u32 = 0x2B6E_1F40;
    const IDLE: &str =
        "S0|interlock tx_client_handle=0x00000000 state=READY reason= source= tx_allowed=1 amplifier=";
    const OUR_TX_SLICE: &str = "S2B6E1F40|slice 0 in_use=1 tx=1 client_handle=0x2B6E1F40";
    /// Our transmit slice in CW, XIT off, and the radio's keyer as a CWX word needs it.
    const OUR_CW_SLICE: &str =
        "S2B6E1F40|slice 0 in_use=1 tx=1 client_handle=0x2B6E1F40 mode=CW xit_on=0";
    const KEYER: &str = "S0|transmit break_in=1 break_in_delay=300 synccwx=0";
    /// The radio's tuner fitted, as the simulator's radio reports it.
    const ATU: &str = "S0|atu status=NONE atu_enabled=1 memories_enabled=0 using_mem=0";

    fn model(lines: &[&str]) -> StatusModel {
        let mut m = StatusModel::default();
        for line in lines {
            match parse_line(line) {
                Ok(Line::Status(s)) => m.apply(&decode(&s)),
                other => panic!("{line:?}: {other:?}"),
            }
        }
        m
    }

    fn facts() -> Facts {
        Facts {
            ready: true,
            transmit_protocol: true,
            handle: Some(OURS),
            keyed: false,
            open: None,
            readback_idle: true,
            dax_tx_stream: None,
            operator_key: None,
        }
    }

    fn refused(m: &StatusModel, f: &Facts) -> Refusal {
        admit(m, f, TxStart::Key).expect_err("refused")
    }

    #[test]
    fn a_key_is_admitted_on_our_tx_slice_with_an_idle_interlock() {
        // The positive control every refusal below is measured against.
        let m = model(&[OUR_TX_SLICE, IDLE]);
        let admitted = admit(&m, &facts(), TxStart::Key).expect("admitted");
        assert_eq!(admitted.into_start(), TxStart::Key);
        // The interlock naming us is allowed as well as naming no client.
        let ours_named =
            "S0|interlock tx_client_handle=0x2B6E1F40 state=READY reason= source= tx_allowed=1";
        assert!(admit(&model(&[OUR_TX_SLICE, ours_named]), &facts(), TxStart::Key).is_ok());
    }

    #[test]
    fn each_session_fact_refuses() {
        let m = model(&[OUR_TX_SLICE, IDLE]);
        let with = |change: fn(&mut Facts)| {
            let mut f = facts();
            change(&mut f);
            refused(&m, &f)
        };
        assert_eq!(with(|f| f.ready = false), Refusal::NotReady);
        assert_eq!(
            with(|f| f.transmit_protocol = false),
            Refusal::ProtocolUnsupported
        );
        assert_eq!(with(|f| f.handle = None), Refusal::NoHandle);
        assert_eq!(with(|f| f.keyed = true), Refusal::AlreadyKeyed);
        assert_eq!(with(|f| f.readback_idle = false), Refusal::ReadbackNotIdle);
    }

    #[test]
    fn the_tx_slice_must_be_ours_and_alone() {
        assert_eq!(refused(&model(&[IDLE]), &facts()), Refusal::NoTxSlice);
        let foreign = "S7A3C0001|slice 1 in_use=1 tx=1 client_handle=0x7A3C0001";
        assert_eq!(
            refused(&model(&[foreign, IDLE]), &facts()),
            Refusal::TxSliceNotOurs {
                slice: 1,
                owner: Owner::Foreign(0x7A3C_0001)
            }
        );
        // An owner not yet reported is not ours either.
        assert_eq!(
            refused(&model(&["S0|slice 0 in_use=1 tx=1", IDLE]), &facts()),
            Refusal::TxSliceNotOurs {
                slice: 0,
                owner: Owner::Unknown
            }
        );
        assert_eq!(
            refused(&model(&[OUR_TX_SLICE, foreign, IDLE]), &facts()),
            Refusal::SeveralTxSlices(vec![0, 1])
        );
    }

    #[test]
    fn each_interlock_condition_refuses() {
        let with = |interlock: &str| refused(&model(&[OUR_TX_SLICE, interlock]), &facts());
        assert_eq!(
            refused(&model(&[OUR_TX_SLICE]), &facts()),
            Refusal::InterlockUnknown
        );
        // A partial line withdraws the sample: never merged into one.
        assert_eq!(
            refused(
                &model(&[OUR_TX_SLICE, IDLE, "S0|interlock tx_allowed=1"]),
                &facts()
            ),
            Refusal::InterlockUnknown
        );
        assert_eq!(
            with("S0|interlock tx_client_handle=0x00000000 state=TRANSMITTING reason= source=MIC tx_allowed=1"),
            Refusal::HardwarePtt { source: "MIC".into() }
        );
        assert_eq!(
            with("S0|interlock tx_client_handle=0x7A3C0001 state=TRANSMITTING reason= source=SW tx_allowed=1"),
            Refusal::TransmitterHeld { by: 0x7A3C_0001 },
            "a named owner is reported before its source"
        );
        assert_eq!(
            with("S0|interlock tx_client_handle=0x00000000 state=TRANSMITTING reason= source=SW tx_allowed=1"),
            Refusal::SourceActive { source: "SW".into() }
        );
        assert_eq!(
            with(
                "S0|interlock tx_client_handle=0x7A3C0001 state=READY reason= source= tx_allowed=1"
            ),
            Refusal::TransmitterHeld { by: 0x7A3C_0001 }
        );
        assert_eq!(
            with(
                "S0|interlock tx_client_handle=0x00000000 state=READY reason= source= tx_allowed=0"
            ),
            Refusal::TransmitNotAllowed {
                reason: String::new()
            }
        );
        assert_eq!(
            with("S0|interlock tx_client_handle=0x00000000 state=NOT_READY reason=OUT_OF_BAND source= tx_allowed=0"),
            Refusal::TransmitNotAllowed { reason: "OUT_OF_BAND".into() }
        );
        assert_eq!(
            with("S0|interlock tx_client_handle=0x00000000 state=RECEIVE reason= source= tx_allowed=1"),
            Refusal::InterlockNotReady { state: "RECEIVE".into() }
        );
    }

    #[test]
    fn starts_without_a_readback_are_refused() {
        let m = model(&[OUR_TX_SLICE, IDLE]);
        for start in [
            TxStart::TuneOn,
            TxStart::AtuStart,
            TxStart::CwxSend(CwxText::new("TEST").unwrap()),
        ] {
            let kind = start.kind();
            assert_eq!(
                admit(&m, &facts(), start).expect_err("refused"),
                Refusal::NoReadback(kind)
            );
        }
    }

    /// A CWX word joins our own open CWX operation while the radio keys it, and nothing else
    /// does; every check but the two that describe that operation still refuses.
    #[test]
    fn a_cwx_word_joins_our_open_cwx_and_nothing_else_does() {
        let cwx = || TxStart::CwxSend(CwxText::new("TEST").unwrap());
        let keying = |source: &str| {
            format!(
                "S0|interlock tx_client_handle=0x2B6E1F40 state=TRANSMITTING reason= \
                 source={source} tx_allowed=1"
            )
        };
        let open = |kind| Facts {
            keyed: true,
            open: Some(kind),
            readback_idle: false,
            ..facts()
        };
        let m = model(&[
            OUR_CW_SLICE,
            KEYER,
            &keying(crate::flex::ptt_evidence::CWX.source),
        ]);
        assert!(admit_unbenched(&m, &open(StartKind::Cwx), cwx()).is_ok());
        // Only a CWX word, and only into our CWX.
        assert_eq!(
            admit_unbenched(&m, &open(StartKind::Cwx), TxStart::Key).unwrap_err(),
            Refusal::AlreadyKeyed
        );
        assert_eq!(
            admit_unbenched(&m, &open(StartKind::Cwx), TxStart::TuneOn).unwrap_err(),
            Refusal::AlreadyKeyed
        );
        assert_eq!(
            admit_unbenched(&m, &open(StartKind::Tune), cwx()).unwrap_err(),
            Refusal::AlreadyKeyed
        );
        // Every other check stands.
        for (line, want) in [
            (
                keying("MIC"),
                Refusal::HardwarePtt {
                    source: "MIC".into(),
                },
            ),
            (
                keying("TUNE"),
                Refusal::SourceActive {
                    source: "TUNE".into(),
                },
            ),
            (
                "S0|interlock tx_client_handle=0x7A3C0001 state=TRANSMITTING reason= source=SW \
                 tx_allowed=1"
                    .to_string(),
                Refusal::TransmitterHeld { by: 0x7A3C_0001 },
            ),
            (
                keying("SW").replace("tx_allowed=1", "tx_allowed=0"),
                Refusal::TransmitNotAllowed {
                    reason: String::new(),
                },
            ),
        ] {
            let m = model(&[OUR_CW_SLICE, KEYER, &line]);
            assert_eq!(
                admit_unbenched(&m, &open(StartKind::Cwx), cwx()).unwrap_err(),
                want,
                "{line}"
            );
        }
        // ...the slice's mode and the radio's keyer among them: a word into our CWX after the
        // slice left CW is refused.
        let usb = OUR_CW_SLICE.replace("mode=CW", "mode=USB");
        let m = model(&[&usb, KEYER, &keying(crate::flex::ptt_evidence::CWX.source)]);
        assert_eq!(
            admit_unbenched(&m, &open(StartKind::Cwx), cwx()).unwrap_err(),
            Refusal::NotCw { mode: "USB".into() }
        );
        // And the bench's list holds: in production, no CWX word at all.
        assert_eq!(
            admit(&m, &open(StartKind::Cwx), cwx()).unwrap_err(),
            Refusal::NoReadback(StartKind::Cwx)
        );
        assert_eq!(BENCHED, &[StartKind::Key]);
    }

    /// ⭐ While the operator's own key keys the radio as the radio reports it (a key named; this
    /// test's own, since no bench has named one), every start is refused with the operator's
    /// reason, ahead of "already keyed": the radio is theirs. Under no client when the bench says
    /// so. With no key named, the same sample refuses as any keying source does today; under
    /// another client's handle, as another client's transmission; released, the key is admitted.
    #[test]
    fn the_operators_key_keying_refuses_every_start() {
        use crate::flex::ptt_evidence::{KeyHandle, OperatorKey};
        let paddle = OperatorKey {
            source: "PADDLE",
            handle: KeyHandle::Ours,
        };
        let keyed = |state: &str, handle: u32| {
            format!(
                "S0|interlock tx_client_handle=0x{handle:08X} state={state} reason= \
                 source=PADDLE tx_allowed=1"
            )
        };
        let cwx = || TxStart::CwxSend(CwxText::new("TEST").unwrap());
        let named = |key| Facts {
            operator_key: Some(key),
            ..facts()
        };
        for (key, handle) in [
            (paddle, OURS),
            (
                OperatorKey {
                    handle: KeyHandle::NoClient,
                    ..paddle
                },
                0,
            ),
        ] {
            for state in ["PTT_REQUESTED", "TRANSMITTING"] {
                let m = model(&[OUR_CW_SLICE, KEYER, ATU, &keyed(state, handle)]);
                for start in [TxStart::Key, TxStart::TuneOn, TxStart::AtuStart, cwx()] {
                    assert_eq!(
                        admit_unbenched(&m, &named(key), start).unwrap_err(),
                        Refusal::OperatorKeying,
                        "{key:?} {state}"
                    );
                }
                assert_eq!(
                    admit(&m, &named(key), TxStart::Key).unwrap_err(),
                    Refusal::OperatorKeying
                );
                // Inside our open CWX: the operator's, not "already keyed".
                let open = Facts {
                    keyed: true,
                    open: Some(StartKind::Cwx),
                    readback_idle: false,
                    ..named(key)
                };
                assert_eq!(
                    admit_unbenched(&m, &open, cwx()).unwrap_err(),
                    Refusal::OperatorKeying
                );
            }
        }
        let m = model(&[OUR_CW_SLICE, KEYER, &keyed("TRANSMITTING", OURS)]);
        assert_eq!(
            admit(&m, &facts(), TxStart::Key).unwrap_err(),
            Refusal::SourceActive {
                source: "PADDLE".into()
            },
            "no key named: today's refusal"
        );
        let m = model(&[OUR_CW_SLICE, KEYER, &keyed("TRANSMITTING", 0x7A3C_0001)]);
        assert_eq!(
            admit(&m, &named(paddle), TxStart::Key).unwrap_err(),
            Refusal::TransmitterHeld { by: 0x7A3C_0001 }
        );
        let m = model(&[OUR_CW_SLICE, KEYER, IDLE]);
        assert!(admit(&m, &named(paddle), TxStart::Key).is_ok());
        assert_eq!(
            Refusal::OperatorKeying.to_string(),
            "your key or paddle is keying the radio"
        );
    }

    /// ⭐ A CWX word goes only to a transmit slice in CW, with the radio's break-in on and its Sync
    /// CWX not on; a tune, an ATU cycle or a CWX word never while the slice's XIT is on. Each
    /// refusal by value against its control, the key untouched by all four, and the switch still
    /// off in production.
    #[test]
    fn cwx_is_refused_off_a_cw_slice_and_with_break_in_off() {
        let cwx = || TxStart::CwxSend(CwxText::new("CQ").unwrap());
        let digu = OUR_CW_SLICE.replace("mode=CW", "mode=DIGU");
        let xit = OUR_CW_SLICE.replace("xit_on=0", "xit_on=1");
        let refused = |lines: &[&str], start: TxStart| {
            admit_unbenched(&model(lines), &facts(), start).unwrap_err()
        };
        // The control: our CW slice, break-in on, Sync CWX off, XIT off.
        assert!(admit_unbenched(&model(&[OUR_CW_SLICE, KEYER, IDLE]), &facts(), cwx()).is_ok());
        for (lines, want) in [
            (
                vec![digu.as_str(), KEYER, IDLE],
                Refusal::NotCw {
                    mode: "DIGU".into(),
                },
            ),
            (
                vec![OUR_TX_SLICE, KEYER, IDLE],
                Refusal::NotCw {
                    mode: String::new(),
                },
            ),
            (
                vec![OUR_CW_SLICE, "S0|transmit break_in=0 synccwx=0", IDLE],
                Refusal::BreakInOff,
            ),
            (
                vec![OUR_CW_SLICE, "S0|transmit synccwx=0", IDLE],
                Refusal::BreakInOff,
            ),
            (
                vec![OUR_CW_SLICE, "S0|transmit break_in=1 synccwx=1", IDLE],
                Refusal::SyncCwx,
            ),
            (vec![xit.as_str(), KEYER, IDLE], Refusal::XitOn),
        ] {
            assert_eq!(refused(&lines, cwx()), want, "{lines:?}");
        }
        // XIT refuses each carrier the radio makes itself; the CW rules are CWX's alone.
        for start in [TxStart::TuneOn, TxStart::AtuStart] {
            assert_eq!(
                refused(&[xit.as_str(), KEYER, IDLE, ATU], start),
                Refusal::XitOn
            );
        }
        let off = [digu.as_str(), "S0|transmit break_in=0 synccwx=1", IDLE, ATU];
        for start in [TxStart::TuneOn, TxStart::AtuStart] {
            assert!(admit_unbenched(&model(&off), &facts(), start).is_ok());
        }
        // The key: none of the four.
        let all = digu.replace("xit_on=0", "xit_on=1");
        let all = [all.as_str(), "S0|transmit break_in=0 synccwx=1", IDLE];
        assert!(admit(&model(&all), &facts(), TxStart::Key).is_ok());
        // And in production the switch is off: no CWX word, before any of these.
        assert_eq!(
            admit(&model(&[OUR_CW_SLICE, KEYER, IDLE]), &facts(), cwx()).unwrap_err(),
            Refusal::NoReadback(StartKind::Cwx)
        );
        assert_eq!(BENCHED, &[StartKind::Key]);
    }

    /// ⭐ An ATU cycle needs the radio's tuner: refused while the radio reports none fitted, by its
    /// `atu` status saying `atu_enabled=0` or by no `atu` status at all; admitted once it says
    /// `atu_enabled=1`. Nothing else is asked of the tuner (its status is the cycle's own), the
    /// tune carrier needs none, and in production the switch is still off, before this check.
    #[test]
    fn an_atu_cycle_needs_the_radios_tuner() {
        let atu = |lines: &[&str]| admit_unbenched(&model(lines), &facts(), TxStart::AtuStart);
        let not_fitted = ATU.replace("atu_enabled=1", "atu_enabled=0");
        for lines in [
            vec![OUR_TX_SLICE, IDLE],
            vec![OUR_TX_SLICE, IDLE, not_fitted.as_str()],
            vec![OUR_TX_SLICE, IDLE, "S0|atu status=TUNE_SUCCESSFUL"],
        ] {
            assert_eq!(atu(&lines).unwrap_err(), Refusal::NoAtu, "{lines:?}");
        }
        for status in ["NONE", "TUNE_SUCCESSFUL", "TUNE_BYPASS", "TUNE_FAIL"] {
            let fitted = ATU.replace("status=NONE", &format!("status={status}"));
            assert!(atu(&[OUR_TX_SLICE, IDLE, &fitted]).is_ok(), "{status}");
        }
        // A later line saying it is not fitted takes it back.
        assert_eq!(
            atu(&[OUR_TX_SLICE, IDLE, ATU, &not_fitted]).unwrap_err(),
            Refusal::NoAtu
        );
        assert!(admit_unbenched(&model(&[OUR_TX_SLICE, IDLE]), &facts(), TxStart::TuneOn).is_ok());
        assert_eq!(
            admit(&model(&[OUR_TX_SLICE, IDLE]), &facts(), TxStart::AtuStart).unwrap_err(),
            Refusal::NoReadback(StartKind::Atu)
        );
        assert_eq!(BENCHED, &[StartKind::Key]);
    }

    // ── The operator's Beta setting ──

    const ALL_KINDS: [StartKind; 4] = [
        StartKind::Key,
        StartKind::Tune,
        StartKind::Atu,
        StartKind::Cwx,
    ];

    fn start_of(kind: StartKind) -> TxStart {
        match kind {
            StartKind::Key => TxStart::Key,
            StartKind::Tune => TxStart::TuneOn,
            StartKind::Atu => TxStart::AtuStart,
            StartKind::Cwx => TxStart::CwxSend(CwxText::new("TEST").unwrap()),
        }
    }

    /// The scene every check passes on, for every kind: our transmit slice in CW with XIT off, the
    /// radio's keyer as a CWX word needs it, its tuner fitted, the interlock idle.
    fn passing_lines() -> Vec<String> {
        [OUR_CW_SLICE, KEYER, ATU, IDLE].map(String::from).to_vec()
    }

    /// One admission check failing on its own: how the session's facts and the radio's lines change
    /// from the scene every check passes on, and what each kind is refused with then (`None`:
    /// admitted).
    struct Failing {
        check: &'static str,
        facts: fn(&mut Facts),
        lines: fn(&mut Vec<String>),
        want: fn(StartKind) -> Option<Refusal>,
    }

    /// Every check but [`BENCHED`]'s, each failing on its own (the slice's lines are at 0, the
    /// keyer's at 1, the tuner's at 2, the interlock's at 3), and our own open CWX, which a word
    /// joins.
    fn failing() -> Vec<Failing> {
        use crate::flex::ptt_evidence::{KeyHandle, OperatorKey};
        fn none(_: &mut Facts) {}
        fn same(_: &mut Vec<String>) {}
        vec![
            Failing {
                check: "every check passes",
                facts: none,
                lines: same,
                want: |_| None,
            },
            Failing {
                check: "not registered",
                facts: |f| f.ready = false,
                lines: same,
                want: |_| Some(Refusal::NotReady),
            },
            Failing {
                check: "an API version not reviewed for transmit",
                facts: |f| f.transmit_protocol = false,
                lines: same,
                want: |_| Some(Refusal::ProtocolUnsupported),
            },
            Failing {
                check: "no client handle",
                facts: |f| f.handle = None,
                lines: same,
                want: |_| Some(Refusal::NoHandle),
            },
            Failing {
                check: "an over of ours still unconfirmed",
                facts: |f| {
                    f.keyed = true;
                    f.open = Some(StartKind::Key);
                },
                lines: same,
                want: |_| Some(Refusal::AlreadyKeyed),
            },
            Failing {
                check: "the readback has not seen the radio idle",
                facts: |f| f.readback_idle = false,
                lines: same,
                want: |_| Some(Refusal::ReadbackNotIdle),
            },
            Failing {
                check: "the operator's own key keying the radio",
                facts: |f| {
                    f.operator_key = Some(OperatorKey {
                        source: "PADDLE",
                        handle: KeyHandle::Ours,
                    });
                },
                lines: |l| {
                    l[3] = "S0|interlock tx_client_handle=0x2B6E1F40 state=TRANSMITTING reason= \
                            source=PADDLE tx_allowed=1"
                        .into();
                },
                want: |_| Some(Refusal::OperatorKeying),
            },
            Failing {
                check: "no transmit slice",
                facts: none,
                lines: |l| l[0] = l[0].replace("tx=1", "tx=0"),
                want: |_| Some(Refusal::NoTxSlice),
            },
            Failing {
                check: "the transmit slice another client's",
                facts: none,
                lines: |l| {
                    l[0] = "S7A3C0001|slice 0 in_use=1 tx=1 client_handle=0x7A3C0001 mode=CW \
                            xit_on=0"
                        .into();
                },
                want: |_| {
                    Some(Refusal::TxSliceNotOurs {
                        slice: 0,
                        owner: Owner::Foreign(0x7A3C_0001),
                    })
                },
            },
            Failing {
                check: "two transmit slices",
                facts: none,
                lines: |l| {
                    l.push("S7A3C0001|slice 1 in_use=1 tx=1 client_handle=0x7A3C0001".into())
                },
                want: |_| Some(Refusal::SeveralTxSlices(vec![0, 1])),
            },
            Failing {
                check: "the transmit slice not in CW",
                facts: none,
                lines: |l| l[0] = l[0].replace("mode=CW", "mode=USB"),
                want: |k| (k == StartKind::Cwx).then(|| Refusal::NotCw { mode: "USB".into() }),
            },
            Failing {
                check: "the radio's break-in off",
                facts: none,
                lines: |l| l[1] = l[1].replace("break_in=1", "break_in=0"),
                want: |k| (k == StartKind::Cwx).then_some(Refusal::BreakInOff),
            },
            Failing {
                check: "the radio's Sync CWX on",
                facts: none,
                lines: |l| l[1] = l[1].replace("synccwx=0", "synccwx=1"),
                want: |k| (k == StartKind::Cwx).then_some(Refusal::SyncCwx),
            },
            Failing {
                check: "the transmit slice's XIT on",
                facts: none,
                lines: |l| l[0] = l[0].replace("xit_on=0", "xit_on=1"),
                want: |k| (k != StartKind::Key).then_some(Refusal::XitOn),
            },
            Failing {
                check: "no tuner fitted",
                facts: none,
                lines: |l| l[2] = l[2].replace("atu_enabled=1", "atu_enabled=0"),
                want: |k| (k == StartKind::Atu).then_some(Refusal::NoAtu),
            },
            Failing {
                check: "no whole interlock sample",
                facts: none,
                lines: |l| l.push("S0|interlock tx_allowed=1".into()),
                want: |_| Some(Refusal::InterlockUnknown),
            },
            Failing {
                check: "another client holding the transmitter",
                facts: none,
                lines: |l| {
                    l[3] =
                        l[3].replace("tx_client_handle=0x00000000", "tx_client_handle=0x7A3C0001")
                },
                want: |_| Some(Refusal::TransmitterHeld { by: 0x7A3C_0001 }),
            },
            Failing {
                check: "the mic's PTT keying the radio",
                facts: none,
                lines: |l| {
                    l[3] = "S0|interlock tx_client_handle=0x00000000 state=TRANSMITTING reason= \
                            source=MIC tx_allowed=1"
                        .into();
                },
                want: |_| {
                    Some(Refusal::HardwarePtt {
                        source: "MIC".into(),
                    })
                },
            },
            Failing {
                check: "something else keying the radio",
                facts: none,
                lines: |l| {
                    l[3] = "S0|interlock tx_client_handle=0x00000000 state=TRANSMITTING reason= \
                            source=SW tx_allowed=1"
                        .into();
                },
                want: |_| {
                    Some(Refusal::SourceActive {
                        source: "SW".into(),
                    })
                },
            },
            Failing {
                check: "transmit not allowed here",
                facts: none,
                lines: |l| l[3] = l[3].replace("tx_allowed=1", "tx_allowed=0"),
                want: |_| {
                    Some(Refusal::TransmitNotAllowed {
                        reason: String::new(),
                    })
                },
            },
            Failing {
                check: "the interlock not READY",
                facts: none,
                lines: |l| l[3] = l[3].replace("state=READY", "state=RECEIVE"),
                want: |_| {
                    Some(Refusal::InterlockNotReady {
                        state: "RECEIVE".into(),
                    })
                },
            },
            Failing {
                check: "our own CWX open and keying",
                facts: |f| {
                    f.keyed = true;
                    f.open = Some(StartKind::Cwx);
                    f.readback_idle = false;
                },
                lines: |l| {
                    l[3] = format!(
                        "S0|interlock tx_client_handle=0x2B6E1F40 state=TRANSMITTING reason= \
                         source={} tx_allowed=1",
                        crate::flex::ptt_evidence::CWX.source
                    );
                },
                want: |k| (k != StartKind::Cwx).then_some(Refusal::AlreadyKeyed),
            },
        ]
    }

    /// The scene `cases` describe together: the radio's model and the session's facts.
    fn scene(cases: &[&Failing]) -> (StatusModel, Facts) {
        let mut f = facts();
        let mut lines = passing_lines();
        for case in cases {
            (case.facts)(&mut f);
            (case.lines)(&mut lines);
        }
        (
            model(&lines.iter().map(String::as_str).collect::<Vec<_>>()),
            f,
        )
    }

    /// ⭐ The operator's Beta setting, per radio and off by default (`session::Config::beta`):
    /// every start kind, with the setting off and on, in the scene every check passes on and with
    /// each other check failing on its own, by value. Off, a tune, an ATU cycle and a CWX word are
    /// refused `NoReadback` whatever else holds, exactly as before the setting existed; on, each
    /// is admitted only where every other check passes, and each other check refuses it with its
    /// own reason. A key is the same either way, and [`BENCHED`] is unchanged.
    #[test]
    fn the_beta_setting_admits_tune_atu_and_cwx_and_every_other_check_still_refuses() {
        for case in &failing() {
            let (m, f) = scene(&[case]);
            for kind in ALL_KINDS {
                let off = admit(&m, &f, start_of(kind)).err();
                let on = admit_beta(&m, &f, start_of(kind)).err();
                let want = (case.want)(kind);
                let today = if kind == StartKind::Key {
                    want.clone()
                } else {
                    Some(Refusal::NoReadback(kind))
                };
                assert_eq!(off, today, "setting off, {kind:?}: {}", case.check);
                assert_eq!(on, want, "setting on, {kind:?}: {}", case.check);
            }
        }
        for kind in ALL_KINDS {
            assert_eq!(switched_on(kind, false), kind == StartKind::Key, "{kind:?}");
            assert!(switched_on(kind, true), "{kind:?}");
        }
        assert_eq!(BENCHED, &[StartKind::Key]);
        assert_eq!(BETA, &[StartKind::Tune, StartKind::Atu, StartKind::Cwx]);
    }

    /// ⭐ In the same order: with any two checks failing at once, the Beta setting refuses a tune,
    /// an ATU cycle and a CWX word with the reason the test-only door gives (the same checks, in
    /// the order the tests above established), and a key with today's; off, nothing but
    /// `NoReadback` for the three.
    #[test]
    fn with_two_checks_failing_the_beta_setting_refuses_in_the_same_order() {
        let cases = failing();
        let mut pairs = 0;
        for a in &cases {
            for b in &cases {
                let (m, f) = scene(&[a, b]);
                for kind in ALL_KINDS {
                    let off = admit(&m, &f, start_of(kind)).err();
                    let on = admit_beta(&m, &f, start_of(kind)).err();
                    let door = admit_unbenched(&m, &f, start_of(kind)).err();
                    let why = format!("{kind:?}: {} + {}", a.check, b.check);
                    assert_eq!(on, door, "setting on, {why}");
                    if kind == StartKind::Key {
                        assert_eq!(off, door, "setting off, {why}");
                    } else {
                        assert_eq!(off, Some(Refusal::NoReadback(kind)), "setting off, {why}");
                    }
                }
                pairs += 1;
            }
        }
        assert_eq!(pairs, cases.len() * cases.len());
    }

    // ── Which slice transmits ──

    /// Our receive slice beside our transmit slice: the pair a split is made of.
    const OUR_RX_SLICE: &str = "S2B6E1F40|slice 1 in_use=1 tx=0 client_handle=0x2B6E1F40";

    fn slice_refused(m: &StatusModel, f: &Facts, slice: u8) -> Refusal {
        admit_tx_slice(m, f, TxSlice { slice }).expect_err("refused")
    }

    #[test]
    fn the_transmit_flag_moves_to_a_slice_of_ours_with_an_idle_transmitter() {
        // The positive control: either slice of ours, the one transmitting now included.
        let m = model(&[OUR_TX_SLICE, OUR_RX_SLICE, IDLE]);
        for slice in [0, 1] {
            let a = admit_tx_slice(&m, &facts(), TxSlice { slice }).expect("admitted");
            assert_eq!(a.into_tx_slice(), TxSlice { slice });
        }
    }

    #[test]
    fn the_transmit_flag_never_moves_while_anything_is_keyed_or_keying() {
        let m = model(&[OUR_TX_SLICE, OUR_RX_SLICE, IDLE]);
        let keyed = Facts {
            keyed: true,
            ..facts()
        };
        assert_eq!(slice_refused(&m, &keyed, 1), Refusal::AlreadyKeyed);
        for (line, want) in [
            (
                "S0|interlock tx_client_handle=0x2B6E1F40 state=TRANSMITTING reason= source=SW tx_allowed=1",
                Refusal::TransmitterHeld { by: OURS },
            ),
            (
                "S0|interlock tx_client_handle=0x00000000 state=PTT_REQUESTED reason= source= tx_allowed=1",
                Refusal::InterlockNotReady {
                    state: "PTT_REQUESTED".into(),
                },
            ),
            (
                "S0|interlock tx_client_handle=0x00000000 state=UNKEY_REQUESTED reason= source= tx_allowed=1",
                Refusal::InterlockNotReady {
                    state: "UNKEY_REQUESTED".into(),
                },
            ),
            (
                "S0|interlock tx_client_handle=0x00000000 state=READY reason= source=MIC tx_allowed=1",
                Refusal::HardwarePtt {
                    source: "MIC".into(),
                },
            ),
        ] {
            let m = model(&[OUR_TX_SLICE, OUR_RX_SLICE, line]);
            assert_eq!(slice_refused(&m, &facts(), 1), want, "{line}");
        }
        let m = model(&[
            OUR_TX_SLICE,
            OUR_RX_SLICE,
            IDLE,
            "S0|interlock tx_allowed=1",
        ]);
        assert_eq!(slice_refused(&m, &facts(), 1), Refusal::InterlockUnknown);
    }

    #[test]
    fn the_transmit_flag_never_moves_to_a_slice_not_ours() {
        let foreign = "S7A3C0001|slice 2 in_use=1 tx=0 client_handle=0x7A3C0001";
        let m = model(&[OUR_TX_SLICE, foreign, "S0|slice 3 in_use=1 tx=0", IDLE]);
        assert_eq!(
            slice_refused(&m, &facts(), 2),
            Refusal::TxSliceNotOurs {
                slice: 2,
                owner: Owner::Foreign(0x7A3C_0001)
            }
        );
        assert_eq!(
            slice_refused(&m, &facts(), 3),
            Refusal::TxSliceNotOurs {
                slice: 3,
                owner: Owner::Unknown
            }
        );
        // A slice the radio has not reported at all is not known to be ours either.
        assert!(matches!(
            slice_refused(&m, &facts(), 5),
            Refusal::TxSliceNotOurs { slice: 5, .. }
        ));
        for (f, want) in [
            (
                Facts {
                    ready: false,
                    ..facts()
                },
                Refusal::NotReady,
            ),
            (
                Facts {
                    transmit_protocol: false,
                    ..facts()
                },
                Refusal::ProtocolUnsupported,
            ),
            (
                Facts {
                    handle: None,
                    ..facts()
                },
                Refusal::NoHandle,
            ),
        ] {
            assert_eq!(slice_refused(&m, &f, 0), want);
        }
    }

    /// ⭐ THE FLAG IS NEVER TAKEN FROM ANOTHER CLIENT. On a multiFLEX radio another station's slice
    /// may hold the transmit flag; moving it to a slice of ours would take their transmitter. Only
    /// a flag that is ours, or on no slice at all, moves.
    #[test]
    fn the_transmit_flag_is_never_taken_off_another_clients_slice() {
        let foreign_tx = "S7A3C0001|slice 2 in_use=1 tx=1 client_handle=0x7A3C0001";
        let m = model(&[OUR_RX_SLICE, foreign_tx, IDLE]);
        assert_eq!(
            slice_refused(&m, &facts(), 1),
            Refusal::TxSliceNotOurs {
                slice: 2,
                owner: Owner::Foreign(0x7A3C_0001)
            }
        );
        // A holder whose owner the radio has not said is not known to be ours either.
        let m = model(&[OUR_RX_SLICE, "S0|slice 3 in_use=1 tx=1", IDLE]);
        assert_eq!(
            slice_refused(&m, &facts(), 1),
            Refusal::TxSliceNotOurs {
                slice: 3,
                owner: Owner::Unknown
            }
        );
        // Ours and a foreign holder at once: the foreign one is named.
        let m = model(&[OUR_TX_SLICE, OUR_RX_SLICE, foreign_tx, IDLE]);
        assert_eq!(
            slice_refused(&m, &facts(), 1),
            Refusal::TxSliceNotOurs {
                slice: 2,
                owner: Owner::Foreign(0x7A3C_0001)
            }
        );
        // Controls: our own flag moves, and so does a flag on no slice at all.
        let m = model(&[OUR_TX_SLICE, OUR_RX_SLICE, IDLE]);
        assert!(admit_tx_slice(&m, &facts(), TxSlice { slice: 1 }).is_ok());
        let m = model(&[OUR_RX_SLICE, IDLE]);
        assert!(admit_tx_slice(&m, &facts(), TxSlice { slice: 1 }).is_ok());
    }

    // ── The transmitter's audio source ──

    const SDR_DAX_TX: &str =
        "S7A3C0001|stream 0x84000001 type=dax_tx client_handle=0x7A3C0001 ip=192.168.1.30";

    fn audio_refused(m: &StatusModel, f: &Facts, audio: TxAudio) -> Refusal {
        admit_tx_audio(m, f, audio).expect_err("refused")
    }

    #[test]
    fn the_dax_source_is_admitted_on_our_tx_slice_with_an_idle_transmitter() {
        let m = model(&[OUR_TX_SLICE, IDLE]);
        for audio in [
            TxAudio::CreateDaxTx,
            TxAudio::DaxSource { dax: true },
            TxAudio::DaxSource { dax: false },
        ] {
            let a = admit_tx_audio(&m, &facts(), audio).expect("admitted");
            assert_eq!(a.into_tx_audio(), audio);
        }
        // Receive-side states other than READY are idle too: no transmit slice yet (RECEIVE), out
        // of band (NOT_READY).
        for state in ["RECEIVE", "NOT_READY"] {
            let line = IDLE.replace("state=READY", &format!("state={state}"));
            let m = model(&[OUR_TX_SLICE, &line]);
            assert!(admit_tx_audio(&m, &facts(), TxAudio::DaxSource { dax: true }).is_ok());
        }
    }

    #[test]
    fn never_while_anything_is_keyed_or_keying() {
        let m = model(&[OUR_TX_SLICE, IDLE]);
        let keyed = Facts {
            keyed: true,
            ..facts()
        };
        assert_eq!(
            audio_refused(&m, &keyed, TxAudio::DaxSource { dax: true }),
            Refusal::AlreadyKeyed
        );
        // Our own transmission, another client's, and every state of a key or an unkey.
        for (line, want) in [
            (
                "S0|interlock tx_client_handle=0x2B6E1F40 state=TRANSMITTING reason= source=SW tx_allowed=1",
                Refusal::TransmitterHeld { by: OURS },
            ),
            (
                "S0|interlock tx_client_handle=0x7A3C0001 state=TRANSMITTING reason= source=SW tx_allowed=1",
                Refusal::TransmitterHeld { by: 0x7A3C_0001 },
            ),
            (
                "S0|interlock tx_client_handle=0x00000000 state=PTT_REQUESTED reason= source= tx_allowed=1",
                Refusal::InterlockNotReady {
                    state: "PTT_REQUESTED".into(),
                },
            ),
            (
                "S0|interlock tx_client_handle=0x00000000 state=UNKEY_REQUESTED reason= source= tx_allowed=1",
                Refusal::InterlockNotReady {
                    state: "UNKEY_REQUESTED".into(),
                },
            ),
            (
                "S0|interlock tx_client_handle=0x00000000 state=READY reason= source=MIC tx_allowed=1",
                Refusal::HardwarePtt {
                    source: "MIC".into(),
                },
            ),
        ] {
            let m = model(&[OUR_TX_SLICE, line]);
            for audio in [TxAudio::CreateDaxTx, TxAudio::DaxSource { dax: false }] {
                assert_eq!(audio_refused(&m, &facts(), audio), want, "{line}");
            }
        }
        // No whole interlock sample: not known to be idle.
        let m = model(&[OUR_TX_SLICE, IDLE, "S0|interlock tx_allowed=1"]);
        assert_eq!(
            audio_refused(&m, &facts(), TxAudio::DaxSource { dax: true }),
            Refusal::InterlockUnknown
        );
    }

    #[test]
    fn never_beside_another_programs_dax_transmit_stream() {
        let m = model(&[OUR_TX_SLICE, IDLE, SDR_DAX_TX]);
        for audio in [
            TxAudio::CreateDaxTx,
            TxAudio::DaxSource { dax: true },
            TxAudio::DaxSource { dax: false },
        ] {
            assert_eq!(
                audio_refused(&m, &facts(), audio),
                Refusal::AnotherDaxFeeder {
                    stream: 0x8400_0001,
                    owner: Owner::Foreign(0x7A3C_0001)
                }
            );
        }
        // An owner the radio did not report is not known to be ours: refused too.
        let m = model(&[
            OUR_TX_SLICE,
            IDLE,
            "S0|stream 0x84000002 type=dax_tx ip=192.168.1.30",
        ]);
        assert!(matches!(
            audio_refused(&m, &facts(), TxAudio::DaxSource { dax: true }),
            Refusal::AnotherDaxFeeder {
                stream: 0x8400_0002,
                owner: Owner::Unknown
            }
        ));
        // Controls: our own stream, by owner or by our create's reply; a dead orphan; another
        // client's receive stream. None of them feeds the transmitter.
        let mine = "S2B6E1F40|stream 0x84000000 type=dax_tx client_handle=0x2B6E1F40 ip=10.0.0.2";
        let by_reply = "S0|stream 0x84000003 type=dax_tx ip=10.0.0.2";
        let orphan = "S0|stream 0x84000004 type=dax_tx client_handle=0x00000000 ip=0.0.0.0";
        let their_rx = "S7A3C0001|stream 0x04000009 type=dax_rx client_handle=0x7A3C0001";
        let m = model(&[OUR_TX_SLICE, IDLE, mine, by_reply, orphan, their_rx]);
        let f = Facts {
            dax_tx_stream: Some(0x8400_0003),
            ..facts()
        };
        assert_eq!(another_dax_feeder(&m, f.handle, f.dax_tx_stream), None);
        assert!(admit_tx_audio(&m, &f, TxAudio::DaxSource { dax: true }).is_ok());
        // Without the reply, the ownerless stream is not known to be ours.
        assert!(another_dax_feeder(&m, Some(OURS), None).is_some());
    }

    #[test]
    fn the_radio_wide_source_needs_our_tx_slice_and_the_stream_does_not() {
        let foreign_tx = "S7A3C0001|slice 1 in_use=1 tx=1 client_handle=0x7A3C0001";
        let m = model(&[foreign_tx, IDLE]);
        assert_eq!(
            audio_refused(&m, &facts(), TxAudio::DaxSource { dax: true }),
            Refusal::TxSliceNotOurs {
                slice: 1,
                owner: Owner::Foreign(0x7A3C_0001)
            }
        );
        assert!(admit_tx_audio(&m, &facts(), TxAudio::CreateDaxTx).is_ok());
        let m = model(&[IDLE]);
        assert_eq!(
            audio_refused(&m, &facts(), TxAudio::DaxSource { dax: false }),
            Refusal::NoTxSlice
        );
    }

    #[test]
    fn each_session_fact_refuses_the_audio_source_too() {
        let m = model(&[OUR_TX_SLICE, IDLE]);
        for (f, want) in [
            (
                Facts {
                    ready: false,
                    ..facts()
                },
                Refusal::NotReady,
            ),
            (
                Facts {
                    transmit_protocol: false,
                    ..facts()
                },
                Refusal::ProtocolUnsupported,
            ),
            (
                Facts {
                    handle: None,
                    ..facts()
                },
                Refusal::NoHandle,
            ),
        ] {
            assert_eq!(audio_refused(&m, &f, TxAudio::CreateDaxTx), want);
        }
    }
}
