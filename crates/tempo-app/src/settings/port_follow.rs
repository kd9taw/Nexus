//! A saved serial port that Windows renumbered, found again by what the device is.
//!
//! # The problem
//!
//! Every port setting is saved as a bare name (`COM5`). Windows hands a USB serial device a new
//! number when it moves to another USB socket or hub, when its driver is reinstalled, and now and
//! then after an update's restart, so a station that worked yesterday can open on a port that no
//! longer exists. The radio loop already follows a moved CAT port within a session, from a memory
//! of the port's USB identity that dies with the process; an update is a restart, so that memory
//! is gone exactly when the renumbering happens.
//!
//! # What this does
//!
//! Beside the names, [`Settings::port_identities`] keeps what each saved port was when Nexus last
//! saw it ([`PortIdentity`]: VID, PID, USB serial number, product, and the interface number that
//! tells a CP2105's Enhanced port from its Standard one). At launch, before the radio loop or any
//! keyer opens anything, [`Settings::follow_moved_ports`] looks at every port setting:
//!
//! * **The saved port is here:** nothing moves, whatever else is plugged in. "The serial port you
//!   choose is the one Nexus opens" (1.19.0): following never overrides a port that exists. Its
//!   identity is refreshed from the device now at that name.
//! * **It is gone, and exactly one present port is that device** by its own serial number, and
//!   by interface number on a multi-port chip ([`crate::serial_port::refind`]): every setting naming the old port is
//!   moved to the new name, and the operator is told in words.
//! * **It is gone, and the match is ambiguous or absent:** nothing is guessed. The words say the
//!   port is not on this computer and list the ports that are.
//!
//! # Transmit safety
//!
//! A PTT, CW-key or FSK port is a keying line: opening one raises its control lines. This module
//! opens nothing; it only rewrites names, and only at launch (and the identity capture at a
//! Settings save, which only reads the enumeration). The keying code opens the new name the same
//! way it would have opened the old one, with every hold-low, watchdog and licence gate in place.
//! A follow onto a name another port setting already uses is refused, so two settings never end
//! up on one port because of this.

use super::Settings;
use crate::serial_port::{refind, same_port, PortIdentity, PresentPort, Refound};

/// What a launch's look at the port settings found ([`Settings::follow_moved_ports`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PortFollow {
    /// The settings changed (a port followed, or an identity remembered or forgotten): save them.
    pub changed: bool,
    /// One sentence per port followed, for the operator.
    pub followed: Vec<String>,
    /// One sentence per saved port that is not here and was not followed, for the log.
    pub missing: Vec<String>,
    /// The active radio's CAT port, when it is one of `missing`: its sentence, for the CAT status
    /// when the launch's connect fails.
    pub cat_missing: Option<String>,
}

/// `Settings::port_identities`' reader, which never fails the file. The map is a cache Nexus
/// rebuilds from the USB enumeration at every launch, and a settings file that does not parse is
/// set aside whole and replaced by defaults (`Settings::load`): a damaged map (a hand edit, a
/// half-written entry, a shape from another build) must cost only itself. Each entry that is not
/// an identity is dropped, and a value that is not a map at all reads as empty, with one log
/// line saying so; a field an entry does not know is ignored, as everywhere in the settings.
pub(crate) fn tolerant_identities<'de, D>(
    d: D,
) -> Result<std::collections::BTreeMap<String, PortIdentity>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let raw = serde_json::Value::deserialize(d)?;
    let mut out = std::collections::BTreeMap::new();
    let mut dropped = 0usize;
    match raw {
        serde_json::Value::Object(entries) => {
            for (port, v) in entries {
                match serde_json::from_value::<PortIdentity>(v) {
                    Ok(id) if !names_no_device(&id) => {
                        out.insert(port, id);
                    }
                    _ => dropped += 1,
                }
            }
        }
        serde_json::Value::Null => {}
        _ => dropped += 1,
    }
    if dropped > 0 {
        let said = format!(
            "forgot {dropped} unreadable serial-port identit{} in the settings; Nexus learns them \
             again from the ports plugged in",
            if dropped == 1 { "y" } else { "ies" }
        );
        eprintln!("tempo: {said}");
        tempo_core::applog::warn("settings_forgiven", &said);
    }
    Ok(out)
}

/// An entry that parsed only because every field defaults (`null`, `{}`) names no device.
fn names_no_device(id: &PortIdentity) -> bool {
    *id == PortIdentity::default()
}

