//! Admission: the server-side checks a TCI key must pass. They can only refuse.
//!
//! This is not Nexus's transmit gate. The engine decides whether a transmission may happen at all
//! (the TX-enable latch, the stop hold, licence privileges on the emitted RF, identity at the
//! keying boundary, the wall-clock watchdog), and every one of those holds before a key reaches
//! this module. Admission adds what only the server's own reports can say, and
//! every check is a reason to refuse:
//!
//! - the session has reached `ready;` and is not closing;
//! - nothing of ours is keyed already, as the session believes;
//! - the device is not receive-only, and the transmitter's `tx_enable` is not false;
//! - the SDR program is not stopped (`stop;`);
//! - the server reports no transmission of anyone else's (another program, the SDR's own
//!   operator): `trx` or `tune` true with no key of ours;
//! - the server's drive is at or below Nexus's ceiling, by readback: the last drive Nexus wrote
//!   ([`Facts::drive_ceiling`]). ExpertSDR restores a band's saved drive on a band change (§3.5),
//!   so after a restore the key is refused until Nexus writes its drive again and the server
//!   echoes it. A Tune checks `tune_drive` against the last tune drive Nexus wrote the same way.
//!   With no ceiling written, the drive is the SDR operator's and is not held to one; a report of
//!   it that could not be read still refuses, as every unreadable report admission reads does;
//! - and for a key that takes **Nexus's audio** ([`TxStart::Audio`], `trx:<trx>,true,tci;`), never
//!   a digital over on the SDR's microphone (TCI 2.0 §4.2: "otherwise the microphone
//!   signal is taken"): Nexus's audio is on for this radio; the dialect's `trx` takes a source
//!   (on a dialect whose `trx` takes none, a plain key's source is not known until a
//!   bench shows it, so Nexus's audio is refused there and a microphone key stays possible); the
//!   server has acknowledged `audio_start:<trx>;` on this connection; and a receive block for this
//!   transceiver arrived within the last second ([`RX_FRESH_MS`]). After the key, the session
//!   watches for the TX_CHRONO that proves the server is pulling Nexus's audio
//!   ([`super::session`]).
//!
//! A parameter the server reported in a form that could not be read is unknown, not unchanged
//! ([`super::model::Reading::Unreadable`]), and refuses as if it said no. A parameter it never
//! reported does not: the spec's servers report each one in their init burst, and an older server
//! that omits one is not refused for the omission.
//!
//! There is no list of benched kinds, and no per-radio Beta switch:
//! the operator's rule holds the release until a TCI bench has proven transmit in every mode, FT
//! and JS8 included, and the settings carry no such switch.
//!
//! [`Admitted`] is the value a key must carry to be rendered ([`Admitted::render`]). Its fields
//! are private to this module and [`admit`] is the only constructor, so the text of a key cannot
//! be produced anywhere else in the app; it is neither `Clone` nor `Copy`: one admission renders
//! one key. Stops are never admitted: [`super::wire::Stop::render`] is open to everyone.
//!
//! Nexus's own design, not a port.

use std::fmt;

use super::model::{Model, Reading};
use super::wire::{ClientText, Source, Start, Unrenderable};
use super::{Dialect, Trx};

/// How recent a receive block must be for a key that takes Nexus's audio.
pub const RX_FRESH_MS: u64 = 1_000;

/// What a key starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TxStart {
    /// An over carrying Nexus's audio (digital modes, the voice keyer, Remote's voice):
    /// `trx:<trx>,true,tci;`, its audio pulled by TX_CHRONO.
    Audio,
    /// An over on the SDR's own selected microphone (Phone at the shack): `trx:<trx>,true;`.
    Microphone,
    /// The tune carrier at the tune drive: `tune:<trx>,true;`.
    Tune,
}

