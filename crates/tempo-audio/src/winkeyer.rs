//! K1EL WinKeyer (WK1/2/3) host-mode CW keyer over serial — a hardware keyer with
//! rock-steady timing, as an alternative to the rig's CAT keyer or the soundcard tone.
//!
//! Protocol verified against the WK3 datasheet:
//! - Serial: **1200 baud, 8 data bits, 2 stop bits, no parity** (8N2).
//! - **Host Open** `00 02` → WK replies with a firmware revision byte; the host must wait
//!   for it before sending anything else (this enters host mode, WK1 command set).
//! - **Set WPM Speed** `02 nn`, nn = 5–99.
//! - **Send**: plain ASCII data bytes are keyed directly.
//! - **Host Close** `00 03` → returns the keyer to standalone mode.
//! - **Key Immediate** `0B nn`, nn = `01` key down, `00` key up — the tune command (#500).
//!   WK3 IC manual Rev 1.3 p.11 and WK2 IC manual V23 p.9, verbatim: "Use this command to
//!   implement a tune function. Once asserted, key down will remain in effect until either a
//!   key immediate with a zero value is received or the internal tune watchdog timer expires.
//!   The tune timer is hard coded to a value of 100 seconds and cannot be disabled. Key down
//!   can be aborted either by the paddles or by a clear buffer command." And of **Clear
//!   Buffer** `0A` (same page): "Tune and Pause are cancelled by this command", so the
//!   shutdown sequence below ends a key-down too.

/// Admin: Host Open — enter host mode (WK replies with its revision byte).
pub const HOST_OPEN: [u8; 2] = [0x00, 0x02];
/// Admin: Host Close — return to standalone mode.
pub const HOST_CLOSE: [u8; 2] = [0x00, 0x03];
/// Clear Buffer (`0A`) — immediately stops keying and flushes WK's send buffer.
pub const CLEAR_BUFFER: [u8; 1] = [0x0A];
/// Key Immediate, key down (`0B 01`): hold the key down for a tune until [`KEY_UP`], Clear
/// Buffer, a paddle press or WK's own 100 s tune watchdog ends it (module header).
pub const KEY_DOWN: [u8; 2] = [0x0B, 0x01];
/// Key Immediate, key up (`0B 00`): end a [`KEY_DOWN`].
pub const KEY_UP: [u8; 2] = [0x0B, 0x00];

/// Set-WPM-speed command bytes (`02 nn`), clamped to the WK range 5–99.
pub fn wpm_cmd(wpm: u32) -> [u8; 2] {
    [0x02, wpm.clamp(5, 99) as u8]
}

/// The byte sequence for a safe shutdown of the keyer: **Clear Buffer** (`0A`) first to
/// abort any CW still keying out of WK's send buffer, then **Host Close** (`00 03`) to
/// return to standalone. Host Close alone hands back to standalone with the buffer
/// intact, so a half-sent message keeps keying on the air after Nexus is gone.
pub fn shutdown_seq() -> [u8; 3] {
    [CLEAR_BUFFER[0], HOST_CLOSE[0], HOST_CLOSE[1]]
}

/// The ASCII bytes to send for `text` to be keyed: uppercased, restricted to characters
/// WinKeyer keys (letters, digits, and common CW punctuation/prosign glyphs).
pub fn encode_text(text: &str) -> Vec<u8> {
    text.to_ascii_uppercase()
        .bytes()
        .filter(|b| b.is_ascii_alphanumeric() || b" /?.,=+-".contains(b))
        .collect()
}

#[cfg(feature = "serial")]
pub use imp::WinKeyer;

#[cfg(feature = "serial")]
mod imp {
    use super::{encode_text, shutdown_seq, wpm_cmd, CLEAR_BUFFER, HOST_OPEN, KEY_DOWN, KEY_UP};
    use serialport::SerialPort;
    use std::io::Read;
    use std::time::Duration;

    /// An open WinKeyer in host mode. Drops back to standalone on close/drop.
    pub struct WinKeyer {
        port: Box<dyn SerialPort>,
    }

