//! HRDLog.net upload transport (the `live` feature) — one authenticated POST to
//! `NewEntry.aspx`, over the operating system's TLS stack.
//!
//! All HRDLog knowledge (URL shape, form body, XML classification) lives in the
//! pure [`tempo_core::hrdlog`]; this module just moves bytes.
//!
//! **Why this connector does not use reqwest like every other one.** HRDLog's upload host
//! (IIS 7.5, Windows Server 2008 R2) offers TLS 1.2 only with ECDHE-RSA AES-CBC suites, DHE over
//! a 1024-bit group, or static-RSA key exchange. rustls supports none of those by design, so the
//! handshake could never succeed from any Nexus build: the server resets the TCP connection at the
//! ClientHello. This module uses the `native-tls` crate directly — SChannel on Windows,
//! Security.framework on macOS, OpenSSL on Linux — which negotiates a forward-secret
//! ECDHE-RSA suite with the certificate validated.
//!
//! **The scope is the point.** reqwest's own `native-tls` feature is NOT used: cargo unifies
//! features, and turning it on anywhere moves EVERY `reqwest::Client::builder()` in the app onto
//! the OS stack and its CBC suites (measured). `scripts/check-reqwest-rustls-only.sh` fails CI if
//! reqwest ever gains it. Here the OS stack is reachable only through [`post_form`], which refuses
//! every URL except [`tempo_core::hrdlog::HRDLOG_NEWENTRY_URL`] before touching the network.
//!
//! The contract kept from the reqwest transport this replaced:
//! - TLS 1.2 minimum (the server also accepts TLS 1.0, so this is the downgrade guard), and the
//!   platform's certificate + hostname validation, never disabled.
//! - No redirect is followed: a 302 is an error, as it was.
//! - 20 s connect/read/write timeouts. No retries here — the upload worker owns retrying.
//! - **Redacted errors.** The body carries the account upload code. Nothing here formats an error
//!   value, a URL, a host or the body: every failure is one fixed sentence ([`Failure::message`]).
//!
//! The TLS failure wording is decided by what the socket SAW, not by error text: a peer that closed
//! or reset the connection before the handshake finished *dropped* it; one that answered but could
//! not be verified *rejected* it. The shared `neterr` wording blamed antivirus or a proxy for
//! HRDLog's own reset, because reqwest's error chain cannot tell those apart.

use std::cell::Cell;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::rc::Rc;
use std::time::Duration;

/// The only host [`post_form`] connects to. Split out of the upload URL (a test pins the two
/// together) because TLS needs the bare name for SNI and certificate validation.
const HOST: &str = "robot.hrdlog.net";
const PORT: u16 = 443;
const PATH: &str = "/NewEntry.aspx";
const TIMEOUT: Duration = Duration::from_secs(20);
/// HRDLog's reply is a few hundred bytes of XML; a response past this is not that reply.
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// Why a request failed. Each variant maps to exactly one fixed sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    /// The URL was not HRDLog's upload address; no connection was attempted.
    Refused,
    /// The platform TLS library could not be set up.
    Init,
    /// DNS failed, or the TCP connect was refused or unreachable.
    Network,
    /// A connect, read or write ran past its timeout.
    Timeout,
    /// The peer closed or reset the connection before the TLS handshake finished.
    HandshakeDropped,
    /// The peer answered, but no verified TLS 1.2+ session could be made with it.
    HandshakeRejected,
    /// A non-2xx status — including a redirect, which is never followed.
    Status(u16),
    /// The request could not be written after the handshake.
    SendFailed,
    /// The response was cut short, malformed or oversized.
    BadResponse,
}

impl Failure {
    fn message(self) -> String {
        match self {
            Failure::Refused => {
                "HRDLog: refused to send — this connection only goes to HRDLog's upload address"
                    .to_string()
            }
            Failure::Init => "HRDLog: HTTP client initialization failed".to_string(),
            Failure::Network => "HRDLog: could not connect — check your network".to_string(),
            Failure::Timeout => "HRDLog: request timed out — try again shortly".to_string(),
            Failure::HandshakeDropped => {
                "HRDLog: the connection was dropped while setting up encryption, before your \
                 upload code was sent — try again later; if it keeps happening, export ADIF and \
                 upload it at hrdlog.net"
                    .to_string()
            }
            Failure::HandshakeRejected => {
                "HRDLog: could not set up a verified secure connection, so your upload code was \
                 not sent"
                    .to_string()
            }
            Failure::Status(code) => format!("HRDLog: server returned HTTP {code}"),
            Failure::SendFailed => "HRDLog: the connection failed while sending".to_string(),
            Failure::BadResponse => "HRDLog: could not read the response body".to_string(),
        }
    }
}

