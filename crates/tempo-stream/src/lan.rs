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
//! [`look`] finds it on this computer.
//!
//! Not on every adapter, though: [`left_out`] drops tunnels and virtual adapters before anything
//! is chosen. A VPN that hands out a private address is another network, not the shack's, and is
//! where the route to the internet leaves while a full-tunnel VPN is up, so without this the
//! listener would follow it there; nobody on the shack's network can reach a virtual switch's
//! address (WSL's, Hyper-V's, VirtualBox's). [`dnssd`] lets the other computer find the shack by
//! name, and [`firewall`] reads what Windows' firewall will let in on the network it listens on.
pub mod dnssd;
pub mod firewall;

use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};

use crate::video::picture::Path;

/// The TCP port the shack listens on, and the UDP port of its stream, unless the operator sets
/// another: beside Field Day sync's 42073 and 42074.
pub const PORT: u16 = 42075;

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
/// address. `addresses` are this computer's IPv4 addresses on adapters that are up and not left out
/// ([`choices`]), each with the length of its subnet's prefix, so a pick or a route on a VPN or a
/// virtual adapter is not one of them.
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

/// One of this computer's adapters that is up, as the listener's choice reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Adapter {
    /// The name Windows shows for it: "Wi-Fi", "Ethernet", "vEthernet (WSL)".
    pub name: String,
    /// Its interface type, as IANA numbers them (`ifType`): 6 Ethernet, 71 Wi-Fi, 23 PPP, 53 a
    /// proprietary virtual interface, 131 a tunnel.
    pub kind: u32,
    /// Windows calls it hardware (`HardwareInterface`): a card, a chip or a dongle, not an adapter a
    /// driver or a virtual switch made.
    pub hardware: bool,
    /// It has a default gateway.
    pub gateway: bool,
    /// Its interface index.
    pub index: u32,
    /// Its GUID as Windows writes it, `{…}`.
    pub id: String,
    /// Its IPv4 addresses, each with the length of its subnet's prefix.
    pub addresses: Vec<(Ipv4Addr, u8)>,
}

/// Why an adapter is not one to listen on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftOut {
    /// A VPN or another tunnel, by its interface type: PPP (Windows' own VPN connections), a
    /// tunnel, or a proprietary virtual interface (the Wintun adapter of WireGuard, Tailscale and
    /// many VPN apps).
    Tunnel,
    /// Not hardware: a Hyper-V virtual switch's adapter (WSL's, the Default Switch's, an internal
    /// switch's), VirtualBox's or VMware's host adapter, Docker's, a TAP adapter (OpenVPN's and many
    /// VPN clients'), a mobile hotspot's, Bluetooth's.
    Virtual,
}

const IF_PPP: u32 = 23;
const IF_PROP_VIRTUAL: u32 = 53;
const IF_TUNNEL: u32 = 131;

/// Is `adapter` left out of the listener's choice, and why? Tunnels by their interface type, then
/// everything Windows does not call hardware. The one virtual adapter kept is Hyper-V's for an
/// external switch: the computer's own way onto the physical network when its card is shared with
/// virtual machines. Hyper-V names it "vEthernet (…)", and it alone has the network's gateway;
/// WSL's, the Default Switch's and an internal switch's have none.
pub fn left_out(adapter: &Adapter) -> Option<LeftOut> {
    if matches!(adapter.kind, IF_PPP | IF_PROP_VIRTUAL | IF_TUNNEL) {
        return Some(LeftOut::Tunnel);
    }
    if adapter.hardware || (adapter.gateway && adapter.name.starts_with("vEthernet (")) {
        return None;
    }
    Some(LeftOut::Virtual)
}

/// A private IPv4 address the listener may use, on an adapter that is not left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub network: Network,
    /// Its adapter's name, which the shack shows beside the address.
    pub name: String,
    /// Its adapter's interface index, the one Windows' name service advertises on.
    pub index: u32,
    /// Its adapter's GUID, by which Windows' network list names the network on it.
    pub id: String,
}

/// Every address the listener may use: each private IPv4 address ([`listenable`]) of each adapter
/// that is not left out ([`left_out`]), in the order the adapters come.
pub fn choices(adapters: &[Adapter]) -> Vec<Choice> {
    adapters
        .iter()
        .filter(|adapter| left_out(adapter).is_none())
        .flat_map(|adapter| {
            adapter.addresses.iter().filter_map(|&(address, prefix)| {
                Some(Choice {
                    network: Network::new(address, prefix)?,
                    name: adapter.name.clone(),
                    index: adapter.index,
                    id: adapter.id.clone(),
                })
            })
        })
        .collect()
}

