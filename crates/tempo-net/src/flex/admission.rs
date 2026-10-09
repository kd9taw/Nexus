//! Admission: the Flex-side checks a transmit start must pass. They can only refuse.
//!
//! This is not Nexus's transmit gate. The engine decides whether a transmission may happen at
//! all (the TX-enable latch, licence privileges on the emitted RF, identity, the watchdog, the
//! one transmitter arbiter), and nothing in the app calls this module yet; the change that wires
//! the Flex client into the engine puts those gates in front of it. Admission adds what only the
//! radio's own status can say, and every check is a reason to refuse (port plan §3.2):
//!
//! - the session is registered, on a TCP API version reviewed for transmit (1.4), and has a
//!   client handle;
//! - nothing of ours is keyed already;
//! - exactly one slice is the transmit slice, and it is ours;
//! - the interlock's last whole sample is `READY`, transmit allowed, no reason, no keying source
//!   (no mic, ACC or RCA PTT), and names no transmitting client, or names us;
//! - the unkey readback is idle: it has seen the radio idle on this connection, so the end of the
//!   transmission can be proven ([`super::ptt_evidence`]);
//! - and the start is of a kind whose readback a tester's bench has confirmed ([`BENCHED`]). Today
//!   that is `xmit` only. Tune, ATU and CWX have readback profiles built from AetherSDR's notes,
//!   but their starts are refused, before any other check, until the bench confirms them.
//!
//! **A CWX word while our own CWX operation is open** joins it: the two checks that describe that
//! operation (nothing of ours keyed, the readback idle) do not apply, nor does the interlock's
//! `READY` while the radio keys our CWX under the CWX source. Every other check does: the transmit
//! slice ours and alone, no other transmitting client, no hardware source, transmit allowed, no
//! reason.
//!
//! [`Admitted`] is the value a start must carry to be rendered ([`super::encode::render_start`]).
//! Its field is private to this module and [`admit`] is the only constructor, so a keying
//! command cannot be produced anywhere else. It is neither `Clone` nor `Copy`: one admission
//! renders one start.
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
//! Nexus's own design, not a port.

use std::fmt;

use super::encode::{StartKind, TxAudio, TxStart};
use super::model::{owner_of, Owner, StatusModel};
use super::ptt_evidence;

/// The kinds whose readback a tester's bench has confirmed on a real radio. Only these are
/// admitted; any other start is refused with [`Refusal::NoReadback`] before every other check. A
/// kind joins this list only after its readback profile ([`super::ptt_evidence`]) has passed the
/// bench, never on the strength of the notes it was built from.
pub const BENCHED: &[StartKind] = &[StartKind::Key];

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
}

/// Why a start was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A kind whose readback no tester's bench has confirmed yet ([`BENCHED`]): tune, ATU, CWX.
    /// Also a CWX word sent with no keying time, which its readback needs.
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
    /// The radio does not allow transmit here (`tx_allowed=0`, or a reason such as
    /// `OUT_OF_BAND`).
    TransmitNotAllowed {
        reason: String,
    },
    /// The interlock is not `READY`.
    InterlockNotReady {
        state: String,
    },
    /// The readback has not seen the radio idle on this connection.
    ReadbackNotIdle,
    /// Another program feeds the radio's DAX transmit audio (SmartSDR's DAX, typically): Nexus
    /// neither writes the DAX source nor opens a transmit stream beside it.
    AnotherDaxFeeder {
        stream: u32,
        owner: Owner,
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
            Refusal::TransmitNotAllowed { reason } => {
                write!(f, "the radio does not allow transmit ({reason})")
            }
            Refusal::InterlockNotReady { state } => write!(f, "the interlock is {state}"),
            Refusal::ReadbackNotIdle => {
                f.write_str("the radio has not been seen idle on this connection")
            }
            Refusal::AnotherDaxFeeder { stream, owner } => write!(
                f,
                "another program feeds the radio's DAX transmit audio (stream 0x{stream:08X}, \
                 {owner:?})"
            ),
        }
    }
}

impl std::error::Error for Refusal {}

/// Run the Flex-side checks for `start`. Pure: no state changes, no I/O, and the only result
/// other than a refusal is the one value that can be rendered as a keying command.
pub fn admit(model: &StatusModel, facts: &Facts, start: TxStart) -> Result<Admitted, Refusal> {
    admit_kinds(model, facts, start, BENCHED)
}

/// [`admit`] for every kind, also those whose readback no bench has confirmed: the unit tests'
/// way to run the readback per kind. Not compiled outside this crate's tests.
#[cfg(test)]
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
    // A CWX word while our own CWX operation is open joins it (see the module header).
    let append = kind == StartKind::Cwx && facts.open == Some(StartKind::Cwx);
    if facts.keyed && !append {
        return Err(Refusal::AlreadyKeyed);
    }
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
        let m = model(&[OUR_TX_SLICE, &keying(crate::flex::ptt_evidence::CWX.source)]);
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
            let m = model(&[OUR_TX_SLICE, &line]);
            assert_eq!(
                admit_unbenched(&m, &open(StartKind::Cwx), cwx()).unwrap_err(),
                want,
                "{line}"
            );
        }
        // And the bench's list holds: in production, no CWX word at all.
        assert_eq!(
            admit(&m, &open(StartKind::Cwx), cwx()).unwrap_err(),
            Refusal::NoReadback(StartKind::Cwx)
        );
        assert_eq!(BENCHED, &[StartKind::Key]);
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
