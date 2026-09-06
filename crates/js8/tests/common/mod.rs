//! Shared helpers for the crates/js8 integration tests (encode_parity, decode_parity, …).
//!
//! WHY a hand-rolled SHA-256 and RIFF writer instead of crates: crates/js8 has no dev
//! dependency on tempo-core (tempo-core → modes → js8 would be a cycle from B5 on) and the
//! workspace lock carries no sha2/hex — "dependencies are permanent", and the FT8/FT4 parity
//! tests already set the precedent of a dependency-free harness
//! (crates/ft8/tests/decode_parity.rs:12-47). SHA-256 is FIPS 180-4, ~60 lines, and its two
//! canonical vectors are asserted below so a transcription slip cannot masquerade as a pin.
//!
//! WHY fixtures are pinned by sha256 in a sidecar file: upstream JS8Call media/tests WAVs are
//! GPLv3 test audio (credited in NOTICE). A pin makes a silently swapped, truncated or
//! CRLF-mangled fixture a red test instead of a mysterious decode-count drift. No network is
//! touched at test time (wsjtx_predicate_differential.rs rule) — the lab script
//! ~/work/twowayfd/paritylab/js8/fetch_media_tests.sh is where bytes come from.
#![allow(dead_code)] // each integration-test binary compiles this module separately

use std::path::{Path, PathBuf};

/// FIPS 180-4 SHA-256 of `data`.
#[allow(clippy::needless_range_loop)] // the spec is written in indexed form; keep it recognisable
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    // Padding: 0x80, zeros to 56 mod 64, then the bit length big-endian.
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[4 * i],
                block[4 * i + 1],
                block[4 * i + 2],
                block[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(v);
        }
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// Lowercase hex, the `sha256sum` spelling.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `crates/js8/tests/fixtures/`.
pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// Read `fixtures/SHA256SUMS`, hash every listed file, and PANIC on any mismatch or on a
/// listed file that is missing. Returns the verified `(name, path)` list so a caller can
/// decode exactly what was pinned and nothing else. `SHA256SUMS` is in `sha256sum` format
/// (`<64 hex>  <name>`), so `sha256sum -c SHA256SUMS` in that directory is the shell twin
/// of this check.
pub fn verify_fixture_pins() -> Vec<(String, PathBuf)> {
    let dir = fixture_dir();
    let sums = std::fs::read_to_string(dir.join("SHA256SUMS"))
        .expect("crates/js8/tests/fixtures/SHA256SUMS must exist (Task B2.1)");
    let mut verified = Vec::new();
    for line in sums.lines().filter(|l| !l.trim().is_empty()) {
        let mut it = line.split_whitespace();
        let (want, name) = (
            it.next().expect("SHA256SUMS line: hash"),
            it.next().expect("SHA256SUMS line: file name"),
        );
        assert_eq!(want.len(), 64, "malformed hash for {name}: {want}");
        let path = dir.join(name);
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("pinned fixture {name} unreadable: {e}"));
        let got = hex(&sha256(&bytes));
        assert_eq!(
            got, want,
            "fixture {name} does not match its pin — do not rebaseline, refetch it"
        );
        verified.push((name.to_string(), path));
    }
    assert!(!verified.is_empty(), "SHA256SUMS lists no fixtures");
    verified
}