    impl WinKeyer {
        /// Open `port` at 1200 baud 8N2, enter host mode, and wait for the revision byte.
        /// Returns the keyer + its firmware revision.
        pub fn open(port: &str) -> std::io::Result<(Self, u8)> {
            let mut sp = serialport::new(port, 1200)
                .data_bits(serialport::DataBits::Eight)
                .stop_bits(serialport::StopBits::Two)
                .parity(serialport::Parity::None)
                .timeout(Duration::from_millis(1500))
                .open()
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            sp.write_all(&HOST_OPEN)?;
            sp.flush()?;
            // WK replies with its revision code; the host MUST wait for it before sending.
            let mut rev = [0u8; 1];
            sp.read_exact(&mut rev)?;
            Ok((Self { port: sp }, rev[0]))
        }

        /// Set keyer speed (WPM, clamped 5–99).
        pub fn set_wpm(&mut self, wpm: u32) -> std::io::Result<()> {
            self.port.write_all(&wpm_cmd(wpm))?;
            self.port.flush()
        }

        /// Abort: stop keying NOW and flush WinKeyer's send buffer (WK Clear Buffer).
        pub fn clear(&mut self) -> std::io::Result<()> {
            self.port.write_all(&CLEAR_BUFFER)?;
            self.port.flush()
        }

        /// Key Immediate (#500): the key held down for a tune (`down`), or let up. Down stays
        /// down until a key up, a Clear Buffer, a paddle press or WK's own 100 s tune watchdog.
        pub fn key_immediate(&mut self, down: bool) -> std::io::Result<()> {
            self.port
                .write_all(if down { &KEY_DOWN } else { &KEY_UP })?;
            self.port.flush()
        }

        /// Queue `text` to be keyed (uppercased ASCII; non-CW characters dropped).
        pub fn send(&mut self, text: &str) -> std::io::Result<()> {
            let bytes = encode_text(text);
            if !bytes.is_empty() {
                self.port.write_all(&bytes)?;
                self.port.flush()?;
            }
            Ok(())
        }

        /// Abort any buffered keying, then return the keyer to standalone mode (best-effort).
        ///
        /// Sends **Clear Buffer** before **Host Close** so a message still keying out of
        /// WK's buffer is stopped and flushed — never left to finish transmitting on the
        /// air after Nexus quits. Call this on shutdown; drop falls back to it too.
        pub fn close(&mut self) {
            let _ = self.port.write_all(&shutdown_seq());
            let _ = self.port.flush();
        }
    }

    impl std::io::Write for WinKeyer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.port.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.port.flush()
        }
    }

    impl Drop for WinKeyer {
        fn drop(&mut self) {
            self.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_commands_match_the_datasheet() {
        assert_eq!(HOST_OPEN, [0x00, 0x02]);
        assert_eq!(HOST_CLOSE, [0x00, 0x03]);
        assert_eq!(CLEAR_BUFFER, [0x0A]);
        // Key Immediate: WK3 IC manual Rev 1.3 p.11, WK2 IC manual V23 p.9.
        assert_eq!(KEY_DOWN, [0x0B, 0x01]);
        assert_eq!(KEY_UP, [0x0B, 0x00]);
    }

    #[test]
    fn shutdown_clears_the_buffer_before_returning_to_standalone() {
        // Regression: quitting mid-message must ABORT buffered CW, not let it keep
        // keying on the air. The shutdown sequence sends Clear Buffer (0A) *first*,
        // then Host Close (00 03) — Host Close alone leaves WK's buffer to finish
        // transmitting after we're gone.
        assert_eq!(shutdown_seq(), [0x0A, 0x00, 0x03]);
        assert_eq!(shutdown_seq()[0], CLEAR_BUFFER[0], "Clear Buffer must lead");
        assert_eq!(&shutdown_seq()[1..], &HOST_CLOSE, "then Host Close");
    }

    #[test]
    fn wpm_command_clamps_to_the_winkeyer_range() {
        assert_eq!(wpm_cmd(20), [0x02, 20]);
        assert_eq!(wpm_cmd(3), [0x02, 5]); // floor 5
        assert_eq!(wpm_cmd(150), [0x02, 99]); // ceil 99
    }

    #[test]
    fn encode_uppercases_and_drops_non_cw_chars() {
        assert_eq!(encode_text("cq de w1abc"), b"CQ DE W1ABC".to_vec());
        assert_eq!(encode_text("5nn k"), b"5NN K".to_vec());
        // strip characters the keyer can't send
        assert_eq!(encode_text("a*b\tc"), b"ABC".to_vec());
        assert_eq!(encode_text("73!"), b"73".to_vec());
    }
}
