//! Packet dispatch — the top-level "received APRS ⇄ structured APRS" API that ties the
//! [`frame`](super::frame), [`parser`](super::parser), and [`mice`](super::mice) layers together.
//!
//! **Two inlets, one parser.** A packet reaches this layer either as an AX.25 [`Frame`] recovered
//! from the air by the HDLC deframer, or as a [`Tnc2`] text line off APRS-IS. Both converge on the
//! same [`AprsPacket`] — and therefore on the same info-field [`parser`], the same station store,
//! and the same map. Nothing below this module knows which way a packet arrived.
//!
//! On RX: a packet whose info field is Mic-E (by its DTI) is decoded with the destination TOCALL
//! (Mic-E hides latitude there); everything else goes through the info-field [`parser`]. On TX: a
//! position beacon is built into a ready-to-key [`Frame`].

use super::frame::{Address, Frame};
use super::mice::{self, MicE};
use super::parser::{self, AprsInfo, Message, Position};

/// A TNC2 monitor line split into its parts, borrowed from the source buffer:
/// `SRC>DEST,path1,path2:info`.
///
/// The path tokens are kept **RAW** — `WIDE2-1`, `WIDE1*`, `TCPIP*`, `qAR`, `T2OREGON` — because
/// the callers that matter cannot afford them cleaned up. [`Address::parse`] rejects a server name
/// longer than six characters and folds away the trailing `*`, so a path filtered through it is
/// useless for the iGate rules (`TCPIP`/`NOGATE` detection) and for locating a q-construct. The
/// structured decode in [`AprsPacket::from_tnc2`] parses what it can and drops the rest; anything
/// making a gating decision reads [`Tnc2::path`] instead.
///
/// `info` is a byte slice, not `&str`: real traffic carries non-UTF-8 in the information field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tnc2<'a> {
    pub source: &'a str,
    pub dest: &'a str,
    /// Digipeater / q-construct / server tokens between the destination and the `:`, verbatim.
    pub path: Vec<&'a str>,
    /// The APRS information field: everything after the FIRST `:`, raw.
    pub info: &'a [u8],
}

