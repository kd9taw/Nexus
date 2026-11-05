//! UDP transport for the WSJT-X-compatible protocol.
//!
//! [`WsjtxServer`] binds a local UDP socket and emits [`crate::wsjtx`] datagrams
//! to a configured target (the WSJT-X default is `127.0.0.1:2237`; a multicast
//! group also works for several consumers at once). It also polls the socket
//! non-blocking for inbound control datagrams (Reply / HaltTx / FreeText).
//!
//! All datagrams carry the sender id `"Tempo"`.

use crate::wsjtx::{self, Decode, Inbound, QsoLogged, Status};
use std::io;
use std::net::{IpAddr, SocketAddr, UdpSocket};

// A door for tests only: a shipped build reads each sender's address from the socket, always.
#[cfg(all(feature = "udp-source-sim", not(debug_assertions)))]
compile_error!("`udp-source-sim` is test support: a release build must not turn it on");

/// The sender id ("key") Tempo announces itself with.
pub const APP_ID: &str = "Tempo";

/// A bound UDP socket that speaks the WSJT-X protocol to a target address.
pub struct WsjtxServer {
    socket: UdpSocket,
    /// Every address each datagram is sent to. One socket, many targets — so a single Nexus
    /// can feed a LOCAL tool (GridTracker / JTAlert) AND a REMOTE service (an FT8 contest
    /// scorer at a public IP) at once, which WSJT-X's single-sink UDP cannot. Always
    /// non-empty. Inbound control (Reply / FreeText) still arrives on the one bound socket,
    /// so [`WsjtxServer::poll`] is unaffected by the target count.
    targets: Vec<SocketAddr>,
    id: String,
    /// What a test reads each sender's address as ([`WsjtxServer::read_sources_as`]).
    #[cfg(any(test, feature = "udp-source-sim"))]
    sim_source: Option<IpAddr>,
}

/// ⛔ Whether a request this station has already admitted ([`WsjtxServer::poll_from`]) is refused
/// because it comes from another computer: it could arm or key the transmitter, choose what the
/// next over says, or change what the station sends, and the operator has not said such requests
/// may come from off this computer (`accept_from_other_computers`). The operator's own loggers on
/// this computer send from loopback and are never refused here. The match has no wildcard: a new
/// message type must say which it is.
///
/// - **Reply** works a station and arms TX. **FreeText** chooses the next over, or fills Tx 5
///   ready for one, sent or not. **Location** moves the grid the station's messages carry.
/// - **HaltTx** only ever stops. **Clear** blanks panes, **HighlightCallsign** colours rows, and
///   **Replay** re-sends decodes to the operator's own targets, at a bounded rate.
/// - **Decode**, **QsoLogged** and **Other** do nothing here; `Other` includes Close,
///   SwitchConfiguration and Configure, which Nexus does not act on from anyone.
/// - **LoggedAdif** goes to the log's import. It reaches no transmit path.
pub fn refused_from_other_computer(
    inbound: &Inbound,
    from: IpAddr,
    accept_from_other_computers: bool,
) -> bool {
    let shapes_a_transmission = match inbound {
        Inbound::Reply { .. } | Inbound::FreeText { .. } | Inbound::Location { .. } => true,
        Inbound::HaltTx { .. }
        | Inbound::Clear { .. }
        | Inbound::Replay { .. }
        | Inbound::HighlightCallsign { .. }
        | Inbound::Decode { .. }
        | Inbound::QsoLogged { .. }
        | Inbound::LoggedAdif { .. }
        | Inbound::Other { .. } => false,
    };
    shapes_a_transmission && !accept_from_other_computers && !from.is_loopback()
}

impl WsjtxServer {
    /// Bind a UDP socket at `bind` and send to `target`.
    ///
    /// - `bind` is typically `0.0.0.0:0` (ephemeral) for pure output, or a fixed
    ///   port if you also want to receive control datagrams at a known address.
    /// - `target` is where datagrams go; if it is a multicast group address the
    ///   socket is configured to allow multicast send.
    ///
    /// The socket is set non-blocking so [`WsjtxServer::poll`] never stalls the
    /// caller's loop.
    pub fn new(bind: SocketAddr, target: SocketAddr) -> io::Result<Self> {
        Self::new_multi(bind, vec![target])
    }

