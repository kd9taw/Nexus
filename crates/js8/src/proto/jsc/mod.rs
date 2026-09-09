//! JS8Call's JSC dense text coder (jsc.cpp:31-171, read as the spec) over the 262,144-word
//! dictionary shipped as a generated blob (`dict.bin`, raw deflate; pin `dict.sha256`;
//! generator `scripts/gen-js8-jsc-dict.mjs`; provenance in NOTICE).
//!
//! CONTRACT. (s,c)-dense coding with b=4, s=7, c=9. A word index is written as zero or more
//! 4-bit "high" digits in `s..s+c` (value `(x % c) + s`, computed after `x -= 1`, most
//! significant first) followed by one 5-bit "final" digit `((index % s) << 1) | separator`.
//! `compress` splits the text on single spaces (empty parts between two spaces become the
//! word " "), and for each part repeatedly takes the LONGEST reachable dictionary word that is
//! a prefix, consuming `consume(idx)` bytes (two words consume less than their length upstream
//! — the blob carries the count). The separator flag on a word's last code word encodes the
//! space that followed it; the last part gets none. Everything ordinal is `index` — index i on
//! the air means word i — so the blob's order is the wire and is pinned by sha256.
//!
//! WHY LONGEST-PREFIX EQUALS UPSTREAM'S SCAN: `JSC::lookup` walks a 103-entry first-byte
//! prefix index into overlapping list ranges and returns the FIRST match. The generator
//! proves (P3, P4) that every reachable word's proper prefixes sit after it in its group, so
//! first-match == longest-match, and it marks the one unreachable entry ("@ALLCALL" at list
//! position 256644) so we never return an index upstream cannot produce. The Rust side keeps
//! a sorted index of REACHABLE words and binary-searches up to `max_word_len` prefixes — no
//! hash map, ~1.3 MB resident, built once behind a `OnceLock`.
//!
//! Latin-1: words are bytes; `decompress` maps each byte to the char with that code point,
//! `compress` maps chars > U+00FF to '?' (the operator alphabet is ASCII; JS8Call's editor
//! forces uppercase — `compose` does the same before calling here).
use std::io::Read;
use std::sync::OnceLock;

pub const B: u32 = 4;
pub const S: u32 = 7;
pub const C: u32 = 9;
pub const SIZE: usize = 262_144;

const BLOB: &[u8] = include_bytes!("dict.bin");

/// The inflated dictionary. `starts` has SIZE+1 entries (byte offsets into `words`).
pub struct Dict {
    words: Vec<u8>,
    starts: Vec<u32>,
    consume: Vec<u8>,
    /// Reachable word indices sorted by word bytes — the lookup index.
    sorted: Vec<u32>,
    max_len: usize,
}

/// Inflate `dict.bin` to the canonical bytes (`u8 len · bytes · u8 consume · u8 reachable`
/// × SIZE). Public so the pin test can hash exactly what `dict()` parses.
pub fn canonical_bytes() -> Vec<u8> {
    let mut out = Vec::with_capacity(BLOB.len() * 5);
    flate2::read::DeflateDecoder::new(BLOB)
        .read_to_end(&mut out)
        .expect("dict.bin is a raw-deflate stream written by scripts/gen-js8-jsc-dict.mjs");
    out
}

impl Dict {
    fn parse(canonical: &[u8]) -> Dict {
        let mut words = Vec::with_capacity(canonical.len());
        let mut starts = Vec::with_capacity(SIZE + 1);
        let mut consume = Vec::with_capacity(SIZE);
        let mut reachable = Vec::with_capacity(SIZE);
        let mut i = 0;
        let mut max_len = 0;
        for _ in 0..SIZE {
            let n = canonical[i] as usize;
            let w = &canonical[i + 1..i + 1 + n];
            starts.push(words.len() as u32);
            words.extend_from_slice(w);
            consume.push(canonical[i + 1 + n]);
            reachable.push(canonical[i + 2 + n] == 1);
            max_len = max_len.max(n);
            i += 3 + n;
        }
        // A corrupt or truncated blob must be loud, never a quietly wrong dictionary.
        assert_eq!(
            i,
            canonical.len(),
            "dict.bin: {} trailing bytes after {SIZE} entries",
            canonical.len() - i
        );
        starts.push(words.len() as u32);
        let mut d = Dict {
            words,
            starts,
            consume,
            sorted: Vec::new(),
            max_len,
        };
        let mut sorted: Vec<u32> = (0..SIZE as u32)
            .filter(|&i| reachable[i as usize])
            .collect();
        sorted.sort_by(|&a, &b| d.word(a).cmp(d.word(b)));
        d.sorted = sorted;
        d
    }

