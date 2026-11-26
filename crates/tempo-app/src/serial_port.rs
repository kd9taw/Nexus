//! One serial port, however its stored name is spelled.
//!
//! The same port reaches the settings as `com5` or `COM5`, as `\\.\COM5` (the Win32 device path),
//! with spaces around it, or on a mac as `/dev/tty.*` and its `/dev/cu.*` callout twin, which are
//! one device (the radio loop opens a stored `tty.*` as its `cu.*` twin,
//! `tempo_audio::ports::heal_stored_port`). Every decision that must not be escaped by a spelling
//! reads names through [`same_port`], here in tempo-app so the engine's keyline guards and
//! tempo-audio's port opener share ONE rule and can never disagree about which port is which.

/// Do two stored port names name the SAME serial port, whatever their spelling? For the keying
/// decisions that must not be escaped by a spelling: the engine's keyline guards (FSK and the CW
/// keyline against serial PTT, `Engine::serial_ptt_keys`), the shared control-line handle
/// (`tempo_audio::control_line::SharedControlLines`), and the Hardware-handshake refusal's "RTS
/// keys this radio" fact. Case-insensitive, as every port comparison in Nexus already is; an empty
/// name matches nothing. Over-matching is the safe direction here: a guard can only refuse a
/// keying, never key one. On macOS and Linux a leading `~/` is the home directory
/// ([`expand_home_with`]), as the port opener reads it (`tempo_audio::ports::heal_stored_port`).
pub fn same_port(a: &str, b: &str) -> bool {
    #[cfg(unix)]
    let home = std::env::var_os("HOME");
    #[cfg(not(unix))]
    let home = None::<std::ffi::OsString>;
    same_port_at_home(a, b, home.as_deref())
}

/// [`same_port`] with the home directory a parameter, so the rule is testable without touching
/// the process environment.
fn same_port_at_home(a: &str, b: &str, home: Option<&std::ffi::OsStr>) -> bool {
    let norm = |p: &str| {
        let p = expand_home_with(p.trim().to_string(), home);
        let p = match p.get(..4) {
            Some(head) if head.eq_ignore_ascii_case("\\\\.\\") => &p[4..],
            _ => &p[..],
        };
        let p = p.to_ascii_lowercase();
        match p.strip_prefix("/dev/tty.") {
            Some(rest) => format!("/dev/cu.{rest}"),
            None => p,
        }
    };
    let (a, b) = (norm(a), norm(b));
    !a.is_empty() && a == b
}

/// `~/rest` → `<home>/rest`; every other name (and a missing or empty home) is returned as
/// it came. Only the `~/` form: `~user/` would need the password database, and no program
/// that makes a virtual port hands out that spelling. The home is a parameter so the rule is
/// testable without touching the process environment. One rule for the port opener
/// (`tempo_audio::ports::heal_stored_port`) and [`same_port`], so a guard reads a `~/` port as
/// the port opens it.
pub fn expand_home_with(port: String, home: Option<&std::ffi::OsStr>) -> String {
    match (
        port.trim_start().strip_prefix("~/"),
        home.and_then(|h| h.to_str()),
    ) {
        (Some(rest), Some(home)) if !home.is_empty() => {
            format!("{}/{rest}", home.trim_end_matches('/'))
        }
        _ => port,
    }
}

/// What a serial port was when it last worked: the USB facts that survive Windows renumbering
/// it (another USB socket, a hub, a driver reinstall, an update's restart), so a saved port that
/// is no longer there can be found again under its new name
/// ([`crate::settings::Settings::follow_moved_ports`]).
///
/// `serial` is the device's own USB serial number, `""` when it has none (Windows' generated
/// stand-in and a factory default a whole production run shares are kept but never count as one,
/// [`Self::serial_key`]). `interface` is which port of a multi-port chip this is: the FT-891's
/// CP2105 shows its Enhanced and Standard COM ports with the SAME VID, PID and serial number, and
/// only the interface number tells them apart. It is never a key by itself: Linux numbers EVERY
/// single-interface USB serial 0, and Windows says `MI_00` for any dual chip.
/// `product` is the USB product string, or on Windows the device's friendly name with the
/// ` (COMn)` it carries taken off ([`normalized_product`]), since that part is what changes.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PortIdentity {
    pub vid: u16,
    pub pid: u16,
    pub serial: String,
    pub product: String,
    pub interface: Option<u8>,
}

