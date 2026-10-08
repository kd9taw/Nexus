//! The radio's capabilities reply, read strictly; choosing a radio from it; the sample-rate
//! bitmaps; and the six network models with the name each one's server reports.
//!
//! The radio answers the token-create request with a count of radios (big-endian 16 at `0x40`)
//! and that many fixed `0x66`-byte entries from `0x42`. A radio's own server advertises one; a PC
//! running RS-BA1 can advertise several, which is what [`Choice`] selects between. Each entry
//! carries the radio's 16-byte identity (echoed back, untouched, in the connection-info request),
//! its name, its audio device name, the link type, its CI-V address, and two sample-rate bitmaps
//! ([`Rates`]); a transmit bitmap of 0 means the radio offers no transmit audio at all.
//!
//! The identity block is either a GUID or, when the `commoncap` word overlapping it says so, a
//! block holding the radio's MAC address; both radios upstream examined (an IC-7610 and an
//! IC-9700, on Ethernet) report MAC mode. Either way all 16 bytes are kept as they arrived.
//!
//! The six network models and their names are facts read from the upstream model descriptors
//! (`rigs/icom/ic7610net.c`, `rigs/icom/ic9700net.c`, `rigs/icom/ic705net.c`,
//! `rigs/icom/ic905net.c`, `rigs/icom/ic7760net.c`, `rigs/icom/ic7300mk2net.c`); upstream ran the
//! first three against hardware and marks the IC-905, IC-7760 and IC-7300MK2 untested over the
//! network. The layout facts above are also in `rigs/icom/ICOM.md` §5; no text or code is taken
//! from either.
//!
//! PORTED from Hamlib (https://github.com/Hamlib/Hamlib, pull request #2178, open when taken),
//! `rigs/icom/network_proto.c` and `rigs/icom/network_proto.h` at commit
//! `2e3e4a6add3bd806e828d035200777608d2bdbd5` (2026-10-07), translated from C to Rust.
//! Deliberate differences: a capabilities reply whose radio count disagrees with its length is an
//! error, where upstream keeps the entries that fit; there is no limit of eight radios, since the
//! entries are a list; names are kept as text up to the first NUL with trailing spaces removed,
//! as upstream does, and compared without regard to ASCII case; the rate list is built as a value
//! rather than written into a caller's buffer, so it cannot be truncated; the link type is a typed
//! value; the models are a typed list.
//! The protocol knowledge descends from kappanhang (https://github.com/nonoo/kappanhang, MIT), as the
//! upstream author states. Recorded in the repo-root NOTICE (Hamlib entry), which reproduces
//! kappanhang's notice.
//!
//! The upstream notice, converted to the GNU General Public License under section 3 of the GNU
//! Lesser General Public License, version 2.1, and otherwise unchanged:
//!
//! Copyright (c) 2026 by Mikael Nousiainen OH3BHX
//!
//! This library is free software; you can redistribute it and/or modify it under the terms of the
//! GNU General Public License as published by the Free Software Foundation; either version 3 of the
//! License, or (at your option) any later version.
//!
//! This library is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
//! without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See
//! the GNU General Public License for more details.
//!
//! You should have received a copy of the GNU General Public License along with this library; if
//! not, write to the Free Software Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA
//! 02110-1301 USA
//!
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Modified by KD9TAW, 2026-10-07: translated from C to Rust and changed as listed
//! above. These changes are licensed under the GNU General Public License, version 3 only, as Nexus
//! is (see COPYING), so this file as a whole is distributed under GPL version 3.

use std::fmt;

use super::wire::{be16, be32, le16, text_of, WireError};

/// Where the radio count sits.
const OFF_COUNT: usize = 0x40; // be16
/// Where entry 0 starts.
pub const FIRST_ENTRY: usize = 0x42;
/// The stride between entries.
pub const ENTRY_LEN: usize = 0x66;

// Within an entry.
const CAP_OFF_IDENTITY: usize = 0x00; // 16 bytes, a GUID or a MAC block
const CAP_OFF_COMMONCAP: usize = 0x07; // le16, overlaps the identity
const CAP_OFF_MAC: usize = 0x0a; // 6 bytes, in MAC mode
const CAP_OFF_NAME: usize = 0x10; // 32 bytes
const CAP_OFF_AUDIO: usize = 0x30; // 32 bytes
const CAP_OFF_CONNTYPE: usize = 0x50; // le16
const CAP_OFF_CIV_ADDR: usize = 0x52; // u8
const CAP_OFF_RX_RATE: usize = 0x53; // le16 bitmap
const CAP_OFF_TX_RATE: usize = 0x55; // le16 bitmap, 0 = no transmit audio
const CAP_OFF_BAUD: usize = 0x5a; // be32

