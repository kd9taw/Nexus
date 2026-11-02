//! FreeDV's own app (freedv-gui) beside Nexus: the two things it sends that Nexus reads.
//!
//! Nexus does not encode or decode FreeDV. The operator runs FreeDV's free app beside Nexus, and
//! that app tells other programs two things over UDP, both read here and nothing else:
//!
//! 1. **The contacts it logs.** FreeDV's "Log QSO" sends WSJT-X-protocol datagrams (sender id
//!    `FreeDV`): a Heartbeat, a Status and a Decode, then a **QSO Logged (type 5)** for the
//!    contact. There is no LoggedADIF (type 12). [`read_log_datagram`] keeps the type 5 from id
//!    `FreeDV` as a contact, takes FreeDV's other datagrams only as "FreeDV is there", and drops
//!    everything else.
//! 2. **The calls it hears.** FreeDV 2.4 broadcasts each decoded callsign as a small JSON object
//!    (`"type": "fdv_callsign"`). [`read_heard`] reads one.
//!
//! These facts come from FreeDV's published behaviour (its manual, and its source read for
//! behaviour only). No FreeDV code is in this file or in Nexus.
//!
//! ⚠️ **Everything here is untrusted input from a UDP port**, so every reader is total: a
//! datagram of any bytes and any length answers [`LogIntake::Ignored`] or `None` and never
//! panics, and nothing read here can do more than add a contact to the log through the ordinary
//! import or a line to the heard list. Nothing here keys, tunes or replies.

use crate::wsjtx::{parse_inbound, Inbound, LoggedQso};

/// The sender id FreeDV's app puts on every WSJT-X-protocol datagram it sends.
pub const FREEDV_ID: &str = "FreeDV";

/// The largest datagram either listener reads. FreeDV's datagrams are a few hundred bytes; a
/// longer one is not FreeDV's and is dropped whole rather than read truncated. A listener reads
/// into a buffer one byte longer than this so that it can tell.
pub const MAX_DATAGRAM: usize = 4096;

/// What one datagram on the log port is.
#[derive(Debug, Clone, PartialEq)]
pub enum LogIntake {
    /// FreeDV logged a contact (type 5 from id `FreeDV`). Not yet checked against the station:
    /// that is the importer's job.
    Contact(Box<LoggedQso>),
    /// FreeDV is there: its Heartbeat (0), Status (1) or Decode (2).
    Alive,
    /// Anything else: another program's id, another type, not WSJT-X framing, too long.
    Ignored,
}

/// Read one datagram from the log port.
pub fn read_log_datagram(bytes: &[u8]) -> LogIntake {
    if bytes.len() > MAX_DATAGRAM {
        return LogIntake::Ignored;
    }
    match parse_inbound(bytes) {
        Some(Inbound::QsoLogged { id, qso }) if id == FREEDV_ID => LogIntake::Contact(qso),
        Some(Inbound::Other { id, message_type })
            if id == FREEDV_ID && matches!(message_type, 0 | 1) =>
        {
            LogIntake::Alive
        }
        Some(Inbound::Decode { id, .. }) if id == FREEDV_ID => LogIntake::Alive,
        _ => LogIntake::Ignored,
    }
}

/// One call FreeDV heard, from its callsign broadcast.
#[derive(Debug, Clone, PartialEq)]
pub struct HeardCall {
    /// The decoded callsign, upper-cased. Checked to look like a call.
    pub call: String,
    /// FreeDV's mode name (`RADEV1`), as it sent it; empty when absent.
    pub mode: String,
    /// The SNR in dB, when FreeDV sent one.
    pub snr: Option<i32>,
    /// The radio frequency in Hz, when FreeDV sent one.
    pub frequency_hz: Option<u64>,
}

