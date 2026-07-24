//! AX.25 UI frames + the HDLC frame-check sequence.
//!
//! An APRS packet rides in an AX.25 **UI** (Unnumbered Information) frame. Unstuffed layout
//! (HDLC flags + bit-stuffing are the next layer up):
//!
//! ```text
//!  ┌──────────┬────────────┬───────────────┬─────────┬─────┬────────┬─────────┐
//!  │ dest 7 B │ source 7 B │ digis 0..8 ×7 │ ctrl 1B │ PID │ info N │ FCS 2 B │
//!  └──────────┴────────────┴───────────────┴─────────┴─────┴────────┴─────────┘
//! ```
//!
//! Each address is a callsign (≤6 chars, space-padded) with every byte shifted left one bit,
//! followed by an SSID octet; the low bit of the LAST address's SSID octet is the end-of-list
//! (extension) marker. Control is `0x03` (UI), PID `0xF0` (no layer 3). The FCS is CRC-16/X.25
//! over everything before it, stored low byte first.

/// The HDLC/X.25 frame-check sequence — CRC-16/X.25: poly 0x1021 reflected (`0x8408`),
/// init `0xFFFF`, reflected in/out, `xorout 0xFFFF`. Canonical check: `fcs(b"123456789") == 0x906E`.
pub fn fcs(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= byte as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0x8408 } else { crc >> 1 };
        }
    }
    !crc
}

/// Control field of a UI frame.
pub const CONTROL_UI: u8 = 0x03;
/// PID: no layer-3 protocol (APRS carries its payload directly in the info field).
pub const PID_NO_L3: u8 = 0xF0;
/// Max digipeaters in the path (AX.25 caps the address field at dest + source + 8).
pub const MAX_DIGIS: usize = 8;

/// One AX.25 address: a callsign (≤6 chars) + SSID (0..=15), plus the command / has-been-repeated
/// bit (the high bit of the SSID octet — C-bit on dest/source, H-bit on a digipeater).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    /// Uppercased callsign, ≤6 chars, no `-SSID` suffix.
    pub call: String,
    /// Secondary station ID, 0..=15.
    pub ssid: u8,
    /// SSID-octet high bit: command (dest/source) or "has been repeated" (digi).
    pub cbit: bool,
}

impl Address {
    /// A plain address (C-bit clear). `call` is trimmed + uppercased; `ssid` is masked to 0..=15.
    pub fn new(call: &str, ssid: u8) -> Self {
        Address { call: call.trim().to_ascii_uppercase(), ssid: ssid & 0x0F, cbit: false }
    }

    /// Parse `"N0CALL-9"` / `"APRS"` / `"WIDE1-1*"` (a trailing `*` marks a used digi → C-bit set).
    /// `None` on an empty/over-long call, non-alphanumeric call, or SSID > 15.
    pub fn parse(s: &str) -> Option<Address> {
        let s = s.trim();
        let (s, used) = match s.strip_suffix('*') {
            Some(rest) => (rest, true),
            None => (s, false),
        };
        if s.is_empty() {
            return None;
        }
        let (call, ssid) = match s.split_once('-') {
            Some((c, n)) => (c, n.parse::<u8>().ok()?),
            None => (s, 0),
        };
        if call.is_empty() || call.len() > 6 || ssid > 15 || !call.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return None;
        }
        let mut a = Address::new(call, ssid);
        a.cbit = used;
        Some(a)
    }

    /// Encode to the 7-byte on-air field. `last` sets the address-extension (end-of-list) bit.
    fn encode(&self, last: bool) -> [u8; 7] {
        let mut out = [0u8; 7];
        let call = self.call.as_bytes();
        for (i, slot) in out[..6].iter_mut().enumerate() {
            let c = if i < call.len() { call[i] } else { b' ' };
            *slot = c << 1;
        }
        // SSID octet: C/H | reserved(11) | ssid(4) | extension(1).
        out[6] = (if self.cbit { 0x80 } else { 0 }) | 0x60 | ((self.ssid & 0x0F) << 1) | u8::from(last);
        out
    }

    /// Decode a 7-byte field → `(address, is_last)`. `None` if fewer than 7 bytes.
    fn decode(bytes: &[u8]) -> Option<(Address, bool)> {
        let field: &[u8; 7] = bytes.get(..7)?.try_into().ok()?;
        let mut call = String::new();
        for &b in &field[..6] {
            let c = (b >> 1) & 0x7F;
            if c != b' ' {
                call.push(c as char);
            }
        }
        let ssid_octet = field[6];
        let addr = Address { call, ssid: (ssid_octet >> 1) & 0x0F, cbit: ssid_octet & 0x80 != 0 };
        Some((addr, ssid_octet & 1 != 0))
    }
}

/// An AX.25 UI frame carrying an APRS payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub dest: Address,
    pub source: Address,
    /// Digipeater path (≤ [`MAX_DIGIS`]).
    pub path: Vec<Address>,
    /// The APRS information field (raw bytes).
    pub info: Vec<u8>,
}

impl Frame {
    /// A UI frame ready for APRS (control = UI, PID = no-layer-3 are implied by the encoding).
    pub fn ui(dest: Address, source: Address, path: Vec<Address>, info: &[u8]) -> Frame {
        Frame { dest, source, path, info: info.to_vec() }
    }

