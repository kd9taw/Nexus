//! Packet dispatch — the top-level "received AX.25 frame ⇄ structured APRS" API that ties the
//! [`frame`](super::frame), [`parser`](super::parser), and [`mice`](super::mice) layers together.
//!
//! On RX: a [`Frame`] whose info field is Mic-E (by its DTI) is decoded with the destination TOCALL
//! (Mic-E hides latitude there); everything else goes through the info-field [`parser`]. On TX: a
//! position beacon is built into a ready-to-key [`Frame`].

use super::frame::{Address, Frame};
use super::mice::{self, MicE};
use super::parser::{self, AprsInfo, Message, Position};

/// The decoded payload of an APRS packet.
#[derive(Debug, Clone, PartialEq)]
pub enum AprsBody {
    /// An info-field packet (position / message / status / unrecognized).
    Info(AprsInfo),
    /// A Mic-E report (decoded from destination + info).
    MicE(MicE),
}

/// A fully decoded APRS packet: who sent it, the path, and the payload.
#[derive(Debug, Clone, PartialEq)]
pub struct AprsPacket {
    pub source: Address,
    /// Raw AX.25 destination (a real TOCALL, or the Mic-E-encoded latitude field).
    pub dest: Address,
    pub path: Vec<Address>,
    pub body: AprsBody,
}

impl AprsPacket {
    /// Decode a received AX.25 UI frame into a structured APRS packet (never fails — an
    /// unrecognized info field becomes [`AprsInfo::Other`]).
    pub fn from_frame(frame: &Frame) -> AprsPacket {
        // Third-party traffic ('}') wraps a whole inner packet in TNC2 text (e.g. an I-gated or
        // digipeated station) — decode the REAL originator, not the '}' wrapper.
        if frame.info.first() == Some(&b'}') {
            if let Some(inner) = parse_third_party(&String::from_utf8_lossy(&frame.info[1..])) {
                return inner;
            }
        }
        let body = if mice::is_mic_e(&frame.info) {
            match mice::decode(&frame.dest.call, &frame.info) {
                Some(m) => AprsBody::MicE(m),
                None => AprsBody::Info(parser::parse(&frame.info)),
            }
        } else {
            AprsBody::Info(parser::parse(&frame.info))
        };
        AprsPacket {
            source: frame.source.clone(),
            dest: frame.dest.clone(),
            path: frame.path.clone(),
            body,
        }
    }

    /// Decode straight from de-stuffed frame bytes (addresses…info + FCS, as [`deframe`] yields).
    /// `None` if the bytes aren't a valid, FCS-checked AX.25 UI frame.
    ///
    /// [`deframe`]: super::hdlc::deframe
    pub fn from_bytes(bytes: &[u8]) -> Option<AprsPacket> {
        Frame::decode(bytes).as_ref().map(AprsPacket::from_frame)
    }

    /// The reported position (lat, lon) in degrees, from either a position report or a Mic-E — for
    /// mapping. `None` for message/status/unrecognized packets.
    pub fn position(&self) -> Option<(f64, f64)> {
        match &self.body {
            AprsBody::Info(AprsInfo::Position(p)) => Some((p.lat, p.lon)),
            AprsBody::Info(AprsInfo::Object { position, .. }) => Some((position.lat, position.lon)),
            AprsBody::MicE(m) => Some((m.lat, m.lon)),
            _ => None,
        }
    }
}

/// Parse a third-party payload — a whole inner packet in TNC2 text: `SRC>DEST,digi1,digi2,…:info`.
/// The wrapped station's source/dest/path/info replace the wrapper's. `None` if it isn't well-formed.
fn parse_third_party(text: &str) -> Option<AprsPacket> {
    let (header, info) = text.split_once(':')?;
    let (src, dest_path) = header.split_once('>')?;
    let mut parts = dest_path.split(',');
    let dest = Address::parse(parts.next()?)?;
    let source = Address::parse(src)?;
    let path: Vec<Address> = parts.filter_map(Address::parse).collect();
    let info_bytes = info.as_bytes();
    let body = if mice::is_mic_e(info_bytes) {
        match mice::decode(&dest.call, info_bytes) {
            Some(m) => AprsBody::MicE(m),
            None => AprsBody::Info(parser::parse(info_bytes)),
        }
    } else {
        AprsBody::Info(parser::parse(info_bytes))
    };
    Some(AprsPacket { source, dest, path, body })
}

/// The Nexus experimental APRS TOCALL (destination) for beacons we originate. `APZxxx` is the
/// registered prefix for experimental/homebrew software.
pub const NEXUS_TOCALL: &str = "APZNEX";

/// Build a ready-to-key position-beacon [`Frame`] from the operator's callsign and a position.
/// `path` is the digipeater path (e.g. `["WIDE1-1", "WIDE2-1"]`); `comment` is free text.
pub fn position_beacon(
    mycall: Address,
    lat: f64,
    lon: f64,
    symbol_table: char,
    symbol_code: char,
    comment: &str,
    path: Vec<Address>,
) -> Frame {
    let info = AprsInfo::Position(Position {
        lat,
        lon,
        symbol_table,
        symbol_code,
        timestamp: None,
        messaging: true, // advertise that we accept APRS messages (Nexus can receive + ack them)
        comment: comment.to_string(),
    })
    .encode();
    Frame::ui(Address::new(NEXUS_TOCALL, 0), mycall, path, &info)
}