    /// Bind once and send every datagram to EACH of `targets`. Empty is rejected (there would
    /// be nowhere to send). Any multicast target arms multicast send on the socket.
    pub fn new_multi(bind: SocketAddr, targets: Vec<SocketAddr>) -> io::Result<Self> {
        if targets.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "WsjtxServer needs at least one target",
            ));
        }
        let socket = UdpSocket::bind(bind)?;
        socket.set_nonblocking(true)?;
        for t in &targets {
            if t.ip().is_multicast() {
                // Permit sending to a multicast group (and loop it back locally so a
                // consumer on the same host still hears us).
                socket.set_multicast_loop_v4(true).ok();
                if let SocketAddr::V4(v4) = t {
                    socket
                        .join_multicast_v4(v4.ip(), &std::net::Ipv4Addr::UNSPECIFIED)
                        .ok();
                }
            }
        }
        Ok(Self {
            socket,
            targets,
            id: APP_ID.to_string(),
            #[cfg(any(test, feature = "udp-source-sim"))]
            sim_source: None,
        })
    }

    /// Test support: read every inbound datagram as sent from `source`, at the source check and
    /// in what [`WsjtxServer::poll_from`] returns. A test box may have no address but loopback,
    /// and a sender's address cannot be set from a test's own socket.
    #[cfg(any(test, feature = "udp-source-sim"))]
    pub fn read_sources_as(&mut self, source: IpAddr) {
        self.sim_source = Some(source);
    }

    /// The local address the socket is bound to (useful in tests to learn the
    /// ephemeral port).
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// The first configured target address (compatibility for single-target callers/tests).
    pub fn target(&self) -> SocketAddr {
        self.targets[0]
    }

    /// Every configured target address.
    pub fn targets(&self) -> &[SocketAddr] {
        &self.targets
    }

    /// Send to EVERY target. One dead target (a service that is down) must not stop the
    /// others, so a per-target failure is remembered but the loop continues; the call fails
    /// only if not one target accepted the datagram.
    fn send(&self, bytes: &[u8]) -> io::Result<()> {
        let mut any_ok = false;
        let mut last_err = None;
        for t in &self.targets {
            match self.socket.send_to(bytes, t) {
                Ok(_) => any_ok = true,
                Err(e) => last_err = Some(e),
            }
        }
        if any_ok {
            Ok(())
        } else {
            Err(last_err.unwrap_or_else(|| io::Error::other("no WSJT-X targets")))
        }
    }

    /// Send a Heartbeat. `version`/`revision` describe this Tempo build.
    pub fn send_heartbeat(&self, max_schema: u32, version: &str, revision: &str) -> io::Result<()> {
        self.send(&wsjtx::encode_heartbeat(
            &self.id, max_schema, version, revision,
        ))
    }

    /// Send a Status update.
    pub fn send_status(&self, status: &Status) -> io::Result<()> {
        self.send(&wsjtx::encode_status(&self.id, status))
    }

    /// Send a Decode (a heard signal).
    pub fn send_decode(&self, decode: &Decode) -> io::Result<()> {
        self.send(&wsjtx::encode_decode(&self.id, decode))
    }

    /// Send a QSOLogged record (a completed contact).
    pub fn send_qso_logged(&self, qso: &QsoLogged) -> io::Result<()> {
        self.send(&wsjtx::encode_qso_logged(&self.id, qso))
    }

    /// Send a LoggedADIF (type 12) for one contact — WSJT-X sends it right after QSOLogged.
    /// `adif_record` is one ADIF record; see [`wsjtx::encode_logged_adif`].
    pub fn send_logged_adif(&self, adif_record: &str) -> io::Result<()> {
        self.send(&wsjtx::encode_logged_adif(&self.id, adif_record))
    }

    /// Send a Close (Tempo is shutting down).
    /// **Clear (type 3)** — tell consumers (JTAlert/GridTracker) we erased a
    /// decode window so they clear theirs: 0 = Band Activity, 1 = Rx Frequency,
    /// 2 = both.
    pub fn send_clear(&self, window: u8) -> io::Result<()> {
        self.send(&wsjtx::encode_clear(&self.id, window))
    }

    pub fn send_close(&self) -> io::Result<()> {
        self.send(&wsjtx::encode_close(&self.id))
    }

    /// Non-blocking poll for one inbound control datagram.
    ///
    /// Returns:
    /// - `Ok(Some(inbound))` when a recognized WSJT-X datagram arrived,
    /// - `Ok(None)` when nothing is waiting (`WouldBlock`) or the datagram was
    ///   not a valid WSJT-X frame (silently ignored),
    /// - `Err(_)` only on a genuine socket error.
    pub fn poll(&self) -> io::Result<Option<Inbound>> {
        Ok(self.poll_from()?.map(|(inbound, _)| inbound))
    }

    /// [`WsjtxServer::poll`], with the address the datagram came from, for
    /// [`refused_from_other_computer`] to read.
    pub fn poll_from(&self) -> io::Result<Option<(Inbound, IpAddr)>> {
        let mut buf = [0u8; 4096];
        match self.socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                #[cfg(any(test, feature = "udp-source-sim"))]
                let from = SocketAddr::new(self.sim_source.unwrap_or(from.ip()), from.port());
                // Only accept control from loopback or the configured peer. A datagram
                // from any other source is dropped before it is even parsed. This check
                // is not what lets a request shape a transmission: a source it admits can
                // be forged on a LAN and is anyone at all under a multicast target, so
                // [`refused_from_other_computer`] rules on what is admitted here.
                //
                // Multicast is the exception: when the target is a group the
                // operator explicitly joined, inbound datagrams arrive sourced
                // from members' unicast interface IPs (never the group address),
                // so peer-equality can't apply — group membership IS the opt-in,
                // so accept. Loopback/unicast targets keep the strict filter.
                // With several targets, a trusted source is ANY of them (or loopback, or
                // any target being a multicast group the operator joined) — the same
                // per-target trust model, applied across the list.
                let allowed = from.ip().is_loopback()
                    || self
                        .targets
                        .iter()
                        .any(|t| t.ip().is_multicast() || from.ip() == t.ip());
                if allowed {
                    Ok(wsjtx::parse_inbound(&buf[..n]).map(|inbound| (inbound, from.ip())))
                } else {
                    Ok(None)
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Drain up to `max` inbound control datagrams, stopping early when the
    /// socket runs dry or a datagram fails to parse (same stop condition
    /// [`WsjtxServer::poll`] signals with `Ok(None)`).
    ///
    /// The cap is the point. The caller — the radio loop — processes each
    /// inbound datagram while holding the engine lock, and a HaltTx costs a
    /// blocking CAT round trip (up to 2500 ms on slow serial). An unbounded
    /// `while let Ok(Some(_)) = poll()` therefore let ONE misbehaving consumer
    /// (a logger stuck in a HaltTx retry loop) hold the engine mutex for as long
    /// as it kept sending, which starves every Tauri command behind it. Bounding
    /// the batch bounds the tick. Nothing is dropped here: the remainder stays
    /// in the socket buffer for the next tick, milliseconds later.
    ///
    /// Each comes with the address it came from ([`WsjtxServer::poll_from`]).
    pub fn drain(&self, max: usize) -> impl Iterator<Item = (Inbound, IpAddr)> + '_ {
        (0..max).map_while(|_| self.poll_from().ok().flatten())
    }
}

