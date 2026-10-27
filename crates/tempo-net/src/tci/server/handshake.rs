//! The handshake gate: what may become a program.
//!
//! # Web pages
//!
//! A browser does not apply the same-origin rule to WebSockets: any page the operator visits can
//! open `ws://127.0.0.1:40001`, and the server sees the page's `Origin` header. Native TCI clients
//! send none (Qt adds it only when the program set an origin, python-websockets and
//! gorilla/websocket only when the caller passes one), while a
//! browser must. So a handshake that carries `Origin` (or the old drafts' `Sec-WebSocket-Origin`)
//! is refused, 403, whatever else it says.
//!
//! # DNS rebinding
//!
//! A page can also point its own name at 127.0.0.1, so its requests look local. Its `Host` header
//! still carries its name: `Host` must be exactly one of `127.0.0.1:<port>`, `localhost:<port>` or
//! `[::1]:<port>` (403 otherwise; missing or repeated, too).
//!
//! # The rest
//!
//! tungstenite refuses what is not a WebSocket handshake before the gate runs (method, HTTP
//! version, upgrade headers, a key that is not 16 bytes of base64), and a version other than 13
//! (`Sec-WebSocket-Version`); it answers neither, so this module does: 426 with
//! `Sec-WebSocket-Version: 13` for the version, 400 for the rest. The whole handshake must arrive
//! inside [`Limits::handshake`] (tungstenite bounds its size, 64 KiB and 512 packets, but not its
//! time): a connection that trickles bytes is closed at the deadline, unanswered. A handshake past
//! the server's last free slot is answered 503.

use std::net::{SocketAddr, TcpStream};
use std::time::Instant;

use tungstenite::error::ProtocolError;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::{header, HeaderValue, StatusCode};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{HandshakeError, WebSocket};

use super::backend::{Event, HandshakeRefusal};
use super::conn::Wire;
use super::{Limits, Shared};

/// The answer to a version other than 13 (RFC 6455 §4.2.2).
const UPGRADE_REQUIRED: &[u8] = b"HTTP/1.1 426 Upgrade Required\r\nSec-WebSocket-Version: 13\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";

/// The answer to anything else that is not a WebSocket handshake.
const BAD_REQUEST: &[u8] =
    b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";

/// How much of a refused header the log keeps.
const SHOWN: usize = 100;

/// A header's value as the log may show it: printable ASCII, cut short.
fn shown(bytes: &[u8]) -> String {
    bytes
        .iter()
        .filter(|b| (0x20..=0x7e).contains(*b))
        .take(SHOWN)
        .map(|&b| char::from(b))
        .collect()
}

/// The origin a browser sent, if any header names one.
pub(super) fn origin(request: &Request) -> Option<String> {
    ["origin", "sec-websocket-origin"].iter().find_map(|name| {
        request
            .headers()
            .get_all(*name)
            .iter()
            .next()
            .map(|v| shown(v.as_bytes()))
    })
}

/// `Ok` when `Host` names this computer's loopback and `port`, exactly once. `Err` with what it
/// said.
pub(super) fn host(request: &Request, port: u16) -> Result<(), String> {
    let mut all = request.headers().get_all(header::HOST).iter();
    let value = match (all.next(), all.next()) {
        (Some(value), None) => value,
        (None, _) => return Err("(none)".into()),
        (Some(_), Some(_)) => return Err("(more than one)".into()),
    };
    let said = shown(value.as_bytes());
    let allowed = [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ];
    if value
        .to_str()
        .is_ok_and(|v| allowed.iter().any(|a| a.eq_ignore_ascii_case(v)))
    {
        Ok(())
    } else {
        Err(said)
    }
}

fn refusal(status: StatusCode) -> ErrorResponse {
    let mut response = ErrorResponse::new(None);
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(header::CONNECTION, HeaderValue::from_static("close"));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("0"));
    response
}

/// tungstenite's limits for a program's messages. Text has its own, smaller cap,
/// checked by the reader; tungstenite has one cap for both kinds.
pub(super) fn config(limits: &Limits) -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(limits.binary_message + 4096)
        // Every write goes straight to the outbox (`conn::Wire`).
        .write_buffer_size(0)
        .max_message_size(Some(limits.binary_message))
        .max_frame_size(Some(limits.binary_message))
}

/// Run the handshake on an accepted connection. `Some` with the WebSocket, the writer's handle on
/// the socket, and a handle for `shutdown`; `None` when it was refused (answered, and reported once
/// for the log). A client slot is held on `Some`.
pub(super) fn accept(
    shared: &Shared,
    tcp: TcpStream,
    peer: SocketAddr,
) -> Option<(WebSocket<Wire>, TcpStream, TcpStream)> {
    let (Ok(answer), Ok(handle)) = (tcp.try_clone(), tcp.try_clone()) else {
        return None;
    };
    let limits = &shared.limits;
    let _ = tcp.set_write_timeout(Some(limits.handshake));
    let deadline = Instant::now() + limits.handshake;
    let port = shared.port;
    let mut gate: Option<HandshakeRefusal> = None;
    let mut reserved = false;
    let callback = |request: &Request, response: Response| {
        if let Some(origin) = origin(request) {
            gate = Some(HandshakeRefusal::Origin(origin));
            return Err(refusal(StatusCode::FORBIDDEN));
        }
        if let Err(said) = host(request, port) {
            gate = Some(HandshakeRefusal::Host(said));
            return Err(refusal(StatusCode::FORBIDDEN));
        }
        if !shared.slots.take() {
            gate = Some(HandshakeRefusal::Full);
            return Err(refusal(StatusCode::SERVICE_UNAVAILABLE));
        }
        reserved = true;
        Ok(response)
    };
    let result = tungstenite::accept_hdr_with_config(
        Wire::handshake(tcp, deadline),
        callback,
        Some(config(limits)),
    );
    // Owned now: the callback, and its borrows, go with the handshake.
    let result = match result {
        Ok(ws) => Ok(ws),
        Err(HandshakeError::Failure(e)) => Err(Some(e)),
        Err(HandshakeError::Interrupted(_)) => Err(None),
    };
    let error = match result {
        Ok(ws) => return Some((ws, answer, handle)),
        Err(error) => error,
    };
    if reserved {
        shared.slots.give();
    }
    let why = gate.unwrap_or_else(|| {
        let (why, reply) = match error {
            Some(tungstenite::Error::Protocol(ProtocolError::MissingSecWebSocketVersionHeader)) => {
                (HandshakeRefusal::Version, Some(UPGRADE_REQUIRED))
            }
            Some(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::TimedOut => {
                (HandshakeRefusal::Timeout, None)
            }
            None => (HandshakeRefusal::Timeout, None),
            Some(tungstenite::Error::Io(_))
            | Some(tungstenite::Error::Protocol(ProtocolError::HandshakeIncomplete)) => {
                (HandshakeRefusal::Incomplete, None)
            }
            Some(_) => (HandshakeRefusal::Malformed, Some(BAD_REQUEST)),
        };
        if let Some(reply) = reply {
            use std::io::Write;
            let mut answer = &answer;
            let _ = answer.write_all(reply);
            let _ = answer.flush();
        }
        why
    });
    let _ = handle.shutdown(std::net::Shutdown::Both);
    shared.refused(Event::Refused { peer, why });
    None
}
