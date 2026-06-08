//! PTT / CAT control via Hamlib's `rigctld` daemon over TCP.
//!
//! Using `rigctld` (rather than linking `libhamlib`) keeps Tempo free of a C
//! build dependency: the operator runs `rigctld -m <model> -r <port>` and Tempo
//! talks to it over a socket. The protocol is line-based — commands like `T 1`
//! (PTT on), `T 0` (PTT off), `F 14074000` (set freq), `M USB 0` (set mode); a
//! reply of `RPRT 0` means success.
//!
//! For rigs without CAT, [`PttMode::Vox`] performs no keying and relies on the
//! transceiver's VOX (audio-triggered TX).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Which serial control line keys the transmitter for [`PttMode::Serial`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialLine {
    /// Request To Send.
    Rts,
    /// Data Terminal Ready.
    Dtr,
}

/// How transmit keying is performed.
#[derive(Debug, Clone)]
pub enum PttMode {
    /// No CAT — rely on the rig's VOX. Keying calls are no-ops.
    Vox,
    /// Key + tune via `rigctld` at this `host:port` (e.g. `127.0.0.1:4532`).
    Rigctld { addr: String },
    /// Key directly by asserting a serial control line (RTS or DTR) on `port`.
    ///
    /// With the `serial` feature this drives the line via `serialport`; without
    /// it the keying is logged and otherwise a no-op (the build has no serial
    /// backend), so the engine still runs and can fall back to VOX.
    Serial { port: String, line: SerialLine },
}

impl Default for PttMode {
    fn default() -> Self {
        PttMode::Rigctld {
            addr: "127.0.0.1:4532".to_string(),
        }
    }
}

/// rigctld command line for PTT.
pub fn ptt_line(on: bool) -> String {
    format!("T {}\n", on as u8)
}
/// rigctld command line to set the dial frequency (Hz).
pub fn freq_line(hz: u64) -> String {
    format!("F {hz}\n")
}
/// rigctld command line to set mode + passband (Hz; 0 = rig default).
pub fn mode_line(mode: &str, passband_hz: u32) -> String {
    format!("M {mode} {passband_hz}\n")
}
/// True if a rigctld reply indicates success (`RPRT 0`).
pub fn reply_ok(reply: &str) -> bool {
    reply.lines().any(|l| l.trim() == "RPRT 0")
}

/// A handle to the rig's keying/tuning.
pub struct Rig {
    mode: PttMode,
    stream: Option<TcpStream>,
    /// Lazily-opened serial port for [`PttMode::Serial`] (feature `serial`).
    #[cfg(feature = "serial")]
    serial: Option<Box<dyn serialport::SerialPort>>,
    /// Last PTT state we commanded (also lets callers/tests observe keying).
    pub keyed: bool,
}

impl Rig {
    pub fn new(mode: PttMode) -> Self {
        Self {
            mode,
            stream: None,
            #[cfg(feature = "serial")]
            serial: None,
            keyed: false,
        }
    }
    pub fn vox() -> Self {
        Self::new(PttMode::Vox)
    }
    pub fn rigctld(addr: &str) -> Self {
        Self::new(PttMode::Rigctld {
            addr: addr.to_string(),
        })
    }
    /// Key directly via a serial control line (RTS or DTR) on `port`.
    pub fn serial(port: &str, line: SerialLine) -> Self {
        Self::new(PttMode::Serial {
            port: port.to_string(),
            line,
        })
    }