    pub fn len(&self) -> usize {
        SIZE
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn word(&self, idx: u32) -> &[u8] {
        let i = idx as usize;
        &self.words[self.starts[i] as usize..self.starts[i + 1] as usize]
    }

    /// Bytes `compress` consumes for this word (== word length except for two upstream quirks).
    pub fn consume(&self, idx: u32) -> usize {
        self.consume[idx as usize] as usize
    }

    pub fn max_word_len(&self) -> usize {
        self.max_len
    }

    /// The longest reachable word that is a prefix of `bytes` (upstream `JSC::lookup`).
    pub fn lookup(&self, bytes: &[u8]) -> Option<u32> {
        let top = bytes.len().min(self.max_len);
        for l in (1..=top).rev() {
            let key = &bytes[..l];
            if let Ok(pos) = self.sorted.binary_search_by(|&i| self.word(i).cmp(key)) {
                return Some(self.sorted[pos]);
            }
        }
        None
    }
}

/// The process-wide dictionary, inflated on first use (~30 ms, ~1.3 MB).
pub fn dict() -> &'static Dict {
    static DICT: OnceLock<Dict> = OnceLock::new();
    DICT.get_or_init(|| Dict::parse(&canonical_bytes()))
}

fn push_value(out: &mut Vec<u8>, value: u32, nbits: u32) {
    for k in (0..nbits).rev() {
        out.push(((value >> k) & 1) as u8);
    }
}

/// The code word for `index` (jsc.cpp `JSC::codeword`): high digits then the final digit.
pub(crate) fn codeword(index: u32, separate: bool) -> Vec<u8> {
    let mut high: Vec<u32> = Vec::new();
    let mut x = index / S;
    while x > 0 {
        x -= 1;
        high.push((x % C) + S);
        x /= C;
    }
    let mut out = Vec::with_capacity(high.len() * 4 + 5);
    for &d in high.iter().rev() {
        push_value(&mut out, d, B);
    }
    push_value(&mut out, ((index % S) << 1) | separate as u32, B + 1);
    out
}

fn latin1_bytes(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| {
            if (c as u32) <= 0xFF {
                c as u32 as u8
            } else {
                b'?'
            }
        })
        .collect()
}

/// Compress as many leading chars of `text` as fit STRICTLY under `max_bits` (upstream
/// `packCompressedMessage`: `frameBits + bits < frameSize`). Returns (bits, chars consumed).
pub fn compress(text: &str, max_bits: usize) -> (Vec<u8>, usize) {
    let d = dict();
    let bytes = latin1_bytes(text);
    let parts: Vec<&[u8]> = bytes.split(|&b| b == b' ').collect();
    let mut out: Vec<u8> = Vec::new();
    let mut chars = 0usize;
    for (i, part) in parts.iter().enumerate() {
        let last_part = i + 1 == parts.len();
        let space_char = part.is_empty() && !last_part;
        let mut w: &[u8] = if space_char { b" " } else { part };
        while !w.is_empty() {
            let Some(idx) = d.lookup(w) else { break }; // no match: upstream drops the rest of the word
            w = &w[d.consume(idx).min(w.len())..];
            let sep = w.is_empty() && !space_char && !last_part;
            let cw = codeword(idx, sep);
            if out.len() + cw.len() >= max_bits {
                return (out, chars);
            }
            out.extend_from_slice(&cw);
            chars += d.consume(idx) + sep as usize;
        }
    }
    (out, chars)
}

/// Split bits into 4-bit digits, absorbing the separator bit after each final digit
/// (jsc.cpp `JSC::decompress`, first loop). Returns (digits, positions carrying a separator).
pub(crate) fn digits(bits: &[u8]) -> (Vec<u32>, Vec<usize>) {
    let mut bytes = Vec::new();
    let mut seps = Vec::new();
    let mut i = 0;
    while i + 4 <= bits.len() {
        let v = bits[i..i + 4]
            .iter()
            .fold(0u32, |a, &b| (a << 1) | b as u32);
        bytes.push(v);
        i += 4;
        if v < S {
            if i < bits.len() && bits[i] == 1 {
                seps.push(bytes.len() - 1);
            }
            i += 1;
        }
    }
    (bytes, seps)
}

/// Decode one code word starting at `start`: returns (index, position of its final digit).
pub(crate) fn index_of(bytes: &[u32], start: usize) -> Option<(u32, usize)> {
    let base: [u32; 8] = {
        let mut b = [0u32; 8];
        b[1] = S;
        for k in 2..8 {
            b[k] = b[k - 1] + S * C.pow(k as u32 - 1);
        }
        b
    };
    let mut k = 0usize;
    let mut j: u64 = 0;
    while start + k < bytes.len() && bytes[start + k] >= S {
        j = j * C as u64 + (bytes[start + k] - S) as u64;
        k += 1;
        if j >= SIZE as u64 || k >= 8 {
            return None;
        }
    }
    if start + k >= bytes.len() {
        return None;
    }
    let j = j * S as u64 + bytes[start + k] as u64 + base[k] as u64;
    if j >= SIZE as u64 {
        return None;
    }
    Some((j as u32, start + k))
}

