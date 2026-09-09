//! The JS8 alphabets and the small fixed tables — transcribed FACTS, order IS the wire.
//!
//! * [`SIXBIT`] — the 64-character alphabet of the twelve 6-bit payload chars
//!   (JS8.cpp:849: `0-9 A-Z a-z - +`; the Fortran `alphabet72` (varicode.cpp:37) appends
//!   `/?.` but only indices 0..=63 are representable on the air).
//! * [`CHECKSUM41`] — base-41 digits of the buffered-command checksum (varicode.cpp:36).
//! * [`ALNUM39`] — the callsign/grid alphabet used by pack28 / pack50 (varicode.cpp:44).
//! * [`GROUPS`] — the 51 group destinations at `nbasecall + 4 ..= + 54` (varicode.cpp:214-281).
//! * [`CQS`] / [`HBS`] — the 3-bit CQ / HB index tables of a heartbeat frame
//!   (varicode.cpp:283-306; every HB flag renders "HB" since 2.2).

/// Twelve of these make the 72-bit payload; index = the 6-bit value.
pub const SIXBIT: [u8; 64] = *b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-+";

/// Base-41 digit alphabet of `checksum3` (varicode.cpp `alphabet`, nalphabet = 41).
pub const CHECKSUM41: [u8; 41] = *b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ+-./?";

/// Callsign / grid alphabet (varicode.cpp `alphanumeric`): index 36 = space, 37 = '/', 38 = '@'.
pub const ALNUM39: [u8; 39] = *b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ /@";

/// Group destinations, `nbasecall + 4 + index` on the wire (varicode.cpp:214-281).
pub const GROUPS: [&str; 51] = [
    "@DX/NA",
    "@DX/SA",
    "@DX/EU",
    "@DX/AS",
    "@DX/AF",
    "@DX/OC",
    "@DX/AN", // continental dx
    "@REGION/1",
    "@REGION/2",
    "@REGION/3", // itu regions
    "@GROUP/0",
    "@GROUP/1",
    "@GROUP/2",
    "@GROUP/3",
    "@GROUP/4",
    "@GROUP/5",
    "@GROUP/6",
    "@GROUP/7",
    "@GROUP/8",
    "@GROUP/9",
    "@COMMAND",
    "@CONTROL",
    "@NET",
    "@NTS", // ops
    "@RESERVE/0",
    "@RESERVE/1",
    "@RESERVE/2",
    "@RESERVE/3",
    "@RESERVE/4", // reserved
    "@APRSIS",
    "@RAGCHEW",
    "@JS8",
    "@EMCOMM",
    "@ARES",
    "@MARS",
    "@AMRRON",
    "@RACES",
    "@RAYNET",
    "@RADAR",
    "@SKYWARN",
    "@CQ",
    "@HB",
    "@QSO",
    "@QSOPARTY",
    "@CONTEST",
    "@FIELDDAY",
    "@SOTA",
    "@IOTA",
    "@POTA",
    "@QRP",
    "@QRO",
];

/// CQ variants selected by the 3-bit index of a heartbeat frame with the isAlt bit set.
pub const CQS: [&str; 8] = [
    "CQ CQ CQ",
    "CQ DX",
    "CQ QRP",
    "CQ CONTEST",
    "CQ FIELD",
    "CQ FD",
    "CQ CQ",
    "CQ",
];

/// HB flags (deprecated since 2.2; every index renders "HB").
pub const HBS: [&str; 8] = ["HB"; 8];

/// The 12 six-bit values as their characters (ALL.TXT's 12-char column). Values are masked to 6 bits.
pub fn sixbit_to_string(chars: &[u8; 12]) -> String {
    chars
        .iter()
        .map(|&v| SIXBIT[(v & 63) as usize] as char)
        .collect()
}

