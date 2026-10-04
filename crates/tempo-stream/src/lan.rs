//! What the station says about the shack's own network, and the last check that it says no more.
//!
//! **Security acceptance test A8 was "No LAN address leaves the shack". The operator reversed it on
//! 2026-10-03**, after a browser on the shack's own Wi-Fi never connected. With no relay, a browser
//! on the shack's network can reach the station only at its LAN address: Chrome hides the browser's
//! own address behind an mDNS name, and a home router does not loop a packet sent to its own public
//! address back into the network. The ruling: the shack offers its own LAN address, so a browser on
//! the same network connects directly.
//!
//! So the station signals two candidates. One is server-reflexive, the address a STUN server saw,
//! whose `raddr` str0m writes as `0.0.0.0 0`. The other is a host candidate for its stream socket's
//! own address, when that is a LAN address or a public one ([`host`]). The socket is bound to the
//! address of the interface the route to the STUN server leaves by, and can be reached at no other,
//! so that is one address, and only:
//! - a private IPv4 address (10/8, 172.16/12, 192.168/16): the home network's. It is offered.
//! - a public IPv4 address, a shack with no NAT in front of it. It is offered too (2026-10-03). It is
//!   the very address a STUN server reports, so it names nothing the reflexive candidate would not,
//!   and str0m drops that reflexive candidate as the host one's duplicate: without the host
//!   candidate such a shack signalled no candidate at all, and no browser behind a router reached it.
//! - loopback, link-local (169.254/16) and the unspecified address are not.
//! - nor is the shared space 100.64/10. It is a carrier-grade NAT's, or an overlay VPN's (Tailscale
//!   numbers its interfaces there), which is where the route leaves while such a VPN carries all
//!   traffic. A browser at home is on neither, and an overlay address names the station on every
//!   network it joins.
//! - a VPN whose tunnel address is private IPv4 is offered like the home network's: nothing tells
//!   the two apart short of asking the system about its adapters. A browser that cannot reach that
//!   address connects over the reflexive candidate, as it would without it.
//! - a virtual adapter (WSL, Hyper-V, VirtualBox, Docker) does not carry the route to the internet,
//!   so its address is never the socket's, and never offered.
//! - IPv6: nothing. The socket is IPv4 (the route to an IPv4 STUN server), so no IPv6 address
//!   reaches it, and a global IPv6 address would name the machine itself.
//!
//! [`leaks`] is still the last lock: every SDP and candidate the station is about to send is read
//! for a host candidate or a private, link-local, carrier-NAT or unique-local address, and a leak is
//! never sent. The one thing it lets through is the station's own address, as the address of that
//! one host candidate. A `raddr`, any other host's address, a host candidate for any other address,
//! and the station's own LAN address anywhere else are still leaks.
//!
//! The page and the relay hand every candidate the station names to the browser as it is. A page
//! that refused private or host candidates (one way to stop a relay that forges the station's
//! answer from pointing the browser at the operator's network) would end same-network streaming.
//! That threat is for a station-signed answer to settle, not for a candidate filter.
use std::net::{IpAddr, SocketAddr};

use crate::video::picture::Path;