/// Build a ready-to-key APRS text-message [`Frame`]: `:ADDRESSEE:text{id`. `id` (≤5 chars) is the
/// message line number for acking — empty = no id/no ack expected. An ACK is just a message whose
/// text is `ack<their-id>` addressed back to the sender.
pub fn message_frame(
    mycall: Address,
    addressee: &str,
    text: &str,
    id: &str,
    path: Vec<Address>,
) -> Frame {
    let info = AprsInfo::Message(Message {
        addressee: addressee.trim().to_ascii_uppercase(),
        text: text.to_string(),
        id: (!id.is_empty()).then(|| id.to_string()),
    })
    .encode();
    Frame::ui(Address::new(NEXUS_TOCALL, 0), mycall, path, &info)
}

#[cfg(test)]
mod tests {
    use super::super::parser::AprsInfo;
    use super::*;

    #[test]
    fn decodes_a_position_frame() {
        let frame = Frame::ui(
            Address::new("APRS", 0),
            Address::new("N0CALL", 9),
            vec![Address::new("WIDE1", 1)],
            b"!4903.50N/07201.75W-Home",
        );
        let pkt = AprsPacket::from_frame(&frame);
        assert_eq!(pkt.source.call, "N0CALL");
        assert_eq!(pkt.path.len(), 1);
        match &pkt.body {
            AprsBody::Info(AprsInfo::Position(p)) => {
                assert!((p.lat - 49.0583333).abs() < 1e-6);
                assert_eq!(p.comment, "Home");
            }
            other => panic!("expected position, got {other:?}"),
        }
        let (lat, lon) = pkt.position().unwrap();
        assert!((lat - 49.0583333).abs() < 1e-6 && lon < 0.0);
    }

    #[test]
    fn decodes_a_mic_e_frame_using_the_destination() {
        // The hand-worked Mic-E vector from mice.rs, wrapped in a real frame.
        let frame = Frame::ui(
            Address::new("SSRUVT", 0),
            Address::new("N0CALL", 7),
            vec![],
            &[0x60, b'(', b'#', b'H', 0x1e, 0x1e, b'O', b'>', b'/'],
        );
        let pkt = AprsPacket::from_frame(&frame);
        match &pkt.body {
            AprsBody::MicE(m) => {
                assert_eq!(m.speed_knots, 20);
                assert_eq!(m.course_deg, 251);
            }
            other => panic!("expected Mic-E, got {other:?}"),
        }
        let (lat, lon) = pkt.position().unwrap();
        assert!((lat - 33.427333).abs() < 1e-5);
        assert!((lon - (-112.124)).abs() < 1e-3);
    }

    #[test]
    fn unwraps_a_third_party_packet_to_the_real_originator() {
        // A gateway relays N0CALL's position wrapped in a '}' third-party frame.
        let frame = Frame::ui(
            Address::new("IGATE", 0),
            Address::new("APRS", 0),
            vec![],
            b"}N0CALL>APRS,TCPIP*:!4903.50N/07201.75W-relayed",
        );
        let pkt = AprsPacket::from_frame(&frame);
        assert_eq!(pkt.source.call, "N0CALL"); // the REAL station, not the IGATE wrapper
        let (lat, lon) = pkt.position().unwrap();
        assert!((lat - 49.0583333).abs() < 1e-6);
        assert!((lon - (-72.029166)).abs() < 1e-5);
    }

    #[test]
    fn a_message_packet_has_no_position() {
        let frame = Frame::ui(
            Address::new("APRS", 0),
            Address::new("N0CALL", 0),
            vec![],
            b":WIDE2-1  :hi",
        );
        assert!(AprsPacket::from_frame(&frame).position().is_none());
    }

    #[test]
    fn message_frame_round_trips_through_a_frame() {
        let frame = message_frame(
            Address::new("N0CALL", 0),
            "kd9taw",
            "hi from Nexus",
            "007",
            vec![Address::new("WIDE1", 1)],
        );
        let bytes = frame.encode();
        let pkt = AprsPacket::from_bytes(&bytes).expect("message is a valid frame");
        assert_eq!(pkt.source.call, "N0CALL");
        assert_eq!(pkt.dest.call, NEXUS_TOCALL);
        assert!(pkt.position().is_none());
        match &pkt.body {
            AprsBody::Info(AprsInfo::Message(m)) => {
                assert_eq!(m.addressee, "KD9TAW"); // upper-cased
                assert_eq!(m.text, "hi from Nexus");
                assert_eq!(m.id.as_deref(), Some("007"));
            }
            other => panic!("expected message, got {other:?}"),
        }
    }

    #[test]
    fn position_beacon_round_trips_through_a_frame() {
        let beacon = position_beacon(
            Address::new("N0CALL", 9),
            49.0583333,
            -72.029166,
            '/',
            '-',
            "Nexus beacon",
            vec![Address::new("WIDE1", 1), Address::new("WIDE2", 1)],
        );
        let bytes = beacon.encode();
        let pkt = AprsPacket::from_bytes(&bytes).expect("beacon is a valid frame");
        assert_eq!(pkt.source.call, "N0CALL");
        assert_eq!(pkt.dest.call, NEXUS_TOCALL);
        let (lat, lon) = pkt.position().unwrap();
        assert!((lat - 49.0583333).abs() < 1e-4);
        assert!((lon - (-72.029166)).abs() < 1e-4);
        match pkt.body {
            AprsBody::Info(AprsInfo::Position(p)) => assert_eq!(p.comment, "Nexus beacon"),
            other => panic!("expected position, got {other:?}"),
        }
    }
}
