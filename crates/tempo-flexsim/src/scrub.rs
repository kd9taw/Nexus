//! What the recorder removes from a session before anything is written.
//!
//! **Replaced**, each distinct original by its own stable placeholder, so two fields that held the
//! same address still match after scrubbing:
//!
//! - **IP addresses**, v4 and v6, in any value and in free text: IPv4 becomes `192.0.2.x` (then
//!   `198.51.100.x`, `203.0.113.x`, the RFC 5737 documentation ranges) and IPv6 `2001:db8::x`
//!   (RFC 3849). Two exceptions. The unspecified address (`0.0.0.0`, `::`) is kept: it is a
//!   protocol marker, not an address (a stream with `client_handle=0` and `ip=0.0.0.0` is a dead
//!   orphan, port plan §4.3). Values of version keys (`…version`, `…_ver`) are kept: firmware
//!   versions are dotted numbers too.
//! - **MAC addresses** (the `mac` key, or any `hh:hh:hh:hh:hh:hh`): `02:00:00:00:xx:xx`.
//! - **Serial numbers** (any key containing `serial`): `serial-N`.
//! - **Client ids** (`client_id`): a UUID-shaped placeholder.
//! - **Host and station names** (keys containing `host` or `station`): `host-N`, `station-N`. A
//!   station name is usually the computer's name.
//! - **GPS position and location** (`lat`, `lon`, `altitude`, `grid`, `location`): `0`, `AA00`, or
//!   empty.
//!
//! **Kept**: everything else, including the callsign and the radio's nickname, which the tester
//! sees in the file and decides about. Free-text messages are scanned for addresses only.
//!
//! The scan knows the shapes of the API's text. Status pairs are separated by spaces, `#` (meter
//! and GPS status) or commas (the `info` reply), a value can be quoted, and a value can be a
//! comma-separated list (`gui_client_hosts=a,b`). A comma separates two pairs only when a
//! `key=` follows it. When in doubt it replaces: a dotted quad under an unrecognised key is
//! treated as an address, which can cost a version number but never leaks an address.

use std::collections::HashMap;
use std::net::Ipv6Addr;

/// The kinds of thing replaced, in report order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ipv4,
    Ipv6,
    Mac,
    Serial,
    ClientId,
    Host,
    Station,
    Position,
}

const KINDS: [Kind; 8] = [
    Kind::Ipv4,
    Kind::Ipv6,
    Kind::Mac,
    Kind::Serial,
    Kind::ClientId,
    Kind::Host,
    Kind::Station,
    Kind::Position,
];

impl Kind {
    /// How the recorder's summary names this kind.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Ipv4 => "IPv4 addresses",
            Kind::Ipv6 => "IPv6 addresses",
            Kind::Mac => "MAC addresses",
            Kind::Serial => "serial numbers",
            Kind::ClientId => "client ids",
            Kind::Host => "host names",
            Kind::Station => "station names",
            Kind::Position => "GPS position and location fields",
        }
    }
}

/// Replaces addresses and identifiers line by line, remembering its placeholders.
#[derive(Debug, Default)]
pub struct Scrubber {
    /// Per kind: original → placeholder.
    seen: HashMap<(usize, String), String>,
    /// Per kind: how many distinct originals.
    distinct: [usize; KINDS.len()],
}

impl Scrubber {
    pub fn new() -> Self {
        Self::default()
    }