/// Is this an address that belongs to the shack's own network rather than the internet?
pub fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            v4.is_private()
                || v4.is_link_local()
                // Carrier-grade NAT shared space, 100.64.0.0/10, which is also where an overlay
                // network such as a VPN puts its interfaces.
                || (a == 100 && (64..=127).contains(&b))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return private(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            // Unique local (fc00::/7) and link-local (fe80::/10).
            (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// The host candidate the station offers for its stream socket at `base`: the socket's own address,
/// when that is a private IPv4 address, the shack's own network, or a public one, the shack's own
/// address on the internet. `None` for anything else (see the module header for what is not offered,
/// and why): the station then offers only its reflexive one.
pub fn host(base: SocketAddr) -> Option<SocketAddr> {
    match base.ip() {
        IpAddr::V4(v4) if v4.is_private() => Some(base),
        IpAddr::V4(v4)
            if !private(base.ip())
                && !v4.is_loopback()
                && !v4.is_unspecified()
                && !v4.is_broadcast()
                && !v4.is_multicast() =>
        {
            Some(base)
        }
        _ => None,
    }
}

/// Where a page is that the station sends its picture to at `peer`, the selected ICE pair's remote
/// address: on the shack's own network when that is a private IPv4 address, the ranges [`host`]
/// offers as the shack's own; anywhere else (the internet, a relay, a carrier's or an overlay
/// network's shared 100.64/10) otherwise. It sizes the picture and decides nothing else.
pub fn path(peer: SocketAddr) -> Path {
    let v4 = match peer.ip() {
        IpAddr::V4(v4) => Some(v4),
        IpAddr::V6(v6) => v6.to_ipv4_mapped(),
    };
    if v4.is_some_and(|v4| v4.is_private()) {
        Path::Lan
    } else {
        Path::Internet
    }
}

/// Does this SDP, or this candidate line, give away more of the shack's network than `own`, the
/// host candidate the station offers ([`host`]; `None` when it offers none)? A host candidate does
/// unless it is for `own` exactly, and so does any private address anywhere in the text but that
/// one candidate's own address. The unspecified addresses (`0.0.0.0`, `::`) and loopback identify
/// nothing and pass.
pub fn leaks(text: &str, own: Option<SocketAddr>) -> bool {
    for line in text.lines() {
        let line = line.trim();
        let candidate = line
            .strip_prefix("a=")
            .unwrap_or(line)
            .starts_with("candidate:");
        let words: Vec<&str> = line.split_whitespace().collect();
        // `typ host`: an interface address. Only the station's own passes, as this candidate's own
        // address: `candidate:<foundation> <component> <transport> <priority> <address> <port> typ
        // host`. That one word is then the only private address the line may hold.
        let mut offered = None;
        if candidate && words.windows(2).any(|w| w == ["typ", "host"]) {
            let named = match words.get(4..8) {
                Some([address, port, "typ", "host"]) => format!("{address}:{port}").parse().ok(),
                _ => None,
            };
            if named.is_none() || named != own {
                return true;
            }
            offered = Some(4);
        }
        for (index, word) in words.iter().enumerate() {
            if Some(index) == offered {
                continue;
            }
            for part in word.split(['=', ',']) {
                if let Ok(ip) = part.parse::<IpAddr>() {
                    if private(ip) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture_station_messages() -> Vec<String> {
        let file: Value = serde_json::from_str(include_str!(
            "../../../remote/test/fixtures/stream/signal.json"
        ))
        .unwrap();
        file["stationToRoom"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| {
                let p = &c["message"]["payload"];
                p["sdp"]
                    .as_str()
                    .or(p["candidate"].as_str())
                    .map(str::to_string)
            })
            .collect()
    }

    /// The contract's station: its socket's own address, behind its router's 203.0.113.7:61000 (the
    /// same as in `session`'s tests).
    fn own() -> Option<SocketAddr> {
        Some("10.0.0.5:61000".parse().unwrap())
    }

    /// CONTROL first: the contract's station answer and candidates pass, its host candidate as the
    /// station's own LAN address, and only as that.
    #[test]
    fn the_contract_station_messages_leak_nothing() {
        let sent = fixture_station_messages();
        assert_eq!(
            sent.len(),
            4,
            "premise: the answer, signed and not (S3-M1), and two candidates"
        );
        let host: Vec<&String> = sent.iter().filter(|t| t.contains(" typ host")).collect();
        assert_eq!(host.len(), 1, "premise: one host candidate");
        for text in &sent {
            assert!(!leaks(text, own()), "{text}");
        }
        // CONTROL: from a station that offers no host candidate, the same line is a leak.
        assert!(leaks(host[0], None), "{}", host[0]);
    }

    /// The rule since the operator's ruling of 2026-10-03: the station's own LAN address passes as
    /// its own host candidate, trickled or in SDP. CONTROL: offering none, it is a leak.
    #[test]
    fn the_stations_own_lan_address_passes_as_its_host_candidate() {
        for fine in [
            "candidate:1 1 udp 2130706431 10.0.0.5 61000 typ host",
            "a=candidate:1 1 udp 2130706431 10.0.0.5 61000 typ host ufrag cgyb7Q0Lvq0tP3Ko",
        ] {
            assert!(!leaks(fine, own()), "refused: {fine}");
            assert!(leaks(fine, None), "control: {fine}");
        }
    }

    /// ★ A8's text half, as it stands: a host candidate for any address but the station's own, the
    /// station's own address anywhere but there, and any other LAN address are caught, in SDP and
    /// trickled lines, whether or not the station offers its own.
    #[test]
    fn a_host_candidate_or_a_lan_address_is_caught() {
        for leak in [
            // Another host on the shack's network, another port, a public interface, an mDNS name.
            "candidate:1 1 udp 2130706431 10.0.0.6 61000 typ host",
            "candidate:1 1 udp 2130706431 10.0.0.5 61001 typ host",
            "candidate:1 1 udp 2130706431 192.168.1.20 61000 typ host",
            "a=candidate:1 1 udp 2130706431 203.0.113.7 61000 typ host",
            "candidate:1 1 udp 2130706431 4d2a8f1e-8c1b-4f3a-9e2d-2c3b4a5d6e7f.local 61000 typ host",
            // Link-local, an overlay VPN's, IPv6.
            "candidate:1 1 udp 2130706431 169.254.10.1 61000 typ host",
            "candidate:1 1 udp 2130706431 100.101.102.103 61000 typ host",
            "candidate:1 1 udp 2130706431 fd00::1 61000 typ host",
            // The station's own address, but not as its host candidate's.
            "candidate:1 1 udp 2130706431 10.0.0.5 61000 typ host raddr 10.0.0.5 rport 61000",
            "candidate:1 1 udp 1694498815 203.0.113.7 61000 typ srflx raddr 10.0.0.5 rport 61000",
            "o=- 1 2 IN IP4 10.0.0.5",
            "c=IN IP4 10.0.0.5",
            // Any other LAN address.
            "candidate:1 1 udp 1694498815 203.0.113.7 61000 typ srflx raddr 192.168.1.20 rport 61000",
            "candidate:1 1 udp 1694498815 203.0.113.7 61000 typ srflx raddr 10.0.0.4 rport 5",
            "candidate:1 1 udp 1694498815 203.0.113.7 61000 typ srflx raddr 172.16.3.1 rport 5",
            "candidate:1 1 udp 1694498815 203.0.113.7 61000 typ srflx raddr 169.254.10.1 rport 5",
            "candidate:1 1 udp 1694498815 203.0.113.7 61000 typ srflx raddr 100.101.102.103 rport 5",
            "candidate:1 1 udp 1694498815 2001:db8::1 61000 typ srflx raddr fd00::1 rport 5",
            "candidate:1 1 udp 1694498815 2001:db8::1 61000 typ srflx raddr fe80::1 rport 5",
            "candidate:1 1 udp 1694498815 2001:db8::1 61000 typ srflx raddr ::ffff:192.168.1.20 rport 5",
            "o=- 1 2 IN IP4 192.168.1.20",
            "c=IN IP4 10.1.2.3",
        ] {
            assert!(leaks(leak, own()), "missed: {leak}");
            assert!(leaks(leak, None), "missed, offering none: {leak}");
        }
    }

    #[test]
    fn public_and_unspecified_addresses_pass() {
        for fine in [
            "candidate:1 1 udp 1694498815 203.0.113.7 61000 typ srflx raddr 0.0.0.0 rport 0",
            "candidate:1 1 udp 16777215 198.51.100.9 3478 typ relay raddr 0.0.0.0 rport 0",
            "o=- 1 2 IN IP4 0.0.0.0",
            "o=- 1 2 IN IP4 127.0.0.1",
            "c=IN IP6 ::",
            // A hostname is not an address, and "host" as a word elsewhere is not a candidate type.
            "a=msid:host video",
        ] {
            assert!(!leaks(fine, own()), "false alarm: {fine}");
            assert!(!leaks(fine, None), "false alarm, offering none: {fine}");
        }
    }

    /// Which address is offered: the socket's own, when it is a private IPv4 address, or a public
    /// one (a shack with no NAT in front of it, 2026-10-03). Not loopback, link-local, broadcast,
    /// multicast or the unspecified address; not the shared 100.64/10 (a carrier NAT's, or an
    /// overlay VPN's); and nothing on IPv6, mapped or not.
    #[test]
    fn a_private_or_public_ipv4_socket_address_is_offered() {
        for offered in [
            "10.0.0.5:61000",
            "172.16.3.1:5",
            "172.31.255.254:5",
            "192.168.1.20:61000",
            "203.0.113.7:61000",
            "172.32.0.1:5",
            "8.8.4.4:5",
        ] {
            let base: SocketAddr = offered.parse().unwrap();
            assert_eq!(host(base), Some(base), "{offered}");
        }
        for other in [
            "127.0.0.1:5",
            "169.254.10.1:5",
            "0.0.0.0:5",
            "255.255.255.255:5",
            "224.0.0.251:5",
            "100.64.0.1:5",
            "100.101.102.103:5",
            "100.127.255.254:5",
            "[fd00::1]:5",
            "[fe80::1]:5",
            "[2001:db8::1]:5",
            "[::ffff:192.168.1.20]:5",
            "[::ffff:203.0.113.7]:5",
        ] {
            assert_eq!(host(other.parse().unwrap()), None, "{other}");
        }
    }

    /// The guard, for a shack whose socket holds a public address: that address passes as its own
    /// host candidate and as nothing else, and every other host, every LAN address and every
    /// `raddr` naming one is still caught. CONTROL: offering none, the same line is a leak.
    /// The picture's path: the shack's own network only for the private IPv4 ranges, so a page
    /// reached through a carrier's or an overlay network's shared space gets the internet's budget.
    #[test]
    fn only_a_private_ipv4_peer_is_on_the_shacks_network() {
        for (peer, want) in [
            ("192.168.1.20:58503", Path::Lan),
            ("10.0.0.6:61000", Path::Lan),
            ("172.20.1.2:5000", Path::Lan),
            ("[::ffff:192.168.1.20]:5000", Path::Lan),
            ("203.0.113.9:3478", Path::Internet),
            ("100.85.1.2:41641", Path::Internet),
            ("169.254.10.1:5000", Path::Internet),
            ("[2001:db8::1]:5000", Path::Internet),
            ("[fd00::1]:5000", Path::Internet),
        ] {
            assert_eq!(path(peer.parse().unwrap()), want, "{peer}");
        }
    }

    #[test]
    fn a_public_socket_address_passes_as_its_own_host_candidate_only() {
        let own = Some("203.0.113.7:61000".parse().unwrap());
        for fine in [
            "candidate:1 1 udp 2130706431 203.0.113.7 61000 typ host",
            "a=candidate:1 1 udp 2130706431 203.0.113.7 61000 typ host ufrag cgyb7Q0Lvq0tP3Ko",
        ] {
            assert!(!leaks(fine, own), "refused: {fine}");
            assert!(leaks(fine, None), "control: {fine}");
        }
        for leak in [
            "candidate:1 1 udp 2130706431 203.0.113.8 61000 typ host",
            "candidate:1 1 udp 2130706431 203.0.113.7 61001 typ host",
            "candidate:1 1 udp 2130706431 10.0.0.5 61000 typ host",
            "candidate:1 1 udp 2130706431 169.254.10.1 61000 typ host",
            "candidate:1 1 udp 2130706431 100.101.102.103 61000 typ host",
            "candidate:1 1 udp 2130706431 fd00::1 61000 typ host",
            "candidate:1 1 udp 2130706431 203.0.113.7 61000 typ host raddr 10.0.0.5 rport 61000",
            "candidate:1 1 udp 1694498815 203.0.113.7 61000 typ srflx raddr 192.168.1.20 rport 5",
            "o=- 1 2 IN IP4 192.168.1.20",
            "c=IN IP4 10.1.2.3",
        ] {
            assert!(leaks(leak, own), "missed: {leak}");
        }
    }
}