/// POST a `name=value` form body (built by [`tempo_core::hrdlog::build_upload_body`]) and return
/// the raw XML body for the caller to classify with `hrdlog::classify_response`.
///
/// `url` must be exactly [`tempo_core::hrdlog::HRDLOG_NEWENTRY_URL`]; anything else is refused
/// before any network use. `app_version` names the Nexus build in the User-Agent. `Err` only on a
/// transport failure or a non-2xx status, always **redacted** — never the URL, the code-bearing
/// body, or an error's own text.
pub fn post_form(url: &str, app_version: &str, body: String) -> Result<String, String> {
    post(url, app_version, &body).map_err(Failure::message)
}

fn post(url: &str, app_version: &str, body: &str) -> Result<String, Failure> {
    if url != tempo_core::hrdlog::HRDLOG_NEWENTRY_URL {
        return Err(Failure::Refused);
    }
    let mut tls = tls_connect(HOST, PORT, TIMEOUT)?;
    let (status, bytes) = exchange(&mut tls, &post_request(&user_agent(app_version), body))?;
    if !(200..300).contains(&status) {
        return Err(Failure::Status(status));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `Nexus/<version> (+project URL)`. Only version characters survive, because this lands in a
/// header line and a CR/LF would end the header block early. Also the pota.app spot post's.
pub(crate) fn user_agent(app_version: &str) -> String {
    let version: String = app_version
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        .collect();
    format!("Nexus/{version} (+https://github.com/kd9taw/Nexus)")
}

/// The whole request. Carries the upload code — never log it.
fn post_request(user_agent: &str, body: &str) -> Vec<u8> {
    format!(
        "POST {PATH} HTTP/1.1\r\nHost: {HOST}\r\nUser-Agent: {user_agent}\r\n\
         Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// What the socket saw, recorded by [`Watched`] and read back after a failed handshake — by then
/// the TLS library has consumed the stream, and its error is deliberately never inspected as text.
#[derive(Debug, Default, Clone, Copy)]
struct Seen {
    closed: bool,
    timed_out: bool,
}

/// A `TcpStream` that records EOF, resets and timeouts as they pass through it.
struct Watched {
    tcp: TcpStream,
    seen: Rc<Cell<Seen>>,
}

impl Watched {
    fn note<T>(&self, result: &io::Result<T>, eof: bool) {
        let mut seen = self.seen.get();
        match result {
            Ok(_) if eof => seen.closed = true,
            Ok(_) => {}
            Err(e) => match e.kind() {
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => seen.timed_out = true,
                io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::BrokenPipe
                | io::ErrorKind::UnexpectedEof => seen.closed = true,
                _ => {}
            },
        }
        self.seen.set(seen);
    }
}

impl Read for Watched {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let result = self.tcp.read(buf);
        let eof = !buf.is_empty() && matches!(result, Ok(0));
        self.note(&result, eof);
        result
    }
}

impl Write for Watched {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let result = self.tcp.write(buf);
        self.note(&result, false);
        result
    }

    fn flush(&mut self) -> io::Result<()> {
        self.tcp.flush()
    }
}

/// Open a verified TLS 1.2+ session to `host:port`. Private: outside this module's tests the only
/// caller is [`post`], with the module constants.
fn tls_connect(
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<native_tls::TlsStream<Watched>, Failure> {
    // Default validation: the OS trust store plus hostname verification. Never `danger_*`.
    let connector = native_tls::TlsConnector::builder()
        .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
        .build()
        .map_err(|_| Failure::Init)?;
    let tcp = connect_tcp(host, port, timeout)?;
    tcp.set_read_timeout(Some(timeout))
        .and_then(|()| tcp.set_write_timeout(Some(timeout)))
        .map_err(|_| Failure::Network)?;
    let seen = Rc::new(Cell::new(Seen::default()));
    let stream = Watched {
        tcp,
        seen: Rc::clone(&seen),
    };
    connector.connect(host, stream).map_err(|e| match e {
        // A blocking socket that hits its read timeout reports WouldBlock on Unix.
        native_tls::HandshakeError::WouldBlock(_) => Failure::Timeout,
        native_tls::HandshakeError::Failure(_) => {
            let seen = seen.get();
            if seen.timed_out {
                Failure::Timeout
            } else if seen.closed {
                Failure::HandshakeDropped
            } else {
                Failure::HandshakeRejected
            }
        }
    })
}

fn connect_tcp(host: &str, port: u16, timeout: Duration) -> Result<TcpStream, Failure> {
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|_| Failure::Network)?;
    let mut failure = Failure::Network;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(tcp) => return Ok(tcp),
            Err(e) if e.kind() == io::ErrorKind::TimedOut => failure = Failure::Timeout,
            Err(_) => {}
        }
    }
    Err(failure)
}