/// Read one datagram of FreeDV's callsign broadcast: a JSON object whose `type` is
/// `fdv_callsign`, with a `callsign` that looks like one. Anything else is `None`.
pub fn read_heard(bytes: &[u8]) -> Option<HeardCall> {
    if bytes.len() > MAX_DATAGRAM {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    if v.get("type")?.as_str()? != "fdv_callsign" {
        return None;
    }
    let call = v.get("callsign")?.as_str()?.trim().to_ascii_uppercase();
    if !plausible_call(&call) {
        return None;
    }
    let text = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").trim();
    let mode: String = text("mode").chars().take(16).collect();
    // A number, never a string; an SNR outside any receiver's range is not one.
    let snr = v
        .get("snr")
        .and_then(|x| x.as_f64())
        .filter(|s| s.is_finite() && (-60.0..=99.0).contains(s))
        .map(|s| s.round() as i32);
    let frequency_hz = v
        .get("frequency_hz")
        .and_then(|x| x.as_u64())
        .filter(|f| *f > 0);
    Some(HeardCall {
        call,
        mode,
        snr,
        frequency_hz,
    })
}

/// A callsign's shape: 3 to 12 characters, letters, digits and `/`, at least one letter and one
/// digit. A heard call is only shown, so this is a sanity check on untrusted text, not the
/// logbook's rule.
fn plausible_call(c: &str) -> bool {
    (3..=12).contains(&c.len())
        && c.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'/')
        && c.bytes().any(|b| b.is_ascii_digit())
        && c.bytes().any(|b| b.is_ascii_alphabetic())
}

/// The group FreeDV's callsign broadcast goes to by default (FreeDV 2.4: `224.0.0.1:7177`).
pub const HEARD_GROUP: std::net::Ipv4Addr = std::net::Ipv4Addr::new(224, 0, 0, 1);

/// Open the heard-call listener on `port`, as a receiver of FreeDV's broadcast: bound on every
/// adapter (a group datagram FreeDV sends on this computer arrives on the adapter it went out
/// on), sharing the port with any other program that listens there too (`SO_REUSEADDR`, as
/// [`crate::flexdisc`] does), and a member of [`HEARD_GROUP`]. A datagram FreeDV sends straight to
/// `127.0.0.1:<port>` arrives on the same socket.
///
/// ⚠️ **Bound on every adapter, so the LISTENER, not the bind, keeps it to this computer**: the
/// caller drops every datagram whose source is not an address this computer holds
/// ([`from_this_computer`]). It sends nothing, ever.
pub fn bind_heard(port: u16) -> std::io::Result<std::net::UdpSocket> {
    let raw = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    raw.set_reuse_address(true)?;
    let addr: std::net::SocketAddr = ([0, 0, 0, 0], port).into();
    raw.bind(&addr.into())?;
    let sock: std::net::UdpSocket = raw.into();
    // On the default adapter, and on loopback where the system allows it: a membership the
    // system refuses on one leaves the other, and a unicast to 127.0.0.1 needs neither.
    let any = sock.join_multicast_v4(&HEARD_GROUP, &std::net::Ipv4Addr::UNSPECIFIED);
    let lo = sock.join_multicast_v4(&HEARD_GROUP, &std::net::Ipv4Addr::LOCALHOST);
    if let (Err(e), Err(_)) = (any, lo) {
        return Err(e);
    }
    Ok(sock)
}

