//! Opening a serial port that carries NO data — only its DTR/RTS control lines.
//!
//! Three transmit paths open a port purely to toggle a control line: the CW keyline
//! keyer ([`crate::serial_keyer`]), the true-FSK RTTY keyer ([`crate::rtty_fsk`]), and
//! serial PTT ([`crate::rig`]). None of them ever writes a byte to TxD, so the baud
//! rate is functionally meaningless — but it is not optional. The OS still programs the
//! UART (a Windows DCB) with it at open time, and a rig's USB-serial firmware is free to
//! REJECT a particular rate outright.
//!
//! FIELD REPORT (DA6IT, Yaesu FTX-1, 2026-07): the FTX-1's built-in Standard COM port
//! fails to open at exactly 1200 baud — Windows returns "A device attached to the system
//! is not functioning" — while 2400/4800/9600/19200/38400 all open fine. Nexus hardcoded
//! 1200 at all three call sites, so CW keying, FSK RTTY, and serial PTT were dead on a
//! brand-new rig because of a parameter that carries no meaning on the wire.
//!
//! So: ask for 9600 (the rate every keyline interface accepts) and, if the open fails,
//! walk DOWN a small ladder before giving up. This is deliberately NOT a setting — a
//! parameter with no on-air effect should heal itself rather than land in Settings for
//! the operator to guess at, and the ladder keeps 1200 at its tail so an interface that
//! only likes the historical rate still works.
//!
//! It also owns the IDLE-STATE rule ([`idle_both_lines`]), because that rule turned out to
//! be a property of opening a port rather than of any one caller — see its own docs.
//!
//! NOT for ports that carry data: the WinKeyer's 1200 baud is a PROTOCOL rate (K1EL host
//! mode) and CAT/CI-V ports use the operator's configured rate. Both are real serial
//! traffic where the rate has to match the far end. Those ports still need the idle rule —
//! `civ::broker::CivDaemon::start` opens a CI-V port at the operator's baud and calls
//! [`idle_both_lines`] itself.

/// Baud rates tried, in order, when opening a control-line-only port: the first is the
/// default, the rest are the fallback ladder. Most- to least- universally accepted,
/// ending at the value Nexus used to hardcode.
pub const BAUD_LADDER: [u32; 5] = [9600, 19200, 4800, 2400, 1200];

/// Read timeout for a control-line-only port. Nothing is ever read from it; this just
/// keeps a pathological driver from blocking the keying thread.
pub const OPEN_TIMEOUT_MS: u64 = 200;

/// Just enough of a serial port to state the idle rule — so the rule is unit-testable with
/// no hardware, which is the only reason it can have a test at all.
pub trait ControlLinePins {
    fn set_dtr(&mut self, on: bool) -> std::io::Result<()>;
    fn set_rts(&mut self, on: bool) -> std::io::Result<()>;
}

#[cfg(feature = "serial")]
impl ControlLinePins for Box<dyn serialport::SerialPort> {
    fn set_dtr(&mut self, on: bool) -> std::io::Result<()> {
        (**self)
            .write_data_terminal_ready(on)
            .map_err(std::io::Error::other)
    }
    fn set_rts(&mut self, on: bool) -> std::io::Result<()> {
        (**self)
            .write_request_to_send(on)
            .map_err(std::io::Error::other)
    }
}

