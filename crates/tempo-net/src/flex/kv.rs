//! Present-only, fail-closed field readers for the status decoders: a field is read only when
//! the line reported it, and a malformed value is dropped, never read as `0`.
//!
//! Every reader returns `Option`: `None` when the key is absent, repeated (ambiguous, see
//! [`super::wire::Kvs::get`]) or malformed. A decoder writes a delta field only from `Some`, so a
//! garbled `RF_frequency=` can never retune a slice to 0 Hz and a garbled `rfpower=` can never
//! read as zero power.
//!
//! PORTED from AetherSDR (https://github.com/aethersdr/AetherSDR, GPL-3.0; the upstream file
//! carries no per-file header, the licence is the repository's), `src/core/backends/flex/FlexKvCarry.h`
//! at commit `32fa50e4896a846a6970fa3f443bd49d667c139d` (2026-10-03), translated from C++/Qt to
//! Rust. Deliberate differences: readers return a value instead of writing through a reference;
//! a flag is `0` or `1` and anything else is dropped (upstream reads anything but `1` as false);
//! a real number must be finite; an id must carry its `0x` prefix. Recorded in the repo-root
//! NOTICE (AetherSDR entry).

use super::wire::{parse_id, Kvs};

/// The value, verbatim.
pub fn text(kvs: &Kvs, key: &str) -> Option<String> {
    kvs.get(key).map(str::to_string)
}

/// A decimal integer (an optional sign, then digits).
pub fn int(kvs: &Kvs, key: &str) -> Option<i32> {
    kvs.get(key).and_then(parse_int)
}

/// A finite decimal number.
pub fn real(kvs: &Kvs, key: &str) -> Option<f64> {
    kvs.get(key).and_then(parse_real)
}

/// `1` is true and `0` is false; anything else is not a reading.
pub fn flag(kvs: &Kvs, key: &str) -> Option<bool> {
    kvs.get(key).and_then(parse_flag)
}

/// [`int`], clamped into `lo..=hi` (the transmit ranges).
pub fn clamped(kvs: &Kvs, key: &str, lo: i32, hi: i32) -> Option<i32> {
    int(kvs, key).map(|v| v.clamp(lo, hi))
}

/// An object id or client handle (`0x` and one to eight hex digits).
pub fn id(kvs: &Kvs, key: &str) -> Option<u32> {
    kvs.get(key).and_then(parse_id)
}

/// The value split on commas, each part trimmed, empty parts dropped.
pub fn split_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// [`split_list`] with repeats removed, first appearance kept (upstream `uniqueCommaList`).
pub fn unique_list(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in split_list(raw) {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

pub(super) fn parse_int(text: &str) -> Option<i32> {
    // Rust's parser takes an optional sign and digits, nothing else: no spaces, no hex.
    text.parse::<i32>().ok()
}

pub(super) fn parse_real(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

pub(super) fn parse_flag(text: &str) -> Option<bool> {
    match text {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_reads_only_a_reported_well_formed_value() {
        let kvs = Kvs::parse(
            "f=14.074 n=-5 big=99999999999 inf=inf nan=NaN one=1 zero=0 yes=on hex=0x40000000 \
             bare dup=1 dup=2 empty=",
        );
        assert_eq!(real(&kvs, "f"), Some(14.074));
        assert_eq!(int(&kvs, "n"), Some(-5));
        assert_eq!(int(&kvs, "big"), None, "out of range is malformed");
        assert_eq!(real(&kvs, "inf"), None, "not finite");
        assert_eq!(real(&kvs, "nan"), None, "not finite");
        assert_eq!(flag(&kvs, "one"), Some(true));
        assert_eq!(flag(&kvs, "zero"), Some(false));
        assert_eq!(flag(&kvs, "yes"), None, "only 0 and 1 are flags");
        assert_eq!(id(&kvs, "hex"), Some(0x4000_0000));
        assert_eq!(id(&kvs, "n"), None);
        assert_eq!(text(&kvs, "bare"), None, "a bare word has no value");
        assert_eq!(int(&kvs, "dup"), None, "a repeated key is ambiguous");
        assert_eq!(text(&kvs, "empty"), Some(String::new()));
        assert_eq!(int(&kvs, "empty"), None);
        assert_eq!(int(&kvs, "absent"), None);
        assert_eq!(clamped(&kvs, "n", 0, 100), Some(0));
    }

    #[test]
    fn lists_split_on_commas_and_trim() {
        assert_eq!(split_list("ANT1, RX_A ,,RX_B "), ["ANT1", "RX_A", "RX_B"]);
        assert!(split_list("").is_empty());
        assert_eq!(
            unique_list("USB,LSB,DSTR,DSTR, DSTR,CW"),
            ["USB", "LSB", "DSTR", "CW"]
        );
    }
}
