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
    open: impl FnMut(u32) -> Result<T, E>,
) -> Result<(T, u32), String> {
    open_first_working_baud_checked(port, || Ok(()), open)
}

/// Check permission immediately before each actual port-open attempt. A
/// rejected check ends the ladder; it is not another unsupported baud rate.
fn open_first_working_baud_checked<T, E: std::fmt::Display>(
    port: &str,
    mut before_open: impl FnMut() -> Result<(), String>,
    mut open: impl FnMut(u32) -> Result<T, E>,
) -> Result<(T, u32), String> {
    let mut last_err = String::new();
    for &baud in &BAUD_LADDER {
        before_open()?;
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
    open_control_line_port_checked(port, || Ok(()))
}

/// TESTS ONLY: control-line ports that exist only in memory, for tests that need a keying port
/// without hardware. A port opens EXCLUSIVELY, as a real one does: while a handle to it lives,
/// another open fails, the way Windows answers "Access is denied." And it comes back with both
/// lines deasserted, as [`idle_both_lines`] leaves a real one. Per thread, like the other test
/// doubles: a test's ports are its own.
#[cfg(all(test, feature = "serial"))]
pub(crate) mod fake_ports {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::time::Duration;

    /// Whether a handle holds the port, and the level of its two control lines.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub(crate) struct State {
        pub(crate) held: bool,
        pub(crate) rts: bool,
        pub(crate) dtr: bool,
    }

    thread_local! {
        static PORTS: RefCell<HashMap<String, State>> = RefCell::new(HashMap::new());
    }

    /// Make `name` a fake port: nobody holding it, both lines low.
    pub(crate) fn install(name: &str) {
        PORTS.with(|p| p.borrow_mut().insert(name.to_string(), State::default()));
    }

    pub(crate) fn state(name: &str) -> State {
        PORTS.with(|p| p.borrow()[name])
    }

    /// The opener's door: `None` for a name that is no fake port. `before_open` is the real
    /// opener's own check (a Remote selection's permission), made before the port is touched.
    pub(super) fn open(
        name: &str,
        before_open: impl FnOnce() -> Result<(), String>,
    ) -> Option<std::io::Result<Box<dyn serialport::SerialPort>>> {
        PORTS.with(|p| {
            let mut ports = p.borrow_mut();
            let state = ports.get_mut(name)?;
            if let Err(reason) = before_open() {
                return Some(Err(std::io::Error::other(reason)));
            }
            if state.held {
                return Some(Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Access is denied.",
                )));
            }
            *state = State {
                held: true,
                rts: false,
                dtr: false,
            };
            let port: Box<dyn serialport::SerialPort> = Box::new(FakePort {
                name: name.to_string(),
            });
            Some(Ok(port))
        })
    }

    struct FakePort {
        name: String,
    }

    impl FakePort {
        fn set(&self, line: impl FnOnce(&mut State)) {
            PORTS.with(|p| {
                if let Some(state) = p.borrow_mut().get_mut(&self.name) {
                    line(state);
                }
            });
        }
    }

    impl Drop for FakePort {
        fn drop(&mut self) {
            // `try_with`: a handle outliving the thread's registry must not panic on the way out.
            let _ = PORTS.try_with(|p| {
                if let Some(state) = p.borrow_mut().get_mut(&self.name) {
                    state.held = false;
                }
            });
        }
    }

    impl std::io::Read for FakePort {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Ok(0)
        }
    }

    impl std::io::Write for FakePort {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl serialport::SerialPort for FakePort {
        fn name(&self) -> Option<String> {
            Some(self.name.clone())
        }
        fn baud_rate(&self) -> serialport::Result<u32> {
            Ok(super::BAUD_LADDER[0])
        }
        fn data_bits(&self) -> serialport::Result<serialport::DataBits> {
            Ok(serialport::DataBits::Eight)
        }
        fn flow_control(&self) -> serialport::Result<serialport::FlowControl> {
            Ok(serialport::FlowControl::None)
        }
        fn parity(&self) -> serialport::Result<serialport::Parity> {
            Ok(serialport::Parity::None)
        }
        fn stop_bits(&self) -> serialport::Result<serialport::StopBits> {
            Ok(serialport::StopBits::One)
        }
        fn timeout(&self) -> Duration {
            Duration::from_millis(super::OPEN_TIMEOUT_MS)
        }
        fn set_baud_rate(&mut self, _: u32) -> serialport::Result<()> {
            Ok(())
        }
        fn set_data_bits(&mut self, _: serialport::DataBits) -> serialport::Result<()> {
            Ok(())
        }
        fn set_flow_control(&mut self, _: serialport::FlowControl) -> serialport::Result<()> {
            Ok(())
        }
        fn set_parity(&mut self, _: serialport::Parity) -> serialport::Result<()> {
            Ok(())
        }
        fn set_stop_bits(&mut self, _: serialport::StopBits) -> serialport::Result<()> {
            Ok(())
        }
        fn set_timeout(&mut self, _: Duration) -> serialport::Result<()> {
            Ok(())
        }
        fn write_request_to_send(&mut self, level: bool) -> serialport::Result<()> {
            self.set(|state| state.rts = level);
            Ok(())
        }
        fn write_data_terminal_ready(&mut self, level: bool) -> serialport::Result<()> {
            self.set(|state| state.dtr = level);
            Ok(())
        }
        fn read_clear_to_send(&mut self) -> serialport::Result<bool> {
            Ok(false)
        }
        fn read_data_set_ready(&mut self) -> serialport::Result<bool> {
            Ok(false)
        }
        fn read_ring_indicator(&mut self) -> serialport::Result<bool> {
            Ok(false)
        }
        fn read_carrier_detect(&mut self) -> serialport::Result<bool> {
            Ok(false)
        }
        fn bytes_to_read(&self) -> serialport::Result<u32> {
            Ok(0)
        }
        fn bytes_to_write(&self) -> serialport::Result<u32> {
            Ok(0)
        }
        fn clear(&self, _: serialport::ClearBuffer) -> serialport::Result<()> {
            Ok(())
        }
        fn try_clone(&self) -> serialport::Result<Box<dyn serialport::SerialPort>> {
            Err(serialport::Error::new(
                serialport::ErrorKind::Unknown,
                "a fake port has one handle",
            ))
        }
        fn set_break(&self) -> serialport::Result<()> {
            Ok(())
        }
        fn clear_break(&self) -> serialport::Result<()> {
            Ok(())
        }
    }
}