/// Which setting a port belongs to, in the order a shared port is named by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PortRole {
    Cat,
    Ptt,
    CwKey,
    WinKeyer,
    Fsk,
    Rotator,
    Amp,
}

impl PortRole {
    fn noun(self) -> &'static str {
        match self {
            PortRole::Cat => "Your radio",
            PortRole::Ptt => "Your PTT cable",
            PortRole::CwKey => "Your CW key cable",
            PortRole::WinKeyer => "Your WinKeyer",
            PortRole::Fsk => "Your FSK keying cable",
            PortRole::Rotator => "Your rotator",
            PortRole::Amp => "Your amplifier",
        }
    }
}

/// "COM5 isn't on this computer now. Ports here: COM3, COM7." The words for a saved port that is
/// gone, with the ports that are here, so the operator can pick theirs. `could_be` are the ports
/// that look like it when Nexus would not guess between them.
pub fn not_here_words(port: &str, present: &[PresentPort], could_be: &[String]) -> String {
    let here = if present.is_empty() {
        "none".to_string()
    } else {
        present
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut words = format!("{port} isn't on this computer now. Ports here: {here}.");
    if could_be.len() > 1 {
        words.push_str(&format!(
            " More than one of them could be it ({}), so Nexus didn't guess: choose yours in \
             Settings.",
            could_be.join(", ")
        ));
    }
    words
}

/// The launch's CAT status with the port findings in front: the ports followed, always (the
/// operator must learn the name changed), and the CAT port's "isn't on this computer" words only
/// when the connect failed (`ok == Some(false)`), since a port that is gone is why.
pub fn launch_cat_detail(notes: &PortFollow, ok: Option<bool>, detail: &str) -> String {
    let mut parts: Vec<&str> = notes.followed.iter().map(String::as_str).collect();
    if ok == Some(false) {
        parts.extend(notes.cat_missing.as_deref());
    }
    parts.push(detail);
    parts
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Test CAT's words when the active radio's CAT serial port is not on this computer: `None` while
/// it is here (or `exists` holds its typed path), for a radio that does not reach CAT over a
/// serial port, and when the enumeration says nothing.
pub fn cat_port_not_here(
    s: &Settings,
    present: &[PresentPort],
    exists: impl Fn(&str) -> bool,
) -> Option<String> {
    let port = s.serial_port.trim();
    let serial_cat = !crate::settings::rig_conn_is_network(&s.rig_conn, &s.rig_addr)
        && !crate::settings::rig_conn_is_omnirig(&s.rig_conn)
        && !crate::settings::rig_conn_is_icom_lan(&s.rig_conn)
        && !crate::settings::rig_conn_is_tci(&s.rig_conn);
    if present.is_empty()
        || !serial_cat
        || !crate::settings::looks_like_serial_port(port)
        || present.iter().any(|p| same_port(&p.name, port))
        || exists(port)
    {
        return None;
    }
    let could_be = match s.identity_of(port).map(|id| refind(id, present)) {
        Some(Refound::Ambiguous(c)) => c,
        _ => Vec::new(),
    };
    Some(not_here_words(port, present, &could_be))
}

impl Settings {
    /// Every port setting, flat and per radio, with the role it plays.
    fn port_fields(&mut self) -> Vec<(PortRole, &mut String)> {
        let mut out: Vec<(PortRole, &mut String)> = vec![
            (PortRole::Cat, &mut self.serial_port),
            (PortRole::Ptt, &mut self.ptt_serial_port),
            (PortRole::CwKey, &mut self.cw_key_port),
            (PortRole::WinKeyer, &mut self.winkeyer_port),
            (PortRole::Fsk, &mut self.rtty_fsk_port),
            (PortRole::Rotator, &mut self.rotator_port),
            (PortRole::Amp, &mut self.amp_port),
        ];
        for r in &mut self.radios {
            out.push((PortRole::Cat, &mut r.serial_port));
            out.push((PortRole::Ptt, &mut r.ptt_serial_port));
            out.push((PortRole::CwKey, &mut r.cw_key_port));
            out.push((PortRole::WinKeyer, &mut r.winkeyer_port));
            out.push((PortRole::Fsk, &mut r.rtty_fsk_port));
            out.push((PortRole::Rotator, &mut r.rotator_port));
            out.push((PortRole::Amp, &mut r.amp_port));
        }
        out
    }

    /// The distinct port names the settings use (one spelling each, the first met), with the
    /// first role that names each.
    fn named_ports(&mut self) -> Vec<(String, PortRole)> {
        let mut names: Vec<(String, PortRole)> = Vec::new();
        for (role, field) in self.port_fields() {
            let name = field.trim();
            if name.is_empty() {
                continue;
            }
            match names.iter_mut().find(|(n, _)| same_port(n, name)) {
                Some((_, r)) => *r = (*r).min(role),
                None => names.push((name.to_string(), role)),
            }
        }
        names
    }

    /// The identity saved for `port`, however its name is spelled.
    fn identity_of(&self, port: &str) -> Option<&PortIdentity> {
        self.port_identities
            .iter()
            .find(|(k, _)| same_port(k, port))
            .map(|(_, id)| id)
    }

    /// Remember the identity of every port setting whose port is here now, and forget the ones no
    /// setting names any more. Reads `present` only; opens nothing. Returns whether the map
    /// changed. An empty `present` (no enumeration, or it failed) says nothing and changes
    /// nothing.
    pub fn remember_present_ports(&mut self, present: &[PresentPort]) -> bool {
        if present.is_empty() {
            return false;
        }
        let names = self.named_ports();
        let before = self.port_identities.clone();
        self.port_identities
            .retain(|k, _| names.iter().any(|(n, _)| same_port(n, k)));
        for (name, _) in &names {
            let Some(id) = present
                .iter()
                .find(|p| same_port(&p.name, name))
                .and_then(|p| p.identity.clone())
            else {
                continue;
            };
            self.port_identities.retain(|k, _| !same_port(k, name));
            self.port_identities.insert(name.clone(), id);
        }
        self.port_identities != before
    }

    /// The launch's look at the port settings (this module's header): follow a saved port that is
    /// gone to the one present port that is unambiguously its device, say so, and otherwise
    /// guess nothing. `exists` is the filesystem's answer for a typed path the enumeration cannot
    /// list (a virtual port another program makes); a port it holds is here. An empty `present`
    /// says nothing and changes nothing.
    pub fn follow_moved_ports(
        &mut self,
        present: &[PresentPort],
        exists: impl Fn(&str) -> bool,
    ) -> PortFollow {
        let mut out = PortFollow::default();
        if present.is_empty() {
            return out;
        }
        let names = self.named_ports();
        let cat = self.serial_port.trim().to_string();
        let mut moves: Vec<(String, String)> = Vec::new();
        for (name, role) in &names {
            if present.iter().any(|p| same_port(&p.name, name)) || exists(name) {
                continue;
            }
            let found = self
                .identity_of(name)
                .map_or(Refound::NotFound, |id| refind(id, present));
            let target = match &found {
                Refound::At(new)
                    if !names.iter().any(|(n, _)| same_port(n, new))
                        && !moves.iter().any(|(_, t)| same_port(t, new)) =>
                {
                    Some(new.clone())
                }
                _ => None,
            };
            if let Some(new) = target {
                out.followed.push(format!(
                    "{} moved from {name} to {new}; Nexus followed it.",
                    role.noun()
                ));
                moves.push((name.clone(), new));
                continue;
            }
            if !crate::settings::looks_like_serial_port(name) {
                continue;
            }
            let could_be = match found {
                Refound::Ambiguous(c) => c,
                _ => Vec::new(),
            };
            let words = not_here_words(name, present, &could_be);
            if same_port(name, &cat) {
                out.cat_missing = Some(words.clone());
            }
            out.missing.push(words);
        }
        for (old, new) in &moves {
            for (_, field) in self.port_fields() {
                if same_port(field, old) {
                    *field = new.clone();
                }
            }
        }
        out.changed = !moves.is_empty();
        out.changed |= self.remember_present_ports(present);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usb(name: &str, vid: u16, pid: u16, serial: &str, iface: Option<u8>) -> PresentPort {
        PresentPort {
            name: name.to_string(),
            identity: Some(PortIdentity {
                vid,
                pid,
                serial: serial.to_string(),
                product: String::new(),
                interface: iface,
            }),
        }
    }

    fn plain(name: &str) -> PresentPort {
        PresentPort {
            name: name.to_string(),
            identity: None,
        }
    }

    /// The FT-891's CP2105: one serial number, two interfaces (Enhanced = 0, Standard = 1).
    fn cp2105(enhanced: &str, standard: &str) -> Vec<PresentPort> {
        vec![
            usb(enhanced, 0x10C4, 0xEA70, "00A5B1C2", Some(0)),
            usb(standard, 0x10C4, 0xEA70, "00A5B1C2", Some(1)),
        ]
    }

    fn nothing(_: &str) -> bool {
        false
    }

    /// A station with CAT on the CP2105's Enhanced port and PTT on its Standard port.
    fn ft891_station() -> Settings {
        let mut s = Settings {
            serial_port: "COM5".into(),
            ptt_serial_port: "COM6".into(),
            ..Settings::default()
        };
        s.ensure_radio_profiles();
        s.sync_active_from_flat();
        s
    }

    /// ⭐ The reported bug: the update's restart came back with the radio renumbered. With the
    /// identity remembered from the last launch, both halves of the CP2105 are followed to the
    /// RIGHT new port (Enhanced stays CAT, Standard stays PTT), every copy of the setting moves,
    /// and the operator is told in words.
    #[test]
    fn a_renumbered_dual_uart_radio_is_followed_port_for_port() {
        let mut s = ft891_station();
        assert!(s.remember_present_ports(&cp2105("COM5", "COM6")));
        let after_update = cp2105("COM7", "COM8");
        let got = s.follow_moved_ports(&after_update, nothing);
        assert!(got.changed);
        assert_eq!(s.serial_port, "COM7");
        assert_eq!(s.ptt_serial_port, "COM8");
        assert_eq!(s.radios[0].serial_port, "COM7");
        assert_eq!(s.radios[0].ptt_serial_port, "COM8");
        assert_eq!(
            got.followed,
            vec![
                "Your radio moved from COM5 to COM7; Nexus followed it.".to_string(),
                "Your PTT cable moved from COM6 to COM8; Nexus followed it.".to_string(),
            ]
        );
        assert_eq!(got.cat_missing, None);
        // The identities moved with the names, so the next renumbering is followed too.
        assert!(s
            .identity_of("COM7")
            .is_some_and(|id| id.interface == Some(0)));
        assert!(s.identity_of("COM5").is_none());
    }

    /// 1.19.0: the serial port you choose is the one Nexus opens. A saved port that EXISTS is
    /// never followed, even when the remembered device is plainly somewhere else now.
    #[test]
    fn a_saved_port_that_exists_is_never_followed() {
        let mut s = ft891_station();
        s.remember_present_ports(&cp2105("COM5", "COM6"));
        let mut here = cp2105("COM7", "COM8");
        here.push(plain("COM5"));
        here.push(plain("COM6"));
        let got = s.follow_moved_ports(&here, nothing);
        assert_eq!(
            (s.serial_port.as_str(), s.ptt_serial_port.as_str()),
            ("COM5", "COM6")
        );
        assert!(got.followed.is_empty() && got.missing.is_empty());
    }

    /// A typed path the enumeration cannot list (a virtual port) is here when the filesystem
    /// has it: it is not followed anywhere.
    #[test]
    fn a_typed_path_that_exists_is_here() {
        let mut s = Settings {
            serial_port: "/dev/ttyUSB0".into(),
            ..Settings::default()
        };
        s.port_identities.insert(
            "/dev/ttyUSB0".into(),
            PortIdentity {
                vid: 0x0403,
                pid: 0x6001,
                serial: "A702TB52".into(),
                ..PortIdentity::default()
            },
        );
        let here = vec![usb("/dev/ttyUSB1", 0x0403, 0x6001, "A702TB52", None)];
        let got = s.follow_moved_ports(&here, |p| p == "/dev/ttyUSB0");
        assert_eq!(s.serial_port, "/dev/ttyUSB0");
        assert!(got.followed.is_empty());
    }

    /// Two identical cables with no serial number and no interface number: VID/PID alone is
    /// never enough. Nexus says the port is gone, lists what is here, and does not guess.
    #[test]
    fn an_ambiguous_match_is_not_guessed() {
        let mut s = Settings {
            serial_port: "COM5".into(),
            ..Settings::default()
        };
        s.remember_present_ports(&[usb("COM5", 0x1A86, 0x7523, "", None)]);
        let here = vec![
            usb("COM7", 0x1A86, 0x7523, "", None),
            usb("COM9", 0x1A86, 0x7523, "", None),
        ];
        let got = s.follow_moved_ports(&here, nothing);
        assert_eq!(s.serial_port, "COM5");
        assert!(got.followed.is_empty());
        assert_eq!(
            got.cat_missing.as_deref(),
            Some(
                "COM5 isn't on this computer now. Ports here: COM7, COM9. More than one of them \
                 could be it (COM7, COM9), so Nexus didn't guess: choose yours in Settings."
            )
        );
        // One identical cable is no better: no serial and no interface is too thin to follow on.
        let got = s.follow_moved_ports(&[usb("COM7", 0x1A86, 0x7523, "", None)], nothing);
        assert_eq!(s.serial_port, "COM5");
        assert!(got.followed.is_empty() && got.cat_missing.is_some());
    }

    /// Windows' made-up stand-in for a missing serial number (one digit from the instance id) is
    /// not a serial number: it is shared by unrelated devices and changes with the socket.
    #[test]
    fn windows_stand_in_serial_is_not_a_serial() {
        let mut s = Settings {
            serial_port: "COM5".into(),
            ..Settings::default()
        };
        s.remember_present_ports(&[usb("COM5", 0x1A86, 0x7523, "5", None)]);
        let got = s.follow_moved_ports(&[usb("COM7", 0x1A86, 0x7523, "5", None)], nothing);
        assert_eq!(s.serial_port, "COM5");
        assert!(got.followed.is_empty());
    }

    /// ⭐ Linux gives EVERY single-interface USB serial interface number 0, so an interface
    /// number without a serial number is VID/PID alone: a no-serial CH340 radio cable must never
    /// be followed onto another CH340 cable. The same for a factory-default serial number a whole
    /// production run shares ("0001" on generic CP2102 cables): it is not a serial number.
    #[test]
    fn an_interface_or_a_factory_serial_alone_is_not_a_key() {
        for serial in ["", "0001", "00000001", "0000"] {
            let mut s = Settings {
                serial_port: "/dev/ttyUSB0".into(),
                ..Settings::default()
            };
            s.remember_present_ports(&[usb("/dev/ttyUSB0", 0x1A86, 0x7523, serial, Some(0))]);
            let got = s.follow_moved_ports(
                &[usb("/dev/ttyUSB1", 0x1A86, 0x7523, serial, Some(0))],
                nothing,
            );
            assert_eq!(s.serial_port, "/dev/ttyUSB0", "serial {serial:?}");
            assert!(got.followed.is_empty(), "serial {serial:?}");
        }
        // Windows: a no-serial dual chip shows the stand-in serial and MI_00: still not a key.
        let mut s = Settings {
            serial_port: "COM5".into(),
            ..Settings::default()
        };
        s.remember_present_ports(&[usb("COM5", 0x10C4, 0xEA70, "6", Some(0))]);
        s.follow_moved_ports(&[usb("COM7", 0x10C4, 0xEA70, "7", Some(0))], nothing);
        assert_eq!(s.serial_port, "COM5");
    }

    /// Gone with nothing like it here: the words name the port and the ports that are here.
    #[test]
    fn a_port_not_found_says_what_is_here() {
        let mut s = ft891_station();
        s.remember_present_ports(&cp2105("COM5", "COM6"));
        let got = s.follow_moved_ports(&[plain("COM1"), plain("COM3")], nothing);
        assert_eq!(s.serial_port, "COM5");
        assert_eq!(
            got.cat_missing.as_deref(),
            Some("COM5 isn't on this computer now. Ports here: COM1, COM3.")
        );
        assert_eq!(got.missing.len(), 2, "the PTT port is gone too");
    }

    /// A follow onto a name another port setting already uses would put two settings on one
    /// port. Refused: the port is reported missing instead.
    #[test]
    fn a_follow_onto_a_port_already_in_use_is_refused() {
        let mut s = Settings {
            serial_port: "COM5".into(),
            rotator_port: "COM7".into(),
            ..Settings::default()
        };
        s.port_identities.insert(
            "COM5".into(),
            PortIdentity {
                vid: 0x0403,
                pid: 0x6001,
                serial: "A702TB52".into(),
                ..PortIdentity::default()
            },
        );
        let got = s.follow_moved_ports(&[usb("COM7", 0x0403, 0x6001, "A702TB52", None)], nothing);
        assert_eq!(
            (s.serial_port.as_str(), s.rotator_port.as_str()),
            ("COM5", "COM7")
        );
        assert!(got.followed.is_empty() && got.cat_missing.is_some());
    }

    /// Every port setting follows, not only CAT: the keying lines, the WinKeyer, the rotator and
    /// the amplifier, flat and per radio, each told under its own name.
    #[test]
    fn every_port_setting_follows() {
        let mut s = Settings {
            cw_key_port: "COM11".into(),
            winkeyer_port: "COM12".into(),
            rtty_fsk_port: "COM13".into(),
            rotator_port: "COM14".into(),
            amp_port: "COM15".into(),
            ..Settings::default()
        };
        s.ensure_radio_profiles();
        s.sync_active_from_flat();
        let dev = |name: &str, serial: &str| usb(name, 0x0403, 0x6015, serial, None);
        let was = ["COM11", "COM12", "COM13", "COM14", "COM15"];
        let serials = ["KEY00001", "WK000002", "FSK00003", "ROT00004", "AMP00005"];
        let before: Vec<_> = was.iter().zip(serials).map(|(n, sn)| dev(n, sn)).collect();
        s.remember_present_ports(&before);
        let now: Vec<_> = (21..26)
            .zip(serials)
            .map(|(n, sn)| dev(&format!("COM{n}"), sn))
            .collect();
        let got = s.follow_moved_ports(&now, nothing);
        assert_eq!(
            [
                s.cw_key_port.as_str(),
                s.winkeyer_port.as_str(),
                s.rtty_fsk_port.as_str(),
                s.rotator_port.as_str(),
                s.amp_port.as_str(),
            ],
            ["COM21", "COM22", "COM23", "COM24", "COM25"]
        );
        let r = &s.radios[0];
        assert_eq!(
            [
                r.cw_key_port.as_str(),
                r.winkeyer_port.as_str(),
                r.rtty_fsk_port.as_str(),
                r.rotator_port.as_str(),
                r.amp_port.as_str(),
            ],
            ["COM21", "COM22", "COM23", "COM24", "COM25"]
        );
        assert_eq!(got.followed.len(), 5);
        assert!(got.followed[0].starts_with("Your CW key cable moved from COM11 to COM21"));
        assert!(got.followed[4].starts_with("Your amplifier moved from COM15 to COM25"));
    }

    /// A Windows friendly name carries the COM number, which is exactly what changed: it must
    /// not stop the match, while the rest of the name still has to agree.
    #[test]
    fn the_com_number_in_a_friendly_name_is_not_the_product() {
        let named = |name: &str, product: &str| PresentPort {
            name: name.into(),
            identity: Some(PortIdentity {
                vid: 0x10C4,
                pid: 0xEA70,
                serial: "00A5B1C2".into(),
                product: product.into(),
                interface: Some(0),
            }),
        };
        let mut s = Settings {
            serial_port: "COM5".into(),
            ..Settings::default()
        };
        s.remember_present_ports(&[named(
            "COM5",
            "Silicon Labs Dual CP2105 USB to UART Bridge: Enhanced COM Port (COM5)",
        )]);
        let got = s.follow_moved_ports(
            &[named(
                "COM7",
                "Silicon Labs Dual CP2105 USB to UART Bridge: Enhanced COM Port (COM7)",
            )],
            nothing,
        );
        assert_eq!(s.serial_port, "COM7", "{got:?}");
    }

    /// The launch's CAT status leads with what moved; the "not here" words only lead a failed
    /// connect, and an empty finding leaves the status exactly as it was.
    #[test]
    fn the_launch_status_leads_with_the_port_findings() {
        let notes = PortFollow {
            followed: vec!["Your radio moved from COM5 to COM7; Nexus followed it.".into()],
            cat_missing: Some("COM6 isn't on this computer now. Ports here: COM7.".into()),
            ..PortFollow::default()
        };
        assert_eq!(
            launch_cat_detail(&notes, Some(true), "Connected."),
            "Your radio moved from COM5 to COM7; Nexus followed it. Connected."
        );
        assert_eq!(
            launch_cat_detail(&notes, Some(false), "No answer."),
            "Your radio moved from COM5 to COM7; Nexus followed it. COM6 isn't on this computer \
             now. Ports here: COM7. No answer."
        );
        assert_eq!(
            launch_cat_detail(&PortFollow::default(), Some(false), "No answer."),
            "No answer."
        );
    }

    /// Test CAT names a CAT port that is gone, with the ports here; says nothing while it is
    /// here, for a network radio, or with nothing enumerated.
    #[test]
    fn test_cat_names_a_cat_port_that_is_gone() {
        let s = Settings {
            serial_port: "COM5".into(),
            ..Settings::default()
        };
        assert_eq!(
            cat_port_not_here(&s, &[plain("COM3")], nothing).as_deref(),
            Some("COM5 isn't on this computer now. Ports here: COM3.")
        );
        assert_eq!(cat_port_not_here(&s, &[plain("com5")], nothing), None);
        assert_eq!(cat_port_not_here(&s, &[], nothing), None);
        let net = Settings {
            rig_conn: "network".into(),
            rig_addr: "192.168.1.50:4992".into(),
            ..s.clone()
        };
        assert_eq!(cat_port_not_here(&net, &[plain("COM3")], nothing), None);
    }

    /// An empty enumeration (no serial support, or the walk failed) is never "every port is
    /// gone": nothing moves, nothing is said, nothing is forgotten.
    #[test]
    fn an_empty_enumeration_changes_nothing() {
        let mut s = ft891_station();
        s.remember_present_ports(&cp2105("COM5", "COM6"));
        let kept = s.clone();
        let got = s.follow_moved_ports(&[], nothing);
        assert_eq!(got, PortFollow::default());
        assert_eq!(s, kept);
    }

    /// An identity no setting names any more is forgotten, so the map never grows stale entries.
    #[test]
    fn an_identity_no_setting_names_is_forgotten() {
        let mut s = Settings {
            serial_port: "COM5".into(),
            ..Settings::default()
        };
        s.remember_present_ports(&[usb("COM5", 0x0403, 0x6001, "A702TB52", None)]);
        s.serial_port = "COM3".into();
        assert!(s.remember_present_ports(&[plain("COM3")]));
        assert!(s.port_identities.is_empty());
    }

    /// ⭐ A damaged identity map costs only the damage. A hand-edited or half-written
    /// `portIdentities` (an entry of the wrong type, junk, a field from a newer build, or a map
    /// that is not a map at all) used to fail the whole file: `load` set it aside and the station
    /// came up on defaults, every setting lost. The bad entries are dropped; everything else,
    /// good identities included, loads.
    #[test]
    fn a_damaged_identity_map_loses_only_the_damaged_entries() {
        let dir = std::env::temp_dir().join(format!(
            "nexus-port-follow-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let file = |identities: serde_json::Value| {
            let doc = serde_json::json!({
                "mycall": "W9XYZ",
                "serialPort": "COM5",
                "portIdentities": identities,
            });
            std::fs::write(&path, doc.to_string()).unwrap();
            Settings::load(&path)
        };
        let s = file(serde_json::json!({
            "COM5": { "vid": "oops", "pid": 1 },
            "COM6": 12,
            "COM8": null,
            "COM9": {},
            "COM7": { "vid": 4292, "pid": 60016, "serial": "00A5B1C2", "interface": 1,
                      "fromANewerBuild": true },
        }));
        assert_eq!(
            (s.mycall.as_str(), s.serial_port.as_str()),
            ("W9XYZ", "COM5")
        );
        assert_eq!(s.port_identities.len(), 1, "{:?}", s.port_identities);
        assert_eq!(s.port_identities["COM7"].interface, Some(1));
        for junk in [
            serde_json::json!("junk"),
            serde_json::json!([1, 2]),
            serde_json::json!(7),
        ] {
            let s = file(junk);
            assert_eq!(
                (s.mycall.as_str(), s.serial_port.as_str()),
                ("W9XYZ", "COM5")
            );
            assert!(s.port_identities.is_empty());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MIGRATION: a settings file from before identities loads unchanged, and a station that has
    /// none writes no new key, so the file it saves is the one it always saved.
    #[test]
    fn an_old_settings_file_loads_unchanged() {
        let old = serde_json::json!({ "serialPort": "COM5", "pttSerialPort": "COM6" });
        let s: Settings = serde_json::from_value(old).unwrap();
        assert_eq!(
            (s.serial_port.as_str(), s.ptt_serial_port.as_str()),
            ("COM5", "COM6")
        );
        assert!(s.port_identities.is_empty());
        let written = serde_json::to_value(&s).unwrap();
        assert!(written.get("portIdentities").is_none());
        // And one that has them round-trips them.
        let mut s = s;
        s.remember_present_ports(&cp2105("COM5", "COM6"));
        let back: Settings = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back.port_identities, s.port_identities);
        assert_eq!(back.port_identities["COM6"].interface, Some(1));
    }
}