/// Write one request and read one response. Generic over the stream so the HTTP half is testable
/// over a plain loopback socket.
fn exchange<S: Read + Write>(stream: &mut S, request: &[u8]) -> Result<(u16, Vec<u8>), Failure> {
    stream
        .write_all(request)
        .and_then(|()| stream.flush())
        .map_err(|e| match e.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => Failure::Timeout,
            _ => Failure::SendFailed,
        })?;
    read_response(stream)
}

/// Read until [`frame`] finds a whole response.
fn read_response<R: Read>(reader: &mut R) -> Result<(u16, Vec<u8>), Failure> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let mut eof = false;
    loop {
        if let Some(response) = frame(&buf, eof)? {
            return Ok(response);
        }
        if eof {
            return Err(Failure::BadResponse);
        }
        match reader.read(&mut chunk) {
            Ok(0) => eof = true,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > MAX_RESPONSE_BYTES {
                    return Err(Failure::BadResponse);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(Failure::Timeout)
            }
            // Some TLS stacks report a close without close_notify as an error rather than EOF.
            // Treating it as EOF only completes a response that had no length to begin with;
            // a Content-Length or chunked body that is short still fails in `frame`.
            Err(_) => eof = true,
        }
    }
}

/// `Some((status, body))` once `buf` holds a whole response, `None` while more bytes are needed.
fn frame(buf: &[u8], eof: bool) -> Result<Option<(u16, Vec<u8>)>, Failure> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut response = httparse::Response::new(&mut headers);
    let head_len = match response.parse(buf) {
        Ok(httparse::Status::Complete(n)) => n,
        Ok(httparse::Status::Partial) => return Ok(None),
        Err(_) => return Err(Failure::BadResponse),
    };
    let status = response.code.ok_or(Failure::BadResponse)?;
    let header = |name: &str| {
        response
            .headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .and_then(|h| std::str::from_utf8(h.value).ok())
    };
    let rest = &buf[head_len..];
    if header("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked")) {
        return Ok(dechunk(rest)?.map(|body| (status, body)));
    }
    if let Some(len) = header("content-length") {
        let len: usize = len.trim().parse().map_err(|_| Failure::BadResponse)?;
        return Ok((rest.len() >= len).then(|| (status, rest[..len].to_vec())));
    }
    // Neither length nor chunking: the body runs to the close (we sent `Connection: close`).
    Ok(eof.then(|| (status, rest.to_vec())))
}

