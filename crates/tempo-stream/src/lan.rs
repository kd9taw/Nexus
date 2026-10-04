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
//!
//! **Remote over this network** (the shack's own listener, for a paired computer on the same
//! network, with no relay) asks the other question: where may the shack listen, and whom may it
//! hear? [`listenable`] says exactly which addresses, private IPv4 only (the operator's ruling of
//! 2026-10-04, "any private network"); [`Network`] is the one it listens on, with its subnet, and
//! nothing from outside that subnet is heard or tried, on the listener or on a stream's socket;
//! [`network`] finds it on this computer.
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

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

// ---------------------------------------------------------------------------------------------
// Remote over this network: where the shack's own listener may be, and who it may hear.
// ---------------------------------------------------------------------------------------------

/// May Remote over this network listen on `ip`? A private IPv4 address, and exactly the RFC 1918
/// ranges: 10.0.0.0/8, 172.16.0.0/12 and 192.168.0.0/16 (`Ipv4Addr::is_private`). Never the
/// unspecified address, loopback (127/8), link-local (169.254/16), the shared carrier-NAT space
/// 100.64/10 (where Tailscale and other overlay networks number their machines), multicast,
/// broadcast, the documentation ranges, any public address, and in this version never IPv6. The
/// operator's ruling of 2026-10-04 is "any private network the shack joins": that much, no more.
pub fn listenable(ip: Ipv4Addr) -> bool {
    ip.is_private()
}

/// The shack's own network for Remote over this network: one private IPv4 address of this
/// computer's, and its subnet. Only a peer inside that subnet is heard, by the listener and by a
/// stream's socket alike, and only candidates inside it are tried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Network {
    address: Ipv4Addr,
    prefix: u8,
}

impl Network {
    /// `address` and the length of its subnet's prefix, or `None` when the address is not one to
    /// listen on ([`listenable`]). A prefix shorter than the address's own private range is taken
    /// as the range's, so a misconfigured adapter cannot widen the subnet past it to the internet.
    pub fn new(address: Ipv4Addr, prefix: u8) -> Option<Self> {
        if !listenable(address) {
            return None;
        }
        let range = match address.octets() {
            [10, ..] => 8,
            [172, ..] => 12,
            _ => 16,
        };
        Some(Self {
            address,
            prefix: prefix.clamp(range, 32),
        })
    }

    pub fn address(&self) -> Ipv4Addr {
        self.address
    }

    pub fn prefix(&self) -> u8 {
        self.prefix
    }

    /// Is `ip` on this subnet? An IPv4-mapped IPv6 address is its IPv4 one; any other IPv6 address
    /// is not.
    pub fn contains(&self, ip: IpAddr) -> bool {
        let v4 = match ip {
            IpAddr::V4(v4) => v4,
            IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) => v4,
                None => return false,
            },
        };
        let mask = u32::MAX << (32 - u32::from(self.prefix));
        u32::from(v4) & mask == u32::from(self.address) & mask
    }

    /// The address a candidate line names, when it is on this subnet. An mDNS name, a line that
    /// does not parse, and an address anywhere else (a reflexive or relay candidate, loopback): no.
    pub fn candidate(&self, line: &str) -> Option<SocketAddr> {
        let line = line.trim();
        let words: Vec<&str> = line
            .strip_prefix("a=")
            .unwrap_or(line)
            .strip_prefix("candidate:")?
            .split_whitespace()
            .collect();
        let address: SocketAddr = format!("{}:{}", words.get(4)?, words.get(5)?)
            .parse()
            .ok()?;
        self.contains(address.ip()).then_some(address)
    }

    /// `sdp` without any candidate line that names an address off this subnet, so the station never
    /// tries one: a page's offer, before the session answers it.
    pub fn offer(&self, sdp: &str) -> String {
        sdp.split_inclusive('\n')
            .filter(|line| {
                let line = line.trim();
                !line.starts_with("a=candidate:") || self.candidate(line).is_some()
            })
            .collect()
    }
}

