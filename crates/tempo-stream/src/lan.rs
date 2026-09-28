//! No LAN address leaves the shack (security acceptance test A8).
//!
//! The station advertises only server-reflexive candidates (and relay ones, once TURN exists):
//! the address a STUN server saw, never the interface address behind the NAT. Its WebRTC agent does
//! hold the interface address, which it needs to use its own socket, but only from after the answer
//! is written, and it is never signalled (see `session`); the transport writes a reflexive
//! candidate's `raddr` as `0.0.0.0 0`. [`leaks`] is the last lock behind that: every SDP and
//! candidate the station is about to send is read for a host candidate or a private, link-local,
//! carrier-NAT or unique-local address, and a leak is never sent.
use std::net::IpAddr;

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

/// Does this SDP, or this candidate line, give away the shack's LAN? A host candidate does, and so
/// does any private address anywhere in the text. The unspecified addresses (`0.0.0.0`, `::`) and
/// loopback identify nothing and pass.
pub fn leaks(text: &str) -> bool {
    for line in text.lines() {
        let line = line.trim();
        let candidate = line
            .strip_prefix("a=")
            .unwrap_or(line)
            .starts_with("candidate:");
        let words: Vec<&str> = line.split_whitespace().collect();
        // `typ host`: an interface address, whatever it is.
        if candidate && words.windows(2).any(|w| w == ["typ", "host"]) {
            return true;
        }
        for word in line.split(|c: char| c.is_whitespace() || c == '=' || c == ',') {
            if let Ok(ip) = word.parse::<IpAddr>() {
                if private(ip) {
                    return true;
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

    /// CONTROL first: the contract's station answer and candidate pass.
    #[test]
    fn the_contract_station_messages_leak_nothing() {
        let sent = fixture_station_messages();
        assert_eq!(sent.len(), 2, "premise: the answer and a candidate");
        for text in sent {
            assert!(!leaks(&text), "{text}");
        }
    }

    /// ★ A8's text half: a host candidate or any LAN address is caught, in SDP and trickled lines.
    #[test]
    fn a_host_candidate_or_a_lan_address_is_caught() {
        for leak in [
            "candidate:1 1 udp 2130706431 192.168.1.20 61000 typ host",
            "a=candidate:1 1 udp 2130706431 203.0.113.7 61000 typ host",
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
            assert!(leaks(leak), "missed: {leak}");
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
            assert!(!leaks(fine), "false alarm: {fine}");
        }
    }
}