/// Inverse of [`sixbit_to_string`]: exactly 12 characters, each in [`SIXBIT`].
pub fn sixbit_from_str(s: &str) -> Option<[u8; 12]> {
    let b = s.as_bytes();
    if b.len() != 12 {
        return None;
    }
    let mut out = [0u8; 12];
    for (o, &c) in out.iter_mut().zip(b) {
        *o = SIXBIT.iter().position(|&a| a == c)? as u8;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixbit_alphabet_matches_js8_cpp_static_asserts() {
        // JS8.cpp:849-892: 64 chars, '0'→0, 'A'→10, 'a'→36, '-'→62, '+'→63.
        assert_eq!(SIXBIT.len(), 64);
        assert_eq!(SIXBIT[0], b'0');
        assert_eq!(SIXBIT[10], b'A');
        assert_eq!(SIXBIT[36], b'a');
        assert_eq!(SIXBIT[62], b'-');
        assert_eq!(SIXBIT[63], b'+');
        let mut sorted = SIXBIT.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 64, "no duplicate characters");
    }

    #[test]
    fn checksum_and_alnum_alphabets_match_varicode_cpp() {
        // varicode.cpp:36 (nalphabet 41) and :44.
        assert_eq!(
            &CHECKSUM41[..],
            b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ+-./?"
        );
        assert_eq!(&ALNUM39[..], b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ /@");
        assert_eq!(ALNUM39[36], b' ');
        assert_eq!(ALNUM39[37], b'/');
        assert_eq!(ALNUM39[38], b'@');
    }

    #[test]
    fn group_table_order_is_the_wire() {
        // varicode.cpp:214-281: nbasecall + 4 .. + 54 in this exact order.
        assert_eq!(GROUPS.len(), 51);
        assert_eq!(GROUPS[0], "@DX/NA");
        assert_eq!(GROUPS[6], "@DX/AN");
        assert_eq!(GROUPS[7], "@REGION/1");
        assert_eq!(GROUPS[10], "@GROUP/0");
        assert_eq!(GROUPS[20], "@COMMAND");
        assert_eq!(GROUPS[24], "@RESERVE/0");
        assert_eq!(GROUPS[29], "@APRSIS");
        assert_eq!(GROUPS[41], "@HB");
        assert_eq!(GROUPS[40], "@CQ");
        assert_eq!(GROUPS[50], "@QRO");
        assert!(GROUPS.iter().all(|g| g.starts_with('@')));
        let mut sorted = GROUPS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 51);
    }

    #[test]
    fn cq_and_hb_tables_match_varicode_cpp() {
        // varicode.cpp:283-306.
        assert_eq!(
            CQS,
            [
                "CQ CQ CQ",
                "CQ DX",
                "CQ QRP",
                "CQ CONTEST",
                "CQ FIELD",
                "CQ FD",
                "CQ CQ",
                "CQ"
            ]
        );
        assert_eq!(
            HBS, ["HB"; 8],
            "the HB status flags are deprecated since 2.2 — every index renders HB"
        );
    }

    #[test]
    fn sixbit_string_round_trips_and_rejects_bad_input() {
        let chars: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        assert_eq!(sixbit_to_string(&chars), "0123456789AB");
        assert_eq!(sixbit_from_str("0123456789AB"), Some(chars));
        assert_eq!(
            sixbit_from_str("zzzzzzzzzz-+"),
            Some([61, 61, 61, 61, 61, 61, 61, 61, 61, 61, 62, 63])
        );
        assert_eq!(sixbit_to_string(&[63; 12]), "++++++++++++");
        assert_eq!(sixbit_from_str("0123456789A"), None, "eleven chars");
        assert_eq!(sixbit_from_str("0123456789ABC"), None, "thirteen chars");
        assert_eq!(
            sixbit_from_str("0123456789A/"),
            None,
            "'/' is outside the 64-char alphabet"
        );
        assert_eq!(
            sixbit_from_str("0123456789A "),
            None,
            "space is not in it either"
        );
        assert_eq!(
            sixbit_to_string(&[0x40 | 1; 12]),
            "111111111111",
            "values are masked to 6 bits"
        );
    }
}