/// Why there is no network to listen on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoNetwork {
    /// Not on this platform: what the listener serves, the stream, is Windows only.
    Unavailable,
    /// The address the operator picked is not one of this computer's right now, or not private.
    Gone,
    /// The route to the internet does not leave by a private address of this computer's, and it
    /// has no private address, or more than one: the operator picks.
    Choose,
}

/// Where Remote over this network listens: `chosen`, the operator's pick, when it is a private
/// address this computer has; with none, the address the route to the internet leaves by
/// (`route`), when that is a private address this computer has; else this computer's one private
/// address. `addresses` are this computer's IPv4 addresses on adapters that are up, each with the
/// length of its subnet's prefix.
pub fn choose(
    chosen: Option<Ipv4Addr>,
    route: Option<Ipv4Addr>,
    addresses: &[(Ipv4Addr, u8)],
) -> Result<Network, NoNetwork> {
    let find = |ip: Ipv4Addr| {
        addresses
            .iter()
            .find(|(address, _)| *address == ip)
            .and_then(|&(address, prefix)| Network::new(address, prefix))
    };
    if let Some(ip) = chosen {
        return find(ip).ok_or(NoNetwork::Gone);
    }
    if let Some(network) = route.and_then(find) {
        return Ok(network);
    }
    let mut private = addresses
        .iter()
        .filter_map(|&(address, prefix)| Network::new(address, prefix));
    match (private.next(), private.next()) {
        (Some(only), None) => Ok(only),
        _ => Err(NoNetwork::Choose),
    }
}

/// [`choose`], from this computer's own adapters and route.
pub fn network(chosen: Option<Ipv4Addr>) -> Result<Network, NoNetwork> {
    #[cfg(windows)]
    {
        choose(chosen, adapters::route(), &adapters::up())
    }
    #[cfg(not(windows))]
    {
        let _ = chosen;
        Err(NoNetwork::Unavailable)
    }
}

#[cfg(windows)]
mod adapters {
    use std::net::{IpAddr, Ipv4Addr, UdpSocket};

