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
//! - and the start is one whose end the readback can prove. Today that is `xmit` only: tune, ATU
//!   and CWX starts are refused until the readback covers them (their interlock sequences are
//!   not established on hardware).
//!
//! [`Admitted`] is the value a start must carry to be rendered ([`super::encode::render_start`]).
//! Its field is private to this module and [`admit`] is the only constructor, so a keying
//! command cannot be produced anywhere else. It is neither `Clone` nor `Copy`: one admission
//! renders one start.
//!
//! Nexus's own design, not a port.

use std::fmt;

use super::encode::{StartKind, TxStart};
use super::model::{owner_of, Owner, StatusModel};

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
    /// The readback has seen the radio idle and is ready to track a key.
    pub readback_idle: bool,
}

/// Why a start was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Tune, ATU and CWX: the readback cannot prove their end yet.
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
        }
    }
}

impl std::error::Error for Refusal {}

/// Run the Flex-side checks for `start`. Pure: no state changes, no I/O, and the only result
/// other than a refusal is the one value that can be rendered as a keying command.
pub fn admit(model: &StatusModel, facts: &Facts, start: TxStart) -> Result<Admitted, Refusal> {
    let kind = start.kind();
    if kind != StartKind::Key {
        return Err(Refusal::NoReadback(kind));
    }
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
    if sample.state != "READY" {
        return Err(Refusal::InterlockNotReady {
            state: sample.state.clone(),
        });
    }
    if !facts.readback_idle {
        return Err(Refusal::ReadbackNotIdle);
    }
    Ok(Admitted(start))
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
            readback_idle: true,
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
}