impl<'a> Tnc2<'a> {
    /// Split a TNC2 line. `None` if it is not `SRC>DEST[,path…]:info` — which is also how a
    /// non-packet line (an APRS-IS `#` server comment) is rejected.
    ///
    /// A trailing CR/LF is tolerated. The info field starts after the FIRST `:`, since a message
    /// packet's own payload contains further colons.
    pub fn split(line: &'a [u8]) -> Option<Tnc2<'a>> {
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let colon = line.iter().position(|&b| b == b':')?;
        let (header, rest) = line.split_at(colon);
        let info = &rest[1..];
        // The header is ASCII by construction; a non-ASCII one is a corrupt line, not a packet.
        let header = std::str::from_utf8(header).ok()?;
        let (source, dest_path) = header.split_once('>')?;
        let mut parts = dest_path.split(',');
        let dest = parts.next()?;
        if source.is_empty() || dest.is_empty() {
            return None;
        }
        Some(Tnc2 {
            source,
            dest,
            path: parts.collect(),
            info,
        })
    }

    /// The APRS data-type identifier — the info field's first byte. `None` for an empty info field.
    pub fn dti(&self) -> Option<u8> {
        self.info.first().copied()
    }
}

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
            if let Some(inner) = AprsPacket::from_tnc2(&frame.info[1..]) {
                return inner;
            }
        }
        AprsPacket {
            source: frame.source.clone(),
            dest: frame.dest.clone(),
            path: frame.path.clone(),
            body: decode_body(&frame.dest.call, &frame.info),
        }
    }

    /// Decode a TNC2 text line — `SRC>DEST,path:info` — into the same [`AprsPacket`] an off-air
    /// [`Frame`] produces. This is the APRS-IS inlet: the internet delivers text lines rather than
    /// audio, and from here on a packet's origin is invisible to the parser, the store and the map.
    ///
    /// `None` if the line is not well-formed TNC2 (an APRS-IS `#` comment, a truncated line).
    /// Path tokens the AX.25 address grammar rejects — server names over six characters, `qAI`
    /// trace chains — are dropped from [`AprsPacket::path`]; use [`Tnc2::split`] directly when the
    /// raw tokens matter (they do for every iGate gating rule).
    ///
    /// Nested third-party wrappers are unwrapped up to [`MAX_THIRD_PARTY_DEPTH`].
    pub fn from_tnc2(line: &[u8]) -> Option<AprsPacket> {
        Self::from_tnc2_depth(line, MAX_THIRD_PARTY_DEPTH)
    }

    fn from_tnc2_depth(line: &[u8], depth: u8) -> Option<AprsPacket> {
        let t = Tnc2::split(line)?;
        // A wrapped inner packet replaces the wrapper, exactly as `from_frame` does off-air.
        // Depth-capped: third-party nesting is legal and unbounded on APRS-IS, so a looping or
        // hostile packet must not recurse without a floor.
        if t.info.first() == Some(&b'}') && depth > 0 {
            if let Some(inner) = Self::from_tnc2_depth(&t.info[1..], depth - 1) {
                return Some(inner);
            }
        }
        let dest = lenient_addr(t.dest)?;
        let source = lenient_addr(t.source)?;
        let body = decode_body(&dest.call, t.info);
        Some(AprsPacket {
            source,
            dest,
            path: t.path.iter().filter_map(|p| lenient_addr(p)).collect(),
            body,
        })
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

/// How deep [`AprsPacket::from_tnc2`] will follow nested third-party (`}`) wrappers.
const MAX_THIRD_PARTY_DEPTH: u8 = 4;

/// Parse a TNC2 address token, falling back to a DISPLAY-ONLY address when the text is not a legal
/// AX.25 callsign.
///
/// APRS-IS routinely carries station identifiers AX.25 cannot represent: `EL-IW4ENE` (an EchoLink
/// gateway — `IW4ENE` is not a number, so the SSID parse fails), `2E0TKY-S`, server names longer
/// than six characters. [`Address::parse`] rejects every one of them, and requiring it on this
/// inlet would silently drop a large and entirely legitimate slice of internet traffic — the exact
/// failure mode of an empty map. So a token the grammar rejects is kept verbatim instead.
///
/// ⚠️ The result may violate the AX.25 address grammar and **must never be encoded onto the air**.
/// It cannot reach a transmitter by any path that exists: every TX frame is built from operator
/// settings, and the iGate uplink re-sends only bytes that already arrived as a valid off-air
/// [`Frame`]. `None` only for an empty token.
fn lenient_addr(text: &str) -> Option<Address> {
    if let Some(a) = Address::parse(text) {
        return Some(a);
    }
    let t = text.trim();
    let (t, cbit) = match t.strip_suffix('*') {
        Some(rest) => (rest, true),
        None => (t, false),
    };
    if t.is_empty() {
        return None;
    }
    let mut a = Address::new(t, 0);
    a.cbit = cbit;
    Some(a)
}

/// Decode an information field into a body, routing Mic-E (which hides latitude in the
/// destination TOCALL) to its own decoder. The single point both inlets share.
fn decode_body(dest_call: &str, info: &[u8]) -> AprsBody {
    if mice::is_mic_e(info) {
        if let Some(m) = mice::decode(dest_call, info) {
            return AprsBody::MicE(m);
        }
    }
    AprsBody::Info(parser::parse(info))
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

    // ---- The APRS-IS inlet. Every line below is a REAL captured APRS-IS packet (javAPRSlib's
    // aprs.txt / cwop.txt / w5zfq-mice.txt capture, the Ham::APRS::FAP test vectors, and aprsc's
    // own regression fixtures) — not a hand-written approximation of one.

    #[test]
    fn tnc2_split_keeps_path_tokens_raw() {
        // A path an AX.25 address parser mangles: a used-digi `*`, the q-construct, and a SERVER
        // name too long to be a callsign. All three must survive verbatim — the iGate rules and
        // the q-construct lookup read these, and `Address::parse` would eat all three.
        let t = Tnc2::split(b"WB8HRV>APRS,TCPXX*,qAX,CWOP-4:@291813z3913.47N/08424.67W_220/004")
            .expect("well-formed TNC2");
        assert_eq!(t.source, "WB8HRV");
        assert_eq!(t.dest, "APRS");
        assert_eq!(t.path, vec!["TCPXX*", "qAX", "CWOP-4"]);
        assert_eq!(t.dti(), Some(b'@'));
        // ...and the structured decode drops what it cannot parse, which is exactly why the raw
        // tokens have to be available separately.
        let pkt = AprsPacket::from_tnc2(
            b"WB8HRV>APRS,TCPXX*,qAX,CWOP-4:@291813z3913.47N/08424.67W_220/004",
        )
        .unwrap();
        assert_eq!(pkt.path.len(), 3);
    }

    #[test]
    fn tnc2_split_takes_the_first_colon_so_a_message_keeps_its_own() {
        // ":N3HEV-9  :ack26" has two colons of its own; the info field starts at the FIRST one.
        let t = Tnc2::split(b"KG4LAA>APWW08,TCPIP*,qAS,n3wax::N3HEV-9  :ack26").unwrap();
        assert_eq!(t.dest, "APWW08");
        assert_eq!(t.info, b":N3HEV-9  :ack26");
    }

    #[test]
    fn tnc2_split_rejects_server_comment_lines() {
        // APRS-IS banners, login responses and 20 s keepalives all arrive on the same socket.
        for line in [
            &b"# aprsc 2.1.5-g8af3cdc"[..],
            b"# logresp ab0oo verified, server NINTH",
            b"# javAPRSSrvr 3.15b07",
        ] {
            assert!(Tnc2::split(line).is_none(), "must not parse: {line:?}");
        }
    }

    #[test]
    fn tnc2_decodes_the_same_position_the_frame_inlet_does() {
        // One packet, both inlets, one result: the whole point of the shared parser.
        let pkt = AprsPacket::from_tnc2(
            b"K7PIA-1>APDW14,WIDE2-2,qAR,VCAPK:!4710.42N112207.66W#PHG7160Buckley, WA Local Digi/I-Gate",
        )
        .expect("real captured APRS-IS line");
        assert_eq!(pkt.source.call, "K7PIA");
        assert_eq!(pkt.source.ssid, 1);
        let (lat, lon) = pkt.position().expect("a position report");
        assert!((lat - 47.1737).abs() < 1e-3, "lat {lat}");
        assert!((lon - (-122.1277)).abs() < 1e-3, "lon {lon}");

        let frame = Frame::ui(
            Address::new("APDW14", 0),
            Address::new("K7PIA", 1),
            vec![Address::parse("WIDE2-2").unwrap()],
            b"!4710.42N112207.66W#PHG7160Buckley, WA Local Digi/I-Gate",
        );
        let off_air = AprsPacket::from_frame(&frame);
        assert_eq!(off_air.body, pkt.body, "both inlets must agree exactly");
    }

    #[test]
    fn tnc2_decodes_a_compressed_position() {
        let pkt = AprsPacket::from_tnc2(
            b"DG5MGJ-10>APLS01,WIDE1-1,qAR,DL0IN-15:!/5m&/QD8Opb(H/A=001244Lora Tracker Rudi",
        )
        .unwrap();
        let (lat, lon) = pkt.position().expect("compressed position");
        assert!(
            (48.0..53.0).contains(&lat),
            "lat {lat} should be in Germany"
        );
        assert!((5.0..15.0).contains(&lon), "lon {lon} should be in Germany");
    }

    #[test]
    fn tnc2_decodes_mic_e_using_the_destination_field() {
        // Ham::APRS::FAP 23decode-mice.t asserts lat 41.7877 / lon -71.4202 for this exact line.
        let pkt =
            AprsPacket::from_tnc2(b"OH7LZB-2>TQ4W2V,WIDE2-1,qAo,OH7LZB:`c51!f?>/]\"3x}=").unwrap();
        match &pkt.body {
            AprsBody::MicE(m) => {
                assert!((m.lat - 41.7877).abs() < 1e-3, "lat {}", m.lat);
                assert!((m.lon - (-71.4202)).abs() < 1e-3, "lon {}", m.lon);
            }
            other => panic!("expected Mic-E, got {other:?}"),
        }
    }

    #[test]
    fn tnc2_info_field_survives_non_utf8_bytes() {
        // FAP 23decode-mice.t carries a raw 0x1C in the info field. A lossy String conversion
        // anywhere on this path turns it into U+FFFD and destroys the packet — for display AND,
        // far worse, for anything re-uploaded to APRS-IS.
        let mut line = b"OH7LZB-13>SX15S6,TCPIP*,qAC,FOURTH:'I',l ".to_vec();
        line.push(0x1C);
        line.extend_from_slice(b">/]");
        let t = Tnc2::split(&line).unwrap();
        assert!(
            t.info.contains(&0x1C),
            "the raw byte must survive the split"
        );
        assert!(AprsPacket::from_tnc2(&line).is_some());
    }

    #[test]
    fn tnc2_decodes_a_weather_object_and_a_message() {
        let wx = AprsPacket::from_tnc2(
            b"SR9WXL>AKLPRZ,WIDE2-1,qAR,SR9NSK:!4947.70N/01926.80E_310/000g002t056r...p...P...b09174h75",
        )
        .unwrap();
        assert!(wx.position().is_some(), "a weather position still maps");

        let obj = AprsPacket::from_tnc2(
            b"WX9WL-15>APN382,qAO,KB9MTD-2:;146.970LE*111111z4152.36N/08932.16WrT082 R45M",
        )
        .unwrap();
        match &obj.body {
            AprsBody::Info(AprsInfo::Object { name, killed, .. }) => {
                assert_eq!(name, "146.970LE");
                assert!(!killed);
            }
            other => panic!("expected an object, got {other:?}"),
        }

        // `EL-IW4ENE` is a real APRS-IS source that AX.25 cannot represent (the SSID is not a
        // number). It must still decode — dropping it would erase every EchoLink/DMR gateway.
        let msg =
            AprsPacket::from_tnc2(b"EL-IW4ENE>RXTLM-1,TCPIP,qAR,IW4ENE::N3HEV-9  :ack26").unwrap();
        assert_eq!(msg.source.call, "EL-IW4ENE", "kept verbatim for display");
        match &msg.body {
            AprsBody::Info(AprsInfo::Message(m)) => {
                assert_eq!(m.addressee, "N3HEV-9");
                assert_eq!(m.text, "ack26");
            }
            other => panic!("expected a message, got {other:?}"),
        }
    }

    // ---- Symbols. The map draws these, so a symbol silently lost between the wire and the DTO
    // is a station wearing the wrong icon — which looks exactly as confident as the right one.

    #[test]
    fn an_uncompressed_position_keeps_its_symbol_table_and_code() {
        match AprsPacket::from_tnc2(
            b"K7PIA-1>APDW14,WIDE2-2,qAR,VCAPK:!4710.42N112207.66W#PHG7160Buckley digi",
        )
        .unwrap()
        .body
        {
            AprsBody::Info(AprsInfo::Position(p)) => {
                assert_eq!(p.symbol_table, '1', "an OVERLAY, not the primary table");
                assert_eq!(p.symbol_code, '#', "digipeater");
            }
            other => panic!("expected a position, got {other:?}"),
        }
    }

    #[test]
    fn an_overlay_character_survives_as_the_table_identifier() {
        // Real captured overlaid stations: an iGate marked `G`, and the `R&` receive-only-gateway
        // convention. The overlay rides in the TABLE slot, so a parser that normalised it to '\\'
        // would erase the operator's own annotation.
        for (line, table, code) in [
            (
                &b"YO2CK-10>APMI06,TCPIP*,qAC,FIFTH:@191803z4536.63NG02257.00E#PHG3430/iGate"[..],
                'G',
                '#',
            ),
            (b"XE2N-10>APDW16,qAR,XE2N-10:!2537.90NR10014.20W&", 'R', '&'),
            (
                b"S59DGO-5>APMI01,IR3UEZ-11,WIDE1*,qAR,IR4BA:@191802z4535.30NT01426.84E&PHG2830",
                'T',
                '&',
            ),
        ] {
            match AprsPacket::from_tnc2(line).unwrap().body {
                AprsBody::Info(AprsInfo::Position(p)) => {
                    assert_eq!(p.symbol_table, table, "{}", String::from_utf8_lossy(line));
                    assert_eq!(p.symbol_code, code, "{}", String::from_utf8_lossy(line));
                }
                other => panic!("expected a position, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_compressed_position_keeps_its_symbol_too() {
        // Compressed layout: table, 4-byte lat, 4-byte lon, THEN the code. Off-by-one here quietly
        // hands the renderer a character out of the base-91 coordinate.
        match AprsPacket::from_tnc2(
            b"SP2ST-4>APLS01,WIDE1-1,qAR,SP2ST-10:!\\3YK%S##VUY2HLoRa APRS 433.775MHz",
        )
        .unwrap()
        .body
        {
            AprsBody::Info(AprsInfo::Position(p)) => {
                assert_eq!(p.symbol_table, '\\');
                assert_eq!(p.symbol_code, 'U');
            }
            other => panic!("expected a compressed position, got {other:?}"),
        }
    }

    #[test]
    fn mic_e_reads_its_symbol_from_the_end_of_the_info_field() {
        // Mic-E puts the CODE before the TABLE — the reverse of every other format. The FAP
        // vector for this line decodes to a car on the primary table.
        match AprsPacket::from_tnc2(b"OH7LZB-2>TQ4W2V,WIDE2-1,qAo,OH7LZB:`c51!f?>/]\"3x}=")
            .unwrap()
            .body
        {
            AprsBody::MicE(m) => {
                assert_eq!(m.symbol_code, '>', "car");
                assert_eq!(m.symbol_table, '/', "primary table");
            }
            other => panic!("expected Mic-E, got {other:?}"),
        }
    }

    #[test]
    fn an_object_carries_the_symbol_of_the_thing_it_marks() {
        match AprsPacket::from_tnc2(
            b"WX9WL-15>APN382,qAO,KB9MTD-2:;146.970LE*111111z4152.36N/08932.16WrT082 R45M",
        )
        .unwrap()
        .body
        {
            AprsBody::Info(AprsInfo::Object { position, .. }) => {
                assert_eq!(position.symbol_table, '/');
                assert_eq!(position.symbol_code, 'r', "repeater");
            }
            other => panic!("expected an object, got {other:?}"),
        }
    }

    #[test]
    fn tnc2_unwraps_nested_third_party_to_the_innermost_originator() {
        // aprsc's own 13thirdparty.t acceptance case: three levels of `}` wrapping.
        let pkt = AprsPacket::from_tnc2(
            b"UU1AA>TEST,qAR,IGATE:}OF7LZB>DST,NET,GATE:}OF7LZC>DST,NET2,GATE2:!6013.69NR02450.97E&",
        )
        .unwrap();
        assert_eq!(
            pkt.source.call, "OF7LZC",
            "the innermost station, not a wrapper"
        );
        assert!(pkt.position().is_some());
    }

    #[test]
    fn tnc2_keeps_a_malformed_third_party_body_as_an_ordinary_packet() {
        // aprsc passes `}blah blah` through: no inner '>' and no inner ':' means it is not a
        // third-party frame at all, just an odd payload. It must not vanish.
        let pkt = AprsPacket::from_tnc2(b"K3SRC>APWW08,TCPIP*,qAR,IGA:}blah blah").unwrap();
        assert_eq!(pkt.source.call, "K3SRC");
        assert!(matches!(pkt.body, AprsBody::Info(AprsInfo::Other { .. })));
    }

    #[test]
    fn tnc2_rejects_lines_that_are_not_packets() {
        for line in [
            &b""[..],
            b"no separators at all",
            b"SRC>DEST no colon",
            b">DEST,PATH:info", // empty source
            b"SRC>:info",       // empty destination
        ] {
            assert!(
                AprsPacket::from_tnc2(line).is_none(),
                "must not parse: {}",
                String::from_utf8_lossy(line)
            );
        }
    }

    #[test]
    fn frame_round_trips_through_tnc2_text() {
        // The uplink path: an off-air frame rendered to the exact line APRS-IS will receive.
        let frame = Frame::ui(
            Address::new("APDW14", 0),
            Address::new("K7PIA", 1),
            vec![
                Address::parse("W9XYZ-3*").unwrap(),
                Address::parse("WIDE2-1").unwrap(),
            ],
            b"!4710.42N112207.66W#PHG7160",
        );
        assert_eq!(
            frame.to_tnc2(),
            b"K7PIA-1>APDW14,W9XYZ-3*,WIDE2-1:!4710.42N112207.66W#PHG7160".to_vec()
        );
        let back = AprsPacket::from_tnc2(&frame.to_tnc2()).unwrap();
        assert_eq!(back.body, AprsPacket::from_frame(&frame).body);
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
