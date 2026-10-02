//! The one STUN exchange the station makes itself: a binding request to a public STUN server, to
//! learn the address its NAT maps the stream's socket to. That address is the station's
//! server-reflexive candidate, the only kind it advertises (see [`crate::lan`]).
//!
//! RFC 5389, the minimum: a 20-byte request, and XOR-MAPPED-ADDRESS (or the older MAPPED-ADDRESS)
//! read out of the success response that carries the same transaction id. The transport keeps
//! running its own ICE checks on the same socket; a packet is ours only if it is a binding success
//! for OUR transaction id, so the two never confuse each other.
//!
//! The transaction id comes from the caller's operating-system randomness, so an off-path host
//! cannot forge a response with a mapped address of its choosing. A forged one could only break
//! connectivity, never read or alter media: DTLS protects those whatever candidate is used.
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// The STUN server the station asks. Cloudflare's, the same operator as the relay and the planned
/// TURN service, so the stream adds no new party. It sees only the station's public address, which
/// every server the station talks to sees.
pub const SERVER: &str = "stun.cloudflare.com:3478";

const MAGIC: u32 = 0x2112_A442;
const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS: u16 = 0x0101;
const MAPPED_ADDRESS: u16 = 0x0001;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;

/// A binding request carrying `transaction`.
pub fn request(transaction: &[u8; 12]) -> [u8; 20] {
    let mut out = [0u8; 20];
    out[0..2].copy_from_slice(&BINDING_REQUEST.to_be_bytes());
    // Length 0: no attributes.
    out[4..8].copy_from_slice(&MAGIC.to_be_bytes());
    out[8..20].copy_from_slice(transaction);
    out
}

/// The mapped address in `packet`, if it is the binding success for `transaction`. Anything else
/// (an ICE check, a response to another transaction, a malformed packet) is `None`.
pub fn mapped(packet: &[u8], transaction: &[u8; 12]) -> Option<SocketAddr> {
    if packet.len() < 20 || packet[0] & 0xc0 != 0 {
        return None;
    }
    let kind = u16::from_be_bytes([packet[0], packet[1]]);
    let length = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
    if kind != BINDING_SUCCESS
        || packet[4..8] != MAGIC.to_be_bytes()
        || packet[8..20] != transaction[..]
        || length % 4 != 0
        || packet.len() != 20 + length
    {
        return None;
    }
    let mut plain = None;
    let mut at = 20;
    while at + 4 <= packet.len() {
        let attribute = u16::from_be_bytes([packet[at], packet[at + 1]]);
        let size = usize::from(u16::from_be_bytes([packet[at + 2], packet[at + 3]]));
        let value = packet.get(at + 4..at + 4 + size)?;
        match attribute {
            XOR_MAPPED_ADDRESS => return address(value, Some(transaction)),
            MAPPED_ADDRESS => plain = plain.or_else(|| address(value, None)),
            _ => {}
        }
        // Attributes are padded to four bytes.
        at += 4 + size.div_ceil(4) * 4;
    }
    plain
}