/// The `commoncap` value that marks the identity block as holding a MAC address.
pub const COMMONCAP_MAC: u16 = 0x8010;

// Rate flags, as the bitmap reads little-endian. Most sit in the second wire byte; 24 kHz is the
// one known flag in the first. A radio offering everything advertises 0x8b01.
pub const RATE_12000: u16 = 0x8000;
pub const RATE_44100: u16 = 0x4000;
pub const RATE_22050: u16 = 0x2000;
pub const RATE_11025: u16 = 0x1000;
pub const RATE_48000: u16 = 0x0800;
pub const RATE_32000: u16 = 0x0400;
pub const RATE_16000: u16 = 0x0200;
pub const RATE_8000: u16 = 0x0100;
pub const RATE_24000: u16 = 0x0001;

/// Each flag with the rate it advertises, highest rate first.
const RATE_BITS: [(u16, u32); 9] = [
    (RATE_48000, 48000),
    (RATE_44100, 44100),
    (RATE_32000, 32000),
    (RATE_24000, 24000),
    (RATE_22050, 22050),
    (RATE_16000, 16000),
    (RATE_12000, 12000),
    (RATE_11025, 11025),
    (RATE_8000, 8000),
];

/// A sample-rate capability bitmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rates(pub u16);

impl Rates {
    /// Whether the bitmap advertises `hz`. A rate the protocol has no flag for is never
    /// supported.
    pub fn supports(self, hz: u32) -> bool {
        RATE_BITS
            .iter()
            .find(|(_, rate)| *rate == hz)
            .is_some_and(|(bit, _)| self.0 & bit != 0)
    }

    /// The advertised rates, highest first.
    pub fn list(self) -> Vec<u32> {
        RATE_BITS
            .iter()
            .filter(|(bit, _)| self.0 & bit != 0)
            .map(|(_, rate)| *rate)
            .collect()
    }
}

impl fmt::Display for Rates {
    /// "48000, 24000", or "none".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rates = self.list();
        if rates.is_empty() {
            return f.write_str("none");
        }
        for (i, rate) in rates.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{rate}")?;
        }
        Ok(())
    }
}

/// How the radio is attached to the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Wifi,
    Ethernet,
    Other(u16),
}

/// One radio entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadioCap {
    /// The first 16 bytes, verbatim: the connection-info request echoes them to select this radio.
    pub identity: [u8; 16],
    /// The flag word overlapping the identity.
    pub commoncap: u16,
    /// The radio's MAC address, when `commoncap` selects MAC addressing.
    pub mac: Option<[u8; 6]>,
    pub name: String,
    pub audio: String,
    pub link: Link,
    /// The radio's CI-V address.
    pub civ_addr: u8,
    pub rx_rates: Rates,
    /// 0 when the radio offers no transmit audio.
    pub tx_rates: Rates,
    /// The CI-V link baud rate the radio reports.
    pub baud: u32,
}

/// A decoded capabilities reply.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Capabilities {
    pub radios: Vec<RadioCap>,
}

/// Reads a capabilities reply (the whole datagram, header included). The radio count must
/// account for the length exactly.
pub fn parse(bytes: &[u8]) -> Result<Capabilities, WireError> {
    if bytes.len() < FIRST_ENTRY {
        return Err(WireError::Short { len: bytes.len() });
    }
    let count = usize::from(be16(bytes, OFF_COUNT));
    if bytes.len() != FIRST_ENTRY + count * ENTRY_LEN {
        return Err(WireError::Capabilities);
    }
    let radios = bytes[FIRST_ENTRY..]
        .chunks_exact(ENTRY_LEN)
        .map(parse_entry)
        .collect();
    Ok(Capabilities { radios })
}