/// This computer's networks, as the listener finds them each time it looks.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// Every address it may use ([`choices`]), for the operator to pick from.
    pub choices: Vec<Choice>,
    /// Where it listens, chosen among those ([`choose`]), or why nowhere.
    pub network: Result<Network, NoNetwork>,
}

impl Look {
    /// The choice it listens on, when it listens.
    pub fn chosen(&self) -> Option<&Choice> {
        let network = self.network.as_ref().ok()?;
        self.choices
            .iter()
            .find(|choice| choice.network == *network)
    }
}

/// [`choose`] among this computer's own [`choices`], with its route.
pub fn look(chosen: Option<Ipv4Addr>) -> Look {
    #[cfg(windows)]
    {
        let choices = choices(&adapters::up());
        let addresses: Vec<(Ipv4Addr, u8)> = choices
            .iter()
            .map(|choice| (choice.network.address(), choice.network.prefix()))
            .collect();
        let network = choose(chosen, adapters::route(), &addresses);
        Look { choices, network }
    }
    #[cfg(not(windows))]
    {
        let _ = chosen;
        Look {
            choices: Vec::new(),
            network: Err(NoNetwork::Unavailable),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The other computer's side: the address it was given, and why it could not reach it.
// ---------------------------------------------------------------------------------------------

/// The address an operator typed for a shack, the one the shack shows: a private IPv4 address
/// ([`listenable`]), with a port or with [`PORT`]. `None` for anything else: a name, IPv6, a
/// public address, a system port. Typing it always works, with or without finding by name.
pub fn typed(text: &str) -> Option<SocketAddrV4> {
    let text = text.trim();
    let address = match text.parse::<SocketAddrV4>() {
        Ok(address) => address,
        Err(_) => SocketAddrV4::new(text.parse().ok()?, PORT),
    };
    (listenable(*address.ip()) && address.port() >= 1024).then_some(address)
}

/// Is `ip` on one of this computer's own networks ([`choices`])? Taken to be where that cannot be
/// read (off Windows), so nothing is blamed on the network that the network did not do.
pub fn here(ip: Ipv4Addr) -> bool {
    #[cfg(windows)]
    {
        choices(&adapters::up())
            .iter()
            .any(|choice| choice.network.contains(ip.into()))
    }
    #[cfg(not(windows))]
    {
        let _ = ip;
        true
    }
}

/// Why this computer could not reach a shack, as a code the window puts in words: `otherNetwork`
/// when the address is on none of this computer's networks (`here` false); `refused` when the
/// shack's computer answered that nothing listens on the port, which Windows does only with its
/// firewall off; `noAnswer` for the rest. A firewall that drops what it does not allow, a port
/// nothing listens on behind that firewall, a network that keeps its devices apart and a shack
/// that is asleep all look the same from here, and the words say so.
pub fn unreached(error: &std::io::Error, here: bool) -> &'static str {
    if !here {
        return "otherNetwork";
    }
    match error.kind() {
        std::io::ErrorKind::ConnectionRefused => "refused",
        _ => "noAnswer",
    }
}

#[cfg(windows)]
mod adapters {
    use std::net::{IpAddr, Ipv4Addr, UdpSocket};

    use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS, NO_ERROR};
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GetIfEntry2, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST,
        GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH, MIB_IF_ROW2,
    };
    use windows::Win32::NetworkManagement::Ndis::{IfOperStatusUp, NET_LUID_LH};
    use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

    use super::Adapter;

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

    /// This computer's adapters that are up, each with its IPv4 addresses and their prefix lengths.
    pub fn up() -> Vec<Adapter> {
        let mut size: u32 = 16 * 1024;
        for _ in 0..4 {
            // In u64s: the records the call writes there need 8-byte alignment.
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            let first = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
            let flags = GAA_FLAG_SKIP_ANYCAST
                | GAA_FLAG_SKIP_MULTICAST
                | GAA_FLAG_SKIP_DNS_SERVER
                | GAA_FLAG_INCLUDE_GATEWAYS;
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
                    let mut addresses = Vec::new();
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
                                addresses.push((
                                    Ipv4Addr::from(u32::from_be(ip)),
                                    entry.OnLinkPrefixLength,
                                ));
                            }
                        }
                        unicast = entry.Next.cast_const();
                    }
                    found.push(Adapter {
                        // SAFETY: the call wrote both names as null-terminated strings inside
                        // `buffer` (or left them null, which reads as empty).
                        name: unsafe { wide(record.FriendlyName) },
                        kind: record.IfType,
                        hardware: hardware(record.Luid),
                        gateway: !record.FirstGatewayAddress.is_null(),
                        // SAFETY: the union's two views are the same bytes; the index is a u32.
                        index: unsafe { record.Anonymous1.Anonymous.IfIndex },
                        id: if record.AdapterName.is_null() {
                            String::new()
                        } else {
                            // SAFETY: as for the names above, an ANSI string.
                            unsafe { record.AdapterName.to_string() }.unwrap_or_default()
                        },
                        addresses,
                    });
                }
                adapter = record.Next.cast_const();
            }
            return found;
        }
        Vec::new()
    }

    /// Windows' own word on whether the adapter is hardware (`HardwareInterface`, the first of
    /// `InterfaceAndOperStatusFlags`' bits). An adapter whose row cannot be read is not.
    fn hardware(luid: NET_LUID_LH) -> bool {
        let mut row = MIB_IF_ROW2 {
            InterfaceLuid: luid,
            ..Default::default()
        };
        // SAFETY: `row` is a whole MIB_IF_ROW2 with the LUID the call reads set; it fills the rest.
        let read = unsafe { GetIfEntry2(&mut row) };
        read == NO_ERROR && row.InterfaceAndOperStatusFlags._bitfield & 1 != 0
    }

    /// A UTF-16 string the call wrote, or empty for none.
    ///
    /// # Safety
    /// `text` is null or points at a null-terminated UTF-16 string.
    unsafe fn wide(text: windows::core::PWSTR) -> String {
        if text.is_null() {
            return String::new();
        }
        // SAFETY: as the caller promises.
        unsafe { text.to_string() }.unwrap_or_default()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// On a real Windows network stack: this computer's own route and adapters read back, each
        /// address with a plausible prefix and the route's address among them; loopback is not
        /// hardware, and no choice is on a tunnel or a virtual adapter. Each adapter and what
        /// became of it is printed (`--nocapture`), to hold beside `Get-NetAdapter`'s
        /// `HardwareInterface` column.
        #[test]
        fn this_computers_adapters_and_route_are_read() {
            let up = up();
            let all: Vec<(Ipv4Addr, u8)> = up.iter().flat_map(|a| a.addresses.clone()).collect();
            assert!(!all.is_empty(), "no IPv4 address on an adapter that is up");
            assert!(all.iter().all(|&(_, prefix)| prefix <= 32), "{up:?}");
            if let Some(route) = route() {
                assert!(
                    all.iter().any(|&(ip, _)| ip == route),
                    "route {route} not among {up:?}"
                );
            }
            for adapter in &up {
                println!(
                    "{:?} type {} hardware {} gateway {} index {} {:?}: {:?}",
                    adapter.name,
                    adapter.kind,
                    adapter.hardware,
                    adapter.gateway,
                    adapter.index,
                    adapter.addresses,
                    super::super::left_out(adapter)
                );
                if adapter.addresses.iter().any(|(ip, _)| ip.is_loopback()) {
                    assert!(!adapter.hardware, "loopback read as hardware: {adapter:?}");
                }
            }
            for choice in super::super::choices(&up) {
                let on = up.iter().find(|a| a.index == choice.index).unwrap();
                assert_eq!(super::super::left_out(on), None, "{choice:?}");
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

    // ----- The adapter list -----

    /// An adapter as Windows reports it, for the tables below.
    fn adapter(name: &str, kind: u32, hardware: bool, gateway: bool, at: &str) -> Adapter {
        let (address, prefix) = at.split_once('/').unwrap();
        Adapter {
            name: name.into(),
            kind,
            hardware,
            gateway,
            index: 7,
            id: format!("{{{name}}}"),
            addresses: vec![(v4(address), prefix.parse().unwrap())],
        }
    }

    const ETHERNET: u32 = 6;
    const WIFI: u32 = 71;

    /// ★ Tunnels and virtual adapters are left out, each for its reason, and the networks a card
    /// is on are kept: as this box's own adapters read on Windows (Wi-Fi, the WSL switch, Tailscale)
    /// and the kinds it does not have. CONTROL: the same virtual adapters with Windows calling them
    /// hardware would be kept, so it is the hardware bit and the type that leave them out, not the
    /// name.
    #[test]
    fn tunnels_and_virtual_adapters_are_left_out() {
        let kept = [
            adapter("Wi-Fi", WIFI, true, true, "192.168.1.20/24"),
            adapter("Ethernet", ETHERNET, true, true, "10.0.0.5/24"),
            // A USB Ethernet dongle on a LAN with no router: hardware, no gateway.
            adapter("Ethernet 2", ETHERNET, true, false, "192.168.50.2/24"),
            // Hyper-V's adapter for an external switch: the computer's own way onto the network.
            adapter(
                "vEthernet (External)",
                ETHERNET,
                false,
                true,
                "192.168.1.21/24",
            ),
        ];
        for adapter in &kept {
            assert_eq!(left_out(adapter), None, "{adapter:?}");
        }
        let tunnels = [
            // Wintun (WireGuard, Tailscale, Cloudflare WARP, NordLynx), and OpenVPN's offload.
            adapter("WireGuard", IF_PROP_VIRTUAL, false, true, "10.66.0.2/32"),
            adapter("Tailscale", IF_PROP_VIRTUAL, false, false, "100.85.1.2/32"),
            // Windows' own VPN connections (IKEv2, SSTP, L2TP, PPTP) are PPP adapters.
            adapter("Office VPN", IF_PPP, false, true, "172.20.0.9/32"),
            adapter("A tunnel", IF_TUNNEL, false, false, "10.9.9.9/32"),
            // A tunnel that called itself hardware is still a tunnel.
            adapter("Odd tunnel", IF_PROP_VIRTUAL, true, true, "10.1.1.1/24"),
        ];
        for adapter in &tunnels {
            assert_eq!(left_out(adapter), Some(LeftOut::Tunnel), "{adapter:?}");
        }
        let virtuals = [
            adapter(
                "vEthernet (WSL (Hyper-V firewall))",
                ETHERNET,
                false,
                false,
                "172.25.48.1/20",
            ),
            adapter(
                "vEthernet (Default Switch)",
                ETHERNET,
                false,
                false,
                "172.31.96.1/20",
            ),
            adapter(
                "VirtualBox Host-Only Network",
                ETHERNET,
                false,
                false,
                "192.168.56.1/24",
            ),
            adapter(
                "VMware Network Adapter VMnet8",
                ETHERNET,
                false,
                false,
                "192.168.13.1/24",
            ),
            // OpenVPN's TAP adapter carrying a full tunnel: Ethernet by type, with a gateway, and
            // still not hardware.
            adapter("OpenVPN TAP-Windows6", ETHERNET, false, true, "10.8.0.6/24"),
            adapter(
                "Local Area Connection* 10",
                WIFI,
                false,
                false,
                "192.168.137.1/24",
            ),
        ];
        for adapter in &virtuals {
            assert_eq!(left_out(adapter), Some(LeftOut::Virtual), "{adapter:?}");
            let hardware = Adapter {
                hardware: true,
                ..adapter.clone()
            };
            assert_eq!(left_out(&hardware), None, "control: {hardware:?}");
        }
    }

    /// ★ The choices are the private addresses of the adapters kept, with the adapter's name, index
    /// and GUID; the route's address on a full-tunnel VPN is not among them, so the listener goes
    /// to the shack's own network, and with Wi-Fi and Ethernet both up and no route through either,
    /// the operator picks. CONTROL: the same adapters unfiltered would have put it on the VPN.
    #[test]
    fn the_choice_leaves_the_vpn_and_the_virtual_switch_out() {
        let wifi = adapter("Wi-Fi", WIFI, true, true, "192.168.1.20/24");
        let wsl = adapter("vEthernet (WSL)", ETHERNET, false, false, "172.25.48.1/20");
        let vpn = adapter("NordLynx", IF_PROP_VIRTUAL, false, true, "10.5.0.2/16");
        let found = choices(&[wifi.clone(), wsl.clone(), vpn.clone()]);
        assert_eq!(
            found,
            vec![Choice {
                network: Network::new(v4("192.168.1.20"), 24).unwrap(),
                name: "Wi-Fi".into(),
                index: 7,
                id: "{Wi-Fi}".into(),
            }]
        );
        let pairs = |list: &[Choice]| -> Vec<(Ipv4Addr, u8)> {
            list.iter()
                .map(|c| (c.network.address(), c.network.prefix()))
                .collect()
        };
        // The full-tunnel VPN carries the route: the listener stays on Wi-Fi.
        assert_eq!(
            choose(None, Some(v4("10.5.0.2")), &pairs(&found)),
            Ok(Network::new(v4("192.168.1.20"), 24).unwrap())
        );
        // CONTROL: unfiltered, the same route would have put it on the VPN.
        let unfiltered: Vec<(Ipv4Addr, u8)> = [&wifi, &wsl, &vpn]
            .iter()
            .flat_map(|a| a.addresses.clone())
            .collect();
        assert_eq!(
            choose(None, Some(v4("10.5.0.2")), &unfiltered),
            Ok(Network::new(v4("10.5.0.2"), 16).unwrap())
        );
        // A pick on the VPN is not this computer's to listen on.
        assert_eq!(
            choose(Some(v4("10.5.0.2")), None, &pairs(&found)),
            Err(NoNetwork::Gone)
        );
        // Wi-Fi and Ethernet both up, the route through neither: the operator picks.
        let ethernet = adapter("Ethernet", ETHERNET, true, false, "10.0.0.5/24");
        let both = choices(&[wifi, ethernet, vpn]);
        assert_eq!(both.len(), 2);
        assert_eq!(
            choose(None, Some(v4("10.5.0.2")), &pairs(&both)),
            Err(NoNetwork::Choose)
        );
        let look = Look {
            network: choose(Some(v4("10.0.0.5")), None, &pairs(&both)),
            choices: both,
        };
        assert_eq!(look.chosen().map(|c| c.name.as_str()), Some("Ethernet"));
        // A public address on a kept adapter is no choice either.
        let public = adapter("Ethernet 3", ETHERNET, true, true, "203.0.113.5/24");
        assert!(choices(&[public]).is_empty());
    }

    // ----- The other computer's side -----

    /// The typed address: the one the shack shows, with or without its port, spaces trimmed; with
    /// none, the default port. Refused: a name, IPv6, a public or loopback address, a system port,
    /// garbage. CONTROL: each refusal's private-address twin is taken.
    #[test]
    fn a_typed_address_is_a_private_ipv4_address_and_a_port() {
        for (text, want) in [
            ("192.168.1.20", "192.168.1.20:42075"),
            ("  192.168.1.20:42075 ", "192.168.1.20:42075"),
            ("10.0.0.5:50000", "10.0.0.5:50000"),
            ("172.16.0.1:1024", "172.16.0.1:1024"),
        ] {
            assert_eq!(typed(text), Some(want.parse().unwrap()), "{text}");
        }
        for text in [
            "",
            "nexus.local",
            "nexus-3f2a9b1c.local:42075",
            "fe80::1",
            "[fd00::1]:42075",
            "203.0.113.5",
            "127.0.0.1:42075",
            "100.85.1.2",
            "192.168.1.20:80",
            "192.168.1.20:",
            "192.168.1.20:99999",
            "192.168.1",
        ] {
            assert_eq!(typed(text), None, "{text}");
        }
    }

    /// Why a computer could not reach the shack, from the error its connection got: another
    /// network first, whatever the error; then a refusal; everything else is no answer, which the
    /// words do not pretend to explain.
    #[test]
    fn an_unreached_shack_is_another_network_a_refusal_or_no_answer() {
        use std::io::{Error, ErrorKind};
        assert_eq!(
            unreached(&Error::from(ErrorKind::ConnectionRefused), false),
            "otherNetwork"
        );
        assert_eq!(
            unreached(&Error::from(ErrorKind::ConnectionRefused), true),
            "refused"
        );
        for kind in [
            ErrorKind::TimedOut,
            ErrorKind::HostUnreachable,
            ErrorKind::NetworkUnreachable,
            ErrorKind::ConnectionReset,
        ] {
            assert_eq!(unreached(&Error::from(kind), true), "noAnswer", "{kind:?}");
        }
    }
}
