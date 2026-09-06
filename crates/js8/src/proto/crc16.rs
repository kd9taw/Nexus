//! CRC-16/KERMIT and the 3-character base-41 checksum JS8Call appends to buffered commands.
//!
//! varicode.cpp:479-487 `checksum16` = CRC++ `CRC_16_KERMIT` (poly 0x1021 reflected = 0x8408,
//! init 0, reflected in/out, xorout 0; check value of "123456789" is 0x2189) over the text's
//! bytes, packed by `pack16bits` (:722-737) into three base-41 digits most-significant first.
//! The sender appends `" " + sum` (varicode.cpp:2166); the receiver takes the last 3 chars as
//! the sum and `left(len − 4)` as the body (mainwindow.cpp:8529-8532) and DISCARDS the whole
//! message on mismatch. The bit loop is the same reflected loop as
//! `crates/tempo-core/src/aprs/frame.rs::fcs` with init 0 and no xorout — re-derived here
//! because tempo-core depends on modes which depends on this crate.

use crate::proto::alphabet::CHECKSUM41;

/// CRC-16/KERMIT of `data`.
pub fn crc16_kermit(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc ^= u16::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0x8408
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// Three base-41 digits of the CRC-16/KERMIT of `text` (varicode.cpp `checksum16`).
pub fn checksum3(text: &str) -> String {
    let v = crc16_kermit(text.as_bytes()) as usize;
    let a = v / (41 * 41);
    let b = (v % (41 * 41)) / 41;
    let c = v % 41;
    [CHECKSUM41[a], CHECKSUM41[b], CHECKSUM41[c]]
        .iter()
        .map(|&d| d as char)
        .collect()
}

/// If `text_with_sum` ends in `" XXX"` where `XXX` is the checksum of everything before the
/// space, return that body; otherwise `None` (the message is discarded, as JS8Call does).
pub fn verify_checksum3(text_with_sum: &str) -> Option<&str> {
    let len = text_with_sum.len();
    if len < 4 || !text_with_sum.is_char_boundary(len - 4) {
        return None;
    }
    let (body, trailer) = text_with_sum.split_at(len - 4);
    let sum = trailer.strip_prefix(' ')?;
    (checksum3(body) == sum).then_some(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kermit_check_value() {
        // CRC-16/KERMIT (CRC++ `CRC_16_KERMIT`): poly 0x1021 reflected, init 0, no xorout.
        assert_eq!(crc16_kermit(b"123456789"), 0x2189);
        assert_eq!(crc16_kermit(b""), 0);
        // NOT the X.25 flavour used by AX.25 (init 0xFFFF, xorout 0xFFFF → 0x906E).
        assert_ne!(crc16_kermit(b"123456789"), 0x906E);
    }

    #[test]
    fn checksum3_is_three_base41_digits_most_significant_first() {
        // varicode.cpp:722-737 pack16bits: a = v/41², b = (v%41²)/41, c = v%41.
        // 0x2189 = 8585 = 5·1681 + 4·41 + 16 → "54G".
        assert_eq!(checksum3("123456789"), "54G");
        assert_eq!(checksum3("").len(), 3);
        for text in ["HELLO WORLD", "KD9TAW>W1AW>THIS IS A RELAY", "?"] {
            let sum = checksum3(text);
            assert_eq!(sum.len(), 3, "{text}");
            assert!(
                sum.bytes().all(|b| CHECKSUM41.contains(&b)),
                "{text}: {sum}"
            );
        }
    }

    #[test]
    fn verify_strips_a_valid_trailer_and_rejects_a_bad_one() {
        // varicode.cpp:2166 appends `" " + checksum16(line)`; mainwindow.cpp:8529-8532 takes
        // right(3) as the sum and left(len − 4) as the body.
        let body = "MSG HELLO THERE";
        let wire = format!("{body} {}", checksum3(body));
        assert_eq!(verify_checksum3(&wire), Some(body));
        assert_eq!(verify_checksum3(&format!("{body} 000")), None);
        assert_eq!(
            verify_checksum3(&format!("{body}{}", checksum3(body))),
            None,
            "the space before the sum is part of the layout"
        );
        assert_eq!(verify_checksum3("ABC"), None, "too short");
        assert_eq!(verify_checksum3(""), None);
        let empty_body = format!(" {}", checksum3(""));
        assert_eq!(verify_checksum3(&empty_body), Some(""));
    }

    #[test]
    fn verify_is_utf8_safe() {
        // The body may carry extended Latin-1 (JS8_ALLOW_EXTENDED); the trailer is ASCII.
        let body = "GRÜSSE AUS DL";
        let wire = format!("{body} {}", checksum3(body));
        assert_eq!(verify_checksum3(&wire), Some(body));
        assert_eq!(
            verify_checksum3("ÜÜ"),
            None,
            "shorter than the 4-byte trailer in chars but not bytes — must not panic"
        );
    }
}
