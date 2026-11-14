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
/// keying, never key one.
pub fn same_port(a: &str, b: &str) -> bool {
    fn norm(p: &str) -> String {
        let p = p.trim();
        let p = match p.get(..4) {
            Some(head) if head.eq_ignore_ascii_case("\\\\.\\") => &p[4..],
            _ => p,
        };
        let p = p.to_ascii_lowercase();
        match p.strip_prefix("/dev/tty.") {
            Some(rest) => format!("/dev/cu.{rest}"),
            None => p,
        }
    }
    let (a, b) = (norm(a), norm(b));
    !a.is_empty() && a == b
}
