//! JS8Call's legacy Huffman text code (varicode.cpp:158-204, transcribed as a table — NOTICE).
//!
//! CONTRACT. `decode70` turns a 0/1 bit slice into text by repeatedly taking the unique code
//! word that is a prefix of the remaining bits (the table is prefix-free, asserted in tests)
//! and stops at the first run that is no complete code word — that is how the padding of a
//! `[1][0][…]` Normal-speed data frame falls off. `encode` is the inverse for text made only
//! of table characters; `pack` is the compose-facing form that takes as many leading chars as
//! fit STRICTLY under a bit budget (JS8Call `packHuffMessage`: `frameBits + charBits <
//! frameSize`, varicode.cpp:1810).
//!
//! WHY IT STILL EXISTS. JS8Call deprecated this code in 2.2 but Normal speed still EMITS the
//! `[1][compressed?][70]` data frame and picks Huffman over JSC whenever Huffman packs more
//! characters (varicode.cpp:1855-1870). A receiver that dropped this table would show every
//! such frame as garbage; a sender that never chose it would still interoperate, but "match the
//! deprecated form" is the parity ruling (judge.md risks). Case: the table is UPPERCASE only;
//! upstream validates `toUpper()` but encodes case-sensitively, so a lowercase letter passes
//! validation and is then silently dropped. `compose` uppercases before it gets here, and
//! `encode`/`pack` return `None` on any char outside the table so the bug cannot be
//! reproduced by accident.

/// The 44-entry code, in upstream order (char, code bits as text).
pub const TABLE: [(char, &str); 44] = [
    (' ', "01"),
    ('E', "100"),
    ('T', "1101"),
    ('A', "0011"),
    ('O', "11111"),
    ('I', "11100"),
    ('N', "10111"),
    ('S', "10100"),
    ('H', "00011"),
    ('R', "00000"),
    ('D', "111011"),
    ('L', "110011"),
    ('C', "110001"),
    ('U', "101101"),
    ('M', "101011"),
    ('W', "001011"),
    ('F', "001001"),
    ('G', "000101"),
    ('Y', "000011"),
    ('P', "1111011"),
    ('B', "1111001"),
    ('.', "1110100"),
    ('V', "1100101"),
    ('K', "1100100"),
    ('-', "1100001"),
    ('+', "1100000"),
    ('?', "1011001"),
    ('!', "1011000"),
    ('"', "1010101"),
    ('X', "1010100"),
    ('0', "0010101"),
    ('J', "0010100"),
    ('1', "0010001"),
    ('Q', "0010000"),
    ('2', "0001001"),
    ('Z', "0001000"),
    ('3', "0000101"),
    ('5', "0000100"),
    ('4', "11110101"),
    ('9', "11110100"),
    ('8', "11110001"),
    ('6', "11110000"),
    ('7', "11101011"),
    ('/', "11101010"),
];

fn code_of(c: char) -> Option<&'static str> {
    TABLE.iter().find(|(ch, _)| *ch == c).map(|(_, code)| *code)
}

fn push_code(out: &mut Vec<u8>, code: &str) {
    out.extend(code.bytes().map(|b| b - b'0'));
}

/// Decode as many whole code words as the bits hold. `None` when nothing decodes.
pub fn decode70(bits: &[u8]) -> Option<String> {
    let mut text = String::new();
    let mut pos = 0;
    'outer: while pos < bits.len() {
        for (ch, code) in TABLE.iter() {
            let n = code.len();
            if pos + n <= bits.len()
                && code
                    .bytes()
                    .zip(&bits[pos..pos + n])
                    .all(|(c, &b)| c - b'0' == b)
            {
                text.push(*ch);
                pos += n;
                continue 'outer;
            }
        }
        break; // no code word is a prefix of what is left: padding
    }
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Encode the whole text. `None` if any char is outside the table.
pub fn encode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 6);
    for c in text.chars() {
        push_code(&mut out, code_of(c)?);
    }
    Some(out)
}

/// Encode the longest prefix of `text` whose bits stay STRICTLY under `max_bits`; returns the
/// bits and the number of chars taken. `None` if ANY char of `text` (not just the packed
/// prefix) is outside the table — upstream validates the whole remaining line first.
pub fn pack(text: &str, max_bits: usize) -> Option<(Vec<u8>, usize)> {
    if text.chars().any(|c| code_of(c).is_none()) {
        return None;
    }
    let mut out = Vec::new();
    let mut taken = 0;
    for c in text.chars() {
        let code = code_of(c)?;
        if out.len() + code.len() >= max_bits {
            break;
        }
        push_code(&mut out, code);
        taken += 1;
    }
    Some((out, taken))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(s: &str) -> Vec<u8> {
        s.bytes().map(|b| b - b'0').collect()
    }

    #[test]
    fn table_is_the_44_entry_prefix_free_code() {
        assert_eq!(TABLE.len(), 44);
        for (i, (_, a)) in TABLE.iter().enumerate() {
            for (j, (_, b)) in TABLE.iter().enumerate() {
                if i != j {
                    assert!(!b.starts_with(a), "{a} is a prefix of {b}: not prefix-free");
                }
            }
        }
        assert_eq!(TABLE[0], (' ', "01"));
        assert_eq!(TABLE[1], ('E', "100"));
        assert_eq!(TABLE[43], ('/', "11101010"));
    }

    #[test]
    fn decode_reads_a_prefix_code_and_stops_at_the_first_unknown_run() {
        // "HI " = 00011 11100 01
        assert_eq!(decode70(&bits("000111110001")), Some("HI ".to_string()));
        // trailing junk that is no complete code word is ignored
        assert_eq!(decode70(&bits("0001111100011")), Some("HI ".to_string()));
        assert_eq!(decode70(&bits("")), None);
        assert_eq!(decode70(&bits("1")), None);
    }

    #[test]
    fn encode_round_trips_the_whole_table_alphabet() {
        let alphabet: String = TABLE.iter().map(|(c, _)| *c).collect();
        let enc = encode(&alphabet).expect("every table char encodes");
        assert_eq!(decode70(&enc).as_deref(), Some(alphabet.as_str()));
    }

    #[test]
    fn encode_refuses_a_char_outside_the_table() {
        assert_eq!(
            encode("HELLO WORLD"),
            Some(bits(
                "0001110011001111001111111010010111111100000110011111011"
            ))
        );
        assert_eq!(
            encode("HELLO, WORLD"),
            None,
            "',' is not in the legacy table"
        );
        assert_eq!(
            encode("hello"),
            None,
            "lowercase is not in the table (compose uppercases first)"
        );
    }

    #[test]
    fn pack_stops_strictly_below_the_bit_budget_and_counts_chars() {
        // "HELLO WORLD" is 55 bits (per varicode.cpp:158-204); a 72-bit budget takes it whole.
        assert_eq!(
            pack("HELLO WORLD", 72),
            Some((
                bits("0001110011001111001111111010010111111100000110011111011"),
                11
            ))
        );
        // A 10-bit budget takes H (5) + E (3) = 8 bits — L (6 more) would reach 14 ≥ 10.
        assert_eq!(pack("HELLO", 10), Some((bits("00011100"), 2)));
        // JS8Call's rule is `len + next < budget` (strict): exactly-full is refused.
        assert_eq!(pack("HE", 8), Some((bits("00011"), 1)));
        assert_eq!(pack("HE", 9), Some((bits("00011100"), 2)));
        assert_eq!(pack("A,B", 72), None);
    }
}
