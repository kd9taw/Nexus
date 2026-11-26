//! TCP ADIF output, the way JTDX sends a logged contact to a logger (#529): after each contact is
//! logged, connect to the logger's TCP port, send the contact as one ADIF record, read the reply,
//! close. Loggers made to take JTDX's feed (RadioGes RG40 among them) take this one.
//!
//! ## The framing, as JTDX sends it
//!
//! JTDX's `LogQSO::accept` (jtdx-project/jtdx, `logqso.cpp`) sends
//!
//! ```text
//! <command:3>Log <parameters:N> <CALL:5>W1ABC <MODE:3>FT8 … <EOR>
//! ```
//!
//! the record's fields separated by single spaces and ending `<EOR> ` (a space after it), and `N`
//! the length of everything after `<parameters:N> `, that last space included. It is DXLab's `log`
//! directive, which is why JTDX's default target is 127.0.0.1:52001, DXKeeper's port
//! ([`DEFAULT_PORT`]). JTDX waits one second to connect and one second for a reply, and a reply
//! that begins `NAK` is the logger refusing the contact; no reply at all is not an error.
//!
//! ## One difference: `N` counts bytes
//!
//! JTDX counts the record in UTF-16 units (Qt's `QString::length()`); DXLab's directive spec
//! counts bytes, as [`crate::dxkeeper`] does. For a record that is all ASCII, which nearly every
//! one is, the two are the same number. For an accented name or QTH they differ, and Nexus counts
//! bytes: a logger slicing the buffer by that count then gets the whole record.
//!
//! ## What a caller may do after a failure
//!
//! [`SendError::NotConnected`]: nothing left this machine, so trying again later cannot put the
//! contact in the logger twice. [`SendError::Unsure`]: the connection was made and the write then
//! failed, so the logger may have it. [`SendError::Refused`]: the logger answered `NAK`; sending
//! the same record again would be refused again.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// JTDX's default port: DXKeeper's (DXLab's Base Port 52000, plus one).
pub const DEFAULT_PORT: u16 = 52001;

/// How long to wait for the connection, and then for the logger's reply: JTDX's one second.
pub const TIMEOUT: Duration = Duration::from_secs(1);

/// The `Log` directive for one ADIF record. `adif_record` may or may not end with `<EOR>`; the
/// directive carries exactly one, inside the counted span, followed by JTDX's space. A record
/// that spans lines is put on one: a newline inside a field's value would be counted in its
/// length, and Nexus writes none there.
pub fn build_log_command(adif_record: &str) -> String {
    let trimmed = adif_record.trim();
    let body = trimmed
        .strip_suffix("<EOR>")
        .or_else(|| trimmed.strip_suffix("<eor>"))
        .unwrap_or(trimmed)
        .trim_end();
    let body: String = body
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let params = format!("{body} <EOR> ");
    format!("<command:3>Log <parameters:{}> {params}", params.len())
}

/// Why [`send_qso`] did not get as far as `Ok`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// No connection was made: nothing left this machine, and trying again cannot duplicate.
    NotConnected(String),
    /// Connected, and the write then failed: the logger may have the contact.
    Unsure(String),
    /// The logger answered `NAK`: it refused the contact. Its own words.
    Refused(String),
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConnected(e) | Self::Unsure(e) | Self::Refused(e) => f.write_str(e),
        }
    }
}

/// Send one contact: connect → the `Log` directive → read the reply for up to [`TIMEOUT`] →
/// close. `Ok` when the bytes were written and the logger did not answer `NAK` (a logger that
/// answers nothing is taken at its word, as JTDX takes it).
pub fn send_qso(host: &str, port: u16, adif_record: &str) -> Result<(), SendError> {
    let mut stream = connect(host, port).map_err(|e| {
        SendError::NotConnected(format!("TCP ADIF output: no logger on {host}:{port}: {e}"))
    })?;
    stream
        .write_all(build_log_command(adif_record).as_bytes())
        .and_then(|()| stream.flush())
        .map_err(|e| SendError::Unsure(format!("TCP ADIF output: send to {host}:{port}: {e}")))?;
    let reply = read_reply(&mut stream);
    if reply.starts_with("NAK") {
        return Err(SendError::Refused(format!(
            "TCP ADIF output: the logger on {host}:{port} refused the contact: {reply}"
        )));
    }
    Ok(())
}

fn connect(host: &str, port: u16) -> Result<TcpStream, String> {
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or_else(|| format!("could not resolve {host}"))?;
    let s = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(TIMEOUT)).ok();
    s.set_read_timeout(Some(TIMEOUT)).ok();
    Ok(s)
}

