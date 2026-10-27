//! Which [`Dialect`] a server speaks: learned from what it sends before `ready;`, then pinned for
//! the session's life.
//!
//! A server is never identified by its `protocol:` line alone: piHPSDR announces `ExpertSDR3,2.0`,
//! AetherSDR `ExpertSDR3,1.5`, and Thetis has emulated ExpertSDR3 by default since v2.10.3.14. So
//! each field is read from what the server does wherever its init burst shows it, and from the
//! protocol line only where nothing it sends before a key can:
//!
//! | Field | Decided by |
//! |---|---|
//! | `drive` | the arity of the server's first `drive` notice: `drive:<trx>,<v>;` is per receiver (TCI 2.0), `drive:<v>;` global (TCI 1.x). With none, the base's. |
//! | `trx_source` | the program on the protocol line. `ExpertSDR3`, the spec's server (and what piHPSDR, AetherSDR and Thetis's emulation announce), keys with a source; any other program does not, because nothing a server sends before a key shows whether its `trx` takes one, and a key whose source is dropped takes the SDR's microphone (TCI 2.0 §4.2). WSJT-X 3.0.2 decides the same way (`Transceiver/TCITransceiver.cpp:1084-1090`). The session refuses Nexus's audio on a dialect without a source (T1 choice 3). |
//! | `smeter` | the program: `ExpertSDR3` pushes `rx_channel_sensors` once asked (§4.5); any other is polled with `rx_smeter`, as WSJT-X does (`:469`, `:1278`). Meters arrive only after the session asks, so which ones did is recorded ([`Evidence`]), not used. |
//! | `chrono_length` | values, for every server: a TX_CHRONO has no payload to tell by. A stereo receive block shows which reading the server uses for its own audio ([`Block::reading`]); that is recorded as evidence for the bench, never acted on. |
//!
//! The operator's dialect setting ([`Preference`], the radio profile's `tci_dialect` in tempo-app)
//! can name the server instead of its protocol line: a server named `ExpertSDR3` or `piHPSDR` keys
//! with a source whatever its line says. `Thetis` is read from its own line either way: Thetis
//! announcing itself is [`THETIS_NATIVE`], and its ExpertSDR3 emulation is [`EXPERT_SDR3`].
//!
//! Pure: the session feeds it notices and blocks, and asks for the dialect at `ready;`.

use super::frame::{Block, BlockKind, Channels, LengthMeaning};
use super::wire::{Notice, Setting};
use super::{Dialect, DriveForm, SMeter, EXPERT_SDR2, EXPERT_SDR3, PIHPSDR, THETIS_NATIVE};

/// A server whose protocol line names a program this build does not know (or names none): the
/// spec's drive form until its first `drive` notice shows otherwise, its S-meter polled, and no
/// source on a key, so Nexus's audio is refused on it until a bench shows what its `trx` takes.
pub const UNKNOWN: Dialect = Dialect {
    name: "a TCI server this build does not know",
    drive: DriveForm::PerTrx,
    trx_source: false,
    smeter: SMeter::Poll,
    chrono_length: LengthMeaning::Values,
};

/// The operator's choice of server (the radio profile's `tci_dialect`): learned from the server
/// ([`Preference::Auto`]), or named.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Preference {
    #[default]
    Auto,
    /// ExpertSDR3, TCI 2.0: a key names its source whatever the protocol line says.
    ExpertSdr3,
    /// Thetis 2.10.3.14 and later: read from its own protocol line, as [`Preference::Auto`] does.
    Thetis,
    /// piHPSDR, which speaks the spec's dialect: as [`Preference::ExpertSdr3`].
    PiHpsdr,
}

/// What the server has shown about itself: what the dialect is pinned from, and what the bench
/// reads back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Evidence {
    /// The protocol line's program and version, as sent.
    pub program: Option<String>,
    pub version: Option<String>,
    /// The form of the first `drive` notice.
    pub drive: Option<DriveForm>,
    /// Which receive meters have arrived: `rx_channel_sensors`, `rx_sensors`, `rx_smeter`.
    pub rx_channel_sensors: bool,
    pub rx_sensors: bool,
    pub rx_smeter: bool,
    /// The reading of `length` the last stereo receive block fitted: the server's meaning for its
    /// own audio. Evidence about TX_CHRONO, not proof.
    pub stereo_reading: Option<LengthMeaning>,
}

impl Evidence {
    /// Learn from one notice.
    pub fn notice(&mut self, notice: &Notice) {
        match notice {
            Notice::Protocol { program, version } => {
                self.program = Some(program.as_str().to_string());
                self.version = Some(version.as_str().to_string());
            }
            Notice::State(Setting::Drive { trx, .. }) if self.drive.is_none() => {
                self.drive = Some(if trx.is_some() {
                    DriveForm::PerTrx
                } else {
                    DriveForm::Global
                });
            }
            Notice::RxChannelSensors { .. } => self.rx_channel_sensors = true,
            Notice::RxSensors { .. } => self.rx_sensors = true,
            Notice::RxSmeter { .. } => self.rx_smeter = true,
            _ => {}
        }
    }

    /// Learn from one receive block.
    pub fn block(&mut self, block: &Block<'_>) {
        if block.header.kind == BlockKind::RxAudio && block.header.channels == Channels::Two {
            if let Some(reading) = block.reading {
                self.stereo_reading = Some(reading);
            }
        }
    }

    /// The dialect the session speaks from `ready;` on.
    pub fn pin(&self, preference: Preference) -> Dialect {
        let base = match preference {
            Preference::ExpertSdr3 => EXPERT_SDR3,
            Preference::PiHpsdr => PIHPSDR,
            Preference::Auto | Preference::Thetis => self.by_program(),
        };
        Dialect {
            drive: self.drive.unwrap_or(base.drive),
            ..base
        }
    }