impl PortIdentity {
    /// The serial number when it can tell one device from another. Not one:
    /// - Windows' stand-in for a device with none, the first piece of the instance id it made up
    ///   for that USB socket (`"5"` out of `5&2C3A…`): it changes when the device moves and is
    ///   shared by unrelated devices, so a serial that short is not one;
    /// - a factory default every chip of a run carries: Silicon Labs' `0001` on generic CP210x
    ///   cables (and any other zeros-and-a-digit value), and the `A50285BI` of cloned FTDI chips.
    ///
    /// What this cannot see: two cables whose serial numbers were rewritten to the SAME value
    /// that is not on this list (a reseller's batch programmed alike). They read as one device.
    /// At launch that is two candidates, so ambiguous and not followed, while both are plugged in;
    /// with only the other one present it would be followed. The radio loop's follow also refuses
    /// any port present while CAT worked, so a cable that sat beside the radio is still refused.
    fn serial_key(&self) -> Option<&str> {
        let s = self.serial.trim();
        let significant = s.trim_start_matches('0');
        let factory = (s.bytes().all(|b| b.is_ascii_digit()) && significant.len() <= 1)
            || FACTORY_SERIALS.iter().any(|f| f.eq_ignore_ascii_case(s));
        (s.len() >= 4 && !factory).then_some(s)
    }
}

/// Serial numbers shared by every chip of a production run, which identify no device
/// ([`PortIdentity::serial_key`]).
const FACTORY_SERIALS: &[&str] = &["A50285BI"];

/// A product name without the ` (COM5)` Windows writes at the end of a serial device's friendly
/// name: that part is the port name, which is exactly what changed.
pub fn normalized_product(product: &str) -> String {
    let p = product.trim();
    if let Some(open) = p.rfind(" (") {
        let tail = &p[open + 2..];
        if let Some(inner) = tail.strip_suffix(')') {
            if looks_like_com(inner) {
                return p[..open].trim_end().to_string();
            }
        }
    }
    p.to_string()
}

fn looks_like_com(s: &str) -> bool {
    s.len() > 3
        && s[..3].eq_ignore_ascii_case("com")
        && s.as_bytes()[3..].iter().all(u8::is_ascii_digit)
}

/// A serial port present on this computer now, with its USB identity when it is a USB port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentPort {
    pub name: String,
    pub identity: Option<PortIdentity>,
}

/// Where a saved port's device is now, judged on its identity alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refound {
    /// Exactly one present port is that device.
    At(String),
    /// Several present ports could be it, or the identity is too thin to tell (no serial number
    /// of its own): Nexus does not guess.
    Ambiguous(Vec<String>),
    /// Nothing here is that device.
    NotFound,
}

/// Find the device `was` among the ports present now. Unambiguous only on the device's own
/// serial number ([`PortIdentity::serial_key`]); the interface number then picks the right port
/// of a multi-port chip, whose ports share one serial. VID and PID alone, with or without an
/// interface number (two identical cables, or a radio and its twin), are never enough. Product
/// names, when both sides have one, must agree as well.
pub fn refind(was: &PortIdentity, present: &[PresentPort]) -> Refound {
    let serial = was.serial_key();
    let product = normalized_product(&was.product);
    let hits: Vec<String> = present
        .iter()
        .filter_map(|p| p.identity.as_ref().map(|id| (p, id)))
        .filter(|(_, id)| id.vid == was.vid && id.pid == was.pid)
        .filter(|(_, id)| serial.is_none_or(|s| id.serial_key() == Some(s)))
        .filter(|(_, id)| was.interface.is_none() || id.interface == was.interface)
        .filter(|(_, id)| {
            let theirs = normalized_product(&id.product);
            product.is_empty() || theirs.is_empty() || theirs == product
        })
        .map(|(p, _)| p.name.clone())
        .collect();
    let keyed = serial.is_some();
    match hits.len() {
        0 => Refound::NotFound,
        1 if keyed => Refound::At(hits[0].clone()),
        _ => Refound::Ambiguous(hits),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `~/` path is its home-directory spelling, both ways round, and nothing else is: another
    /// file under home, a missing home, and `~user/` stay apart.
    #[test]
    fn a_tilde_path_is_the_same_port_as_its_home_spelling() {
        let home = Some(std::ffi::OsStr::new("/Users/op"));
        let same = |a, b| same_port_at_home(a, b, home);
        assert!(same("~/rig-pty1", "/Users/op/rig-pty1"));
        assert!(same(" /users/op/RIG-PTY1", "~/rig-pty1 "));
        assert!(!same("~/rig-pty1", "~/rig-pty2"));
        assert!(!same("~/rig-pty1", "/Users/other/rig-pty1"));
        assert!(!same_port_at_home("~/rig-pty1", "/Users/op/rig-pty1", None));
        assert!(!same("~op/rig-pty1", "/Users/op/rig-pty1"));
        // The rest of the rule is unchanged by a home.
        assert!(same("/dev/tty.usbserial-1", "/dev/cu.usbserial-1"));
        assert!(same("\\\\.\\COM7", "com7"));
        assert!(!same("", ""));
    }
}