fn parse_entry(r: &[u8]) -> RadioCap {
    let mut identity = [0u8; 16];
    identity.copy_from_slice(&r[CAP_OFF_IDENTITY..CAP_OFF_IDENTITY + 16]);
    let commoncap = le16(r, CAP_OFF_COMMONCAP);
    let mac = (commoncap == COMMONCAP_MAC).then(|| {
        let mut mac = [0u8; 6];
        mac.copy_from_slice(&r[CAP_OFF_MAC..CAP_OFF_MAC + 6]);
        mac
    });
    // The link type and the rate bitmaps are little-endian 16 here; the rate a connection-info
    // request names is a big-endian 32 in hertz, a different encoding in a different packet.
    let link = match le16(r, CAP_OFF_CONNTYPE) {
        0x0707 => Link::Wifi,
        0x073f => Link::Ethernet,
        other => Link::Other(other),
    };
    RadioCap {
        identity,
        commoncap,
        mac,
        name: text_of(&r[CAP_OFF_NAME..CAP_OFF_NAME + 32]),
        audio: text_of(&r[CAP_OFF_AUDIO..CAP_OFF_AUDIO + 32]),
        link,
        civ_addr: r[CAP_OFF_CIV_ADDR],
        rx_rates: Rates(le16(r, CAP_OFF_RX_RATE)),
        tx_rates: Rates(le16(r, CAP_OFF_TX_RATE)),
        baud: be32(r, CAP_OFF_BAUD),
    }
}

/// Which advertised radio a session uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// By position in the advertised list.
    Index(usize),
    /// By name, without regard to ASCII case.
    Name(String),
}

/// Why no radio could be chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unselected {
    /// The reply advertised no radio at all.
    NoRadios,
    /// The index is past the end of the list.
    IndexOutOfRange { index: usize, advertised: usize },
    /// No advertised radio has the name.
    NoSuchName,
    /// An empty name selects nothing.
    EmptyName,
}

/// Picks a radio: by index when one is given, else by name. Returns its position in the list.
pub fn select(caps: &Capabilities, choice: &Choice) -> Result<usize, Unselected> {
    if caps.radios.is_empty() {
        return Err(Unselected::NoRadios);
    }
    match choice {
        Choice::Index(index) if *index < caps.radios.len() => Ok(*index),
        Choice::Index(index) => Err(Unselected::IndexOutOfRange {
            index: *index,
            advertised: caps.radios.len(),
        }),
        Choice::Name(name) if name.is_empty() => Err(Unselected::EmptyName),
        Choice::Name(name) => caps
            .radios
            .iter()
            .position(|r| r.name.eq_ignore_ascii_case(name))
            .ok_or(Unselected::NoSuchName),
    }
}

/// The network Icoms, each with the name its built-in server is expected to report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Model {
    Ic7610,
    Ic9700,
    Ic705,
    Ic905,
    Ic7760,
    Ic7300Mk2,
}

impl Model {
    /// Every network model.
    pub const ALL: [Model; 6] = [
        Model::Ic7610,
        Model::Ic9700,
        Model::Ic705,
        Model::Ic905,
        Model::Ic7760,
        Model::Ic7300Mk2,
    ];

    /// The name the radio's server reports in its capabilities entry, as the upstream model
    /// descriptors record it.
    pub fn radio_name(self) -> &'static str {
        match self {
            Model::Ic7610 => "IC-7610",
            Model::Ic9700 => "IC-9700",
            Model::Ic705 => "IC-705",
            Model::Ic905 => "IC-905",
            Model::Ic7760 => "IC-7760",
            Model::Ic7300Mk2 => "IC-7300MK2",
        }
    }
}

#[cfg(test)]
mod tests {
    //! Translated from upstream's `test/test_icom_network_proto.c`: its capabilities,
    //! radio-selection and rate cases, each named after the case it translates.
    use super::*;
    use crate::icom::wire::{put_be16, put_be32, put_le16, put_le32, HEADER_LEN};

    /// One entry with the fields a session uses (upstream's `build_radio_entry`).
    fn entry(name: &str, civ: u8, rx: u16, tx: u16) -> Vec<u8> {
        let mut r = vec![0u8; ENTRY_LEN];
        r[0x10..0x10 + name.len()].copy_from_slice(name.as_bytes());
        r[0x30..0x30 + 11].copy_from_slice(b"ICOM_VAUDIO");
        put_le16(&mut r, 0x50, 0x073f); // Ethernet
        r[0x52] = civ;
        put_le16(&mut r, 0x53, rx);
        put_le16(&mut r, 0x55, tx);
        put_be32(&mut r, 0x5a, 19200); // the CI-V baud rate
        r
    }