    /// The dialect the protocol line's program names.
    fn by_program(&self) -> Dialect {
        let Some(program) = self.program.as_deref() else {
            return UNKNOWN;
        };
        if program.eq_ignore_ascii_case("ExpertSDR3") {
            EXPERT_SDR3
        } else if program.eq_ignore_ascii_case("Thetis") {
            THETIS_NATIVE
        } else if program.eq_ignore_ascii_case("ExpertSDR2") || program.eq_ignore_ascii_case("esdr")
        {
            // `esdr,1.2` is WSJT-X's example of an ExpertSDR2 line (unverified).
            EXPERT_SDR2
        } else {
            UNKNOWN
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tci::frame::{encode_block, BlockHeader, SampleFormat};
    use crate::tci::wire::notices;
    use crate::tci::{Trx, AETHERSDR};

    fn learn(text: &str) -> Evidence {
        let mut e = Evidence::default();
        for n in notices(text) {
            e.notice(&n.expect("a notice"));
        }
        e
    }

    #[test]
    fn the_program_decides_the_source_and_the_meters_and_the_drive_its_arity() {
        // The spec's server, and the two that announce it.
        for line in [
            "protocol:ExpertSDR3,2.0;",
            "protocol:expertsdr3,1.5;",
            "PROTOCOL:EXPERTSDR3,2.0;",
        ] {
            let d = learn(line).pin(Preference::Auto);
            assert!(d.trx_source, "{line}");
            assert_eq!(d.smeter, SMeter::Channel, "{line}");
            assert_eq!(d.drive, DriveForm::PerTrx, "{line}");
        }
        // Thetis announcing itself: no source, the S-meter polled (WSJT-X's reading).
        let thetis = learn("protocol:Thetis,2.10.3.14;").pin(Preference::Auto);
        assert_eq!(thetis, THETIS_NATIVE);
        // TCI 1.x: the drive form follows the arity of its drive line.
        let esdr2 = learn("protocol:esdr,1.2;drive:40;").pin(Preference::Auto);
        assert_eq!(esdr2, EXPERT_SDR2);
        assert_eq!(esdr2.drive, DriveForm::Global);
        // An unknown program, or none: no source, polled, the spec's drive until a drive line.
        for text in ["protocol:MysterySDR,9.9;", "device:SunSDR2DX;", ""] {
            assert_eq!(learn(text).pin(Preference::Auto), UNKNOWN, "{text:?}");
        }
        let unknown_global = learn("protocol:MysterySDR,9.9;drive:20;").pin(Preference::Auto);
        assert_eq!(unknown_global.drive, DriveForm::Global);
        assert!(!unknown_global.trx_source);
    }

    #[test]
    fn behaviour_beats_the_name_for_the_drive_form() {
        // An ExpertSDR3 line whose server writes the 1.x drive: the drive form is what it sends.
        let d = learn("protocol:ExpertSDR3,2.0;drive:35;drive:0,50;").pin(Preference::Auto);
        assert_eq!(d.drive, DriveForm::Global, "the first drive line decides");
        assert!(d.trx_source);
    }

    #[test]
    fn a_named_server_keys_with_a_source_whatever_its_line_says() {
        let e = learn("protocol:MysterySDR,9.9;drive:0,10;");
        assert_eq!(e.pin(Preference::ExpertSdr3), EXPERT_SDR3);
        assert_eq!(e.pin(Preference::PiHpsdr), PIHPSDR);
        // Thetis is read from its own line, native or emulating.
        assert_eq!(e.pin(Preference::Thetis), UNKNOWN);
        assert_eq!(
            learn("protocol:Thetis,2.10;").pin(Preference::Thetis),
            THETIS_NATIVE
        );
        assert_eq!(
            learn("protocol:ExpertSDR3,2.0;").pin(Preference::Thetis),
            EXPERT_SDR3
        );
        // AetherSDR's line pins the spec's dialect: the two differ in name only.
        let aether = learn("protocol:ExpertSDR3,1.5;device:AetherSDR;").pin(Preference::Auto);
        assert_eq!(
            (
                aether.drive,
                aether.trx_source,
                aether.smeter,
                aether.chrono_length
            ),
            (
                AETHERSDR.drive,
                AETHERSDR.trx_source,
                AETHERSDR.smeter,
                AETHERSDR.chrono_length
            )
        );
    }

    #[test]
    fn meters_and_a_stereo_reading_are_recorded_and_never_change_the_pin() {
        let mut e = learn("protocol:ExpertSDR3,2.0;rx_smeter:0,0,-73.5;rx_sensors:0,-80.0;");
        assert!(e.rx_smeter && e.rx_sensors && !e.rx_channel_sensors);
        let header = BlockHeader {
            trx: Trx::new(0).expect("trx 0"),
            kind: BlockKind::RxAudio,
            sample_rate: 48_000,
            format: SampleFormat::Float32,
            channels: Channels::Two,
        };
        let bytes = encode_block(&header, LengthMeaning::Frames, &[0.0; 8]).expect("a block");
        let crate::tci::frame::Frame::Block(block) =
            crate::tci::frame::decode(&bytes).expect("decodes")
        else {
            panic!("a block");
        };
        e.block(&block);
        assert_eq!(e.stereo_reading, Some(LengthMeaning::Frames));
        let pinned = e.pin(Preference::Auto);
        assert_eq!(
            pinned.smeter,
            SMeter::Channel,
            "the program's, not what arrived"
        );
        assert_eq!(pinned.chrono_length, LengthMeaning::Values, "every server");
    }
}