/// Inbound control datagrams one radio-loop tick will process. At the loop's
/// 20 ms cadence this is ~3200/s — orders of magnitude past any real consumer
/// (WSJT-X companions send a handful per QSO), so it never throttles honest
/// traffic; it only stops a flood from owning the engine lock. See
/// [`WsjtxServer::drain`].
pub const INBOUND_PER_TICK: usize = 64;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qds::QdsWriter;
    use std::net::Ipv4Addr;

    fn loopback(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    /// End-to-end over loopback: a server sends to a listener socket, which
    /// decodes the datagram. No real network, just 127.0.0.1.
    #[test]
    fn status_roundtrips_over_loopback() {
        let listener = UdpSocket::bind(loopback(0)).unwrap();
        listener
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let target = listener.local_addr().unwrap();

        let server = WsjtxServer::new(loopback(0), target).unwrap();
        let status = Status {
            dial_freq: 14_074_000,
            mode: "FT8",
            de_call: "KD9TAW",
            de_grid: "EN52",
            special_op: 3,
            tr_period: 15,
            config_name: "Default",
            ..Default::default()
        };
        server.send_status(&status).unwrap();

        let mut buf = [0u8; 4096];
        let (n, _) = listener.recv_from(&mut buf).unwrap();
        // It parses as a WSJT-X frame and is the Status type ("Other" since the
        // inbound parser doesn't model Status payloads — but header is valid).
        match wsjtx::parse_inbound(&buf[..n]).unwrap() {
            Inbound::Other { id, message_type } => {
                assert_eq!(id, APP_ID);
                assert_eq!(message_type, wsjtx::msg_type::STATUS);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    /// ISSUE #37. The datagram a LOGGER logs from — N1MM+, HRD and Log4OM all consume type 5.
    /// `send_qso_logged` had exactly one call site in the whole tree, inside the Field Day block
    /// of the radio loop, so an ordinary contact put Status and Decode on the wire and never
    /// this. The link looked alive (N1MM's WSJT decode window fills) and logged nothing.
    ///
    /// This proves the delivery end: it really leaves the socket and really parses back as a
    /// QsoLogged carrying the call we sent — not merely that the encoder was called.
    /// The FT8-Battle-Royale ask (2026-09-03): one Nexus feeding a LOCAL tool AND a remote
    /// service at once. `new_multi` must deliver each datagram to EVERY target — and only to
    /// the targets, never to an uninvolved socket.
    #[test]
    fn qso_logged_reaches_every_target_and_no_bystander() {
        let a = UdpSocket::bind(loopback(0)).unwrap();
        let b = UdpSocket::bind(loopback(0)).unwrap();
        let bystander = UdpSocket::bind(loopback(0)).unwrap();
        for l in [&a, &b, &bystander] {
            l.set_read_timeout(Some(std::time::Duration::from_millis(500)))
                .unwrap();
        }
        let ta = a.local_addr().unwrap();
        let tb = b.local_addr().unwrap();
        let server = WsjtxServer::new_multi(loopback(0), vec![ta, tb]).unwrap();
        let qso = QsoLogged {
            time_off: 1_700_000_100,
            dx_call: "W1AW",
            dx_grid: "FN31",
            tx_freq: 14_074_000,
            mode: "FT8",
            report_sent: "-12",
            report_recvd: "-08",
            tx_power: "",
            comments: "",
            name: "",
            time_on: 1_700_000_000,
            op_call: "KD9TAW",
            my_call: "KD9TAW",
            my_grid: "EN52",
            exchange_sent: "",
            exchange_recvd: "",
            adif_propmode: "",
            ..Default::default()
        };
        server.send_qso_logged(&qso).unwrap();

        // BOTH targets receive the same contact.
        for (l, who) in [(&a, "target A"), (&b, "target B")] {
            let mut buf = [0u8; 4096];
            let (n, _) = l
                .recv_from(&mut buf)
                .unwrap_or_else(|e| panic!("{who} heard nothing: {e}"));
            match wsjtx::parse_inbound(&buf[..n]).unwrap() {
                Inbound::QsoLogged { qso, .. } => assert_eq!(qso.dx_call, "W1AW", "{who}"),
                other => panic!("{who} got {other:?}"),
            }
        }
        // ⭐ CONTROL: a socket that is NOT a target hears nothing — the send is targeted,
        // not a broadcast to everything on the host.
        let mut buf = [0u8; 4096];
        assert!(
            bystander.recv_from(&mut buf).is_err(),
            "a non-target socket received the datagram — the send is not confined to its targets"
        );
    }

    #[test]
    fn qso_logged_roundtrips_over_loopback() {
        let listener = UdpSocket::bind(loopback(0)).unwrap();
        listener
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let target = listener.local_addr().unwrap();

        let server = WsjtxServer::new(loopback(0), target).unwrap();
        server
            .send_qso_logged(&QsoLogged {
                time_off: 1_700_000_100,
                dx_call: "W1AW",
                dx_grid: "FN31",
                tx_freq: 14_074_000,
                mode: "FT8",
                report_sent: "-12",
                report_recvd: "-08",
                tx_power: "",
                comments: "",
                name: "",
                time_on: 1_700_000_000,
                op_call: "KD9TAW",
                my_call: "KD9TAW",
                my_grid: "EN52",
                exchange_sent: "",
                exchange_recvd: "",
                adif_propmode: "",
                ..Default::default()
            })
            .unwrap();

        let mut buf = [0u8; 4096];
        let (n, _) = listener.recv_from(&mut buf).unwrap();
        match wsjtx::parse_inbound(&buf[..n]).unwrap() {
            Inbound::QsoLogged { id, qso } => {
                assert_eq!(id, APP_ID);
                assert_eq!(qso.dx_call, "W1AW", "the logger must see who was worked");
                assert_eq!(qso.mode, "FT8");
                assert_eq!(qso.tx_freq, 14_074_000);
                assert_eq!(qso.report_sent, "-12");
            }
            other => panic!("expected a QsoLogged datagram, got: {other:?}"),
        }
    }

    /// #267: the ADIF form of a logged contact reaches the logger as one LoggedADIF datagram.
    #[test]
    fn logged_adif_roundtrips_over_loopback() {
        let listener = UdpSocket::bind(loopback(0)).unwrap();
        listener
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let target = listener.local_addr().unwrap();

        let server = WsjtxServer::new(loopback(0), target).unwrap();
        server
            .send_logged_adif("<CALL:4>W1AW<BAND:3>20m<MODE:3>FT8<EOR>\n")
            .unwrap();

        let mut buf = [0u8; 4096];
        let (n, _) = listener.recv_from(&mut buf).unwrap();
        match wsjtx::parse_inbound(&buf[..n]).unwrap() {
            Inbound::LoggedAdif { id, adif } => {
                assert_eq!(id, APP_ID);
                assert!(adif.contains("<EOH>\n<CALL:4>W1AW"), "{adif:?}");
                assert!(adif.ends_with("FT8 <EOR>"), "{adif:?}");
            }
            other => panic!("expected a LoggedAdif datagram, got: {other:?}"),
        }
    }

    #[test]
    fn poll_returns_none_when_idle() {
        let server = WsjtxServer::new(loopback(0), loopback(2237)).unwrap();
        assert_eq!(server.poll().unwrap(), None);
    }

    /// A correctly-framed HaltTx, as a logger sends it.
    fn halt_tx_datagram() -> Vec<u8> {
        let mut w = QdsWriter::new();
        w.put_u32(wsjtx::MAGIC)
            .put_u32(wsjtx::SCHEMA)
            .put_u32(wsjtx::msg_type::HALT_TX)
            .put_utf8(Some("JTAlert"))
            .put_bool(true);
        w.into_bytes()
    }

    /// One radio-loop tick must never process an unbounded batch: every inbound
    /// datagram is handled with the engine lock held, and HaltTx costs a
    /// blocking CAT round trip, so an unbounded drain hands a stuck consumer the
    /// engine mutex for as long as it keeps sending. Verified failing first:
    /// against the old `while let Ok(Some(_)) = poll()` this drained all 200
    /// ("one tick drained 200 inbound datagrams under the engine lock").
    #[test]
    fn drain_bounds_one_tick_and_leaves_the_rest_queued() {
        let server = WsjtxServer::new(loopback(0), loopback(0)).unwrap();
        let addr = server.local_addr().unwrap();
        let sender = UdpSocket::bind(loopback(0)).unwrap();
        let dg = halt_tx_datagram();
        let sent = INBOUND_PER_TICK * 3;
        for _ in 0..sent {
            sender.send_to(&dg, addr).unwrap();
        }
        // Let the kernel queue them (a UDP send to loopback is not synchronous).
        std::thread::sleep(std::time::Duration::from_millis(100));

        let first = server.drain(INBOUND_PER_TICK).count();
        assert_eq!(first, INBOUND_PER_TICK, "one tick must stop at the cap");
        // Nothing was thrown away — the next tick picks the backlog up.
        let second = server.drain(INBOUND_PER_TICK).count();
        assert_eq!(second, INBOUND_PER_TICK, "the remainder stays queued");
    }

    #[test]
    fn drain_stops_early_on_an_idle_socket() {
        let server = WsjtxServer::new(loopback(0), loopback(0)).unwrap();
        let addr = server.local_addr().unwrap();
        UdpSocket::bind(loopback(0))
            .unwrap()
            .send_to(&halt_tx_datagram(), addr)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        // One waiting datagram, a cap of 64: the drain returns after the one.
        assert_eq!(server.drain(INBOUND_PER_TICK).count(), 1);
    }

    #[test]
    fn server_receives_inbound_reply() {
        // The server binds a fixed-ish port; a "consumer" sends a Reply to it.
        let server = WsjtxServer::new(loopback(0), loopback(0)).unwrap();
        let server_addr = server.local_addr().unwrap();

        let consumer = UdpSocket::bind(loopback(0)).unwrap();
        // A correctly-framed Reply datagram, as a controlling app would send.
        let reply = {
            let mut w = QdsWriter::new();
            w.put_u32(wsjtx::MAGIC)
                .put_u32(wsjtx::SCHEMA)
                .put_u32(wsjtx::msg_type::REPLY)
                .put_utf8(Some("GridTracker"))
                .put_u32(1000) // time_ms
                .put_i32(-5) // snr
                .put_f64(0.1) // delta_time
                .put_u32(1500) // delta_freq
                .put_utf8(Some("FT8"))
                .put_utf8(Some("CQ W1AW FN31"))
                .put_bool(false) // low_confidence
                .put_u8(0); // modifiers
            w.into_bytes()
        };
        consumer.send_to(&reply, server_addr).unwrap();

        // Give the datagram a moment to arrive, polling non-blocking.
        let mut got = None;
        for _ in 0..100 {
            if let Some(inb) = server.poll().unwrap() {
                got = Some(inb);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        match got.expect("no inbound received") {
            Inbound::Reply {
                id,
                message,
                snr,
                delta_freq,
                ..
            } => {
                assert_eq!(id, "GridTracker");
                assert_eq!(message, "CQ W1AW FN31");
                assert_eq!(snr, -5);
                assert_eq!(delta_freq, 1500);
            }
            other => panic!("expected Reply, got {other:?}"),
        }
    }

    /// One message of each type the station can be sent, a Reply and FreeText first.
    fn each_message() -> Vec<(&'static str, Inbound, bool)> {
        let id = || "Logger".to_string();
        vec![
            (
                "Reply",
                Inbound::Reply {
                    id: id(),
                    time_ms: 0,
                    snr: 0,
                    delta_time: 0.0,
                    delta_freq: 1500,
                    mode: "~".into(),
                    message: "CQ W1AW FN31".into(),
                    low_confidence: false,
                    modifiers: 0,
                },
                true,
            ),
            (
                "FreeText with send",
                Inbound::FreeText {
                    id: id(),
                    text: "TNX 73".into(),
                    send: true,
                },
                true,
            ),
            (
                "FreeText without send",
                Inbound::FreeText {
                    id: id(),
                    text: "TNX 73".into(),
                    send: false,
                },
                true,
            ),
            (
                "Location",
                Inbound::Location {
                    id: id(),
                    location: "JO22".into(),
                },
                true,
            ),
            (
                "HaltTx",
                Inbound::HaltTx {
                    id: id(),
                    auto_only: false,
                },
                false,
            ),
            (
                "Clear",
                Inbound::Clear {
                    id: id(),
                    window: 2,
                },
                false,
            ),
            ("Replay", Inbound::Replay { id: id() }, false),
            (
                "HighlightCallsign",
                Inbound::HighlightCallsign {
                    id: id(),
                    call: "W1AW".into(),
                    bg: None,
                    fg: None,
                    highlight_last: false,
                },
                false,
            ),
            (
                "Close",
                Inbound::Other {
                    id: id(),
                    message_type: wsjtx::msg_type::CLOSE,
                },
                false,
            ),
            (
                "SwitchConfiguration",
                Inbound::Other {
                    id: id(),
                    message_type: wsjtx::msg_type::SWITCH_CONFIGURATION,
                },
                false,
            ),
            (
                "Configure",
                Inbound::Other {
                    id: id(),
                    message_type: wsjtx::msg_type::CONFIGURE,
                },
                false,
            ),
        ]
    }

    /// ⛔ From another computer, with the setting off, what shapes a transmission is refused and
    /// nothing else is. With the setting on nothing is, and from loopback nothing ever is.
    #[test]
    fn only_a_request_that_shapes_a_transmission_is_refused_from_another_computer() {
        let lan: IpAddr = "192.168.1.20".parse().unwrap();
        for (name, message, shapes) in each_message() {
            assert_eq!(
                refused_from_other_computer(&message, lan, false),
                shapes,
                "{name} from another computer, the setting off"
            );
            assert!(
                !refused_from_other_computer(&message, lan, true),
                "{name} from another computer, the setting on"
            );
            for local in ["127.0.0.1", "127.0.0.2", "::1"] {
                assert!(
                    !refused_from_other_computer(&message, local.parse().unwrap(), false),
                    "{name} from {local}, the setting off"
                );
            }
        }
    }

    /// A datagram of `kind`, framed as a logger frames it, with nothing after the sender's id.
    fn bare_datagram(kind: u32) -> Vec<u8> {
        let mut w = QdsWriter::new();
        w.put_u32(wsjtx::MAGIC)
            .put_u32(wsjtx::SCHEMA)
            .put_u32(kind)
            .put_utf8(Some("Logger"));
        w.into_bytes()
    }

    /// Close, SwitchConfiguration and Configure reach nothing: each parses as `Other`, which the
    /// radio loop does not act on, from any source.
    #[test]
    fn close_and_the_configuration_requests_are_not_acted_on() {
        for kind in [
            wsjtx::msg_type::CLOSE,
            wsjtx::msg_type::SWITCH_CONFIGURATION,
            wsjtx::msg_type::CONFIGURE,
        ] {
            assert!(
                matches!(
                    wsjtx::parse_inbound(&bare_datagram(kind)),
                    Some(Inbound::Other { .. })
                ),
                "message type {kind}"
            );
        }
    }

    /// Whether a HaltTx read as sent from `source` is admitted by a server sending to `targets`,
    /// and the address it is reported with.
    fn admitted_from(source: &str, targets: &[&str]) -> Option<IpAddr> {
        let targets: Vec<SocketAddr> = targets.iter().map(|t| t.parse().unwrap()).collect();
        let mut server = WsjtxServer::new_multi(loopback(0), targets).unwrap();
        server.read_sources_as(source.parse().unwrap());
        let sender = UdpSocket::bind(loopback(0)).unwrap();
        sender
            .send_to(&halt_tx_datagram(), server.local_addr().unwrap())
            .unwrap();
        for _ in 0..100 {
            if let Some((_, from)) = server.poll_from().unwrap() {
                return Some(from);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        None
    }

    /// The source check as it has always been, which the refusal above sits behind: loopback, a
    /// target's address, or anyone at all under a multicast target. A stranger under a unicast
    /// target is dropped.
    #[test]
    fn the_source_check_admits_loopback_a_target_and_anyone_under_multicast() {
        let lan = "192.168.1.20";
        let unicast = ["192.168.1.20:2237"];
        let group = ["239.255.0.1:2237"];
        let local = Some("127.0.0.1".parse().unwrap());
        assert_eq!(admitted_from("127.0.0.1", &unicast), local);
        assert_eq!(admitted_from(lan, &unicast), Some(lan.parse().unwrap()));
        assert_eq!(admitted_from(lan, &group), Some(lan.parse().unwrap()));
        assert_eq!(admitted_from("192.168.1.99", &unicast), None);
    }
}