/// The logger's reply, trimmed and cut to a line's worth of printable text; empty when it sent
/// nothing within [`TIMEOUT`] or closed without a word.
fn read_reply(stream: &mut TcpStream) -> String {
    let mut buf = [0u8; 256];
    let n = stream.read(&mut buf).unwrap_or(0);
    String::from_utf8_lossy(&buf[..n])
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// The declared `<parameters:N>` and what follows it.
    fn declared_and_rest(msg: &str) -> (usize, &str) {
        let tag = "<parameters:";
        let i = msg.find(tag).expect("parameters field") + tag.len();
        let j = msg[i..].find('>').unwrap() + i;
        // JTDX puts one space between the tag and the record, outside the count.
        (msg[i..j].parse().unwrap(), &msg[j + 2..])
    }

    /// A logger on a local port that answers `reply` (nothing when empty) to each contact, and
    /// hands back what it was sent.
    fn logger(reply: &'static str) -> (u16, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let got = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            conn.set_read_timeout(Some(Duration::from_millis(300))).ok();
            let mut got = Vec::new();
            let mut buf = [0u8; 1024];
            while let Ok(n) = conn.read(&mut buf) {
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
                if got.ends_with(b"<EOR> ") {
                    break;
                }
            }
            if !reply.is_empty() {
                conn.write_all(reply.as_bytes()).unwrap();
            }
            String::from_utf8(got).unwrap()
        });
        (port, got)
    }

    #[test]
    fn the_directive_is_jtdxs_log_with_one_eor_and_its_trailing_space_counted() {
        let msg = build_log_command("<CALL:5>W1ABC <MODE:3>FT8 <EOR>\n");
        assert_eq!(
            msg,
            "<command:3>Log <parameters:32> <CALL:5>W1ABC <MODE:3>FT8 <EOR> "
        );
        let (n, rest) = declared_and_rest(&msg);
        assert_eq!(n, rest.len());
        assert_eq!(msg.matches("<EOR>").count(), 1);
        // A record without its terminator gets one; one with a lower-case terminator gets one.
        assert_eq!(build_log_command("<CALL:5>W1ABC <MODE:3>FT8"), msg);
        assert_eq!(build_log_command("<CALL:5>W1ABC <MODE:3>FT8<eor>"), msg);
    }

    #[test]
    fn a_record_on_several_lines_goes_on_one() {
        let msg = build_log_command("<CALL:5>W1ABC\r\n<MODE:3>FT8\n<EOR>\n");
        assert_eq!(
            msg,
            "<command:3>Log <parameters:32> <CALL:5>W1ABC <MODE:3>FT8 <EOR> "
        );
    }

    #[test]
    fn the_count_is_bytes_for_an_accented_name() {
        let msg = build_log_command("<CALL:5>EA5AR <NAME:5>José <EOR>");
        let (n, rest) = declared_and_rest(&msg);
        assert_eq!(n, rest.len());
        assert!(n > rest.chars().count(), "bytes exceed chars here");
    }

    /// ⭐ A logger listening on a local port gets the contact, framed as JTDX frames it.
    #[test]
    fn a_listening_logger_gets_the_contact() {
        let (port, got) = logger("");
        send_qso("127.0.0.1", port, "<CALL:5>W1ABC <MODE:3>FT8 <EOR>").expect("sent");
        assert_eq!(
            got.join().unwrap(),
            "<command:3>Log <parameters:32> <CALL:5>W1ABC <MODE:3>FT8 <EOR> "
        );
    }

    #[test]
    fn a_nak_is_a_refusal_in_the_loggers_words_and_an_ack_is_fine() {
        let (port, got) = logger("NAK duplicate");
        let err = send_qso("127.0.0.1", port, "<CALL:5>W1ABC <EOR>").unwrap_err();
        assert!(
            matches!(&err, SendError::Refused(w) if w.ends_with("NAK duplicate")),
            "{err}"
        );
        got.join().unwrap();
        let (port, got) = logger("ACK");
        assert_eq!(send_qso("127.0.0.1", port, "<CALL:5>W1ABC <EOR>"), Ok(()));
        got.join().unwrap();
    }

    /// ⛔ A logger that is not there costs no more than the timeout, and is told apart from a
    /// send: nothing left, so the caller may try again without a duplicate.
    #[test]
    fn no_logger_is_not_connected_and_quick() {
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let started = std::time::Instant::now();
        let err = send_qso("127.0.0.1", port, "<CALL:5>W1ABC <EOR>").unwrap_err();
        assert!(matches!(err, SendError::NotConnected(_)), "{err}");
        assert!(started.elapsed() <= TIMEOUT + Duration::from_millis(500));
    }
}