/// Remote selection may need the incoming radio's serial PTT handle. Opening
/// itself can change line state, so every baud attempt carries the original
/// permission. Once opened, idle cleanup is unconditional even after expiry.
#[cfg(feature = "serial")]
pub(crate) fn open_control_line_port_permitted(
    port: &str,
    permission: &tempo_app::remote_control::WritePermission,
) -> std::io::Result<Box<dyn serialport::SerialPort>> {
    open_control_line_port_checked(port, || {
        permission
            .begin_write(std::time::Instant::now())
            .map_err(|reason| format!("Remote serial permission: {reason:?}"))
    })
}

#[cfg(feature = "serial")]
fn open_control_line_port_checked(
    port: &str,
    before_open: impl FnMut() -> Result<(), String>,
) -> std::io::Result<Box<dyn serialport::SerialPort>> {
    // TESTS ONLY: a port [`fake_ports`] made, for both openers. Not compiled outside the test
    // harness.
    #[cfg(test)]
    let before_open = {
        let mut before_open = before_open;
        if let Some(opened) = fake_ports::open(port, &mut before_open) {
            return opened;
        }
        before_open
    };
    let (mut sp, baud) = open_first_working_baud_checked(port, before_open, |baud| {
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

    #[test]
    fn checked_open_stops_before_a_later_baud_after_permission_is_lost() {
        let allowed = std::cell::Cell::new(true);
        let mut tried = Vec::new();
        let result = open_first_working_baud_checked(
            "test-port",
            || allowed.get().then_some(()).ok_or_else(|| "revoked".into()),
            |baud| {
                tried.push(baud);
                allowed.set(false);
                Err::<(), _>("port refused baud")
            },
        );
        assert_eq!(result, Err("revoked".into()));
        assert_eq!(tried, [BAUD_LADDER[0]]);
        // The same gate allows the native fallback ladder while permitted.
        tried.clear();
        let result = open_first_working_baud_checked(
            "test-port",
            || Ok(()),
            |baud| {
                tried.push(baud);
                if baud == BAUD_LADDER[1] {
                    Ok(baud)
                } else {
                    Err("unsupported baud")
                }
            },
        );
        assert_eq!(result, Ok((BAUD_LADDER[1], BAUD_LADDER[1])));
        assert_eq!(tried, BAUD_LADDER[..2]);
    }

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