    /// The line with every address and identifier replaced.
    pub fn line(&mut self, line: &str) -> String {
        let bytes = line.as_bytes();
        let mut out = String::with_capacity(line.len());
        let mut start = 0;
        let mut quoted = false;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'"' {
                quoted = !quoted;
                continue;
            }
            let splits = !quoted
                && match b {
                    b' ' | b'|' | b'#' => true,
                    b',' => starts_with_key(&bytes[i + 1..]),
                    _ => false,
                };
            if splits {
                out.push_str(&self.field(&line[start..i]));
                out.push(char::from(b));
                start = i + 1;
            }
        }
        out.push_str(&self.field(&line[start..]));
        out
    }

    /// What was replaced: each kind with how many distinct originals, kinds with none left out.
    pub fn report(&self) -> Vec<(Kind, usize)> {
        KINDS
            .iter()
            .zip(self.distinct)
            .filter(|(_, n)| *n > 0)
            .map(|(k, n)| (*k, n))
            .collect()
    }

    fn field(&mut self, field: &str) -> String {
        let Some((key, value)) = field.split_once('=') else {
            return self.scan(field);
        };
        if !is_key(key) {
            return self.scan(field);
        }
        let k = key.to_ascii_lowercase();
        let (quoted, inner) = match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            Some(inner) => (true, inner),
            None => (false, value),
        };
        let replaced = match key_kind(&k) {
            _ if inner.is_empty() => String::new(),
            Some(Kind::Position) => {
                self.note(Kind::Position, inner);
                match k.as_str() {
                    "grid" => "AA00".to_string(),
                    "location" => String::new(),
                    _ => "0".to_string(),
                }
            }
            Some(kind) => inner
                .split(',')
                .map(|item| match item {
                    "" => String::new(),
                    _ => self.placeholder(kind, item),
                })
                .collect::<Vec<_>>()
                .join(","),
            None if k.ends_with("version") || k.ends_with("_ver") => inner.to_string(),
            None => self.scan(inner),
        };
        if quoted {
            format!("{key}=\"{replaced}\"")
        } else {
            format!("{key}={replaced}")
        }
    }

    /// Replace every IP or MAC address inside free text.
    fn scan(&mut self, text: &str) -> String {
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let (mut i, mut copied) = (0, 0);
        while i < bytes.len() {
            if !is_address_char(bytes[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && is_address_char(bytes[i]) {
                i += 1;
            }
            // A sentence can end an address with a full stop.
            let mut end = i;
            while end > start && bytes[end - 1] == b'.' {
                end -= 1;
            }
            let run = &text[start..end];
            if let Some(kind) = address_kind(run) {
                out.push_str(&text[copied..start]);
                let replacement = self.placeholder(kind, run);
                out.push_str(&replacement);
                copied = end;
            }
        }
        out.push_str(&text[copied..]);
        out
    }

    /// Count an original the first time it is seen, and give it its placeholder.
    fn note(&mut self, kind: Kind, original: &str) {
        let slot = KINDS.iter().position(|k| *k == kind).unwrap_or(0);
        let key = (slot, original.to_string());
        if !self.seen.contains_key(&key) {
            self.distinct[slot] += 1;
            let placeholder = make_placeholder(kind, self.distinct[slot]);
            self.seen.insert(key, placeholder);
        }
    }

    fn placeholder(&mut self, kind: Kind, original: &str) -> String {
        self.note(kind, original);
        let slot = KINDS.iter().position(|k| *k == kind).unwrap_or(0);
        self.seen[&(slot, original.to_string())].clone()
    }
}

fn make_placeholder(kind: Kind, n: usize) -> String {
    match kind {
        Kind::Ipv4 => {
            const BLOCKS: [&str; 3] = ["192.0.2", "198.51.100", "203.0.113"];
            let i = n - 1;
            format!("{}.{}", BLOCKS[(i / 254) % 3], i % 254 + 1)
        }
        Kind::Ipv6 => format!("2001:db8::{n:x}"),
        Kind::Mac => format!("02:00:00:00:{:02x}:{:02x}", (n >> 8) & 0xFF, n & 0xFF),
        Kind::Serial => format!("serial-{n}"),
        Kind::ClientId => format!("00000000-0000-4000-8000-{n:012x}"),
        Kind::Host => format!("host-{n}"),
        Kind::Station => format!("station-{n}"),
        Kind::Position => String::new(),
    }
}

/// What a key's whole value is, when the key alone says so.
fn key_kind(key: &str) -> Option<Kind> {
    match key {
        "mac" => Some(Kind::Mac),
        "client_id" => Some(Kind::ClientId),
        "lat" | "lon" | "latitude" | "longitude" | "altitude" | "grid" | "location" => {
            Some(Kind::Position)
        }
        k if k.contains("serial") => Some(Kind::Serial),
        k if k.contains("host") => Some(Kind::Host),
        k if k.contains("station") => Some(Kind::Station),
        _ => None,
    }
}

/// Whether a run of text is an address: a MAC, an IPv6 address or a dotted-quad IPv4 address,
/// other than the unspecified address.
fn address_kind(run: &str) -> Option<Kind> {
    if is_mac(run) {
        return Some(Kind::Mac);
    }
    if run.contains(':') {
        let ip: Ipv6Addr = run.parse().ok()?;
        return (!ip.is_unspecified()).then_some(Kind::Ipv6);
    }
    let parts: Vec<&str> = run.split('.').collect();
    let quad = parts.len() == 4
        && parts.iter().all(|p| {
            (1..=3).contains(&p.len())
                && p.bytes().all(|b| b.is_ascii_digit())
                && p.parse::<u16>().is_ok_and(|v| v <= 255)
        });
    (quad && run != "0.0.0.0").then_some(Kind::Ipv4)
}

fn is_mac(run: &str) -> bool {
    let b = run.as_bytes();
    b.len() == 17
        && b.iter().enumerate().all(|(i, c)| {
            if i % 3 == 2 {
                *c == b':'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

fn is_address_char(b: u8) -> bool {
    b.is_ascii_hexdigit() || b == b'.' || b == b':'
}

fn is_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// Whether the bytes start with `key=`: the test for a comma that separates two pairs.
fn starts_with_key(rest: &[u8]) -> bool {
    let n = rest
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(**b, b'_' | b'.' | b'-'))
        .count();
    n > 0 && rest.get(n) == Some(&b'=')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The documented `info` reply's shape (TCPIP-info), with invented values.
    const INFO: &str = "model=\"FLEX-6400\",chassis_serial=\"0000-1111-6400-0001\",name=\"Shack\",\
        callsign=\"N0CALL\",gps=\"Not Present\",scu=1,slice=2,software_ver=4.2.18.41174,\
        mac=00:1C:2D:00:00:01,ip=192.168.1.20,netmask=255.255.255.0,gateway=192.168.1.1,\
        location=\"Grid square EN61, back bedroom\"";

    #[test]
    fn the_info_reply_loses_its_serial_mac_addresses_and_location() {
        let mut s = Scrubber::new();
        let out = s.line(INFO);
        assert_eq!(
            out,
            "model=\"FLEX-6400\",chassis_serial=\"serial-1\",name=\"Shack\",callsign=\"N0CALL\",\
             gps=\"Not Present\",scu=1,slice=2,software_ver=4.2.18.41174,\
             mac=02:00:00:00:00:01,ip=192.0.2.1,netmask=192.0.2.2,gateway=192.0.2.3,location=\"\""
        );
        assert_eq!(
            s.report(),
            vec![
                (Kind::Ipv4, 3),
                (Kind::Mac, 1),
                (Kind::Serial, 1),
                (Kind::Position, 1)
            ]
        );
    }

    #[test]
    fn status_lines_lose_identifiers_and_keep_their_protocol_values() {
        let mut s = Scrubber::new();
        let line = "S7A3C0001|client 0x7A3C0001 connected local_ptt=1 \
                    client_id=9D2E4F60-0000-4000-8000-00000000F001 program=SmartSDR-Win station=Shack-PC";
        assert_eq!(
            s.line(line),
            "S7A3C0001|client 0x7A3C0001 connected local_ptt=1 \
             client_id=00000000-0000-4000-8000-000000000001 program=SmartSDR-Win station=station-1"
        );
        // A stream's address is replaced; the unspecified address is a protocol marker and stays.
        assert_eq!(
            s.line("S0|stream 0x04000001 type=dax_rx client_handle=0x2B6E1F40 ip=10.1.2.3"),
            "S0|stream 0x04000001 type=dax_rx client_handle=0x2B6E1F40 ip=192.0.2.1"
        );
        let orphan = "S0|stream 0x04000002 type=dax_rx client_handle=0x00000000 ip=0.0.0.0";
        assert_eq!(s.line(orphan), orphan);
        // Frequencies, handles, stream ids, meter definitions and versions pass untouched.
        for unchanged in [
            "S2B6E1F40|slice 0 in_use=1 RF_frequency=14.074000 mode=DIGU tx=1 client_handle=0x2B6E1F40",
            "S0|meter 1.src=TX-#1.num=1#1.nam=FWDPWR#1.unit=dBm#1.low=-150.0#1.hi=60.0#",
            "R12|0|0x40000000,0x42000000",
            "S0|interlock tx_client_handle=0x00000000 state=READY reason= source= tx_allowed=1 amplifier=",
            "S0|radio version=1.4.0.0 max_licensed_version=v4",
        ] {
            assert_eq!(s.line(unchanged), unchanged);
        }
        // A bare dotted quad is taken for an address, which is why the recorder writes the
        // prologue's version line without scrubbing it.
        assert_ne!(s.line("V1.4.0.0"), "V1.4.0.0");
    }

    #[test]
    fn the_same_original_keeps_its_placeholder_and_lists_are_split() {
        let mut s = Scrubber::new();
        let out = s.line(
            "S0|radio gui_client_ips=192.168.1.5,192.168.1.6 gui_client_hosts=DESKTOP-1,LAPTOP-2 \
             gui_client_stations=Shack,Office",
        );
        assert_eq!(
            out,
            "S0|radio gui_client_ips=192.0.2.1,192.0.2.2 gui_client_hosts=host-1,host-2 \
             gui_client_stations=station-1,station-2"
        );
        assert_eq!(
            s.line("ip=192.168.1.6"),
            "ip=192.0.2.2",
            "a placeholder is stable"
        );
    }

    #[test]
    fn free_text_and_unknown_keys_are_scanned_for_addresses() {
        let mut s = Scrubber::new();
        assert_eq!(
            s.line("M10000001|Client connected from 192.168.7.9. Link fe80::1c2d:ff:fe00:1 up"),
            "M10000001|Client connected from 192.0.2.1. Link 2001:db8::1 up"
        );
        assert_eq!(s.line("S0|foo bar=10.0.0.1"), "S0|foo bar=192.0.2.2");
        assert_eq!(
            s.line("hwaddr 00:1c:2d:ab:cd:ef"),
            "hwaddr 02:00:00:00:00:01"
        );
        assert_eq!(s.line("S0|x unspecified=::"), "S0|x unspecified=::");
        // Not addresses: times, three-part numbers, five parts, out-of-range octets.
        for unchanged in [
            "time=12:34:56Z",
            "cal=15.000.1",
            "x=1.2.3.4.5",
            "x=300.1.1.1",
        ] {
            assert_eq!(s.line(unchanged), unchanged);
        }
    }

    #[test]
    fn gps_position_is_removed() {
        let mut s = Scrubber::new();
        let out = s.line(
            "S0|gps status=Fine Lock#tracked=8#visible=11#grid=EN61fv#altitude=180 m#\
             lat=41.881832#lon=-87.623177#time=12:34:56Z#freq_error=0 ppb",
        );
        assert_eq!(
            out,
            "S0|gps status=Fine Lock#tracked=8#visible=11#grid=AA00#altitude=0 m#\
             lat=0#lon=0#time=12:34:56Z#freq_error=0 ppb"
        );
        for gone in ["EN61", "41.88", "87.62", "180"] {
            assert!(!out.contains(gone), "{gone} survived");
        }
    }
}