    use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS};
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

    /// The address the route to the internet leaves by: a UDP socket "connected" to a documentation
    /// address (RFC 5737) learns the interface it would leave by. Nothing is sent, and no name is
    /// looked up.
    pub fn route() -> Option<Ipv4Addr> {
        let probe = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
        probe.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
        match probe.local_addr().ok()?.ip() {
            IpAddr::V4(v4) => Some(v4),
            IpAddr::V6(_) => None,
        }
    }

    /// This computer's IPv4 addresses on adapters that are up, each with its prefix length.
    pub fn up() -> Vec<(Ipv4Addr, u8)> {
        let mut size: u32 = 16 * 1024;
        for _ in 0..4 {
            // In u64s: the records the call writes there need 8-byte alignment.
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            let first = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
            let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
            // SAFETY: `first` points at `size` writable bytes, aligned for the records; the call
            // writes no more than `size` and says so when it needs more.
            let result = unsafe {
                GetAdaptersAddresses(u32::from(AF_INET.0), flags, None, Some(first), &mut size)
            };
            if result == ERROR_BUFFER_OVERFLOW.0 {
                continue;
            }
            if result != ERROR_SUCCESS.0 {
                return Vec::new();
            }
            let mut found = Vec::new();
            let mut adapter = first.cast_const();
            // SAFETY: on success the call wrote a list of records inside `buffer`, each `Next`
            // null or pointing at another record there, and `buffer` outlives this walk.
            while let Some(record) = unsafe { adapter.as_ref() } {
                if record.OperStatus == IfOperStatusUp {
                    let mut unicast = record.FirstUnicastAddress.cast_const();
                    // SAFETY: as above, the unicast list lives inside `buffer`.
                    while let Some(entry) = unsafe { unicast.as_ref() } {
                        let socket = entry.Address.lpSockaddr;
                        // SAFETY: a non-null address points at a SOCKADDR the call wrote, and one
                        // whose family is AF_INET is a SOCKADDR_IN.
                        if let Some(address) = unsafe { socket.as_ref() } {
                            if address.sa_family == AF_INET {
                                let ip =
                                    unsafe { (*socket.cast::<SOCKADDR_IN>()).sin_addr.S_un.S_addr };
                                found.push((
                                    Ipv4Addr::from(u32::from_be(ip)),
                                    entry.OnLinkPrefixLength,
                                ));
                            }
                        }
                        unicast = entry.Next.cast_const();
                    }
                }
                adapter = record.Next.cast_const();
            }
            return found;
        }
        Vec::new()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// On a real Windows network stack: this computer's own route and addresses read back, each
        /// a plausible prefix, and the route's address among them.
        #[test]
        fn this_computers_addresses_and_route_are_read() {
            let up = up();
            assert!(!up.is_empty(), "no IPv4 address on an adapter that is up");
            assert!(up.iter().all(|&(_, prefix)| prefix <= 32), "{up:?}");
            if let Some(route) = route() {
                assert!(
                    up.iter().any(|&(ip, _)| ip == route),
                    "route {route} not among {up:?}"
                );
            }
        }
    }
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

    // ----- Remote over this network -----

    fn v4(text: &str) -> Ipv4Addr {
        text.parse().unwrap()
    }

    /// ★ As ruled on 2026-10-04 ("any private network"): the listener may be on a private IPv4
    /// address and nothing else. Each RFC 1918 range's first and last host are taken; the addresses
    /// just outside each, and every other kind (loopback, link-local, carrier NAT, multicast,
    /// broadcast, documentation, public) is not.
    #[test]
    fn only_an_rfc_1918_address_is_listenable() {
        for ok in [
            "10.0.0.1",
            "10.255.255.254",
            "172.16.0.1",
            "172.31.255.254",
            "192.168.0.1",
            "192.168.255.254",
        ] {
            assert!(listenable(v4(ok)), "refused {ok}");
        }
        for refused in [
            "0.0.0.0",
            "127.0.0.1",
            "169.254.1.1",
            "100.64.0.1",
            "100.127.255.254",
            "9.255.255.255",
            "11.0.0.0",
            "172.15.255.255",
            "172.32.0.0",
            "192.167.255.255",
            "192.169.0.0",
            "192.0.2.1",
            "198.51.100.1",
            "203.0.113.1",
            "198.18.0.1",
            "224.0.0.251",
            "255.255.255.255",
            "8.8.8.8",
        ] {
            assert!(!listenable(v4(refused)), "listenable: {refused}");
        }
    }

    /// ★ The subnet check: a peer on the shack's subnet is heard and one anywhere else is not,
    /// loopback included. A prefix shorter than the private range cannot widen it past the range,
    /// and an address that is not private makes no network at all.
    #[test]
    fn a_network_holds_its_subnet_and_never_past_its_private_range() {
        let home = Network::new(v4("192.168.1.20"), 24).unwrap();
        for on in [
            "192.168.1.1",
            "192.168.1.20",
            "192.168.1.254",
            "::ffff:192.168.1.7",
        ] {
            assert!(home.contains(on.parse().unwrap()), "{on} refused");
        }
        for off in [
            "192.168.2.1",
            "127.0.0.1",
            "8.8.8.8",
            "10.0.0.1",
            "::1",
            "fe80::1",
        ] {
            assert!(!home.contains(off.parse().unwrap()), "{off} heard");
        }
        let wide = Network::new(v4("10.1.2.3"), 4).unwrap();
        assert_eq!(wide.prefix(), 8, "a /4 widened past 10/8");
        assert!(wide.contains("10.200.0.1".parse().unwrap()));
        assert!(
            !wide.contains("11.0.0.1".parse().unwrap()),
            "a /4 reached 11/8"
        );
        let zero = Network::new(v4("172.20.5.5"), 0).unwrap();
        assert!(zero.contains("172.31.0.1".parse().unwrap()));
        assert!(
            !zero.contains("172.32.0.1".parse().unwrap()),
            "a /0 reached past 172.16/12"
        );
        let single = Network::new(v4("192.168.1.20"), 32).unwrap();
        assert!(single.contains("192.168.1.20".parse().unwrap()));
        assert!(!single.contains("192.168.1.21".parse().unwrap()));
        for not_private in ["8.8.8.8", "100.64.0.1", "127.0.0.1", "169.254.0.1"] {
            assert_eq!(Network::new(v4(not_private), 24), None, "{not_private}");
        }
    }

    /// ★ Only a candidate on the shack's subnet is tried: a page's host candidate there is taken; a
    /// reflexive or relay candidate, an mDNS name, loopback and a malformed line are not. An offer
    /// keeps every other line exactly as it was.
    #[test]
    fn only_candidates_on_the_subnet_are_tried() {
        let home = Network::new(v4("192.168.1.20"), 24).unwrap();
        let host = "candidate:1 1 udp 2122260223 192.168.1.33 50000 typ host generation 0";
        assert_eq!(
            home.candidate(host),
            Some("192.168.1.33:50000".parse().unwrap())
        );
        assert!(
            home.candidate(&format!("a={host}")).is_some(),
            "the a= form refused"
        );
        for refused in [
            "candidate:2 1 udp 1686052607 203.0.113.9 50001 typ srflx raddr 192.168.1.33 rport 50000",
            "candidate:3 1 udp 41885439 198.51.100.4 3478 typ relay raddr 203.0.113.9 rport 50001",
            "candidate:4 1 udp 2122260223 4f3c-9a.local 50002 typ host",
            "candidate:5 1 udp 2122260223 127.0.0.1 50003 typ host",
            "candidate:6 1 udp 2122260223 192.168.2.33 50004 typ host",
            "candidate:7 1 udp",
            "not a candidate",
        ] {
            assert_eq!(home.candidate(refused), None, "tried: {refused}");
        }
        let offer = "v=0\r\na=group:BUNDLE 0\r\n\
            a=candidate:1 1 udp 2122260223 192.168.1.33 50000 typ host\r\n\
            a=candidate:2 1 udp 1686052607 203.0.113.9 50001 typ srflx raddr 0.0.0.0 rport 0\r\n\
            a=candidate:4 1 udp 2122260223 4f3c-9a.local 50002 typ host\r\n\
            a=fingerprint:sha-256 AB:CD\r\n";
        assert_eq!(
            home.offer(offer),
            "v=0\r\na=group:BUNDLE 0\r\n\
            a=candidate:1 1 udp 2122260223 192.168.1.33 50000 typ host\r\n\
            a=fingerprint:sha-256 AB:CD\r\n"
        );
    }

    /// ★ Where the listener goes: the operator's pick when this computer has it; with none, the
    /// route's address when it is private; else this computer's one private address. CONTROLS, in
    /// the same table: a pick that is gone or public, and a choice that is not one.
    #[test]
    fn the_listener_goes_on_the_pick_the_route_or_the_only_private_address() {
        let wifi = (v4("192.168.1.20"), 24);
        let wsl = (v4("172.25.48.1"), 20);
        let public = (v4("203.0.113.5"), 24);
        let cgnat = (v4("100.70.1.2"), 10);
        let at = |n: (Ipv4Addr, u8)| Ok(Network::new(n.0, n.1).unwrap());
        // The operator's pick.
        assert_eq!(choose(Some(wsl.0), Some(wifi.0), &[wifi, wsl]), at(wsl));
        assert_eq!(
            choose(Some(v4("192.168.9.9")), Some(wifi.0), &[wifi]),
            Err(NoNetwork::Gone)
        );
        assert_eq!(
            choose(Some(public.0), None, &[public, wifi]),
            Err(NoNetwork::Gone)
        );
        // No pick: the route, when private.
        assert_eq!(choose(None, Some(wifi.0), &[wsl, wifi]), at(wifi));
        // A route that is not private: the one private address there is, or the operator picks.
        assert_eq!(choose(None, Some(public.0), &[public, wifi]), at(wifi));
        assert_eq!(choose(None, Some(cgnat.0), &[cgnat, wifi]), at(wifi));
        assert_eq!(
            choose(None, Some(public.0), &[public, wifi, wsl]),
            Err(NoNetwork::Choose)
        );
        assert_eq!(
            choose(None, Some(public.0), &[public, cgnat]),
            Err(NoNetwork::Choose)
        );
        // A LAN with no internet at all: no route.
        assert_eq!(choose(None, None, &[wifi]), at(wifi));
        assert_eq!(choose(None, None, &[]), Err(NoNetwork::Choose));
    }
}