/// What the session knows about itself, for admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    /// The transceiver this session keys.
    pub trx: Trx,
    /// The dialect pinned at `ready;`.
    pub dialect: Dialect,
    /// `ready;` has arrived and the session is not closing.
    pub ready: bool,
    /// A key of ours may be on the air: written and not yet proven ended.
    pub keyed: bool,
    /// Nexus's audio over TCI is on for this radio (the profile's `tci_audio`).
    pub audio: bool,
    /// A receive block for this transceiver arrived within [`RX_FRESH_MS`].
    pub rx_audio_fresh: bool,
    /// The last drive Nexus wrote, if any: the ceiling a key's drive readback is held to.
    pub drive_ceiling: Option<u32>,
    /// The last tune drive Nexus wrote, if any: the ceiling for a Tune.
    pub tune_drive_ceiling: Option<u32>,
}

/// Why a key was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No `ready;` yet, or the session is closing.
    NotReady,
    /// A key of ours may still be on the air.
    AlreadyKeyed,
    /// The device is receive-only (`receive_only:true;`), or its report could not be read.
    ReceiveOnly { unreadable: bool },
    /// The server does not allow this transmitter to transmit here (`tx_enable:<trx>,false;`), or
    /// its report could not be read.
    TxNotEnabled { unreadable: bool },
    /// The SDR program is stopped (`stop;`), or its run state could not be read.
    SdrStopped { unreadable: bool },
    /// The server reports a transmission that is not ours: another program's, or the SDR's own
    /// operator's. `tune`: it is the tune carrier.
    Transmitting { tune: bool },
    /// The server's report of whether it transmits could not be read.
    TransmitStateUnknown,
    /// The server's drive is above the last drive Nexus wrote (`tune`: the tune drive).
    DriveAboveCeiling {
        drive: u32,
        ceiling: u32,
        tune: bool,
    },
    /// The server's drive is not known: its report could not be read, or Nexus wrote a drive
    /// (`ceiling`) and the server has not reported one.
    DriveUnknown { ceiling: Option<u32>, tune: bool },
    /// Nexus's audio over TCI is off for this radio.
    AudioOff,
    /// The dialect's `trx` takes no source, so a key would take the SDR's microphone.
    NoSourceArgument { dialect: &'static str },
    /// The server has not acknowledged `audio_start:<trx>;` on this connection.
    AudioNotAcknowledged,
    /// No receive audio from the server within [`RX_FRESH_MS`].
    NoReceiveAudio,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let drive = |tune: bool| if tune { "tune drive" } else { "drive" };
        match self {
            Refusal::NotReady => f.write_str("the SDR program has not finished connecting"),
            Refusal::AlreadyKeyed => f.write_str("a transmission of ours may still be on the air"),
            Refusal::ReceiveOnly { unreadable: false } => {
                f.write_str("the SDR program reports a receive-only device")
            }
            Refusal::ReceiveOnly { unreadable: true } => {
                f.write_str("the SDR program's receive-only report could not be read")
            }
            Refusal::TxNotEnabled { unreadable: false } => {
                f.write_str("the SDR program does not allow transmitting here")
            }
            Refusal::TxNotEnabled { unreadable: true } => {
                f.write_str("the SDR program's transmit permission could not be read")
            }
            Refusal::SdrStopped { unreadable: false } => f.write_str("the SDR program is stopped"),
            Refusal::SdrStopped { unreadable: true } => {
                f.write_str("whether the SDR program runs could not be read")
            }
            Refusal::Transmitting { tune: false } => {
                f.write_str("the SDR program is transmitting for someone else")
            }
            Refusal::Transmitting { tune: true } => {
                f.write_str("the SDR program's tune carrier is up for someone else")
            }
            Refusal::TransmitStateUnknown => {
                f.write_str("whether the SDR program is transmitting could not be read")
            }
            Refusal::DriveAboveCeiling {
                drive: level,
                ceiling,
                tune,
            } => write!(
                f,
                "the SDR program's {} is {level}, above Nexus's {ceiling}",
                drive(*tune)
            ),
            Refusal::DriveUnknown {
                ceiling: Some(ceiling),
                tune,
            } => write!(
                f,
                "the SDR program's {} is not known (Nexus set {ceiling})",
                drive(*tune)
            ),
            Refusal::DriveUnknown {
                ceiling: None,
                tune,
            } => write!(f, "the SDR program's {} could not be read", drive(*tune)),
            Refusal::AudioOff => f.write_str("Nexus's audio over TCI is off for this radio"),
            Refusal::NoSourceArgument { dialect } => write!(
                f,
                "{dialect} cannot be told to take Nexus's audio, so it would transmit its microphone"
            ),
            Refusal::AudioNotAcknowledged => {
                f.write_str("the SDR program has not started Nexus's audio stream")
            }
            Refusal::NoReceiveAudio => {
                f.write_str("no audio has come from the SDR program in the last second")
            }
        }
    }
}