    /// A reply carrying `entries`, claiming `count` radios (upstream's `build_capabilities`).
    fn reply(count: u16, entries: &[Vec<u8>]) -> Vec<u8> {
        let len = FIRST_ENTRY + entries.len() * ENTRY_LEN;
        let mut b = vec![0u8; len];
        put_le32(&mut b, 0, len as u32);
        put_be16(&mut b, 0x40, count);
        for (i, e) in entries.iter().enumerate() {
            let at = FIRST_ENTRY + i * ENTRY_LEN;
            b[at..at + ENTRY_LEN].copy_from_slice(e);
        }
        b
    }

    // upstream: test_capabilities_parse
    #[test]
    fn capabilities_parse() {
        let c = parse(&reply(1, &[entry("IC-7610", 0x98, 0x8b01, 0x8b01)])).unwrap();
        assert_eq!(c.radios.len(), 1);
        let r = &c.radios[0];
        assert_eq!(
            (r.name.as_str(), r.audio.as_str()),
            ("IC-7610", "ICOM_VAUDIO")
        );
        assert_eq!((r.link, r.civ_addr), (Link::Ethernet, 0x98));
        assert_eq!((r.rx_rates, r.tx_rates), (Rates(0x8b01), Rates(0x8b01)));
        assert_eq!(r.baud, 19200);
        assert_eq!(r.mac, None);
    }

    // upstream: test_capabilities_parse_padded_name
    #[test]
    fn capabilities_parse_padded_name() {
        let mut e = entry("IC-7610", 0x98, 0x8b01, 0);
        e[0x10 + 7..0x10 + 15].fill(b' ');
        let c = parse(&reply(1, &[e])).unwrap();
        assert_eq!(c.radios[0].name, "IC-7610");
    }

    // upstream: test_capabilities_parse_mac_mode
    #[test]
    fn capabilities_parse_mac_mode() {
        // commoncap 0x8010 marks the identity block as holding a MAC. The whole block is still
        // kept verbatim, because it is echoed back on connect. (A made-up address.)
        let mac = [0x02, 0x00, 0x5e, 0x10, 0x20, 0x30];
        let mut e = entry("IC-7610", 0x98, 0x8b01, 0x8b01);
        put_le16(&mut e, 0x07, COMMONCAP_MAC);
        e[0x0a..0x10].copy_from_slice(&mac);
        let c = parse(&reply(1, &[e.clone()])).unwrap();
        let r = &c.radios[0];
        assert_eq!(r.commoncap, COMMONCAP_MAC);
        assert_eq!(r.mac, Some(mac));
        assert_eq!(r.identity[..], e[..16]);
    }

    // upstream: test_capabilities_parse_multiple
    #[test]
    fn capabilities_parse_multiple() {
        let c = parse(&reply(
            3,
            &[
                entry("IC-7610", 0x98, 0x8b01, 0x8b01),
                entry("IC-9700", 0xa2, 0x8b01, 0),
                entry("IC-705", 0xa4, 0x0800, 0x0800),
            ],
        ))
        .unwrap();
        assert_eq!(c.radios.len(), 3);
        assert_eq!(
            (c.radios[1].name.as_str(), c.radios[1].civ_addr),
            ("IC-9700", 0xa2)
        );
        assert_eq!(
            (c.radios[2].name.as_str(), c.radios[2].civ_addr),
            ("IC-705", 0xa4)
        );
    }

    // upstream: test_capabilities_parse_truncated
    #[test]
    fn capabilities_parse_truncated() {
        // Upstream keeps the entries that fit and reports fewer decoded than claimed. Here a
        // count that disagrees with the length is a malformed reply.
        let full = reply(
            2,
            &[
                entry("IC-7610", 0x98, 0x8b01, 0x8b01),
                entry("IC-9700", 0xa2, 0x8b01, 0),
            ],
        );
        let mut cut = full[..FIRST_ENTRY + ENTRY_LEN + 4].to_vec();
        let len = cut.len() as u32;
        put_le32(&mut cut, 0, len);
        assert_eq!(parse(&cut), Err(WireError::Capabilities));
        // one entry, claimed as two
        let one = reply(2, &[entry("IC-7610", 0x98, 0x8b01, 0x8b01)]);
        assert_eq!(parse(&one), Err(WireError::Capabilities));
        assert!(parse(&full).is_ok());
    }