/// Is `ip` this computer: loopback, or one of the addresses its adapters hold (`own`, from
/// [`crate::fdsync::lan_addrs`])? A group datagram FreeDV sends on this computer carries the
/// address of the adapter it went out on, so loopback alone would refuse FreeDV itself.
pub fn from_this_computer(ip: std::net::IpAddr, own: &[std::net::Ipv4Addr]) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || own.contains(&v4),
        std::net::IpAddr::V6(v6) => v6.is_loopback(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wsjtx::{encode_decode, encode_heartbeat, encode_qso_logged, Decode, QsoLogged};

    fn contact(id: &str) -> Vec<u8> {
        encode_qso_logged(
            id,
            &QsoLogged {
                time_off: 1_760_000_000,
                time_on: 1_760_000_000,
                dx_call: "W5FDV",
                dx_grid: "EM12",
                tx_freq: 14_236_000,
                mode: "DIGITALVOICE",
                my_call: "KD9TAW",
                ..Default::default()
            },
        )
    }

    #[test]
    fn freedvs_qso_logged_is_a_contact_and_anothers_is_not() {
        match read_log_datagram(&contact(FREEDV_ID)) {
            LogIntake::Contact(q) => {
                assert_eq!(q.dx_call, "W5FDV");
                assert_eq!(q.tx_freq, 14_236_000);
                assert_eq!(q.my_call, "KD9TAW");
            }
            other => panic!("expected a contact, got {other:?}"),
        }
        assert_eq!(read_log_datagram(&contact("WSJT-X")), LogIntake::Ignored);
        assert_eq!(read_log_datagram(&contact("freedv")), LogIntake::Ignored);
    }

    #[test]
    fn freedvs_heartbeat_and_decode_say_it_is_there() {
        assert_eq!(
            read_log_datagram(&encode_heartbeat(FREEDV_ID, 3, "2.4.0", "")),
            LogIntake::Alive
        );
        let d = encode_decode(
            FREEDV_ID,
            &Decode {
                message: "W5FDV",
                ..Default::default()
            },
        );
        assert_eq!(read_log_datagram(&d), LogIntake::Alive);
        assert_eq!(
            read_log_datagram(&encode_heartbeat("JTDX", 3, "1", "")),
            LogIntake::Ignored
        );
    }

    #[test]
    fn a_datagram_longer_than_the_bound_is_dropped_whole() {
        let mut d = contact(FREEDV_ID);
        assert!(matches!(read_log_datagram(&d), LogIntake::Contact(_)));
        d.resize(MAX_DATAGRAM + 1, 0);
        assert_eq!(read_log_datagram(&d), LogIntake::Ignored);
    }

    /// Random bytes, truncations and bit-flips of a real datagram: every one answers, none
    /// panics. A fixed-seed generator, so a failure repeats.
    #[test]
    fn any_bytes_at_all_answer_without_a_panic() {
        let mut seed: u64 = 0x5eed_f00d_cafe_d00d;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let good = contact(FREEDV_ID);
        let heard = br#"{"type":"fdv_callsign","version":1,"timestamp":"2026-10-14T00:00:00Z","callsign":"W5FDV","mode":"RADEV1","snr":7,"frequency_hz":14236000}"#;
        let mut contacts = 0;
        for i in 0..20_000 {
            let mut bytes: Vec<u8> = match i % 4 {
                0 => (0..(next() % 600) as usize).map(|_| next() as u8).collect(),
                1 => good[..(next() as usize % good.len())].to_vec(),
                2 => heard[..(next() as usize % heard.len())].to_vec(),
                _ => good.clone(),
            };
            if i % 4 == 3 && !bytes.is_empty() {
                let at = next() as usize % bytes.len();
                bytes[at] ^= 1 << (next() % 8);
            }
            if let LogIntake::Contact(q) = read_log_datagram(&bytes) {
                contacts += 1;
                assert!(q.dx_call.len() <= bytes.len());
            }
            let _ = read_heard(&bytes);
        }
        // Control: some bit-flipped copies (a flip in a field's text) still read as a contact,
        // so the loop did reach the reader with frames it could parse.
        assert!(contacts > 0, "no flipped frame ever parsed");
    }

    #[test]
    fn a_heard_call_is_read_from_freedvs_broadcast() {
        let b = br#"{"type":"fdv_callsign","version":1,"timestamp":"2026-10-14T01:02:03Z","callsign":"w5fdv","mode":"RADEV1","snr":7.4,"frequency_hz":14236000}"#;
        assert_eq!(
            read_heard(b),
            Some(HeardCall {
                call: "W5FDV".into(),
                mode: "RADEV1".into(),
                snr: Some(7),
                frequency_hz: Some(14_236_000),
            })
        );
        // Optional fields absent: still a call.
        let b = br#"{"type":"fdv_callsign","callsign":"VK5DGR"}"#;
        assert_eq!(read_heard(b).map(|h| h.call), Some("VK5DGR".into()));
        // Not FreeDV's, or not a call.
        for b in [
            &br#"{"type":"other","callsign":"W5FDV"}"#[..],
            br#"{"type":"fdv_callsign","callsign":"<script>"}"#,
            br#"{"type":"fdv_callsign","callsign":"HELLO"}"#,
            br#"{"type":"fdv_callsign","callsign":7}"#,
            br#"[1,2,3]"#,
            b"",
        ] {
            assert_eq!(read_heard(b), None, "{}", String::from_utf8_lossy(b));
        }
    }
    #[test]
    fn only_this_computers_addresses_are_this_computer() {
        use std::net::{IpAddr, Ipv4Addr};
        let own = [Ipv4Addr::new(192, 168, 1, 5)];
        assert!(from_this_computer(IpAddr::V4(Ipv4Addr::LOCALHOST), &own));
        assert!(from_this_computer(
            IpAddr::V4(Ipv4Addr::new(127, 0, 1, 1)),
            &own
        ));
        assert!(from_this_computer(IpAddr::V4(own[0]), &own));
        assert!(!from_this_computer(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 6)),
            &own
        ));
        assert!(!from_this_computer(
            "::ffff:192.168.1.6".parse().unwrap(),
            &own
        ));
        assert!(from_this_computer("::1".parse().unwrap(), &own));
    }
}
