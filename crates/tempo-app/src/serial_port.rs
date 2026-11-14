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