/// Decode a chunked body; `None` until the terminating zero-size chunk has arrived.
fn dechunk(mut data: &[u8]) -> Result<Option<Vec<u8>>, Failure> {
    let mut body = Vec::new();
    loop {
        let (consumed, size) = match httparse::parse_chunk_size(data) {
            Ok(httparse::Status::Complete(parsed)) => parsed,
            Ok(httparse::Status::Partial) => return Ok(None),
            Err(_) => return Err(Failure::BadResponse),
        };
        data = &data[consumed..];
        if size == 0 {
            return Ok(Some(body));
        }
        let size = usize::try_from(size).map_err(|_| Failure::BadResponse)?;
        let end = size.checked_add(2).ok_or(Failure::BadResponse)?;
        if data.len() < end {
            return Ok(None);
        }
        if &data[size..end] != b"\r\n" {
            return Err(Failure::BadResponse);
        }
        body.extend_from_slice(&data[..size]);
        data = &data[end..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::net::TcpListener;

    const BODY: &str = "Callsign=KD9TAW&Code=SECRETCODE&App=Nexus%2F1.12.0&ADIFData=%3Ceor%3E";

    /// Every operator-facing failure: prefixed, and carrying no code, host, address or blame.
    fn assert_clean(msg: &str) {
        assert!(msg.starts_with("HRDLog: "), "unexpected message: {msg}");
        assert!(!msg.contains("SECRETCODE"), "upload code leaked: {msg}");
        assert!(!msg.contains("robot.hrdlog"), "host leaked: {msg}");
        assert!(!msg.contains("127.0.0.1"), "address leaked: {msg}");
        assert!(
            !msg.contains("antivirus") && !msg.contains("proxy"),
            "blamed antivirus/proxy: {msg}"
        );
    }

    #[test]
    fn the_host_and_path_are_the_upload_url_constant() {
        assert_eq!(
            format!("https://{HOST}{PATH}"),
            tempo_core::hrdlog::HRDLOG_NEWENTRY_URL
        );
        assert_eq!(PORT, 443);
    }

    #[test]
    fn http_url_rejected_without_leaking_the_code_body() {
        let err = post_form(
            "http://robot.hrdlog.example/NewEntry.aspx",
            "1.12.0",
            BODY.to_string(),
        )
        .unwrap_err();
        assert_clean(&err);
        assert!(!err.contains("robot.hrdlog.example"), "URL leaked: {err}");
    }

    #[test]
    fn any_url_but_hrdlogs_upload_address_is_refused_before_connecting() {
        // A live listener stands in for "somewhere else": if the refusal ever let a request
        // through, it would arrive here. The accept below is the positive check that nothing did.
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        l.set_nonblocking(true).expect("nonblocking");
        for url in [
            format!("https://127.0.0.1:{port}/NewEntry.aspx"),
            "http://robot.hrdlog.net/NewEntry.aspx".to_string(),
            "https://logbook.qrz.com/NewEntry.aspx".to_string(),
            "https://robot.hrdlog.net.example/NewEntry.aspx".to_string(),
            "https://robot.hrdlog.net:8443/NewEntry.aspx".to_string(),
            "https://robot.hrdlog.net/NewEntry.aspx?next=https://example.com".to_string(),
            "https://robot.hrdlog.net/Other.aspx".to_string(),
            "HTTPS://ROBOT.HRDLOG.NET/NewEntry.aspx".to_string(),
        ] {
            assert_eq!(
                post(&url, "1.12.0", BODY),
                Err(Failure::Refused),
                "{url} was not refused"
            );
            assert_clean(&post_form(&url, "1.12.0", BODY.to_string()).unwrap_err());
        }
        assert!(
            matches!(l.accept(), Err(e) if e.kind() == io::ErrorKind::WouldBlock),
            "a refused URL still opened a connection"
        );
    }

    /// A server that takes the TCP connection and then drops it mid-handshake. `reset`: close with
    /// the ClientHello unread, which makes Linux answer with RST — what IIS/SChannel does to a
    /// ClientHello it shares no suite with. Otherwise read it and close cleanly (FIN).
    fn dropping_server(reset: bool) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                std::thread::sleep(Duration::from_millis(300));
                if !reset {
                    let mut buf = [0u8; 4096];
                    let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
                    let _ = s.read(&mut buf);
                }
            }
        });
        port
    }

    #[test]
    fn a_handshake_the_server_resets_is_reported_as_dropped_not_antivirus() {
        let err = tls_connect("127.0.0.1", dropping_server(true), Duration::from_secs(5))
            .err()
            .expect("a reset handshake must fail");
        assert_eq!(err, Failure::HandshakeDropped);
        let msg = err.message();
        assert_clean(&msg);
        assert!(msg.contains("dropped"), "{msg}");
    }

    #[test]
    fn a_handshake_the_server_closes_cleanly_is_reported_as_dropped() {
        let err = tls_connect("127.0.0.1", dropping_server(false), Duration::from_secs(5))
            .err()
            .expect("a closed handshake must fail");
        assert_eq!(err, Failure::HandshakeDropped);
    }

    #[test]
    fn a_peer_that_answers_but_is_not_a_tls_server_is_rejected_not_dropped() {
        // The control for the two above: the split must also NOT fire. This peer keeps the
        // connection open and answers in plain HTTP, so the handshake fails on what it SENT.
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
                let _ = s.flush();
                std::thread::sleep(Duration::from_millis(1500));
            }
        });
        let err = tls_connect("127.0.0.1", port, Duration::from_secs(5))
            .err()
            .expect("a non-TLS peer must fail");
        assert_eq!(err, Failure::HandshakeRejected);
        assert_clean(&err.message());
    }

    #[test]
    fn nothing_listening_says_check_your_network() {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        drop(l);
        let err = tls_connect("127.0.0.1", port, Duration::from_secs(5))
            .err()
            .expect("a refused connect must fail");
        assert_eq!(err, Failure::Network);
        assert_eq!(
            err.message(),
            "HRDLog: could not connect — check your network"
        );
    }

    #[test]
    fn a_server_that_never_answers_times_out() {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((_s, _)) = l.accept() {
                std::thread::sleep(Duration::from_secs(3));
            }
        });
        let err = tls_connect("127.0.0.1", port, Duration::from_millis(300))
            .err()
            .expect("a silent server must fail");
        assert_eq!(err, Failure::Timeout);
    }

    #[test]
    fn every_failure_message_is_fixed_and_redacted() {
        for f in [
            Failure::Refused,
            Failure::Init,
            Failure::Network,
            Failure::Timeout,
            Failure::HandshakeDropped,
            Failure::HandshakeRejected,
            Failure::Status(302),
            Failure::Status(500),
            Failure::SendFailed,
            Failure::BadResponse,
        ] {
            assert_clean(&f.message());
        }
    }

    /// A plain-HTTP loopback server that answers with `status` and echoes the whole request back
    /// as the body — the worst case for a leak, since the request carries the upload code.
    fn echo_server(status: &'static str) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
                let mut req = Vec::new();
                let mut buf = [0u8; 4096];
                while let Ok(n) = s.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    req.extend_from_slice(&buf[..n]);
                }
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    req.len()
                );
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&req);
            }
        });
        port
    }

    #[test]
    fn a_non_2xx_status_is_redacted_even_when_the_server_echoes_the_code() {
        let port = echo_server("500 Internal Server Error");
        let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let (status, body) =
            exchange(&mut s, &post_request(&user_agent("1.12.0"), BODY)).expect("exchange");
        assert_eq!(status, 500);
        assert!(
            String::from_utf8_lossy(&body).contains("SECRETCODE"),
            "control: the echo must carry the code, or this test proves nothing"
        );
        assert_clean(&Failure::Status(status).message());
    }

    #[test]
    fn the_request_is_one_well_formed_post_with_an_honest_user_agent() {
        let req = String::from_utf8(post_request(&user_agent("1.12.0"), BODY)).expect("utf8");
        let (head, body) = req.split_once("\r\n\r\n").expect("header/body split");
        assert!(
            head.starts_with("POST /NewEntry.aspx HTTP/1.1\r\n"),
            "{head}"
        );
        assert!(head.contains("\r\nHost: robot.hrdlog.net\r\n"), "{head}");
        assert!(
            head.contains("\r\nUser-Agent: Nexus/1.12.0 (+https://github.com/kd9taw/Nexus)"),
            "{head}"
        );
        assert!(!head.contains("propagation"), "{head}");
        assert!(head.contains(&format!("\r\nContent-Length: {}\r\n", BODY.len())));
        assert!(head.contains("\r\nConnection: close"));
        assert_eq!(body, BODY);
    }

    #[test]
    fn a_version_cannot_inject_a_header() {
        let ua = user_agent("1.12.0\r\nX-Evil: 1");
        assert!(!ua.contains('\r') && !ua.contains('\n'), "{ua}");
        assert_eq!(ua, "Nexus/1.12.0X-Evil1 (+https://github.com/kd9taw/Nexus)");
    }

    fn read(bytes: &[u8]) -> Result<(u16, Vec<u8>), Failure> {
        read_response(&mut Cursor::new(bytes.to_vec()))
    }

    #[test]
    fn responses_are_framed_by_length_chunks_or_close() {
        let xml = b"<HrdLog><NewEntry><insert>1</insert></NewEntry></HrdLog>";
        let mut by_length =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", xml.len()).into_bytes();
        by_length.extend_from_slice(xml);
        by_length.extend_from_slice(b"TRAILING GARBAGE");
        assert_eq!(read(&by_length), Ok((200, xml.to_vec())));

        let (a, b) = xml.split_at(15);
        let mut chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        for part in [a, b] {
            chunked.extend_from_slice(format!("{:x};ext=1\r\n", part.len()).as_bytes());
            chunked.extend_from_slice(part);
            chunked.extend_from_slice(b"\r\n");
        }
        chunked.extend_from_slice(b"0\r\n\r\n");
        assert_eq!(read(&chunked), Ok((200, xml.to_vec())));

        let mut to_close = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
        to_close.extend_from_slice(xml);
        assert_eq!(read(&to_close), Ok((200, xml.to_vec())));

        // A redirect is returned as a status, never followed.
        let redirect =
            b"HTTP/1.1 302 Found\r\nLocation: /Default.aspx\r\nContent-Length: 0\r\n\r\n";
        assert_eq!(read(redirect), Ok((302, Vec::new())));
    }

    #[test]
    fn short_malformed_or_oversized_responses_are_errors() {
        assert_eq!(
            read(b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\n<insert>1"),
            Err(Failure::BadResponse)
        );
        assert_eq!(
            read(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n10\r\nshort"),
            Err(Failure::BadResponse)
        );
        assert_eq!(
            read(b"HTTP/1.1 200 OK\r\nContent-"),
            Err(Failure::BadResponse)
        );
        assert_eq!(read(b"not http at all\r\n\r\n"), Err(Failure::BadResponse));
        let mut huge = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
        huge.resize(MAX_RESPONSE_BYTES + 10, b'x');
        assert_eq!(read(&huge), Err(Failure::BadResponse));
    }

    #[test]
    fn a_read_timeout_is_a_timeout() {
        struct Stalls;
        impl Read for Stalls {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::WouldBlock.into())
            }
        }
        assert_eq!(read_response(&mut Stalls), Err(Failure::Timeout));
    }

    // ---- Network tests: `#[ignore]`d, run on demand with
    // `cargo test -p propagation --features live -- --ignored live::hrdlog`.

    #[test]
    #[ignore = "network: robot.hrdlog.net"]
    fn live_rustls_still_cannot_reach_hrdlog_but_this_client_can() {
        // The natural positive control for the scope: a default reqwest client is on rustls, which
        // HRDLog refuses. If this ever succeeds, either reqwest gained native-tls (the CI guard
        // failed) or HRDLog modernised and this whole module can go.
        let rustls = reqwest::blocking::Client::builder()
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("client")
            .get(tempo_core::hrdlog::HRDLOG_NEWENTRY_URL)
            .send();
        assert!(rustls.is_err(), "rustls reached HRDLog: {rustls:?}");

        let mut tls = tls_connect(HOST, PORT, TIMEOUT).expect("native-tls handshake with HRDLog");
        // A harmless GET: no body, no code. HRDLog answers it with a redirect.
        let get = format!(
            "GET {PATH} HTTP/1.1\r\nHost: {HOST}\r\nUser-Agent: {}\r\nConnection: close\r\n\r\n",
            user_agent("test")
        );
        let (status, _) = exchange(&mut tls, get.as_bytes()).expect("GET over the handshake");
        println!("HRDLog answered the credential-free GET with HTTP {status}");
        assert!((200..400).contains(&status), "HTTP {status}");
    }

    #[test]
    #[ignore = "network: badssl.com"]
    fn live_bad_certificates_are_refused() {
        tls_connect("tls-v1-2.badssl.com", 1012, TIMEOUT).expect(
            "control: a valid TLS 1.2 host must connect, or the refusals below prove nothing",
        );
        for host in [
            "expired.badssl.com",
            "wrong.host.badssl.com",
            "self-signed.badssl.com",
            "untrusted-root.badssl.com",
        ] {
            let err = tls_connect(host, 443, TIMEOUT).err();
            assert_eq!(err, Some(Failure::HandshakeRejected), "{host}");
        }
    }

    #[test]
    #[ignore = "network: badssl.com"]
    fn live_a_tls_1_0_only_server_is_refused() {
        tls_connect("tls-v1-2.badssl.com", 1012, TIMEOUT)
            .expect("control: the TLS 1.2 twin must connect");
        let err = tls_connect("tls-v1-0.badssl.com", 1010, TIMEOUT).err();
        assert!(
            matches!(
                err,
                Some(Failure::HandshakeRejected | Failure::HandshakeDropped)
            ),
            "TLS 1.0 was accepted or failed oddly: {err:?}"
        );
    }
}