/// Decode bits back to text (Latin-1 bytes → chars). Stops at the first undecodable run.
pub fn decompress(bits: &[u8]) -> String {
    let d = dict();
    let (bytes, seps) = digits(bits);
    let mut out: Vec<u8> = Vec::new();
    let mut seps = seps.into_iter().peekable();
    let mut start = 0;
    while start < bytes.len() {
        let Some((idx, end)) = index_of(&bytes, start) else {
            break;
        };
        out.extend_from_slice(d.word(idx));
        if seps.peek() == Some(&end) {
            out.push(b' ');
            seps.next();
        }
        start = end + 1;
    }
    out.into_iter().map(|b| b as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(s: &str) -> Vec<u8> {
        s.bytes().map(|b| b - b'0').collect()
    }

    #[test]
    fn dictionary_inflates_to_262144_entries_with_the_two_consume_quirks() {
        let d = dict();
        assert_eq!(d.len(), SIZE);
        assert_eq!(d.word(81), b"@ALLCALL");
        assert_eq!(d.consume(81), 7, "map[81] consumes 7 of 8 bytes upstream");
        assert_eq!(d.word(262_143), b"ROSIDS");
        assert_eq!(d.consume(262_143), 1);
        assert_eq!(d.word(67), b" ");
        assert!(d.max_word_len() >= 4 && d.max_word_len() <= 64);
    }

    #[test]
    fn lookup_is_longest_reachable_prefix_and_never_returns_the_unreachable_allcall() {
        let d = dict();
        assert_eq!(d.lookup(b" "), Some(67));
        assert_eq!(d.lookup(b"ZZZZ"), Some(91_474));
        assert_eq!(
            d.lookup(b"ZZZZY"),
            Some(91_474),
            "longest prefix wins over ZZZ"
        );
        assert_eq!(d.lookup(b"ZZZ"), Some(70_901));
        assert_ne!(
            d.lookup(b"@ALLCALL"),
            Some(81),
            "list position 256644 is unreachable upstream"
        );
        assert_eq!(d.lookup(b""), None);
    }

    #[test]
    fn codeword_layout_is_sc_dense_b4_s7_c9() {
        // index < s: one 5-bit digit  [(index%s)<<1 | sep]
        assert_eq!(codeword(3, false), bits("00110"));
        assert_eq!(codeword(3, true), bits("00111"));
        // index 7 = (x=1 → x-1=0 → digit 0+7) then (7%7=0)
        assert_eq!(
            codeword(7, false),
            bits("0111")
                .into_iter()
                .chain(bits("00000"))
                .collect::<Vec<_>>()
        );
        // decode(encode) identity over a spread of indices
        for idx in [0u32, 6, 7, 62, 63, 69, 70, 1_000, 91_474, 262_143] {
            let cw = codeword(idx, false);
            let (bytes, seps) = digits(&cw);
            assert!(seps.is_empty());
            assert_eq!(index_of(&bytes, 0).map(|(i, _)| i), Some(idx), "idx {idx}");
        }
    }

    #[test]
    fn compress_then_decompress_is_identity_and_honours_the_bit_budget() {
        for text in [
            "HELLO WORLD",
            "CQ CQ CQ",
            "A  B",
            "TNX 73 GL",
            "MSG FRIDAY CONTACT.",
            "SIGNAL IS FADING FOR ",
        ] {
            let (b, chars) = compress(text, 10_000);
            assert_eq!(chars, text.len(), "{text:?} fully consumed");
            assert_eq!(decompress(&b), text, "{text:?}");
        }
        // Budget: strictly under. 72 bits of "HELLO WORLD…" stops before the code word that
        // would reach 72, and what it took decompresses to a prefix of the text.
        let long = "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG";
        let (b, chars) = compress(long, 72);
        assert!(b.len() < 72 && chars < long.len());
        assert!(long.starts_with(&decompress(&b)));
        let (b2, chars2) = compress(long, 10_000);
        assert!(b2.len() > b.len() && chars2 == long.len());
    }

    #[test]
    fn a_trailing_space_is_the_separator_flag_not_a_word() {
        // "A B" → A(sep) B. "A " splits to ["A", ""]: "A" is not the last part, so its code
        // word carries the separator flag and the empty last part emits nothing — the
        // trailing space survives as a FLAG, not as the word " " (jsc.cpp:60-85).
        let (ab, _) = compress("A B", 100);
        let (a_sp, n) = compress("A ", 100);
        assert_eq!(n, 2, "A plus its separator space");
        assert_eq!(decompress(&a_sp), "A ");
        assert_eq!(decompress(&ab), "A B");
        // "A" alone: last part, no flag.
        let (a, n1) = compress("A", 100);
        assert_eq!(n1, 1);
        assert_eq!(decompress(&a), "A");
    }

    #[test]
    fn decompress_ignores_a_dangling_partial_digit() {
        let (mut b, _) = compress("HELLO", 100);
        let whole = decompress(&b);
        b.push(1);
        b.push(0);
        assert_eq!(decompress(&b), whole);
        assert_eq!(decompress(&[]), "");
    }
}
