//! The session socket's receive rule (`session::receive_ends_session`) against the other reports
//! Windows makes about ONE datagram on an unconnected UDP socket. Like the ICMP port-unreachable it
//! reports as `ConnectionReset`, each is about that datagram, not the socket, which is unharmed. A
//! security review (2026-10-03) found that the rule still ended a working stream on both:
//! - WSAEMSGSIZE (10040): a datagram longer than the receive buffer. The session loop reads into
//!   2048 bytes (`src-tauri/src/remote_service/stream.rs`, `run`), so one datagram of 2049 bytes or
//!   more, from anyone who can reach the session socket, ended the stream. Windows hands back the
//!   part that fit with the error; the loop drops it, and no part of it reaches str0m.
//! - WSAENETRESET (10052): "For a datagram socket, this error indicates that the time to live has
//!   expired" (Microsoft's `recvfrom` reference): an ICMP time-exceeded for an earlier ICE check.
//!
//! Elsewhere an oversized datagram is truncated and the receive succeeds, so the first test passes
//! on Linux; the second is about Windows error codes and runs only there. CI has no Windows runner
//! for them: cross-build with `cargo test -p tempo-stream --target x86_64-pc-windows-gnu --no-run
//! --test receive_errors` (with `VPX_MINGW_PREFIX`), then run the `.exe` on Windows (under WSL,
//! straight from bash).
use std::net::UdpSocket;
use std::time::Duration;

use tempo_stream::session::receive_ends_session;

/// The session loop's receive buffer (`stream.rs::run`: `vec![0u8; 2048]`).
const SESSION_BUFFER: usize = 2048;

/// One datagram longer than the buffer, sent to the session socket, must not end the session: the
/// socket still receives the next one. CONTROL: a datagram that fits is read whole.
#[test]
fn a_datagram_longer_than_the_session_buffer_is_not_the_end_of_the_session() {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let to = socket.local_addr().unwrap();
    let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut buf = vec![0u8; SESSION_BUFFER];

    peer.send_to(&[7u8; SESSION_BUFFER], to).unwrap();
    let (n, _) = socket
        .recv_from(&mut buf)
        .expect("CONTROL: a datagram that fits");
    assert_eq!(
        n, SESSION_BUFFER,
        "CONTROL: a datagram that fits is read whole"
    );

    peer.send_to(&[7u8; SESSION_BUFFER + 1], to).unwrap();
    if let Err(refused) = socket.recv_from(&mut buf) {
        assert!(
            !receive_ends_session(&refused),
            "one datagram of {} bytes ended the session: {refused} (os error {:?}, {:?})",
            SESSION_BUFFER + 1,
            refused.raw_os_error(),
            refused.kind()
        );
    }

    peer.send_to(b"next", to).unwrap();
    let (n, _) = socket
        .recv_from(&mut buf)
        .expect("the socket still receives");
    assert_eq!(&buf[..n], b"next");
}

/// An ICE check that met a routing loop or a too-short path comes back as ICMP time-exceeded, which
/// Windows reports on the next receive as WSAENETRESET. Like a reset, it is the destination's word
/// about one datagram. CONTROLS: WSAECONNRESET, which the rule already let pass, and WSAENETDOWN,
/// the network itself gone, which still ends the session.
#[cfg(windows)]
#[test]
fn a_check_whose_time_to_live_ran_out_is_not_the_end_of_the_session() {
    let reset = std::io::Error::from_raw_os_error(10054);
    assert!(!receive_ends_session(&reset), "CONTROL: {reset}");
    let expired = std::io::Error::from_raw_os_error(10052);
    assert!(
        !receive_ends_session(&expired),
        "an expired time to live ended the session: {expired} ({:?})",
        expired.kind()
    );
    let down = std::io::Error::from_raw_os_error(10050);
    assert!(receive_ends_session(&down), "CONTROL: {down} still ends it");
}