/// ⚠️ CRITICAL (stuck PTT): drive DTR **and** RTS deasserted, right after opening a port.
///
/// The kernel asserts DTR and RTS when a serial port is opened, and `serialport`'s
/// `dtr_on_open(false)` is documented as unreliable on Linux. Every path that opens a port
/// then manages at most ONE line — the CW keyer keys its keyline, the FSK keyer keys its
/// keyline, serial PTT keys the PTT line, the native CI-V daemon keys nothing at all — so
/// the line each of them does NOT manage stays asserted for the whole session. On a
/// conventional interface (DTR = key, RTS = PTT) that is the rig held in transmit from the
/// moment you connect, with nothing in Nexus that would take it back down.
///
/// Deasserted is the correct idle for every one of those wirings: key up, PTT off, FSK mark.
///
/// **This lived as a comment and a copy-pasted pair of calls in two of the four openers**
/// (`serial_keyer`, `rtty_fsk`); the other two — `rig::Rig::serial_ptt` and
/// `civ::broker::CivDaemon::start` — simply did not have it. A rule that has to be remembered
/// at each call site is a rule that will be missed at the next one, so it is one function now,
/// called from the opener itself where it can be, and by name where the port carries data and
/// therefore cannot go through [`open_control_line_port`].
///
/// Errors are swallowed deliberately: a port that refuses a control-line write is still a
/// port we want to key on (some virtual/BT ports refuse), and the caller's own first real
/// operation is where a dead port should surface.
pub fn idle_both_lines<P: ControlLinePins + ?Sized>(port: &mut P) {
    let _ = port.set_dtr(false);
    let _ = port.set_rts(false);
}

/// Walk [`BAUD_LADDER`] and return the first successful open plus the rate that worked.
///
/// `open` is the only thing that touches hardware, so the fallback behaviour is unit
/// testable with no serial port. On total failure the error names the port, EVERY rate
/// tried, and the underlying OS error text — that string is the whole diagnosis. (The
/// message this replaced guessed at causes and withheld what the OS actually said, which
/// is why the FTX-1 report had to be diagnosed by hand in PowerShell.)
pub fn open_first_working_baud<T, E: std::fmt::Display>(
    port: &str,
    mut open: impl FnMut(u32) -> Result<T, E>,
) -> Result<(T, u32), String> {
    let mut last_err = String::new();
    for &baud in &BAUD_LADDER {
        match open(baud) {
            Ok(handle) => return Ok((handle, baud)),
            Err(e) => last_err = e.to_string(),
        }
    }
    let tried = BAUD_LADDER
        .iter()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "couldn't open {port} at any baud rate (tried {tried}) — the system said: {last_err}"
    ))
}