/// A (XOR-)MAPPED-ADDRESS value. `xor` carries the transaction id when the value is XOR'd.
fn address(value: &[u8], xor: Option<&[u8; 12]>) -> Option<SocketAddr> {
    if value.len() < 4 {
        return None;
    }
    let magic = MAGIC.to_be_bytes();
    let mut port = u16::from_be_bytes([value[2], value[3]]);
    if xor.is_some() {
        port ^= u16::from_be_bytes([magic[0], magic[1]]);
    }
    let ip = match (value[1], value.len()) {
        (0x01, 8) => {
            let mut octets = [value[4], value[5], value[6], value[7]];
            if xor.is_some() {
                for (o, m) in octets.iter_mut().zip(magic) {
                    *o ^= m;
                }
            }
            IpAddr::V4(Ipv4Addr::from(octets))
        }
        (0x02, 20) => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&value[4..20]);
            if let Some(transaction) = xor {
                let key: Vec<u8> = magic.iter().chain(transaction.iter()).copied().collect();
                for (o, k) in octets.iter_mut().zip(key) {
                    *o ^= k;
                }
            }
            IpAddr::V6(Ipv6Addr::from(octets))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TX: [u8; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

    /// A binding success for `tx` carrying `attributes`, as a server writes it.
    fn success(tx: &[u8; 12], attributes: &[(u16, Vec<u8>)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (kind, value) in attributes {
            body.extend_from_slice(&kind.to_be_bytes());
            body.extend_from_slice(&(value.len() as u16).to_be_bytes());
            body.extend_from_slice(value);
            while body.len() % 4 != 0 {
                body.push(0);
            }
        }
        let mut out = Vec::new();
        out.extend_from_slice(&BINDING_SUCCESS.to_be_bytes());
        out.extend_from_slice(&(body.len() as u16).to_be_bytes());
        out.extend_from_slice(&MAGIC.to_be_bytes());
        out.extend_from_slice(tx);
        out.extend_from_slice(&body);
        out
    }

    /// XOR-MAPPED-ADDRESS for 203.0.113.7:61000, worked by hand from RFC 5389 section 15.2.
    fn xor_v4() -> Vec<u8> {
        let port = 61000u16 ^ 0x2112;
        // 203.0.113.7 XOR the magic cookie; the second octet is 0 ^ 0x12.
        let ip = [203 ^ 0x21, 0x12, 113 ^ 0xA4, 7 ^ 0x42];
        let mut v = vec![0, 0x01];
        v.extend_from_slice(&port.to_be_bytes());
        v.extend_from_slice(&ip);
        v
    }

    #[test]
    fn a_request_is_a_bare_binding_request() {
        let r = request(&TX);
        assert_eq!(&r[0..4], &[0x00, 0x01, 0x00, 0x00]);
        assert_eq!(&r[4..8], &[0x21, 0x12, 0xA4, 0x42]);
        assert_eq!(&r[8..20], &TX);
    }

    #[test]
    fn the_reflexive_address_is_read_from_our_response() {
        let packet = success(
            &TX,
            &[(0x8022, b"srv".to_vec()), (XOR_MAPPED_ADDRESS, xor_v4())],
        );
        assert_eq!(
            mapped(&packet, &TX),
            Some("203.0.113.7:61000".parse().unwrap())
        );
    }

    #[test]
    fn an_ipv6_reflexive_address_is_read() {
        let want: Ipv6Addr = "2001:db8::7".parse().unwrap();
        let key: Vec<u8> = MAGIC
            .to_be_bytes()
            .iter()
            .chain(TX.iter())
            .copied()
            .collect();
        let mut v = vec![0, 0x02];
        v.extend_from_slice(&(61000u16 ^ 0x2112).to_be_bytes());
        v.extend(want.octets().iter().zip(key).map(|(o, k)| o ^ k));
        let packet = success(&TX, &[(XOR_MAPPED_ADDRESS, v)]);
        assert_eq!(
            mapped(&packet, &TX),
            Some(SocketAddr::new(want.into(), 61000))
        );
    }

    /// An old server that sends only the plain MAPPED-ADDRESS still works.
    #[test]
    fn a_plain_mapped_address_is_read() {
        let mut v = vec![0, 0x01];
        v.extend_from_slice(&61000u16.to_be_bytes());
        v.extend_from_slice(&[203, 0, 113, 7]);
        let packet = success(&TX, &[(MAPPED_ADDRESS, v)]);
        assert_eq!(
            mapped(&packet, &TX),
            Some("203.0.113.7:61000".parse().unwrap())
        );
    }

    /// Nothing that is not our success is mistaken for one: another transaction, a request (an
    /// ICE check arriving on the same socket), a truncated packet.
    #[test]
    fn only_our_own_success_is_read() {
        let packet = success(&TX, &[(XOR_MAPPED_ADDRESS, xor_v4())]);
        let other = [9u8; 12];
        assert_eq!(
            mapped(&packet, &other),
            None,
            "another transaction's answer was read"
        );
        assert_eq!(
            mapped(&request(&TX), &TX),
            None,
            "a request was read as an answer"
        );
        assert_eq!(
            mapped(&packet[..packet.len() - 4], &TX),
            None,
            "a truncated packet was read"
        );
        let mut dtls = packet.clone();
        dtls[0] = 0x16; // a DTLS record's first byte
        assert_eq!(mapped(&dtls, &TX), None);
        // CONTROL: the unaltered packet is read.
        assert!(mapped(&packet, &TX).is_some());
    }
}