    /// Encode to the AX.25 byte sequence WITHOUT HDLC flags/bit-stuffing, WITH the 2-byte FCS
    /// appended (low byte first). This is exactly what the HDLC/NRZI layer will stuff + frame.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.path.len() * 7 + self.info.len());
        out.extend_from_slice(&self.dest.encode(false));
        let path_empty = self.path.is_empty();
        out.extend_from_slice(&self.source.encode(path_empty));
        for (i, d) in self.path.iter().enumerate() {
            out.extend_from_slice(&d.encode(i + 1 == self.path.len()));
        }
        out.push(CONTROL_UI);
        out.push(PID_NO_L3);
        out.extend_from_slice(&self.info);
        let crc = fcs(&out);
        out.push((crc & 0xFF) as u8); // FCS low byte first (on-air order)
        out.push((crc >> 8) as u8);
        out
    }

    /// Decode an AX.25 UI frame from its unstuffed bytes (addresses..info + 2-byte FCS), verifying
    /// the FCS. `None` on a short buffer, malformed address field, non-UI/non-APRS control/PID, or
    /// FCS mismatch.
    pub fn decode(bytes: &[u8]) -> Option<Frame> {
        // Minimum: dest(7) + source(7) + control(1) + PID(1) + FCS(2), info may be empty.
        if bytes.len() < 18 {
            return None;
        }
        let (content, fcs_bytes) = bytes.split_at(bytes.len() - 2);
        let got = u16::from(fcs_bytes[0]) | (u16::from(fcs_bytes[1]) << 8);
        if fcs(content) != got {
            return None;
        }
        let mut addrs = Vec::new();
        let mut i = 0;
        loop {
            let (a, last) = Address::decode(content.get(i..i + 7)?)?;
            addrs.push(a);
            i += 7;
            if last {
                break;
            }
            if addrs.len() > 2 + MAX_DIGIS {
                return None; // runaway address field (extension bit never set)
            }
        }
        if addrs.len() < 2 {
            return None;
        }
        let control = *content.get(i)?;
        let pid = *content.get(i + 1)?;
        i += 2;
        if control != CONTROL_UI || pid != PID_NO_L3 {
            return None; // only UI/APRS frames for now
        }
        let info = content[i..].to_vec();
        let mut it = addrs.into_iter();
        let dest = it.next()?;
        let source = it.next()?;
        let path: Vec<Address> = it.collect();
        Some(Frame { dest, source, path, info })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fcs_matches_the_canonical_check_value() {
        // CRC-16/X.25 check vector — the standard "123456789" → 0x906E.
        assert_eq!(fcs(b"123456789"), 0x906E);
    }

    #[test]
    fn dest_address_encodes_to_the_known_shifted_bytes() {
        // "APRS" (space-padded to 6) with every byte << 1, then SSID octet 0x60 (C=0, RR=11,
        // SSID=0, not last). Hand-verified against the AX.25 address encoding.
        let bytes = Address::new("APRS", 0).encode(false);
        assert_eq!(bytes, [0x82, 0xA0, 0xA4, 0xA6, 0x40, 0x40, 0x60]);
    }

    #[test]
    fn source_address_last_sets_the_extension_bit() {
        let bytes = Address::new("N0CALL", 9).encode(true);
        // SSID octet: reserved 11, ssid 9 (<<1 = 0x12), extension 1 → 0x60 | 0x12 | 0x01 = 0x73.
        assert_eq!(bytes[6], 0x73);
    }

    #[test]
    fn address_parse_handles_ssid_and_used_marker() {
        assert_eq!(Address::parse("N0CALL-9"), Some(Address { call: "N0CALL".into(), ssid: 9, cbit: false }));
        assert_eq!(Address::parse("APRS"), Some(Address { call: "APRS".into(), ssid: 0, cbit: false }));
        assert_eq!(Address::parse("WIDE1-1*"), Some(Address { call: "WIDE1".into(), ssid: 1, cbit: true }));
        assert_eq!(Address::parse("TOOLONGCALL"), None);
        assert_eq!(Address::parse("N0CALL-16"), None);
        assert_eq!(Address::parse(""), None);
    }

    #[test]
    fn address_round_trips_through_encode_decode() {
        for (call, ssid, last) in [("APRS", 0u8, false), ("N0CALL", 9, true), ("W", 15, false)] {
            let a = Address::new(call, ssid);
            let (back, got_last) = Address::decode(&a.encode(last)).unwrap();
            assert_eq!(back, a);
            assert_eq!(got_last, last);
        }
    }

    #[test]
    fn frame_round_trips_with_a_digi_path() {
        let f = Frame::ui(
            Address::new("APRS", 0),
            Address::new("N0CALL", 9),
            vec![Address::new("WIDE1", 1), Address::new("WIDE2", 1)],
            b">Nexus APRS test",
        );
        let bytes = f.encode();
        let back = Frame::decode(&bytes).expect("valid frame decodes");
        assert_eq!(back, f);
    }

    #[test]
    fn frame_round_trips_with_empty_path_and_empty_info() {
        let f = Frame::ui(Address::new("APRS", 0), Address::new("N0CALL", 0), vec![], b"");
        let back = Frame::decode(&f.encode()).unwrap();
        assert_eq!(back, f);
        assert!(back.path.is_empty());
        assert!(back.info.is_empty());
    }

    #[test]
    fn a_single_bit_flip_fails_the_fcs() {
        let f = Frame::ui(Address::new("APRS", 0), Address::new("N0CALL", 7), vec![], b"payload");
        let mut bytes = f.encode();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0x01; // corrupt one bit in the info field
        assert!(Frame::decode(&bytes).is_none(), "corrupted frame must fail the FCS");
    }

    #[test]
    fn a_truncated_frame_is_rejected() {
        let f = Frame::ui(Address::new("APRS", 0), Address::new("N0CALL", 0), vec![], b"x");
        let bytes = f.encode();
        assert!(Frame::decode(&bytes[..bytes.len() - 1]).is_none());
        assert!(Frame::decode(&[]).is_none());
    }
}