    fn ensure_connected(&mut self) -> std::io::Result<&mut TcpStream> {
        if self.stream.is_none() {
            if let PttMode::Rigctld { addr } = &self.mode {
                let s = TcpStream::connect(addr)?;
                s.set_read_timeout(Some(Duration::from_millis(500)))?;
                s.set_write_timeout(Some(Duration::from_millis(500)))?;
                self.stream = Some(s);
            }
        }
        self.stream
            .as_mut()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "no rig stream"))
    }

    /// Send one command line and read whatever reply is available.
    fn command(&mut self, line: &str) -> std::io::Result<String> {
        let stream = self.ensure_connected()?;
        stream.write_all(line.as_bytes())?;
        let mut buf = [0u8; 256];
        let n = stream.read(&mut buf).unwrap_or(0);
        Ok(String::from_utf8_lossy(&buf[..n]).to_string())
    }

    /// Key (true) or unkey (false) the transmitter. No-op under VOX.
    pub fn ptt(&mut self, on: bool) -> std::io::Result<()> {
        self.keyed = on;
        match &self.mode {
            PttMode::Vox => Ok(()),
            PttMode::Serial { .. } => self.serial_ptt(on),
            PttMode::Rigctld { .. } => {
                let reply = self.command(&ptt_line(on))?;
                if reply_ok(&reply) || reply.is_empty() {
                    Ok(())
                } else {
                    Err(std::io::Error::other(format!(
                        "rigctld PTT error: {reply:?}"
                    )))
                }
            }
        }
    }

    /// Assert/deassert the configured serial PTT line.
    ///
    /// With the `serial` feature this lazily opens the port and drives RTS/DTR;
    /// without it, keying is logged and treated as a no-op so the engine can
    /// still run (effectively VOX) on a build with no serial backend.
    #[cfg(feature = "serial")]
    fn serial_ptt(&mut self, on: bool) -> std::io::Result<()> {
        let (port, line) = match &self.mode {
            PttMode::Serial { port, line } => (port.clone(), *line),
            _ => return Ok(()),
        };
        if self.serial.is_none() {
            // 1200 baud is arbitrary — we only toggle control lines, not data.
            let opened = serialport::new(&port, 1200)
                .timeout(Duration::from_millis(200))
                .open()?;
            self.serial = Some(opened);
        }
        let sp = self.serial.as_mut().unwrap();
        match line {
            SerialLine::Rts => sp.write_request_to_send(on)?,
            SerialLine::Dtr => sp.write_data_terminal_ready(on)?,
        }
        Ok(())
    }

    /// Serial PTT no-op fallback when built without the `serial` feature.
    #[cfg(not(feature = "serial"))]
    fn serial_ptt(&mut self, on: bool) -> std::io::Result<()> {
        if let PttMode::Serial { port, line } = &self.mode {
            eprintln!(
                "tempo-audio: serial PTT requested ({line:?} on {port}, key={on}) but the \
                 `serial` feature is not enabled — treating as VOX (no-op)."
            );
        }
        Ok(())
    }

    /// Set the dial frequency (Hz). No-op unless under rigctld CAT.
    pub fn set_freq(&mut self, hz: u64) -> std::io::Result<()> {
        if !matches!(self.mode, PttMode::Rigctld { .. }) {
            return Ok(());
        }
        self.command(&freq_line(hz)).map(|_| ())
    }

    /// Set the operating mode (e.g. "USB") + passband. A BLANK mode is a no-op —
    /// the caller is choosing to OBEY the radio's current mode (max compatibility),
    /// so Nexus sends no `M` command. Also a no-op unless under rigctld CAT.
    pub fn set_mode(&mut self, mode: &str, passband_hz: u32) -> std::io::Result<()> {
        if mode.trim().is_empty() {
            return Ok(());
        }
        if !matches!(self.mode, PttMode::Rigctld { .. }) {
            return Ok(());
        }
        self.command(&mode_line(mode, passband_hz)).map(|_| ())
    }

    /// Probe the rig by reading its current dial frequency (Hz) over CAT — the
    /// basis of a WSJT-X-style "Test CAT". Connects to rigctld and sends `f`,
    /// which replies with the frequency on its own line. Returns a descriptive
    /// error when rigctld is unreachable (connection refused) or the rig itself
    /// doesn't answer (bad baud / serial port / CAT disabled → no numeric reply).
    /// Only valid under [`PttMode::Rigctld`].
    pub fn read_freq(&mut self) -> std::io::Result<u64> {
        if !matches!(self.mode, PttMode::Rigctld { .. }) {
            return Err(std::io::Error::other("not a CAT rig"));
        }
        let reply = self.command("f\n")?;
        reply
            .lines()
            .find_map(|l| l.trim().parse::<u64>().ok())
            .filter(|hz| *hz > 0)
            .ok_or_else(|| {
                std::io::Error::other(format!(
                    "rig did not return a frequency (reply {reply:?}) — check the serial port, \
                     baud rate, and that CAT/CI-V is enabled on the rig"
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lines_match_rigctld_protocol() {
        assert_eq!(ptt_line(true), "T 1\n");
        assert_eq!(ptt_line(false), "T 0\n");
        assert_eq!(freq_line(14_074_000), "F 14074000\n");
        assert_eq!(mode_line("USB", 0), "M USB 0\n");
        assert!(reply_ok("RPRT 0\n"));
        assert!(!reply_ok("RPRT -1\n"));
    }

    #[test]
    fn vox_mode_keys_without_a_socket() {
        let mut rig = Rig::vox();
        rig.ptt(true).unwrap();
        assert!(rig.keyed);
        rig.ptt(false).unwrap();
        assert!(!rig.keyed);
        // freq/mode are also no-ops under VOX (no connection attempted).
        rig.set_freq(14_074_000).unwrap();
        rig.set_mode("USB", 0).unwrap();
    }

    // Without the `serial` feature, Serial PTT must fall back to a no-op (like
    // VOX) so the engine can run with no serial backend and no real port.
    #[cfg(not(feature = "serial"))]
    #[test]
    fn serial_mode_falls_back_to_vox_without_a_port() {
        let mut rig = Rig::serial("COM_DOES_NOT_EXIST", SerialLine::Rts);
        rig.ptt(true).unwrap();
        assert!(rig.keyed);
        rig.ptt(false).unwrap();
        assert!(!rig.keyed);
        // freq/mode are no-ops outside rigctld CAT — no connection attempted.
        rig.set_freq(14_074_000).unwrap();
        rig.set_mode("USB", 0).unwrap();
    }

    #[test]
    fn serial_constructor_sets_mode() {
        let rig = Rig::serial("COM5", SerialLine::Dtr);
        assert!(matches!(
            rig.mode,
            PttMode::Serial { ref port, line: SerialLine::Dtr } if port == "COM5"
        ));
        assert!(!rig.keyed);
    }

    // ---- Mock-rigctld round-trip harness (no hardware, runs in CI) ----------
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A throwaway mock rigctld: binds an ephemeral port, accepts one connection,
    /// replies to each command line via `reply`, and records every command for
    /// assertions. Models the rigctl line protocol (`f`→freq, `F`/`M`/`T`→RPRT).
    fn mock_rigctld(
        reply: impl Fn(&str) -> String + Send + 'static,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let log_w = log.clone();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 256];
                loop {
                    let n = match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    for line in text.lines() {
                        log_w.lock().unwrap().push(line.to_string());
                        if stream.write_all(reply(line).as_bytes()).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        (addr, log)
    }

    /// Healthy rig: `f`→`freq`, everything else→`RPRT 0`.
    fn ok_reply(freq: u64) -> impl Fn(&str) -> String + Send + 'static {
        move |line: &str| {
            if line.starts_with('f') {
                format!("{freq}\n")
            } else {
                "RPRT 0\n".to_string()
            }
        }
    }

    #[test]
    fn read_freq_parses_the_dial_over_tcp() {
        let (addr, log) = mock_rigctld(ok_reply(14_074_000));
        let mut rig = Rig::rigctld(&addr);
        assert_eq!(rig.read_freq().unwrap(), 14_074_000);
        assert_eq!(log.lock().unwrap().as_slice(), &["f".to_string()]);
    }

    #[test]
    fn set_freq_mode_ptt_send_correct_lines() {
        let (addr, log) = mock_rigctld(ok_reply(7_074_000));
        let mut rig = Rig::rigctld(&addr);
        rig.set_freq(7_074_000).unwrap();
        rig.set_mode("USB", 0).unwrap();
        rig.ptt(true).unwrap();
        assert!(rig.keyed);
        rig.ptt(false).unwrap();
        assert!(!rig.keyed);
        assert_eq!(
            *log.lock().unwrap(),
            vec!["F 7074000", "M USB 0", "T 1", "T 0"]
        );
    }

    #[test]
    fn ptt_errors_when_rig_reports_failure() {
        // rigctld answers RPRT -1 (e.g. CAT not ready) → ptt must surface an error.
        let (addr, _log) = mock_rigctld(|_l| "RPRT -1\n".to_string());
        let mut rig = Rig::rigctld(&addr);
        assert!(rig.ptt(true).is_err());
    }

    #[test]
    fn read_freq_errors_on_non_numeric_reply() {
        let (addr, _log) = mock_rigctld(|_l| "RPRT -1\n".to_string());
        let mut rig = Rig::rigctld(&addr);
        assert!(rig.read_freq().is_err());
    }

    #[test]
    fn read_freq_errors_when_rigctld_unreachable() {
        // Grab then drop a port so nothing is listening → connection refused.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        drop(l);
        let mut rig = Rig::rigctld(&addr);
        assert!(rig.read_freq().is_err());
    }
}