impl std::error::Error for Refusal {}

/// A key that passed admission. Constructed only by [`admit`]; neither `Clone` nor `Copy`.
#[derive(Debug, PartialEq, Eq)]
pub struct Admitted {
    trx: Trx,
    start: TxStart,
}

impl Admitted {
    pub fn start(&self) -> TxStart {
        self.start
    }

    /// The key's wire text for `dialect`. Consumes the admission.
    pub fn render(self, dialect: &Dialect) -> Result<ClientText, Unrenderable> {
        let start = match self.start {
            TxStart::Audio => Start::Trx {
                trx: self.trx,
                source: Some(Source::Tci),
            },
            TxStart::Microphone => Start::Trx {
                trx: self.trx,
                source: None,
            },
            TxStart::Tune => Start::Tune(self.trx),
        };
        start.render(dialect)
    }
}

/// Run the checks for `start`. Pure: no state changes, no I/O, and the only result other than a
/// refusal is the one value that can be rendered as a key.
pub fn admit(model: &Model, facts: &Facts, start: TxStart) -> Result<Admitted, Refusal> {
    if !facts.ready {
        return Err(Refusal::NotReady);
    }
    if facts.keyed {
        return Err(Refusal::AlreadyKeyed);
    }
    match model.receive_only {
        Reading::Known(true) => return Err(Refusal::ReceiveOnly { unreadable: false }),
        Reading::Unreadable => return Err(Refusal::ReceiveOnly { unreadable: true }),
        Reading::Known(false) | Reading::NotReported => {}
    }
    let trx = model.trx(facts.trx);
    match trx.tx_enable {
        Reading::Known(false) => return Err(Refusal::TxNotEnabled { unreadable: false }),
        Reading::Unreadable => return Err(Refusal::TxNotEnabled { unreadable: true }),
        Reading::Known(true) | Reading::NotReported => {}
    }
    match model.running {
        Reading::Known(false) => return Err(Refusal::SdrStopped { unreadable: false }),
        Reading::Unreadable => return Err(Refusal::SdrStopped { unreadable: true }),
        Reading::Known(true) | Reading::NotReported => {}
    }
    if trx.transmitting.is_unreadable() || trx.tuning.is_unreadable() {
        return Err(Refusal::TransmitStateUnknown);
    }
    if trx.transmitting.is_true() {
        return Err(Refusal::Transmitting { tune: false });
    }
    if trx.tuning.is_true() {
        return Err(Refusal::Transmitting { tune: true });
    }
    let tune = start == TxStart::Tune;
    let (ceiling, level) = if tune {
        (facts.tune_drive_ceiling, &trx.tune_drive)
    } else {
        (facts.drive_ceiling, model.drive(facts.trx))
    };
    match (level, ceiling) {
        // Unknown is never trusted, ceiling or not; never reported matters only once Nexus has
        // written a drive the readback must match.
        (Reading::Unreadable, _) | (Reading::NotReported, Some(_)) => {
            return Err(Refusal::DriveUnknown { ceiling, tune })
        }
        (Reading::Known(drive), Some(ceiling)) if *drive > ceiling => {
            return Err(Refusal::DriveAboveCeiling {
                drive: *drive,
                ceiling,
                tune,
            })
        }
        (Reading::Known(_), _) | (Reading::NotReported, None) => {}
    }
    if start == TxStart::Audio {
        if !facts.audio {
            return Err(Refusal::AudioOff);
        }
        if !facts.dialect.trx_source {
            return Err(Refusal::NoSourceArgument {
                dialect: facts.dialect.name,
            });
        }
        if !trx.audio.is_true() {
            return Err(Refusal::AudioNotAcknowledged);
        }
        if !facts.rx_audio_fresh {
            return Err(Refusal::NoReceiveAudio);
        }
    }
    Ok(Admitted {
        trx: facts.trx,
        start,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tci::wire::{split, Notice};
    use crate::tci::{EXPERT_SDR2, EXPERT_SDR3, THETIS_NATIVE};

    fn trx(n: u32) -> Trx {
        Trx::new(n).expect("a receiver number")
    }

    fn model(text: &str) -> Model {
        let mut m = Model::new();
        for raw in split(text) {
            match raw {
                Ok(raw) => match Notice::parse(&raw) {
                    Ok(n) => m.apply(&n),
                    Err(e) => m.refused(e.command().expect("a named command"), Some(raw.args())),
                },
                Err(e) => m.refused(e.command().expect("a named command"), None),
            }
        }
        m
    }

    /// A server ready to transmit for receiver 0 with Nexus's audio running.
    const READY: &str = "receive_only:false;start;tx_enable:0,true;trx:0,false;tune:0,false;\
                         drive:0,30;tune_drive:0,10;audio_start:0;ready;";

    fn facts() -> Facts {
        Facts {
            trx: trx(0),
            dialect: EXPERT_SDR3,
            ready: true,
            keyed: false,
            audio: true,
            rx_audio_fresh: true,
            drive_ceiling: Some(30),
            tune_drive_ceiling: Some(10),
        }
    }

    fn rendered(m: &Model, f: &Facts, start: TxStart) -> Result<String, Refusal> {
        admit(m, f, start).map(|a| {
            a.render(&f.dialect)
                .expect("an admitted key renders")
                .into_string()
        })
    }

    #[test]
    fn a_ready_server_admits_each_kind_and_renders_its_key() {
        let m = model(READY);
        let f = facts();
        assert_eq!(
            rendered(&m, &f, TxStart::Audio).as_deref(),
            Ok("trx:0,true,tci;")
        );
        assert_eq!(
            rendered(&m, &f, TxStart::Microphone).as_deref(),
            Ok("trx:0,true;")
        );
        assert_eq!(
            rendered(&m, &f, TxStart::Tune).as_deref(),
            Ok("tune:0,true;")
        );
        // Another receiver's key names that receiver.
        let m1 = model(&READY.replace(":0", ":1"));
        let f1 = Facts { trx: trx(1), ..f };
        assert_eq!(
            rendered(&m1, &f1, TxStart::Audio).as_deref(),
            Ok("trx:1,true,tci;")
        );
    }

    #[test]
    fn every_refusal_names_its_cause() {
        let f = facts();
        let case = |text: &str, facts: Facts, start: TxStart| rendered(&model(text), &facts, start);
        assert_eq!(
            case(READY, Facts { ready: false, ..f }, TxStart::Microphone),
            Err(Refusal::NotReady)
        );
        assert_eq!(
            case(READY, Facts { keyed: true, ..f }, TxStart::Tune),
            Err(Refusal::AlreadyKeyed)
        );
        // Receive-only and tx_enable, said or unreadable.
        for (text, refusal) in [
            (
                "receive_only:true;",
                Refusal::ReceiveOnly { unreadable: false },
            ),
            (
                "receive_only:maybe;",
                Refusal::ReceiveOnly { unreadable: true },
            ),
            (
                "tx_enable:0,false;",
                Refusal::TxNotEnabled { unreadable: false },
            ),
            (
                "tx_enable:0,maybe;",
                Refusal::TxNotEnabled { unreadable: true },
            ),
            ("stop;", Refusal::SdrStopped { unreadable: false }),
            ("stop:now;", Refusal::SdrStopped { unreadable: true }),
            ("trx:0,true;", Refusal::Transmitting { tune: false }),
            ("tune:0,true;", Refusal::Transmitting { tune: true }),
            ("trx:0,maybe;", Refusal::TransmitStateUnknown),
            ("tune:0,maybe;", Refusal::TransmitStateUnknown),
        ] {
            for start in [TxStart::Audio, TxStart::Microphone, TxStart::Tune] {
                assert_eq!(
                    case(&format!("{READY}{text}"), f, start),
                    Err(refusal.clone()),
                    "{text} {start:?}"
                );
            }
        }
        // The drive by readback, against the last drive Nexus wrote.
        assert_eq!(
            case(&format!("{READY}drive:0,100;"), f, TxStart::Audio),
            Err(Refusal::DriveAboveCeiling {
                drive: 100,
                ceiling: 30,
                tune: false
            })
        );
        assert_eq!(
            case(&format!("{READY}drive:0,x;"), f, TxStart::Microphone),
            Err(Refusal::DriveUnknown {
                ceiling: Some(30),
                tune: false
            })
        );
        assert_eq!(
            case(&format!("{READY}tune_drive:0,50;"), f, TxStart::Tune),
            Err(Refusal::DriveAboveCeiling {
                drive: 50,
                ceiling: 10,
                tune: true
            })
        );
        // The tune drive does not hold an over back, nor the drive a Tune.
        assert!(case(&format!("{READY}tune_drive:0,50;"), f, TxStart::Audio).is_ok());
        assert!(case(&format!("{READY}drive:0,100;"), f, TxStart::Tune).is_ok());
        // Item 3: never a digital over on the SDR's microphone. Each term alone refuses Audio,
        // and none of them refuses a microphone key or a Tune.
        let unacknowledged = READY.replace("audio_start:0;", "");
        let stopped = format!("{READY}audio_stop:0;");
        let unreadable = format!("{READY}audio_start:x;");
        for (text, facts, refusal) in [
            (READY, Facts { audio: false, ..f }, Refusal::AudioOff),
            (
                READY,
                Facts {
                    dialect: EXPERT_SDR2,
                    ..f
                },
                Refusal::NoSourceArgument {
                    dialect: EXPERT_SDR2.name,
                },
            ),
            (
                READY,
                Facts {
                    dialect: THETIS_NATIVE,
                    ..f
                },
                Refusal::NoSourceArgument {
                    dialect: THETIS_NATIVE.name,
                },
            ),
            (unacknowledged.as_str(), f, Refusal::AudioNotAcknowledged),
            (stopped.as_str(), f, Refusal::AudioNotAcknowledged),
            (unreadable.as_str(), f, Refusal::AudioNotAcknowledged),
            (
                READY,
                Facts {
                    rx_audio_fresh: false,
                    ..f
                },
                Refusal::NoReceiveAudio,
            ),
        ] {
            assert_eq!(
                case(text, facts, TxStart::Audio),
                Err(refusal.clone()),
                "{refusal:?}"
            );
            assert!(
                case(text, facts, TxStart::Microphone).is_ok(),
                "{refusal:?}"
            );
            assert!(case(text, facts, TxStart::Tune).is_ok(), "{refusal:?}");
        }
    }

    #[test]
    fn what_was_never_reported_does_not_refuse_but_a_written_drive_needs_its_readback() {
        // An older server that reports nothing but `ready;`.
        let bare = model("audio_start:0;ready;");
        let f = Facts {
            drive_ceiling: None,
            tune_drive_ceiling: None,
            ..facts()
        };
        for start in [TxStart::Audio, TxStart::Microphone, TxStart::Tune] {
            assert!(admit(&bare, &f, start).is_ok(), "{start:?}");
        }
        // Once Nexus has written a drive, a key needs the server's word on it.
        assert_eq!(
            admit(&bare, &facts(), TxStart::Microphone).err(),
            Some(Refusal::DriveUnknown {
                ceiling: Some(30),
                tune: false
            })
        );
        // The 1.x drive form counts as the receiver's drive.
        let global = model("audio_start:0;drive:25;ready;");
        assert!(admit(
            &global,
            &Facts {
                tune_drive_ceiling: None,
                ..facts()
            },
            TxStart::Audio
        )
        .is_ok());
    }

    #[test]
    fn every_refusal_reads_as_a_sentence() {
        for r in [
            Refusal::NotReady,
            Refusal::AlreadyKeyed,
            Refusal::ReceiveOnly { unreadable: false },
            Refusal::ReceiveOnly { unreadable: true },
            Refusal::TxNotEnabled { unreadable: false },
            Refusal::TxNotEnabled { unreadable: true },
            Refusal::SdrStopped { unreadable: false },
            Refusal::SdrStopped { unreadable: true },
            Refusal::Transmitting { tune: false },
            Refusal::Transmitting { tune: true },
            Refusal::TransmitStateUnknown,
            Refusal::DriveAboveCeiling {
                drive: 100,
                ceiling: 30,
                tune: false,
            },
            Refusal::DriveUnknown {
                ceiling: Some(30),
                tune: true,
            },
            Refusal::DriveUnknown {
                ceiling: None,
                tune: false,
            },
            Refusal::AudioOff,
            Refusal::NoSourceArgument {
                dialect: EXPERT_SDR2.name,
            },
            Refusal::AudioNotAcknowledged,
            Refusal::NoReceiveAudio,
        ] {
            let text = r.to_string();
            assert!(!text.is_empty() && !text.ends_with('.'), "{r:?}: {text}");
        }
    }

    #[test]
    fn a_global_drive_after_a_receivers_own_is_held_to_the_ceiling() {
        // READY reports `drive:0,30;`; a 1.x `drive:100;` after it sets the transmitter to 100.
        let m = model(&format!("{READY}drive:100;"));
        for start in [TxStart::Audio, TxStart::Microphone] {
            assert_eq!(
                admit(&m, &facts(), start).err(),
                Some(Refusal::DriveAboveCeiling {
                    drive: 100,
                    ceiling: 30,
                    tune: false
                }),
                "{start:?}"
            );
        }
    }

    #[test]
    fn an_unreadable_global_drive_after_a_receivers_own_refuses_the_key() {
        // The review's scenario (2026-10-20): `drive:0,30;` then a 1.x `drive:x;` that cannot be
        // read. The stale 30 must not pass the ceiling.
        let m = model(&format!("{READY}drive:x;"));
        for start in [TxStart::Audio, TxStart::Microphone] {
            assert_eq!(
                admit(&m, &facts(), start).err(),
                Some(Refusal::DriveUnknown {
                    ceiling: Some(30),
                    tune: false
                }),
                "{start:?}"
            );
        }
    }

    #[test]
    fn an_unreadable_drive_refuses_a_key_even_with_no_ceiling() {
        // A report of the drive that could not be read refuses, as any unreadable report admission
        // reads does, whether or not Nexus has written a drive of its own.
        let no_ceiling = Facts {
            drive_ceiling: None,
            tune_drive_ceiling: None,
            ..facts()
        };
        for (text, start) in [
            ("drive:0,x;", TxStart::Audio),
            ("drive:x;", TxStart::Microphone),
            ("tune_drive:0,x;", TxStart::Tune),
        ] {
            let m = model(&format!("{READY}{text}"));
            assert_eq!(
                admit(&m, &no_ceiling, start).err(),
                Some(Refusal::DriveUnknown {
                    ceiling: None,
                    tune: start == TxStart::Tune
                }),
                "{text} {start:?}"
            );
        }
    }
}
