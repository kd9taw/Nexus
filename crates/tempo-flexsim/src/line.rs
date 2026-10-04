//! Line assembly and the command grammar of the SmartSDR TCP API, shared by the simulator (which
//! reads client commands) and the recorder (which reads the radio's replies and status).
//!
//! The API is ASCII lines, and a TCP read can end anywhere: mid-key, mid-value, between `\r` and
//! `\n`. [`LineBuf`] holds bytes until a terminator arrives and hands on whole lines only, so a
//! partial line is never returned as a value. The simulator's split-line fault exists to catch a
//! client that skips this step.

use std::io::{self, ErrorKind};

/// Whether a failed read on a socket with a read timeout means "nothing yet, read again" rather
/// than a dead peer. Besides the timeout itself, Linux fails such a read with EINTR when the
/// process is stopped and continued, even with no signal handler installed (`man 7 signal`: socket
/// reads with `SO_RCVTIMEO` are not restarted after a stop signal). A Ctrl-Z and `fg` must not end
/// a recording or a simulated session.
pub fn read_again(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
    )
}

/// The longest line accepted before the peer is treated as broken. The same 16 MiB cap as the
/// session the port plan takes from AetherSDR (`RadioConnection`), so a peer that never sends a
/// terminator cannot grow the buffer without bound.
pub const MAX_LINE: usize = 16 * 1024 * 1024;

/// A peer sent more than [`MAX_LINE`] bytes without a terminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLong;

/// Bytes in, whole lines out.
#[derive(Debug, Default)]
pub struct LineBuf {
    buf: Vec<u8>,
}

impl LineBuf {
    /// Append `bytes` and return every line they complete, terminator stripped. A line ends at
    /// `\n` or `\r`, so `\r\n` works too (the documented command terminators are 0x0D, 0x0A and
    /// 0x0D 0x0A); the empty line between `\r` and `\n` is skipped. Bytes after the last
    /// terminator stay buffered for the next call.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>, TooLong> {
        let mut lines = Vec::new();
        for &b in bytes {
            if b == b'\n' || b == b'\r' {
                if !self.buf.is_empty() {
                    lines.push(String::from_utf8_lossy(&self.buf).into_owned());
                    self.buf.clear();
                }
            } else {
                if self.buf.len() >= MAX_LINE {
                    return Err(TooLong);
                }
                self.buf.push(b);
            }
        }
        Ok(lines)
    }

    /// Whether a partial line is waiting for its terminator.
    pub fn has_partial(&self) -> bool {
        !self.buf.is_empty()
    }
}

/// One client command: `C[D]<seq_number>|<command>` (SmartSDR-TCPIP-API, "Command Format").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The client's sequence number, echoed in the reply.
    pub seq: u32,
    /// The `D` flag: the client asked for debug output in the reply.
    pub debug: bool,
    /// The command text, trimmed.
    pub text: String,
}

/// Parse a client line strictly: `C`, an optional `D`, decimal digits that fit in 32 bits, `|`,
/// then the command. Anything else is `None`, never a command with a guessed sequence number.
pub fn parse_command(line: &str) -> Option<Command> {
    let rest = line.strip_prefix('C')?;
    let (debug, rest) = match rest.strip_prefix('D') {
        Some(r) => (true, r),
        None => (false, rest),
    };
    let (seq, text) = rest.split_once('|')?;
    if seq.is_empty() || !seq.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let seq = seq.parse::<u32>().ok()?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(Command {
        seq,
        debug,
        text: text.to_string(),
    })
}

/// Render a command line as a client sends it, terminator included.
pub fn command_line(seq: u32, text: &str) -> String {
    format!("C{seq}|{text}\n")
}

/// A reply from the radio: `R<seq_number>|<hex_response>|<message>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub seq: u32,
    /// The response code exactly as sent (hex digits; `0` is success).
    pub code: String,
    /// Everything after the second `|`, including any debug output.
    pub message: String,
}

/// Parse a reply line strictly: a decimal sequence number and a hex response code, or `None`.
pub fn parse_reply(line: &str) -> Option<Reply> {
    let rest = line.strip_prefix('R')?;
    let (seq, rest) = rest.split_once('|')?;
    if seq.is_empty() || !seq.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let seq = seq.parse::<u32>().ok()?;
    let (code, message) = rest.split_once('|').unwrap_or((rest, ""));
    if !is_hex_code(code) {
        return None;
    }
    Some(Reply {
        seq,
        code: code.to_string(),
        message: message.to_string(),
    })
}

/// Whether `code` is a well-formed response code: 1 to 8 hex digits.
pub fn is_hex_code(code: &str) -> bool {
    !code.is_empty() && code.len() <= 8 && code.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether a well-formed response code means success.
pub fn is_success(code: &str) -> bool {
    is_hex_code(code) && code.bytes().all(|b| b == b'0')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_cut_anywhere_comes_out_whole() {
        let line = b"S0|slice 0 RF_frequency=14.074000 mode=DIGU\n";
        for cut in 1..line.len() {
            let mut lb = LineBuf::default();
            let first = lb.push(&line[..cut]).unwrap();
            assert!(first.is_empty(), "cut at {cut} returned a partial line");
            assert!(lb.has_partial());
            let rest = lb.push(&line[cut..]).unwrap();
            assert_eq!(
                rest,
                vec!["S0|slice 0 RF_frequency=14.074000 mode=DIGU".to_string()]
            );
            assert!(!lb.has_partial());
        }
    }

    #[test]
    fn every_documented_terminator_ends_a_line() {
        let mut lb = LineBuf::default();
        let lines = lb.push(b"C1|ping\rC2|ping\nC3|ping\r\nC4|").unwrap();
        assert_eq!(lines, vec!["C1|ping", "C2|ping", "C3|ping"]);
        assert_eq!(lb.push(b"ping\n").unwrap(), vec!["C4|ping"]);
    }

    #[test]
    fn a_line_without_a_terminator_is_capped() {
        let mut lb = LineBuf::default();
        assert!(lb.push(&vec![b'x'; MAX_LINE]).is_ok());
        assert_eq!(lb.push(b"x"), Err(TooLong));
    }

    #[test]
    fn commands_parse_strictly() {
        assert_eq!(
            parse_command("C19|xmit 1"),
            Some(Command {
                seq: 19,
                debug: false,
                text: "xmit 1".into()
            })
        );
        assert_eq!(
            parse_command("CD7|ping").map(|c| (c.seq, c.debug)),
            Some((7, true))
        );
        for bad in [
            "C|ping",
            "Cx|ping",
            "C+1|ping",
            "C1 |ping",
            "C99999999999|ping",
            "C1|",
            "R1|0|",
            "ping",
        ] {
            assert_eq!(parse_command(bad), None, "{bad:?} parsed");
        }
    }

    #[test]
    fn replies_parse_strictly() {
        let r = parse_reply("R42|0|72E8C7F3-5766-4ADE-9286-EBF2F525C77|OK").unwrap();
        assert_eq!((r.seq, r.code.as_str()), (42, "0"));
        assert_eq!(r.message, "72E8C7F3-5766-4ADE-9286-EBF2F525C77|OK");
        assert!(is_success(&r.code));
        assert_eq!(
            parse_reply("R5|50000015").map(|r| r.message),
            Some(String::new())
        );
        assert!(!is_success("50000015"));
        for bad in [
            "R|0|",
            "Rx|0|",
            "R1|zz|",
            "R1||",
            "S0|interlock state=READY",
        ] {
            assert_eq!(parse_reply(bad), None, "{bad:?} parsed");
        }
    }
}