    // upstream: test_capabilities_parse_invalid
    #[test]
    fn capabilities_parse_invalid() {
        assert_eq!(
            parse(&[0u8; HEADER_LEN]),
            Err(WireError::Short { len: HEADER_LEN })
        );
        // A count of 0 in a reply with room for one entry disagrees with its length (upstream
        // reads it as well-formed and empty).
        assert_eq!(
            parse(&reply(0, &[vec![0u8; ENTRY_LEN]])),
            Err(WireError::Capabilities)
        );
        // a count of 0 with no entries is well-formed, just empty
        let c = parse(&reply(0, &[])).unwrap();
        assert!(c.radios.is_empty());
        assert_eq!(select(&c, &Choice::Index(0)), Err(Unselected::NoRadios));
    }

    // upstream: test_select_radio
    #[test]
    fn select_radio() {
        let c = parse(&reply(
            2,
            &[
                entry("IC-7610", 0x98, 0x8b01, 0x8b01),
                entry("IC-9700", 0xa2, 0x8b01, 0),
            ],
        ))
        .unwrap();
        // an index selects by position
        assert_eq!(select(&c, &Choice::Index(1)), Ok(1));
        assert_eq!(select(&c, &Choice::Index(0)), Ok(0));
        // a name matches without regard to case
        assert_eq!(select(&c, &Choice::Name("IC-9700".into())), Ok(1));
        assert_eq!(select(&c, &Choice::Name("ic-9700".into())), Ok(1));
        // out of range, an unknown name and an empty name all fail
        assert_eq!(
            select(&c, &Choice::Index(2)),
            Err(Unselected::IndexOutOfRange {
                index: 2,
                advertised: 2
            })
        );
        assert_eq!(
            select(&c, &Choice::Name("IC-705".into())),
            Err(Unselected::NoSuchName)
        );
        assert_eq!(
            select(&c, &Choice::Name(String::new())),
            Err(Unselected::EmptyName)
        );
        let empty = Capabilities::default();
        assert_eq!(
            select(&empty, &Choice::Name("IC-7610".into())),
            Err(Unselected::NoRadios)
        );
    }

    // upstream: test_rate_bitmap
    #[test]
    fn rate_bitmap() {
        // 0x8b01 is what an IC-7610 advertises: 48, 24, 16, 12 and 8 kHz and nothing else.
        let r = Rates(0x8b01);
        for hz in [48000, 24000, 16000, 12000, 8000] {
            assert!(r.supports(hz), "{hz}");
        }
        assert!(!r.supports(44100));
        assert!(!r.supports(32000));
        // a rate the protocol has no flag for is never supported
        assert!(!Rates(0xffff).supports(96000));
        assert!(!Rates(0).supports(48000));
        assert_eq!(r.to_string(), "48000, 24000, 16000, 12000, 8000");
        assert_eq!(Rates(RATE_48000).to_string(), "48000");
        assert_eq!(Rates(0).to_string(), "none");
    }

    // upstream: test_rate_list_truncates
    #[test]
    fn rate_list_truncates() {
        // Upstream guards a C buffer so the list never ends in a partial number that reads as a
        // real rate. A built list cannot be cut short; what carries over is the property: for
        // every bitmap, the text names exactly the advertised rates, each one whole.
        for bits in 0..=u16::MAX {
            let r = Rates(bits);
            let text = r.to_string();
            if r.list().is_empty() {
                assert_eq!(text, "none");
                continue;
            }
            let read: Vec<u32> = text.split(", ").map(|n| n.parse().unwrap()).collect();
            assert_eq!(read, r.list(), "{bits:#06x}");
            assert!(read.iter().all(|hz| r.supports(*hz)));
        }
    }

    #[test]
    fn every_model_has_its_servers_name() {
        let names: Vec<&str> = Model::ALL.iter().map(|m| m.radio_name()).collect();
        assert_eq!(
            names,
            [
                "IC-7610",
                "IC-9700",
                "IC-705",
                "IC-905",
                "IC-7760",
                "IC-7300MK2"
            ]
        );
    }
}
