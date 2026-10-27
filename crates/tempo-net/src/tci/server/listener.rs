//! Where the server listens: this computer's loopback only, IPv4 and IPv6, never a
//! wildcard and never a LAN address (operator ruling: loopback only in this release). A program
//! that resolves `localhost` to `::1` first still reaches it.
//!
//! # The port
//!
//! The operator's port, 40001 by default: WSJT-X's, JTDX's, the Go TCI library's and
//! MacLoggerDX's own default, and clear of the 50001 that Thetis and AetherSDR listen on. A second
//! Nexus window (a second radio) finds it taken and takes the next one: [`PORTS_TRIED`] ports are
//! tried in turn, each on both addresses, and the server reports the one it got. A port held on
//! either address is skipped whole, so the two listeners always share one number.

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use super::backend::{Event, HandshakeRefusal};
use super::{conn, Shared};

/// How many ports from the configured one are tried: the operator's, then the next one for a
/// second window.
pub const PORTS_TRIED: u16 = 2;

/// How often the accept loop looks for a connection, and for the stop.
const ACCEPT_POLL: Duration = Duration::from_millis(50);

/// The loopback listeners on one port: IPv4 always, IPv6 when this computer has `::1`. Port 0
/// asks the system for a free one (the tests).
pub(super) fn bind(port: u16) -> io::Result<Vec<TcpListener>> {
    let tries: Vec<u16> = if port == 0 {
        vec![0; 8]
    } else {
        (0..PORTS_TRIED)
            .filter_map(|i| port.checked_add(i))
            .collect()
    };
    let mut last = io::Error::new(io::ErrorKind::AddrInUse, "no port to try");
    for p in tries {
        let v4 = match TcpListener::bind((Ipv4Addr::LOCALHOST, p)) {
            Ok(l) => l,
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
                last = e;
                continue;
            }
            Err(e) => return Err(e),
        };
        let p = v4.local_addr()?.port();
        match TcpListener::bind((Ipv6Addr::LOCALHOST, p)) {
            Ok(v6) => return Ok(vec![v4, v6]),
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => last = e,
            // No IPv6 loopback on this computer: IPv4 alone.
            Err(_) => return Ok(vec![v4]),
        }
    }
    Err(last)
}

/// Accept until the server stops. Each connection gets its own thread for its handshake, which
/// then becomes its reader (`conn::serve`); at most [`super::Limits::pending`] handshakes run at
/// once.
pub(super) fn accept_loop(shared: Arc<Shared>, listeners: Vec<TcpListener>) {
    for l in &listeners {
        let _ = l.set_nonblocking(true);
    }
    while !shared.stopping() {
        let mut idle = true;
        for l in &listeners {
            match l.accept() {
                Ok((tcp, peer)) => {
                    idle = false;
                    admit(&shared, tcp, peer);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                // A connection reset before it was accepted, or out of descriptors: the next poll
                // tries again, after the pause below.
                Err(_) => {}
            }
        }
        if idle {
            std::thread::sleep(ACCEPT_POLL);
        }
    }
    // The listeners drop here: the port is free again when `Server::stop` returns.
}

fn admit(shared: &Arc<Shared>, tcp: TcpStream, peer: SocketAddr) {
    if !peer.ip().to_canonical().is_loopback() {
        shared.refused(Event::Refused {
            peer,
            why: HandshakeRefusal::NotLocal,
        });
        return;
    }
    if !shared.pending.take() {
        shared.refused(Event::Refused {
            peer,
            why: HandshakeRefusal::Busy,
        });
        return;
    }
    // WinSock hands an accepted socket the listener's non-blocking mode; the reader wants
    // blocking reads with a timeout. Nagle would hold a small answer back behind an unacknowledged
    // one: a `trx` echo or a TX_CHRONO must go at once.
    let _ = tcp.set_nonblocking(false);
    let _ = tcp.set_nodelay(true);
    let worker = Arc::clone(shared);
    let spawned = std::thread::Builder::new()
        .name("tci-in".into())
        .spawn(move || conn::serve(worker, tcp, peer));
    if spawned.is_err() {
        shared.pending.give();
    }
}

/// The addresses a set of listeners is bound to.
pub(super) fn addrs(listeners: &[TcpListener]) -> Vec<SocketAddr> {
    listeners
        .iter()
        .filter_map(|l| l.local_addr().ok())
        .collect()
}