/// Open `port` for control-line-only use (DTR/RTS toggling), walking [`BAUD_LADDER`]
/// until one rate is accepted. **Both control lines come back deasserted** — see
/// [`idle_both_lines`] for why that is the opener's job and not the caller's.
#[cfg(feature = "serial")]
pub fn open_control_line_port(port: &str) -> std::io::Result<Box<dyn serialport::SerialPort>> {
    let (mut sp, baud) = open_first_working_baud(port, |baud| {
        serialport::new(port, baud)
            .timeout(std::time::Duration::from_millis(OPEN_TIMEOUT_MS))
            .open()
    })
    .map_err(std::io::Error::other)?;
    idle_both_lines(&mut sp);
    if baud != BAUD_LADDER[0] {
        // Worth a line in the log: it tells a rig-specific quirk (FTX-1) apart from a
        // cable/port problem, and the operator never has to know the rate exists.
        eprintln!(
            "tempo-audio: {port} refused {} baud, opened at {baud} instead (control-line only \
             port — no data is sent, so the rate has no on-air effect)",
            BAUD_LADDER[0]
        );
    }
    Ok(sp)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mock open that accepts only the rates in `accept` and otherwise fails with the
    /// exact Windows text the FTX-1 produces.
    fn accepting_only(accept: &'static [u32]) -> impl FnMut(u32) -> Result<u32, String> {
        move |baud| {
            if accept.contains(&baud) {
                Ok(baud)
            } else {
                Err("A device attached to the system is not functioning.".to_string())
            }
        }
    }

    #[test]
    fn defaults_to_9600_without_touching_the_rest_of_the_ladder() {
        let mut tried = Vec::new();
        let got = open_first_working_baud("COM10", |baud| {
            tried.push(baud);
            Ok::<u32, String>(baud)
        });
        assert_eq!(got, Ok((9600, 9600)));
        assert_eq!(tried, vec![9600], "the default rate must be tried alone");
    }

    #[test]
    fn falls_back_when_the_rig_refuses_the_default_rate() {
        // The FTX-1 shape, generalized: only ONE rate in the ladder is accepted, and it
        // is the last one — the walk has to reach it instead of giving up on the first
        // refusal. (The real FTX-1 refuses only 1200, which now isn't even attempted.)
        let mut tried = Vec::new();
        let got = open_first_working_baud("COM10", |baud| {
            tried.push(baud);
            if baud == 1200 {
                Ok(baud)
            } else {
                Err("A device attached to the system is not functioning.".to_string())
            }
        });
        assert_eq!(got, Ok((1200, 1200)));
        assert_eq!(tried.first(), Some(&9600), "the default is tried first");
        assert_eq!(tried, BAUD_LADDER.to_vec(), "must walk the whole ladder");
    }

    #[test]
    fn ladder_leads_with_9600_and_still_ends_at_1200() {
        // 9600 is the rate keyline interfaces universally accept; 1200 is the historical
        // Nexus value and must stay reachable for any interface that only likes it.
        assert_eq!(BAUD_LADDER[0], 9600);
        assert_eq!(BAUD_LADDER[BAUD_LADDER.len() - 1], 1200);
    }

    #[test]
    fn total_failure_reports_the_os_error_the_port_and_every_rate_tried() {
        let err = open_first_working_baud("COM10", accepting_only(&[])).unwrap_err();
        assert!(err.contains("COM10"), "must name the port: {err}");
        assert!(
            err.contains("A device attached to the system is not functioning."),
            "must carry the OS error verbatim: {err}"
        );
        for baud in BAUD_LADDER {
            assert!(
                err.contains(&baud.to_string()),
                "must list {baud} as tried: {err}"
            );
        }
        assert!(
            !err.contains("Check the port name"),
            "must not guess at causes in place of the real error: {err}"
        );
    }

    /// A port that records what was asked of its control lines, and can be made to refuse.
    #[derive(Default)]
    struct FakePort {
        driven: Vec<(&'static str, bool)>,
        refuse: bool,
    }
    impl ControlLinePins for FakePort {
        fn set_dtr(&mut self, on: bool) -> std::io::Result<()> {
            self.driven.push(("dtr", on));
            if self.refuse {
                return Err(std::io::Error::other("port refuses control lines"));
            }
            Ok(())
        }
        fn set_rts(&mut self, on: bool) -> std::io::Result<()> {
            self.driven.push(("rts", on));
            if self.refuse {
                return Err(std::io::Error::other("port refuses control lines"));
            }
            Ok(())
        }
    }

    /// ⚠️ THE STUCK-PTT RULE. **Both** lines, deasserted, at open. Every path that opens a
    /// port then manages at most one of them, so the un-managed line stays asserted for the
    /// session — on a conventional interface (DTR = key, RTS = PTT) that is the rig held in
    /// transmit from the moment you connect. `Rig::serial_ptt` and `CivDaemon::start` both had
    /// exactly that hole while the rule lived as a copy-pasted pair of calls inside the other
    /// two openers; it is one function now, so the next opener cannot miss it.
    #[test]
    fn the_idle_rule_deasserts_both_lines_not_just_the_keyed_one() {
        let mut p = FakePort::default();
        idle_both_lines(&mut p);
        assert_eq!(
            p.driven,
            vec![("dtr", false), ("rts", false)],
            "the line a caller does NOT key is the one that gets left asserted"
        );
    }

    /// A port that refuses a control-line write (some virtual/Bluetooth ports do) must not
    /// stop the other line being deasserted, and must not fail the open: the caller's own
    /// first real operation is where a dead port should surface.
    #[test]
    fn a_refused_control_line_does_not_strand_the_other_one() {
        let mut p = FakePort {
            refuse: true,
            ..Default::default()
        };
        idle_both_lines(&mut p);
        assert_eq!(p.driven, vec![("dtr", false), ("rts", false)]);
    }

    #[test]
    fn a_port_that_only_accepts_a_middle_rate_still_opens() {
        let (handle, baud) = open_first_working_baud("/dev/ttyUSB0", accepting_only(&[4800]))
            .expect("4800 is in the ladder");
        assert_eq!((handle, baud), (4800, 4800));
    }
}
